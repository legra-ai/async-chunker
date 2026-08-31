//! [`PackageObserver`] — the event tap a caller may attach to the
//! canonicalizing `ooxml-v1` chunker.

/// Observes the canonical package stream as it is produced: member
/// names, the members' canonical (inflated) bytes, and canonical
/// offsets — the coordinates any derived artifact should anchor to.
///
/// Every method has a no-op default; the tap costs nothing when
/// unused. Callbacks are synchronous and must stay cheap and
/// bounded — heavy derivation belongs in a consumer fed from these
/// events, not inside them.
pub trait PackageObserver: Send {
    /// A member begins. `canonical_offset` is the offset of its
    /// canonical local header in the canonical stream.
    fn member_start(&mut self, name: &[u8], canonical_offset: u64) {
        let _ = (name, canonical_offset);
    }

    /// One window of the member's canonical (inflated) bytes, in
    /// order.
    fn member_bytes(&mut self, bytes: &[u8]) {
        let _ = bytes;
    }

    /// The member's canonical bytes ended; `canonical_len` is their
    /// total length.
    fn member_end(&mut self, canonical_len: u64) {
        let _ = canonical_len;
    }

    /// All members have been seen (the central directory began).
    fn package_end(&mut self, member_count: u64) {
        let _ = member_count;
    }
}
