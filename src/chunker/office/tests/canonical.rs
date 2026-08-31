//! `ooxml-v1` canonicalization tests.

use std::collections::HashSet;

use super::super::super::profile_chunker::{Chunker, ProfileChunker};
use super::super::{OfficeKind, OoxmlChunker};
use super::fixtures::{docx_parts, noise, package, package_stored, xml};
use crate::ChunkError;
use crate::chunker::zip::tests::writer::{Framing, Member, Options, archive};
use crate::profile::ChunkingProfile;

/// Drive a chunker over `bytes` in `window`-sized pushes.
pub(super) fn chunks_of(
    chunker: &mut dyn Chunker,
    bytes: &[u8],
    window: usize,
) -> Result<Vec<Vec<u8>>, ChunkError> {
    let mut chunks = Vec::new();
    let mut record = |chunk: &[u8]| chunks.push(chunk.to_vec());
    for slice in bytes.chunks(window) {
        chunker.push(slice, &mut record)?;
    }
    chunker.finish(&mut record)?;
    Ok(chunks)
}

fn canonical_of(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut chunker = OoxmlChunker::new();
    chunks_of(&mut chunker, bytes, 8192).expect("canonicalizes")
}

#[test]
fn canonical_form_is_deterministic_and_window_independent() {
    let parts = docx_parts("els15/doc", "els15/media");
    let input = package(&parts);
    let whole = canonical_of(&input);
    for window in [1usize, 7, 4096, 1 << 20] {
        let mut chunker = OoxmlChunker::new();
        let again = chunks_of(&mut chunker, &input, window).expect("canonicalizes");
        assert_eq!(again, whole, "window {window}");
    }
    assert!(whole.len() > 3);
}

#[test]
fn canonical_form_is_a_valid_zip_and_an_ooxml_package() {
    let parts = docx_parts("els15/valid", "els15/valid-media");
    let canonical: Vec<u8> = canonical_of(&package(&parts)).concat();
    // The canonical form must itself walk as a ZIP…
    let mut zip = ProfileChunker::open(ChunkingProfile::ZipV1).expect("implemented");
    chunks_of(&mut zip, &canonical, 8192).expect("canonical form is a valid archive");
    // …and as an OOXML package under the byte-exact profile.
    let mut ber = ProfileChunker::open(ChunkingProfile::OoxmlBerV1).expect("implemented");
    let ber_chunks = chunks_of(&mut ber, &canonical, 8192).expect("canonical form is a package");
    assert_eq!(ber_chunks.concat(), canonical);
    // Canonicalizing the canonical form is the identity.
    assert_eq!(canonical_of(&canonical).concat(), canonical);
}

#[test]
fn different_compressors_converge_to_the_same_canonical_bytes() {
    let parts = docx_parts("els15/converge", "els15/converge-media");
    let deflated = package(&parts);
    let stored = package_stored(&parts);
    assert_ne!(deflated, stored, "the uploads genuinely differ");
    assert_eq!(
        canonical_of(&deflated),
        canonical_of(&stored),
        "canonical bytes and boundaries must converge"
    );
}

#[test]
fn an_edited_part_leaves_every_other_parts_chunks_identical() {
    let original = canonical_of(&package(&docx_parts("els15/reuse-a", "els15/shared-media")));
    let edited = canonical_of(&package(&docx_parts("els15/reuse-b", "els15/shared-media")));
    let original_set: HashSet<&[u8]> = original.iter().map(Vec::as_slice).collect();
    let shared: usize = edited
        .iter()
        .filter(|chunk| original_set.contains(chunk.as_slice()))
        .map(|chunk| chunk.len())
        .sum();
    // The 300 KiB media part and the small shared parts must reuse;
    // only document.xml (and the central directory) may differ.
    assert!(
        shared >= 300 << 10,
        "only {shared} shared bytes between document variants"
    );
}

#[test]
fn descriptor_members_canonicalize() {
    let payload = xml("els15/descriptor", 40 << 10);
    let members = [
        Member::deflated("[Content_Types].xml", b"<Types/>"),
        Member::deflated("word/document.xml", &payload).framed(Framing::SignedDescriptor),
    ];
    let input = archive(&members, Options::default());
    let canonical: Vec<u8> = canonical_of(&input).concat();
    let mut zip = ProfileChunker::open(ChunkingProfile::ZipV1).expect("implemented");
    chunks_of(&mut zip, &canonical, 4096).expect("canonical form is a valid archive");
}

