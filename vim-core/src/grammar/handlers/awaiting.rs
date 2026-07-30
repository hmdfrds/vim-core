//! Awaiting state handlers.
//!
//! Handles key processing for states waiting for specific input.

use crate::grammar::types::Operator;
use crate::keymap::KeyEvent;
use crate::primitives::{MarkName, RegisterName};
use compact_str::CompactString;
use std::num::NonZeroU32;

use crate::grammar::command::{count_or_default, Command, InsertKind, MacroKind, PrefixCommand};
use crate::grammar::input_state::{InputState, MacroAwaitKind};
use crate::grammar::parser::Parser;
use crate::grammar::result::GrammarResult;
use crate::grammar::types::{CharCommand, MarkType};

/// Check if a character is a Unicode combining mark (General Category Mn/Mc/Me).
///
/// Covers the most common combining mark ranges. A full Unicode-aware check
/// would use a crate like `unicode-general-category`, but these ranges cover
/// the vast majority of combining characters encountered in practice.
pub(crate) use crate::primitives::is_combining_mark;

impl Parser {
    /// Handle `AwaitingRegister` state.
    ///
    /// After pressing `"`, we need a register name.
    /// If `operator` is `Some`, we came from operator state (`d"a`) and return
    /// to `Operator` with the register set. Otherwise, return to `Ready`.
    pub(crate) const fn handle_awaiting_register(
        &self,
        count: Option<u32>,
        phase: crate::grammar::input_state::RegisterPhase,
        key: KeyEvent,
    ) -> GrammarResult {
        use crate::grammar::input_state::RegisterPhase;

        // Accept any valid register name — this is THE validation boundary.
        // RegisterName::new() validates and returns None for invalid chars.
        if let Some(c) = key.as_char() {
            if let Some(reg) = RegisterName::new(c) {
                return match phase {
                    RegisterPhase::AfterOperator { operator, count2 } => {
                        // d"a → return to Operator state with register set
                        GrammarResult::Continue(InputState::Operator {
                            count,
                            register: Some(reg),
                            operator,
                            count2,
                            force_type: None,
                        })
                    }
                    RegisterPhase::BeforeOperator => {
                        // "a → return to Ready state with register set
                        GrammarResult::Continue(InputState::Ready {
                            count,
                            register: Some(reg),
                        })
                    }
                };
            }
        }
        GrammarResult::Invalid
    }

    /// Handle `AwaitingInsertRegister` state.
    ///
    /// After Ctrl-R in insert mode, we need a register name.
    /// Special case: `=` enters expression input mode instead of looking up a register.
    pub(crate) fn handle_awaiting_insert_register(key: KeyEvent) -> GrammarResult {
        // Ctrl-R again stays in the state (handles <C-r><C-r>{reg} = literal)
        if key == KeyEvent::ctrl('r') {
            return GrammarResult::Continue(InputState::AwaitingInsertRegister);
        }
        // Ctrl-W/A/L: insert word/WORD/line under cursor (pseudo-registers)
        if key == KeyEvent::ctrl('w') {
            return GrammarResult::Execute(Command::Insert(InsertKind::InsertWordUnderCursor));
        }
        if key == KeyEvent::ctrl('a') {
            return GrammarResult::Execute(Command::Insert(InsertKind::InsertWORDUnderCursor));
        }
        if key == KeyEvent::ctrl('l') {
            return GrammarResult::Execute(Command::Insert(InsertKind::InsertCurrentLine));
        }
        // Next key is the register name
        if let Some(c) = key.as_char() {
            // '=' enters expression input mode: collect expression until <CR>
            if c == '=' {
                return GrammarResult::Continue(InputState::AwaitingInsertExpression {
                    collected: CompactString::new(""),
                });
            }
            if let Some(reg) = RegisterName::new(c) {
                return GrammarResult::Execute(Command::Insert(InsertKind::Register {
                    register: reg,
                }));
            }
        }
        // Invalid register key
        GrammarResult::Invalid
    }

    /// Handle `AwaitingInsertExpression` state.
    ///
    /// Collects expression text until `<CR>`, then emits `InsertKind::ExpressionResult`.
    /// `<Esc>` cancels the expression input.
    pub(crate) fn handle_awaiting_insert_expression(
        key: KeyEvent,
        mut collected: CompactString,
    ) -> GrammarResult {
        use crate::keymap::Key;

        match key.key() {
            Key::Enter => {
                // Evaluate expression: for now, emit the collected text as the result.
                // The execution layer / host can evaluate it (for simple numeric literals,
                // the text itself IS the result).
                return GrammarResult::Execute(Command::Insert(InsertKind::ExpressionResult {
                    expression: collected.into(),
                }));
            }
            Key::Escape => {
                // Cancel expression input, return to insert mode
                return GrammarResult::Execute(Command::Insert(InsertKind::Nop));
            }
            Key::Backspace => {
                collected.pop();
                return GrammarResult::Continue(InputState::AwaitingInsertExpression { collected });
            }
            Key::Char(c) => {
                collected.push(c);
                return GrammarResult::Continue(InputState::AwaitingInsertExpression { collected });
            }
            // drift: non-printable keys (arrows, F-keys, etc.) are silently ignored while accumulating an expression literal
            _ => {}
        }
        GrammarResult::Continue(InputState::AwaitingInsertExpression { collected })
    }

    /// Handle `AwaitingInsertCtrlG` state.
    ///
    /// After Ctrl-G in insert mode: `u` breaks undo, `j`/`k`/arrows move cursor.
    pub(crate) const fn handle_awaiting_insert_ctrl_g(key: KeyEvent) -> GrammarResult {
        use crate::grammar::types::Motion;
        use crate::keymap::Key;
        // Arrow keys → motions (engine routes through normal executor in insert mode)
        match key.key {
            Key::Down => {
                return GrammarResult::Execute(Command::Motion {
                    motion: Motion::Down,
                    count: NonZeroU32::MIN,
                    explicit_count: false,
                })
            }
            Key::Up => {
                return GrammarResult::Execute(Command::Motion {
                    motion: Motion::Up,
                    count: NonZeroU32::MIN,
                    explicit_count: false,
                })
            }
            // drift: Ctrl-G sub-commands are char-based; non-arrow named keys fall through to the char dispatch below
            _ => {}
        }
        if let Some(c) = key.as_char() {
            return match c {
                'u' => GrammarResult::Execute(Command::Insert(InsertKind::BreakUndoSequence)),
                'U' => GrammarResult::Execute(Command::Insert(InsertKind::DontSyncUndo)),
                'j' => GrammarResult::Execute(Command::Motion {
                    motion: Motion::Down,
                    count: NonZeroU32::MIN,
                    explicit_count: false,
                }),
                'k' => GrammarResult::Execute(Command::Motion {
                    motion: Motion::Up,
                    count: NonZeroU32::MIN,
                    explicit_count: false,
                }),
                _ => GrammarResult::Invalid,
            };
        }
        GrammarResult::Invalid
    }

    /// Handle `AwaitingInsertDigraph1` state.
    ///
    /// After Ctrl-K in insert mode, the next printable character is the
    /// first half of the digraph pair.
    pub(crate) const fn handle_awaiting_insert_digraph1(key: KeyEvent) -> GrammarResult {
        if let Some(c) = key.as_char() {
            if c >= ' ' {
                return GrammarResult::Continue(InputState::AwaitingInsertDigraph2 { c1: c });
            }
        }
        GrammarResult::Invalid
    }

    /// Handle `AwaitingInsertDigraph2` state.
    ///
    /// After Ctrl-K + first char, the next printable character completes
    /// the digraph pair. Resolution is deferred to the execution layer via
    /// `InsertKind::Digraph` so user-defined digraphs (stored on the engine's
    /// `DigraphRegistry`) are consulted alongside the built-in table.
    pub(crate) const fn handle_awaiting_insert_digraph2(key: KeyEvent, c1: char) -> GrammarResult {
        if let Some(c2) = key.as_char() {
            if c2 >= ' ' {
                return GrammarResult::Execute(Command::Insert(InsertKind::Digraph { c1, c2 }));
            }
        }
        GrammarResult::Invalid
    }

