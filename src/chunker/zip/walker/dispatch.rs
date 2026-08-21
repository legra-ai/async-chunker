//! Record dispatch: signatures, fixed parts, variable parts, and the
//! end-record reconciliation.

use super::super::fault::ZipFault;
use super::super::records::{
    CENTRAL_HEADER, CentralHeader, END_OF_CENTRAL_DIRECTORY, EndRecord, LOCAL_HEADER, LocalHeader,
    ZIP64_END_OF_CENTRAL_DIRECTORY, ZIP64_LOCATOR, ZIP64_MARKER_16, ZIP64_MARKER_32,
    Zip64EndRecord,
};
use super::core::Walker;
use super::state::{DescriptorShape, Phase, Record, State, Variable};
use crate::constants::GENERIC_CDC_CHUNK_MIN_BYTES;

impl Walker {
    pub(super) fn dispatch_signature(&mut self) -> Result<(), ZipFault> {
        let signature: [u8; 4] = [self.fixed[0], self.fixed[1], self.fixed[2], self.fixed[3]];
        let kind = match signature {
            LOCAL_HEADER => Record::Local,
            CENTRAL_HEADER => Record::Central,
            END_OF_CENTRAL_DIRECTORY => Record::End,
            ZIP64_END_OF_CENTRAL_DIRECTORY => Record::Zip64End,
            ZIP64_LOCATOR => Record::Zip64Locator,
            _ => return Err(ZipFault::UnknownSignature),
        };
        let record_start = self.offset.saturating_sub(3);
        match (kind, self.phase) {
            (Record::Local, Phase::Members) => {}
            (Record::Local, _) => return Err(ZipFault::MemberAfterCentralDirectory),
            (Record::Central, Phase::Members) => {
                self.phase = Phase::Central;
                self.central_start = Some(record_start);
            }
            (Record::Central, Phase::Central) => {}
            (Record::Central, _) => return Err(ZipFault::RecordOutOfSequence),
            (Record::Zip64End, Phase::Members | Phase::Central) => {
                self.begin_end_sequence(record_start);
                self.phase = Phase::Zip64EndSeen;
            }
            (Record::Zip64Locator, Phase::Zip64EndSeen) => self.phase = Phase::LocatorSeen,
            (Record::End, Phase::Members | Phase::Central) => self.begin_end_sequence(record_start),
            (Record::End, Phase::LocatorSeen) => {}
            (_, _) => return Err(ZipFault::RecordOutOfSequence),
        }
        self.state = State::Fixed { kind, len: 0 };
        Ok(())
    }

    /// The first end record marks where the central directory ended
    /// (and, for a memberless archive, where it began).
    fn begin_end_sequence(&mut self, record_start: u64) {
        if self.central_start.is_none() {
            self.central_start = Some(record_start);
        }
        self.central_end = record_start;
    }

    pub(super) fn dispatch_fixed(&mut self, kind: Record) -> Result<Option<usize>, ZipFault> {
        match kind {
            Record::Local => {
                let header = LocalHeader::parse(
                    self.fixed[..LocalHeader::FIXED_LEN]
                        .try_into()
                        .expect("sized"),
                );
                self.local_count += 1;
                self.begin_variable(
                    Variable::Local(header),
                    usize::from(header.name_len) + usize::from(header.extra_len),
                )
            }
            Record::Central => {
                let header = CentralHeader::parse(
                    self.fixed[..CentralHeader::FIXED_LEN]
                        .try_into()
                        .expect("sized"),
                );
                self.central_count += 1;
                self.begin_variable(
                    Variable::Central(header),
                    usize::from(header.name_len)
                        + usize::from(header.extra_len)
                        + usize::from(header.comment_len),
                )
            }
            Record::End => {
                let end = EndRecord::parse(
                    self.fixed[..EndRecord::FIXED_LEN]
                        .try_into()
                        .expect("sized"),
                );
                self.reconcile_end(end)?;
                self.skip(u64::from(end.comment_len), Phase::Complete);
                Ok(None)
            }
            Record::Zip64End => {
                let end = Zip64EndRecord::parse(
                    self.fixed[..Zip64EndRecord::FIXED_LEN]
                        .try_into()
                        .expect("sized"),
                )?;
                self.zip64_end = Some(end);
                self.skip(end.extensible_len, Phase::Zip64EndSeen);
                Ok(None)
            }
            Record::Zip64Locator => {
                self.state = State::Signature { len: 0 };
                Ok(None)
            }
        }
    }

