//! Helper functions for text objects.
//!
//! **Thin re-export layer.** Canonical implementations live in `commands/helpers`.
//! This module exists for backwards compatibility so that `super::helpers::*`
//! imports within `textobjects/` continue to work.

pub use crate::commands::helpers::{
    char_at, class_at, is_blank_line, next_char_boundary, prev_char_boundary, CharClass,
};

// Offset-based line helpers — re-exported with original names for compatibility.
// The canonical names are `line_start_for_offset` / `line_end_for_offset` in
// `commands/helpers`, but textobjects consumers know them as `line_start` / `line_end`.
pub use crate::commands::helpers::line_end_for_offset as line_end;
pub use crate::commands::helpers::line_end_for_offset;
pub use crate::commands::helpers::line_start_for_offset as line_start;
pub use crate::commands::helpers::line_start_for_offset;

// Also re-export `current_line` for use by `is_blank_line` and direct consumers.
pub use crate::commands::helpers::current_line;
