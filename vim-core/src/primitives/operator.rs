//! Operator types — pure Vim domain concepts.
//!
//! Operators act on a motion or text object. This type lives at the
//! `primitives` layer because it has zero internal dependencies and
//! is consumed by every layer (state, grammar, effects, commands, execution).

use strum::Display;

/// Vim operators.
///
/// Operators act on a motion or text object.
///
/// This type is always `Copy`. When the `composed-operators` feature is
/// enabled, the `Composed` variant stores two sub-operators using a
/// compact `Copy`-safe encoding (`ComposedPair`) rather than heap allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Operator {
    /// Delete (d)
    Delete,
    /// Change (c)
    Change,
    /// Yank (y)
    Yank,
    /// Indent right (>)
    Indent,
    /// Indent left (<)
    Outdent,
    /// Toggle case (g~)
    ToggleCase,
    /// Uppercase (gU)
    Uppercase,
    /// Lowercase (gu)
    Lowercase,
    /// Format (gq)
    Format,
    /// Format keeping cursor position (gw)
    FormatKeepCursor,
    /// Call operatorfunc (g@) — runtime extension gateway.
    CallOperatorFunc,
    /// Filter through external program (!)
    Filter,
    /// Reindent (=)
    Reindent,
    /// ROT13 cipher (g?)
    Rot13,
    /// ROT47 cipher (g&)
    Rot47,
    /// Host-registered custom operator (runtime extension).
    ///
    /// The `u32` is a unique ID assigned by the host when registering.
    /// Dispatches to `Effect::CallOperatorFunc` with the ID for host-side execution.
    Custom(u32),

    /// Two operators composed into a single operation (e.g., `y>w` = yank + indent).
    ///
    /// Created when a DIFFERENT operator key is pressed during operator-pending state.
    /// Same-key doubling (`dd`, `yy`) still produces `OperatorLine` as before.
    ///
    /// Both sub-operators are stored in a `ComposedPair`, which is `Copy`
    /// and avoids heap allocation.
    ///
    /// Constraints (enforced by `compose()`):
    /// - Neither operator may be `Change`
    /// - Both operators must not both be destructive (`Delete` + `Delete` is rejected)
    /// - The two operators must be different
    Composed(ComposedPair),
}

/// Discriminator for the [`Operator`] enum.
///
/// Mirrors the variant structure of `Operator` but without payloads,
/// allowing exhaustive pattern matching and set-membership checks without
/// binding to specific `Custom(u32)` or `Composed(ComposedPair)` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum OperatorKind {
    /// Delete (d).
    Delete,
    /// Change (c).
    Change,
    /// Yank (y).
    Yank,
    /// Indent right (>).
    Indent,
    /// Indent left (<).
    Outdent,
    /// Toggle case (g~).
    ToggleCase,
    /// Uppercase (gU).
    Uppercase,
    /// Lowercase (gu).
    Lowercase,
    /// Format (gq).
    Format,
    /// Format keeping cursor position (gw).
    FormatKeepCursor,
    /// Call operatorfunc (g@).
    CallOperatorFunc,
    /// Filter through external program (!).
    Filter,
    /// Reindent (=).
    Reindent,
    /// ROT13 cipher (g?).
    Rot13,
    /// ROT47 cipher (g&).
    Rot47,
    /// Host-registered custom operator — payload stripped.
    Custom,
    /// Two operators composed into a single operation — payload stripped.
    Composed,
}

impl OperatorKind {
    /// All 17 `OperatorKind` variants, in declaration order.
    ///
    /// Used by guard tests to verify completeness and absence of duplicates.
    pub const ALL: [Self; 17] = [
        Self::Delete,
        Self::Change,
        Self::Yank,
        Self::Indent,
        Self::Outdent,
        Self::ToggleCase,
        Self::Uppercase,
        Self::Lowercase,
        Self::Format,
        Self::FormatKeepCursor,
        Self::CallOperatorFunc,
        Self::Filter,
        Self::Reindent,
        Self::Rot13,
        Self::Rot47,
        Self::Custom,
        Self::Composed,
    ];
}

