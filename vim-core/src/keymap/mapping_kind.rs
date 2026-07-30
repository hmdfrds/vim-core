//! Mapping recursion behavior.

/// Whether a key mapping is recursive (`:map`) or non-recursive (`:noremap`).
///
/// Recursive mappings will re-expand the output through the mapping system,
/// allowing chained mappings. Non-recursive mappings bypass re-expansion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum MappingKind {
    /// Recursive mapping (`:map`, `:nmap`, etc.).
    /// The output is fed back through the mapping system.
    Recursive,
    /// Non-recursive mapping (`:noremap`, `:nnoremap`, etc.).
    /// The output bypasses the mapping system.
    NonRecursive,
}

impl MappingKind {
    /// Whether this mapping is recursive.
    #[inline]
    #[must_use]
    pub const fn is_recursive(self) -> bool {
        matches!(self, Self::Recursive)
    }

    /// Create from a `bool` flag (true = recursive).
    #[inline]
    #[must_use]
    pub const fn from_recursive_flag(recursive: bool) -> Self {
        if recursive {
            Self::Recursive
        } else {
            Self::NonRecursive
        }
    }
}

impl Default for MappingKind {
    /// Default is non-recursive (safer, prevents infinite loops).
    fn default() -> Self {
        Self::NonRecursive
    }
}
