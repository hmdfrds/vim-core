//! Core types and traits for the mode subsystem.
//!
//! Contains `ModeHandler`, `ModeContext`, `ModeAction`, `ModeDispatcher`,
//! and related enums. Extracted from `mod.rs` to keep it a pure facade.

use crate::grammar::{Command, GrammarResult, Parser};
use crate::keymap::{KeyEvent, Keymap};
use crate::mode::command_line::CommandLineAction;
use crate::primitives::Mode;
use crate::state::VimState;

use super::{
    CommandLineModeHandler, InsertModeHandler, NormalModeHandler, OperatorPendingModeHandler,
    ReplaceModeHandler, SelectModeHandler, VisualModeHandler,
};

// ═══════════════════════════════════════════════════════════════════════════
// ModeHandler trait — compiler-enforced contract
// ═══════════════════════════════════════════════════════════════════════════

/// Mode-specific key processing.
///
/// Each Vim mode implements this trait. The signature IS the enforcement:
/// handlers may only access what `ModeContext` provides. They physically
/// cannot reach `Pipeline`, `Executor`, `ExecutionContext`, or any other
/// engine internal.
///
/// # Contract
///
/// - Receive a keystroke and mode context
/// - Return a `ModeAction` telling the engine what to do
/// - **Never** produce side effects outside of `ModeContext`
///
/// # One handler per mode — no aliasing
///
/// Every mode has its own struct implementing this trait.
/// Even if two modes share similar logic (e.g., Normal and Visual both
/// delegate to the grammar pipeline), they are separate handlers because:
/// 1. They pass different `Mode` variants to the parser
/// 2. They may diverge in the future without affecting each other
/// 3. It's enterprise-strict: no shortcuts, no aliasing
/// # Sealed
///
/// This trait is **sealed** — only the 7 handler types defined in this
/// module may implement it. External crates cannot add new implementations.
/// This prevents unauthorized mode handlers from bypassing the architecture.
pub trait ModeHandler: super::sealed::Sealed {
    /// Process a keystroke in this mode.
    fn handle_key(&self, key: KeyEvent, ctx: &mut ModeContext<'_>) -> ModeAction;
}

// ═══════════════════════════════════════════════════════════════════════════
// ModeContext — dependency injection
// ═══════════════════════════════════════════════════════════════════════════

/// Read/write context passed to mode handlers.
///
/// This struct is the **only** thing mode handlers can see.
/// It provides access to state, keymap, and parser — nothing else.
/// The engine constructs it from its own fields before calling handlers.
///
/// # Why not just pass fields directly?
///
/// `ModeContext` enforces a stable API surface. If the engine adds new
/// fields, handlers don't automatically get access — the context must
/// explicitly expose them. This is intentional gatekeeping.
pub struct ModeContext<'eng> {
    state: &'eng mut VimState,
    keymap: &'eng Keymap,
    parser: &'eng mut Parser,
}

impl std::fmt::Debug for ModeContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModeContext")
            .field("mode", &self.state.mode())
            .finish_non_exhaustive()
    }
}

impl<'eng> ModeContext<'eng> {
    /// Create a new mode context.
    #[inline]
    pub(crate) const fn new(
        state: &'eng mut VimState,
        keymap: &'eng Keymap,
        parser: &'eng mut Parser,
    ) -> Self {
        Self {
            state,
            keymap,
            parser,
        }
    }

    /// Read-only access to state.
    #[inline]
    #[must_use]
    pub const fn state(&self) -> &VimState {
        self.state
    }

    /// Mutable access to state (e.g., command-line text editing).
    #[inline]
    pub const fn state_mut(&mut self) -> &mut VimState {
        self.state
    }

    /// Read-only access to keymap.
    #[inline]
    #[must_use]
    pub const fn keymap(&self) -> &Keymap {
        self.keymap
    }

    /// Split borrow: get mutable parser and immutable keymap simultaneously.
    ///
    /// Rust's borrow checker cannot split borrows through separate `&self` /
    /// `&mut self` method calls. This method returns both references in a
    /// single call, enabling safe concurrent access.
    #[inline]
    pub const fn parser_and_keymap(&mut self) -> (&mut Parser, &Keymap) {
        (self.parser, self.keymap)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ModeAction — instruction from handler to engine
// ═══════════════════════════════════════════════════════════════════════════

/// Whether an insert-mode command is in insert or replace mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum InsertMode {
    /// Normal insert mode.
    Insert,
    /// Replace mode (overwrite characters).
    Replace,
    /// Virtual replace mode (overwrite by screen columns, not bytes).
    VirtualReplace,
}

/// Instruction returned by a mode handler.
///
/// Mode handlers **decide**; the engine **executes**.
/// This enum describes what the engine should do after the mode handler
/// has processed a keystroke.
#[derive(Debug)]
#[must_use]
#[non_exhaustive]
pub enum ModeAction {
    /// Route through the grammar pipeline (Normal/Visual/OperatorPending).
    ///
    /// The engine classifies Invalid/Cancel as errors, resolves the result
    /// into a `PlannedAction`, and executes it.
    Pipeline(GrammarResult),