/// A `Copy`-safe pair of two composed operators.
///
/// Stores two `Operator` values in a compact encoding — one byte for the
/// discriminant and up to four bytes for a payload (`Custom(u32)`).
/// This allows `Operator` itself to remain `Copy` even when the
/// `composed-operators` feature is enabled.
///
/// The encoding is deliberately opaque; use [`ComposedPair::new`],
/// [`ComposedPair::first`], and [`ComposedPair::second`] for access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ComposedPair {
    /// Encoded first operator: low byte = discriminant, upper 4 bytes = Custom payload.
    first: u64,
    /// Encoded second operator: low byte = discriminant, upper 4 bytes = Custom payload.
    second: u64,
}

impl ComposedPair {
    // Discriminant constants (must not overlap with each other; 0 is reserved for "none").
    const DISC_DELETE: u8 = 1;
    const DISC_CHANGE: u8 = 2;
    const DISC_YANK: u8 = 3;
    const DISC_INDENT: u8 = 4;
    const DISC_OUTDENT: u8 = 5;
    const DISC_TOGGLE_CASE: u8 = 6;
    const DISC_UPPERCASE: u8 = 7;
    const DISC_LOWERCASE: u8 = 8;
    const DISC_FORMAT: u8 = 9;
    const DISC_FORMAT_KEEP_CURSOR: u8 = 10;
    const DISC_CALL_OPERATOR_FUNC: u8 = 11;
    const DISC_FILTER: u8 = 12;
    const DISC_REINDENT: u8 = 13;
    const DISC_CUSTOM: u8 = 14;
    const DISC_ROT13: u8 = 15;
    const DISC_ROT47: u8 = 16;

    /// Encode a single `Operator` into a `u64`.
    ///
    /// Layout: `[payload: u32][_reserved: u24][discriminant: u8]`
    const fn encode(op: Operator) -> u64 {
        match op {
            Operator::Delete => Self::DISC_DELETE as u64,
            Operator::Change => Self::DISC_CHANGE as u64,
            Operator::Yank => Self::DISC_YANK as u64,
            Operator::Indent => Self::DISC_INDENT as u64,
            Operator::Outdent => Self::DISC_OUTDENT as u64,
            Operator::ToggleCase => Self::DISC_TOGGLE_CASE as u64,
            Operator::Uppercase => Self::DISC_UPPERCASE as u64,
            Operator::Lowercase => Self::DISC_LOWERCASE as u64,
            Operator::Format => Self::DISC_FORMAT as u64,
            Operator::FormatKeepCursor => Self::DISC_FORMAT_KEEP_CURSOR as u64,
            Operator::CallOperatorFunc => Self::DISC_CALL_OPERATOR_FUNC as u64,
            Operator::Filter => Self::DISC_FILTER as u64,
            Operator::Reindent => Self::DISC_REINDENT as u64,
            Operator::Rot13 => Self::DISC_ROT13 as u64,
            Operator::Rot47 => Self::DISC_ROT47 as u64,
            Operator::Custom(id) => ((id as u64) << 32) | (Self::DISC_CUSTOM as u64),
            // Nested Composed operators are not supported; treat as no-op encoding.
            Operator::Composed(_) => 0,
        }
    }

    /// Decode a `u64` back into an `Operator`.
    ///
    /// Returns `None` if the discriminant is unrecognised (defensive; should not happen
    /// for values created via `encode`).
    const fn decode(raw: u64) -> Option<Operator> {
        let disc = (raw & 0xFF) as u8;
        let payload = (raw >> 32) as u32;
        match disc {
            Self::DISC_DELETE => Some(Operator::Delete),
            Self::DISC_CHANGE => Some(Operator::Change),
            Self::DISC_YANK => Some(Operator::Yank),
            Self::DISC_INDENT => Some(Operator::Indent),
            Self::DISC_OUTDENT => Some(Operator::Outdent),
            Self::DISC_TOGGLE_CASE => Some(Operator::ToggleCase),
            Self::DISC_UPPERCASE => Some(Operator::Uppercase),
            Self::DISC_LOWERCASE => Some(Operator::Lowercase),
            Self::DISC_FORMAT => Some(Operator::Format),
            Self::DISC_FORMAT_KEEP_CURSOR => Some(Operator::FormatKeepCursor),
            Self::DISC_CALL_OPERATOR_FUNC => Some(Operator::CallOperatorFunc),
            Self::DISC_FILTER => Some(Operator::Filter),
            Self::DISC_REINDENT => Some(Operator::Reindent),
            Self::DISC_ROT13 => Some(Operator::Rot13),
            Self::DISC_ROT47 => Some(Operator::Rot47),
            Self::DISC_CUSTOM => Some(Operator::Custom(payload)),
            _ => None,
        }
    }

