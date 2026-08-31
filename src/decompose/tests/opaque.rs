//! Opaque and rejection outcomes.

use super::super::fault::{DecomposeError, OpaqueReason};
use super::fixtures::{TarFix, gzip, noise, prose, tar};
use super::recorder::run;
use crate::chunker::zip::tests::writer::{Member, Options, archive};

fn expect_opaque(result: Result<super::recorder::Recorder, DecomposeError>) -> OpaqueReason {
    match result {
        Err(DecomposeError::Opaque(reason)) => reason,
        Err(other) => panic!("expected opaque, got {other}"),
        Ok(_) => panic!("expected opaque, got a decomposition"),
    }
}

#[test]
fn unrecognized_bytes_are_not_a_container() {
    let result = run(&prose("dec/not-container", 4 << 10), 512);
    assert!(matches!(result, Err(DecomposeError::NotAContainer)));
    let result = run(b"tiny", 512);
    assert!(matches!(result, Err(DecomposeError::NotAContainer)));
}

#[test]
fn encrypted_zip_members_are_opaque() {
    let members = [Member::deflated("secret.txt", b"cipher bytes here")];
    let mut input = archive(&members, Options::default());
    // Set the encryption bit in the local header's flags.
    input[6] |= 0x01;
    assert_eq!(
        expect_opaque(run(&input, 4096)),
        OpaqueReason::EncryptedWithoutKey
    );
}

#[test]
fn malformed_and_truncated_containers_are_opaque() {
    let mut bad = tar(&[TarFix::File {
        path: b"a.txt",
        bytes: b"hello world",
    }]);
    bad[150] ^= 0x01; // break the header checksum
    assert!(matches!(
        expect_opaque(run(&bad, 512)),
        OpaqueReason::Malformed { .. }
    ));

    let whole = tar(&[TarFix::File {
        path: b"a.txt",
        bytes: &noise("dec/trunc", 4 << 10),
    }]);
    let truncated = &whole[..whole.len() - 700];
    assert!(matches!(
        expect_opaque(run(truncated, 512)),
        OpaqueReason::Malformed { .. }
    ));

    let mut gz = gzip(b"payload", None);
    let len = gz.len();
    gz.truncate(len - 3);
    assert!(matches!(
        expect_opaque(run(&gz, 512)),
        OpaqueReason::Malformed { .. }
    ));
}

#[test]
fn unsafe_paths_are_opaque() {
    let escape = tar(&[TarFix::File {
        path: b"../escape.txt",
        bytes: b"nope",
    }]);
    assert_eq!(expect_opaque(run(&escape, 512)), OpaqueReason::UnsafePath);
    let absolute = tar(&[TarFix::File {
        path: b"/etc/passwd",
        bytes: b"nope",
    }]);
    assert_eq!(expect_opaque(run(&absolute, 512)), OpaqueReason::UnsafePath);
}

#[test]
fn nesting_beyond_the_depth_cap_is_opaque() {
    // gz(gz(gz(...))) — each wrap is a container level.
    let mut payload = prose("dec/depth", 2 << 10);
    for _ in 0..9 {
        payload = gzip(&payload, None);
    }
    assert_eq!(
        expect_opaque(run(&payload, 512)),
        OpaqueReason::DepthExceeded
    );
}

#[test]
fn sparse_tar_members_are_unsupported() {
    let mut input = tar(&[TarFix::File {
        path: b"normal.txt",
        bytes: b"hello world padding padding",
    }]);
    input[156] = b'S';
    // Fix the checksum for the altered typeflag.
    input[148..156].copy_from_slice(b"        ");
    let sum: u64 = input[..512].iter().map(|byte| u64::from(*byte)).sum();
    let text = format!("{sum:06o}");
    input[148..154].copy_from_slice(text.as_bytes());
    input[154] = 0;
    input[155] = b' ';
    assert!(matches!(
        expect_opaque(run(&input, 512)),
        OpaqueReason::UnsupportedFeature { .. }
    ));
}

#[test]
fn the_stream_stays_rejected_after_an_opaque_outcome() {
    let mut decomposer = super::super::Decomposer::new();
    let mut recorder = super::recorder::Recorder::default();
    let escape = tar(&[TarFix::File {
        path: b"../escape.txt",
        bytes: b"nope",
    }]);
    let mut failed = false;
    for slice in escape.chunks(512) {
        if decomposer.push(slice, &mut recorder).is_err() {
            failed = true;
            break;
        }
    }
    assert!(failed);
    assert!(matches!(
        decomposer.push(b"more", &mut recorder),
        Err(DecomposeError::StreamRejected)
    ));
    assert!(matches!(
        decomposer.finish(&mut recorder),
        Err(DecomposeError::StreamRejected)
    ));
}
