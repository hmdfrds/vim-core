//! Display line provider trait for shell integration.
//!
//! Shells that support soft-wrap implement this trait to let the engine
//! navigate display lines (gj/gk/g0/g$/g^) rather than physical lines.

/// Provider for display-line (soft-wrap) navigation.
///
/// Methods receive the physical line's text (`&str`, no trailing newline)
/// and return line-relative values. The caller handles document↔line
/// conversion.
///
/// If no `DisplayLineProvider` is supplied, display-line motions fall
/// back to their physical-line equivalents.
pub trait DisplayLineProvider: Send {
    /// How many display sub-lines does this line occupy?
    ///
    /// Returns 1 for unwrapped lines, >1 for wrapped lines.
    fn display_line_count(&self, line_text: &str) -> usize;

    /// Convert display coordinates to byte offset within the line.
    ///
    /// - `sub_line`: 0-based sub-line index within the wrapped line
    /// - `col`: 0-based grapheme column within the sub-line
    ///
    /// Returns byte offset relative to line start, or `None` if out of bounds.
    fn display_col_to_byte(&self, line_text: &str, sub_line: usize, col: usize) -> Option<usize>;

    /// Convert byte offset within the line to display coordinates.
    ///
    /// Returns `(sub_line, grapheme_col)` or `None` if `byte_offset` is
    /// out of bounds.
    fn byte_to_display_col(&self, line_text: &str, byte_offset: usize) -> Option<(usize, usize)>;
}
