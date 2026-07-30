//! The `TextOp` enum — the atomic unit of a changeset.
//!
//! A changeset is a sequence of `TextOp` values that describe how to
//! transform one document into another. The three variants form a closed
//! algebra from OT/CRDT literature — no `#[non_exhaustive]` because adding
//! a fourth variant would break the composition algorithm.

use compact_str::CompactString;

/// An atomic text-transformation operation.
///
/// Changesets are sequences of these operations applied left-to-right
/// against a source document. The three variants are exhaustive by design:
/// Retain/Insert/Delete is the canonical OT basis.
///
/// # Byte semantics
///
/// `Retain` and `Delete` count **bytes** (matching `Offset(usize)` throughout
/// vim-core). `Insert` carries the literal text. All byte counts must
/// land on UTF-8 char boundaries — this is enforced by debug-assertions
/// in the builder and composition algorithms.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextOp {
    /// Advance past `n` bytes unchanged.
    ///
    /// Invariant: `n > 0` (enforced by the builder — zero-length retains
    /// are coalesced away).
    Retain(usize),

    /// Insert literal text at the current position.
    ///
    /// Uses `CompactString` for SSO (24-byte inline on 64-bit).
    /// Invariant: text is non-empty (enforced by the builder).
    Insert(CompactString),

    /// Delete (skip) `n` bytes from the input document.
    ///
    /// Invariant: `n > 0` (enforced by the builder).
    Delete(usize),
}

impl TextOp {
    /// Byte length of this operation's effect.
    ///
    /// - `Retain(n)` → `n` (bytes consumed and emitted)
    /// - `Insert(t)` → `t.len()` (bytes emitted)
    /// - `Delete(n)` → `n` (bytes consumed)
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::Retain(n) | Self::Delete(n) => *n,
            Self::Insert(t) => t.len(),
        }
    }

    /// Whether this operation has zero byte length.
    ///
    /// Well-formed changesets never contain empty ops, but this is useful
    /// during construction before coalescing.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Is this a `Retain`?
    #[inline]
    #[must_use]
    pub const fn is_retain(&self) -> bool {
        matches!(self, Self::Retain(_))
    }

    /// Is this an `Insert`?
    #[inline]
    #[must_use]
    pub const fn is_insert(&self) -> bool {
        matches!(self, Self::Insert(_))
    }

    /// Is this a `Delete`?
    #[inline]
    #[must_use]
    pub const fn is_delete(&self) -> bool {
        matches!(self, Self::Delete(_))
    }
}

impl std::fmt::Display for TextOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Retain(n) => write!(f, "={n}"),
            Self::Insert(t) => write!(f, "+\"{}\"", t.escape_default()),
            Self::Delete(n) => write!(f, "-{n}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retain_len() {
        assert_eq!(TextOp::Retain(10).len(), 10);
    }

    #[test]
    fn insert_len() {
        let op = TextOp::Insert(CompactString::from("hello"));
        assert_eq!(op.len(), 5);
    }

    #[test]
    fn delete_len() {
        assert_eq!(TextOp::Delete(7).len(), 7);
    }

    #[test]
    fn empty_ops() {
        assert!(TextOp::Retain(0).is_empty());
        assert!(TextOp::Insert(CompactString::from("")).is_empty());
        assert!(TextOp::Delete(0).is_empty());
        assert!(!TextOp::Retain(1).is_empty());
    }

    #[test]
    fn type_predicates() {
        assert!(TextOp::Retain(5).is_retain());
        assert!(!TextOp::Retain(5).is_insert());
        assert!(!TextOp::Retain(5).is_delete());

        let ins = TextOp::Insert(CompactString::from("x"));
        assert!(ins.is_insert());
        assert!(!ins.is_retain());
        assert!(!ins.is_delete());

        assert!(TextOp::Delete(3).is_delete());
        assert!(!TextOp::Delete(3).is_retain());
        assert!(!TextOp::Delete(3).is_insert());
    }

    #[test]
    fn display_format() {
        assert_eq!(TextOp::Retain(5).to_string(), "=5");
        assert_eq!(TextOp::Delete(3).to_string(), "-3");
        let ins = TextOp::Insert(CompactString::from("hi"));
        assert_eq!(ins.to_string(), "+\"hi\"");
    }

    #[test]
    fn clone_and_eq() {
        let op = TextOp::Insert(CompactString::from("test"));
        let cloned = op.clone();
        assert_eq!(op, cloned);
    }

    #[test]
    fn different_variants_not_equal() {
        assert_ne!(TextOp::Retain(5), TextOp::Delete(5));
    }

    #[test]
    fn insert_utf8_multibyte() {
        let op = TextOp::Insert(CompactString::from("héllo 世界"));
        // "héllo 世界" = h(1) + é(2) + l(1) + l(1) + o(1) + space(1) + 世(3) + 界(3) = 13
        assert_eq!(op.len(), 13);
    }
}
