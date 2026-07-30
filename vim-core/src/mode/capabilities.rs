//! Capability types and mode profiles for Vim mode subsystem.
//!
//! `CapabilitySet` encodes which features a mode supports (motions, operators,
//! text objects, etc.) as a bitfield. `ModeProfile` pairs a `CapabilitySet`
//! with a priority ordering and a fallthrough flag.
//!
//! Const profiles are provided for all seven standard Vim modes plus the
//! synthetic `MOTION_ONLY` profile used in restricted contexts.

use crate::keymap::KeyEvent;
use crate::primitives::Mode;

use super::types::{ModeAction, ModeContext};

use bitflags::bitflags;

// ═══════════════════════════════════════════════════════════════════════════
// Capability enum — ordered list of individual capabilities
// ═══════════════════════════════════════════════════════════════════════════

/// An individual Vim capability.
///
/// Used to express a priority ordering within a [`ModeProfile`]. The enum
/// variant order does **not** determine the bitflag bit position; see
/// [`CapabilitySet`] for that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Capability {
    /// Motion commands (h, j, k, l, w, b, …).
    Motions,
    /// Operator commands (d, c, y, …).
    Operators,
    /// Text-object selectors (iw, aw, i(, …).
    TextObjects,
    /// Action commands (p, u, `<C-r>`, ZZ, …).
    Actions,
    /// Insert-mode text entry.
    Insert,
    /// Visual/selection operations.
    Selection,
    /// Scroll commands (`<C-f>`, `<C-b>`, zz, …).
    Scroll,
    /// Window/split management (`<C-w>`…).
    Window,
    /// Count prefix (1–9 followed by command).
    Count,
    /// Register specification ("a…).
    Register,
    /// Mark operations (m, ', `).
    Marks,
}

// ═══════════════════════════════════════════════════════════════════════════
// CapabilitySet — bitfield of active capabilities
// ═══════════════════════════════════════════════════════════════════════════

