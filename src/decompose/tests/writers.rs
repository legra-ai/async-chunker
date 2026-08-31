//! Canonical writers: deterministic output that decomposes back to
//! the same logical content.

use super::super::sink::{ContainerKind, EntryKind};
use super::super::writer::{CanonicalTarWriter, CanonicalZipWriter, GzipWriter};
use super::fixtures::{noise, prose};
use super::recorder::{Ev, run};

#[test]
fn canonical_tar_round_trips_including_long_paths() {
    let a = prose("dec/wtar-a", 20 << 10);
    let long_path = [b"nested/".repeat(20).as_slice(), b"leaf.bin"].concat();
    let b = noise("dec/wtar-b", 5 << 10);
    let mut out = Vec::new();
    let mut emit = |bytes: &[u8]| out.extend_from_slice(bytes);
    let mut writer = CanonicalTarWriter::new();
    writer.entry(&EntryKind::Directory, b"nested/", &mut emit);
    writer.begin_member(b"a.txt", a.len() as u64, &mut emit);
    writer.member_bytes(&a, &mut emit);
    writer.end_member(&mut emit);
    writer.begin_member(&long_path, b.len() as u64, &mut emit);
    writer.member_bytes(&b, &mut emit);
    writer.end_member(&mut emit);
    writer.finish(&mut emit);

    let again = {
        let mut out2 = Vec::new();
        let mut emit2 = |bytes: &[u8]| out2.extend_from_slice(bytes);
        let mut writer = CanonicalTarWriter::new();
        writer.entry(&EntryKind::Directory, b"nested/", &mut emit2);
        writer.begin_member(b"a.txt", a.len() as u64, &mut emit2);
        writer.member_bytes(&a, &mut emit2);
        writer.end_member(&mut emit2);
        writer.begin_member(&long_path, b.len() as u64, &mut emit2);
        writer.member_bytes(&b, &mut emit2);
        writer.end_member(&mut emit2);
        writer.finish(&mut emit2);
        out2
    };
    assert_eq!(out, again, "deterministic");

    let walked = run(&out, 512).expect("canonical tar decomposes");
    assert_eq!(walked.events[0], Ev::Start(ContainerKind::Tar, 0));
    assert_eq!(walked.member_bytes_of(b"a.txt"), a);
    assert_eq!(walked.member_bytes_of(&long_path), b);
}

#[test]
fn canonical_zip_round_trips() {
    let text = prose("dec/wzip", 30 << 10);
    let mut out = Vec::new();
    let mut emit = |bytes: &[u8]| out.extend_from_slice(bytes);
    let mut writer = CanonicalZipWriter::new();
    writer.directory(b"docs", &mut emit);
    writer.begin_member(b"docs/readme.md", &mut emit);
    writer.member_bytes(&text, &mut emit);
    writer.end_member(&mut emit);
    writer.finish(&mut emit);

    let walked = run(&out, 4096).expect("canonical zip decomposes");
    assert_eq!(walked.events[0], Ev::Start(ContainerKind::Zip, 0));
    assert_eq!(walked.member_bytes_of(b"docs/readme.md"), text);
}

#[test]
fn gzip_writer_round_trips_through_the_reader() {
    let payload = prose("dec/wgz", 40 << 10);
    let mut out = Vec::new();
    let mut emit = |bytes: &[u8]| out.extend_from_slice(bytes);
    let mut writer = GzipWriter::new();
    for slice in payload.chunks(1000) {
        writer.push(slice, &mut emit);
    }
    writer.finish(&mut emit);

    let walked = run(&out, 512).expect("canonical gzip decomposes");
    assert_eq!(walked.events[0], Ev::Start(ContainerKind::Gzip, 0));
    assert_eq!(walked.member_bytes_of(b""), payload);
}

#[test]
fn canonical_tar_of_a_decomposed_gzip_stays_stable() {
    // writer(reader(writer(x))) == writer(x): logical stability.
    let payload = noise("dec/stable", 24 << 10);
    let build = |bytes: &[u8]| {
        let mut out = Vec::new();
        let mut emit = |chunk: &[u8]| out.extend_from_slice(chunk);
        let mut writer = CanonicalTarWriter::new();
        writer.begin_member(b"data.bin", bytes.len() as u64, &mut emit);
        writer.member_bytes(bytes, &mut emit);
        writer.end_member(&mut emit);
        writer.finish(&mut emit);
        out
    };
    let first = build(&payload);
    let walked = run(&first, 512).expect("decomposes");
    let second = build(&walked.member_bytes_of(b"data.bin"));
    assert_eq!(first, second);
}
