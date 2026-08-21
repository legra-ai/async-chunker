//! A small EBML writer for fixtures: elements with minimal-length
//! sizes, unknown-size segments and clusters, and Matroska-shaped
//! files.

/// Deterministic pseudo-random bytes (incompressible, like media).
pub(super) fn noise(seed: &str, len: usize) -> Vec<u8> {
    // bounded: fixture payloads are test constants.
    let mut bytes = vec![0u8; len];
    let mut hasher = blake3::Hasher::new();
    hasher.update(seed.as_bytes());
    hasher.finalize_xof().fill(&mut bytes);
    bytes
}

/// An element ID's raw bytes.
pub(super) fn vid(id: u32) -> Vec<u8> {
    let raw = id.to_be_bytes();
    let skip = raw.iter().take_while(|&&byte| byte == 0).count();
    raw[skip..].to_vec()
}

/// A minimal-length size varint.
pub(super) fn vsize(value: u64) -> Vec<u8> {
    for len in 1usize..=8 {
        let capacity = (1u64 << (7 * len)) - 1;
        // The all-ones pattern is reserved for "unknown".
        if value < capacity {
            let marker = 0x80u8 >> (len - 1);
            let mut out = vec![0u8; len];
            let mut rest = value;
            for slot in out.iter_mut().rev() {
                *slot = (rest & 0xFF) as u8;
                rest >>= 8;
            }
            out[0] |= marker;
            return out;
        }
    }
    panic!("size too large for a varint");
}

/// The one-byte unknown-size varint.
pub(super) const UNKNOWN_SIZE: u8 = 0xFF;

/// A known-size element.
pub(super) fn el(id: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = vid(id);
    out.extend_from_slice(&vsize(payload.len() as u64));
    out.extend_from_slice(payload);
    out
}

/// An unknown-size element (its payload runs until something closes
/// it).
pub(super) fn el_unknown(id: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = vid(id);
    out.push(UNKNOWN_SIZE);
    out.extend_from_slice(payload);
    out
}

pub(super) const EBML_HEADER: u32 = 0x1A45_DFA3;
pub(super) const SEGMENT: u32 = 0x1853_8067;
pub(super) const INFO: u32 = 0x1549_A966;
pub(super) const TRACKS: u32 = 0x1654_AE6B;
pub(super) const TAGS: u32 = 0x1254_C367;
pub(super) const ATTACHMENTS: u32 = 0x1941_A469;
pub(super) const CUES: u32 = 0x1C53_BB6B;
pub(super) const CLUSTER: u32 = 0x1F43_B675;
pub(super) const TIMESTAMP: u32 = 0xE7;
pub(super) const SIMPLE_BLOCK: u32 = 0xA3;

/// The EBML header of a Matroska/WebM file.
pub(super) fn ebml_header(doc_type: &str) -> Vec<u8> {
    let mut body = el(0x4286, &[1]); // EBMLVersion
    body.extend_from_slice(&el(0x42F7, &[1])); // EBMLReadVersion
    body.extend_from_slice(&el(0x4282, doc_type.as_bytes())); // DocType
    body.extend_from_slice(&el(0x4287, &[4])); // DocTypeVersion
    el(EBML_HEADER, &body)
}

/// A known-size cluster: opaque to the walker, so its interior is
/// whatever bytes we like.
pub(super) fn cluster(seed: &str, len: usize) -> Vec<u8> {
    el(CLUSTER, &noise(seed, len))
}

/// An unknown-size cluster: its children must be valid cluster
/// elements (a timestamp, then blocks).
pub(super) fn open_cluster(seed: &str, blocks: usize, block_len: usize) -> Vec<u8> {
    let mut body = el(TIMESTAMP, &noise(seed, 2));
    for index in 0..blocks {
        body.extend_from_slice(&el(
            SIMPLE_BLOCK,
            &noise(&format!("{seed}/block{index}"), block_len),
        ));
    }
    el_unknown(CLUSTER, &body)
}

/// A complete known-size file: header, then a segment holding
/// `Info`, `Tracks`, the given clusters, and trailing elements.
pub(super) fn mkv(seed: &str, clusters: &[Vec<u8>], trailing: &[Vec<u8>]) -> Vec<u8> {
    let mut body = el(INFO, &noise(&format!("{seed}/info"), 180));
    body.extend_from_slice(&el(TRACKS, &noise(&format!("{seed}/tracks"), 300)));
    for cluster in clusters {
        body.extend_from_slice(cluster);
    }
    for element in trailing {
        body.extend_from_slice(element);
    }
    let mut out = ebml_header("matroska");
    out.extend_from_slice(&el(SEGMENT, &body));
    out
}

/// A streamed file: header, then an unknown-size segment whose
/// clusters are unknown-size too, running to the end of the stream.
pub(super) fn streamed_webm(seed: &str, clusters: usize) -> Vec<u8> {
    let mut out = ebml_header("webm");
    let mut body = el(INFO, &noise(&format!("{seed}/info"), 140));
    body.extend_from_slice(&el(TRACKS, &noise(&format!("{seed}/tracks"), 260)));
    for index in 0..clusters {
        body.extend_from_slice(&open_cluster(
            &format!("{seed}/cluster{index}"),
            24,
            16 << 10,
        ));
    }
    out.extend_from_slice(&el_unknown(SEGMENT, &body));
    out
}
