//! Rich diagnostic rendering for regex compilation errors.
//!
//! `DiagnosticRenderer` produces caret-annotated error output that shows
//! exactly which part of the pattern is problematic, with optional suggestions.

use crate::ir::{AuxiliarySpan, PercentEscapeContext, Span, VimRegexError, VimRegexErrorKind};

// ═══════════════════════════════════════════════════════════════════════════════
// DIAGNOSTIC RENDERER
// ═══════════════════════════════════════════════════════════════════════════════

/// Renders a `VimRegexError` with caret-annotated output.
///
/// Produces output like:
/// ```text
/// E69: Unknown POSIX class '[:alphaa:]' at byte 1
///   [[:alphaa:]]
///    ^^^^^^^^^
///   suggestion: did you mean '[:alpha:]'?
/// ```
///
/// This is separate from the `Display` impl on `VimRegexError` (which produces
/// a single-line message). `DiagnosticRenderer` is for rich, multi-line output
/// suitable for editor UI or terminal display.
pub struct DiagnosticRenderer<'p> {
    pattern: &'p str,
}

impl<'p> DiagnosticRenderer<'p> {
    /// Create a renderer for the given pattern.
    pub fn new(pattern: &'p str) -> Self {
        Self { pattern }
    }

    /// Render a full diagnostic for the given error.
    ///
    /// Returns a multi-line string with:
    /// 1. The error message (same as Display)
    /// 2. The pattern with caret annotation under the error span
    /// 3. Optional suggestion line
    pub fn render(&self, err: &VimRegexError) -> String {
        let mut output = err.to_string();
        output.push('\n');

        if let Some(span) = err.span() {
            self.render_caret_annotation(&mut output, span);
        }

        if let Some(suggestion) = self.compute_suggestion(&err.kind) {
            output.push_str("  suggestion: ");
            output.push_str(&suggestion);
            output.push('\n');
        }

        output
    }

    /// Render a diagnostic with an auxiliary span annotation.
    ///
    /// Produces output like:
    /// ```text
    /// E54: Unmatched \( or \) at bytes 10..12
    ///   foo\(bar\(baz\)
    ///       ^^
    ///   note: first opened here:
    ///   foo\(bar\(baz\)
    ///            ^^
    /// ```
    pub fn render_with_auxiliary(&self, err: &VimRegexError, aux: &AuxiliarySpan) -> String {
        let mut output = err.to_string();
        output.push('\n');

        // Primary span annotation
        self.render_caret_annotation(&mut output, &aux.primary);

        // Auxiliary span annotation
        output.push_str("  note: ");
        output.push_str(aux.auxiliary_label);
        output.push_str(":\n");
        self.render_caret_annotation(&mut output, &aux.auxiliary);

        // Suggestion (if any)
        if let Some(suggestion) = self.compute_suggestion(&err.kind) {
            output.push_str("  suggestion: ");
            output.push_str(&suggestion);
            output.push('\n');
        }

        output
    }

