//! Indent provider trait for shell integration, and indent auto-detection.
//!
//! Shells that support language-aware indentation implement [`IndentProvider`]
//! to let the engine compute correct indent for new lines (`o`/`O`, Enter).
//!
//! The [`detect_indent_style`] function analyses raw document text and returns
//! the dominant [`IndentStyle`] — useful for "respect the file's own style"
//! behaviour without requiring a language server.

use crate::primitives::LineNumber;

/// Provider for language-aware indentation.
///
/// Shells implement this to expose their indentation logic to the engine.
/// The engine uses this for `o`/`O` commands and Enter in insert mode.
///
/// If no `IndentProvider` is supplied, the engine falls back to copying
/// the previous line's leading whitespace (basic autoindent).
///
/// # External implementors
///
/// External crates (e.g. `godot-vim`'s indent provider) will need to
/// update their signatures to use `LineNumber` instead of bare `usize`.
pub trait IndentProvider: Send {
    /// Compute the indent for a new line opened after `line`.
    ///
    /// Returns an [`IndentResult`] describing the indent string, optional
    /// append text (e.g. `* ` for JSDoc continuations), and whether a
    /// second newline is needed (e.g. Enter between `{}`).
    ///
    /// `line` is a 0-based line index into the document.
    fn indent_for_new_line(&self, line: LineNumber) -> IndentResult;
}

/// A simple indent provider that stores a pre-computed indent string for
/// the next new line.
///
/// The host pushes this before each `processKey` call so that `o`/`O` (and
/// Enter in insert mode) produce correct language-aware indentation without
/// the engine needing a language server.
///
/// The host computes the indent from its own language intelligence
/// (e.g., on-enter rules, tree-sitter, etc.) and caches it here. The
/// engine retrieves it via [`IndentProvider::indent_for_new_line`].
pub struct CachedIndentHint {
    indent: compact_str::CompactString,
}

impl CachedIndentHint {
    /// Create a new cached indent hint with the given indent string.
    ///
    /// The string should be the full leading whitespace (spaces and/or tabs)
    /// that will prefix the new line.
    #[must_use]
    pub fn new(indent: &str) -> Self {
        Self {
            indent: indent.into(),
        }
    }
}

impl IndentProvider for CachedIndentHint {
    fn indent_for_new_line(&self, _line: LineNumber) -> IndentResult {
        IndentResult::Simple {
            indent: self.indent.clone(),
            append: None,
        }
    }
}

/// Cached indent action -- stores a full `IndentResult` for the host to push
/// before each `processKey`. More expressive than `CachedIndentHint` (which only
/// stores a bare indent string).
pub struct CachedIndentAction {
    result: IndentResult,
}

impl CachedIndentAction {
    /// Create a simple indent action (indent + optional append text).
    #[must_use]
    pub fn simple(indent: &str, append: Option<&str>) -> Self {
        Self {
            result: IndentResult::Simple {
                indent: indent.into(),
                append: append.map(Into::into),
            },
        }
    }

    /// Create an indent-outdent action (two newlines for bracket pairs).
    #[must_use]
    pub fn indent_outdent(indent: &str, append: Option<&str>, closing_indent: &str) -> Self {
        Self {
            result: IndentResult::IndentOutdent {
                indent: indent.into(),
                append: append.map(Into::into),
                closing_indent: closing_indent.into(),
            },
        }
    }
}

impl IndentProvider for CachedIndentAction {
    fn indent_for_new_line(&self, _line: LineNumber) -> IndentResult {
        self.result.clone()
    }
}

/// Detected indentation style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentStyle {
    /// Tab characters for indentation.
    Tabs,
    /// N spaces per indent level (1..=16).
    Spaces(u8),
}