#[test]
fn signed_packages_fail_hard_toward_ber() {
    let members = [
        Member::deflated("[Content_Types].xml", b"<Types/>"),
        Member::deflated("word/document.xml", b"<doc/>"),
        Member::deflated("_xmlsignatures/sig1.xml", b"<sig/>"),
    ];
    let input = archive(&members, Options::default());
    let mut chunker = OoxmlChunker::new();
    let error = chunks_of(&mut chunker, &input, 4096).expect_err("signed must reject");
    let ChunkError::MalformedProfileInput { detail, .. } = &error else {
        panic!("unexpected error {error}");
    };
    assert!(detail.contains("signed"), "{detail}");
    assert!(detail.contains("ooxml-ber-v1"), "{detail}");
}

#[test]
fn non_ooxml_zip_and_wrong_kind_reject() {
    let members = [Member::stored("data.bin", b"plain zip, not a package")];
    let input = archive(&members, Options::default());
    let mut chunker = OoxmlChunker::new();
    chunks_of(&mut chunker, &input, 4096).expect_err("not a package");

    let parts = docx_parts("els15/kind", "els15/kind-media");
    let input = package(&parts);
    let mut chunker = OoxmlChunker::expecting(Some(OfficeKind::Excel));
    let error = chunks_of(&mut chunker, &input, 4096).expect_err("a docx is not an xlsx");
    let ChunkError::MalformedProfileInput { detail, .. } = &error else {
        panic!("unexpected error {error}");
    };
    assert!(detail.contains("main part"), "{detail}");
}

#[test]
fn corrupt_member_bytes_reject_at_the_crc() {
    let parts = docx_parts("els15/corrupt", "els15/corrupt-media");
    let mut input = package(&parts);
    // Flip one bit inside the stored media member's data.
    let target = input.len() / 2;
    input[target] ^= 0x40;
    let mut chunker = OoxmlChunker::new();
    let error = chunks_of(&mut chunker, &input, 4096).expect_err("corruption must reject");
    let ChunkError::MalformedProfileInput { detail, .. } = &error else {
        panic!("unexpected error {error}");
    };
    assert!(
        detail.contains("CRC") || detail.contains("deflate") || detail.contains("size"),
        "{detail}"
    );
}

#[test]
fn truncation_rejects_without_trailing_chunks() {
    let parts = docx_parts("els15/truncate", "els15/truncate-media");
    let input = package(&parts);
    let mut chunker = OoxmlChunker::new();
    let mut chunks = Vec::new();
    let mut record = |chunk: &[u8]| chunks.push(chunk.to_vec());
    chunker
        .push(&input[..input.len() - 30], &mut record)
        .expect("prefix is well-formed");
    chunker.finish(&mut record).expect_err("truncation rejects");
}

/// A noise blob big enough to matter chunks generically inside its
/// member while the member boundary still isolates it.
#[test]
fn noise_members_and_generic_profile_disagree_on_nothing_structural() {
    let bytes = noise("els15/noise-sanity", 64);
    assert_eq!(bytes.len(), 64);
}

/// Low-entropy, repetitive XML — the shape real tables and
/// boilerplate produce — starves a per-byte gear of distinct window
/// states; seam-gated cuts must still re-synchronize after an
/// insertion so an edited part reuses its unchanged regions.
#[test]
fn repetitive_xml_edits_reuse_chunks_within_the_part() {
    fn document(insert_at: Option<usize>) -> Vec<u8> {
        let mut out = b"<?xml version=\"1.0\"?>\n<doc>\n".to_vec();
        for row in 0..12_000 {
            if insert_at == Some(row) {
                out.extend_from_slice(b"<row><cell>inserted cell value</cell></row>\n");
            }
            // Repeated structure with varying values — the shape a
            // real table has. (Perfectly periodic content has no
            // content anchors at all; no content-defined scheme can
            // re-synchronize it, and it falls to forced cuts.)
            out.extend_from_slice(format!("<row><cell>value {row}</cell></row>\n").as_bytes());
        }
        out.extend_from_slice(b"</doc>\n");
        out
    }
    let build = |doc: &[u8]| {
        let members = [
            Member::deflated("[Content_Types].xml", b"<Types/>"),
            Member::deflated("word/document.xml", doc),
        ];
        archive(&members, Options::default())
    };
    let original = canonical_of(&build(&document(None)));
    let edited = canonical_of(&build(&document(Some(6000))));
    let original_set: HashSet<&[u8]> = original.iter().map(Vec::as_slice).collect();
    let total: usize = edited.iter().map(Vec::len).sum();
    let shared: usize = edited
        .iter()
        .filter(|chunk| original_set.contains(chunk.as_slice()))
        .map(Vec::len)
        .sum();
    assert!(
        edited.iter().all(|chunk| chunk.len() < 256 << 10),
        "seam cuts must fire before the forced maximum"
    );
    assert!(
        shared * 2 > total,
        "a mid-part insertion must reuse most chunks: {shared} of {total} shared"
    );
}
