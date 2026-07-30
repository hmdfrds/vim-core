use compact_str::CompactString;

use crate::primitives::{Offset, Range, SubFlags};

/// A single match in a substitute confirm session.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ConfirmMatchPayload {
    /// Document byte range of the match.
    pub range: Range,
    /// Line index (0-indexed).
    pub line_idx: usize,
    /// Byte offset of the line start.
    pub line_start: Offset,
    /// Full line text (pre-replacement).
    pub line_text: CompactString,
}

/// Initialization data for a `:s///c` confirm session.
///
/// Carried by `Effect::SetSubstituteConfirmState` so the effects layer
/// does not depend on `crate::state`. The effect processor converts this
/// to a full `SubstituteConfirmState` during processing.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SubstituteConfirmPayload {
    /// All match ranges.
    pub matches: Vec<ConfirmMatchPayload>,
    /// Replacement string.
    pub replacement: CompactString,
    /// Compiled pattern string form.
    pub pattern: CompactString,
    /// Flags from the original `:s` command.
    pub flags: SubFlags,
    /// Whether gdefault was active.
    pub gdefault: bool,
}