    /// Exit insert/replace mode (Escape/Ctrl-[/Ctrl-C).
    InsertExit,

    /// Execute an insert-mode command.
    InsertCommand {
        /// The parsed insert command (InsertChar, InsertBackspace, etc.)
        command: Command,
        /// Whether we're in Insert or Replace mode (different execution path).
        /// Guaranteed `Replace` from `ReplaceModeHandler`, `Insert` from `InsertModeHandler`.
        insert_mode: InsertMode,
    },

    /// Command-line action result.
    CommandLine(CommandLineResult),

    /// Replace selection with a character (Select mode — printable char typed).
    ///
    /// Deletes the current selection, enters Insert mode, and inserts the char.
    SelectReplace {
        /// The character that replaces the selection.
        char: char,
    },

    /// Delete selection (Select mode — Backspace/Delete pressed).
    ///
    /// Deletes the current selection and returns to Normal mode.
    SelectDelete,

    /// More input needed (key consumed, waiting for next).
    Pending,

    /// Key not consumed / no-op.
    Ignored,
}

/// Result from command-line mode key processing.
#[derive(Debug)]
#[must_use]
#[non_exhaustive]
pub enum CommandLineResult {
    /// User edited the command-line input.
    Edit(crate::grammar::CommandLineEdit),
    /// User cancelled (Escape).
    Cancel,
    /// User committed input (Enter).
    Commit,
    /// Enter register-awaiting sub-state (Ctrl-R).
    AwaitRegister,
    /// Open command-line window with current input as prefill (Ctrl-F / cedit).
    OpenCommandWindow,
}

impl From<CommandLineAction> for ModeAction {
    fn from(action: CommandLineAction) -> Self {
        match action {
            CommandLineAction::Edit(edit) => Self::CommandLine(CommandLineResult::Edit(edit)),
            CommandLineAction::Ignore => Self::Pending,
            CommandLineAction::Cancel => Self::CommandLine(CommandLineResult::Cancel),
            CommandLineAction::Commit => Self::CommandLine(CommandLineResult::Commit),
            CommandLineAction::AwaitRegister => Self::CommandLine(CommandLineResult::AwaitRegister),
            CommandLineAction::OpenCommandWindow => {
                Self::CommandLine(CommandLineResult::OpenCommandWindow)
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ModeDispatcher — zero-cost, 1:1 mode-to-handler mapping
// ═══════════════════════════════════════════════════════════════════════════

/// Dispatches keystrokes to the appropriate mode handler.
///
/// Uses static dispatch (direct field access) — zero vtable overhead.
/// Each handler is a zero-sized type (ZST), so `ModeDispatcher` itself
/// is also zero-sized.
///
/// **Every mode has its own handler** — no aliasing, no shortcuts.
#[derive(Debug, Default, Clone, Copy)]
pub struct ModeDispatcher {
    pub(super) normal: NormalModeHandler,
    pub(super) visual: VisualModeHandler,
    pub(super) select: SelectModeHandler,
    pub(super) operator_pending: OperatorPendingModeHandler,
    pub(super) insert: InsertModeHandler,
    pub(super) replace: ReplaceModeHandler,
    pub(super) command_line: CommandLineModeHandler,
}

impl ModeDispatcher {
    /// Create a new dispatcher.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Dispatch a keystroke to the handler for the current mode.
    ///
    /// Each mode maps to exactly one handler — no fallthrough.
    #[inline]
    pub fn dispatch(&self, mode: Mode, key: KeyEvent, ctx: &mut ModeContext<'_>) -> ModeAction {
        match mode {
            Mode::Normal => self.normal.handle_key(key, ctx),
            Mode::Visual(_) => self.visual.handle_key(key, ctx),
            Mode::Select(_) => self.select.handle_key(key, ctx),
            Mode::OperatorPending(_) => self.operator_pending.handle_key(key, ctx),
            Mode::Insert => self.insert.handle_key(key, ctx),
            Mode::Replace | Mode::VirtualReplace => self.replace.handle_key(key, ctx),
            Mode::CommandLine => self.command_line.handle_key(key, ctx),
        }
    }
}
