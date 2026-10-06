//! Grammar parser.
//!
//! Main entry point for parsing Vim commands.
//! Handler logic is split into separate modules under `handlers/`.

use crate::keymap::{KeyClass, KeyEvent, Keymap};
use crate::primitives::{Mode, RegisterName};
use smart_default::SmartDefault;

use super::command::{count_or_default, Command};
use super::command_line_intent::CommandLineIntent;
use super::input_state::InputState;
use super::result::GrammarResult;

/// Grammar parser for Vim commands.
///
/// The parser is a state machine that processes keys one at a time.
/// It tracks the current parsing state and produces commands when
/// a complete command is recognized.
///
/// # Architecture
///
/// Handler methods are split across files in `handlers/`:
/// - `handlers/ready.rs` - Ready state
/// - `handlers/operator.rs` - Operator state
/// - `handlers/awaiting.rs` - `AwaitingRegister`, `AwaitingChar`, `AwaitingMark`
/// - `handlers/prefix.rs` - `AwaitingPrefix`
///
/// # Example
///
/// ```ignore
/// use vim_core::grammar::Parser;
/// use vim_core::keymap::{KeyEvent, Keymap};
/// use vim_core::state::Mode;
///
/// let mut parser = Parser::new();
/// let keymap = Keymap::default();
///
/// // Parse "dw" (delete word)
/// parser.process(KeyEvent::char('d'), &keymap, Mode::Normal);
/// let result = parser.process(KeyEvent::char('w'), &keymap, Mode::Normal);
/// ```
#[derive(Clone, SmartDefault)]
pub struct Parser {
    /// Current parsing state
    #[default(InputState::default())]
    state: InputState,
    /// Last executed command (for repeat with .)
    last_command: Option<Command>,
    /// Previous command saved before Ctrl-O one-shot normal mode.
    ///
    /// When entering Ctrl-O from insert mode, `last_command` is saved here.
    /// During the Ctrl-O session, `.` replays `previous_command` instead of
    /// `last_command` (which would be the Ctrl-O command itself).
    previous_command: Option<Command>,
    /// Macro recording state.
    /// Tracks which register is being recorded to.
    recording: Option<RegisterName>,
    /// Whether the last executed command was a dot-repeat.
    /// Set by the grammar's repeat handler, read by the engine
    /// to know when to intercept BeginInsert for replay.
    last_was_repeat: bool,
    /// Typed command-line intent captured at parse boundary.
    command_line_intent: Option<CommandLineIntent>,
    /// Whether sneak mode is enabled (`s`/`S` become two-char find motions).
    /// Set by the engine when VimOptions change.
    sneak_mode: bool,
    /// Whether the next motion/textobject resolution should produce
    /// `AwaitingSurroundChar` instead of `Execute(OperatorMotion/TextObject)`.
    /// Set by `ys` detection in the operator handler.
    surround_pending: bool,
    /// Key to re-process after composing finalization.
    ///
    /// Mirrors Neovim's `vungetc()`. When `AwaitingComposingChars`
    /// finalizes because a non-combining key arrived, that key is stored
    /// here instead of being consumed. The engine re-queues it into the
    /// typeahead buffer for normal processing.
    reprocess_key: Option<crate::keymap::KeyEvent>,
}

impl std::fmt::Debug for Parser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Parser")
            .field("state", &self.state)
            .field("has_last_command", &self.last_command.is_some())
            .field("recording", &self.recording)
            .field("last_was_repeat", &self.last_was_repeat)
            .field(
                "has_command_line_intent",
                &self.command_line_intent.is_some(),
            )
            .finish_non_exhaustive()
    }
}

