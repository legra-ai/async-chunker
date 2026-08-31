use std::collections::BTreeSet;

use super::ProfileRegistry;
use crate::media_type::MediaType;
use crate::profile::ChunkingProfile;

fn media(text: &str) -> MediaType {
    MediaType::parse(text).expect("valid media type")
}

#[test]
fn selects_each_specialist_family() {
    let registry = ProfileRegistry::V1;
    let cases = [
        ("text/plain", ChunkingProfile::StructuredTextV1),
        ("image/svg+xml", ChunkingProfile::StructuredTextV1),
        ("application/zip", ChunkingProfile::ZipV1),
        (
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            ChunkingProfile::ZipV1,
        ),
        ("video/mp4", ChunkingProfile::IsobmffV1),
        ("image/avif", ChunkingProfile::IsobmffV1),
        ("video/webm", ChunkingProfile::MatroskaV1),
        ("video/mp2t", ChunkingProfile::MpegtsV1),
        ("audio/flac", ChunkingProfile::FramedAudioV1),
    ];
    for (text, expected) in cases {
        assert_eq!(registry.select(&media(text)), expected, "{text}");
        assert_eq!(registry.specialist(&media(text)), Some(expected), "{text}");
    }
}

#[test]
fn unlisted_media_types_select_the_generic_profile_explicitly() {
    let registry = ProfileRegistry::V1;
    for text in ["application/octet-stream", "image/png", "application/pdf"] {
        assert_eq!(registry.specialist(&media(text)), None, "{text}");
        assert_eq!(
            registry.select(&media(text)),
            ChunkingProfile::GenericCdcV1,
            "{text}"
        );
    }
}

#[test]
fn parameters_and_case_never_change_the_selection() {
    let registry = ProfileRegistry::V1;
    assert_eq!(
        registry.select(&media("Text/Plain; charset=utf-8")),
        ChunkingProfile::StructuredTextV1
    );
}

#[test]
fn v1_families_are_disjoint_and_frozen() {
    let registry = ProfileRegistry::V1;
    assert_eq!(registry.version().value(), 1);
    assert_eq!(registry.version().to_string(), "registry-v1");
    let mut seen = BTreeSet::new();
    let mut total = 0;
    for profile in ChunkingProfile::ALL {
        for essence in registry.essences(profile) {
            assert!(seen.insert(essence), "{essence} listed twice");
            assert_eq!(
                media(essence).essence(),
                essence,
                "{essence} not normalized"
            );
            total += 1;
        }
    }
    assert_eq!(total, registry.specialist_count());
    assert_eq!(total, 52);
    assert_eq!(registry.essences(ChunkingProfile::GenericCdcV1).count(), 0);
}

#[test]
fn v2_moves_office_to_ooxml_and_adds_pdf() {
    let registry = ProfileRegistry::V2;
    assert_eq!(registry.version().value(), 2);
    assert_eq!(ProfileRegistry::default().version().value(), 2);
    for essence in [
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ] {
        assert_eq!(registry.select(&media(essence)), ChunkingProfile::OoxmlV1);
        assert_eq!(
            ProfileRegistry::V1.select(&media(essence)),
            ChunkingProfile::ZipV1,
            "v1 stays frozen"
        );
    }
    assert_eq!(
        registry.select(&media("application/pdf")),
        ChunkingProfile::PdfV1
    );
    assert_eq!(
        registry.select(&media("application/vnd.oasis.opendocument.text")),
        ChunkingProfile::ZipV1,
        "ODF stays on zip-v1 (its first member is 'mimetype')"
    );
    assert_eq!(
        registry.select(&media("application/zip")),
        ChunkingProfile::ZipV1
    );
    assert_eq!(
        registry.essences(ChunkingProfile::OoxmlBerV1).count(),
        0,
        "BER is a policy selection, never a registry mapping"
    );
}

#[test]
fn v2_families_are_disjoint() {
    let registry = ProfileRegistry::V2;
    let mut seen = BTreeSet::new();
    let mut total = 0;
    for profile in ChunkingProfile::ALL {
        for essence in registry.essences(profile) {
            assert!(seen.insert(essence), "{essence} listed twice");
            total += 1;
        }
    }
    assert_eq!(total, registry.specialist_count());
    assert_eq!(total, 53);
}
