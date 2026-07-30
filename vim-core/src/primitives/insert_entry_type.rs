//! Insert entry type — how insert mode was entered.
//!
//! A pure enum describing the method of insert mode entry. This type
//! lives at the `primitives` layer because it has zero internal
//! dependencies and is consumed by state, grammar, effects, commands,
//! and execution.

use smart_default::SmartDefault;

/// How insert mode was entered.
///
/// Each entry type determines initial cursor positioning and repeat behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SmartDefault)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum InsertEntryType {
    /// `i` - Insert before cursor
    #[default]
    BeforeCursor,
    /// `I` - Insert at first non-blank of line
    FirstNonBlank,
    /// `a` - Insert after cursor
    AfterCursor,
    /// `A` - Insert at end of line
    EndOfLine,
    /// `o` - Open new line below
    NewLineBelow,
    /// `O` - Open new line above
    NewLineAbove,
    /// `s` - Substitute character (delete char, then insert)
    SubstituteChar,
    /// `S` - Substitute line (delete line content, then insert)
    SubstituteLine,
    /// `c{motion}` - Change operator
    ChangeOperator,
    /// `gI` - Insert at column 0 (beginning of line, before any whitespace)
    Column0,
    /// `R` - Replace mode (overwrite characters in place)
    ReplaceMode,
}
