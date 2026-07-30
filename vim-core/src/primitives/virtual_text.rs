//! Virtual text and diagnostic types.
//!
//! Types for inlay hints, ghost text, and diagnostic overlays that the
//! host renders alongside the buffer text. These are pure data definitions
//! with no behavior logic — the host interprets them via the effect system.

use compact_str::CompactString;

use super::position::{LineNumber, Offset};

/// Position of virtual text relative to the buffer line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum VirtualTextPosition {
    /// After the end of the line.
    Eol,
    /// Between characters (at the specified column).
    Inline,
    /// Overlaid on top of existing text.
    Overlay,
    /// Right-aligned in the window.
    RightAlign,
}

/// Severity level for diagnostics.
///
/// Ordered by severity via a manual `Ord` impl: `Error > Warning > Info > Hint`.
/// The standard comparison operators (`>`, `<`) reflect semantic severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DiagnosticSeverity {
    /// Error (highest severity).
    Error,
    /// Warning.
    Warning,
    /// Informational.
    Info,
    /// Hint (lowest severity).
    Hint,
}

impl DiagnosticSeverity {
    /// Returns a numeric priority where higher means more severe.
    ///
    /// `Error` = 3, `Warning` = 2, `Info` = 1, `Hint` = 0.
    #[inline]
    #[must_use]
    pub const fn priority(self) -> u8 {
        match self {
            Self::Error => 3,
            Self::Warning => 2,
            Self::Info => 1,
            Self::Hint => 0,
        }
    }

    /// Returns `true` if `self` is more severe than `other`.
    #[inline]
    #[must_use]
    pub const fn is_more_severe_than(self, other: Self) -> bool {
        self.priority() > other.priority()
    }
}

impl PartialOrd for DiagnosticSeverity {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DiagnosticSeverity {
    /// Ordered by severity: `Error > Warning > Info > Hint`.
    #[inline]
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.priority().cmp(&other.priority())
    }
}

/// A single diagnostic entry (error, warning, etc.).
///
/// Diagnostics are produced by external tools (language servers, linters)
/// and forwarded through the engine's effect system to the host for
/// rendering. The engine itself does not interpret diagnostic content.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Diagnostic {
    /// 0-indexed line number.
    pub line: LineNumber,
    /// Byte offset of diagnostic start within the line.
    pub col: Offset,
    /// Byte offset of diagnostic end (`None` = point diagnostic).
    pub end_col: Option<Offset>,
    /// Severity level.
    pub severity: DiagnosticSeverity,
    /// Human-readable diagnostic message.
    pub message: CompactString,
    /// Source of the diagnostic (e.g., "gdscript", "rust-analyzer").
    pub source: Option<CompactString>,
}

impl Diagnostic {
    /// Create a new diagnostic with required fields.
    ///
    /// Sets `end_col` and `source` to `None` (point diagnostic, no source).
    /// Use struct update syntax to override optional fields:
    ///
    /// ```ignore
    /// Diagnostic::new(range_start, col, severity, "message")
    ///     .with_end_col(end)
    ///     .with_source("rust-analyzer");
    /// // — or —
    /// Diagnostic { end_col: Some(end), source: Some("ra".into()), ..Diagnostic::new(..) }
    /// ```
    #[must_use]
    pub fn new(
        line: LineNumber,
        col: Offset,
        severity: DiagnosticSeverity,
        message: impl Into<CompactString>,
    ) -> Self {
        Self {
            line,
            col,
            end_col: None,
            severity,
            message: message.into(),
            source: None,
        }
    }

    /// Returns the human-readable diagnostic message.
    #[inline]
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── VirtualTextPosition ───────────────────────────────────────────

    #[test]
    fn virtual_text_position_construction() {
        let positions = [
            VirtualTextPosition::Eol,
            VirtualTextPosition::Inline,
            VirtualTextPosition::Overlay,
            VirtualTextPosition::RightAlign,
        ];
        // All four variants are distinct.
        for (i, a) in positions.iter().enumerate() {
            for (j, b) in positions.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }

    #[test]
    fn virtual_text_position_debug_and_clone() {
        let pos = VirtualTextPosition::Inline;
        let cloned = pos;
        assert_eq!(pos, cloned);
        // Debug impl exists
        let _ = format!("{pos:?}");
    }

    #[test]
    fn virtual_text_position_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(VirtualTextPosition::Eol);
        set.insert(VirtualTextPosition::Inline);
        set.insert(VirtualTextPosition::Overlay);
        set.insert(VirtualTextPosition::RightAlign);
        assert_eq!(set.len(), 4);
    }

    // ── DiagnosticSeverity ────────────────────────────────────────────

    #[test]
    fn diagnostic_severity_ordering_error_is_highest() {
        assert!(DiagnosticSeverity::Error > DiagnosticSeverity::Warning);
        assert!(DiagnosticSeverity::Warning > DiagnosticSeverity::Info);
        assert!(DiagnosticSeverity::Info > DiagnosticSeverity::Hint);
    }

