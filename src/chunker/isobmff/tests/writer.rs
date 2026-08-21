//! A small box writer for fixtures: compact, extended-size,
//! open-ended, and `uuid` boxes, containers, and MP4/HEIF-shaped
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

/// A compact-size box.
pub(super) fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(&((8 + payload.len()) as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);
    out
}

/// An extended-size (`size == 1`, 64-bit `largesize`) box.
pub(super) fn bx_large(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(16 + payload.len());
    out.extend_from_slice(&1u32.to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(&((16 + payload.len()) as u64).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// An open-ended (`size == 0`) box running to the end of the stream.
pub(super) fn bx_open(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);
    out
}

/// A `uuid` box with a 16-byte user type.
pub(super) fn bx_uuid(user_type: &[u8; 16], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(24 + payload.len());
    out.extend_from_slice(&((24 + payload.len()) as u32).to_be_bytes());
    out.extend_from_slice(b"uuid");
    out.extend_from_slice(user_type);
    out.extend_from_slice(payload);
    out
}

/// A container: a box whose payload is the concatenation of children.
pub(super) fn container(kind: &[u8; 4], children: &[Vec<u8>]) -> Vec<u8> {
    bx(kind, &children.concat())
}

/// A `FullBox` payload prefix (version + flags).
fn full(version: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![version, 0, 0, 0];
    out.extend_from_slice(body);
    out
}

/// An `ftyp` with the given major brand.
pub(super) fn ftyp(brand: &[u8; 4]) -> Vec<u8> {
    let mut payload = brand.to_vec();
    payload.extend_from_slice(&0u32.to_be_bytes());
    payload.extend_from_slice(brand);
    payload.extend_from_slice(b"isom");
    bx(b"ftyp", &payload)
}

/// A plausible `moov` for one track; `seed` varies the metadata so
/// a re-mux can be simulated.
pub(super) fn moov(seed: &str, sample_count: u32) -> Vec<u8> {
    let meta = noise(seed, 64);
    let mvhd = bx(b"mvhd", &full(0, &[&meta[..], &[0u8; 36]].concat()));
    let tkhd = bx(b"tkhd", &full(0, &[&meta[8..40], &[0u8; 48]].concat()));
    let mdhd = bx(b"mdhd", &full(0, &meta[..20]));
    let hdlr = bx(
        b"hdlr",
        &full(0, b"\0\0\0\0vide\0\0\0\0\0\0\0\0\0\0\0\0VideoHandler\0"),
    );
    let stsd = bx(
        b"stsd",
        &full(
            0,
            &[&1u32.to_be_bytes()[..], &bx(b"avc1", &noise(seed, 86))].concat(),
        ),
    );
    let stts = bx(
        b"stts",
        &full(
            0,
            &[
                &1u32.to_be_bytes()[..],
                &sample_count.to_be_bytes(),
                &1000u32.to_be_bytes(),
            ]
            .concat(),
        ),
    );
    let stsc = bx(
        b"stsc",
        &full(
            0,
            &[
                &1u32.to_be_bytes()[..],
                &1u32.to_be_bytes(),
                &sample_count.to_be_bytes(),
                &1u32.to_be_bytes(),
            ]
            .concat(),
        ),
    );
    let mut stsz_body = 0u32.to_be_bytes().to_vec();
    stsz_body.extend_from_slice(&sample_count.to_be_bytes());
    for index in 0..sample_count {
        stsz_body.extend_from_slice(&(4000 + (index * 37) % 900).to_be_bytes());
    }
    let stsz = bx(b"stsz", &full(0, &stsz_body));
    let stco = bx(
        b"stco",
        &full(0, &[&1u32.to_be_bytes()[..], &48u32.to_be_bytes()].concat()),
    );
    let stbl = container(b"stbl", &[stsd, stts, stsc, stsz, stco]);
    let dref = bx(
        b"dref",
        &full(
            0,
            &[&1u32.to_be_bytes()[..], &bx(b"url ", &full(0, &[]))].concat(),
        ),
    );
    let dinf = container(b"dinf", &[dref]);
    let vmhd = bx(b"vmhd", &full(0, &[0u8; 8]));
    let minf = container(b"minf", &[vmhd, dinf, stbl]);
    let mdia = container(b"mdia", &[mdhd, hdlr, minf]);
    let trak = container(b"trak", &[tkhd, mdia]);
    let udta = container(b"udta", &[bx(b"meta", &full(0, &noise(seed, 200)))]);
    container(b"moov", &[mvhd, trak, udta])
}

/// A progressive MP4: `ftyp`, `free`, `moov`, `mdat`.
pub(super) fn mp4(seed: &str, mdat: &[u8]) -> Vec<u8> {
    [
        ftyp(b"isom"),
        bx(b"free", &[0u8; 8]),
        moov(seed, 1200),
        bx(b"mdat", mdat),
    ]
    .concat()
}

/// A fragmented MP4: `ftyp`, `moov` (with `mvex`), then `moof`/`mdat`
/// pairs built from `fragments`.
pub(super) fn fragmented_mp4(seed: &str, fragments: &[&[u8]]) -> Vec<u8> {
    let mut out = ftyp(b"iso5");
    let mut movie = moov(seed, 0);
    // Graft an mvex into the moov payload.
    let mvex = container(b"mvex", &[bx(b"trex", &full(0, &[0u8; 20]))]);
    movie.extend_from_slice(&mvex);
    let total = movie.len() as u32;
    movie[..4].copy_from_slice(&total.to_be_bytes());
    out.extend_from_slice(&movie);
    for (index, fragment) in fragments.iter().enumerate() {
        let mfhd = bx(b"mfhd", &full(0, &(index as u32 + 1).to_be_bytes()));
        let tfhd = bx(b"tfhd", &full(0, &1u32.to_be_bytes()));
        let trun = bx(
            b"trun",
            &full(
                0,
                &[
                    &1u32.to_be_bytes()[..],
                    &(fragment.len() as u32).to_be_bytes(),
                ]
                .concat(),
            ),
        );
        let traf = container(b"traf", &[tfhd, trun]);
        out.extend_from_slice(&container(b"moof", &[mfhd, traf]));
        out.extend_from_slice(&bx(b"mdat", fragment));
    }
    out
}

/// A HEIF-shaped file: `ftyp(heic)`, an opaque `meta`, and the
/// picture data in `mdat`.
pub(super) fn heif(seed: &str, picture: &[u8]) -> Vec<u8> {
    let meta_body = full(
        0,
        &[
            bx(b"hdlr", &full(0, b"\0\0\0\0pict\0\0\0\0\0\0\0\0\0\0\0\0\0")),
            bx(b"pitm", &full(0, &1u16.to_be_bytes())),
            bx(b"iloc", &full(0, &noise(seed, 40))),
            bx(b"iinf", &full(0, &noise(seed, 30))),
        ]
        .concat(),
    );
    [ftyp(b"heic"), bx(b"meta", &meta_body), bx(b"mdat", picture)].concat()
}
