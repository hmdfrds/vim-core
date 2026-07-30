//! Clipboard metadata for intelligent paste behavior.

use compact_str::CompactString;

/// Metadata attached to clipboard/register content for intelligent paste behavior.
///
/// Carries contextual information about how text was copied, enabling
/// smarter paste operations (e.g., adjusting indentation, detecting
/// line-level copies from external editors).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ClipboardMetadata {
    /// Whether the copied text represents an entire line (including newline).
    pub is_entire_line: bool,
    /// The indentation level (in columns) of the first line when it was copied.
    pub first_line_indent: u32,
    /// The file path the text was copied from, if known.
    pub source_path: Option<CompactString>,
}