    /// Create a new `ComposedPair` from two operators.
    #[must_use]
    pub const fn new(first: Operator, second: Operator) -> Self {
        Self {
            first: Self::encode(first),
            second: Self::encode(second),
        }
    }

    /// Decode and return the first operator.
    ///
    /// # Panics
    ///
    /// Panics if the encoding is corrupt (should never happen — only values
    /// created via [`ComposedPair::new`] are valid).
    #[must_use]
    pub const fn first(self) -> Operator {
        match Self::decode(self.first) {
            Some(op) => op,
            #[expect(
                clippy::panic,
                reason = "invariant: only values built via Self::new (which uses Self::encode) are observable; corruption would indicate UB elsewhere"
            )]
            None => panic!("ComposedPair: corrupt first operator encoding"),
        }
    }

    /// Decode and return the second operator.
    ///
    /// # Panics
    ///
    /// Panics if the encoding is corrupt (should never happen — only values
    /// created via [`ComposedPair::new`] are valid).
    #[must_use]
    pub const fn second(self) -> Operator {
        match Self::decode(self.second) {
            Some(op) => op,
            #[expect(
                clippy::panic,
                reason = "invariant: only values built via Self::new (which uses Self::encode) are observable; corruption would indicate UB elsewhere"
            )]
            None => panic!("ComposedPair: corrupt second operator encoding"),
        }
    }
}

