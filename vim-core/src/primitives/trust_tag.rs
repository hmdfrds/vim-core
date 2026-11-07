//! The tag a host puts on a config, kept with what the config defines.

/// A tag a host puts on a config, such as how far it trusts the file.
/// vim-core keeps it with each mapping and abbreviation the config
/// defines, and hands it back to the host.
///
/// It only compares tags where it must keep one of several: a mapping or
/// abbreviation that a command line made by keys of several tags defines
/// keeps the highest, so a host numbers its tags from the most trusted
/// up.
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

/// The non-default tags of the keys that made something, such as a command
/// line, each once and in order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct Tags(smallvec::SmallVec<[TrustTag; 1]>);

impl Tags {
    /// No tag.
    pub(crate) fn new() -> Self {
        Self(smallvec::SmallVec::new())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn as_slice(&self) -> &[TrustTag] {
        &self.0
    }

    /// Add `tag`, unless it is the default.
    pub(crate) fn insert(&mut self, tag: TrustTag) {
        if tag == TrustTag::default() {
            return;
        }
        if let Err(at) = self.0.binary_search_by_key(&tag.get(), |t| t.get()) {
            self.0.insert(at, tag);
        }
    }

    /// The highest tag, the strictest (see [`TrustTag`]); the default when
    /// there is none.
    pub(crate) fn strictest(&self) -> TrustTag {
        self.0.last().copied().unwrap_or_default()
    }

    /// Add every tag of `other`.
    pub(crate) fn extend(&mut self, other: &Self) {
        for &tag in &other.0 {
            self.insert(tag);
        }
    }
}
