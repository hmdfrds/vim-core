//! The tag a host puts on a config, kept with what the config defines.

/// A tag a host puts on a config, such as how far it trusts the file.
/// vim-core never reads it; it keeps it with each mapping and
/// abbreviation the config defines, and hands it back to the host.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TrustTag(pub u32);

impl TrustTag {
    /// The tag `n`.
    #[must_use]
    pub const fn new(n: u32) -> Self {
        Self(n)
    }

    /// The number of the tag.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}