    /// Handle `InsertLiteral` state (all sub-states).
    ///
    /// After Ctrl-V in insert mode, dispatches to the appropriate sub-state:
    /// - `AwaitingFirst`: routes based on first key (digit, o, x, u, U, or literal)
    /// - `Decimal/Octal/Hex/UnicodeBmp/UnicodeFull`: collects digits and emits when full
    pub(crate) fn handle_insert_literal(
        key: KeyEvent,
        lit_state: crate::grammar::input_state::InsertLiteralState,
    ) -> GrammarResult {
        use crate::grammar::input_state::InsertLiteralState;

        match lit_state {
            InsertLiteralState::AwaitingFirst => handle_insert_literal_first(key),
            InsertLiteralState::Decimal(digits) => collect_decimal(key, digits),
            InsertLiteralState::Octal(digits) => collect_octal(key, digits),
            InsertLiteralState::Hex(digits) => collect_hex(key, digits),
            InsertLiteralState::UnicodeBmp(digits) => collect_unicode_bmp(key, digits),
            InsertLiteralState::UnicodeFull(digits) => collect_unicode_full(key, digits),
        }
    }

    /// Handle `AwaitingInsertCtrlX` state.
    ///
    /// After Ctrl-X in insert mode, the next key selects a completion kind.
    /// Ctrl-{L,N,P,K,T,I,],F,D,V,U,O,S} and plain `s` map to completion kinds.
    /// Ctrl-E and Ctrl-Y cancel the completion sub-mode.
    /// Any other key is invalid.
    pub(crate) fn handle_awaiting_insert_ctrl_x(key: KeyEvent) -> GrammarResult {
        use crate::keymap::{Key, Modifiers};
        use crate::primitives::CompletionKind;

        // Check for Ctrl-modified keys first
        if key.modifiers.contains(Modifiers::CTRL) {
            if let Key::Char(c) = key.key {
                let kind = match c {
                    'l' => Some(CompletionKind::Line),
                    'n' => Some(CompletionKind::KeywordNext),
                    'p' => Some(CompletionKind::KeywordPrev),
                    'k' => Some(CompletionKind::Dictionary),
                    't' => Some(CompletionKind::Thesaurus),
                    'i' => Some(CompletionKind::IncludePath),
                    ']' => Some(CompletionKind::Tag),
                    'f' => Some(CompletionKind::FileName),
                    'd' => Some(CompletionKind::DefinitionMacro),
                    'v' => Some(CompletionKind::VimCommand),
                    'u' => Some(CompletionKind::UserDefined),
                    'o' => Some(CompletionKind::Omni),
                    's' => Some(CompletionKind::Spelling),
                    // Ctrl-E and Ctrl-Y cancel completion sub-mode
                    'e' | 'y' => return GrammarResult::Cancel,
                    _ => None,
                };
                if let Some(kind) = kind {
                    return GrammarResult::Execute(Command::Insert(
                        InsertKind::RequestCompletion { kind },
                    ));
                }
            }
        }

        // Plain 's' (no modifiers) also triggers spelling completion
        if key.as_char() == Some('s') {
            return GrammarResult::Execute(Command::Insert(InsertKind::RequestCompletion {
                kind: crate::primitives::CompletionKind::Spelling,
            }));
        }

        GrammarResult::Invalid
    }

    /// Handle `AwaitingChar` state.
    ///
    /// After f, F, t, T, or r - we need a target character.
    ///
    /// For non-ASCII base characters, transitions to `AwaitingComposingChars`
    /// so that subsequent Unicode combining marks can be accumulated into a
    /// grapheme cluster (e.g. `e` + U+0301 = `é`). ASCII base characters
    /// execute immediately since combining marks after ASCII are extremely
    /// rare in practice and the one-key delay would hurt common-case UX.
    pub(crate) fn handle_awaiting_char(
        count: Option<u32>,
        register: Option<RegisterName>,
        operator: Option<Operator>,
        char_command: CharCommand,
        key: KeyEvent,
    ) -> GrammarResult {
        // Accept any printable character, plus special keys that Vim treats as valid
        // r<Tab> → replace with tab, r<CR> → replace with newline,
        // r<C-c> → replace with Ctrl-C character (0x03)
        let target_char = key
            .as_char()
            .or({
                match key.key {
                    crate::keymap::Key::Tab => Some('\t'),
                    crate::keymap::Key::Enter => Some('\n'),
                    // drift: named keys other than Tab/Enter have no single-char replacement target; control chars handled by the or_else branch below
                    _ => None,
                }
            })
            .or_else(|| {
                // Control characters: Ctrl-A through Ctrl-Z map to 0x01-0x1A
                // except Ctrl-[ (escape) and Ctrl-C which Vim handles specially
                use crate::keymap::Modifiers;
                if key.modifiers.contains(Modifiers::CTRL) {
                    if let crate::keymap::Key::Char(c) = key.key {
                        // Map ctrl+letter to control character (e.g., Ctrl-C → 0x03)
                        let ctrl_char = (c.to_ascii_lowercase() as u8).wrapping_sub(b'a' - 1);
                        if ctrl_char <= 26 {
                            return char::from_u32(u32::from(ctrl_char));
                        }
                    }
                }
                None
            });
        if let Some(c) = target_char {
            // For non-ASCII characters, transition to AwaitingComposingChars
            // to allow combining marks to be appended to the base character.
            // ASCII characters execute immediately (combining marks after
            // ASCII are extremely rare; avoiding the one-key delay is better UX).
            if !c.is_ascii() {
                let mut grapheme = CompactString::new("");
                grapheme.push(c);
                return GrammarResult::Continue(InputState::AwaitingComposingChars {
                    grapheme,
                    count,
                    register,
                    operator,
                    char_command,
                });
            }
            let mut target = CompactString::new("");
            target.push(c);
            GrammarResult::Execute(Command::CharCommand {
                count: count_or_default(count),
                register,
                operator,
                command: char_command,
                target,
            })
        } else {
            GrammarResult::Invalid
        }
    }

    /// Handle `AwaitingComposingChars` state.
    ///
    /// After a non-ASCII base character has been captured, this state waits
    /// for optional Unicode combining marks. If the next keystroke is a
    /// combining mark it is appended and we stay in this state. Otherwise
    /// the accumulated grapheme is finalized and the command is emitted.
    ///
    /// The non-combining key that terminates the composing sequence is
    /// consumed (not re-processed). This matches Neovim's composing behavior
    /// where `vgetc()` internally accumulates combining marks and returns
    /// the complete grapheme as a single key event.
    pub(crate) fn handle_awaiting_composing_chars(
        grapheme: &CompactString,
        count: Option<u32>,
        register: Option<RegisterName>,
        operator: Option<Operator>,
        char_command: CharCommand,
        key: KeyEvent,
    ) -> GrammarResult {
        // Check if the incoming key is a combining mark
        if let Some(c) = key.as_char() {
            if is_combining_mark(c) {
                let mut new_grapheme = grapheme.clone();
                new_grapheme.push(c);
                return GrammarResult::Continue(InputState::AwaitingComposingChars {
                    grapheme: new_grapheme,
                    count,
                    register,
                    operator,
                    char_command,
                });
            }
        }

        // Non-combining key: finalize with the accumulated grapheme.
        // The terminating key is consumed, matching Neovim's vgetc() behavior.
        GrammarResult::Execute(Command::CharCommand {
            count: count_or_default(count),
            register,
            operator,
            command: char_command,
            target: grapheme.clone(),
        })
    }

    /// Handle `AwaitingMark` state.
    ///
    /// After m, ', or ` - we need a mark name.
    pub(crate) const fn handle_awaiting_mark(
        count: Option<u32>,
        mark_type: MarkType,
        operator: Option<Operator>,
        register: Option<RegisterName>,
        key: KeyEvent,
    ) -> GrammarResult {
        // Accept any valid mark name
        if let Some(c) = key.as_char() {
            if let Some(mark) = MarkName::new(c) {
                // If operator is present, emit OperatorMark
                if let Some(op) = operator {
                    return GrammarResult::Execute(Command::OperatorMark {
                        count: count_or_default(count),
                        register,
                        operator: op,
                        mark,
                        mark_type,
                    });
                }
                // Otherwise emit standalone Mark command
                return GrammarResult::Execute(Command::Mark {
                    count: count_or_default(count),
                    mark_type,
                    mark,
                });
            }
        }
        GrammarResult::Invalid
    }