/// Result of host-side indent computation for a new line.
#[derive(Debug, Clone)]
pub enum IndentResult {
    /// Single newline: `\n` + indent + optional append text.
    Simple {
        /// The whitespace prefix for the new line.
        indent: compact_str::CompactString,
        /// Optional text appended after indent (e.g. `* ` for JSDoc).
        append: Option<compact_str::CompactString>,
    },
    /// Two newlines for bracket pairs (e.g., Enter between `{}`).
    IndentOutdent {
        /// The whitespace prefix for the first (cursor) line.
        indent: compact_str::CompactString,
        /// Optional text appended after indent on the cursor line.
        append: Option<compact_str::CompactString>,
        /// The whitespace prefix for the closing-bracket line.
        closing_indent: compact_str::CompactString,
    },
}

impl IndentResult {
    /// Total byte length of indent + append for cursor positioning.
    #[must_use]
    pub fn cursor_indent_len(&self) -> usize {
        match self {
            Self::Simple { indent, append } | Self::IndentOutdent { indent, append, .. } => {
                indent.len() + append.as_ref().map_or(0, compact_str::CompactString::len)
            }
        }
    }

    /// The indent string (without append).
    #[must_use]
    pub fn indent(&self) -> &str {
        match self {
            Self::Simple { indent, .. } | Self::IndentOutdent { indent, .. } => indent,
        }
    }

    /// Just the indent length (for auto_indent_len tracking).
    #[must_use]
    pub fn indent_len(&self) -> usize {
        match self {
            Self::Simple { indent, .. } | Self::IndentOutdent { indent, .. } => indent.len(),
        }
    }
}

