//! Effect algebra — algebraic inverses for undo/redo without an undo stack.
//!
//! The three document-mutating effects (`Insert`, `Delete`, `Replace`) plus
//! `SetCursor`, `SetMode`, `SetRegister`, `ClearNamedRegister`, `SetMark`,
//! `ClearMark`, `SetSearchPattern`, `SetLastFind`, `SetSelection`, and
//! `ClearSelection` can produce algebraic inverses. Given a document state,
//! applying `effect` then `inverse` returns to the original state.
//!
//! Other state-mutating effects (`PushJumpList`, etc.) are excluded because
//! their inverses require internal `VimState` access, which this module cannot
//! import (it lives in the `effects` layer, below `state`).
//!
//! # Import constraints
//!
//! This module only depends on:
//! - `primitives` — `Offset`, `Range`, `Mode`, `RegisterName`, `RegisterContent`,
//!   `MarkName`, `Mark`, `LastFind`, `SelectionRange`, `SearchDirection`
//! - sibling `effect` module — `Effect`
//! - `compact_str` — `CompactString` (same as `effect.rs`)
//!
//! It does NOT import `state`, `commands`, `execution`, or any other layer.

use compact_str::CompactString;

use crate::effects::effect::Effect;
use crate::primitives::byte_delta;
use crate::primitives::{
    LastFind, Mark, MarkName, Mode, Offset, Range, RegisterContent, RegisterName, SearchDirection,
    SelectionRange, SelectionShape,
};

// ═══════════════════════════════════════════════════════════════════════════════
// TYPES
// ═══════════════════════════════════════════════════════════════════════════════

/// Context needed to compute inverses that depend on pre-effect state.
///
/// Most inverses are not self-contained: a `Delete { range }` inverse needs the
/// deleted text, a `SetCursor` inverse needs the previous cursor position, etc.
/// This struct carries that "before" state so the caller can supply it without
/// the algebra module reaching into engine internals.
///
/// Fields are optional because not every effect needs every piece of context.
/// The `inverse()` function returns `None` when a required field is missing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InverseContext {
    /// Text that was deleted by a `Delete` effect (the content of the range
    /// before deletion). Required for `Delete` inverses.
    pub deleted_text: Option<CompactString>,

    /// Text that was replaced by a `Replace` effect (the original content of
    /// the range before replacement). Required for `Replace` inverses.
    pub replaced_text: Option<CompactString>,

    /// Cursor position before the effect was applied.
    /// Required for `SetCursor` inverses.
    pub cursor_before: Option<Offset>,

    /// Editing mode before the effect was applied.
    /// Required for `SetMode` inverses.
    pub mode_before: Option<Mode>,

    /// Register state before a `SetRegister` or `ClearNamedRegister` effect.
    ///
    /// The tuple is `(name, prev_content)` where `prev_content` is `None` if
    /// the register was empty before the effect.
    pub register_before: Option<(RegisterName, Option<RegisterContent>)>,

    /// Mark state before a `SetMark` or `ClearMark` effect.
    ///
    /// The tuple is `(name, prev_mark)` where `prev_mark` is `None` if the
    /// mark did not exist before the effect.
    pub mark_before: Option<(MarkName, Option<Mark>)>,

    /// Search pattern and direction before a `SetSearchPattern` effect.
    ///
    /// The tuple is `(pattern, direction)` capturing the previous search state.
    pub search_pattern_before: Option<(CompactString, SearchDirection)>,

    /// Last-find state before a `SetLastFind` effect.
    pub last_find_before: Option<LastFind>,

    /// Selection before a `SetSelection` or `ClearSelection` effect.
    ///
    /// `None` means no selection was active before the effect.
    pub selection_before: Option<SelectionRange>,

    /// Selection shape before a `SetSelection` or `ClearSelection` effect.
    ///
    /// Used to restore the correct shape (Char/Line/Block) when computing
    /// inverses. Falls back to `SelectionShape::Char` if not provided.
    pub selection_shape_before: Option<SelectionShape>,
}

impl InverseContext {
    /// Create an empty context (no pre-effect state provided).
    #[inline]
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Create a context for a `Delete` effect.
    #[inline]
    #[must_use]
    pub fn for_delete(deleted_text: impl Into<CompactString>) -> Self {
        Self {
            deleted_text: Some(deleted_text.into()),
            ..Self::default()
        }
    }

    /// Create a context for a `Replace` effect.
    #[inline]
    #[must_use]
    pub fn for_replace(replaced_text: impl Into<CompactString>) -> Self {
        Self {
            replaced_text: Some(replaced_text.into()),
            ..Self::default()
        }
    }

    /// Create a context for a `SetCursor` effect.
    #[inline]
    #[must_use]
    pub fn for_cursor(cursor_before: Offset) -> Self {
        Self {
            cursor_before: Some(cursor_before),
            ..Self::default()
        }
    }

    /// Create a context for a `SetMode` effect.
    #[inline]
    #[must_use]
    pub fn for_mode(mode_before: Mode) -> Self {
        Self {
            mode_before: Some(mode_before),
            ..Self::default()
        }
    }

    /// Builder: set deleted text.
    #[inline]
    #[must_use]
    pub fn with_deleted_text(mut self, text: impl Into<CompactString>) -> Self {
        self.deleted_text = Some(text.into());
        self
    }

    /// Builder: set replaced text.
    #[inline]
    #[must_use]
    pub fn with_replaced_text(mut self, text: impl Into<CompactString>) -> Self {
        self.replaced_text = Some(text.into());
        self
    }

    /// Builder: set cursor before.
    #[inline]
    #[must_use]
    pub const fn with_cursor_before(mut self, offset: Offset) -> Self {
        self.cursor_before = Some(offset);
        self
    }

    /// Builder: set mode before.
    #[inline]
    #[must_use]
    pub const fn with_mode_before(mut self, mode: Mode) -> Self {
        self.mode_before = Some(mode);
        self
    }

    /// Builder: set register state before a `SetRegister` or `ClearNamedRegister` effect.
    #[inline]
    #[must_use]
    pub fn with_register_before(
        mut self,
        name: RegisterName,
        prev: Option<RegisterContent>,
    ) -> Self {
        self.register_before = Some((name, prev));
        self
    }

    /// Builder: set mark state before a `SetMark` or `ClearMark` effect.
    #[inline]
    #[must_use]
    pub const fn with_mark_before(mut self, name: MarkName, prev: Option<Mark>) -> Self {
        self.mark_before = Some((name, prev));
        self
    }

    /// Builder: set search pattern and direction before a `SetSearchPattern` effect.
    #[inline]
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn with_search_before(mut self, pattern: CompactString, dir: SearchDirection) -> Self {
        self.search_pattern_before = Some((pattern, dir));
        self
    }

    /// Builder: set last-find state before a `SetLastFind` effect.
    #[inline]
    #[must_use]
    pub const fn with_last_find_before(mut self, find: LastFind) -> Self {
        self.last_find_before = Some(find);
        self
    }

    /// Builder: set selection before a `SetSelection` or `ClearSelection` effect.
    #[inline]
    #[must_use]
    pub const fn with_selection_before(mut self, sel: SelectionRange) -> Self {
        self.selection_before = Some(sel);
        self
    }

    /// Builder: set selection shape before a `SetSelection` or `ClearSelection` effect.
    #[inline]
    #[must_use]
    pub const fn with_selection_shape_before(mut self, shape: SelectionShape) -> Self {
        self.selection_shape_before = Some(shape);
        self
    }
}

/// A mutating effect paired with its algebraic inverse.
///
/// Given a document state, applying `effect` then `inverse` returns to the
/// original state. This enables undo without a separate undo stack.
///
/// # Invariant
///
/// For any document `D`, if `D' = apply(D, effect)`, then `apply(D', inverse) == D`.
/// This invariant is verified by `verify_round_trip` in tests.
#[derive(Debug, Clone, PartialEq)]
pub struct ReversibleEffect {
    /// The forward effect.
    pub effect: Effect,
    /// The inverse that undoes the forward effect.
    pub inverse: Effect,
}

