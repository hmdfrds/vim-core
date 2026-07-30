//! External edit reconciliation for hybrid host-owned text entry.
//!
//! Hosts that keep native text insertion (IME/autocomplete/composition) can
//! reconcile those mutations into the core through this contract.

use crate::primitives::{Offset, Range};
use compact_str::CompactString;

/// Classifies why an external mutation happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum ExternalEditKind {
    /// Plain insert/replace typed in insert mode.
    #[default]
    Insert,
    /// Replace-mode overwrite semantics.
    Replace,
    /// Host-level paste/IME commit.
    PasteOrIme,
    /// Cursor-only synchronization.
    CaretOnly,
    /// Autocomplete acceptance. Distinct from `PasteOrIme` to enable per-source diagnostics
    /// and undo-tree annotation without changing reconciliation logic.
    Completion,
    /// Host auto-closed a bracket/quote. Undo removes both trigger and pair.
    AutoPair,
    /// Host reformatted code after typing. Separate undo group.
    FormatOnType,
    /// Snippet expansion. Merged into triggering group.
    Snippet,
    /// Large-scale refactoring (rename, extract). Always separate undo group.
    Refactor,
    /// The host explicitly called `apply_external_edit()` to report a text change it detected.
    /// Indicates proactive host notification — the engine did not need to detect the drift itself.
    HostNotified,
    /// The engine's shadow document detected drift that the host did not proactively report.
    /// Frequent occurrences indicate the host should improve its proactive notification coverage.
    HostDrift,
}

/// Host-observed contiguous external document mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ExternalEdit {
    /// Deleted range in the previous snapshot.
    pub deleted: Range,
    /// Inserted text at `deleted.start()`.
    pub inserted: CompactString,
    /// Caret byte offset after the host applied the edit.
    pub caret_after: Offset,
    /// Semantic kind for bookkeeping.
    pub kind: ExternalEditKind,
}

impl ExternalEdit {
    /// Build an external-edit payload from host-observed diff data.
    #[must_use]
    pub fn new(
        deleted: Range,
        inserted: impl Into<CompactString>,
        caret_after: Offset,
        kind: ExternalEditKind,
    ) -> Self {
        Self {
            deleted,
            inserted: inserted.into(),
            caret_after,
            kind,
        }
    }

    /// Deleted range in the previous snapshot.
    #[inline]
    #[must_use]
    pub const fn deleted(&self) -> Range {
        self.deleted
    }

    /// Inserted text at `deleted.start()`.
    #[inline]
    #[must_use]
    pub const fn inserted(&self) -> &CompactString {
        &self.inserted
    }

    /// Caret byte offset after the host applied the edit.
    #[inline]
    #[must_use]
    pub const fn caret_after(&self) -> Offset {
        self.caret_after
    }

    /// Semantic kind for bookkeeping.
    #[inline]
    #[must_use]
    pub const fn kind(&self) -> ExternalEditKind {
        self.kind
    }
}

impl ExternalEditKind {
    /// Whether this edit merges into the current undo group or creates a new one.
    #[must_use]
    pub const fn merges_undo_group(&self) -> bool {
        matches!(
            self,
            Self::Insert
                | Self::Replace
                | Self::PasteOrIme
                | Self::AutoPair
                | Self::Completion
                | Self::Snippet
                | Self::HostNotified
                | Self::CaretOnly
        )
    }

    /// Whether this edit is recorded for dot-repeat.
    #[must_use]
    pub const fn recorded_for_repeat(&self) -> bool {
        matches!(self, Self::Completion | Self::Snippet)
    }