    /// Handle `AwaitingMacroRegister` state.
    ///
    /// After q or @ - we need a register name.
    pub(crate) fn handle_awaiting_macro_register(
        count: Option<u32>,
        kind: MacroAwaitKind,
        key: KeyEvent,
    ) -> GrammarResult {
        // Escape cancels silently. Ctrl-C also cancels — the error
        // message (E354) is handled at the execution layer when the
        // grammar returns Cancel for a key classified as Escape.
        if key.key() == crate::keymap::Key::Escape || key.is_ctrl_c() {
            return GrammarResult::Cancel;
        }

        // q: / q/ / q? — open command-line history windows
        if kind == MacroAwaitKind::Record {
            if let Some(c) = key.as_char() {
                let prefix_cmd = match c {
                    ':' => Some(PrefixCommand::OpenExHistory),
                    '/' => Some(PrefixCommand::OpenSearchForwardHistory),
                    '?' => Some(PrefixCommand::OpenSearchBackwardHistory),
                    _ => None,
                };
                if let Some(cmd) = prefix_cmd {
                    return GrammarResult::Execute(Command::Prefix {
                        count: count_or_default(count),
                        register: None,
                        command: cmd,
                    });
                }
            }
        }

        // Accept a-z register names (@ also accepts @@ for last macro, @: for last ex command)
        if let Some(c) = key.as_char() {
            // @: must be intercepted before the register check because ':' is
            // a valid register character (read-only last-ex register in Vim).
            if kind == MacroAwaitKind::Play && c == ':' {
                // @: = repeat last ex command
                return GrammarResult::Execute(Command::Macro(MacroKind::RepeatLastEx {
                    count: count_or_default(count),
                }));
            }
            if let Some(reg) = RegisterName::new(c) {
                match kind {
                    MacroAwaitKind::Record if reg.is_named() || reg.is_append() => {
                        // Vim records into named registers (a-z) or append
                        // registers (A-Z). Uppercase appends to the lowercase register.
                        return GrammarResult::Execute(Command::Macro(MacroKind::Record {
                            register: reg,
                        }));
                    }
                    MacroAwaitKind::Play => {
                        // Vim allows playing any register: named (a-z),
                        // numbered (0-9), unnamed ("), clipboard (+/*), etc.
                        return GrammarResult::Execute(Command::Macro(MacroKind::Play {
                            register: reg,
                            count: count_or_default(count),
                        }));
                    }
                    _ => {}
                }
            }
            if kind == MacroAwaitKind::Play && c == '@' {
                // @@ = repeat last macro (uses LAST_MACRO sentinel '@')
                // '@' is not a valid register char, so it's not handled above.
                return GrammarResult::Execute(Command::Macro(MacroKind::Play {
                    register: RegisterName::LAST_MACRO,
                    count: count_or_default(count),
                }));
            }
        }
        GrammarResult::Invalid
    }

    // ── Sneak handlers ────────────────────────────────────────────────────

    /// Handle `AwaitingSneakChar1` state: waiting for first sneak target char.
    pub(crate) const fn handle_awaiting_sneak_char1(
        count: Option<u32>,
        register: Option<RegisterName>,
        operator: Option<Operator>,
        forward: bool,
        key: KeyEvent,
    ) -> GrammarResult {
        if let Some(c1) = key.as_char() {
            GrammarResult::Continue(InputState::AwaitingSneakChar2 {
                count,
                register,
                operator,
                forward,
                c1,
            })
        } else {
            GrammarResult::Invalid
        }
    }

    /// Handle `AwaitingSneakChar2` state: waiting for second sneak target char.
    pub(crate) const fn handle_awaiting_sneak_char2(
        count: Option<u32>,
        register: Option<RegisterName>,
        operator: Option<Operator>,
        forward: bool,
        c1: char,
        key: KeyEvent,
    ) -> GrammarResult {
        if let Some(c2) = key.as_char() {
            GrammarResult::Execute(Command::Sneak {
                count: count_or_default(count),
                register,
                operator,
                c1,
                c2,
                forward,
            })
        } else {
            GrammarResult::Invalid
        }
    }
}

// ── Insert literal helper functions ─────────────────────────────────────────

use crate::grammar::input_state::InsertLiteralState;
use arrayvec::ArrayVec;

/// Handle the first key after Ctrl-V.
fn handle_insert_literal_first(key: KeyEvent) -> GrammarResult {
    use crate::keymap::Key;

    match key.key {
        Key::Char(c) if c.is_ascii_digit() => {
            let mut digits = ArrayVec::new();
            digits.push(c as u8 - b'0');
            GrammarResult::Continue(InputState::InsertLiteral(InsertLiteralState::Decimal(
                digits,
            )))
        }
        Key::Char('o') => GrammarResult::Continue(InputState::InsertLiteral(
            InsertLiteralState::Octal(ArrayVec::new()),
        )),
        Key::Char('x') => GrammarResult::Continue(InputState::InsertLiteral(
            InsertLiteralState::Hex(ArrayVec::new()),
        )),
        Key::Char('u') => GrammarResult::Continue(InputState::InsertLiteral(
            InsertLiteralState::UnicodeBmp(ArrayVec::new()),
        )),
        Key::Char('U') => GrammarResult::Continue(InputState::InsertLiteral(
            InsertLiteralState::UnicodeFull(ArrayVec::new()),
        )),
        Key::Char(c) => {
            GrammarResult::Execute(Command::Insert(InsertKind::LiteralChar { char: c }))
        }
        Key::Escape => {
            GrammarResult::Execute(Command::Insert(InsertKind::LiteralChar { char: '\x1b' }))
        }
        Key::Enter => {
            GrammarResult::Execute(Command::Insert(InsertKind::LiteralChar { char: '\r' }))
        }
        Key::Tab => GrammarResult::Execute(Command::Insert(InsertKind::LiteralChar { char: '\t' })),
        Key::Backspace => {
            GrammarResult::Execute(Command::Insert(InsertKind::LiteralChar { char: '\x08' }))
        }
        Key::Delete => {
            GrammarResult::Execute(Command::Insert(InsertKind::LiteralChar { char: '\x7f' }))
        }
        _ => GrammarResult::Invalid, // drift: unrecognized keys in Ctrl-V literal input are rejected
    }
}

/// Emit a `LiteralChar` command from accumulated digit values.
fn emit_literal(digits: &[u8], radix: u32) -> GrammarResult {
    let value = digits
        .iter()
        .fold(0u32, |acc, &d| acc * radix + u32::from(d));
    let ch = char::from_u32(value).unwrap_or('\0');
    GrammarResult::Execute(Command::Insert(InsertKind::LiteralChar { char: ch }))
}

/// Extract unmodified char from key, if any.
const fn plain_char(key: KeyEvent) -> Option<char> {
    use crate::keymap::{Key, Modifiers};
    if key.modifiers.contains(Modifiers::CTRL) {
        return None;
    }
    match key.key {
        Key::Char(c) => Some(c),
        _ => None, // drift: non-Char keys (Backspace, F-keys, etc.) are not plain chars
    }
}

/// Collect decimal digits (0-9), max 3.
fn collect_decimal(key: KeyEvent, mut digits: ArrayVec<u8, 3>) -> GrammarResult {
    if let Some(c) = plain_char(key) {
        if c.is_ascii_digit() {
            digits.push(c as u8 - b'0');
            if digits.is_full() {
                return emit_literal(&digits, 10);
            }
            return GrammarResult::Continue(InputState::InsertLiteral(
                InsertLiteralState::Decimal(digits),
            ));
        }
    }
    // Non-digit terminates: convert what we have
    emit_literal(&digits, 10)
}