    /// Render the pattern line and caret annotation.
    fn render_caret_annotation(&self, output: &mut String, span: &Span) {
        let start = span.start.min(self.pattern.len());
        let end = span.end.min(self.pattern.len()).max(start);

        // Pattern line
        output.push_str("  ");
        output.push_str(self.pattern);
        output.push('\n');

        // Caret line: spaces up to start, then carets for the span
        output.push_str("  ");
        // Count display columns for bytes before the span
        let prefix = &self.pattern[..start];
        for _ in prefix.chars() {
            output.push(' ');
        }

        // Carets under the error region
        let caret_count = if end > start {
            // Count characters in the span for proper alignment
            let span_text = &self.pattern[start..end];
            span_text.chars().count()
        } else {
            1 // single-byte span: at least one caret
        };

        for _ in 0..caret_count.max(1) {
            output.push('^');
        }
        output.push('\n');
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SUGGESTION SYSTEM
// ═══════════════════════════════════════════════════════════════════════════════

/// All valid POSIX class names for Levenshtein matching.
const POSIX_CLASS_NAMES: &[&str] = &[
    "alnum",
    "alpha",
    "blank",
    "cntrl",
    "digit",
    "graph",
    "lower",
    "print",
    "punct",
    "space",
    "upper",
    "xdigit",
    "tab",
    "return",
    "backspace",
    "escape",
    "ident",
    "keyword",
    "fname",
];

impl DiagnosticRenderer<'_> {
    /// Compute a suggestion for the given error kind.
    ///
    /// Returns `None` if no suggestion is applicable.
    fn compute_suggestion(&self, kind: &VimRegexErrorKind) -> Option<String> {
        match kind {
            VimRegexErrorKind::UnknownPosixClass { name, .. } => self.suggest_posix_class(name),
            VimRegexErrorKind::InvalidEscape { ch, .. } if *ch == 'z' => {
                Some("did you mean '\\zs' (match start) or '\\ze' (match end)?".to_string())
            }
            VimRegexErrorKind::InvalidCollectionRange { .. } => {
                Some("swap the range endpoints, e.g., '[a-z]' instead of '[z-a]'".to_string())
            }
            VimRegexErrorKind::TrailingBackslash { .. } => {
                Some("add a character after '\\', or escape it as '\\\\'".to_string())
            }
            VimRegexErrorKind::InvalidPercentEscape { found, .. } => {
                self.suggest_percent_escape(*found)
            }
            VimRegexErrorKind::CharCodeOutOfRange { .. } => {
                Some("Unicode code points range from U+0000 to U+10FFFF".to_string())
            }
            VimRegexErrorKind::CharCodeSurrogate { value, .. } => Some(format!(
                "U+{value:04X} is a UTF-16 surrogate; use the actual Unicode code point instead"
            )),
            _ => None,
        }
    }

    /// Suggest a POSIX class name using Levenshtein distance.
    fn suggest_posix_class(&self, unknown: &str) -> Option<String> {
        let mut best: Option<(&str, usize)> = None;
        for &name in POSIX_CLASS_NAMES {
            let dist = levenshtein(unknown, name);
            if dist <= 2 && (best.is_none() || dist < best.unwrap().1) {
                best = Some((name, dist));
            }
        }
        best.map(|(name, _)| format!("did you mean '[:{name}:]'?"))
    }

    /// Suggest for `\%` escape errors.
    fn suggest_percent_escape(&self, found: PercentEscapeContext) -> Option<String> {
        match found {
            PercentEscapeContext::EndOfInput => {
                Some("\\% must be followed by a position spec, mark, or char code".to_string())
            }
            PercentEscapeContext::InvalidPositionSuffix => {
                Some("valid suffixes: 'l' (line), 'c' (column), 'v' (virtual column)".to_string())
            }
            PercentEscapeContext::MissingMarkChar => {
                Some("\\%'m requires a mark letter (a-z, A-Z) or special mark (<, >)".to_string())
            }
            _ => None,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// LEVENSHTEIN DISTANCE
// ═══════════════════════════════════════════════════════════════════════════════

/// Compute the Levenshtein edit distance between two strings.
///
/// Uses a single-row DP approach (O(min(m,n)) space).
fn levenshtein(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();

    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }

    // Use shorter string as the "column" for O(min(m,n)) space
    let (a_chars, b_chars, m, n) = if m <= n {
        (a_chars, b_chars, m, n)
    } else {
        (b_chars, a_chars, n, m)
    };

    let mut prev_row: Vec<usize> = (0..=m).collect();
    let mut curr_row: Vec<usize> = vec![0; m + 1];

    for j in 1..=n {
        curr_row[0] = j;
        for i in 1..=m {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            curr_row[i] = (prev_row[i] + 1) // deletion
                .min(curr_row[i - 1] + 1) // insertion
                .min(prev_row[i - 1] + cost); // substitution
        }
        std::mem::swap(&mut prev_row, &mut curr_row);
    }

    prev_row[m]
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VimRegex;

    #[test]
    fn levenshtein_identical() {
        assert_eq!(levenshtein("alpha", "alpha"), 0);
    }

    #[test]
    fn levenshtein_one_edit() {
        assert_eq!(levenshtein("alpha", "alphaa"), 1);
        assert_eq!(levenshtein("alpha", "alph"), 1);
        assert_eq!(levenshtein("alpha", "alpho"), 1);
    }

    #[test]
    fn levenshtein_two_edits() {
        assert_eq!(levenshtein("alpha", "alphoo"), 2);
    }

    #[test]
    fn levenshtein_empty() {
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
        assert_eq!(levenshtein("", ""), 0);
    }

    #[test]
    fn render_unknown_posix_with_suggestion() {
        let err = VimRegex::new("[[:alphaa:]]").unwrap_err();
        let renderer = DiagnosticRenderer::new("[[:alphaa:]]");
        let output = renderer.render(&err);
        assert!(output.contains("[:alphaa:]"), "output: {output}");
        assert!(output.contains('^'), "should have carets: {output}");
        assert!(
            output.contains("[:alpha:]"),
            "should suggest alpha: {output}"
        );
    }

    #[test]
    fn render_unknown_posix_no_suggestion_for_distant_name() {
        let err = VimRegex::new("[[:zzzzz:]]").unwrap_err();
        let renderer = DiagnosticRenderer::new("[[:zzzzz:]]");
        let output = renderer.render(&err);
        assert!(
            !output.contains("did you mean"),
            "should not suggest: {output}"
        );
    }

    #[test]
    fn render_trailing_backslash_suggestion() {
        let err = VimRegex::new(r"abc\").unwrap_err();
        let renderer = DiagnosticRenderer::new(r"abc\");
        let output = renderer.render(&err);
        assert!(output.contains("suggestion:"), "output: {output}");
        assert!(output.contains("escape"), "output: {output}");
    }

    #[test]
    fn render_reversed_range_suggestion() {
        let err = VimRegex::new("[z-a]").unwrap_err();
        let renderer = DiagnosticRenderer::new("[z-a]");
        let output = renderer.render(&err);
        assert!(output.contains("swap"), "output: {output}");
    }

    #[test]
    fn render_no_span_error_omits_carets() {
        let err = VimRegex::new("").unwrap_err();
        let renderer = DiagnosticRenderer::new("");
        let output = renderer.render(&err);
        // EmptyPattern has no span, so no caret line
        assert!(!output.contains('^'), "output: {output}");
    }

    #[test]
    fn caret_annotation_covers_multi_byte_span() {
        // \%' at end of input, span covers bytes 0..3 (\, %, ')
        let err = VimRegex::new(r"\%'").unwrap_err();
        let renderer = DiagnosticRenderer::new(r"\%'");
        let output = renderer.render(&err);
        // Should have carets under the whole \%' sequence
        assert!(output.contains("^^^"), "output: {output}");
    }

    #[test]
    fn render_percent_escape_end_of_input_suggestion() {
        let err = VimRegex::new(r"\%").unwrap_err();
        let renderer = DiagnosticRenderer::new(r"\%");
        let output = renderer.render(&err);
        assert!(output.contains("suggestion:"), "output: {output}");
        assert!(
            output.contains("position spec"),
            "should suggest position spec: {output}"
        );
    }

    #[test]
    fn render_percent_escape_invalid_suffix_suggestion() {
        let err = VimRegex::new(r"\%23z").unwrap_err();
        let renderer = DiagnosticRenderer::new(r"\%23z");
        let output = renderer.render(&err);
        assert!(output.contains("suggestion:"), "output: {output}");
        assert!(
            output.contains("valid suffixes"),
            "should list valid suffixes: {output}"
        );
    }

    #[test]
    fn render_invalid_escape_z_suggestion() {
        let err = VimRegex::new(r"\z").unwrap_err();
        let renderer = DiagnosticRenderer::new(r"\z");
        let output = renderer.render(&err);
        assert!(output.contains("suggestion:"), "output: {output}");
        assert!(output.contains("\\zs"), "should mention \\zs: {output}");
        assert!(output.contains("\\ze"), "should mention \\ze: {output}");
    }

    #[test]
    fn display_shows_byte_range_for_multichar_span() {
        // \%' at end of input: span covers \%' (3 chars, 3 bytes, span 0..3)
        let err = VimRegex::new(r"\%'").unwrap_err();
        let display = err.to_string();
        assert!(
            display.contains("bytes 0..3"),
            "display should show byte range: {display}"
        );
    }

    #[test]
    fn display_shows_single_byte_for_point_span() {
        let err = VimRegex::new(r"abc\").unwrap_err();
        let display = err.to_string();
        assert!(display.contains("byte 3"), "display: {display}");
        assert!(!display.contains("bytes"), "should be singular: {display}");
    }

    #[test]
    fn caret_alignment_with_multibyte_prefix() {
        // Pattern: "aaa\%'" — the \%' starts at byte 3
        // The caret should be under the \%' portion (3 chars wide)
        let err = VimRegex::new(r"aaa\%'").unwrap_err();
        let renderer = DiagnosticRenderer::new(r"aaa\%'");
        let output = renderer.render(&err);
        // Pattern line: "  aaa\%'"
        // Caret line:   "     ^^^" (3 spaces for "aaa", then 3 carets for \%')
        assert!(output.contains("   ^^^"), "output:\n{output}");
    }

    #[test]
    fn suggest_posix_class_one_edit() {
        // "alphz" -> distance 1 from "alpha"
        let renderer = DiagnosticRenderer::new("[[:alphz:]]");
        let suggestion = renderer.suggest_posix_class("alphz");
        assert_eq!(suggestion, Some("did you mean '[:alpha:]'?".to_string()));
    }

    #[test]
    fn suggest_posix_class_two_edits() {
        // "digt" -> distance 2 from "digit" (missing 'i')
        let renderer = DiagnosticRenderer::new("[[:digt:]]");
        let suggestion = renderer.suggest_posix_class("digt");
        assert_eq!(suggestion, Some("did you mean '[:digit:]'?".to_string()));
    }

    #[test]
    fn suggest_posix_class_exact_match() {
        // exact match -> distance 0, still suggests (valid class should not trigger
        // UnknownPosixClass, but testing the algorithm)
        let renderer = DiagnosticRenderer::new("[[:alpha:]]");
        let suggestion = renderer.suggest_posix_class("alpha");
        assert_eq!(suggestion, Some("did you mean '[:alpha:]'?".to_string()));
    }

    #[test]
    fn suggest_posix_class_no_match() {
        let renderer = DiagnosticRenderer::new("[[:zzzzzz:]]");
        let suggestion = renderer.suggest_posix_class("zzzzzz");
        assert_eq!(suggestion, None);
    }
}