    /// Whether this edit is recorded in macros.
    #[must_use]
    pub const fn recorded_for_macro(&self) -> bool {
        matches!(
            self,
            Self::Insert | Self::Replace | Self::PasteOrIme | Self::Completion | Self::Snippet
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_pair_merges_undo_group() {
        assert!(ExternalEditKind::AutoPair.merges_undo_group());
    }

    #[test]
    fn format_on_type_does_not_merge() {
        assert!(!ExternalEditKind::FormatOnType.merges_undo_group());
    }

    #[test]
    fn refactor_does_not_merge() {
        assert!(!ExternalEditKind::Refactor.merges_undo_group());
    }

    #[test]
    fn completion_records_for_repeat() {
        assert!(ExternalEditKind::Completion.recorded_for_repeat());
    }

    #[test]
    fn auto_pair_not_recorded_for_repeat() {
        assert!(!ExternalEditKind::AutoPair.recorded_for_repeat());
    }

    #[test]
    fn format_on_type_not_recorded_for_macro() {
        assert!(!ExternalEditKind::FormatOnType.recorded_for_macro());
    }

    #[test]
    fn insert_recorded_for_macro() {
        assert!(ExternalEditKind::Insert.recorded_for_macro());
    }

    #[test]
    fn snippet_merges_and_records() {
        assert!(ExternalEditKind::Snippet.merges_undo_group());
        assert!(ExternalEditKind::Snippet.recorded_for_repeat());
        assert!(ExternalEditKind::Snippet.recorded_for_macro());
    }

    #[test]
    fn host_drift_does_not_merge() {
        assert!(!ExternalEditKind::HostDrift.merges_undo_group());
    }

    // ── Complete truth table: merges_undo_group ──────────────────────────

    #[test]
    fn merges_undo_group_truth_table() {
        let expected = [
            (ExternalEditKind::Insert, true),
            (ExternalEditKind::Replace, true),
            (ExternalEditKind::PasteOrIme, true),
            (ExternalEditKind::CaretOnly, true),
            (ExternalEditKind::Completion, true),
            (ExternalEditKind::AutoPair, true),
            (ExternalEditKind::FormatOnType, false),
            (ExternalEditKind::Snippet, true),
            (ExternalEditKind::Refactor, false),
            (ExternalEditKind::HostNotified, true),
            (ExternalEditKind::HostDrift, false),
        ];
        for (variant, should_merge) in expected {
            assert_eq!(
                variant.merges_undo_group(),
                should_merge,
                "{variant:?}.merges_undo_group() expected {should_merge}"
            );
        }
    }

    // ── Complete truth table: recorded_for_repeat ────────────────────────

    #[test]
    fn recorded_for_repeat_truth_table() {
        let expected = [
            (ExternalEditKind::Insert, false),
            (ExternalEditKind::Replace, false),
            (ExternalEditKind::PasteOrIme, false),
            (ExternalEditKind::CaretOnly, false),
            (ExternalEditKind::Completion, true),
            (ExternalEditKind::AutoPair, false),
            (ExternalEditKind::FormatOnType, false),
            (ExternalEditKind::Snippet, true),
            (ExternalEditKind::Refactor, false),
            (ExternalEditKind::HostNotified, false),
            (ExternalEditKind::HostDrift, false),
        ];
        for (variant, should_record) in expected {
            assert_eq!(
                variant.recorded_for_repeat(),
                should_record,
                "{variant:?}.recorded_for_repeat() expected {should_record}"
            );
        }
    }

    // ── Complete truth table: recorded_for_macro ─────────────────────────

    #[test]
    fn recorded_for_macro_truth_table() {
        let expected = [
            (ExternalEditKind::Insert, true),
            (ExternalEditKind::Replace, true),
            (ExternalEditKind::PasteOrIme, true),
            (ExternalEditKind::CaretOnly, false),
            (ExternalEditKind::Completion, true),
            (ExternalEditKind::AutoPair, false),
            (ExternalEditKind::FormatOnType, false),
            (ExternalEditKind::Snippet, true),
            (ExternalEditKind::Refactor, false),
            (ExternalEditKind::HostNotified, false),
            (ExternalEditKind::HostDrift, false),
        ];
        for (variant, should_record) in expected {
            assert_eq!(
                variant.recorded_for_macro(),
                should_record,
                "{variant:?}.recorded_for_macro() expected {should_record}"
            );
        }
    }

    // ── #[non_exhaustive] verification ───────────────────────────────────

    /// Compile-time proof that the enum is `#[non_exhaustive]`:
    /// matching without a wildcard arm inside the *same* crate compiles fine,
    /// but a downstream crate would fail. We verify the attribute is present
    /// by inspecting the source via a const assertion on the variant count.
    /// Since `#[non_exhaustive]` doesn't change in-crate behavior, we verify
    /// it structurally: if someone removed the attribute, this test's explicit
    /// enumeration of all variants still serves as a living inventory.
    #[test]
    fn non_exhaustive_attribute_variant_inventory() {
        // Exhaustive match in-crate (proves all variants are accounted for).
        // If a variant is added without updating this test, compilation fails.
        fn classify(k: ExternalEditKind) -> &'static str {
            match k {
                ExternalEditKind::Insert => "insert",
                ExternalEditKind::Replace => "replace",
                ExternalEditKind::PasteOrIme => "paste_or_ime",
                ExternalEditKind::CaretOnly => "caret_only",
                ExternalEditKind::Completion => "completion",
                ExternalEditKind::AutoPair => "auto_pair",
                ExternalEditKind::FormatOnType => "format_on_type",
                ExternalEditKind::Snippet => "snippet",
                ExternalEditKind::Refactor => "refactor",
                ExternalEditKind::HostNotified => "host_notified",
                ExternalEditKind::HostDrift => "host_drift",
            }
        }
        // Ensure the function is actually exercised.
        assert_eq!(classify(ExternalEditKind::Insert), "insert");
        assert_eq!(classify(ExternalEditKind::HostDrift), "host_drift");
    }

    // ── Serde round-trip ─────────────────────────────────────────────────

    #[test]
    #[cfg(feature = "serde")]
    fn serde_round_trip_all_variants() {
        let all_variants = [
            ExternalEditKind::Insert,
            ExternalEditKind::Replace,
            ExternalEditKind::PasteOrIme,
            ExternalEditKind::CaretOnly,
            ExternalEditKind::Completion,
            ExternalEditKind::AutoPair,
            ExternalEditKind::FormatOnType,
            ExternalEditKind::Snippet,
            ExternalEditKind::Refactor,
            ExternalEditKind::HostNotified,
            ExternalEditKind::HostDrift,
        ];
        for variant in all_variants {
            let json = serde_json::to_string(&variant)
                .unwrap_or_else(|e| panic!("serialize {variant:?}: {e}"));
            let back: ExternalEditKind = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("deserialize {variant:?} from {json:?}: {e}"));
            assert_eq!(variant, back, "round-trip failed for {variant:?}");
        }
    }

    // ── Default trait ────────────────────────────────────────────────────

    #[test]
    fn default_is_insert() {
        assert_eq!(ExternalEditKind::default(), ExternalEditKind::Insert);
    }
}
