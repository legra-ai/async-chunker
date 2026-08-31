//! `ooxml-ber-v1` tests: byte-exactness, part-aligned reuse, and
//! validation.

use std::collections::HashSet;

use super::super::OoxmlBerChunker;
use super::canonical::chunks_of;
use super::fixtures::{docx_parts, package};
use crate::chunker::zip::tests::writer::{Member, Options, archive};

fn ber_chunks(bytes: &[u8], window: usize) -> Vec<Vec<u8>> {
    let mut chunker = OoxmlBerChunker::new();
    chunks_of(&mut chunker, bytes, window).expect("valid package")
}

#[test]
fn chunks_concatenate_to_the_input_bytes() {
    let input = package(&docx_parts("els15/ber", "els15/ber-media"));
    let chunks = ber_chunks(&input, 4096);
    assert_eq!(chunks.concat(), input, "BER must be byte-exact");
    assert_eq!(ber_chunks(&input, 1 << 20), chunks, "window independent");
}

#[test]
fn signed_packages_are_accepted() {
    let members = [
        Member::deflated("[Content_Types].xml", b"<Types/>"),
        Member::deflated("word/document.xml", b"<doc/>"),
        Member::deflated("_xmlsignatures/sig1.xml", b"<sig/>"),
    ];
    let input = archive(&members, Options::default());
    let chunks = ber_chunks(&input, 4096);
    assert_eq!(chunks.concat(), input);
}

#[test]
fn unchanged_stored_parts_share_chunks_across_edits() {
    let original = ber_chunks(
        &package(&docx_parts("els15/ber-a", "els15/ber-media")),
        4096,
    );
    let edited = ber_chunks(
        &package(&docx_parts("els15/ber-b", "els15/ber-media")),
        4096,
    );
    let original_set: HashSet<&[u8]> = original.iter().map(Vec::as_slice).collect();
    let shared: usize = edited
        .iter()
        .filter(|chunk| original_set.contains(chunk.as_slice()))
        .map(|chunk| chunk.len())
        .sum();
    assert!(
        shared >= 280 << 10,
        "only {shared} shared bytes; the stored media member must reuse"
    );
}

#[test]
fn non_package_zips_reject() {
    let members = [Member::stored("data.bin", b"not a package")];
    let input = archive(&members, Options::default());
    let mut chunker = OoxmlBerChunker::new();
    chunks_of(&mut chunker, &input, 4096).expect_err("no [Content_Types].xml first");
}