    #[test]
    fn diagnostic_severity_ordering_transitive() {
        assert!(DiagnosticSeverity::Error > DiagnosticSeverity::Hint);
        assert!(DiagnosticSeverity::Error > DiagnosticSeverity::Info);
        assert!(DiagnosticSeverity::Warning > DiagnosticSeverity::Hint);
    }

    #[test]
    fn diagnostic_severity_equality() {
        assert_eq!(DiagnosticSeverity::Error, DiagnosticSeverity::Error);
        assert_eq!(DiagnosticSeverity::Warning, DiagnosticSeverity::Warning);
        assert_eq!(DiagnosticSeverity::Info, DiagnosticSeverity::Info);
        assert_eq!(DiagnosticSeverity::Hint, DiagnosticSeverity::Hint);
    }

    #[test]
    fn diagnostic_severity_is_more_severe_than() {
        assert!(DiagnosticSeverity::Error.is_more_severe_than(DiagnosticSeverity::Warning));
        assert!(DiagnosticSeverity::Error.is_more_severe_than(DiagnosticSeverity::Info));
        assert!(DiagnosticSeverity::Error.is_more_severe_than(DiagnosticSeverity::Hint));
        assert!(DiagnosticSeverity::Warning.is_more_severe_than(DiagnosticSeverity::Info));
        assert!(DiagnosticSeverity::Warning.is_more_severe_than(DiagnosticSeverity::Hint));
        assert!(DiagnosticSeverity::Info.is_more_severe_than(DiagnosticSeverity::Hint));
        assert!(!DiagnosticSeverity::Hint.is_more_severe_than(DiagnosticSeverity::Error));
        assert!(!DiagnosticSeverity::Error.is_more_severe_than(DiagnosticSeverity::Error));
    }

    #[test]
    fn diagnostic_severity_priority_values() {
        assert_eq!(DiagnosticSeverity::Error.priority(), 3);
        assert_eq!(DiagnosticSeverity::Warning.priority(), 2);
        assert_eq!(DiagnosticSeverity::Info.priority(), 1);
        assert_eq!(DiagnosticSeverity::Hint.priority(), 0);
    }

    #[test]
    fn diagnostic_severity_sort_produces_ascending_order() {
        let mut severities = vec![
            DiagnosticSeverity::Hint,
            DiagnosticSeverity::Error,
            DiagnosticSeverity::Info,
            DiagnosticSeverity::Warning,
        ];
        severities.sort();
        assert_eq!(
            severities,
            vec![
                DiagnosticSeverity::Hint,
                DiagnosticSeverity::Info,
                DiagnosticSeverity::Warning,
                DiagnosticSeverity::Error,
            ]
        );
    }

    #[test]
    fn diagnostic_severity_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(DiagnosticSeverity::Error);
        set.insert(DiagnosticSeverity::Warning);
        set.insert(DiagnosticSeverity::Info);
        set.insert(DiagnosticSeverity::Hint);
        assert_eq!(set.len(), 4);
    }

    // ── Diagnostic ────────────────────────────────────────────────────

    #[test]
    fn diagnostic_construction_all_fields() {
        let diag = Diagnostic {
            line: LineNumber::new(10),
            col: Offset::new(5),
            end_col: Some(Offset::new(15)),
            severity: DiagnosticSeverity::Error,
            message: CompactString::from("undefined variable"),
            source: Some(CompactString::from("rust-analyzer")),
        };
        assert_eq!(diag.line, LineNumber::new(10));
        assert_eq!(diag.col, Offset::new(5));
        assert_eq!(diag.end_col, Some(Offset::new(15)));
        assert_eq!(diag.severity, DiagnosticSeverity::Error);
        assert_eq!(diag.message.as_str(), "undefined variable");
        assert_eq!(diag.source.as_deref(), Some("rust-analyzer"));
    }

    #[test]
    fn diagnostic_construction_point_diagnostic() {
        let diag = Diagnostic {
            line: LineNumber::new(0),
            col: Offset::new(0),
            end_col: None,
            severity: DiagnosticSeverity::Hint,
            message: CompactString::from("consider using let"),
            source: None,
        };
        assert_eq!(diag.end_col, None);
        assert_eq!(diag.source, None);
    }

    #[test]
    fn diagnostic_equality() {
        let a = Diagnostic {
            line: LineNumber::new(1),
            col: Offset::new(0),
            end_col: None,
            severity: DiagnosticSeverity::Warning,
            message: CompactString::from("unused"),
            source: None,
        };
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn diagnostic_inequality_different_severity() {
        let a = Diagnostic {
            line: LineNumber::new(1),
            col: Offset::new(0),
            end_col: None,
            severity: DiagnosticSeverity::Warning,
            message: CompactString::from("unused"),
            source: None,
        };
        let mut b = a.clone();
        b.severity = DiagnosticSeverity::Error;
        assert_ne!(a, b);
    }

    #[test]
    fn diagnostic_debug_format() {
        let diag = Diagnostic {
            line: LineNumber::new(0),
            col: Offset::new(0),
            end_col: None,
            severity: DiagnosticSeverity::Info,
            message: CompactString::from("info"),
            source: None,
        };
        let debug = format!("{diag:?}");
        assert!(debug.contains("Diagnostic"));
        assert!(debug.contains("Info"));
    }
}
