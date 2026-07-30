//! Action effect builders.
//!
//! Pure effect construction for mode transitions and macro operations.
//! These are stateless functions that take primitive parameters and return
//! `Effects` — they don't need an `ActionContext` because they don't
//! operate on document text.
//!
//! The file layout for `actions/`, as described in `commands/mod.rs`:
//! - **Command files** (`*.rs`): `fn execute_x(&ActionContext) → CommandResult`
//! - **Effect builders** (`effects.rs`): `fn name(primitives) → Effects`

use compact_str::CompactString;

use crate::effects::undo_state::EffectState;
use crate::effects::Effects;
use crate::primitives::Mode;
use crate::primitives::{Direction, MotionType, RegisterName};

// ═══════════════════════════════════════════════════════════════════════════
// Mode transitions
// ═══════════════════════════════════════════════════════════════════════════

/// Switch to a new mode.
///
/// Pure effect: emits `SetMode`. The execution layer delegates here
/// instead of constructing `Effect::SetMode` inline.
pub fn switch_mode(mode: Mode) -> Effects {
    Effects::new().set_mode(mode)
}

/// Exit visual mode to normal, clearing selection.
///
/// Used by `gq` (format) and similar commands that need to leave
/// visual mode without the full visual exit ceremony.
pub fn exit_visual_to_normal() -> Effects {
    Effects::new().clear_selection().set_mode(Mode::Normal)
}

/// Enter replace mode with an undo group.
///
/// Replace mode behaves like insert mode for undo purposes:
/// it starts a new undo group so that `u` reverts all replacements.
pub fn enter_replace() -> Effects {
    Effects::new()
        .begin_undo()
        .set_mode(Mode::Replace)
        .into_raw_closed()
}

// ═══════════════════════════════════════════════════════════════════════════
// Macro operations
// ═══════════════════════════════════════════════════════════════════════════

/// Start recording a macro into a register.
pub fn start_recording(register: RegisterName) -> Effects {
    Effects::new().start_recording(register)
}

/// Stop the current macro recording.
pub fn stop_recording() -> Effects {
    Effects::new().stop_recording().clear_message()
}

/// Play a recorded macro from the given register.
pub fn play_macro(register: RegisterName, count: u32) -> Effects {
    Effects::new().play_macro(register, count)
}

// ═══════════════════════════════════════════════════════════════════════════
// Register routing for action-level deletes
// ═══════════════════════════════════════════════════════════════════════════

/// Route deleted text to registers for action-level deletes (x, X, s, D, C).
///
/// Canonical register routing per Vim spec:
/// - **Blackhole register** (`"_`): set blackhole only, skip unnamed
/// - **Explicit register** (`"a`): set register + unnamed
/// - **No register + multiline**: set numbered_1 + unnamed
/// - **No register + single-line**: set small_delete + unnamed
///
/// All three action-delete files (`delete_char.rs`, `delete_to_end.rs`,
/// `substitute.rs`) delegate here to avoid divergent routing logic.
pub fn route_action_delete_registers<S: EffectState>(
    effects: Effects<S>,
    register: Option<RegisterName>,
    text: &str,
    motion_type: MotionType,
    is_multiline: bool,
) -> Effects<S> {
    match register {
        // Blackhole register: suppress ALL register updates (consistent with
        // operators/registers.rs::route_delete_registers)
        Some(reg) if reg.is_blackhole() => effects,
        Some(reg) => effects.set_register(reg, text, motion_type).set_register(
            RegisterName::UNNAMED,
            text,
            motion_type,
        ),
        None if is_multiline => effects
            .set_register(RegisterName::NUMBERED_1, text, motion_type)
            .set_register(RegisterName::UNNAMED, text, motion_type),
        None => effects
            .set_register(RegisterName::SMALL_DELETE, text, motion_type)
            .set_register(RegisterName::UNNAMED, text, motion_type),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Star search
// ═══════════════════════════════════════════════════════════════════════════

/// Compute star search effects for word under cursor (normal mode).
///
/// Wraps the cursor word in `\<...\>` word boundary markers.
/// Sets both the search pattern and the `/` register.
///
/// Returns empty effects if no word is found under cursor.
pub fn star_search_normal(text: &str, cursor: usize, direction: Direction) -> Effects {
    let (word, _) = crate::commands::motions::word_under_cursor_or_next(
        text,
        cursor,
        &crate::primitives::WordCharSet::default_vim(),
    );
    if word.is_empty() {
        return Effects::new();
    }

    let escaped = crate::commands::helpers::vim_regex_escape(word);
    // Use CompactString SSO (24 bytes inline) to avoid heap allocation
    // for typical word lengths (<=20 chars).
    let mut pattern = compact_str::CompactString::with_capacity(escaped.len() + 4);
    pattern.push_str("\\<");
    pattern.push_str(&escaped);
    pattern.push_str("\\>");
    Effects::new()
        .set_search_pattern(pattern.as_str(), direction)
        .set_register(RegisterName::SEARCH, pattern, MotionType::CharWise)
}

/// Compute partial word star search effects (g*/g#) — no word boundaries.
pub fn partial_star_search_normal(text: &str, cursor: usize, direction: Direction) -> Effects {
    let (word, _) = crate::commands::motions::word_under_cursor_or_next(
        text,
        cursor,
        &crate::primitives::WordCharSet::default_vim(),
    );
    if word.is_empty() {
        return Effects::new();
    }

    let pattern = crate::commands::helpers::vim_regex_escape(word);
    Effects::new()
        .set_search_pattern(pattern.as_str(), direction)
        .set_register(RegisterName::SEARCH, pattern, MotionType::CharWise)
}

/// Compute star search effects for visual selection.
///
/// Uses `\V` (very nomagic) prefix so literal text is matched.
/// Also exits visual mode (clears selection, sets Normal mode).
///
/// Returns empty effects if selection is empty.
pub fn star_search_visual(text: &str, start: usize, end: usize, direction: Direction) -> Effects {
    let end_inclusive = crate::commands::helpers::next_char_boundary(text, end);
    let selected = text.get(start..end_inclusive).unwrap_or("");
    if selected.is_empty() {
        return Effects::new();
    }

    let mut pattern = CompactString::with_capacity(selected.len() + 2);
    pattern.push_str("\\V");
    pattern.push_str(selected);
    Effects::new()
        .set_search_pattern(pattern.as_str(), direction)
        .set_register(RegisterName::SEARCH, pattern, MotionType::CharWise)
        .set_mark(
            crate::primitives::MarkName::VISUAL_START,
            crate::primitives::Offset::new(start),
            None,
        )
        .set_mark(
            crate::primitives::MarkName::VISUAL_END,
            crate::primitives::Offset::new(end),
            None,
        )
        .clear_selection()
        .set_mode(Mode::Normal)
}
