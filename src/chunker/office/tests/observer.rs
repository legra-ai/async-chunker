//! The `ooxml-v1` event tap: names, canonical coordinates, and
//! member byte streams.

use super::super::{OoxmlChunker, PackageObserver};
use super::canonical::chunks_of;
use super::fixtures::{docx_parts, package};

#[derive(Default)]
struct Recorder {
    starts: Vec<(Vec<u8>, u64)>,
    byte_totals: Vec<u64>,
    ends: Vec<u64>,
    package_end: Option<u64>,
}

/// Shared recording target: the observer moves into the chunker.
struct Tap(std::sync::Arc<std::sync::Mutex<Recorder>>);

impl PackageObserver for Tap {
    fn member_start(&mut self, name: &[u8], canonical_offset: u64) {
        let mut recorder = self.0.lock().expect("no poison");
        recorder.starts.push((name.to_vec(), canonical_offset));
        recorder.byte_totals.push(0);
    }

    fn member_bytes(&mut self, bytes: &[u8]) {
        let mut recorder = self.0.lock().expect("no poison");
        let total = recorder.byte_totals.last_mut().expect("member started");
        *total += bytes.len() as u64;
    }

    fn member_end(&mut self, canonical_len: u64) {
        self.0.lock().expect("no poison").ends.push(canonical_len);
    }

    fn package_end(&mut self, member_count: u64) {
        self.0.lock().expect("no poison").package_end = Some(member_count);
    }
}

#[test]
fn the_tap_reports_names_lengths_and_monotonic_canonical_offsets() {
    let parts = docx_parts("els15/tap", "els15/tap-media");
    let input = package(&parts);
    let recorder = std::sync::Arc::new(std::sync::Mutex::new(Recorder::default()));
    let mut chunker = OoxmlChunker::new();
    chunker.set_observer(Box::new(Tap(recorder.clone())));
    let canonical: Vec<u8> = chunks_of(&mut chunker, &input, 8192)
        .expect("canonicalizes")
        .concat();

    let recorder = recorder.lock().expect("no poison");
    assert_eq!(recorder.starts.len(), parts.len());
    for (part, (name, _)) in parts.iter().zip(&recorder.starts) {
        assert_eq!(name.as_slice(), part.name.as_bytes());
    }
    let offsets: Vec<u64> = recorder.starts.iter().map(|(_, offset)| *offset).collect();
    assert!(offsets.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(*offsets.last().expect("members") < canonical.len() as u64);
    assert_eq!(recorder.byte_totals, recorder.ends);
    for (part, len) in parts.iter().zip(&recorder.ends) {
        assert_eq!(*len, part.bytes.len() as u64, "{}", part.name);
    }
    assert_eq!(recorder.package_end, Some(parts.len() as u64));

    // The reported coordinates address the canonical stream: each
    // member's canonical bytes appear at its offset + header size
    // (30 + name length).
    for (part, (name, offset)) in parts.iter().zip(&recorder.starts) {
        let data_start = usize::try_from(*offset).expect("fits") + 30 + name.len();
        let data_end = data_start + part.bytes.len();
        assert_eq!(
            &canonical[data_start..data_end],
            part.bytes.as_slice(),
            "{} canonical coordinates",
            part.name
        );
    }
}
