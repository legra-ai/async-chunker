//! [`ProfileSet`] — a bounded set of chunking profiles.

use std::fmt;

use crate::profile::ChunkingProfile;

/// A set of registry profiles: one bit per entry of
/// [`ChunkingProfile::ALL`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ProfileSet(u16);

impl ProfileSet {
    /// The empty set.
    pub const EMPTY: Self = Self(0);

    /// A set holding only `profile`.
    #[must_use]
    pub fn single(profile: ChunkingProfile) -> Self {
        let mut set = Self::EMPTY;
        set.insert(profile);
        set
    }

    fn bit(profile: ChunkingProfile) -> u16 {
        let index = ChunkingProfile::ALL
            .iter()
            .position(|entry| *entry == profile)
            .expect("every profile is a registry entry");
        1 << index
    }

    /// Add `profile`.
    pub fn insert(&mut self, profile: ChunkingProfile) {
        self.0 |= Self::bit(profile);
    }

    /// Whether the set holds `profile`.
    #[must_use]
    pub fn contains(self, profile: ChunkingProfile) -> bool {
        self.0 & Self::bit(profile) != 0
    }

    /// How many profiles the set holds.
    #[must_use]
    pub const fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    /// Whether the set is empty.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The members in registry order.
    pub fn iter(self) -> impl Iterator<Item = ChunkingProfile> {
        ChunkingProfile::ALL
            .into_iter()
            .filter(move |profile| self.contains(*profile))
    }
}

impl fmt::Display for ProfileSet {
    /// The member names, comma-separated, in registry order.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for profile in self.iter() {
            if !first {
                f.write_str(", ")?;
            }
            first = false;
            f.write_str(profile.name())?;
        }
        Ok(())
    }
}
