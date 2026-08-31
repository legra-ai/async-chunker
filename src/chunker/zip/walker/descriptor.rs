//! Data descriptors: scanning unknown-size members for the
//! descriptor that closes them, and closing known-size members.

use super::super::fault::ZipFault;
use super::super::records::{DATA_DESCRIPTOR, MemberSizes};
use super::core::Walker;
use super::events::ZipEvents;
use super::state::{DescriptorShape, State};

impl Walker {
    /// Data-descriptor members of unknown size: watch for the
    /// descriptor signature and accept it only when its compressed
    /// size equals the bytes it covers. The lookahead is the
    /// descriptor's own length; bytes that fail the test are member
    /// data and re-enter the scan one by one.
    pub(super) fn scan(
        &mut self,
        byte: u8,
        mut consumed: u64,
        method: u16,
        zip64: bool,
        mut pending: usize,
        events: &mut dyn ZipEvents,
    ) -> Result<(), ZipFault> {
        let shape = DescriptorShape { zip64 };
        self.fixed[pending] = byte;
        pending += 1;
        loop {
            let head = &self.fixed[..pending];
            let looks_like_descriptor = head.len() < 4 || head[..4] == DATA_DESCRIPTOR;
            if looks_like_descriptor && head.len() >= 4 + shape.body_len() {
                let body = &head[4..4 + shape.body_len()];
                if shape.compressed(body) == consumed {
                    let sizes = MemberSizes {
                        compressed: consumed,
                        uncompressed: shape.uncompressed(body),
                    };
                    sizes.check(method)?;
                    events.member_end(sizes, shape.crc(body));
                    self.state = State::Signature { len: 0 };
                    return Ok(());
                }
            } else if looks_like_descriptor {
                break;
            }
            // The first pending byte is member data (a non-signature,
            // or a false signature inside the data).
            events.member_data(self.fixed[0]);
            self.fixed.copy_within(1..pending, 0);
            pending -= 1;
            consumed += 1;
            if pending == 0 {
                break;
            }
        }
        self.state = State::DataScan {
            consumed,
            method,
            zip64,
            pending,
        };
        Ok(())
    }

    /// Known-size member followed by a descriptor: the signature is
    /// optional, so decide the layout once four bytes are in.
    pub(super) fn try_close_descriptor(
        &mut self,
        shape: DescriptorShape,
        data_len: u64,
        method: u16,
        len: usize,
        events: &mut dyn ZipEvents,
    ) -> Result<(), ZipFault> {
        if len < 4 {
            return Ok(());
        }
        let signed = self.fixed[..4] == DATA_DESCRIPTOR;
        let total = if signed {
            4 + shape.body_len()
        } else {
            shape.body_len()
        };
        if len < total {
            return Ok(());
        }
        let body = if signed {
            &self.fixed[4..total]
        } else {
            &self.fixed[..total]
        };
        if shape.compressed(body) != data_len {
            return Err(ZipFault::DescriptorSizeMismatch);
        }
        let sizes = MemberSizes {
            compressed: data_len,
            uncompressed: shape.uncompressed(body),
        };
        sizes.check(method)?;
        events.member_end(sizes, shape.crc(body));
        self.state = State::Signature { len: 0 };
        Ok(())
    }
}