impl Parser {
    /// Create a new parser in Ready state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Process a single key event.
    ///
    /// Returns the result of parsing this key. The parser's internal
    /// state is updated accordingly.
    pub fn process(&mut self, key: KeyEvent, keymap: &Keymap, mode: Mode) -> GrammarResult {
        // Reset repeat flag at start of each key processing cycle
        self.last_was_repeat = false;
        self.command_line_intent = None;
        trace!(target: "Grammar", key = ?key, state = ?self.state, "Processing key");

        // ── Ctrl-\ Ctrl-N universal escape ─────────────────────────────
        // Handle the intermediate state: after Ctrl-\ was pressed, check
        // if this key is Ctrl-N to complete the escape sequence.
        if matches!(self.state, InputState::AwaitingCtrlBackslashN) {
            if key == KeyEvent::ctrl('n') {
                self.state = InputState::default();
                return GrammarResult::ModeChange(Mode::Normal, None);
            }
            // Not Ctrl-N: cancel the sequence and reprocess the key
            // by falling through with state reset to Ready.
            self.state = InputState::default();
            return GrammarResult::Cancel;
        }

        // Intercept Ctrl-\ from any mode to start the escape sequence.
        // This works in insert, visual, command-line, operator-pending,
        // select, and normal modes.
        if key == KeyEvent::ctrl('\\') {
            self.state = InputState::AwaitingCtrlBackslashN;
            return GrammarResult::Continue(InputState::AwaitingCtrlBackslashN);
        }

        // Derive effective mode from current state
        let effective_mode = self.effective_mode(mode);

        // Classify the key in effective mode
        let class = keymap.classify(key, effective_mode);
        trace!(target: "Keymap", ?class, ?effective_mode, "Key classified");

        // Insert mode has its own key processing
        if mode.is_insert() {
            // Handle Escape to exit insert mode — but NOT when awaiting
            // a literal character (Ctrl-V Escape should insert \x1b).
            if class == KeyClass::Escape
                && !matches!(
                    self.state,
                    InputState::InsertLiteral(_) | InputState::AwaitingInsertExpression { .. }
                )
            {
                self.state = InputState::default();
                return GrammarResult::Cancel;
            }
            // Awaiting expression input for <C-r>=
            if matches!(&self.state, InputState::AwaitingInsertExpression { .. }) {
                let collected = match &self.state {
                    InputState::AwaitingInsertExpression { collected } => collected.clone(),
                    _ => compact_str::CompactString::new(""),
                };
                let result = Self::handle_awaiting_insert_expression(key, collected);
                self.update_state(&result);
                return result;
            }
            // Awaiting register for Ctrl-R — delegate to proper handler
            if matches!(&self.state, InputState::AwaitingInsertRegister) {
                let result = Self::handle_awaiting_insert_register(key);
                self.update_state(&result);
                return result;
            }
            // Awaiting Ctrl-G sub-command
            if matches!(&self.state, InputState::AwaitingInsertCtrlG) {
                let result = Self::handle_awaiting_insert_ctrl_g(key);
                self.update_state(&result);
                return result;
            }
            // Awaiting first digraph character after Ctrl-K
            if matches!(&self.state, InputState::AwaitingInsertDigraph1) {
                let result = Self::handle_awaiting_insert_digraph1(key);
                self.update_state(&result);
                return result;
            }
            // Awaiting second digraph character
            if let InputState::AwaitingInsertDigraph2 { c1 } = self.state {
                let result = Self::handle_awaiting_insert_digraph2(key, c1);
                self.update_state(&result);
                return result;
            }
            // Awaiting Ctrl-X completion sub-command
            if matches!(&self.state, InputState::AwaitingInsertCtrlX) {
                let result = Self::handle_awaiting_insert_ctrl_x(key);
                self.update_state(&result);
                return result;
            }
            // Awaiting literal character after Ctrl-V (all sub-states)
            if let InputState::InsertLiteral(ref lit_state) = self.state {
                let lit_state = lit_state.clone();
                let result = Self::handle_insert_literal(key, lit_state);
                self.update_state(&result);
                return result;
            }

            let result = self.handle_insert(key);
            self.update_state(&result);
            return result;
        }

        // Visual mode has special key handling (including Escape)
        if let Mode::Visual(visual_type) = mode {
            // Escape always exits visual mode, even from intermediate parser states
            // like AwaitingVisualTextObject or AwaitingPrefix.
            #[allow(
                clippy::if_same_then_else,
                reason = "Escape branch is semantically distinct from normal dispatch"
            )]
            let result = if class == KeyClass::Escape {
                self.handle_visual(key, class, visual_type)
            } else if self.state.is_ready() {
                self.handle_visual(key, class, visual_type)
            } else {
                // Continue in parser sub-state (prefix, textobject trigger, etc.)
                self.dispatch_state(key, class)
            };
            self.update_state(&result);
            return result;
        }

        // Handle Escape globally for normal mode - always cancels.
        // Exception: `r<C-c>` in Vim replaces with the literal Ctrl-C character (0x03).
        // Ctrl-C is classified as Escape but `r` accepts any character including
        // control characters. True Escape (`<Esc>`) still cancels `r`.
        if class == KeyClass::Escape {
            let is_replace_ctrl_c = matches!(
                &self.state,
                InputState::AwaitingChar {
                    char_command: crate::grammar::CharCommand::Replace,
                    ..
                }
            ) && key.is_ctrl_c();
            if !is_replace_ctrl_c {
                debug!(target: "Grammar", "Escape -> Cancel, resetting to Ready");
                self.state = InputState::default();
                return GrammarResult::Cancel;
            }
        }

        // Dispatch to state-specific handler
        let result = self.dispatch_state(key, class);

        // Surround interception: when surround_pending is set and the dispatch
        // produced an Execute(OperatorMotion/OperatorTextObject with Yank),
        // convert to Continue(AwaitingSurroundChar) to collect the delimiter char.
        let result = self.maybe_intercept_surround(result);

        // Update state based on result
        self.update_state(&result);

        result
    }

    /// Reset parser to initial state.
    pub fn reset(&mut self) {
        self.state = InputState::default();
    }

    /// Record `command` as the one `.` repeats.
    ///
    /// For a command the engine builds itself instead of the parser.
    pub(crate) fn record_for_repeat(&mut self, command: Command) {
        self.last_command = Some(command);
    }

    /// Force the parser into a specific [`InputState`].
    ///
    /// Used by the engine to re-enter sticky sub-modes (e.g. `Ctrl-W`).
    /// The caller is responsible for passing a valid state.
    pub(crate) fn set_state(&mut self, state: InputState) {
        self.state = state;
    }

    /// Get current state (for debugging/testing).
    #[must_use]
    pub const fn state(&self) -> &InputState {
        &self.state
    }

    /// Take the key that should be re-processed after composing finalization.
    ///
    /// The engine calls this after `process()` returns `Execute` from
    /// `AwaitingComposingChars` finalization. The non-combining key that
    /// triggered finalization is stored here (like Neovim's `vungetc`).
    pub const fn take_reprocess_key(&mut self) -> Option<crate::keymap::KeyEvent> {
        self.reprocess_key.take()
    }

    /// Auto-finalize `AwaitingComposingChars` when no more keys are pending.
    ///
    /// Mirrors Neovim's `normal_get_additional_char()` which uses non-blocking
    /// `vpeekc()` and exits immediately when the input buffer is empty.
    /// The engine calls this after each key when `has_pending_keys()` is false.
    pub fn try_finalize_composing(&mut self) -> Option<GrammarResult> {
        if let InputState::AwaitingComposingChars {
            ref grapheme,
            count,
            register,
            operator,
            char_command,
        } = self.state
        {
            let cmd = Command::CharCommand {
                count: crate::grammar::command::count_or_default(count),
                register,
                operator,
                command: char_command,
                target: grapheme.clone(),
            };
            if cmd.properties().repeat != crate::primitives::RepeatBehavior::Skip {
                self.last_command = Some(cmd.clone());
            }
            self.state = InputState::default();
            Some(GrammarResult::Execute(cmd))
        } else {
            None
        }
    }

    /// Get last executed command (for repeat).
    #[must_use]
    pub const fn last_command(&self) -> Option<&Command> {
        self.last_command.as_ref()
    }

    /// Set the last command directly (test-only).
    ///
    /// Allows tests to simulate a prior command execution for
    /// dot-repeat testing without processing real keystrokes.
    #[cfg(test)]
    pub fn set_last_command_for_test(&mut self, command: Command) {
        self.last_command = Some(command);
    }

    /// Get the previous command (saved before Ctrl-O one-shot).
    #[must_use]
    pub const fn previous_command(&self) -> Option<&Command> {
        self.previous_command.as_ref()
    }

    /// Save the current `last_command` as `previous_command`.
    ///
    /// Called when entering Ctrl-O one-shot normal mode from insert mode.
    pub fn save_previous_command(&mut self) {
        self.previous_command = self.last_command.clone();
    }

    /// Restore `last_command` from `previous_command` and clear it.
    ///
    /// Called when returning from Ctrl-O one-shot to insert mode, so that
    /// subsequent `.` replays the command from before Ctrl-O, not the
    /// one-shot command itself.
    pub fn restore_previous_command(&mut self) {
        if let Some(prev) = self.previous_command.take() {
            self.last_command = Some(prev);
        }
    }

    /// Whether the last grammar result was from a dot-repeat.
    ///
    /// The engine checks this after receiving GrammarResult::Execute
    /// to know whether to intercept BeginInsert effects for replay.
    #[must_use]
    pub const fn was_repeat(&self) -> bool {
        self.last_was_repeat
    }

    /// Mark the current command as a repeat (called by ready.rs handler).
    pub(crate) const fn set_repeat(&mut self) {
        self.last_was_repeat = true;
    }

    /// Capture command-line intent for the current parse cycle.
    pub(crate) const fn set_command_line_intent(&mut self, intent: CommandLineIntent) {
        self.command_line_intent = Some(intent);
    }

    /// Consume and return command-line intent, if any.
    pub const fn take_command_line_intent(&mut self) -> Option<CommandLineIntent> {
        self.command_line_intent.take()
    }

    /// Check if currently recording a macro.
    #[inline]
    #[must_use]
    pub const fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    /// Set the recording register (called by engine after MacroRecord).
    pub const fn set_recording(&mut self, register: Option<RegisterName>) {
        self.recording = register;
    }

    /// Whether sneak mode is enabled.
    #[inline]
    #[must_use]
    pub const fn sneak_mode(&self) -> bool {
        self.sneak_mode
    }

    /// Set sneak mode (called by engine when VimOptions change).
    #[inline]
    pub const fn set_sneak_mode(&mut self, value: bool) {
        self.sneak_mode = value;
    }

    /// Whether surround char collection is pending after a motion resolves.
    #[inline]
    #[must_use]
    pub const fn surround_pending(&self) -> bool {
        self.surround_pending
    }

    /// Set surround pending flag (called by `ys` detection in operator handler).
    #[inline]
    pub(crate) const fn set_surround_pending(&mut self, value: bool) {
        self.surround_pending = value;
    }

    /// Intercept grammar results for surround mode (`ys`).
    ///
    /// When `surround_pending` is true and the dispatch produced an
    /// `Execute(OperatorMotion)` or `Execute(OperatorTextObject)` with
    /// Yank operator, convert to `Continue(AwaitingSurroundChar)` to
    /// collect the delimiter character before emitting `SurroundAdd`.
    fn maybe_intercept_surround(&mut self, result: GrammarResult) -> GrammarResult {
        if !self.surround_pending {
            return result;
        }

        match &result {
            GrammarResult::Execute(Command::OperatorMotion {
                operator: crate::primitives::Operator::Yank,
                motion,
                count,
                ..
            }) => {
                self.surround_pending = false;
                GrammarResult::Continue(InputState::AwaitingSurroundChar {
                    count: Some(count.get()),
                    motion: Some(*motion),
                    textobject: None,
                })
            }
            GrammarResult::Execute(Command::OperatorTextObject {
                operator: crate::primitives::Operator::Yank,
                textobject,
                count,
                ..
            }) => {
                self.surround_pending = false;
                GrammarResult::Continue(InputState::AwaitingSurroundChar {
                    count: Some(count.get()),
                    motion: None,
                    textobject: Some(*textobject),
                })
            }
            // Cancel/Invalid/Continue/other Execute — clear surround flag
            GrammarResult::Cancel | GrammarResult::Invalid => {
                self.surround_pending = false;
                result
            }
            _ => result,
        }
    }

    /// Dispatch to state-specific handler.
    fn dispatch_state(&mut self, key: KeyEvent, class: KeyClass) -> GrammarResult {
        match &self.state {
            InputState::Ready { count, register } => {
                self.handle_ready(*count, *register, key, class)
            }
            InputState::AwaitingRegister { count, phase } => {
                self.handle_awaiting_register(*count, *phase, key)
            }
            InputState::Operator {
                count,
                register,
                operator,
                count2,
                force_type,
            } => {
                let op = crate::grammar::handlers::operator::OpCtx {
                    count: *count,
                    register: *register,
                    operator: *operator,
                    count2: *count2,
                    force_type: *force_type,
                };
                self.handle_operator(op, key, class)
            }
            InputState::AwaitingChar {
                count,
                register,
                operator,
                char_command,
            } => Self::handle_awaiting_char(*count, *register, *operator, *char_command, key),
            InputState::AwaitingComposingChars {
                grapheme,
                count,
                register,
                operator,
                char_command,
            } => {
                let was_composing = true;
                let result = Self::handle_awaiting_composing_chars(
                    grapheme,
                    *count,
                    *register,
                    *operator,
                    *char_command,
                    key,
                );
                // Neovim's vungetc(): when a non-combining key finalizes
                // the composing sequence, re-queue it for normal processing
                // instead of consuming it.
                if was_composing && matches!(result, GrammarResult::Execute(_)) {
                    let is_combining = key
                        .as_char()
                        .is_some_and(crate::grammar::handlers::awaiting::is_combining_mark);
                    if !is_combining {
                        self.reprocess_key = Some(key);
                    }
                }
                result
            }
            InputState::AwaitingTextObject {
                count,
                register,
                operator,
                scope,
            } => Self::handle_awaiting_textobject(*count, *register, *operator, *scope, key),
            InputState::AwaitingTextObjectWithModifier {
                count,
                register,
                operator,
                scope,
                seek,
            } => Self::handle_awaiting_textobject_with_modifier(
                *count, *register, *operator, *scope, *seek, key,
            ),
            InputState::AwaitingPrefix {
                count,
                register,
                prefix,
                operator,
                force_type,
            } => {
                self.handle_awaiting_prefix(*count, *register, *prefix, *operator, *force_type, key)
            }
            InputState::AwaitingMark {
                count,
                mark_type,
                operator,
                register,
            } => Self::handle_awaiting_mark(*count, *mark_type, *operator, *register, key),
            InputState::AwaitingInsertRegister => Self::handle_awaiting_insert_register(key),
            InputState::AwaitingInsertExpression { collected } => {
                Self::handle_awaiting_insert_expression(key, collected.clone())
            }
            InputState::AwaitingInsertCtrlG => Self::handle_awaiting_insert_ctrl_g(key),
            InputState::AwaitingInsertDigraph1 => Self::handle_awaiting_insert_digraph1(key),
            InputState::AwaitingInsertDigraph2 { c1 } => {
                Self::handle_awaiting_insert_digraph2(key, *c1)
            }
            InputState::AwaitingInsertCtrlX => Self::handle_awaiting_insert_ctrl_x(key),
            InputState::InsertLiteral(ref lit_state) => {
                Self::handle_insert_literal(key, lit_state.clone())
            }
            InputState::AwaitingMacroRegister { count, kind } => {
                Self::handle_awaiting_macro_register(*count, *kind, key)
            }
            InputState::AwaitingVisualTextObject {
                count,
                scope,
                register,
            } => Self::handle_awaiting_visual_textobject(*count, *scope, *register, key),
            InputState::AwaitingVisualTextObjectWithModifier {
                count,
                scope,
                register,
                seek,
            } => Self::handle_awaiting_visual_textobject_with_modifier(
                *count, *scope, *register, *seek, key,
            ),
            InputState::AwaitingWindowCommand { count, register } => {
                Self::handle_window_command(*count, *register, key)
            }
            InputState::AwaitingSneakChar1 {
                count,
                register,
                operator,
                forward,
            } => Self::handle_awaiting_sneak_char1(*count, *register, *operator, *forward, key),
            InputState::AwaitingSneakChar2 {
                count,
                register,
                operator,
                forward,
                c1,
            } => {
                Self::handle_awaiting_sneak_char2(*count, *register, *operator, *forward, *c1, key)
            }
            InputState::AwaitingSurroundChar {
                count,
                motion,
                textobject,
            } => Self::handle_awaiting_surround_char(*count, *motion, *textobject, key),
            InputState::AwaitingSurroundDeleteChar => {
                Self::handle_awaiting_surround_delete_char(key)
            }
            InputState::AwaitingSurroundOldChar => Self::handle_awaiting_surround_old_char(key),
            InputState::AwaitingSurroundNewChar { old_char } => {
                Self::handle_awaiting_surround_new_char(*old_char, key)
            }
            InputState::AwaitingVisualZPrefix { register } => {
                Self::handle_awaiting_visual_z_prefix(*register, key)
            }
            InputState::AwaitingCtrlBackslashN => {
                // Handled in process() before dispatch_state() is called,
                // but included for exhaustiveness.
                if key == KeyEvent::ctrl('n') {
                    GrammarResult::ModeChange(Mode::Normal, None)
                } else {
                    GrammarResult::Cancel
                }
            }
        }
    }

    /// Update internal state based on result.
    fn update_state(&mut self, result: &GrammarResult) {
        match result {
            GrammarResult::Continue(new_state) => {
                self.state = new_state.clone();
            }
            GrammarResult::Execute(cmd) => {
                if cmd.properties().repeat != crate::primitives::RepeatBehavior::Skip {
                    self.last_command = Some(cmd.clone());
                }
                // Preserve register after VisualTextObject so subsequent
                // operator commands (e.g., `"aiwd`) can use it.
                if let Command::VisualTextObject { register, .. } = cmd {
                    self.state = InputState::Ready {
                        count: None,
                        register: *register,
                    };
                } else {
                    self.state = InputState::default();
                }
            }
            GrammarResult::ModeChange(mode, count) => {
                // For Replace mode (R command), save a synthetic InsertEntry
                // command so dot-repeat can replay the replace operation.
                if matches!(mode, Mode::Replace | Mode::VirtualReplace) {
                    self.last_command = Some(Command::InsertEntry {
                        count: count_or_default(*count),
                        entry_type: crate::primitives::InsertEntryType::ReplaceMode,
                        register: None,
                    });
                }
                self.state = InputState::default();
            }
            GrammarResult::Cancel => {
                self.state = InputState::default();
            }
            GrammarResult::Invalid => {
                // Keep current state - user can try again or Escape
            }
        }
    }

    /// Get the effective mode for key classification based on current state.
    const fn effective_mode(&self, base_mode: Mode) -> Mode {
        match &self.state {
            InputState::Operator { operator, .. } => Mode::OperatorPending(*operator),
            InputState::AwaitingTextObject { operator, .. } => Mode::OperatorPending(*operator),
            InputState::AwaitingTextObjectWithModifier { operator, .. } => {
                Mode::OperatorPending(*operator)
            }
            _ => base_mode,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_state_changes_parser_state() {
        let mut parser = Parser::new();
        assert!(parser.state().is_ready());

        parser.set_state(InputState::AwaitingWindowCommand {
            count: None,
            register: None,
        });
        assert!(matches!(
            parser.state(),
            InputState::AwaitingWindowCommand { .. }
        ));

        parser.reset();
        assert!(parser.state().is_ready());
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Ctrl-\ Ctrl-N universal escape
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn ctrl_backslash_transitions_to_awaiting_state() {
        let mut parser = Parser::new();
        let keymap = Keymap::default();

        let result = parser.process(KeyEvent::ctrl('\\'), &keymap, Mode::Normal);
        assert!(matches!(
            result,
            GrammarResult::Continue(InputState::AwaitingCtrlBackslashN)
        ));
        assert!(matches!(parser.state(), InputState::AwaitingCtrlBackslashN));
    }

    #[test]
    fn ctrl_backslash_ctrl_n_from_normal() {
        let mut parser = Parser::new();
        let keymap = Keymap::default();

        parser.process(KeyEvent::ctrl('\\'), &keymap, Mode::Normal);
        let result = parser.process(KeyEvent::ctrl('n'), &keymap, Mode::Normal);

        assert!(matches!(
            result,
            GrammarResult::ModeChange(Mode::Normal, None)
        ));
        assert!(parser.state().is_ready());
    }

    #[test]
    fn ctrl_backslash_ctrl_n_from_insert() {
        let mut parser = Parser::new();
        let keymap = Keymap::default();

        parser.process(KeyEvent::ctrl('\\'), &keymap, Mode::Insert);
        let result = parser.process(KeyEvent::ctrl('n'), &keymap, Mode::Insert);

        assert!(matches!(
            result,
            GrammarResult::ModeChange(Mode::Normal, None)
        ));
        assert!(parser.state().is_ready());
    }

    #[test]
    fn ctrl_backslash_ctrl_n_from_visual() {
        let mut parser = Parser::new();
        let keymap = Keymap::default();
        let mode = Mode::Visual(crate::primitives::VisualType::Char);

        parser.process(KeyEvent::ctrl('\\'), &keymap, mode);
        let result = parser.process(KeyEvent::ctrl('n'), &keymap, mode);

        assert!(matches!(
            result,
            GrammarResult::ModeChange(Mode::Normal, None)
        ));
        assert!(parser.state().is_ready());
    }

    #[test]
    fn ctrl_backslash_ctrl_n_from_command_line() {
        let mut parser = Parser::new();
        let keymap = Keymap::default();

        parser.process(KeyEvent::ctrl('\\'), &keymap, Mode::CommandLine);
        let result = parser.process(KeyEvent::ctrl('n'), &keymap, Mode::CommandLine);

        assert!(matches!(
            result,
            GrammarResult::ModeChange(Mode::Normal, None)
        ));
        assert!(parser.state().is_ready());
    }

    #[test]
    fn ctrl_backslash_ctrl_n_from_select() {
        let mut parser = Parser::new();
        let keymap = Keymap::default();
        let mode = Mode::Select(crate::primitives::VisualType::Char);

        parser.process(KeyEvent::ctrl('\\'), &keymap, mode);
        let result = parser.process(KeyEvent::ctrl('n'), &keymap, mode);

        assert!(matches!(
            result,
            GrammarResult::ModeChange(Mode::Normal, None)
        ));
        assert!(parser.state().is_ready());
    }

    #[test]
    fn ctrl_backslash_other_key_cancels() {
        let mut parser = Parser::new();
        let keymap = Keymap::default();

        parser.process(KeyEvent::ctrl('\\'), &keymap, Mode::Insert);
        let result = parser.process(KeyEvent::char('x'), &keymap, Mode::Insert);

        // Non-Ctrl-N after Ctrl-\ should cancel
        assert!(matches!(result, GrammarResult::Cancel));
        assert!(parser.state().is_ready());
    }

    #[test]
    fn ctrl_backslash_pending_display() {
        let state = InputState::AwaitingCtrlBackslashN;
        assert_eq!(state.pending_display().as_str(), "^\\");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Ctrl-O previous_command save/restore
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn task_8_6_save_previous_command_stores_last() {
        let mut parser = Parser::new();
        let keymap = Keymap::default();

        // Execute "dd" so last_command is set
        parser.process(KeyEvent::char('d'), &keymap, Mode::Normal);
        parser.process(KeyEvent::char('d'), &keymap, Mode::Normal);
        assert!(parser.last_command().is_some());

        // Save it as previous
        parser.save_previous_command();
        assert!(parser.previous_command().is_some());
    }

    #[test]
    fn task_8_6_restore_previous_command_overwrites_last() {
        let mut parser = Parser::new();
        let keymap = Keymap::default();

        // Execute "dd" → last_command = dd
        parser.process(KeyEvent::char('d'), &keymap, Mode::Normal);
        parser.process(KeyEvent::char('d'), &keymap, Mode::Normal);

        // Save previous
        parser.save_previous_command();

        // Execute "x" → last_command = x (overwritten)
        parser.process(KeyEvent::char('x'), &keymap, Mode::Normal);

        // Restore → last_command should be back to dd
        parser.restore_previous_command();
        let restored = parser.last_command().unwrap();
        // The restored command should be a delete-line (dd)
        assert!(
            matches!(
                restored,
                Command::OperatorMotion { .. }
                    | Command::OperatorLine { .. }
                    | Command::Action { .. }
            ),
            "Restored command should be the dd, not x"
        );
    }

    #[test]
    fn task_8_6_restore_clears_previous() {
        let mut parser = Parser::new();
        let keymap = Keymap::default();

        parser.process(KeyEvent::char('x'), &keymap, Mode::Normal);
        parser.save_previous_command();
        assert!(parser.previous_command().is_some());

        parser.restore_previous_command();
        assert!(
            parser.previous_command().is_none(),
            "previous_command should be cleared after restore"
        );
    }

    #[test]
    fn task_8_6_no_previous_restore_is_noop() {
        let mut parser = Parser::new();
        let keymap = Keymap::default();

        parser.process(KeyEvent::char('x'), &keymap, Mode::Normal);
        let before = parser.last_command().cloned();
        parser.restore_previous_command(); // no previous saved
        assert_eq!(parser.last_command(), before.as_ref());
    }
}
