//! Decomposition behavior: per-format walks, recursion, inference.

use super::super::sink::{ContainerKind, EntryKind};
use super::fixtures::{TarFix, gzip, noise, prose, tar};
use super::recorder::{Ev, run};
use crate::chunker::zip::tests::writer::{Member, Options, archive};

#[test]
fn zip_members_decompress_and_directories_are_entries() {
    let text = prose("dec/zip-text", 40 << 10);
    let binary = noise("dec/zip-bin", 30 << 10);
    let members = [
        Member::deflated("docs/readme.md", &text),
        Member::stored("assets/", b""),
        Member::stored("assets/blob.bin", &binary),
    ];
    let input = archive(&members, Options::default());
    let whole = run(&input, 4096).expect("decomposes");
    assert_eq!(whole.events[0], Ev::Start(ContainerKind::Zip, 0));
    assert!(
        whole
            .events
            .contains(&Ev::Entry(EntryKind::Directory, b"assets/".to_vec(), 0))
    );
    assert_eq!(whole.member_bytes_of(b"docs/readme.md"), text);
    assert_eq!(whole.member_bytes_of(b"assets/blob.bin"), binary);
    let Ev::End(facts, 0) = whole.events.last().expect("events") else {
        panic!("last event must be container end");
    };
    assert_eq!(facts.kind, ContainerKind::Zip);
    assert_eq!(facts.member_count, 2);
    assert_eq!(facts.entry_count, 1);
    assert_eq!(facts.office_kind, None);
    for window in [1usize, 7, 1 << 20] {
        assert_eq!(
            run(&input, window).expect("decomposes").events,
            whole.events,
            "window {window}"
        );
    }
}

#[test]
fn zip_member_media_types_are_inferred() {
    let text = prose("dec/media-text", 8 << 10);
    let photo = noise("dec/media-noise", 8 << 10);
    let mystery = noise("dec/media-unknown", 4 << 10);
    let members = [
        Member::deflated("notes.md", &text),
        Member::stored("photo.png", &photo),
        Member::stored("mystery", &mystery),
    ];
    let input = archive(&members, Options::default());
    let whole = run(&input, 4096).expect("decomposes");
    let media_of = |path: &[u8]| {
        whole.events.iter().find_map(|event| match event {
            Ev::Member {
                path: at, media, ..
            } if at == path => Some(media.clone()),
            _ => None,
        })
    };
    assert_eq!(media_of(b"notes.md"), Some(Some("text/markdown".into())));
    assert_eq!(media_of(b"photo.png"), Some(Some("image/png".into())));
    assert_eq!(media_of(b"mystery"), Some(None));
}

#[test]
fn office_packages_report_their_kind() {
    let document = prose("dec/office", 8 << 10);
    let members = [
        Member::deflated("[Content_Types].xml", b"<Types/>"),
        Member::deflated("word/document.xml", &document),
    ];
    let input = archive(&members, Options::default());
    let whole = run(&input, 4096).expect("decomposes");
    let Ev::End(facts, 0) = whole.events.last().expect("events") else {
        panic!("last event must be container end");
    };
    assert_eq!(facts.office_kind, Some("word"));
}

#[test]
fn tar_walks_ustar_pax_gnu_links_and_dirs() {
    let a = prose("dec/tar-a", 20 << 10);
    let b = noise("dec/tar-b", 3 << 10);
    let long_path = [b"deep/".repeat(30).as_slice(), b"leaf.txt"].concat();
    let input = tar(&[
        TarFix::Dir { path: b"deep/" },
        TarFix::File {
            path: b"deep/a.txt",
            bytes: &a,
        },
        TarFix::Symlink {
            path: b"deep/link",
            target: b"a.txt",
        },
        TarFix::PaxFile {
            path: &long_path,
            bytes: &b,
        },
        TarFix::GnuFile {
            path: &long_path,
            bytes: &b,
        },
    ]);
    let whole = run(&input, 512).expect("decomposes");
    assert_eq!(whole.events[0], Ev::Start(ContainerKind::Tar, 0));
    assert!(
        whole
            .events
            .contains(&Ev::Entry(EntryKind::Directory, b"deep/".to_vec(), 0))
    );
    assert!(whole.events.contains(&Ev::Entry(
        EntryKind::Symlink {
            target: b"a.txt".to_vec().into_boxed_slice()
        },
        b"deep/link".to_vec(),
        0
    )));
    assert_eq!(whole.member_bytes_of(b"deep/a.txt"), a);
    assert_eq!(whole.member_bytes_of(&long_path), b, "pax path override");
    let gnu_members = whole
        .events
        .iter()
        .filter(|event| matches!(event, Ev::Member { path, .. } if path == &long_path))
        .count();
    assert_eq!(gnu_members, 2, "pax and GNU long names both resolve");
    assert_eq!(run(&input, 1).expect("decomposes").events, whole.events);
}