    fn begin_variable(&mut self, kind: Variable, total: usize) -> Result<Option<usize>, ZipFault> {
        self.variable.clear();
        if total == 0 {
            self.state = State::Variable { kind, total };
            return self.dispatch_variable(kind);
        }
        self.state = State::Variable { kind, total };
        Ok(None)
    }

    fn skip(&mut self, remaining: u64, then: Phase) {
        if remaining == 0 {
            self.phase = then;
            self.state = State::Signature { len: 0 };
        } else {
            self.state = State::Skip { remaining, then };
        }
    }

    /// Returns the local header's length for a large member.
    pub(super) fn dispatch_variable(&mut self, kind: Variable) -> Result<Option<usize>, ZipFault> {
        match kind {
            Variable::Local(header) => {
                let extra = &self.variable[usize::from(header.name_len)..];
                let zip64 = header.needs_zip64();
                let sizes = header.sizes(extra)?;
                let header_len = LocalHeader::FIXED_LEN + 4 + self.variable.len();
                let large =
                    (sizes.compressed >= GENERIC_CDC_CHUNK_MIN_BYTES as u64).then_some(header_len);
                if header.has_descriptor {
                    if sizes.compressed == 0 {
                        self.state = State::DataScan {
                            consumed: 0,
                            method: header.method,
                            zip64,
                            pending: 0,
                        };
                    } else {
                        sizes.check(header.method)?;
                        self.state = State::Data {
                            remaining: sizes.compressed,
                            total: sizes.compressed,
                            method: header.method,
                            descriptor: Some(DescriptorShape { zip64 }),
                        };
                    }
                } else {
                    sizes.check(header.method)?;
                    self.state = if sizes.compressed == 0 {
                        State::Signature { len: 0 }
                    } else {
                        State::Data {
                            remaining: sizes.compressed,
                            total: sizes.compressed,
                            method: header.method,
                            descriptor: None,
                        }
                    };
                }
                Ok(large)
            }
            Variable::Central(header) => {
                let name_end = usize::from(header.name_len);
                let extra = &self.variable[name_end..name_end + usize::from(header.extra_len)];
                let (sizes, local_offset) = header.resolve(extra)?;
                sizes.check(header.method)?;
                let central_start = self.central_start.ok_or(ZipFault::RecordOutOfSequence)?;
                if local_offset >= central_start {
                    return Err(ZipFault::CentralOffsetOutOfRange);
                }
                self.state = State::Signature { len: 0 };
                Ok(None)
            }
        }
    }

    /// Reconcile the end record against the members and central
    /// entries that streamed past.
    fn reconcile_end(&self, end: EndRecord) -> Result<(), ZipFault> {
        let (entries, central_size, central_offset) = match self.zip64_end {
            Some(zip64) => (
                if end.entries_total == ZIP64_MARKER_16 {
                    zip64.entries_total
                } else {
                    u64::from(end.entries_total)
                },
                if end.central_size == ZIP64_MARKER_32 {
                    zip64.central_size
                } else {
                    u64::from(end.central_size)
                },
                if end.central_offset == ZIP64_MARKER_32 {
                    zip64.central_offset
                } else {
                    u64::from(end.central_offset)
                },
            ),
            None => (
                u64::from(end.entries_total),
                u64::from(end.central_size),
                u64::from(end.central_offset),
            ),
        };
        if entries != self.central_count || entries != self.local_count {
            return Err(ZipFault::EntryCountMismatch);
        }
        let central_start = self.central_start.ok_or(ZipFault::RecordOutOfSequence)?;
        if central_offset != central_start || central_size != self.central_end - central_start {
            return Err(ZipFault::CentralDirectoryGeometry);
        }
        Ok(())
    }
}
