//! vim-text: Persistent B+ tree text buffer with O(log n) operations.

pub mod bloom;
pub mod changeset;
pub mod chunk;
pub mod diff;
pub mod edit_batch;
mod iter;
pub mod queries;
pub mod regex_adapter;
mod rope_slice;
pub mod summary;
pub(crate) mod tree;
mod vim_text;

// Re-exports
pub use bloom::BloomFilter;
pub use changeset::{Assoc, Change, ChangeSet, Op, OverlapError};
pub use chunk::{chunk_text, TextChunk, CHUNK_MAX_BYTES, CHUNK_MIN_BYTES};
pub use diff::{DiffHunk, DiffKind};
pub use edit_batch::EditBatch;
pub use iter::{Bytes, Chars, Chunks, Lines};
pub use queries::{TextSearch, VimQueries};
pub use regex_adapter::{RopeCursor, VimTextCursor};
pub use rope_slice::{RopeSlice, SliceChars, SliceChunks, SliceLines};
pub use summary::{
    BracketPair, BracketSummary, ByteOffset, CharOffset, IndentFlags, IndentSummary, LineOffset,
    LineSummary, MetricsSummary, SummaryFlags, TextSummary, Utf16Offset,
};
pub use vim_text::{Position, SummaryCompat, SummaryFlagsCompat};

use tree::SumTree;

// Re-export SummaryFlags as alias in the summary module for backward compatibility.
// Old code used `vim_text::summary::SummaryFlags`, new code uses `IndentFlags`.

/// Persistent text buffer backed by a B+ tree with Arc-based COW.
/// Clone is O(1) (Arc refcount bump). Mutations use ChangeSet.
#[derive(Clone)]
pub struct VimText {
    pub(crate) tree: SumTree<TextChunk>,
}

impl VimText {
    pub fn new() -> Self {
        Self {
            tree: SumTree::from_item(TextChunk::new("")),
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        if s.is_empty() {
            return Self::new();
        }
        let chunks = chunk_text(s);
        Self {
            tree: SumTree::from_items(chunks),
        }
    }
}

impl Default for VimText {
    fn default() -> Self {
        Self::new()
    }
}

impl From<&str> for VimText {
    fn from(s: &str) -> Self {
        Self::from_str(s)
    }
}

impl From<String> for VimText {
    fn from(s: String) -> Self {
        Self::from_str(&s)
    }
}

impl PartialEq for VimText {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(self.tree.root(), other.tree.root())
            || self.to_string() == other.to_string()
    }
}

impl Eq for VimText {}

impl std::fmt::Display for VimText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut cursor = self.tree.cursor::<ByteOffset>();
        cursor.seek(&ByteOffset(0), tree::Bias::Right);
        loop {
            if let Some(chunk) = cursor.item() {
                f.write_str(chunk.as_str())?;
            }
            if !cursor.next() {
                break;
            }
        }
        Ok(())
    }
}

impl std::fmt::Debug for VimText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = self.to_string();
        if text.len() > 80 {
            write!(f, "VimText({:?}...{} bytes)", &text[..40], text.len())
        } else {
            write!(f, "VimText({:?})", text)
        }
    }
}

#[cfg(test)]
mod static_checks {
    use super::*;

    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}

    #[test]
    fn vim_text_is_send_sync() {
        assert_send::<VimText>();
        assert_sync::<VimText>();
    }

    #[test]
    fn changeset_is_send_sync() {
        assert_send::<ChangeSet>();
        assert_sync::<ChangeSet>();
    }

    #[test]
    fn edit_batch_is_send_sync() {
        assert_send::<EditBatch>();
        assert_sync::<EditBatch>();
    }

    #[test]
    fn rope_slice_is_send_sync() {
        assert_send::<RopeSlice<'static>>();
        assert_sync::<RopeSlice<'static>>();
    }
}