bitflags! {
    /// A set of [`Capability`] values encoded as a bitfield.
    ///
    /// Use the associated constants (e.g., `CapabilitySet::MOTIONS`) to build
    /// sets with `|`.
    ///
    /// # Example
    /// ```
    /// use vim_core::mode::capabilities::CapabilitySet;
    /// let set = CapabilitySet::MOTIONS | CapabilitySet::COUNT;
    /// assert!(set.contains(CapabilitySet::MOTIONS));
    /// assert!(!set.contains(CapabilitySet::INSERT));
    /// ```
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct CapabilitySet: u16 {
        /// Motion commands.
        const MOTIONS      = 1 << 0;
        /// Operator commands.
        const OPERATORS    = 1 << 1;
        /// Text-object selectors.
        const TEXT_OBJECTS = 1 << 2;
        /// Action commands.
        const ACTIONS      = 1 << 3;
        /// Insert-mode text entry.
        const INSERT       = 1 << 4;
        /// Visual/selection operations.
        const SELECTION    = 1 << 5;
        /// Scroll commands.
        const SCROLL       = 1 << 6;
        /// Window/split management.
        const WINDOW       = 1 << 7;
        /// Count prefix.
        const COUNT        = 1 << 8;
        /// Register specification.
        const REGISTER     = 1 << 9;
        /// Mark operations.
        const MARKS        = 1 << 10;
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ModeProfile — full descriptor for a mode
// ═══════════════════════════════════════════════════════════════════════════

/// Descriptor for a Vim mode.
///
/// Combines a [`CapabilitySet`] (which features are active) with a static
/// priority slice (preferred parse order) and a `fallthrough_to_host` flag
/// (whether unhandled keys should be forwarded to the host application).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeProfile {
    /// Which capabilities are active in this mode.
    pub capabilities: CapabilitySet,
    /// Priority ordering used during grammar resolution.
    ///
    /// Earlier entries are tried first when a key matches multiple categories.
    pub priority: &'static [Capability],
    /// If `true`, keys not consumed by this mode are forwarded to the host.
    pub fallthrough_to_host: bool,
}

// ═══════════════════════════════════════════════════════════════════════════
// Const profiles for all standard modes
// ═══════════════════════════════════════════════════════════════════════════

/// Profile for Normal mode.
///
/// All non-insert capabilities are active. Keys are fully consumed; nothing
/// falls through to the host.
pub const NORMAL: ModeProfile = ModeProfile {
    capabilities: CapabilitySet::COUNT
        .union(CapabilitySet::REGISTER)
        .union(CapabilitySet::OPERATORS)
        .union(CapabilitySet::MOTIONS)
        .union(CapabilitySet::ACTIONS)
        .union(CapabilitySet::MARKS)
        .union(CapabilitySet::SCROLL)
        .union(CapabilitySet::WINDOW),
    priority: &[
        Capability::Count,
        Capability::Register,
        Capability::Operators,
        Capability::Motions,
        Capability::Actions,
        Capability::Marks,
        Capability::Scroll,
        Capability::Window,
    ],
    fallthrough_to_host: false,
};

/// Profile for Visual mode (char, line, and block sub-modes share this).
///
/// Adds `Selection` on top of Normal capabilities. Fallthrough disabled.
pub const VISUAL: ModeProfile = ModeProfile {
    capabilities: CapabilitySet::COUNT
        .union(CapabilitySet::REGISTER)
        .union(CapabilitySet::OPERATORS)
        .union(CapabilitySet::MOTIONS)
        .union(CapabilitySet::SELECTION)
        .union(CapabilitySet::ACTIONS)
        .union(CapabilitySet::MARKS)
        .union(CapabilitySet::SCROLL)
        .union(CapabilitySet::WINDOW),
    priority: &[
        Capability::Count,
        Capability::Register,
        Capability::Operators,
        Capability::Motions,
        Capability::Selection,
        Capability::Actions,
        Capability::Marks,
        Capability::Scroll,
        Capability::Window,
    ],
    fallthrough_to_host: false,
};

/// Profile for Operator-pending mode.
///
/// Only the capabilities needed to complete an operator are active.
/// No register, no actions, no window management. Fallthrough disabled.
pub const OPERATOR_PENDING: ModeProfile = ModeProfile {
    capabilities: CapabilitySet::COUNT
        .union(CapabilitySet::MOTIONS)
        .union(CapabilitySet::TEXT_OBJECTS)
        .union(CapabilitySet::MARKS),
    priority: &[
        Capability::Count,
        Capability::Motions,
        Capability::TextObjects,
        Capability::Marks,
    ],
    fallthrough_to_host: false,
};

/// Profile for Insert mode.
///
/// Only `Insert` is active. Unhandled keys fall through to the host so that
/// editor-level shortcuts (e.g., `<C-s>` save) can still fire.
pub const INSERT: ModeProfile = ModeProfile {
    capabilities: CapabilitySet::INSERT,
    priority: &[Capability::Insert],
    fallthrough_to_host: true,
};

/// Profile for Replace mode (same capability shape as Insert).
///
/// Overwrite semantics are handled by the executor, not the capability set.
pub const REPLACE: ModeProfile = ModeProfile {
    capabilities: CapabilitySet::INSERT,
    priority: &[Capability::Insert],
    fallthrough_to_host: true,
};

/// Profile for Select mode.
///
/// Combines `Selection` and `Insert`: printable chars replace the selection
/// and trigger insert-mode entry. Fallthrough disabled.
pub const SELECT: ModeProfile = ModeProfile {
    capabilities: CapabilitySet::SELECTION.union(CapabilitySet::INSERT),
    priority: &[Capability::Selection, Capability::Insert],
    fallthrough_to_host: false,
};

/// Profile for Command-line mode.
///
/// The command line has its own independent input model; no standard Vim
/// capabilities apply. Fallthrough disabled.
pub const COMMAND_LINE: ModeProfile = ModeProfile {
    capabilities: CapabilitySet::empty(),
    priority: &[],
    fallthrough_to_host: false,
};

/// Synthetic profile for contexts where only motion/scroll is allowed.
///
/// Used in restricted contexts (e.g., preview windows). Unhandled keys fall
/// through to the host.
pub const MOTION_ONLY: ModeProfile = ModeProfile {
    capabilities: CapabilitySet::COUNT
        .union(CapabilitySet::MOTIONS)
        .union(CapabilitySet::SCROLL),
    priority: &[Capability::Count, Capability::Motions, Capability::Scroll],
    fallthrough_to_host: true,
};

// ═══════════════════════════════════════════════════════════════════════════
// Mode::default_profile() — maps each mode to its canonical const profile
// ═══════════════════════════════════════════════════════════════════════════

impl Mode {
    /// Return the canonical default [`ModeProfile`] for this mode.
    ///
    /// This maps every `Mode` variant to the appropriate const profile
    /// defined in this module. It is the single source of truth for
    /// which capabilities each mode exposes by default.
    ///
    /// Host code that needs to override the profile for a specific mode
    /// should use [`VimEngine::set_mode_profile`](crate::execution::VimEngine::set_mode_profile) rather than calling this
    /// method directly.
    #[must_use]
    pub const fn default_profile(self) -> &'static ModeProfile {
        match self {
            Self::Normal => &NORMAL,
            Self::Insert => &INSERT,
            Self::Visual(_) => &VISUAL,
            Self::Select(_) => &SELECT,
            Self::Replace | Self::VirtualReplace => &REPLACE,
            Self::CommandLine => &COMMAND_LINE,
            Self::OperatorPending(_) => &OPERATOR_PENDING,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// CapabilityResult — outcome of a per-capability routing attempt
// ═══════════════════════════════════════════════════════════════════════════

/// Outcome of attempting to route a key through a single capability.
///
/// Each `try_*` function returns this to indicate whether it handled the key.
/// `route_through_capabilities` iterates capabilities in priority order and
/// stops at the first `Handled` result.
#[derive(Debug)]
#[must_use]
pub enum CapabilityResult {
    /// The capability handled the key and produced a `ModeAction`.
    Handled(ModeAction),
    /// The capability does not apply to this key — try the next one.
    NotApplicable,
}

// ═══════════════════════════════════════════════════════════════════════════
// Capability routing — shared entry point for pipeline modes
// ═══════════════════════════════════════════════════════════════════════════

/// Route a key through a mode profile's capabilities in priority order.
///
/// Iterates `profile.priority`, calling `try_capability` for each entry.
/// Returns the first `Handled` result. If no capability claims the key,
/// falls through to `delegate_to_parser` which sends the key through
/// the grammar parser — this ensures that routing always produces the
/// same result as the original direct-delegation handlers.
///
/// For profiles with an empty priority list (e.g., `COMMAND_LINE`),
/// the fallback is `ModeAction::Ignored` since those modes have their
/// own independent input models.
pub fn route_through_capabilities(
    key: KeyEvent,
    ctx: &mut ModeContext<'_>,
    profile: &ModeProfile,
) -> ModeAction {
    for &cap in profile.priority {
        match try_capability(cap, key, ctx) {
            CapabilityResult::Handled(action) => return action,
            CapabilityResult::NotApplicable => {}
        }
    }
    // Fallback: delegate to the grammar parser.
    // Profiles with empty priority (e.g., COMMAND_LINE) return Ignored
    // because they have their own input model outside the grammar parser.
    if profile.priority.is_empty() {
        ModeAction::Ignored
    } else {
        delegate_to_parser(key, ctx)
    }
}

/// Dispatch to the per-capability routing function.
///
/// This is the single match point — adding a new capability means adding
/// one arm here and one `try_*` function below.
const fn try_capability(
    cap: Capability,
    key: KeyEvent,
    ctx: &mut ModeContext<'_>,
) -> CapabilityResult {
    match cap {
        Capability::Count => try_count(key, ctx),
        Capability::Register => try_register(key, ctx),
        Capability::Operators => try_operators(key, ctx),
        Capability::Motions => try_motions(key, ctx),
        Capability::Actions => try_actions(key, ctx),
        Capability::TextObjects => try_text_objects(key, ctx),
        Capability::Marks => try_marks(key, ctx),
        // Stubs — will be implemented when their mode handlers are refactored.
        Capability::Insert => CapabilityResult::NotApplicable,
        Capability::Selection => CapabilityResult::NotApplicable,
        Capability::Scroll => try_scroll(key, ctx),
        Capability::Window => try_window(key, ctx),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Per-capability routing functions
// ═══════════════════════════════════════════════════════════════════════════
//
// IMPORTANT: these are framework stubs. The grammar parser already handles
// key classification (motion vs operator vs action etc.). Normal, Visual, and
// OperatorPending all call `parser.process(key, keymap, mode)` and return
// `ModeAction::Pipeline(result)`. The per-capability functions below follow
// the same pattern — they report NotApplicable and let the shared fallback
// delegate to the parser.
//
// They exist so mode handlers can be migrated to call
// `route_through_capabilities` instead of the parser directly. Once that
// happens, individual capabilities can grow pre/post processing specific to
// their domain without every mode handler having to know about it.

/// Route through the parser for pipeline-based capabilities.
///
/// This is the common delegation pattern shared by most capabilities.
/// The parser's grammar handles the actual classification of keys.
#[inline]
fn delegate_to_parser(key: KeyEvent, ctx: &mut ModeContext<'_>) -> ModeAction {
    let mode = ctx.state().mode();
    let (parser, keymap) = ctx.parser_and_keymap();
    let result = parser.process(key, keymap, mode);
    ModeAction::Pipeline(result)
}

/// Try to handle a count prefix (digits 1-9, or 0 when already in a count).
///
/// Delegates to the parser which tracks count accumulation in its state.
const fn try_count(key: KeyEvent, ctx: &mut ModeContext<'_>) -> CapabilityResult {
    // The parser handles count accumulation internally.
    // For now, we don't pre-filter — let the full pipeline handle it.
    // Pre-filtering here needs read access to parser state, which this
    // signature does not yet give us.
    let _ = (key, ctx);
    CapabilityResult::NotApplicable
}

/// Try to handle a register specification (`"x`).
///
/// Delegates to the parser which tracks the pending register.
const fn try_register(key: KeyEvent, ctx: &mut ModeContext<'_>) -> CapabilityResult {
    // The parser handles register parsing internally.
    // For now, we don't pre-filter — let the full pipeline handle it.
    let _ = (key, ctx);
    CapabilityResult::NotApplicable
}

/// Try to handle an operator command (d, c, y, etc.).
const fn try_operators(key: KeyEvent, ctx: &mut ModeContext<'_>) -> CapabilityResult {
    let _ = (key, ctx);
    CapabilityResult::NotApplicable
}

/// Try to handle a motion command (h, j, k, l, w, b, etc.).
const fn try_motions(key: KeyEvent, ctx: &mut ModeContext<'_>) -> CapabilityResult {
    let _ = (key, ctx);
    CapabilityResult::NotApplicable
}

/// Try to handle an action command (p, u, Ctrl-R, ZZ, etc.).
const fn try_actions(key: KeyEvent, ctx: &mut ModeContext<'_>) -> CapabilityResult {
    let _ = (key, ctx);
    CapabilityResult::NotApplicable
}

/// Try to handle a text-object selector (iw, aw, i(, etc.).
const fn try_text_objects(key: KeyEvent, ctx: &mut ModeContext<'_>) -> CapabilityResult {
    let _ = (key, ctx);
    CapabilityResult::NotApplicable
}

/// Try to handle a mark operation (m, ', `).
const fn try_marks(key: KeyEvent, ctx: &mut ModeContext<'_>) -> CapabilityResult {
    let _ = (key, ctx);
    CapabilityResult::NotApplicable
}

/// Try to handle a scroll command (Ctrl-F, Ctrl-B, zz, etc.).
const fn try_scroll(key: KeyEvent, ctx: &mut ModeContext<'_>) -> CapabilityResult {
    let _ = (key, ctx);
    CapabilityResult::NotApplicable
}

/// Try to handle a window/split management command (Ctrl-W …).
const fn try_window(key: KeyEvent, ctx: &mut ModeContext<'_>) -> CapabilityResult {
    let _ = (key, ctx);
    CapabilityResult::NotApplicable
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
#[expect(
    clippy::assertions_on_constants,
    reason = "tests assert invariants over `const` mode profiles; the assert! ensures we still get a clear test-failure message when a profile changes, while a const-context assert would only abort at compile time"
)]
mod tests {
    use super::*;

    #[test]
    fn normal_has_expected_capabilities() {
        assert!(NORMAL.capabilities.contains(CapabilitySet::MOTIONS));
        assert!(NORMAL.capabilities.contains(CapabilitySet::OPERATORS));
        assert!(NORMAL.capabilities.contains(CapabilitySet::ACTIONS));
        assert!(NORMAL.capabilities.contains(CapabilitySet::COUNT));
        assert!(NORMAL.capabilities.contains(CapabilitySet::REGISTER));
        assert!(NORMAL.capabilities.contains(CapabilitySet::MARKS));
        assert!(NORMAL.capabilities.contains(CapabilitySet::SCROLL));
        assert!(NORMAL.capabilities.contains(CapabilitySet::WINDOW));
        assert!(!NORMAL.capabilities.contains(CapabilitySet::INSERT));
        assert!(!NORMAL.capabilities.contains(CapabilitySet::SELECTION));
        assert!(!NORMAL.capabilities.contains(CapabilitySet::TEXT_OBJECTS));
        assert!(!NORMAL.fallthrough_to_host);
    }

    #[test]
    fn visual_has_selection() {
        assert!(VISUAL.capabilities.contains(CapabilitySet::SELECTION));
        assert!(VISUAL.capabilities.contains(CapabilitySet::MOTIONS));
        assert!(!VISUAL.capabilities.contains(CapabilitySet::INSERT));
        assert!(!VISUAL.fallthrough_to_host);
    }

    #[test]
    fn operator_pending_has_text_objects_not_actions() {
        assert!(OPERATOR_PENDING
            .capabilities
            .contains(CapabilitySet::TEXT_OBJECTS));
        assert!(OPERATOR_PENDING
            .capabilities
            .contains(CapabilitySet::MOTIONS));
        assert!(!OPERATOR_PENDING
            .capabilities
            .contains(CapabilitySet::ACTIONS));
        assert!(!OPERATOR_PENDING
            .capabilities
            .contains(CapabilitySet::REGISTER));
        assert!(!OPERATOR_PENDING.fallthrough_to_host);
    }

    #[test]
    fn insert_and_replace_fallthrough() {
        assert!(INSERT.capabilities.contains(CapabilitySet::INSERT));
        assert!(INSERT.fallthrough_to_host);
        assert!(REPLACE.capabilities.contains(CapabilitySet::INSERT));
        assert!(REPLACE.fallthrough_to_host);
    }

    #[test]
    fn select_no_fallthrough() {
        assert!(SELECT.capabilities.contains(CapabilitySet::SELECTION));
        assert!(SELECT.capabilities.contains(CapabilitySet::INSERT));
        assert!(!SELECT.fallthrough_to_host);
    }

    #[test]
    fn command_line_empty_capabilities() {
        assert!(COMMAND_LINE.capabilities.is_empty());
        assert!(COMMAND_LINE.priority.is_empty());
        assert!(!COMMAND_LINE.fallthrough_to_host);
    }

    #[test]
    fn motion_only_fallthrough() {
        assert!(MOTION_ONLY.capabilities.contains(CapabilitySet::MOTIONS));
        assert!(MOTION_ONLY.capabilities.contains(CapabilitySet::COUNT));
        assert!(MOTION_ONLY.capabilities.contains(CapabilitySet::SCROLL));
        assert!(!MOTION_ONLY.capabilities.contains(CapabilitySet::OPERATORS));
        assert!(MOTION_ONLY.fallthrough_to_host);
    }

    #[test]
    fn capability_set_bit_positions_are_distinct() {
        // Verify every named constant is a power of two (single bit).
        let singles = [
            CapabilitySet::MOTIONS,
            CapabilitySet::OPERATORS,
            CapabilitySet::TEXT_OBJECTS,
            CapabilitySet::ACTIONS,
            CapabilitySet::INSERT,
            CapabilitySet::SELECTION,
            CapabilitySet::SCROLL,
            CapabilitySet::WINDOW,
            CapabilitySet::COUNT,
            CapabilitySet::REGISTER,
            CapabilitySet::MARKS,
        ];
        for cap in singles {
            let bits = cap.bits();
            assert_eq!(bits.count_ones(), 1, "expected single bit for {cap:?}");
        }
    }

    #[test]
    fn priority_slices_match_capabilities() {
        // Every capability listed in a priority slice must be present in
        // the capability set of that profile.
        let profiles = [
            ("NORMAL", NORMAL),
            ("VISUAL", VISUAL),
            ("OPERATOR_PENDING", OPERATOR_PENDING),
            ("INSERT", INSERT),
            ("REPLACE", REPLACE),
            ("SELECT", SELECT),
            ("COMMAND_LINE", COMMAND_LINE),
            ("MOTION_ONLY", MOTION_ONLY),
        ];
        for (name, profile) in profiles {
            for &cap in profile.priority {
                let flag = capability_to_flag(cap);
                assert!(
                    profile.capabilities.contains(flag),
                    "profile {name}: priority entry {cap:?} not in capability set",
                );
            }
        }
    }

    /// Helper: map a [`Capability`] variant to its [`CapabilitySet`] flag.
    fn capability_to_flag(cap: Capability) -> CapabilitySet {
        match cap {
            Capability::Motions => CapabilitySet::MOTIONS,
            Capability::Operators => CapabilitySet::OPERATORS,
            Capability::TextObjects => CapabilitySet::TEXT_OBJECTS,
            Capability::Actions => CapabilitySet::ACTIONS,
            Capability::Insert => CapabilitySet::INSERT,
            Capability::Selection => CapabilitySet::SELECTION,
            Capability::Scroll => CapabilitySet::SCROLL,
            Capability::Window => CapabilitySet::WINDOW,
            Capability::Count => CapabilitySet::COUNT,
            Capability::Register => CapabilitySet::REGISTER,
            Capability::Marks => CapabilitySet::MARKS,
        }
    }

    // ───────────────────────────────────────────────────────────────────
    // Routing function tests
    // ───────────────────────────────────────────────────────────────────

    use crate::grammar::Parser;
    use crate::keymap::Keymap;
    use crate::state::VimState;

    #[test]
    fn route_through_empty_profile_returns_ignored() {
        let mut state = VimState::default();
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        let action = route_through_capabilities(KeyEvent::char('j'), &mut ctx, &COMMAND_LINE);
        assert!(matches!(action, ModeAction::Ignored));
    }

    #[test]
    fn route_through_normal_profile_falls_through_to_parser() {
        // All per-capability functions currently return NotApplicable,
        // so the router falls through to delegate_to_parser.
        let mut state = VimState::default();
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        let action = route_through_capabilities(KeyEvent::char('j'), &mut ctx, &NORMAL);
        assert!(
            matches!(action, ModeAction::Pipeline(_)),
            "routing through NORMAL profile should fall through to parser, got {:?}",
            action,
        );
    }

    #[test]
    fn try_capability_dispatches_all_variants() {
        // Verify every Capability variant is handled without panicking.
        let all_caps = [
            Capability::Motions,
            Capability::Operators,
            Capability::TextObjects,
            Capability::Actions,
            Capability::Insert,
            Capability::Selection,
            Capability::Scroll,
            Capability::Window,
            Capability::Count,
            Capability::Register,
            Capability::Marks,
        ];
        for cap in all_caps {
            let mut state = VimState::default();
            let keymap = Keymap::default();
            let mut parser = Parser::new();
            let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);
            let result = try_capability(cap, KeyEvent::char('x'), &mut ctx);
            // All stubs currently return NotApplicable.
            assert!(
                matches!(result, CapabilityResult::NotApplicable),
                "expected NotApplicable for stub {cap:?}",
            );
        }
    }

    #[test]
    fn delegate_to_parser_returns_pipeline() {
        let mut state = VimState::default();
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        let action = delegate_to_parser(KeyEvent::char('j'), &mut ctx);
        assert!(
            matches!(action, ModeAction::Pipeline(_)),
            "delegate_to_parser should always return Pipeline",
        );
    }
}