/// Collect octal digits (0-7), max 3.
fn collect_octal(key: KeyEvent, mut digits: ArrayVec<u8, 3>) -> GrammarResult {
    if let Some(c) = plain_char(key) {
        if matches!(c, '0'..='7') {
            digits.push(c as u8 - b'0');
            if digits.is_full() {
                return emit_literal(&digits, 8);
            }
            return GrammarResult::Continue(InputState::InsertLiteral(InsertLiteralState::Octal(
                digits,
            )));
        }
    }
    // Non-octal terminates: convert what we have, or NUL if empty
    emit_literal(&digits, 8)
}

/// Collect hex digits (0-9, a-f, A-F), max 2.
fn collect_hex(key: KeyEvent, mut digits: ArrayVec<u8, 2>) -> GrammarResult {
    if let Some(c) = plain_char(key) {
        if c.is_ascii_hexdigit() {
            digits.push(hex_digit_value(c));
            if digits.is_full() {
                return emit_literal(&digits, 16);
            }
            return GrammarResult::Continue(InputState::InsertLiteral(InsertLiteralState::Hex(
                digits,
            )));
        }
    }
    emit_literal(&digits, 16)
}

/// Collect hex digits for Unicode BMP (0-9, a-f, A-F), max 4.
fn collect_unicode_bmp(key: KeyEvent, mut digits: ArrayVec<u8, 4>) -> GrammarResult {
    if let Some(c) = plain_char(key) {
        if c.is_ascii_hexdigit() {
            digits.push(hex_digit_value(c));
            if digits.is_full() {
                return emit_literal(&digits, 16);
            }
            return GrammarResult::Continue(InputState::InsertLiteral(
                InsertLiteralState::UnicodeBmp(digits),
            ));
        }
    }
    emit_literal(&digits, 16)
}

/// Collect hex digits for full Unicode (0-9, a-f, A-F), max 8.
fn collect_unicode_full(key: KeyEvent, mut digits: ArrayVec<u8, 8>) -> GrammarResult {
    if let Some(c) = plain_char(key) {
        if c.is_ascii_hexdigit() {
            digits.push(hex_digit_value(c));
            if digits.is_full() {
                return emit_literal(&digits, 16);
            }
            return GrammarResult::Continue(InputState::InsertLiteral(
                InsertLiteralState::UnicodeFull(digits),
            ));
        }
    }
    emit_literal(&digits, 16)
}

