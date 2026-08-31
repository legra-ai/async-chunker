//! [`GzipWriter`] — deterministic gzip wrapping at a pinned
//! compression level.

use miniz_oxide::deflate::core::{CompressorOxide, create_comp_flags_from_zip_params};
use miniz_oxide::deflate::stream::deflate;
use miniz_oxide::{MZFlush, MZStatus};

/// The pinned compression level for canonical output.
const LEVEL: i32 = 6;
/// Output window per call.
const OUT_BYTES: usize = 16 << 10;

/// Streaming deterministic gzip writer: fixed header (no name, no
/// timestamp), pinned-level deflate, CRC32 + ISIZE trailer.
pub struct GzipWriter {
    compressor: Box<CompressorOxide>,
    crc: crc32fast::Hasher,
    isize_acc: u64,
    header_written: bool,
    finished: bool,
}

impl Default for GzipWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl GzipWriter {
    /// A fresh writer.
    #[must_use]
    pub fn new() -> Self {
        // Raw deflate (negative window bits), default strategy.
        let flags = create_comp_flags_from_zip_params(LEVEL, -15, 0);
        Self {
            compressor: Box::new(CompressorOxide::new(flags)),
            crc: crc32fast::Hasher::new(),
            isize_acc: 0,
            header_written: false,
            finished: false,
        }
    }

    fn header(&mut self, emit: &mut dyn FnMut(&[u8])) {
        if !self.header_written {
            // Magic, CM=deflate, no flags, zero MTIME, no XFL,
            // OS=unknown (0xFF): fully deterministic.
            emit(&[0x1F, 0x8B, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF]);
            self.header_written = true;
        }
    }

    /// Compress one window.
    ///
    /// # Panics
    ///
    /// Panics if called after [`Self::finish`], or on a compressor
    /// contract violation (which pinned parameters cannot produce).
    pub fn push(&mut self, bytes: &[u8], emit: &mut dyn FnMut(&[u8])) {
        assert!(!self.finished, "GzipWriter::push after finish");
        self.header(emit);
        self.crc.update(bytes);
        self.isize_acc += bytes.len() as u64;
        let mut input = bytes;
        let mut out = [0u8; OUT_BYTES];
        while !input.is_empty() {
            let result = deflate(&mut self.compressor, input, &mut out, MZFlush::None);
            result.status.expect("pinned deflate parameters are valid");
            input = &input[result.bytes_consumed..];
            if result.bytes_written > 0 {
                emit(&out[..result.bytes_written]);
            }
        }
    }

    /// Flush the stream and write the trailer.
    ///
    /// # Panics
    ///
    /// As [`Self::push`].
    pub fn finish(mut self, emit: &mut dyn FnMut(&[u8])) {
        self.header(emit);
        self.finished = true;
        let mut out = [0u8; OUT_BYTES];
        loop {
            let result = deflate(&mut self.compressor, &[], &mut out, MZFlush::Finish);
            let status = result.status.expect("pinned deflate parameters are valid");
            if result.bytes_written > 0 {
                emit(&out[..result.bytes_written]);
            }
            if status == MZStatus::StreamEnd {
                break;
            }
        }
        emit(&self.crc.clone().finalize().to_le_bytes());
        #[allow(clippy::cast_possible_truncation)]
        emit(&(self.isize_acc as u32).to_le_bytes());
    }
}