impl ReversibleEffect {
    /// Create a new reversible effect pair.
    #[inline]
    #[must_use]
    pub const fn new(effect: Effect, inverse: Effect) -> Self {
        Self { effect, inverse }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// INVERSE COMPUTATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Compute the inverse of a mutating effect, given pre-effect context.
///
/// Returns `None` for:
/// - Effects not in the supported set
/// - Supported effects where the required context field is missing
///
/// # Complexity
///
/// Time: O(t) where t = text length (for cloning text in Delete/Replace inverses)
/// Space: O(t) for the cloned text in the returned Effect
///
/// # Inverse rules
///
/// | Forward effect                | Inverse effect                                           | Required context          |
/// |-------------------------------|----------------------------------------------------------|---------------------------|
/// | `Insert { offset, text }`     | `Delete { range: offset..offset+text.len() }`            | None                      |
/// | `Delete { range }`            | `Insert { offset: range.start, text: deleted_text }`     | `deleted_text`            |
/// | `Replace { range, text }`     | `Replace { range: new_range, text: replaced_text }`      | `replaced_text`           |
/// | `SetCursor { offset }`        | `SetCursor { offset: cursor_before }`                    | `cursor_before`           |
/// | `SetMode { mode }`            | `SetMode { mode: mode_before }`                          | `mode_before`             |
/// | `SetRegister { name, .. }`    | `SetRegister` (prev) or `ClearNamedRegister`             | `register_before`         |
/// | `ClearNamedRegister { .. }`   | `SetRegister` (prev) or `Noop`                           | `register_before`         |
/// | `SetMark { name, .. }`        | `SetMark` (prev) or `ClearMark`                          | `mark_before`             |
/// | `ClearMark { mark }`          | `SetMark` (prev) or `Noop`                               | `mark_before`             |
/// | `SetSearchPattern { .. }`     | `SetSearchPattern` with prev pattern/dir                 | `search_pattern_before`   |
/// | `SetLastFind { .. }`          | `SetLastFind` (prev) or `Noop`                           | `last_find_before`        |
/// | `SetSelection { .. }`         | `SetSelection` (prev) or `ClearSelection`                | None / `selection_before` |
/// | `ClearSelection`              | `SetSelection` with prev                                 | `selection_before`        |
/// | `Noop`                        | `Noop`                                                   | None                      |
///
/// For `Replace`, the inverse range starts at `range.start()` and spans
/// `text.len()` bytes (the length of the *replacement* text, which is now
/// in the document).
#[must_use]
pub fn inverse(effect: &Effect, ctx: &InverseContext) -> Option<Effect> {
    match effect {
        // ── Document mutations ──────────────────────────────────────────
        //
        // Insert { offset, text } → Delete the just-inserted text.
        // The inserted text occupies [offset, offset + text.len()).
        Effect::Insert { offset, text } => {
            let start = *offset;
            let end = start.saturating_add_raw(text.len());
            Some(Effect::Delete {
                range: Range::new(start, end),
            })
        }

        // Delete { range } → Re-insert the deleted text at range.start().
        // Requires the caller to supply the deleted text via context.
        Effect::Delete { range } => {
            let deleted_text = ctx.deleted_text.as_ref()?;
            Some(Effect::Insert {
                offset: range.start(),
                text: deleted_text.clone(),
            })
        }

        // Replace { range, text } → Replace the new text with the original.
        // After the forward replace:
        //   - The original range [range.start, range.end) was replaced by `text`.
        //   - The new range in the document is [range.start, range.start + text.len()).
        // The inverse replaces that new range with the original text.
        Effect::Replace { range, text } => {
            let replaced_text = ctx.replaced_text.as_ref()?;
            let new_end = range.start().saturating_add_raw(text.len());
            let new_range = Range::new(range.start(), new_end);
            Some(Effect::Replace {
                range: new_range,
                text: replaced_text.clone(),
            })
        }

        // ── State mutations ─────────────────────────────────────────────

        // SetCursor → SetCursor back to previous position.
        Effect::SetCursor { .. } => {
            let prev = ctx.cursor_before?;
            Some(Effect::SetCursor { offset: prev })
        }

        // SetMode → SetMode back to previous mode.
        Effect::SetMode { .. } => {
            let prev = ctx.mode_before?;
            Some(Effect::set_mode(prev))
        }

        // ── Register mutations ───────────────────────────────────────────

        // SetRegister → restore previous register content (or clear if empty before).
        Effect::SetRegister { .. } => {
            let (reg_name, prev_content) = ctx.register_before.as_ref()?;
            match prev_content {
                Some(prev) => Some(Effect::SetRegister {
                    name: *reg_name,
                    text: CompactString::from(prev.text()),
                    motion_type: prev.motion_type(),
                }),
                None => Some(Effect::ClearNamedRegister {
                    register: *reg_name,
                }),
            }
        }

        // ClearNamedRegister → restore previous content (or Noop if already empty).
        Effect::ClearNamedRegister { .. } => {
            let (reg_name, prev_content) = ctx.register_before.as_ref()?;
            match prev_content {
                Some(prev) => Some(Effect::SetRegister {
                    name: *reg_name,
                    text: CompactString::from(prev.text()),
                    motion_type: prev.motion_type(),
                }),
                None => Some(Effect::Noop),
            }
        }

        // ── Mark mutations ───────────────────────────────────────────────

        // SetMark → restore previous mark (or clear if mark didn't exist before).
        Effect::SetMark { name, .. } => {
            let (_mark_name, prev_mark) = ctx.mark_before.as_ref()?;
            match prev_mark {
                Some(prev) => Some(Effect::SetMark {
                    name: *name,
                    offset: prev.offset(),
                    topline_offset: prev.topline_offset(),
                }),
                None => Some(Effect::ClearMark { mark: *name }),
            }
        }

        // ClearMark → restore previous mark (or Noop if mark didn't exist before).
        Effect::ClearMark { mark } => {
            let (_mark_name, prev_mark) = ctx.mark_before.as_ref()?;
            match prev_mark {
                Some(prev) => Some(Effect::SetMark {
                    name: *mark,
                    offset: prev.offset(),
                    topline_offset: prev.topline_offset(),
                }),
                None => Some(Effect::Noop),
            }
        }

        // ── Search pattern mutations ─────────────────────────────────────

        // SetSearchPattern → restore previous pattern and direction.
        Effect::SetSearchPattern { .. } => {
            let (prev_pattern, prev_dir) = ctx.search_pattern_before.as_ref()?;
            Some(Effect::SetSearchPattern {
                pattern: prev_pattern.clone(),
                direction: (*prev_dir).into(),
            })
        }

        // ── Last-find mutations ──────────────────────────────────────────

        // SetLastFind → restore previous last-find state.
        Effect::SetLastFind { .. } => {
            let prev = ctx.last_find_before?;
            match prev.direction() {
                Some(dir) => Some(Effect::SetLastFind {
                    direction: dir,
                    target_char: prev.target_char().unwrap_or('\0'),
                    sneak_c2: prev.sneak_c2(),
                    resolved_ignorecase: prev.resolved_ignorecase(),
                    resolved_smartcase: prev.resolved_smartcase(),
                }),
                None => Some(Effect::Noop),
            }
        }

        // ── Selection mutations ──────────────────────────────────────────

        // SetSelection → restore previous selection (or clear if none before).
        Effect::SetSelection { .. } => match ctx.selection_before {
            Some(prev) => Some(Effect::SetSelection {
                anchor: prev.anchor(),
                head: prev.head(),
                shape: ctx.selection_shape_before.unwrap_or(SelectionShape::Char),
            }),
            None => Some(Effect::ClearSelection),
        },

        // ClearSelection → restore previous selection (if any).
        Effect::ClearSelection => {
            let prev = ctx.selection_before?;
            Some(Effect::SetSelection {
                anchor: prev.anchor(),
                head: prev.head(),
                shape: ctx.selection_shape_before.unwrap_or(SelectionShape::Char),
            })
        }

        // ── No-op ────────────────────────────────────────────────────────

        // Noop → Noop (trivially invertible).
        Effect::Noop => Some(Effect::Noop),

        // ── Everything else: no meaningful inverse ──────────────────────
        _ => None,
    }
}

/// Create a [`ReversibleEffect`] by computing the inverse.
///
/// Returns `None` if the effect has no meaningful inverse (non-mutating
/// effects) or if the required context is missing.
///
/// # Complexity
///
/// Time: O(t) where t = text length (clones both effect and inverse)
/// Space: O(t)
#[must_use]
pub fn make_reversible(effect: &Effect, ctx: &InverseContext) -> Option<ReversibleEffect> {
    let inv = inverse(effect, ctx)?;
    Some(ReversibleEffect::new(effect.clone(), inv))
}

// ═══════════════════════════════════════════════════════════════════════════════
// OFFSET ADJUSTMENT (OT KERNEL)
// ═══════════════════════════════════════════════════════════════════════════════

/// Adjust a byte offset by a single already-applied effect.
///
/// Given an `offset` that was valid *before* `effect` was applied, returns the
/// offset that points to the same logical position *after* `effect` has been
/// applied to the document.
///
/// This is the core primitive of Operational Transformation: it answers "where
/// does my position end up after someone else's edit?"
///
/// # Rules
///
/// | Effect | offset < at / range.start | offset in range | offset >= end / at+len |
/// |--------|---------------------------|-----------------|------------------------|
/// | `Insert { at, text }` | unchanged | — | shifted right by `text.len()` |
/// | `Delete { range }` | unchanged | clamped to `range.start` | shifted left by `range.len()` |
/// | `Replace { range, text }` | unchanged | clamped to `range.start` | adjusted for net length change |
/// | all others | unchanged | — | — |
///
/// # Complexity
///
/// Time: O(1). Space: O(1).
///
/// # Examples
///
/// ```
/// use vim_core::effects::{Effect, algebra::adjust_offset};
/// use vim_core::primitives::{Offset, Range};
/// use compact_str::CompactString;
///
/// // Insert 3 bytes at position 5 — offsets >= 5 shift right.
/// let ins = Effect::Insert { offset: Offset::new(5), text: CompactString::new("abc") };
/// assert_eq!(adjust_offset(Offset::new(8), &ins), Offset::new(11));
/// assert_eq!(adjust_offset(Offset::new(4), &ins), Offset::new(4));
///
/// // Delete range 3..6 — offsets in [3,6) clamp to 3; offsets >= 6 shift left by 3.
/// let del = Effect::Delete { range: Range::from_raw(3, 6) };
/// assert_eq!(adjust_offset(Offset::new(4), &del), Offset::new(3));
/// assert_eq!(adjust_offset(Offset::new(7), &del), Offset::new(4));
/// ```
#[must_use]
pub fn adjust_offset(offset: Offset, effect: &Effect) -> Offset {
    match effect {
        // ── Insert ───────────────────────────────────────────────────────
        Effect::Insert { offset: at, text } => {
            if offset.get() >= at.get() {
                offset.saturating_add_raw(text.len())
            } else {
                offset
            }
        }

        // ── Delete ───────────────────────────────────────────────────────
        Effect::Delete { range } => {
            let start = range.start().get();
            let end = range.end().get();
            if offset.get() >= end {
                // After the deleted region — shift left by deleted length.
                offset.saturating_sub_raw(range.len())
            } else if offset.get() >= start {
                // Inside the deleted region — clamp to range start.
                range.start()
            } else {
                offset
            }
        }

        // ── Replace (inline: delete then insert adjustment) ───────────────
        //
        // A Replace { range, text } is semantically "delete range then insert
        // text at range.start". We apply both adjustments inline without
        // calling adjust_offset recursively.
        Effect::Replace { range, text } => {
            let del_start = range.start().get();
            let del_end = range.end().get();

            // Step 1: apply the delete adjustment.
            let after_delete = if offset.get() >= del_end {
                offset.saturating_sub_raw(range.len())
            } else if offset.get() >= del_start {
                range.start()
            } else {
                offset
            };

            // Step 2: apply the insert adjustment (insert text.len() bytes at
            // del_start, which after the delete is still del_start).
            let ins_at = del_start;
            if after_delete.get() >= ins_at {
                after_delete.saturating_add_raw(text.len())
            } else {
                after_delete
            }
        }

        // ── All other effects don't move document positions ───────────────
        _ => offset,
    }
}

/// Sum all net text insertions minus deletions across an effect slice.
///
/// Each [`Effect::Insert`] contributes `+text.len()`, each
/// [`Effect::Delete`] contributes `-range.len()`, and each
/// [`Effect::Replace`] contributes `text.len() as i64 - range.len() as i64`.
/// All other effects contribute `0`.
///
/// # Complexity
///
/// Time: O(n). Space: O(1).
///
/// # Examples
///
/// ```
/// use vim_core::effects::{Effect, algebra::compute_length_change};
/// use vim_core::primitives::{Offset, Range};
/// use compact_str::CompactString;
///
/// let effects = vec![
///     Effect::Insert { offset: Offset::new(0), text: CompactString::new("hello") }, // +5
///     Effect::Delete { range: Range::from_raw(0, 2) },                              // -2
/// ];
/// assert_eq!(compute_length_change(&effects), 3);
/// ```
#[must_use]
pub fn compute_length_change(effects: &[Effect]) -> i64 {
    let mut delta: i64 = 0;
    for effect in effects {
        match effect {
            Effect::Insert { text, .. } => {
                delta += byte_delta::to_i64(text.len());
            }
            Effect::Delete { range } => {
                delta -= byte_delta::to_i64(range.len());
            }
            Effect::Replace { range, text } => {
                delta += byte_delta::delta_i64(text.len(), range.len());
            }
            _ => {}
        }
    }
    delta
}

/// Rebase a single effect by a signed byte delta.
///
/// Shared implementation for [`rebase_effects_by_delta`] and [`rebase_effects_into`].
/// Translates all position-bearing fields (offsets, ranges) by `delta`.
/// Non-positional effects pass through unchanged via clone.
fn rebase_single_effect(effect: &Effect, delta: i64) -> Effect {
    match effect {
        Effect::Insert { offset, text } => Effect::Insert {
            offset: apply_delta_to_offset(*offset, delta),
            text: text.clone(),
        },
        Effect::Delete { range } => Effect::Delete {
            range: Range::new(
                apply_delta_to_offset(range.start(), delta),
                apply_delta_to_offset(range.end(), delta),
            ),
        },
        Effect::SetCursor { offset } => Effect::SetCursor {
            offset: apply_delta_to_offset(*offset, delta),
        },
        Effect::Replace { range, ref text } => Effect::Replace {
            range: Range::new(
                apply_delta_to_offset(range.start(), delta),
                apply_delta_to_offset(range.end(), delta),
            ),
            text: text.clone(),
        },
        // NOTE: BeginInsert is a global effect (not in is_positional_effect),
        // so this arm is currently unreachable through replicate_effects_precise.
        // Kept for completeness in case the classification changes.
        Effect::BeginInsert {
            entry_type,
            count,
            auto_indent_len,
            entry_offset,
        } => Effect::BeginInsert {
            entry_type: *entry_type,
            count: *count,
            auto_indent_len: *auto_indent_len,
            entry_offset: apply_delta_to_offset(*entry_offset, delta),
        },
        Effect::SetMark {
            name,
            offset,
            topline_offset,
        } => Effect::SetMark {
            name: *name,
            offset: apply_delta_to_offset(*offset, delta),
            topline_offset: *topline_offset,
        },
        Effect::SetSelection {
            anchor,
            head,
            shape,
        } => Effect::SetSelection {
            anchor: apply_delta_to_offset(*anchor, delta),
            head: apply_delta_to_offset(*head, delta),
            shape: *shape,
        },
        Effect::ScrollTo { offset } => Effect::ScrollTo {
            offset: apply_delta_to_offset(*offset, delta),
        },
        Effect::PushJumpList { offset } => Effect::PushJumpList {
            offset: apply_delta_to_offset(*offset, delta),
        },
        other => other.clone(),
    }
}

/// Shift all position-bearing effects by a signed byte delta.
///
/// Applies `delta` to every [`Effect`] that carries a document position.
/// Non-positional effects pass through unchanged.
///
/// Returns a new `Vec<Effect>`. See [`rebase_effects_into`] for the
/// zero-allocation variant that pushes directly into an existing `Vec`.
#[must_use]
pub fn rebase_effects_by_delta(effects: &[Effect], delta: i64) -> Vec<Effect> {
    effects
        .iter()
        .map(|effect| rebase_single_effect(effect, delta))
        .collect()
}

/// Like [`rebase_effects_by_delta`], but pushes directly into `target` instead
/// of allocating an intermediate `Vec`. Avoids N-1 temporary allocations when
/// called in a loop for multi-cursor replication.
pub fn rebase_effects_into(effects: &[Effect], delta: i64, target: &mut Vec<Effect>) {
    target.reserve(effects.len());
    for effect in effects {
        target.push(rebase_single_effect(effect, delta));
    }
}

/// Apply a signed delta to a `usize`-backed [`Offset`], saturating at 0.
///
/// Goes through `Offset`'s own saturating arithmetic rather than
/// `Offset::new`: the upper bound clamps to `OFFSET_MAX`, because `usize::MAX`
/// is the reserved niche sentinel and `Offset::new` panics on it.
#[inline]
fn apply_delta_to_offset(offset: Offset, delta: i64) -> Offset {
    if delta >= 0 {
        offset.saturating_add_raw(usize::try_from(delta).unwrap_or(usize::MAX))
    } else {
        offset.saturating_sub_raw(usize::try_from(delta.unsigned_abs()).unwrap_or(usize::MAX))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTING UTILITIES
// ═══════════════════════════════════════════════════════════════════════════════

/// Verify that applying an effect then its inverse produces the identity on text.
///
/// Takes the text before the effect, the forward effect, and the proposed
/// inverse effect. Simulates applying both and checks that the text returns
/// to the original.
///
/// This only verifies text-mutating effects (`Insert`, `Delete`, `Replace`).
/// Returns `true` for non-text-mutating effects (vacuously correct).
#[cfg(test)]
fn apply_text_effect(text: &str, effect: &Effect) -> Option<String> {
    match effect {
        Effect::Insert { offset, text: ins } => {
            let pos = offset.get();
            if pos > text.len() {
                return None;
            }
            let mut result = String::with_capacity(text.len() + ins.len());
            result.push_str(&text[..pos]);
            result.push_str(ins);
            result.push_str(&text[pos..]);
            Some(result)
        }
        Effect::Delete { range } => {
            let start = range.start().get();
            let end = range.end().get();
            if start > text.len() || end > text.len() {
                return None;
            }
            let mut result = String::with_capacity(text.len() - (end - start));
            result.push_str(&text[..start]);
            result.push_str(&text[end..]);
            Some(result)
        }
        Effect::Replace { range, text: repl } => {
            let start = range.start().get();
            let end = range.end().get();
            if start > text.len() || end > text.len() {
                return None;
            }
            let mut result = String::with_capacity(text.len() - (end - start) + repl.len());
            result.push_str(&text[..start]);
            result.push_str(repl);
            result.push_str(&text[end..]);
            Some(result)
        }
        _ => Some(text.to_string()),
    }
}

/// Verify round-trip: apply `effect` to `before`, then apply `inv` to the
/// intermediate result. Returns `true` iff the final text equals `before`.
#[cfg(test)]
fn verify_round_trip(before: &str, effect: &Effect, inv: &Effect) -> bool {
    let Some(after) = apply_text_effect(before, effect) else {
        return false;
    };
    let Some(restored) = apply_text_effect(&after, inv) else {
        return false;
    };
    restored == before
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{Mode, Offset, Range};
    use compact_str::CompactString;

    // ── Insert ↔ Delete round-trip ──────────────────────────────────────

    #[test]
    fn insert_inverse_is_delete() {
        let effect = Effect::Insert {
            offset: Offset::new(5),
            text: CompactString::new("hello"),
        };
        let ctx = InverseContext::empty();
        let inv = inverse(&effect, &ctx).expect("Insert should have an inverse");

        // The inverse of Insert{5, "hello"} is Delete{5..10}.
        assert_eq!(
            inv,
            Effect::Delete {
                range: Range::from_raw(5, 10),
            }
        );
    }

    #[test]
    fn insert_inverse_round_trip() {
        let before = "Hello, world!";
        let effect = Effect::Insert {
            offset: Offset::new(7),
            text: CompactString::new("beautiful "),
        };
        let ctx = InverseContext::empty();
        let inv = inverse(&effect, &ctx).unwrap();

        assert!(verify_round_trip(before, &effect, &inv));
    }

    #[test]
    fn insert_at_start_round_trip() {
        let before = "world";
        let effect = Effect::Insert {
            offset: Offset::new(0),
            text: CompactString::new("Hello "),
        };
        let ctx = InverseContext::empty();
        let inv = inverse(&effect, &ctx).unwrap();

        assert!(verify_round_trip(before, &effect, &inv));
    }

    #[test]
    fn insert_at_end_round_trip() {
        let before = "Hello";
        let effect = Effect::Insert {
            offset: Offset::new(5),
            text: CompactString::new(" world"),
        };
        let ctx = InverseContext::empty();
        let inv = inverse(&effect, &ctx).unwrap();

        assert!(verify_round_trip(before, &effect, &inv));
    }

    #[test]
    fn insert_empty_text() {
        let before = "abc";
        let effect = Effect::Insert {
            offset: Offset::new(1),
            text: CompactString::new(""),
        };
        let ctx = InverseContext::empty();
        let inv = inverse(&effect, &ctx).unwrap();

        // Inverse of inserting empty text is deleting an empty range.
        assert_eq!(
            inv,
            Effect::Delete {
                range: Range::from_raw(1, 1),
            }
        );
        assert!(verify_round_trip(before, &effect, &inv));
    }

    // ── Delete ↔ Insert round-trip ──────────────────────────────────────

    #[test]
    fn delete_inverse_is_insert() {
        let effect = Effect::Delete {
            range: Range::from_raw(5, 10),
        };
        let ctx = InverseContext::for_delete("hello");
        let inv = inverse(&effect, &ctx).expect("Delete with context should have inverse");

        assert_eq!(
            inv,
            Effect::Insert {
                offset: Offset::new(5),
                text: CompactString::new("hello"),
            }
        );
    }

    #[test]
    fn delete_inverse_round_trip() {
        let before = "Hello, beautiful world!";
        let deleted = "beautiful ";
        let effect = Effect::Delete {
            range: Range::from_raw(7, 17),
        };
        let ctx = InverseContext::for_delete(deleted);
        let inv = inverse(&effect, &ctx).unwrap();

        assert!(verify_round_trip(before, &effect, &inv));
    }

    #[test]
    fn delete_without_context_returns_none() {
        let effect = Effect::Delete {
            range: Range::from_raw(0, 5),
        };
        let ctx = InverseContext::empty();
        assert!(inverse(&effect, &ctx).is_none());
    }

    #[test]
    fn delete_at_start_round_trip() {
        let before = "Hello world";
        let effect = Effect::Delete {
            range: Range::from_raw(0, 6),
        };
        let ctx = InverseContext::for_delete("Hello ");
        let inv = inverse(&effect, &ctx).unwrap();

        assert!(verify_round_trip(before, &effect, &inv));
    }

    #[test]
    fn delete_at_end_round_trip() {
        let before = "Hello world";
        let effect = Effect::Delete {
            range: Range::from_raw(5, 11),
        };
        let ctx = InverseContext::for_delete(" world");
        let inv = inverse(&effect, &ctx).unwrap();

        assert!(verify_round_trip(before, &effect, &inv));
    }

    // ── Replace ↔ Replace round-trip ────────────────────────────────────

    #[test]
    fn replace_inverse_is_replace() {
        // Replace "world" (bytes 7..12) with "Rust" in "Hello, world!"
        let effect = Effect::Replace {
            range: Range::from_raw(7, 12),
            text: CompactString::new("Rust"),
        };
        let ctx = InverseContext::for_replace("world");
        let inv = inverse(&effect, &ctx).expect("Replace with context should have inverse");

        // Inverse replaces the new text "Rust" (bytes 7..11) with original "world".
        assert_eq!(
            inv,
            Effect::Replace {
                range: Range::from_raw(7, 11),
                text: CompactString::new("world"),
            }
        );
    }

    #[test]
    fn replace_inverse_round_trip() {
        let before = "Hello, world!";
        let effect = Effect::Replace {
            range: Range::from_raw(7, 12),
            text: CompactString::new("Rust"),
        };
        let ctx = InverseContext::for_replace("world");
        let inv = inverse(&effect, &ctx).unwrap();

        assert!(verify_round_trip(before, &effect, &inv));
    }

    #[test]
    fn replace_with_longer_text_round_trip() {
        let before = "ab";
        let effect = Effect::Replace {
            range: Range::from_raw(0, 2),
            text: CompactString::new("xyzw"),
        };
        let ctx = InverseContext::for_replace("ab");
        let inv = inverse(&effect, &ctx).unwrap();

        // After forward: "xyzw". Inverse range: 0..4, text: "ab".
        assert_eq!(
            inv,
            Effect::Replace {
                range: Range::from_raw(0, 4),
                text: CompactString::new("ab"),
            }
        );
        assert!(verify_round_trip(before, &effect, &inv));
    }

    #[test]
    fn replace_with_shorter_text_round_trip() {
        let before = "abcdef";
        let effect = Effect::Replace {
            range: Range::from_raw(1, 5),
            text: CompactString::new("X"),
        };
        let ctx = InverseContext::for_replace("bcde");
        let inv = inverse(&effect, &ctx).unwrap();

        // After forward: "aXf". Inverse range: 1..2, text: "bcde".
        assert_eq!(
            inv,
            Effect::Replace {
                range: Range::from_raw(1, 2),
                text: CompactString::new("bcde"),
            }
        );
        assert!(verify_round_trip(before, &effect, &inv));
    }

    #[test]
    fn replace_same_length_round_trip() {
        let before = "Hello";
        let effect = Effect::Replace {
            range: Range::from_raw(0, 5),
            text: CompactString::new("World"),
        };
        let ctx = InverseContext::for_replace("Hello");
        let inv = inverse(&effect, &ctx).unwrap();

        // Same-length replace: inverse range is identical.
        assert_eq!(
            inv,
            Effect::Replace {
                range: Range::from_raw(0, 5),
                text: CompactString::new("Hello"),
            }
        );
        assert!(verify_round_trip(before, &effect, &inv));
    }

    #[test]
    fn replace_without_context_returns_none() {
        let effect = Effect::Replace {
            range: Range::from_raw(0, 3),
            text: CompactString::new("X"),
        };
        let ctx = InverseContext::empty();
        assert!(inverse(&effect, &ctx).is_none());
    }

    // ── Replace with empty text (equivalent to Delete) ──────────────────

    #[test]
    fn replace_with_empty_replacement_round_trip() {
        let before = "abcdef";
        let effect = Effect::Replace {
            range: Range::from_raw(2, 4),
            text: CompactString::new(""),
        };
        let ctx = InverseContext::for_replace("cd");
        let inv = inverse(&effect, &ctx).unwrap();

        // After forward: "abef". Inverse: replace 2..2 with "cd".
        assert_eq!(
            inv,
            Effect::Replace {
                range: Range::from_raw(2, 2),
                text: CompactString::new("cd"),
            }
        );
        assert!(verify_round_trip(before, &effect, &inv));
    }

    #[test]
    fn replace_empty_range_with_text_round_trip() {
        // Replacing an empty range = pure insertion via Replace.
        let before = "ab";
        let effect = Effect::Replace {
            range: Range::from_raw(1, 1),
            text: CompactString::new("XY"),
        };
        let ctx = InverseContext::for_replace("");
        let inv = inverse(&effect, &ctx).unwrap();

        // After forward: "aXYb". Inverse: replace 1..3 with "".
        assert_eq!(
            inv,
            Effect::Replace {
                range: Range::from_raw(1, 3),
                text: CompactString::new(""),
            }
        );
        assert!(verify_round_trip(before, &effect, &inv));
    }

    // ── SetCursor inverse ───────────────────────────────────────────────

    #[test]
    fn set_cursor_inverse() {
        let effect = Effect::SetCursor {
            offset: Offset::new(42),
        };
        let ctx = InverseContext::for_cursor(Offset::new(10));
        let inv = inverse(&effect, &ctx).expect("SetCursor with context should have inverse");

        assert_eq!(
            inv,
            Effect::SetCursor {
                offset: Offset::new(10),
            }
        );
    }

    #[test]
    fn set_cursor_without_context_returns_none() {
        let effect = Effect::SetCursor {
            offset: Offset::new(42),
        };
        let ctx = InverseContext::empty();
        assert!(inverse(&effect, &ctx).is_none());
    }

    // ── SetMode inverse ─────────────────────────────────────────────────

    #[test]
    fn set_mode_inverse() {
        let effect = Effect::set_mode(Mode::Insert);
        let ctx = InverseContext::for_mode(Mode::Normal);
        let inv = inverse(&effect, &ctx).expect("SetMode with context should have inverse");

        assert_eq!(inv, Effect::set_mode(Mode::Normal));
    }

    #[test]
    fn set_mode_without_context_returns_none() {
        let effect = Effect::set_mode(Mode::Insert);
        let ctx = InverseContext::empty();
        assert!(inverse(&effect, &ctx).is_none());
    }

    // ── Non-mutating effects return None ────────────────────────────────

    #[test]
    fn show_info_has_no_inverse() {
        let effect = Effect::ShowInfo {
            info: crate::effects::InfoMessage::Text(CompactString::new("3 lines yanked")),
        };
        let ctx = InverseContext::empty();
        assert!(inverse(&effect, &ctx).is_none());
    }

    #[test]
    fn clear_message_has_no_inverse() {
        let effect = Effect::ClearMessage;
        let ctx = InverseContext::empty();
        assert!(inverse(&effect, &ctx).is_none());
    }

    #[test]
    fn clear_selection_without_context_returns_none() {
        // ClearSelection requires selection_before context to produce an inverse.
        let effect = Effect::ClearSelection;
        let ctx = InverseContext::empty();
        assert!(inverse(&effect, &ctx).is_none());
    }

    #[test]
    fn begin_undo_group_has_no_inverse() {
        let effect = Effect::BeginUndoGroup {
            cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
        };
        let ctx = InverseContext::empty();
        assert!(inverse(&effect, &ctx).is_none());
    }

    #[test]
    fn end_undo_group_has_no_inverse() {
        let effect = Effect::EndUndoGroup { node_id: None };
        let ctx = InverseContext::empty();
        assert!(inverse(&effect, &ctx).is_none());
    }

    #[test]
    fn scroll_to_has_no_inverse() {
        let effect = Effect::ScrollTo {
            offset: Offset::new(100),
        };
        let ctx = InverseContext::empty();
        assert!(inverse(&effect, &ctx).is_none());
    }

    #[test]
    fn window_split_has_no_inverse() {
        let effect = Effect::WindowSplit;
        let ctx = InverseContext::empty();
        assert!(inverse(&effect, &ctx).is_none());
    }

    #[test]
    fn set_selection_without_prior_selection_clears() {
        // SetSelection with no prior selection: inverse is ClearSelection.
        let effect = Effect::SetSelection {
            anchor: Offset::new(0),
            head: Offset::new(10),
            shape: crate::primitives::SelectionShape::Char,
        };
        let ctx = InverseContext::empty();
        assert_eq!(inverse(&effect, &ctx), Some(Effect::ClearSelection));
    }

    #[test]
    fn highlight_matches_has_no_inverse() {
        let effect = Effect::HighlightMatches {
            ranges: vec![Range::from_raw(0, 5)],
        };
        let ctx = InverseContext::empty();
        assert!(inverse(&effect, &ctx).is_none());
    }

    #[test]
    fn clear_highlights_has_no_inverse() {
        let effect = Effect::ClearHighlights;
        let ctx = InverseContext::empty();
        assert!(inverse(&effect, &ctx).is_none());
    }

    #[test]
    fn center_cursor_has_no_inverse() {
        let effect = Effect::CenterCursor;
        let ctx = InverseContext::empty();
        assert!(inverse(&effect, &ctx).is_none());
    }

    // ── SetRegister / ClearNamedRegister inverses ────────────────────────

    #[test]
    fn set_register_inverse_with_prev_content() {
        use crate::primitives::{MotionType, RegisterContent, RegisterName};
        let effect = Effect::SetRegister {
            name: RegisterName::new_unchecked('a'),
            text: CompactString::new("new text"),
            motion_type: MotionType::CharWise,
        };
        let prev = RegisterContent::char_wise("old text");
        let ctx = InverseContext::empty()
            .with_register_before(RegisterName::new_unchecked('a'), Some(prev));
        let inv = inverse(&effect, &ctx).expect("SetRegister with context should have inverse");
        assert_eq!(
            inv,
            Effect::SetRegister {
                name: RegisterName::new_unchecked('a'),
                text: CompactString::new("old text"),
                motion_type: MotionType::CharWise,
            }
        );
    }

    #[test]
    fn set_register_inverse_with_no_prev_content() {
        use crate::primitives::{MotionType, RegisterName};
        let effect = Effect::SetRegister {
            name: RegisterName::new_unchecked('b'),
            text: CompactString::new("something"),
            motion_type: MotionType::CharWise,
        };
        let ctx =
            InverseContext::empty().with_register_before(RegisterName::new_unchecked('b'), None);
        let inv = inverse(&effect, &ctx).expect("SetRegister should have inverse");
        assert_eq!(
            inv,
            Effect::ClearNamedRegister {
                register: RegisterName::new_unchecked('b')
            }
        );
    }

    #[test]
    fn set_register_without_context_returns_none() {
        use crate::primitives::{MotionType, RegisterName};
        let effect = Effect::SetRegister {
            name: RegisterName::new_unchecked('a'),
            text: CompactString::new("text"),
            motion_type: MotionType::CharWise,
        };
        let ctx = InverseContext::empty();
        assert!(inverse(&effect, &ctx).is_none());
    }

    #[test]
    fn clear_named_register_inverse_with_prev_content() {
        use crate::primitives::{MotionType, RegisterContent, RegisterName};
        let effect = Effect::ClearNamedRegister {
            register: RegisterName::new_unchecked('c'),
        };
        let prev = RegisterContent::line_wise("saved\n");
        let ctx = InverseContext::empty()
            .with_register_before(RegisterName::new_unchecked('c'), Some(prev));
        let inv = inverse(&effect, &ctx).expect("ClearNamedRegister with prev should have inverse");
        assert_eq!(
            inv,
            Effect::SetRegister {
                name: RegisterName::new_unchecked('c'),
                text: CompactString::new("saved\n"),
                motion_type: MotionType::LineWise,
            }
        );
    }

    #[test]
    fn clear_named_register_inverse_with_no_prev_content() {
        use crate::primitives::RegisterName;
        let effect = Effect::ClearNamedRegister {
            register: RegisterName::new_unchecked('d'),
        };
        let ctx =
            InverseContext::empty().with_register_before(RegisterName::new_unchecked('d'), None);
        let inv =
            inverse(&effect, &ctx).expect("ClearNamedRegister (was empty) should have inverse");
        assert_eq!(inv, Effect::Noop);
    }

    #[test]
    fn clear_named_register_without_context_returns_none() {
        use crate::primitives::RegisterName;
        let effect = Effect::ClearNamedRegister {
            register: RegisterName::new_unchecked('a'),
        };
        let ctx = InverseContext::empty();
        assert!(inverse(&effect, &ctx).is_none());
    }

    // ── SetMark / ClearMark inverses ─────────────────────────────────────

    #[test]
    fn set_mark_inverse_with_prev_mark() {
        use crate::primitives::{Mark, MarkName};
        let effect = Effect::SetMark {
            name: MarkName::new_unchecked('a'),
            offset: Offset::new(100),
            topline_offset: None,
        };
        let prev = Mark::new(Offset::new(50));
        let ctx =
            InverseContext::empty().with_mark_before(MarkName::new_unchecked('a'), Some(prev));
        let inv = inverse(&effect, &ctx).expect("SetMark with prev should have inverse");
        assert_eq!(
            inv,
            Effect::SetMark {
                name: MarkName::new_unchecked('a'),
                offset: Offset::new(50),
                topline_offset: None,
            }
        );
    }

    #[test]
    fn set_mark_inverse_with_no_prev_mark() {
        use crate::primitives::MarkName;
        let effect = Effect::SetMark {
            name: MarkName::new_unchecked('b'),
            offset: Offset::new(42),
            topline_offset: None,
        };
        let ctx = InverseContext::empty().with_mark_before(MarkName::new_unchecked('b'), None);
        let inv = inverse(&effect, &ctx).expect("SetMark (no prior) should have inverse");
        assert_eq!(
            inv,
            Effect::ClearMark {
                mark: MarkName::new_unchecked('b')
            }
        );
    }

    #[test]
    fn set_mark_without_context_returns_none() {
        use crate::primitives::MarkName;
        let effect = Effect::SetMark {
            name: MarkName::new_unchecked('a'),
            offset: Offset::new(0),
            topline_offset: None,
        };
        let ctx = InverseContext::empty();
        assert!(inverse(&effect, &ctx).is_none());
    }

    #[test]
    fn clear_mark_inverse_with_prev_mark() {
        use crate::primitives::{Mark, MarkName};
        let effect = Effect::ClearMark {
            mark: MarkName::new_unchecked('c'),
        };
        let prev = Mark::with_topline_offset(Offset::new(20), Some(5));
        let ctx =
            InverseContext::empty().with_mark_before(MarkName::new_unchecked('c'), Some(prev));
        let inv = inverse(&effect, &ctx).expect("ClearMark with prev should have inverse");
        assert_eq!(
            inv,
            Effect::SetMark {
                name: MarkName::new_unchecked('c'),
                offset: Offset::new(20),
                topline_offset: Some(5),
            }
        );
    }

    #[test]
    fn clear_mark_inverse_with_no_prev_mark() {
        use crate::primitives::MarkName;
        let effect = Effect::ClearMark {
            mark: MarkName::new_unchecked('d'),
        };
        let ctx = InverseContext::empty().with_mark_before(MarkName::new_unchecked('d'), None);
        let inv = inverse(&effect, &ctx).expect("ClearMark (was absent) should have inverse");
        assert_eq!(inv, Effect::Noop);
    }

    #[test]
    fn clear_mark_without_context_returns_none() {
        use crate::primitives::MarkName;
        let effect = Effect::ClearMark {
            mark: MarkName::new_unchecked('a'),
        };
        let ctx = InverseContext::empty();
        assert!(inverse(&effect, &ctx).is_none());
    }

    // ── SetSearchPattern inverse ─────────────────────────────────────────

    #[test]
    fn set_search_pattern_inverse() {
        use crate::primitives::{Direction, SearchDirection};
        let effect = Effect::SetSearchPattern {
            pattern: CompactString::new("new_pat"),
            direction: Direction::Forward,
        };
        let ctx = InverseContext::empty()
            .with_search_before(CompactString::new("old_pat"), SearchDirection::Backward);
        let inv =
            inverse(&effect, &ctx).expect("SetSearchPattern with context should have inverse");
        assert_eq!(
            inv,
            Effect::SetSearchPattern {
                pattern: CompactString::new("old_pat"),
                direction: Direction::Backward,
            }
        );
    }

    #[test]
    fn set_search_pattern_without_context_returns_none() {
        use crate::primitives::Direction;
        let effect = Effect::SetSearchPattern {
            pattern: CompactString::new("foo"),
            direction: Direction::Forward,
        };
        let ctx = InverseContext::empty();
        assert!(inverse(&effect, &ctx).is_none());
    }

    // ── SetLastFind inverse ──────────────────────────────────────────────

    #[test]
    fn set_last_find_inverse_with_prev() {
        use crate::primitives::{FindDirection, LastFind};
        let effect = Effect::SetLastFind {
            direction: FindDirection::FindForward,
            target_char: 'x',
            sneak_c2: None,
            resolved_ignorecase: false,
            resolved_smartcase: false,
        };
        let mut prev = LastFind::new();
        prev.record(FindDirection::FindBackward, 'z');
        let ctx = InverseContext::empty().with_last_find_before(prev);
        let inv = inverse(&effect, &ctx).expect("SetLastFind with context should have inverse");
        assert_eq!(
            inv,
            Effect::SetLastFind {
                direction: FindDirection::FindBackward,
                target_char: 'z',
                sneak_c2: None,
                resolved_ignorecase: false,
                resolved_smartcase: false,
            }
        );
    }

    #[test]
    fn set_last_find_inverse_with_empty_prev() {
        use crate::primitives::{FindDirection, LastFind};
        let effect = Effect::SetLastFind {
            direction: FindDirection::FindForward,
            target_char: 'a',
            sneak_c2: None,
            resolved_ignorecase: false,
            resolved_smartcase: false,
        };
        let prev = LastFind::new(); // empty
        let ctx = InverseContext::empty().with_last_find_before(prev);
        let inv = inverse(&effect, &ctx).expect("SetLastFind with empty prev should have inverse");
        assert_eq!(inv, Effect::Noop);
    }

    #[test]
    fn set_last_find_without_context_returns_none() {
        use crate::primitives::FindDirection;
        let effect = Effect::SetLastFind {
            direction: FindDirection::TillForward,
            target_char: 'q',
            sneak_c2: None,
            resolved_ignorecase: false,
            resolved_smartcase: false,
        };
        let ctx = InverseContext::empty();
        assert!(inverse(&effect, &ctx).is_none());
    }

    // ── SetSelection / ClearSelection inverses ───────────────────────────

    #[test]
    fn set_selection_inverse_with_prev_selection() {
        use crate::primitives::{SelectionRange, SelectionShape};
        let effect = Effect::SetSelection {
            anchor: Offset::new(10),
            head: Offset::new(20),
            shape: SelectionShape::Char,
        };
        let prev = SelectionRange::new(Offset::new(3), Offset::new(7));
        let ctx = InverseContext::empty().with_selection_before(prev);
        let inv = inverse(&effect, &ctx).expect("SetSelection with prev should have inverse");
        assert_eq!(
            inv,
            Effect::SetSelection {
                anchor: Offset::new(3),
                head: Offset::new(7),
                shape: SelectionShape::Char,
            }
        );
    }

    #[test]
    fn clear_selection_inverse_with_prev_selection() {
        use crate::primitives::{SelectionRange, SelectionShape};
        let effect = Effect::ClearSelection;
        let prev = SelectionRange::new(Offset::new(5), Offset::new(15));
        let ctx = InverseContext::empty().with_selection_before(prev);
        let inv = inverse(&effect, &ctx).expect("ClearSelection with prev should have inverse");
        assert_eq!(
            inv,
            Effect::SetSelection {
                anchor: Offset::new(5),
                head: Offset::new(15),
                shape: SelectionShape::Char,
            }
        );
    }

    // ── Noop inverse ─────────────────────────────────────────────────────

    #[test]
    fn noop_inverse_is_noop() {
        let effect = Effect::Noop;
        let ctx = InverseContext::empty();
        let inv = inverse(&effect, &ctx).expect("Noop should have an inverse");
        assert_eq!(inv, Effect::Noop);
    }

    // ── InverseContext builder (new fields) ──────────────────────────────

    #[test]
    fn context_builder_new_fields_compose() {
        use crate::primitives::{
            FindDirection, LastFind, Mark, MarkName, RegisterContent, RegisterName,
            SearchDirection, SelectionRange,
        };
        let mut prev_find = LastFind::new();
        prev_find.record(FindDirection::TillForward, 'q');

        let ctx = InverseContext::empty()
            .with_register_before(
                RegisterName::new_unchecked('a'),
                Some(RegisterContent::char_wise("hi")),
            )
            .with_mark_before(
                MarkName::new_unchecked('b'),
                Some(Mark::new(Offset::new(10))),
            )
            .with_search_before(CompactString::new("pat"), SearchDirection::Forward)
            .with_last_find_before(prev_find)
            .with_selection_before(SelectionRange::new(Offset::new(0), Offset::new(5)));

        assert!(ctx.register_before.is_some());
        assert!(ctx.mark_before.is_some());
        assert!(ctx.search_pattern_before.is_some());
        assert!(ctx.last_find_before.is_some());
        assert!(ctx.selection_before.is_some());
    }

    // ── make_reversible ─────────────────────────────────────────────────

    #[test]
    fn make_reversible_insert() {
        let effect = Effect::Insert {
            offset: Offset::new(0),
            text: CompactString::new("abc"),
        };
        let ctx = InverseContext::empty();
        let rev = make_reversible(&effect, &ctx).expect("Insert should be reversible");

        assert_eq!(rev.effect, effect);
        assert_eq!(
            rev.inverse,
            Effect::Delete {
                range: Range::from_raw(0, 3),
            }
        );
    }

    #[test]
    fn make_reversible_delete_with_context() {
        let effect = Effect::Delete {
            range: Range::from_raw(2, 5),
        };
        let ctx = InverseContext::for_delete("xyz");
        let rev = make_reversible(&effect, &ctx).expect("Delete with context should be reversible");

        assert_eq!(rev.effect, effect);
        assert_eq!(
            rev.inverse,
            Effect::Insert {
                offset: Offset::new(2),
                text: CompactString::new("xyz"),
            }
        );
    }

    #[test]
    fn make_reversible_returns_none_for_non_mutating() {
        let effect = Effect::ClearMessage;
        let ctx = InverseContext::empty();
        assert!(make_reversible(&effect, &ctx).is_none());
    }

    #[test]
    fn make_reversible_returns_none_when_context_missing() {
        let effect = Effect::Delete {
            range: Range::from_raw(0, 5),
        };
        let ctx = InverseContext::empty();
        assert!(make_reversible(&effect, &ctx).is_none());
    }

    // ── Double inverse (inverse of inverse = original) ──────────────────

    #[test]
    fn insert_double_inverse_is_identity() {
        let original = Effect::Insert {
            offset: Offset::new(3),
            text: CompactString::new("hello"),
        };
        let ctx = InverseContext::empty();
        let inv = inverse(&original, &ctx).unwrap();

        // The inverse is Delete{3..8}. Now invert the delete.
        let ctx2 = InverseContext::for_delete("hello");
        let double_inv = inverse(&inv, &ctx2).unwrap();

        assert_eq!(original, double_inv);
    }

    #[test]
    fn delete_double_inverse_is_identity() {
        let original = Effect::Delete {
            range: Range::from_raw(5, 10),
        };
        let deleted_text = "world";
        let ctx = InverseContext::for_delete(deleted_text);
        let inv = inverse(&original, &ctx).unwrap();

        // The inverse is Insert{5, "world"}. Now invert the insert.
        let ctx2 = InverseContext::empty();
        let double_inv = inverse(&inv, &ctx2).unwrap();

        assert_eq!(original, double_inv);
    }

    #[test]
    fn replace_double_inverse_is_identity() {
        // Replace "abc" (0..3) with "XY" → document changes from "abc..." to "XY..."
        let original = Effect::Replace {
            range: Range::from_raw(0, 3),
            text: CompactString::new("XY"),
        };
        let ctx = InverseContext::for_replace("abc");
        let inv = inverse(&original, &ctx).unwrap();

        // inv is Replace{0..2, "abc"}. Invert that.
        let ctx2 = InverseContext::for_replace("XY");
        let double_inv = inverse(&inv, &ctx2).unwrap();

        assert_eq!(original, double_inv);
    }

    // ── Unicode / multi-byte text ───────────────────────────────────────

    #[test]
    fn insert_unicode_round_trip() {
        let before = "cafe";
        let effect = Effect::Insert {
            offset: Offset::new(4),
            // 2-byte UTF-8 character
            text: CompactString::new("\u{0301}"),
        };
        let ctx = InverseContext::empty();
        let inv = inverse(&effect, &ctx).unwrap();

        assert!(verify_round_trip(before, &effect, &inv));
    }

    #[test]
    fn delete_emoji_round_trip() {
        // Emoji is 4 bytes in UTF-8.
        let before = "a\u{1F600}b";
        let emoji = "\u{1F600}";
        let start = 1; // byte offset after 'a'
        let end = start + emoji.len(); // 1 + 4 = 5
        let effect = Effect::Delete {
            range: Range::from_raw(start, end),
        };
        let ctx = InverseContext::for_delete(emoji);
        let inv = inverse(&effect, &ctx).unwrap();

        assert!(verify_round_trip(before, &effect, &inv));
    }

    #[test]
    fn replace_multibyte_round_trip() {
        let before = "\u{00E9}lite"; // "elite" with e-acute (2 bytes)
        let effect = Effect::Replace {
            range: Range::from_raw(0, 2),  // replace 2-byte char
            text: CompactString::new("e"), // with 1-byte char
        };
        let ctx = InverseContext::for_replace("\u{00E9}");
        let inv = inverse(&effect, &ctx).unwrap();

        assert!(verify_round_trip(before, &effect, &inv));
    }

    // ── InverseContext builder ───────────────────────────────────────────

    #[test]
    fn context_builder_composes() {
        let ctx = InverseContext::empty()
            .with_deleted_text("del")
            .with_replaced_text("repl")
            .with_cursor_before(Offset::new(42))
            .with_mode_before(Mode::Insert);

        assert_eq!(ctx.deleted_text.as_deref(), Some("del"));
        assert_eq!(ctx.replaced_text.as_deref(), Some("repl"));
        assert_eq!(ctx.cursor_before, Some(Offset::new(42)));
        assert_eq!(ctx.mode_before, Some(Mode::Insert));
    }

    #[test]
    fn context_default_is_empty() {
        let ctx = InverseContext::default();
        assert!(ctx.deleted_text.is_none());
        assert!(ctx.replaced_text.is_none());
        assert!(ctx.cursor_before.is_none());
        assert!(ctx.mode_before.is_none());
        assert!(ctx.register_before.is_none());
        assert!(ctx.mark_before.is_none());
        assert!(ctx.search_pattern_before.is_none());
        assert!(ctx.last_find_before.is_none());
        assert!(ctx.selection_before.is_none());
    }

    // ── Convenience constructors ────────────────────────────────────────

    #[test]
    fn for_delete_sets_field() {
        let ctx = InverseContext::for_delete("abc");
        assert_eq!(ctx.deleted_text.as_deref(), Some("abc"));
        assert!(ctx.replaced_text.is_none());
    }

    #[test]
    fn for_replace_sets_field() {
        let ctx = InverseContext::for_replace("xyz");
        assert_eq!(ctx.replaced_text.as_deref(), Some("xyz"));
        assert!(ctx.deleted_text.is_none());
    }

    #[test]
    fn for_cursor_sets_field() {
        let ctx = InverseContext::for_cursor(Offset::new(99));
        assert_eq!(ctx.cursor_before, Some(Offset::new(99)));
    }

    #[test]
    fn for_mode_sets_field() {
        let ctx = InverseContext::for_mode(Mode::Replace);
        assert_eq!(ctx.mode_before, Some(Mode::Replace));
    }

    // ── ReversibleEffect ────────────────────────────────────────────────

    #[test]
    fn reversible_effect_new() {
        let fwd = Effect::Insert {
            offset: Offset::new(0),
            text: CompactString::new("x"),
        };
        let inv = Effect::Delete {
            range: Range::from_raw(0, 1),
        };
        let rev = ReversibleEffect::new(fwd.clone(), inv.clone());
        assert_eq!(rev.effect, fwd);
        assert_eq!(rev.inverse, inv);
    }

    // ── Edge cases ──────────────────────────────────────────────────────

    #[test]
    fn insert_single_char_round_trip() {
        let before = "";
        let effect = Effect::Insert {
            offset: Offset::new(0),
            text: CompactString::new("x"),
        };
        let ctx = InverseContext::empty();
        let inv = inverse(&effect, &ctx).unwrap();
        assert!(verify_round_trip(before, &effect, &inv));
    }

    #[test]
    fn delete_entire_document_round_trip() {
        let before = "Hello, world!";
        let effect = Effect::Delete {
            range: Range::from_raw(0, 13),
        };
        let ctx = InverseContext::for_delete("Hello, world!");
        let inv = inverse(&effect, &ctx).unwrap();
        assert!(verify_round_trip(before, &effect, &inv));
    }

    #[test]
    fn replace_entire_document_round_trip() {
        let before = "abc";
        let effect = Effect::Replace {
            range: Range::from_raw(0, 3),
            text: CompactString::new("longer replacement text"),
        };
        let ctx = InverseContext::for_replace("abc");
        let inv = inverse(&effect, &ctx).unwrap();
        assert!(verify_round_trip(before, &effect, &inv));
    }

    #[test]
    fn set_cursor_to_same_position() {
        let effect = Effect::SetCursor {
            offset: Offset::new(5),
        };
        let ctx = InverseContext::for_cursor(Offset::new(5));
        let inv = inverse(&effect, &ctx).unwrap();
        assert_eq!(
            inv,
            Effect::SetCursor {
                offset: Offset::new(5)
            }
        );
    }

    #[test]
    fn set_mode_to_same_mode() {
        let effect = Effect::set_mode(Mode::Normal);
        let ctx = InverseContext::for_mode(Mode::Normal);
        let inv = inverse(&effect, &ctx).unwrap();
        assert_eq!(inv, Effect::set_mode(Mode::Normal));
    }

    // ── Newline / whitespace handling ───────────────────────────────────

    #[test]
    fn insert_newline_round_trip() {
        let before = "line1line2";
        let effect = Effect::Insert {
            offset: Offset::new(5),
            text: CompactString::new("\n"),
        };
        let ctx = InverseContext::empty();
        let inv = inverse(&effect, &ctx).unwrap();
        assert!(verify_round_trip(before, &effect, &inv));
    }

    #[test]
    fn delete_newline_round_trip() {
        let before = "line1\nline2";
        let effect = Effect::Delete {
            range: Range::from_raw(5, 6),
        };
        let ctx = InverseContext::for_delete("\n");
        let inv = inverse(&effect, &ctx).unwrap();
        assert!(verify_round_trip(before, &effect, &inv));
    }

    #[test]
    fn replace_with_newlines_round_trip() {
        let before = "abc";
        let effect = Effect::Replace {
            range: Range::from_raw(1, 2),
            text: CompactString::new("\n\n\n"),
        };
        let ctx = InverseContext::for_replace("b");
        let inv = inverse(&effect, &ctx).unwrap();
        assert!(verify_round_trip(before, &effect, &inv));
    }

    // ── Multiline text operations ───────────────────────────────────────

    #[test]
    fn insert_multiline_block_round_trip() {
        let before = "fn main() {\n}\n";
        let effect = Effect::Insert {
            offset: Offset::new(12),
            text: CompactString::new("    println!(\"Hello\");\n"),
        };
        let ctx = InverseContext::empty();
        let inv = inverse(&effect, &ctx).unwrap();
        assert!(verify_round_trip(before, &effect, &inv));
    }

    #[test]
    fn delete_multiline_round_trip() {
        let before = "line1\nline2\nline3\n";
        let effect = Effect::Delete {
            range: Range::from_raw(6, 12),
        };
        let ctx = InverseContext::for_delete("line2\n");
        let inv = inverse(&effect, &ctx).unwrap();
        assert!(verify_round_trip(before, &effect, &inv));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // adjust_offset
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn adjust_offset_insert_before_offset_shifts_right() {
        let ins = Effect::Insert {
            offset: Offset::new(5),
            text: CompactString::new("abc"),
        };
        // offset 8 >= at 5 → shifts right by 3 → 11
        assert_eq!(super::adjust_offset(Offset::new(8), &ins), Offset::new(11));
    }

    #[test]
    fn adjust_offset_insert_after_offset_unchanged() {
        let ins = Effect::Insert {
            offset: Offset::new(5),
            text: CompactString::new("abc"),
        };
        // offset 3 < at 5 → unchanged
        assert_eq!(super::adjust_offset(Offset::new(3), &ins), Offset::new(3));
    }

    #[test]
    fn adjust_offset_insert_at_boundary_shifts_right() {
        // offset == at → shifts right (>= check)
        let ins = Effect::Insert {
            offset: Offset::new(5),
            text: CompactString::new("x"),
        };
        assert_eq!(super::adjust_offset(Offset::new(5), &ins), Offset::new(6));
    }

    #[test]
    fn adjust_offset_delete_before_range_unchanged() {
        let del = Effect::Delete {
            range: Range::from_raw(5, 10),
        };
        assert_eq!(super::adjust_offset(Offset::new(3), &del), Offset::new(3));
    }

    #[test]
    fn adjust_offset_delete_inside_range_clamps_to_start() {
        let del = Effect::Delete {
            range: Range::from_raw(5, 10),
        };
        // offset 7 is in [5, 10) → clamp to 5
        assert_eq!(super::adjust_offset(Offset::new(7), &del), Offset::new(5));
    }

    #[test]
    fn adjust_offset_delete_after_range_shifts_left() {
        let del = Effect::Delete {
            range: Range::from_raw(5, 10),
        };
        // offset 12 >= end 10 → shift left by len 5 → 7
        assert_eq!(super::adjust_offset(Offset::new(12), &del), Offset::new(7));
    }

    #[test]
    fn adjust_offset_replace_before_range_unchanged() {
        let rep = Effect::Replace {
            range: Range::from_raw(5, 10),
            text: CompactString::new("ab"),
        };
        assert_eq!(super::adjust_offset(Offset::new(3), &rep), Offset::new(3));
    }

    #[test]
    fn adjust_offset_replace_inside_range_clamps_then_shifts() {
        // Delete 5..10 (len 5), insert "ab" (len 2) at 5.
        // offset 7 → after delete: clamped to 5 → after insert: 5 >= 5, +2 → 7
        let rep = Effect::Replace {
            range: Range::from_raw(5, 10),
            text: CompactString::new("ab"),
        };
        assert_eq!(super::adjust_offset(Offset::new(7), &rep), Offset::new(7));
    }

    #[test]
    fn adjust_offset_replace_after_range() {
        // Delete 5..10 (len 5), insert "ab" (len 2) at 5.
        // offset 12 → after delete: 12 - 5 = 7 → after insert: 7 >= 5, +2 → 9
        let rep = Effect::Replace {
            range: Range::from_raw(5, 10),
            text: CompactString::new("ab"),
        };
        assert_eq!(super::adjust_offset(Offset::new(12), &rep), Offset::new(9));
    }

    #[test]
    fn adjust_offset_noop_unchanged() {
        let effect = Effect::Noop;
        assert_eq!(
            super::adjust_offset(Offset::new(42), &effect),
            Offset::new(42)
        );
    }

    #[test]
    fn adjust_offset_set_mode_unchanged() {
        let effect = Effect::set_mode(Mode::Insert);
        assert_eq!(
            super::adjust_offset(Offset::new(10), &effect),
            Offset::new(10)
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // compute_length_change
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn compute_length_change_empty_slice() {
        assert_eq!(super::compute_length_change(&[]), 0);
    }

    #[test]
    fn compute_length_change_insert_only() {
        let effects = vec![Effect::Insert {
            offset: Offset::new(0),
            text: CompactString::new("hello"),
        }];
        assert_eq!(super::compute_length_change(&effects), 5);
    }

    #[test]
    fn compute_length_change_delete_only() {
        let effects = vec![Effect::Delete {
            range: Range::from_raw(0, 3),
        }];
        assert_eq!(super::compute_length_change(&effects), -3);
    }

    #[test]
    fn compute_length_change_replace() {
        // Replace 5 bytes with 2 → net -3
        let effects = vec![Effect::Replace {
            range: Range::from_raw(0, 5),
            text: CompactString::new("ab"),
        }];
        assert_eq!(super::compute_length_change(&effects), -3);
    }

    #[test]
    fn compute_length_change_mixed() {
        let effects = vec![
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::new("hello"),
            }, // +5
            Effect::Delete {
                range: Range::from_raw(0, 2),
            }, // -2
            Effect::SetCursor {
                offset: Offset::new(3),
            }, // 0
        ];
        assert_eq!(super::compute_length_change(&effects), 3);
    }

    #[test]
    fn compute_length_change_non_mutating_effects_contribute_zero() {
        let effects = vec![Effect::Noop, Effect::set_mode(Mode::Insert)];
        assert_eq!(super::compute_length_change(&effects), 0);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // rebase_effects_by_delta
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn rebase_effects_by_delta_empty_slice() {
        assert!(super::rebase_effects_by_delta(&[], 10).is_empty());
    }

    #[test]
    fn rebase_effects_by_delta_positive_insert() {
        let effects = vec![Effect::Insert {
            offset: Offset::new(5),
            text: CompactString::new("x"),
        }];
        let result = super::rebase_effects_by_delta(&effects, 3);
        assert_eq!(
            result[0],
            Effect::Insert {
                offset: Offset::new(8),
                text: CompactString::new("x")
            }
        );
    }

    #[test]
    fn rebase_effects_by_delta_negative_insert() {
        let effects = vec![Effect::Insert {
            offset: Offset::new(10),
            text: CompactString::new("y"),
        }];
        let result = super::rebase_effects_by_delta(&effects, -4);
        assert_eq!(
            result[0],
            Effect::Insert {
                offset: Offset::new(6),
                text: CompactString::new("y")
            }
        );
    }

    #[test]
    fn rebase_effects_by_delta_delete() {
        let effects = vec![Effect::Delete {
            range: Range::from_raw(3, 7),
        }];
        let result = super::rebase_effects_by_delta(&effects, 2);
        assert_eq!(
            result[0],
            Effect::Delete {
                range: Range::from_raw(5, 9)
            }
        );
    }

    #[test]
    fn rebase_effects_by_delta_set_cursor() {
        let effects = vec![Effect::SetCursor {
            offset: Offset::new(5),
        }];
        let result = super::rebase_effects_by_delta(&effects, 3);
        assert_eq!(
            result[0],
            Effect::SetCursor {
                offset: Offset::new(8)
            }
        );
    }

    #[test]
    fn rebase_effects_by_delta_non_positional_passthrough() {
        let effects = vec![Effect::set_mode(Mode::Insert), Effect::Noop];
        let result = super::rebase_effects_by_delta(&effects, 100);
        assert_eq!(result, effects);
    }

    #[test]
    fn rebase_effects_by_delta_saturates_at_zero() {
        let effects = vec![Effect::Insert {
            offset: Offset::new(2),
            text: CompactString::new("a"),
        }];
        // delta -10 would underflow usize — saturate at 0
        let result = super::rebase_effects_by_delta(&effects, -10);
        assert_eq!(
            result[0],
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::new("a")
            }
        );
    }
}