const fn hex_digit_value(c: char) -> u8 {
    match c {
        '0'..='9' => c as u8 - b'0',
        'a'..='f' => c as u8 - b'a' + 10,
        'A'..='F' => c as u8 - b'A' + 10,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use crate::grammar::command::InsertKind;
    use crate::grammar::result::GrammarResult;
    use crate::grammar::Parser;
    use crate::grammar::{Command, MacroKind};
    use crate::keymap::{KeyEvent, Keymap};
    use crate::primitives::{CompletionKind, Mode};

    #[test]
    fn at_at_produces_macro_play() {
        let mut parser = Parser::new();
        let keymap = Keymap::default();

        // First @ → AwaitingMacroRegister
        let r1 = parser.process(KeyEvent::char('@'), &keymap, Mode::Normal);
        assert!(
            matches!(r1, GrammarResult::Continue(_)),
            "First @ should be Continue, got: {:?}",
            r1
        );

        // Second @ → MacroPlay { LAST_MACRO }
        let r2 = parser.process(KeyEvent::char('@'), &keymap, Mode::Normal);
        assert!(
            matches!(
                r2,
                GrammarResult::Execute(Command::Macro(MacroKind::Play { .. }))
            ),
            "Second @ should be MacroPlay, got: {:?}",
            r2
        );
    }

    #[test]
    fn at_a_produces_macro_play_named() {
        let mut parser = Parser::new();
        let keymap = Keymap::default();

        let r1 = parser.process(KeyEvent::char('@'), &keymap, Mode::Normal);
        assert!(matches!(r1, GrammarResult::Continue(_)));

        let r2 = parser.process(KeyEvent::char('a'), &keymap, Mode::Normal);
        assert!(
            matches!(
                r2,
                GrammarResult::Execute(Command::Macro(MacroKind::Play { .. }))
            ),
            "@a should be MacroPlay, got: {:?}",
            r2
        );
    }

    #[test]
    fn at_colon_produces_repeat_last_ex() {
        let mut parser = Parser::new();
        let keymap = Keymap::default();

        // First @ → AwaitingMacroRegister
        let r1 = parser.process(KeyEvent::char('@'), &keymap, Mode::Normal);
        assert!(
            matches!(r1, GrammarResult::Continue(_)),
            "First @ should be Continue, got: {:?}",
            r1
        );

        // Colon → RepeatLastEx
        let r2 = parser.process(KeyEvent::char(':'), &keymap, Mode::Normal);
        assert!(
            matches!(
                r2,
                GrammarResult::Execute(Command::Macro(MacroKind::RepeatLastEx {
                    count: NonZeroU32::MIN
                }))
            ),
            "@: should be RepeatLastEx, got: {:?}",
            r2
        );
    }

    #[test]
    fn at_colon_with_count_produces_repeat_last_ex() {
        let mut parser = Parser::new();
        let keymap = Keymap::default();

        // 3 → set count
        parser.process(KeyEvent::char('3'), &keymap, Mode::Normal);
        // @ → AwaitingMacroRegister
        let r1 = parser.process(KeyEvent::char('@'), &keymap, Mode::Normal);
        assert!(matches!(r1, GrammarResult::Continue(_)));

        // Colon → RepeatLastEx with count=3
        let r2 = parser.process(KeyEvent::char(':'), &keymap, Mode::Normal);
        assert!(
            matches!(
                r2,
                GrammarResult::Execute(Command::Macro(MacroKind::RepeatLastEx { count })) if count == NonZeroU32::new(3).unwrap()
            ),
            "3@: should be RepeatLastEx{{count:3}}, got: {:?}",
            r2
        );
    }

    // ── Ctrl-X completion sub-mode ──────────────────────────────────────

    /// Helper: send Ctrl-X then a second key in insert mode.
    fn ctrl_x_then(second_key: KeyEvent) -> GrammarResult {
        let mut parser = Parser::new();
        let keymap = Keymap::default();

        let r1 = parser.process(KeyEvent::ctrl('x'), &keymap, Mode::Insert);
        assert_eq!(
            r1,
            GrammarResult::Continue(crate::grammar::InputState::AwaitingInsertCtrlX),
            "Ctrl-X should enter AwaitingInsertCtrlX"
        );

        parser.process(second_key, &keymap, Mode::Insert)
    }

    /// Assert that a Ctrl-X + key produces the expected CompletionKind.
    fn assert_completion(second_key: KeyEvent, expected_kind: CompletionKind) {
        let result = ctrl_x_then(second_key);
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Insert(InsertKind::RequestCompletion {
                kind: expected_kind,
            })),
            "Ctrl-X + {:?} should produce {:?}",
            second_key,
            expected_kind,
        );
    }

    #[test]
    fn ctrl_x_ctrl_l_line_completion() {
        assert_completion(KeyEvent::ctrl('l'), CompletionKind::Line);
    }

    #[test]
    fn ctrl_x_ctrl_n_keyword_next() {
        assert_completion(KeyEvent::ctrl('n'), CompletionKind::KeywordNext);
    }

    #[test]
    fn ctrl_x_ctrl_p_keyword_prev() {
        assert_completion(KeyEvent::ctrl('p'), CompletionKind::KeywordPrev);
    }

    #[test]
    fn ctrl_x_ctrl_k_dictionary() {
        assert_completion(KeyEvent::ctrl('k'), CompletionKind::Dictionary);
    }

    #[test]
    fn ctrl_x_ctrl_t_thesaurus() {
        assert_completion(KeyEvent::ctrl('t'), CompletionKind::Thesaurus);
    }

    #[test]
    fn ctrl_x_ctrl_i_include_path() {
        assert_completion(KeyEvent::ctrl('i'), CompletionKind::IncludePath);
    }

    #[test]
    fn ctrl_x_ctrl_bracket_tag() {
        assert_completion(KeyEvent::ctrl(']'), CompletionKind::Tag);
    }

    #[test]
    fn ctrl_x_ctrl_f_file_name() {
        assert_completion(KeyEvent::ctrl('f'), CompletionKind::FileName);
    }

    #[test]
    fn ctrl_x_ctrl_d_definition_macro() {
        assert_completion(KeyEvent::ctrl('d'), CompletionKind::DefinitionMacro);
    }

    #[test]
    fn ctrl_x_ctrl_v_vim_command() {
        assert_completion(KeyEvent::ctrl('v'), CompletionKind::VimCommand);
    }

    #[test]
    fn ctrl_x_ctrl_u_user_defined() {
        assert_completion(KeyEvent::ctrl('u'), CompletionKind::UserDefined);
    }

    #[test]
    fn ctrl_x_ctrl_o_omni() {
        assert_completion(KeyEvent::ctrl('o'), CompletionKind::Omni);
    }

    #[test]
    fn ctrl_x_ctrl_s_spelling() {
        assert_completion(KeyEvent::ctrl('s'), CompletionKind::Spelling);
    }

    #[test]
    fn ctrl_x_plain_s_spelling() {
        // Plain 's' (no modifiers) also triggers spelling completion.
        assert_completion(KeyEvent::char('s'), CompletionKind::Spelling);
    }

    #[test]
    fn ctrl_x_ctrl_e_cancels() {
        let result = ctrl_x_then(KeyEvent::ctrl('e'));
        assert_eq!(result, GrammarResult::Cancel, "Ctrl-X Ctrl-E should cancel");
    }

    #[test]
    fn ctrl_x_ctrl_y_cancels() {
        let result = ctrl_x_then(KeyEvent::ctrl('y'));
        assert_eq!(result, GrammarResult::Cancel, "Ctrl-X Ctrl-Y should cancel");
    }

    #[test]
    fn ctrl_x_unrecognized_key_invalid() {
        // Plain 'a' (not a completion trigger) should be Invalid.
        let result = ctrl_x_then(KeyEvent::char('a'));
        assert_eq!(
            result,
            GrammarResult::Invalid,
            "Ctrl-X + unrecognized key should be Invalid"
        );
    }

    #[test]
    fn ctrl_x_ctrl_unrecognized_key_invalid() {
        // Ctrl-A after Ctrl-X is not a completion trigger.
        let result = ctrl_x_then(KeyEvent::ctrl('a'));
        assert_eq!(
            result,
            GrammarResult::Invalid,
            "Ctrl-X Ctrl-A should be Invalid"
        );
    }

    #[test]
    fn ctrl_x_escape_cancels() {
        // Escape during Ctrl-X sub-mode is intercepted by the insert mode
        // block in parser.process() before reaching the handler.
        let result = ctrl_x_then(KeyEvent::escape());
        assert_eq!(
            result,
            GrammarResult::Cancel,
            "Ctrl-X then Escape should cancel"
        );
    }

    #[test]
    fn ctrl_x_all_thirteen_kinds() {
        // Exhaustive check of all 13 Ctrl-X completion kinds.
        let cases: [(KeyEvent, CompletionKind); 14] = [
            (KeyEvent::ctrl('l'), CompletionKind::Line),
            (KeyEvent::ctrl('n'), CompletionKind::KeywordNext),
            (KeyEvent::ctrl('p'), CompletionKind::KeywordPrev),
            (KeyEvent::ctrl('k'), CompletionKind::Dictionary),
            (KeyEvent::ctrl('t'), CompletionKind::Thesaurus),
            (KeyEvent::ctrl('i'), CompletionKind::IncludePath),
            (KeyEvent::ctrl(']'), CompletionKind::Tag),
            (KeyEvent::ctrl('f'), CompletionKind::FileName),
            (KeyEvent::ctrl('d'), CompletionKind::DefinitionMacro),
            (KeyEvent::ctrl('v'), CompletionKind::VimCommand),
            (KeyEvent::ctrl('u'), CompletionKind::UserDefined),
            (KeyEvent::ctrl('o'), CompletionKind::Omni),
            (KeyEvent::ctrl('s'), CompletionKind::Spelling),
            (KeyEvent::char('s'), CompletionKind::Spelling), // plain s alias
        ];
        for (key, expected) in &cases {
            assert_completion(*key, *expected);
        }
    }

    #[test]
    fn request_completion_is_not_mutating() {
        // RequestCompletion should not be considered a text mutation.
        let kind = InsertKind::RequestCompletion {
            kind: CompletionKind::Omni,
        };
        assert!(
            !kind.is_mutating(),
            "RequestCompletion should not be mutating"
        );
    }

    #[test]
    fn request_completion_register_is_none() {
        // RequestCompletion has no register.
        let kind = InsertKind::RequestCompletion {
            kind: CompletionKind::FileName,
        };
        assert_eq!(kind.register(), None);
    }

    #[test]
    fn host_inserted_is_mutating() {
        let kind = InsertKind::HostInserted;
        assert!(kind.is_mutating());
    }

    #[test]
    fn host_inserted_register_is_none() {
        let kind = InsertKind::HostInserted;
        assert_eq!(kind.register(), None);
    }

    // ── Ctrl-V literal character insertion ──────────────────────────────

    /// Helper: feed Ctrl-V then a sequence of keys in insert mode.
    fn ctrl_v_sequence(keys: &[KeyEvent]) -> GrammarResult {
        let mut parser = Parser::new();
        let keymap = Keymap::default();

        let r = parser.process(KeyEvent::ctrl('v'), &keymap, Mode::Insert);
        assert!(
            matches!(r, GrammarResult::Continue(_)),
            "Ctrl-V should be Continue, got: {r:?}"
        );

        let mut result = GrammarResult::Invalid;
        for &key in keys {
            result = parser.process(key, &keymap, Mode::Insert);
        }
        result
    }

    /// Assert that a key sequence after Ctrl-V produces a LiteralChar with the expected char.
    fn assert_literal(keys: &[KeyEvent], expected: char) {
        let result = ctrl_v_sequence(keys);
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Insert(InsertKind::LiteralChar { char: expected })),
            "Ctrl-V + {keys:?} should produce LiteralChar('{expected}')"
        );
    }

    // ── Decimal ─────────────────────────────────────────────────────────

    #[test]
    fn ctrl_v_decimal_065_is_a() {
        // Ctrl-V 065 → 'A' (ASCII 65)
        assert_literal(
            &[
                KeyEvent::char('0'),
                KeyEvent::char('6'),
                KeyEvent::char('5'),
            ],
            'A',
        );
    }

    #[test]
    fn ctrl_v_decimal_097_is_lowercase_a() {
        // Ctrl-V 097 → 'a' (ASCII 97)
        assert_literal(
            &[
                KeyEvent::char('0'),
                KeyEvent::char('9'),
                KeyEvent::char('7'),
            ],
            'a',
        );
    }

    #[test]
    fn ctrl_v_decimal_0_terminated_by_non_digit() {
        // Ctrl-V 0 <Esc> → NUL (value 0)
        assert_literal(&[KeyEvent::char('0'), KeyEvent::escape()], '\0');
    }

    #[test]
    fn ctrl_v_decimal_single_digit_terminated() {
        // Ctrl-V 9 <letter> → char(9) = tab
        assert_literal(&[KeyEvent::char('9'), KeyEvent::char('a')], '\u{9}');
    }

    // ── Octal ───────────────────────────────────────────────────────────

    #[test]
    fn ctrl_v_octal_101_is_a() {
        // Ctrl-V o 101 → 'A' (octal 101 = 65)
        assert_literal(
            &[
                KeyEvent::char('o'),
                KeyEvent::char('1'),
                KeyEvent::char('0'),
                KeyEvent::char('1'),
            ],
            'A',
        );
    }

    #[test]
    fn ctrl_v_octal_141_is_lowercase_a() {
        // Ctrl-V o 141 → 'a' (octal 141 = 97)
        assert_literal(
            &[
                KeyEvent::char('o'),
                KeyEvent::char('1'),
                KeyEvent::char('4'),
                KeyEvent::char('1'),
            ],
            'a',
        );
    }

    #[test]
    fn ctrl_v_octal_terminated_by_non_octal() {
        // Ctrl-V o 1 8 → char(1) since '8' is not a valid octal digit
        assert_literal(
            &[
                KeyEvent::char('o'),
                KeyEvent::char('1'),
                KeyEvent::char('8'),
            ],
            '\u{1}',
        );
    }

    #[test]
    fn ctrl_v_octal_empty_terminated() {
        // Ctrl-V o <Esc> → NUL (no digits collected)
        assert_literal(&[KeyEvent::char('o'), KeyEvent::escape()], '\0');
    }

    // ── Hex ─────────────────────────────────────────────────────────────

    #[test]
    fn ctrl_v_hex_41_is_a() {
        // Ctrl-V x 41 → 'A' (0x41 = 65)
        assert_literal(
            &[
                KeyEvent::char('x'),
                KeyEvent::char('4'),
                KeyEvent::char('1'),
            ],
            'A',
        );
    }

    #[test]
    fn ctrl_v_hex_ff() {
        // Ctrl-V x ff → char(255)
        assert_literal(
            &[
                KeyEvent::char('x'),
                KeyEvent::char('f'),
                KeyEvent::char('f'),
            ],
            '\u{ff}',
        );
    }

    #[test]
    fn ctrl_v_hex_uppercase_digits() {
        // Ctrl-V x 4A → 'J' (0x4A = 74)
        assert_literal(
            &[
                KeyEvent::char('x'),
                KeyEvent::char('4'),
                KeyEvent::char('A'),
            ],
            'J',
        );
    }

    #[test]
    fn ctrl_v_hex_terminated_by_non_hex() {
        // Ctrl-V x 4 g → char(4) since 'g' is not hex
        assert_literal(
            &[
                KeyEvent::char('x'),
                KeyEvent::char('4'),
                KeyEvent::char('g'),
            ],
            '\u{4}',
        );
    }

    // ── Unicode BMP ─────────────────────────────────────────────────────

    #[test]
    fn ctrl_v_unicode_bmp_00e9_is_e_acute() {
        // Ctrl-V u 00e9 → 'e' with acute accent (U+00E9)
        assert_literal(
            &[
                KeyEvent::char('u'),
                KeyEvent::char('0'),
                KeyEvent::char('0'),
                KeyEvent::char('e'),
                KeyEvent::char('9'),
            ],
            '\u{00e9}',
        );
    }

    #[test]
    fn ctrl_v_unicode_bmp_03b1_is_alpha() {
        // Ctrl-V u 03b1 → Greek alpha (U+03B1)
        assert_literal(
            &[
                KeyEvent::char('u'),
                KeyEvent::char('0'),
                KeyEvent::char('3'),
                KeyEvent::char('b'),
                KeyEvent::char('1'),
            ],
            '\u{03b1}',
        );
    }

    #[test]
    fn ctrl_v_unicode_bmp_partial_terminated() {
        // Ctrl-V u 4 1 <non-hex> → char(0x41) = 'A'
        assert_literal(
            &[
                KeyEvent::char('u'),
                KeyEvent::char('4'),
                KeyEvent::char('1'),
                KeyEvent::char('z'),
            ],
            'A',
        );
    }

    // ── Unicode Full ────────────────────────────────────────────────────

    #[test]
    fn ctrl_v_unicode_full_0001f600_is_grinning_face() {
        // Ctrl-V U 0001f600 → Grinning face emoji (U+1F600)
        assert_literal(
            &[
                KeyEvent::char('U'),
                KeyEvent::char('0'),
                KeyEvent::char('0'),
                KeyEvent::char('0'),
                KeyEvent::char('1'),
                KeyEvent::char('f'),
                KeyEvent::char('6'),
                KeyEvent::char('0'),
                KeyEvent::char('0'),
            ],
            '\u{1f600}',
        );
    }

    #[test]
    fn ctrl_v_unicode_full_00000041_is_a() {
        // Ctrl-V U 00000041 → 'A'
        assert_literal(
            &[
                KeyEvent::char('U'),
                KeyEvent::char('0'),
                KeyEvent::char('0'),
                KeyEvent::char('0'),
                KeyEvent::char('0'),
                KeyEvent::char('0'),
                KeyEvent::char('0'),
                KeyEvent::char('4'),
                KeyEvent::char('1'),
            ],
            'A',
        );
    }

    // ── Non-digit first key (literal passthrough) ───────────────────────

    #[test]
    fn ctrl_v_literal_char() {
        // Ctrl-V a → insert 'a' literally
        assert_literal(&[KeyEvent::char('a')], 'a');
    }

    #[test]
    fn ctrl_v_escape_literal() {
        // Ctrl-V Escape → insert ESC character
        assert_literal(&[KeyEvent::escape()], '\x1b');
    }

    #[test]
    fn ctrl_v_tab_literal() {
        // Ctrl-V Tab → insert tab
        assert_literal(
            &[KeyEvent::new(
                crate::keymap::Key::Tab,
                crate::keymap::Modifiers::NONE,
            )],
            '\t',
        );
    }

    #[test]
    fn ctrl_v_enter_literal() {
        // Ctrl-V Enter → insert carriage return
        assert_literal(&[KeyEvent::enter()], '\r');
    }

    // ── pending_display tests ───────────────────────────────────────────

    #[test]
    fn ctrl_v_pending_display_awaiting_first() {
        use crate::grammar::input_state::{InputState, InsertLiteralState};
        let s = InputState::InsertLiteral(InsertLiteralState::AwaitingFirst);
        assert_eq!(s.pending_display().as_str(), "^V");
    }

    #[test]
    fn ctrl_v_pending_display_decimal() {
        use crate::grammar::input_state::{InputState, InsertLiteralState};
        let mut digits = arrayvec::ArrayVec::new();
        digits.push(6);
        digits.push(5);
        let s = InputState::InsertLiteral(InsertLiteralState::Decimal(digits));
        assert_eq!(s.pending_display().as_str(), "^V65");
    }

    #[test]
    fn ctrl_v_pending_display_hex() {
        use crate::grammar::input_state::{InputState, InsertLiteralState};
        let mut digits = arrayvec::ArrayVec::new();
        digits.push(4);
        digits.push(1);
        let s = InputState::InsertLiteral(InsertLiteralState::Hex(digits));
        assert_eq!(s.pending_display().as_str(), "^Vx41");
    }

    #[test]
    fn ctrl_v_pending_display_octal() {
        use crate::grammar::input_state::{InputState, InsertLiteralState};
        let mut digits = arrayvec::ArrayVec::new();
        digits.push(1);
        let s = InputState::InsertLiteral(InsertLiteralState::Octal(digits));
        assert_eq!(s.pending_display().as_str(), "^Vo1");
    }

    #[test]
    fn ctrl_v_pending_display_unicode_bmp() {
        use crate::grammar::input_state::{InputState, InsertLiteralState};
        let mut digits = arrayvec::ArrayVec::new();
        digits.push(0);
        digits.push(0);
        digits.push(14); // 'e'
        let s = InputState::InsertLiteral(InsertLiteralState::UnicodeBmp(digits));
        assert_eq!(s.pending_display().as_str(), "^Vu00e");
    }

    #[test]
    fn ctrl_v_pending_display_unicode_full() {
        use crate::grammar::input_state::{InputState, InsertLiteralState};
        let mut digits = arrayvec::ArrayVec::new();
        digits.push(1);
        digits.push(15); // 'f'
        let s = InputState::InsertLiteral(InsertLiteralState::UnicodeFull(digits));
        assert_eq!(s.pending_display().as_str(), "^VU1f");
    }

    // ═══════════════════════════════════════════════════════════════════
    // Sneak grammar tests
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn sneak_forward_sab_produces_sneak_command() {
        let mut parser = Parser::new();
        parser.set_sneak_mode(true);
        let keymap = Keymap::default();

        let r1 = parser.process(KeyEvent::char('s'), &keymap, Mode::Normal);
        assert!(matches!(
            r1,
            GrammarResult::Continue(
                crate::grammar::input_state::InputState::AwaitingSneakChar1 { forward: true, .. }
            )
        ));

        let r2 = parser.process(KeyEvent::char('a'), &keymap, Mode::Normal);
        assert!(matches!(
            r2,
            GrammarResult::Continue(
                crate::grammar::input_state::InputState::AwaitingSneakChar2 {
                    c1: 'a',
                    forward: true,
                    ..
                }
            )
        ));

        let r3 = parser.process(KeyEvent::char('b'), &keymap, Mode::Normal);
        match r3 {
            GrammarResult::Execute(Command::Sneak {
                c1, c2, forward, ..
            }) => {
                assert_eq!(c1, 'a');
                assert_eq!(c2, 'b');
                assert!(forward);
            }
            other => panic!("expected Sneak command, got {other:?}"),
        }
    }

    #[test]
    fn sneak_backward_sab_produces_sneak_command() {
        let mut parser = Parser::new();
        parser.set_sneak_mode(true);
        let keymap = Keymap::default();

        let r1 = parser.process(KeyEvent::char('S'), &keymap, Mode::Normal);
        assert!(matches!(
            r1,
            GrammarResult::Continue(
                crate::grammar::input_state::InputState::AwaitingSneakChar1 { forward: false, .. }
            )
        ));

        parser.process(KeyEvent::char('x'), &keymap, Mode::Normal);
        let r3 = parser.process(KeyEvent::char('y'), &keymap, Mode::Normal);
        match r3 {
            GrammarResult::Execute(Command::Sneak {
                c1, c2, forward, ..
            }) => {
                assert_eq!(c1, 'x');
                assert_eq!(c2, 'y');
                assert!(!forward);
            }
            other => panic!("expected Sneak command, got {other:?}"),
        }
    }

    #[test]
    fn sneak_disabled_s_produces_substitute() {
        let mut parser = Parser::new();
        // sneak_mode defaults to false
        let keymap = Keymap::default();

        let r = parser.process(KeyEvent::char('s'), &keymap, Mode::Normal);
        assert!(
            matches!(r, GrammarResult::Execute(Command::InsertEntry { .. })),
            "without sneak_mode, s should produce InsertEntry (substitute), got {r:?}"
        );
    }

    #[test]
    fn sneak_operator_dsab() {
        let mut parser = Parser::new();
        parser.set_sneak_mode(true);
        let keymap = Keymap::default();

        // d → Operator
        parser.process(KeyEvent::char('d'), &keymap, Mode::Normal);
        // s → AwaitingSneakChar1 with operator
        let r2 = parser.process(KeyEvent::char('s'), &keymap, Mode::Normal);
        assert!(matches!(
            r2,
            GrammarResult::Continue(
                crate::grammar::input_state::InputState::AwaitingSneakChar1 {
                    operator: Some(crate::grammar::types::Operator::Delete),
                    forward: true,
                    ..
                }
            )
        ));

        parser.process(KeyEvent::char('a'), &keymap, Mode::Normal);
        let r4 = parser.process(KeyEvent::char('b'), &keymap, Mode::Normal);
        match r4 {
            GrammarResult::Execute(Command::Sneak {
                c1,
                c2,
                forward,
                operator,
                ..
            }) => {
                assert_eq!(c1, 'a');
                assert_eq!(c2, 'b');
                assert!(forward);
                assert_eq!(operator, Some(crate::grammar::types::Operator::Delete));
            }
            other => panic!("expected Sneak with Delete operator, got {other:?}"),
        }
    }

    #[test]
    fn sneak_expects_literal_char() {
        use crate::grammar::input_state::InputState;
        let s = InputState::AwaitingSneakChar1 {
            count: None,
            register: None,
            operator: None,
            forward: true,
        };
        assert!(s.expects_literal_char());

        let s2 = InputState::AwaitingSneakChar2 {
            count: None,
            register: None,
            operator: None,
            forward: true,
            c1: 'a',
        };
        assert!(s2.expects_literal_char());
    }

    /// Helper: feed keys with sneak enabled and return the final result.
    fn sneak_keys(keys: &[char]) -> GrammarResult {
        let mut parser = Parser::new();
        parser.set_sneak_mode(true);
        let km = Keymap::default();
        let mut r = GrammarResult::Invalid;
        for &c in keys {
            r = parser.process(KeyEvent::char(c), &km, Mode::Normal);
        }
        r
    }

    #[test]
    fn sneak_operator_csab() {
        match sneak_keys(&['c', 's', 'x', 'y']) {
            GrammarResult::Execute(Command::Sneak {
                c1,
                c2,
                forward,
                operator,
                ..
            }) => {
                assert_eq!((c1, c2, forward), ('x', 'y', true));
                assert_eq!(operator, Some(crate::grammar::types::Operator::Change));
            }
            other => panic!("expected Sneak with Change, got {other:?}"),
        }
    }

    #[test]
    fn sneak_disabled_s_upper_produces_substitute_line() {
        let mut p = Parser::new();
        let r = p.process(KeyEvent::char('S'), &Keymap::default(), Mode::Normal);
        assert!(
            matches!(r, GrammarResult::Execute(Command::InsertEntry { .. })),
            "without sneak_mode, S should be substitute, got {r:?}"
        );
    }

    #[test]
    fn sneak_with_count_grammar() {
        let mut p = Parser::new();
        p.set_sneak_mode(true);
        let km = Keymap::default();
        p.process(KeyEvent::char('2'), &km, Mode::Normal);
        p.process(KeyEvent::char('s'), &km, Mode::Normal);
        p.process(KeyEvent::char('a'), &km, Mode::Normal);
        match p.process(KeyEvent::char('b'), &km, Mode::Normal) {
            GrammarResult::Execute(Command::Sneak { count, c1, c2, .. }) => {
                assert_eq!((count.get(), c1, c2), (2, 'a', 'b'));
            }
            other => panic!("expected Sneak count=2, got {other:?}"),
        }
    }

    #[test]
    fn sneak_backward_operator_d_upper_s() {
        match sneak_keys(&['d', 'S', 'x', 'y']) {
            GrammarResult::Execute(Command::Sneak {
                forward, operator, ..
            }) => {
                assert!(!forward);
                assert_eq!(operator, Some(crate::grammar::types::Operator::Delete));
            }
            other => panic!("expected backward Sneak with Delete, got {other:?}"),
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // Composing character tests
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn fa_ascii_executes_immediately() {
        // ASCII target: no composing wait, Execute on the char key itself.
        let mut parser = Parser::new();
        let keymap = Keymap::default();

        let r1 = parser.process(KeyEvent::char('f'), &keymap, Mode::Normal);
        assert!(matches!(r1, GrammarResult::Continue(_)));

        let r2 = parser.process(KeyEvent::char('a'), &keymap, Mode::Normal);
        match r2 {
            GrammarResult::Execute(Command::CharCommand {
                command,
                ref target,
                ..
            }) => {
                assert_eq!(command, crate::grammar::types::CharCommand::FindForward);
                assert_eq!(target.as_str(), "a");
            }
            other => panic!("expected Execute(CharCommand), got {other:?}"),
        }
    }

    #[test]
    fn f_combining_acute_on_base() {
        // Non-ASCII base + combining acute (U+0301) -> grapheme cluster.
        let mut parser = Parser::new();
        let keymap = Keymap::default();

        // f
        parser.process(KeyEvent::char('f'), &keymap, Mode::Normal);

        // Non-ASCII base char (e.g. Latin 'e' with cedilla U+0229)
        let r2 = parser.process(KeyEvent::char('\u{0229}'), &keymap, Mode::Normal);
        assert!(
            matches!(
                r2,
                GrammarResult::Continue(
                    crate::grammar::input_state::InputState::AwaitingComposingChars { .. }
                )
            ),
            "non-ASCII base should enter AwaitingComposingChars, got {r2:?}"
        );

        // Combining acute accent (U+0301)
        let r3 = parser.process(KeyEvent::char('\u{0301}'), &keymap, Mode::Normal);
        assert!(
            matches!(
                r3,
                GrammarResult::Continue(
                    crate::grammar::input_state::InputState::AwaitingComposingChars { .. }
                )
            ),
            "combining mark should stay in AwaitingComposingChars, got {r3:?}"
        );

        // Non-combining key finalizes
        let r4 = parser.process(KeyEvent::char('x'), &keymap, Mode::Normal);
        match r4 {
            GrammarResult::Execute(Command::CharCommand {
                command,
                ref target,
                ..
            }) => {
                assert_eq!(command, crate::grammar::types::CharCommand::FindForward);
                // Target should be base + combining mark
                assert_eq!(target.as_str(), "\u{0229}\u{0301}");
            }
            other => panic!("expected Execute(CharCommand) with composed target, got {other:?}"),
        }
    }

    #[test]
    fn r_non_ascii_with_combining_grave() {
        // r + non-ASCII base + combining grave (U+0300) -> replace with grapheme
        let mut parser = Parser::new();
        let keymap = Keymap::default();

        // r
        parser.process(KeyEvent::char('r'), &keymap, Mode::Normal);

        // Non-ASCII base (e.g. Greek alpha U+03B1)
        let r2 = parser.process(KeyEvent::char('\u{03B1}'), &keymap, Mode::Normal);
        assert!(matches!(
            r2,
            GrammarResult::Continue(
                crate::grammar::input_state::InputState::AwaitingComposingChars { .. }
            )
        ));

        // Combining grave (U+0300)
        let r3 = parser.process(KeyEvent::char('\u{0300}'), &keymap, Mode::Normal);
        assert!(matches!(
            r3,
            GrammarResult::Continue(
                crate::grammar::input_state::InputState::AwaitingComposingChars { .. }
            )
        ));

        // Non-combining finalizes
        let r4 = parser.process(KeyEvent::char('z'), &keymap, Mode::Normal);
        match r4 {
            GrammarResult::Execute(Command::CharCommand {
                command,
                ref target,
                ..
            }) => {
                assert_eq!(command, crate::grammar::types::CharCommand::Replace);
                assert_eq!(target.as_str(), "\u{03B1}\u{0300}");
            }
            other => panic!("expected Replace with composed target, got {other:?}"),
        }
    }

    #[test]
    fn t_non_ascii_base_plus_combining_plus_noncombining() {
        // t + non-ASCII base + combining + non-combining: finalizes at 2 chars,
        // non-combining is consumed as the terminator.
        let mut parser = Parser::new();
        let keymap = Keymap::default();

        parser.process(KeyEvent::char('t'), &keymap, Mode::Normal);

        // Non-ASCII base (e.g. Devanagari Ka U+0915)
        let r2 = parser.process(KeyEvent::char('\u{0915}'), &keymap, Mode::Normal);
        assert!(matches!(
            r2,
            GrammarResult::Continue(
                crate::grammar::input_state::InputState::AwaitingComposingChars { .. }
            )
        ));

        // Combining mark (Devanagari Vowel Sign Aa, U+093E)
        let r3 = parser.process(KeyEvent::char('\u{093E}'), &keymap, Mode::Normal);
        assert!(matches!(
            r3,
            GrammarResult::Continue(
                crate::grammar::input_state::InputState::AwaitingComposingChars { .. }
            )
        ));

        // Non-combining key finalizes
        let r4 = parser.process(KeyEvent::char('j'), &keymap, Mode::Normal);
        match r4 {
            GrammarResult::Execute(Command::CharCommand {
                command,
                ref target,
                ..
            }) => {
                assert_eq!(command, crate::grammar::types::CharCommand::TillForward);
                assert_eq!(target.as_str(), "\u{0915}\u{093E}");
            }
            other => panic!("expected TillForward with composed target, got {other:?}"),
        }
    }

    #[test]
    fn composing_pending_display() {
        // Verify pending_display for AwaitingComposingChars state
        use crate::grammar::input_state::InputState;
        use compact_str::CompactString;

        let s = InputState::AwaitingComposingChars {
            grapheme: CompactString::from("\u{0915}\u{093E}"),
            count: Some(2),
            register: None,
            operator: None,
            char_command: crate::grammar::types::CharCommand::FindForward,
        };
        // Should display: "2f" + the grapheme
        assert_eq!(s.pending_display().as_str(), "2f\u{0915}\u{093E}");
    }

    #[test]
    fn composing_expects_literal_char() {
        use crate::grammar::input_state::InputState;
        use compact_str::CompactString;

        let s = InputState::AwaitingComposingChars {
            grapheme: CompactString::from("e"),
            count: None,
            register: None,
            operator: None,
            char_command: crate::grammar::types::CharCommand::FindForward,
        };
        assert!(s.expects_literal_char());
    }

    #[test]
    fn is_combining_mark_basic() {
        // U+0301 = combining acute accent
        assert!(super::is_combining_mark('\u{0301}'));
        // U+0300 = combining grave accent
        assert!(super::is_combining_mark('\u{0300}'));
        // U+0302 = combining circumflex
        assert!(super::is_combining_mark('\u{0302}'));
        // U+0327 = combining cedilla
        assert!(super::is_combining_mark('\u{0327}'));
        // Regular ASCII characters are NOT combining marks
        assert!(!super::is_combining_mark('a'));
        assert!(!super::is_combining_mark('Z'));
        assert!(!super::is_combining_mark('0'));
        assert!(!super::is_combining_mark(' '));
        assert!(!super::is_combining_mark('!'));
    }

    #[test]
    fn is_combining_mark_non_latin_scripts() {
        // Hebrew combining marks
        assert!(super::is_combining_mark('\u{05B0}')); // Hebrew Point Sheva
                                                       // Arabic combining marks
        assert!(super::is_combining_mark('\u{064B}')); // Arabic Fathatan
                                                       // Thai combining marks
        assert!(super::is_combining_mark('\u{0E31}')); // Thai Mai Han Akat
        assert!(super::is_combining_mark('\u{0E34}')); // Thai Sara I
                                                       // Devanagari combining marks
        assert!(super::is_combining_mark('\u{093E}')); // Devanagari Vowel Sign Aa
        assert!(super::is_combining_mark('\u{0940}')); // Devanagari Vowel Sign Ii
    }

    #[test]
    fn composing_escape_cancels() {
        // Escape during AwaitingComposingChars should cancel
        let mut parser = Parser::new();
        let keymap = Keymap::default();

        parser.process(KeyEvent::char('f'), &keymap, Mode::Normal);
        // Non-ASCII base
        parser.process(KeyEvent::char('\u{0915}'), &keymap, Mode::Normal);
        // Escape cancels
        let r = parser.process(KeyEvent::escape(), &keymap, Mode::Normal);
        assert_eq!(r, GrammarResult::Cancel);
    }

    #[test]
    fn composing_operator_pending() {
        // d + f + non-ASCII + combining + non-combining
        let mut parser = Parser::new();
        let keymap = Keymap::default();

        parser.process(KeyEvent::char('d'), &keymap, Mode::Normal);
        parser.process(KeyEvent::char('f'), &keymap, Mode::Normal);
        // Non-ASCII base
        parser.process(KeyEvent::char('\u{03B1}'), &keymap, Mode::Normal);
        // Combining
        parser.process(KeyEvent::char('\u{0301}'), &keymap, Mode::Normal);
        // Finalize
        let r = parser.process(KeyEvent::char('x'), &keymap, Mode::Normal);
        match r {
            GrammarResult::Execute(Command::CharCommand {
                command,
                ref target,
                operator,
                ..
            }) => {
                assert_eq!(command, crate::grammar::types::CharCommand::FindForward);
                assert_eq!(target.as_str(), "\u{03B1}\u{0301}");
                assert_eq!(operator, Some(crate::grammar::types::Operator::Delete));
            }
            other => panic!("expected operator + CharCommand, got {other:?}"),
        }
    }
}