/// Detect the dominant indent style from document text.
///
/// Scans up to `max_lines` non-blank lines, building a histogram of
/// indentation *increases* between consecutive lines. Returns `None`
/// if confidence is too low.
///
/// # Algorithm
///
/// 1. Walk lines, skip blanks.
/// 2. For each consecutive non-blank pair, compute indent increase.
/// 3. Histogram: index 0 = tabs, indices 1-16 = N spaces.
/// 4. Weighting: tabs 2x (strong signal), single-space 0.5x (noisy).
/// 5. Winner must have > 1.5x the runner-up's score. Otherwise `None`.
///
/// # Complexity
///
/// O(min(max_lines, line_count) * avg_line_length) — single pass, zero allocation.
///
/// Note: takes `&str` matching the `Document` trait's `text() -> &str` contract.
// Histogram indices guarded by <= 16.
#[allow(clippy::indexing_slicing)]
#[must_use]
pub fn detect_indent_style(text: &str, max_lines: usize) -> Option<IndentStyle> {
    // Helper: a run of `n` tab-indent levels, weighted 2x, in half-units.
    fn weigh_tabs(n: usize) -> u32 {
        u32::try_from(n).unwrap_or(u32::MAX).saturating_mul(4)
    }
    // Helper: one space-indent event of width `n`, in half-units. A single
    // space is a noisy signal and counts half as much as any other width.
    const fn weigh_spaces(n: usize) -> u32 {
        if n == 1 {
            1
        } else {
            2
        }
    }
    // Helper: count leading tabs and spaces for a line.
    // Returns (tab_count, space_count) of the leading whitespace.
    fn leading_indent(line: &str) -> (usize, usize) {
        let mut tabs = 0usize;
        let mut spaces = 0usize;
        for ch in line.chars() {
            match ch {
                '\t' => {
                    if spaces == 0 {
                        tabs += 1;
                    } else {
                        break;
                    }
                }
                ' ' => {
                    if tabs == 0 {
                        spaces += 1;
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }
        (tabs, spaces)
    }

    // histogram[0] = tabs, histogram[1..=16] = spaces count.
    //
    // Scores are kept in *half-units* (1 == 0.5 of a vote) so the weighting
    // scheme — tabs 2x, ordinary spaces 1x, single-space 0.5x — is exact
    // integer arithmetic instead of accumulated `f32`.
    let mut histogram = [0_u32; 17];

    let mut prev_tabs: usize = 0;
    let mut prev_spaces: usize = 0;
    let mut prev_indent_kind: Option<bool> = None; // true = tabs, false = spaces
    let mut lines_seen: usize = 0;

    for line in text.lines() {
        if lines_seen >= max_lines {
            break;
        }
        // Skip blank lines.
        if line.trim().is_empty() {
            continue;
        }
        lines_seen += 1;

        let (tabs, spaces) = leading_indent(line);

        if let Some(was_tabs) = prev_indent_kind {
            if tabs > 0 && tabs > prev_tabs && spaces == 0 {
                // Tab increase detected.
                let increase = tabs - prev_tabs;
                // Weight tabs 2x.
                histogram[0] = histogram[0].saturating_add(weigh_tabs(increase));
            } else if spaces > 0 && spaces > prev_spaces && tabs == 0 && !was_tabs {
                // Space increase detected.
                let increase = spaces - prev_spaces;
                if increase <= 16 {
                    histogram[increase] =
                        histogram[increase].saturating_add(weigh_spaces(increase));
                }
            } else if tabs > 0 && prev_tabs == 0 && spaces == 0 {
                // Switched to tabs from a zero-indented line.
                histogram[0] = histogram[0].saturating_add(weigh_tabs(tabs));
            } else if spaces > 0 && prev_spaces == 0 && tabs == 0 {
                // Switched to spaces from a zero-indented line.
                let increase = spaces;
                if increase <= 16 {
                    histogram[increase] =
                        histogram[increase].saturating_add(weigh_spaces(increase));
                }
            }
        } else {
            // First non-blank line: record initial indent as a weak signal
            // only if it is > 0.
            if tabs > 0 && spaces == 0 {
                histogram[0] = histogram[0].saturating_add(weigh_tabs(tabs));
            } else if spaces > 0 && tabs == 0 && spaces <= 16 {
                histogram[spaces] = histogram[spaces].saturating_add(weigh_spaces(spaces));
            }
        }

        // Record indent kind for this line.
        if tabs > 0 && spaces == 0 {
            prev_indent_kind = Some(true);
            prev_tabs = tabs;
            prev_spaces = 0;
        } else if spaces > 0 && tabs == 0 {
            prev_indent_kind = Some(false);
            prev_tabs = 0;
            prev_spaces = spaces;
        } else {
            // Unindented line.
            prev_indent_kind = Some(false); // treat as neutral
            prev_tabs = 0;
            prev_spaces = 0;
        }
    }

    // Find winner and runner-up.
    let mut winner_idx: usize = 0;
    let mut winner_score: u32 = 0;
    let mut runner_up_score: u32 = 0;

    for (i, &score) in histogram.iter().enumerate() {
        if score > winner_score {
            runner_up_score = winner_score;
            winner_score = score;
            winner_idx = i;
        } else if score > runner_up_score {
            runner_up_score = score;
        }
    }

    // No signal at all.
    if winner_score == 0 {
        return None;
    }

    // Winner must dominate runner-up by > 1.5x. Both scores are in half-units,
    // so `winner > 1.5 * runner` is exactly `2 * winner > 3 * runner`.
    if winner_score.saturating_mul(2) <= runner_up_score.saturating_mul(3) {
        return None;
    }

    if winner_idx == 0 {
        Some(IndentStyle::Tabs)
    } else {
        // winner_idx is in 1..=16, so the conversion cannot fail; propagating
        // the failure through the existing `Option` keeps the function total.
        u8::try_from(winner_idx).ok().map(IndentStyle::Spaces)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── helpers ──────────────────────────────────────────────────────────────

    fn tabs_file() -> &'static str {
        "fn foo() {\n\tlet x = 1;\n\tif x > 0 {\n\t\tlet y = 2;\n\t\tlet z = 3;\n\t}\n}\n"
    }

    fn spaces4_file() -> &'static str {
        "fn foo() {\n    let x = 1;\n    if x > 0 {\n        let y = 2;\n        let z = 3;\n    }\n}\n"
    }

    fn spaces2_file() -> &'static str {
        "fn foo() {\n  let x = 1;\n  if x > 0 {\n    let y = 2;\n    let z = 3;\n  }\n}\n"
    }

    // ── tests ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_all_tabs() {
        assert_eq!(
            detect_indent_style(tabs_file(), 100),
            Some(IndentStyle::Tabs)
        );
    }

    #[test]
    fn test_all_4_spaces() {
        assert_eq!(
            detect_indent_style(spaces4_file(), 100),
            Some(IndentStyle::Spaces(4))
        );
    }

    #[test]
    fn test_all_2_spaces() {
        assert_eq!(
            detect_indent_style(spaces2_file(), 100),
            Some(IndentStyle::Spaces(2))
        );
    }

    #[test]
    fn test_mixed_tabs_dominant() {
        // 6 tab-indented lines, 2 space-indented lines → tabs win.
        let text = concat!(
            "fn foo() {\n",
            "\tlet a = 1;\n",
            "\tlet b = 2;\n",
            "\tlet c = 3;\n",
            "\tif a > 0 {\n",
            "\t\tlet d = 4;\n",
            "\t\tlet e = 5;\n",
            "}\n",
            "fn bar() {\n",
            "  let x = 1;\n",
            "  let y = 2;\n",
            "}\n",
        );
        assert_eq!(detect_indent_style(text, 100), Some(IndentStyle::Tabs));
    }

    #[test]
    fn test_no_indentation_returns_none() {
        let text = "hello\nworld\nfoo\nbar\nbaz\n";
        assert_eq!(detect_indent_style(text, 100), None);
    }

    #[test]
    fn test_single_line_returns_none() {
        assert_eq!(detect_indent_style("hello world\n", 100), None);
    }

    #[test]
    fn test_empty_string_returns_none() {
        assert_eq!(detect_indent_style("", 100), None);
    }

    #[test]
    fn test_ambiguous_50_50_returns_none() {
        // Tabs get 2x weight per transition event. To get an exactly equal
        // score we need the space side to have twice as many depth-increase
        // events. Here we construct: 2 tab transition events (score 4.0) vs
        // 4 space-of-4 transition events (score 4.0).
        //
        // Tab transitions: 0→1 (score 2) and 0→1 again in a second block (score 2) = 4.0
        // Space transitions: 0→4, 4→8, 0→4, 4→8 (score 1 each) = 4.0
        let text = concat!(
            "fn a() {\n",
            "\tx = 1;\n", // 0→1 tab: score[0] += 2
            "}\n",
            "fn b() {\n",
            "\tx = 2;\n", // 0→1 tab: score[0] += 2
            "}\n",
            "fn c() {\n",
            "    x = 1;\n",     // 0→4 spaces: score[4] += 1
            "        x = 2;\n", // 4→8 spaces: score[4] += 1
            "}\n",
            "fn d() {\n",
            "    x = 1;\n",     // 0→4 spaces: score[4] += 1
            "        x = 2;\n", // 4→8 spaces: score[4] += 1
            "}\n",
        );
        assert_eq!(detect_indent_style(text, 100), None);
    }

    #[test]
    fn test_respects_max_lines() {
        // The first 4 non-blank lines are tab-indented; after that all spaces.
        // With max_lines=4 we should see tabs; with max_lines=100 ambiguous.
        let text = concat!(
            "fn a() {\n",
            "\tlet x = 1;\n",
            "\tlet y = 2;\n",
            "\tlet z = 3;\n",
            "}\n",
            "fn b() {\n",
            "    let x = 1;\n",
            "    let y = 2;\n",
            "    let z = 3;\n",
            "}\n",
        );
        // With only 4 lines, we only see the tab section.
        assert_eq!(detect_indent_style(text, 4), Some(IndentStyle::Tabs));
    }

    #[test]
    fn test_blank_lines_skipped() {
        // Blank lines interspersed should not affect the result.
        let text = concat!(
            "fn foo() {\n",
            "\n",
            "    let x = 1;\n",
            "\n",
            "    let y = 2;\n",
            "\n",
            "        let z = 3;\n",
            "\n",
            "}\n",
        );
        assert_eq!(detect_indent_style(text, 100), Some(IndentStyle::Spaces(4)));
    }
}