impl Operator {
    /// Return the Vim key notation string for this operator.
    ///
    /// This is the keystroke sequence that the user would type to invoke
    /// the operator (e.g., `"d"` for Delete, `"g~"` for ToggleCase).
    /// Used by `InputState::pending_display()` for showcmd display.
    #[must_use]
    pub const fn key_notation(&self) -> &'static str {
        match self {
            Self::Delete => "d",
            Self::Change => "c",
            Self::Yank => "y",
            Self::Indent => ">",
            Self::Outdent => "<",
            Self::ToggleCase => "g~",
            Self::Uppercase => "gU",
            Self::Lowercase => "gu",
            Self::Format => "gq",
            Self::FormatKeepCursor => "gw",
            Self::CallOperatorFunc => "g@",
            Self::Filter => "!",
            Self::Reindent => "=",
            Self::Rot13 => "g?",
            Self::Rot47 => "g&",
            Self::Custom(_) => "g@",
            Self::Composed(_) => "<composed>",
        }
    }

    /// Return the single-character key that invokes this operator, if any.
    ///
    /// Multi-key operators (g-prefix variants like `g~`, `gU`, etc.) and
    /// custom operators return `None`.
    #[must_use]
    pub const fn key_char(&self) -> Option<char> {
        match self {
            Self::Delete => Some('d'),
            Self::Change => Some('c'),
            Self::Yank => Some('y'),
            Self::Indent => Some('>'),
            Self::Outdent => Some('<'),
            Self::Filter => Some('!'),
            Self::Reindent => Some('='),
            // Multi-char operators and custom operators have no single key.
            Self::ToggleCase
            | Self::Uppercase
            | Self::Lowercase
            | Self::Format
            | Self::FormatKeepCursor
            | Self::CallOperatorFunc
            | Self::Rot13
            | Self::Rot47
            | Self::Custom(_) => None,
            Self::Composed(_) => None,
        }
    }

    /// Return the [`OperatorKind`] discriminator for this operator.
    ///
    /// Strips the payload from `Custom(u32)` and `Composed(ComposedPair)`,
    /// returning a plain enum variant suitable for `match` arms, `HashSet`
    /// membership, and serialisation without binding to specific payloads.
    #[must_use]
    pub const fn kind(&self) -> OperatorKind {
        match self {
            Operator::Delete => OperatorKind::Delete,
            Operator::Change => OperatorKind::Change,
            Operator::Yank => OperatorKind::Yank,
            Operator::Indent => OperatorKind::Indent,
            Operator::Outdent => OperatorKind::Outdent,
            Operator::ToggleCase => OperatorKind::ToggleCase,
            Operator::Uppercase => OperatorKind::Uppercase,
            Operator::Lowercase => OperatorKind::Lowercase,
            Operator::Format => OperatorKind::Format,
            Operator::FormatKeepCursor => OperatorKind::FormatKeepCursor,
            Operator::CallOperatorFunc => OperatorKind::CallOperatorFunc,
            Operator::Filter => OperatorKind::Filter,
            Operator::Reindent => OperatorKind::Reindent,
            Operator::Rot13 => OperatorKind::Rot13,
            Operator::Rot47 => OperatorKind::Rot47,
            Operator::Custom(_) => OperatorKind::Custom,
            Operator::Composed(_) => OperatorKind::Composed,
        }
    }

    /// Check if this operator enters insert mode after execution.
    #[must_use]
    pub const fn enters_insert(self) -> bool {
        matches!(self, Self::Change)
    }

    /// Check if this is a case-changing operator.
    #[must_use]
    pub const fn is_case_operator(self) -> bool {
        matches!(self, Self::ToggleCase | Self::Uppercase | Self::Lowercase)
    }

    /// Check if this operator modifies text.
    ///
    /// Uses positive matching so that new `#[non_exhaustive]` variants
    /// default to non-mutating (safe) rather than silently becoming mutating.
    /// `Custom` operators are included because they are host-executed and
    /// assumed to mutate text for safety (undo groups are always created).
    #[must_use]
    pub const fn is_mutating(self) -> bool {
        match self {
            Self::Delete
            | Self::Change
            | Self::Indent
            | Self::Outdent
            | Self::ToggleCase
            | Self::Uppercase
            | Self::Lowercase
            | Self::Rot13
            | Self::Rot47
            | Self::Format
            | Self::FormatKeepCursor
            | Self::CallOperatorFunc
            | Self::Filter
            | Self::Reindent
            | Self::Custom(_) => true,
            Self::Composed(pair) => pair.first().is_mutating() || pair.second().is_mutating(),
            _ => false,
        }
    }

    /// Check if this operator reads document content to compute its output.
    ///
    /// Content-reading operators need the actual text under the cursor to
    /// produce their result (case transforms, ciphers, format).  Operators
    /// that don't depend on existing content (delete, yank, shift, reindent)
    /// return `false`.
    ///
    /// Host-delegated operators (`Reindent`, `CallOperatorFunc`, `Filter`,
    /// `Custom`) return `false` — they fall back to algebraic rebase per
    /// design spec Section 7.8.
    ///
    /// Uses exhaustive matching (no wildcard arm) so the compiler forces
    /// classification of any new variant.
    #[must_use]
    pub const fn is_content_reading(&self) -> bool {
        match self {
            // Case transforms — must read text to transform it.
            Self::ToggleCase | Self::Uppercase | Self::Lowercase => true,

            // Ciphers — must read text to encode it.
            Self::Rot13 | Self::Rot47 => true,

            // Format — must read text to reflow it.
            Self::Format | Self::FormatKeepCursor => true,

            // Indent/Outdent — content-reading for multi-cursor:
            // Indent inserts at line_start, which doesn't shift by cursor delta
            // when cursors are at different columns within their lines.
            // Outdent reads leading_spaces to determine deletion range.
            Self::Indent | Self::Outdent => true,

            // Structural / positional — don't depend on text content.
            Self::Delete | Self::Change | Self::Yank => false,

            // Host-delegated — algebraic rebase fallback (spec §7.8).
            Self::Reindent | Self::CallOperatorFunc | Self::Filter | Self::Custom(_) => false,

            // Composed — content-reading if either sub-operator is.
            Self::Composed(pair) => {
                pair.first().is_content_reading() || pair.second().is_content_reading()
            }
        }
    }

    /// Whether this operator produces Delete/Replace effects that change the
    /// document text. Used by `Command::is_content_dependent()` to classify
    /// operator+motion combos: mutating operators have content-dependent range
    /// widths (different lines/words have different byte widths).
    ///
    /// Distinct from `is_content_reading()`: Delete/Change don't READ content
    /// to TRANSFORM it (like case/format do), but their DELETE RANGES depend
    /// on what content exists at each cursor position.
    #[must_use]
    pub const fn is_mutating_text(self) -> bool {
        matches!(self, Self::Delete | Self::Change)
    }

    /// Check if this operator is destructive (deletes or changes text content).
    ///
    /// Used by `compose()` to reject invalid combinations.
    #[must_use]
    pub const fn is_destructive(self) -> bool {
        matches!(self, Self::Delete | Self::Change)
    }

    /// Attempt to compose two operators into a single `Composed` operator.
    ///
    /// Returns `None` if the combination is invalid:
    /// - Either operator is `Change`
    /// - Both operators are destructive (`Delete` + `Delete`)
    /// - The two operators are identical
    ///
    /// The same-key doubling (`dd`, `yy`) must NOT use this method —
    /// it produces `OperatorLine` at the grammar layer, not a `Composed` operator.
    #[must_use]
    pub fn compose(self, other: Self) -> Option<Self> {
        // Reject Change in either position.
        if matches!(self, Self::Change) || matches!(other, Self::Change) {
            return None;
        }
        // Reject nested composition (Composed cannot be composed further).
        if matches!(self, Self::Composed(_)) || matches!(other, Self::Composed(_)) {
            return None;
        }
        // Reject both-destructive (Delete + Delete).
        if self.is_destructive() && other.is_destructive() {
            return None;
        }
        // Reject identical operators (same-key doubling is handled by grammar layer).
        if self == other {
            return None;
        }
        Some(Self::Composed(ComposedPair::new(self, other)))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    // ── key_notation() — exhaustive coverage of all 17 variants ─────────

    #[test]
    fn key_notation_delete() {
        assert_eq!(Operator::Delete.key_notation(), "d");
    }

    #[test]
    fn key_notation_change() {
        assert_eq!(Operator::Change.key_notation(), "c");
    }

    #[test]
    fn key_notation_yank() {
        assert_eq!(Operator::Yank.key_notation(), "y");
    }

    #[test]
    fn key_notation_indent() {
        assert_eq!(Operator::Indent.key_notation(), ">");
    }

    #[test]
    fn key_notation_outdent() {
        assert_eq!(Operator::Outdent.key_notation(), "<");
    }

    #[test]
    fn key_notation_toggle_case() {
        assert_eq!(Operator::ToggleCase.key_notation(), "g~");
    }

    #[test]
    fn key_notation_uppercase() {
        assert_eq!(Operator::Uppercase.key_notation(), "gU");
    }

    #[test]
    fn key_notation_lowercase() {
        assert_eq!(Operator::Lowercase.key_notation(), "gu");
    }

    #[test]
    fn key_notation_format() {
        assert_eq!(Operator::Format.key_notation(), "gq");
    }

    #[test]
    fn key_notation_format_keep_cursor() {
        assert_eq!(Operator::FormatKeepCursor.key_notation(), "gw");
    }

    #[test]
    fn key_notation_call_operator_func() {
        assert_eq!(Operator::CallOperatorFunc.key_notation(), "g@");
    }

    #[test]
    fn key_notation_filter() {
        assert_eq!(Operator::Filter.key_notation(), "!");
    }

    #[test]
    fn key_notation_reindent() {
        assert_eq!(Operator::Reindent.key_notation(), "=");
    }

    #[test]
    fn key_notation_custom() {
        assert_eq!(Operator::Custom(0).key_notation(), "g@");
        assert_eq!(Operator::Custom(42).key_notation(), "g@");
        assert_eq!(Operator::Custom(u32::MAX).key_notation(), "g@");
    }

    /// Verify all variants in a single table-driven test for regression.
    #[test]
    fn key_notation_exhaustive_table() {
        let cases: &[(Operator, &str)] = &[
            (Operator::Delete, "d"),
            (Operator::Change, "c"),
            (Operator::Yank, "y"),
            (Operator::Indent, ">"),
            (Operator::Outdent, "<"),
            (Operator::ToggleCase, "g~"),
            (Operator::Uppercase, "gU"),
            (Operator::Lowercase, "gu"),
            (Operator::Rot13, "g?"),
            (Operator::Rot47, "g&"),
            (Operator::Format, "gq"),
            (Operator::FormatKeepCursor, "gw"),
            (Operator::CallOperatorFunc, "g@"),
            (Operator::Filter, "!"),
            (Operator::Reindent, "="),
            (Operator::Custom(99), "g@"),
        ];

        for (op, expected) in cases {
            assert_eq!(
                op.key_notation(),
                *expected,
                "{op:?}.key_notation() should be {expected:?}"
            );
        }
    }

    // ── composed-operators feature tests ────────────────────────────────

    mod composed {
        use super::*;

        #[test]
        fn compose_yank_indent_succeeds() {
            let composed = Operator::Yank.compose(Operator::Indent);
            assert!(composed.is_some(), "y> should compose successfully");
            let composed = composed.unwrap();
            assert!(matches!(composed, Operator::Composed(_)));
            if let Operator::Composed(pair) = composed {
                assert_eq!(pair.first(), Operator::Yank);
                assert_eq!(pair.second(), Operator::Indent);
            }
        }

        #[test]
        fn compose_delete_delete_rejected() {
            assert!(
                Operator::Delete.compose(Operator::Delete).is_none(),
                "Delete+Delete must be rejected"
            );
        }

        #[test]
        fn compose_change_anything_rejected() {
            assert!(Operator::Change.compose(Operator::Yank).is_none());
            assert!(Operator::Change.compose(Operator::Indent).is_none());
            assert!(Operator::Yank.compose(Operator::Change).is_none());
        }

        #[test]
        fn compose_same_operator_rejected() {
            assert!(Operator::Yank.compose(Operator::Yank).is_none());
            assert!(Operator::Indent.compose(Operator::Indent).is_none());
        }

        #[test]
        fn composed_pair_roundtrip_custom() {
            let pair = ComposedPair::new(Operator::Custom(42), Operator::Yank);
            assert_eq!(pair.first(), Operator::Custom(42));
            assert_eq!(pair.second(), Operator::Yank);
        }

        #[test]
        fn composed_pair_roundtrip_rot47() {
            let pair = ComposedPair::new(Operator::Rot47, Operator::Yank);
            assert_eq!(pair.first(), Operator::Rot47);
            assert_eq!(pair.second(), Operator::Yank);

            let pair2 = ComposedPair::new(Operator::Delete, Operator::Rot47);
            assert_eq!(pair2.first(), Operator::Delete);
            assert_eq!(pair2.second(), Operator::Rot47);
        }

        #[test]
        fn operator_is_still_copy_with_feature() {
            // This test verifies Operator remains Copy when the feature is enabled.
            // If it were non-Copy, this assignment would fail to compile.
            let op: Operator =
                Operator::Composed(ComposedPair::new(Operator::Yank, Operator::Indent));
            let _op2 = op; // Copy: op is still valid after this
            let _ = op; // Would be a compile error if Operator were non-Copy
        }
    }

    // ── is_content_reading() — exhaustive coverage of all variants ─────

    mod content_reading {
        use super::*;

        // Content-reading operators (return true).

        #[test]
        fn toggle_case_is_content_reading() {
            assert!(Operator::ToggleCase.is_content_reading());
        }

        #[test]
        fn uppercase_is_content_reading() {
            assert!(Operator::Uppercase.is_content_reading());
        }

        #[test]
        fn lowercase_is_content_reading() {
            assert!(Operator::Lowercase.is_content_reading());
        }

        #[test]
        fn rot13_is_content_reading() {
            assert!(Operator::Rot13.is_content_reading());
        }

        #[test]
        fn rot47_is_content_reading() {
            assert!(Operator::Rot47.is_content_reading());
        }

        #[test]
        fn format_is_content_reading() {
            assert!(Operator::Format.is_content_reading());
        }

        #[test]
        fn format_keep_cursor_is_content_reading() {
            assert!(Operator::FormatKeepCursor.is_content_reading());
        }

        // Non-content-reading operators (return false).

        #[test]
        fn delete_is_not_content_reading() {
            assert!(!Operator::Delete.is_content_reading());
        }

        #[test]
        fn change_is_not_content_reading() {
            assert!(!Operator::Change.is_content_reading());
        }

        #[test]
        fn yank_is_not_content_reading() {
            assert!(!Operator::Yank.is_content_reading());
        }

        #[test]
        fn indent_is_content_reading() {
            assert!(Operator::Indent.is_content_reading());
        }

        #[test]
        fn outdent_is_content_reading() {
            assert!(Operator::Outdent.is_content_reading());
        }

        #[test]
        fn reindent_is_not_content_reading() {
            assert!(!Operator::Reindent.is_content_reading());
        }

        #[test]
        fn call_operator_func_is_not_content_reading() {
            assert!(!Operator::CallOperatorFunc.is_content_reading());
        }

        #[test]
        fn filter_is_not_content_reading() {
            assert!(!Operator::Filter.is_content_reading());
        }

        #[test]
        fn custom_is_not_content_reading() {
            assert!(!Operator::Custom(0).is_content_reading());
            assert!(!Operator::Custom(42).is_content_reading());
            assert!(!Operator::Custom(u32::MAX).is_content_reading());
        }

        // Composed operators — delegates to sub-operators.

        #[test]
        fn composed_both_non_reading_is_not_content_reading() {
            let op = Operator::Composed(ComposedPair::new(Operator::Yank, Operator::Delete));
            assert!(!op.is_content_reading());
        }

        #[test]
        fn composed_first_reading_is_content_reading() {
            let op = Operator::Composed(ComposedPair::new(Operator::Uppercase, Operator::Yank));
            assert!(op.is_content_reading());
        }

        #[test]
        fn composed_second_reading_is_content_reading() {
            let op = Operator::Composed(ComposedPair::new(Operator::Yank, Operator::Rot13));
            assert!(op.is_content_reading());
        }

        #[test]
        fn composed_both_reading_is_content_reading() {
            let op = Operator::Composed(ComposedPair::new(Operator::Uppercase, Operator::Rot47));
            assert!(op.is_content_reading());
        }

        /// Table-driven exhaustive test for regression.
        #[test]
        fn is_content_reading_exhaustive_table() {
            let cases: &[(Operator, bool)] = &[
                // Content-reading (true).
                (Operator::ToggleCase, true),
                (Operator::Uppercase, true),
                (Operator::Lowercase, true),
                (Operator::Rot13, true),
                (Operator::Rot47, true),
                (Operator::Format, true),
                (Operator::FormatKeepCursor, true),
                (Operator::Indent, true),
                (Operator::Outdent, true),
                // Non-content-reading (false).
                (Operator::Delete, false),
                (Operator::Change, false),
                (Operator::Yank, false),
                (Operator::Reindent, false),
                (Operator::CallOperatorFunc, false),
                (Operator::Filter, false),
                (Operator::Custom(99), false),
            ];

            for (op, expected) in cases {
                assert_eq!(
                    op.is_content_reading(),
                    *expected,
                    "{op:?}.is_content_reading() should be {expected}"
                );
            }
        }
    }

    // ── OperatorKind guard tests ─────────────────────────────────────────

    #[test]
    fn operator_kind_all_no_duplicates() {
        let unique: HashSet<OperatorKind> = OperatorKind::ALL.iter().copied().collect();
        assert_eq!(
            unique.len(),
            OperatorKind::ALL.len(),
            "Duplicate in OperatorKind::ALL"
        );
    }

    #[test]
    fn operator_kind_covers_every_variant() {
        let ops = [
            Operator::Delete,
            Operator::Change,
            Operator::Yank,
            Operator::Indent,
            Operator::Outdent,
            Operator::ToggleCase,
            Operator::Uppercase,
            Operator::Lowercase,
            Operator::Format,
            Operator::FormatKeepCursor,
            Operator::CallOperatorFunc,
            Operator::Filter,
            Operator::Reindent,
            Operator::Rot13,
            Operator::Rot47,
            Operator::Custom(0),
            Operator::Composed(ComposedPair::new(Operator::Yank, Operator::Indent)),
        ];
        let kinds: HashSet<OperatorKind> = ops.iter().map(|o| o.kind()).collect();
        let all: HashSet<OperatorKind> = OperatorKind::ALL.iter().copied().collect();
        let missing: Vec<_> = all.difference(&kinds).collect();
        assert!(
            missing.is_empty(),
            "OperatorKind variants not covered: {:?}",
            missing
        );
    }
}