#[test]
fn tar_gz_detects_the_wrapped_archive() {
    let payload = prose("dec/targz", 30 << 10);
    let inner = tar(&[TarFix::File {
        path: b"data/file.txt",
        bytes: &payload,
    }]);
    let input = gzip(&inner, Some("data.tar"));
    let whole = run(&input, 4096).expect("decomposes");
    assert_eq!(whole.events[0], Ev::Start(ContainerKind::TarGzip, 0));
    assert_eq!(whole.member_bytes_of(b"data/file.txt"), payload);
    let Ev::End(facts, 0) = whole.events.last().expect("events") else {
        panic!("last event must be container end");
    };
    assert_eq!(facts.member_count, 1);
}

#[test]
fn bare_gzip_is_a_single_member_container() {
    let payload = prose("dec/gz-single", 20 << 10);
    let input = gzip(&payload, Some("report.csv"));
    let whole = run(&input, 4096).expect("decomposes");
    assert_eq!(whole.events[0], Ev::Start(ContainerKind::Gzip, 0));
    assert_eq!(whole.member_bytes_of(b"report.csv"), payload);
    let Ev::End(facts, 0) = whole.events.last().expect("events") else {
        panic!("last event must be container end");
    };
    assert_eq!(facts.member_count, 1);

    // Concatenated gzip members decompress to one logical stream.
    let mut concatenated = gzip(&payload[..8 << 10], None);
    concatenated.extend_from_slice(&gzip(&payload[8 << 10..], None));
    let whole = run(&concatenated, 4096).expect("decomposes");
    assert_eq!(whole.member_bytes_of(b""), payload);
}

#[test]
fn nested_containers_recurse_with_depths() {
    let leaf = prose("dec/nested-leaf", 12 << 10);
    let inner_zip = archive(
        &[Member::deflated("inner/leaf.txt", &leaf)],
        Options::default(),
    );
    let outer = tar(&[
        TarFix::File {
            path: b"bundle.zip",
            bytes: &inner_zip,
        },
        TarFix::File {
            path: b"plain.txt",
            bytes: &leaf,
        },
    ]);
    let whole = run(&outer, 4096).expect("decomposes");
    assert!(whole.events.contains(&Ev::Member {
        path: b"bundle.zip".to_vec(),
        media: Some("application/zip".into()),
        nested: true,
        depth: 0,
    }));
    assert_eq!(whole.events[0], Ev::Start(ContainerKind::Tar, 0));
    assert!(whole.events.contains(&Ev::Start(ContainerKind::Zip, 1)));
    let inner_member = whole.events.iter().find(|event| {
        matches!(event, Ev::Member { path, depth, .. } if path == b"inner/leaf.txt" && *depth == 1)
    });
    assert!(inner_member.is_some(), "inner members surface at depth 1");
    assert_eq!(whole.member_bytes_of(b"inner/leaf.txt"), leaf);
    assert_eq!(whole.member_bytes_of(b"plain.txt"), leaf);
    // The nested member's container closed before the outer resumed.
    let inner_end = whole
        .events
        .iter()
        .position(|event| matches!(event, Ev::End(facts, 1) if facts.kind == ContainerKind::Zip))
        .expect("inner container end");
    let outer_end = whole.events.len() - 1;
    assert!(inner_end < outer_end);
    assert_eq!(run(&outer, 3).expect("decomposes").events, whole.events);
}

#[test]
fn container_bytes_under_a_non_container_name_stay_a_member() {
    let inner_zip = archive(&[Member::deflated("x.txt", b"hello")], Options::default());
    let outer = tar(&[TarFix::File {
        path: b"snapshot.png",
        bytes: &inner_zip,
    }]);
    let whole = run(&outer, 4096).expect("decomposes");
    assert!(whole.events.contains(&Ev::Member {
        path: b"snapshot.png".to_vec(),
        media: Some("image/png".into()),
        nested: false,
        depth: 0,
    }));
    assert_eq!(whole.member_bytes_of(b"snapshot.png"), inner_zip);
}
