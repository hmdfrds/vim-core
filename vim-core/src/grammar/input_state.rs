//! Input state enum.
//!
//! Represents the current state of command parsing.

use super::types::{CharCommand, MarkType, Operator, TextObjectScope};
use crate::primitives::{MotionType, RegisterName};
use arrayvec::ArrayVec;
use compact_str::CompactString;
use smart_default::SmartDefault;
use std::fmt::Write;

/// Phase of register selection in `AwaitingRegister` state.
///
/// Encodes the invariant that operator context and count2 are always
/// logically paired: both absent (from Ready) or both present (from Operator).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegisterPhase {
    /// From Ready state: `"a3dw` — no operator pending.
    BeforeOperator,
    /// From Operator state: `d"aw` — operator is pending.
    AfterOperator {
        /// The pending operator
        operator: Operator,
        /// Count after operator (e.g., `d3"aw`)
        count2: Option<u32>,
    },
}

/// Whether the macro register await is for recording or playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MacroAwaitKind {
    /// Recording a macro (`q{reg}`)
    Record,
    /// Playing a macro (`@{reg}`)
    Play,
}

/// Sub-state for multi-format literal character insertion (Ctrl-V).
///
/// After Ctrl-V in insert mode, the parser enters `AwaitingFirst` and then
/// transitions to a digit-collecting sub-state based on the first key:
///
/// | First key | Sub-state | Radix | Max digits |
/// |-----------|-----------|-------|------------|
/// | `0`-`9`   | `Decimal` | 10    | 3          |
/// | `o`       | `Octal`   | 8     | 3          |
/// | `x`       | `Hex`     | 16    | 2          |
/// | `u`       | `UnicodeBmp` | 16 | 4          |
/// | `U`       | `UnicodeFull` | 16 | 8         |
/// | other     | *(immediate literal insert)* | | |
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsertLiteralState {
    /// Waiting for the first key after Ctrl-V.
    AwaitingFirst,
    /// Collecting decimal digits (0-9), max 3. Values 0-255.
    Decimal(ArrayVec<u8, 3>),
    /// Collecting octal digits (0-7), max 3. Values 0-377 (0-255).
    Octal(ArrayVec<u8, 3>),
    /// Collecting hex digits (0-9, a-f), max 2. Values 0x00-0xFF.
    Hex(ArrayVec<u8, 2>),
    /// Collecting hex digits for Unicode BMP, max 4. Values 0x0000-0xFFFF.
    UnicodeBmp(ArrayVec<u8, 4>),
    /// Collecting hex digits for full Unicode, max 8. Values 0x00000000-0x7FFFFFFF.
    UnicodeFull(ArrayVec<u8, 8>),
}

/// Current state of command parsing.
///
/// The grammar parser is a state machine. Each state represents
/// what the parser has seen so far and what it expects next.
#[derive(Debug, Clone, PartialEq, Eq, SmartDefault)]
#[non_exhaustive]
pub enum InputState {
    /// Ready for new command.
    ///
    /// This is the initial state. From here, the parser can accept:
    /// - Digit (1-9) → builds count
    /// - `RegisterTrigger` (") → awaits register name
    /// - Operator (d, c, y) → awaits motion/textobj
    /// - Motion (j, w, $) → execute immediately
    /// - `CharMotion` (f, t) → awaits char
    /// - Action (x, p, .) → execute immediately
    /// - `ModeSwitch` (i, v) → change mode
    /// - Prefix (g, z) → awaits continuation
    #[default]
    Ready {
        /// Accumulated count (e.g., "3" in "3dw")
        count: Option<u32>,
        /// Selected register (e.g., "a" in "\"a3dw")
        register: Option<RegisterName>,
    },

    /// Waiting for register name after ".
    ///
    /// Reached from Ready state (`"a3dw`) or Operator state (`d"aw`).
    /// The `phase` field encodes whether we came from Ready or Operator,
    /// making the invariant type-safe.
    AwaitingRegister {
        /// Accumulated count before register
        count: Option<u32>,
        /// Whether we're before or after an operator
        phase: RegisterPhase,
    },

    /// Have operator, waiting for motion or text object.
    ///
    /// Example: After pressing 'd' in "dw"
    Operator {
        /// Count before operator (e.g., "2" in "2dw")
        count: Option<u32>,
        /// Selected register
        register: Option<RegisterName>,
        /// The operator (d, c, y, etc.)
        operator: Operator,
        /// Count after operator (e.g., "3" in "d3w")
        count2: Option<u32>,
        /// Motion force override (`v` = charwise, `V` = linewise, `Ctrl-V` = blockwise).
        /// Set by pressing v/V/Ctrl-V in operator-pending mode (e.g., `dv$` forces charwise).
        force_type: Option<MotionType>,
    },

    /// Waiting for character argument.
    ///
    /// Example: After pressing 'f' in "fa"
    AwaitingChar {
        /// Accumulated count
        count: Option<u32>,
        /// Selected register
        register: Option<RegisterName>,
        /// Optional operator (for "dfa")
        operator: Option<Operator>,
        /// The char command (f, F, t, T, r)
        char_command: CharCommand,
    },

    /// Waiting for text object type after 'i' or 'a'.
    ///
    /// Example: After pressing 'i' in "diw"
    AwaitingTextObject {
        /// Accumulated count
        count: Option<u32>,
        /// Selected register
        register: Option<RegisterName>,
        /// The operator (text objects require an operator)
        operator: Operator,
        /// Inner or around scope.
        scope: TextObjectScope,
    },

    /// Waiting for text object type after targets.vim seek modifier (`n` or `l`).
    ///
    /// Three-key sequence: `i`/`a` → `n`/`l` → delimiter key.
    /// Example: After pressing 'n' in "din\"" (delete inside next double quote).
    AwaitingTextObjectWithModifier {
        /// Accumulated count
        count: Option<u32>,
        /// Selected register
        register: Option<RegisterName>,
        /// The operator (text objects require an operator)
        operator: Operator,
        /// Inner or around scope.
        scope: TextObjectScope,
        /// Seek direction: Next (n) or Last (l).
        seek: super::types::SeekDirection,
    },

    /// Waiting for text object type after targets.vim seek modifier in visual mode.
    ///
    /// Three-key sequence: `i`/`a` → `n`/`l` → delimiter key (in visual mode).
    /// Example: After pressing 'n' in "vin\"" (visual inside next double quote).
    AwaitingVisualTextObjectWithModifier {
        /// Accumulated count
        count: Option<u32>,
        /// Inner or around scope.
        scope: TextObjectScope,
        /// Register selected before text object
        register: Option<RegisterName>,
        /// Seek direction: Next (n) or Last (l).
        seek: super::types::SeekDirection,
    },

    /// Waiting for prefix continuation.
    ///
    /// Example: After pressing 'g' in "gg" or "gU"
    AwaitingPrefix {
        /// Accumulated count
        count: Option<u32>,
        /// Selected register
        register: Option<RegisterName>,
        /// The prefix key (`g`, `z`, `[`, `]`)
        prefix: char,
        /// Optional operator (for "dgj")
        operator: Option<Operator>,
        /// Optional motion type override (from `v`/`V`/`Ctrl-V` in operator-pending)
        force_type: Option<MotionType>,
    },

    /// Waiting for mark name.
    ///
    /// Example: After pressing 'm' in "ma"
    AwaitingMark {
        /// Accumulated count (for jump marks)
        count: Option<u32>,
        /// The mark command type (m, ', `)
        mark_type: MarkType,
        /// Pending operator (for operator-mark motions like y'a)
        operator: Option<Operator>,
        /// Register for operator
        register: Option<RegisterName>,
    },

    /// Waiting for register name in insert mode (after Ctrl-R).
    ///
    /// Example: After pressing Ctrl-R waiting for register like "a" or "0"
    AwaitingInsertRegister,

    /// Collecting expression text for `<C-r>=` in insert mode.
    ///
    /// After `<C-r>=`, collects characters until `<CR>` and then emits
    /// an `InsertKind::ExpressionResult` command with the collected text.
    AwaitingInsertExpression {
        /// The expression text accumulated so far.
        /// Uses `CompactString` for SSO — expression text is typically short.
        collected: CompactString,
    },

    /// Waiting for sub-command after Ctrl-G in insert mode.
    ///
    /// Sub-commands: `u` (break undo), `j`/Down (cursor down), `k`/Up (cursor up)
    AwaitingInsertCtrlG,

    /// Waiting for first digraph character after Ctrl-K.
    ///
    /// Example: After pressing Ctrl-K, waiting for first char of digraph pair.
    AwaitingInsertDigraph1,

    /// Waiting for second digraph character.
    ///
    /// Example: After pressing Ctrl-K then 'a', waiting for second char.
    AwaitingInsertDigraph2 {
        /// First character of the digraph pair.
        c1: char,
    },

    /// Waiting for completion sub-command after Ctrl-X in insert mode.
    ///
    /// Sub-commands: Ctrl-{L,N,P,K,T,I,],F,D,V,U,O,S} and plain `s`.
    /// Ctrl-E/Ctrl-Y cancel. Anything else is invalid.
    AwaitingInsertCtrlX,

    /// Ctrl-V literal character insertion (multi-format).
    ///
    /// After Ctrl-V in insert mode, collects character codes in various formats:
    /// decimal, octal, hex, Unicode BMP, or Unicode full.
    /// The sub-state tracks which format and digits collected so far.
    InsertLiteral(InsertLiteralState),

    /// Waiting for register name for macro.
    ///
    /// Examples:
    /// - After pressing 'q' waiting for register to record to
    /// - After pressing '@' waiting for register to play from
    AwaitingMacroRegister {
        /// Count for macro playback (e.g., "3@a")
        count: Option<u32>,
        /// Whether we're recording (q) or playing (@).
        kind: MacroAwaitKind,
    },

    /// Waiting for text object type after 'i' or 'a' in visual mode.
    ///
    /// Example: After pressing 'i' in visual mode for "viw"
    /// Unlike `AwaitingTextObject`, this doesn't require an operator -
    /// it sets the selection range directly.
    AwaitingVisualTextObject {
        /// Accumulated count
        count: Option<u32>,
        /// Inner or around scope.
        scope: TextObjectScope,
        /// Register selected before text object (preserved for subsequent operator)
        register: Option<RegisterName>,
    },

    /// Waiting for window sub-command after `Ctrl-W`.
    ///
    /// Example: After pressing `Ctrl-W` waiting for `s`, `v`, `h`, etc.
    AwaitingWindowCommand {
        /// Accumulated count (e.g., "3Ctrl-W+" to increase height by 3)
        count: Option<u32>,
        /// Selected register (typically unused for window commands)
        register: Option<RegisterName>,
    },

    /// Waiting for first sneak character after `s`/`S` (sneak_mode enabled).
    ///
    /// Example: After pressing `s` in sneak mode, waiting for first target char.
    AwaitingSneakChar1 {
        /// Accumulated count
        count: Option<u32>,
        /// Selected register
        register: Option<RegisterName>,
        /// Optional operator (for `dsab`)
        operator: Option<Operator>,
        /// Search direction: true = forward (`s`), false = backward (`S`)
        forward: bool,
    },

    /// Waiting for second sneak character.
    ///
    /// Example: After pressing `s` then `a`, waiting for second target char.
    AwaitingSneakChar2 {
        /// Accumulated count
        count: Option<u32>,
        /// Selected register
        register: Option<RegisterName>,
        /// Optional operator (for `dsab`)
        operator: Option<Operator>,
        /// Search direction
        forward: bool,
        /// First target character
        c1: char,
    },

    /// Waiting for the key after `z` in visual mode.
    ///
    /// `zy` → YankTrimmed (block yank trimming trailing whitespace).
    /// Any other key falls through to normal z-prefix handling.
    AwaitingVisualZPrefix {
        /// The register to yank into.
        register: Option<crate::primitives::RegisterName>,
    },

    /// Waiting for surround char after `ys{motion}` resolves or visual `S`.
    ///
    /// The motion/textobject has already been parsed; now we need the
    /// delimiter character to wrap the range with.
    AwaitingSurroundChar {
        /// Accumulated count
        count: Option<u32>,
        /// The motion that defines the range (None for visual S)
        motion: Option<super::types::Motion>,
        /// Text object if used instead of motion
        textobject: Option<super::types::TextObject>,
    },

    /// Waiting for surround delete char after `ds`.
    ///
    /// Example: After pressing `ds`, waiting for the delimiter char to find and remove.
    AwaitingSurroundDeleteChar,

    /// Waiting for the "old" char in `cs{old}{new}`.
    ///
    /// Example: After pressing `cs`, waiting for the char identifying the existing pair.
    AwaitingSurroundOldChar,

    /// Waiting for the "new" char in `cs{old}{new}`.
    ///
    /// Example: After pressing `cs"`, waiting for the replacement delimiter.
    AwaitingSurroundNewChar {
        /// The old delimiter character that was already typed.
        old_char: char,
    },

    /// Accumulating combining marks after the base character for f/t/r.
    ///
    /// After receiving a base character in `AwaitingChar`, the parser enters
    /// this state if the command supports composing characters (f, F, t, T, r).
    /// Each subsequent keystroke that is a Unicode combining mark (category
    /// Mn/Mc/Me) is appended to `grapheme`. When a non-combining key arrives,
    /// the accumulated grapheme is finalized and the non-combining key is
    /// re-processed.
    AwaitingComposingChars {
        /// Accumulated grapheme cluster so far (base + combining marks).
        grapheme: CompactString,
        /// Accumulated count
        count: Option<u32>,
        /// Selected register
        register: Option<RegisterName>,
        /// Optional operator (for "dfa")
        operator: Option<Operator>,
        /// The char command (f, F, t, T, r)
        char_command: CharCommand,
    },

    /// Intermediate state for `Ctrl-\ Ctrl-N` universal escape sequence.
    ///
    /// Reached from any mode when `Ctrl-\` is pressed. The parser waits for
    /// the next key:
    /// - `Ctrl-N` → unconditional `ModeChange(Normal)`
    /// - Anything else → cancel, returning to the previous mode/state
    AwaitingCtrlBackslashN,
}

/// Write the register prefix (`"x`) to the buffer if present.
fn write_register(buf: &mut CompactString, register: Option<RegisterName>) {
    if let Some(reg) = register {
        buf.push('"');
        buf.push(reg.char());
    }
}

/// Write the count digits to the buffer if present.
fn write_count(buf: &mut CompactString, count: Option<u32>) {
    if let Some(n) = count {
        let _ = write!(buf, "{n}");
    }
}

/// Write the operator key notation to the buffer if present.
fn write_operator(buf: &mut CompactString, operator: Option<&Operator>) {
    if let Some(op) = operator {
        buf.push_str(op.key_notation());
    }
}

/// Convert a digit value (0-15) to its lowercase hex character.
const fn hex_digit_char(d: u8) -> char {
    match d {
        0..=9 => (d + b'0') as char,
        10..=15 => (d - 10 + b'a') as char,
        _ => '?',
    }
}

impl InputState {
    /// Format the current parser state into a Vim showcmd string.
    ///
    /// Returns the partially-typed command as the user would see it in the
    /// bottom-right corner of the Vim status line. Examples:
    /// - `"3d"` — count 3, operator delete, awaiting motion
    /// - `"\"ad"` — register a, operator delete, awaiting motion
    /// - `"ci"` — change operator, awaiting text object after `i`
    /// - `"df"` — delete operator, awaiting find-forward target char
    /// - `"^W"` — awaiting window sub-command
    #[must_use]
    pub fn pending_display(&self) -> CompactString {
        let mut buf = CompactString::new("");

        match self {
            Self::Ready { count, register } => {
                write_register(&mut buf, *register);
                write_count(&mut buf, *count);
            }

            Self::AwaitingRegister { count, phase } => {
                write_count(&mut buf, *count);
                match phase {
                    RegisterPhase::BeforeOperator => {
                        buf.push('"');
                    }
                    RegisterPhase::AfterOperator { operator, count2 } => {
                        buf.push_str(operator.key_notation());
                        write_count(&mut buf, *count2);
                        buf.push('"');
                    }
                }
            }

            Self::Operator {
                count,
                register,
                operator,
                count2,
                ..
            } => {
                write_register(&mut buf, *register);
                write_count(&mut buf, *count);
                buf.push_str(operator.key_notation());
                write_count(&mut buf, *count2);
            }

            Self::AwaitingChar {
                count,
                register,
                operator,
                char_command,
            } => {
                write_register(&mut buf, *register);
                write_count(&mut buf, *count);
                write_operator(&mut buf, operator.as_ref());
                buf.push(char_command.key_char());
            }

            Self::AwaitingTextObject {
                count,
                register,
                operator,
                scope,
            } => {
                write_register(&mut buf, *register);
                write_count(&mut buf, *count);
                buf.push_str(operator.key_notation());
                buf.push(scope.key_char());
            }

            Self::AwaitingTextObjectWithModifier {
                count,
                register,
                operator,
                scope,
                seek,
            } => {
                write_register(&mut buf, *register);
                write_count(&mut buf, *count);
                buf.push_str(operator.key_notation());
                buf.push(scope.key_char());
                buf.push(seek.key_char());
            }

            Self::AwaitingVisualTextObjectWithModifier {
                count,
                scope,
                register,
                seek,
            } => {
                write_register(&mut buf, *register);
                write_count(&mut buf, *count);
                buf.push(scope.key_char());
                buf.push(seek.key_char());
            }

            Self::AwaitingPrefix {
                count,
                register,
                prefix,
                operator,
                ..
            } => {
                write_register(&mut buf, *register);
                write_count(&mut buf, *count);
                write_operator(&mut buf, operator.as_ref());
                buf.push(*prefix);
            }

            Self::AwaitingMark {
                count,
                mark_type,
                operator,
                register,
            } => {
                write_register(&mut buf, *register);
                write_count(&mut buf, *count);
                write_operator(&mut buf, operator.as_ref());
                buf.push(mark_type.key_char());
            }

            Self::AwaitingMacroRegister { count, kind } => {
                write_count(&mut buf, *count);
                match kind {
                    MacroAwaitKind::Record => buf.push('q'),
                    MacroAwaitKind::Play => buf.push('@'),
                }
            }

            Self::AwaitingWindowCommand { count, register } => {
                write_register(&mut buf, *register);
                write_count(&mut buf, *count);
                buf.push_str("^W");
            }

            Self::AwaitingVisualTextObject {
                count,
                scope,
                register,
            } => {
                write_register(&mut buf, *register);
                write_count(&mut buf, *count);
                buf.push(scope.key_char());
            }

            Self::AwaitingSneakChar1 {
                count,
                register,
                operator,
                forward,
            } => {
                write_register(&mut buf, *register);
                write_count(&mut buf, *count);
                write_operator(&mut buf, operator.as_ref());
                buf.push(if *forward { 's' } else { 'S' });
            }

            Self::AwaitingSneakChar2 {
                count,
                register,
                operator,
                forward,
                c1,
            } => {
                write_register(&mut buf, *register);
                write_count(&mut buf, *count);
                write_operator(&mut buf, operator.as_ref());
                buf.push(if *forward { 's' } else { 'S' });
                buf.push(*c1);
            }

            Self::AwaitingSurroundChar { count, .. } => {
                write_count(&mut buf, *count);
                buf.push_str("ys");
            }

            Self::AwaitingSurroundDeleteChar => {
                buf.push_str("ds");
            }

            Self::AwaitingSurroundOldChar => {
                buf.push_str("cs");
            }

            Self::AwaitingSurroundNewChar { old_char } => {
                buf.push_str("cs");
                buf.push(*old_char);
            }

            Self::AwaitingComposingChars {
                count,
                register,
                operator,
                char_command,
                grapheme,
            } => {
                write_register(&mut buf, *register);
                write_count(&mut buf, *count);
                write_operator(&mut buf, operator.as_ref());
                buf.push(char_command.key_char());
                buf.push_str(grapheme);
            }

            Self::AwaitingCtrlBackslashN => {
                buf.push_str("^\\");
            }

            Self::AwaitingInsertRegister => buf.push_str("^R"),
            Self::AwaitingInsertExpression { collected } => {
                buf.push_str("^R=");
                buf.push_str(collected);
            }
            Self::AwaitingInsertCtrlG => buf.push_str("^G"),
            Self::AwaitingInsertDigraph1 => buf.push_str("^K"),
            Self::AwaitingInsertDigraph2 { c1 } => {
                buf.push_str("^K");
                buf.push(*c1);
            }
            Self::AwaitingVisualZPrefix { .. } => buf.push('z'),
            Self::AwaitingInsertCtrlX => buf.push_str("^X"),
            Self::InsertLiteral(lit) => {
                buf.push_str("^V");
                match lit {
                    InsertLiteralState::AwaitingFirst => {}
                    InsertLiteralState::Decimal(digits) => {
                        for &d in digits {
                            buf.push((d + b'0') as char);
                        }
                    }
                    InsertLiteralState::Octal(digits) => {
                        buf.push('o');
                        for &d in digits {
                            buf.push((d + b'0') as char);
                        }
                    }
                    InsertLiteralState::Hex(digits) => {
                        buf.push('x');
                        for &d in digits {
                            buf.push(hex_digit_char(d));
                        }
                    }
                    InsertLiteralState::UnicodeBmp(digits) => {
                        buf.push('u');
                        for &d in digits {
                            buf.push(hex_digit_char(d));
                        }
                    }
                    InsertLiteralState::UnicodeFull(digits) => {
                        buf.push('U');
                        for &d in digits {
                            buf.push(hex_digit_char(d));
                        }
                    }
                }
            }
        }

        buf
    }

    /// Create a Ready state with no count or register.
    #[must_use]
    pub fn ready() -> Self {
        Self::default()
    }

    /// Create a Ready state with a count.
    #[must_use]
    pub const fn with_count(count: u32) -> Self {
        Self::Ready {
            count: Some(count),
            register: None,
        }
    }

    /// Check if this is the Ready state.
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }

    /// Whether this state expects the next key to be a literal character
    /// (as opposed to a command/motion/action key).
    ///
    /// Used by the engine's layout normalization: command-key states normalize
    /// non-Latin keys to their Latin equivalents, literal-char states preserve
    /// the original character (for f/t/r targets, surround chars, sneak chars).
    ///
    /// Uses positive-match on literal states so that new variants default to
    /// `false` (= command key = normalize), which is the safe default.
    #[must_use]
    pub const fn expects_literal_char(&self) -> bool {
        matches!(
            self,
            Self::AwaitingChar { .. }
                | Self::AwaitingComposingChars { .. }
                | Self::AwaitingSneakChar1 { .. }
                | Self::AwaitingSneakChar2 { .. }
                | Self::AwaitingSurroundChar { .. }
                | Self::AwaitingSurroundDeleteChar
                | Self::AwaitingSurroundOldChar
                | Self::AwaitingSurroundNewChar { .. }
        )
    }

    /// Whether the parser is currently accumulating a count (digits 0-9).
    ///
    /// Returns `true` when the user has already typed at least one digit
    /// as part of a count prefix. This is used to suppress mapping lookup
    /// for `0` when it appears mid-count (e.g., `10j` should not expand
    /// a `0 → ^` mapping after the `1`).
    ///
    /// `0` at the *start* of input (no count yet) is a motion, not a count
    /// digit, so this correctly returns `false` for `Ready { count: None }`.
    #[must_use]
    pub const fn is_accumulating_count(&self) -> bool {
        match self {
            Self::Ready { count: Some(_), .. } => true,
            Self::Operator {
                count2: Some(_), ..
            } => true,
            _ => false,
        }
    }

    /// Check if we're in an operator-pending state.
    #[must_use]
    pub const fn is_operator_pending(&self) -> bool {
        matches!(
            self,
            Self::Operator { .. }
                | Self::AwaitingChar {
                    operator: Some(_),
                    ..
                }
                | Self::AwaitingComposingChars {
                    operator: Some(_),
                    ..
                }
                | Self::AwaitingTextObject { .. }
                | Self::AwaitingTextObjectWithModifier { .. }
                | Self::AwaitingPrefix {
                    operator: Some(_),
                    ..
                }
                | Self::AwaitingSneakChar1 {
                    operator: Some(_),
                    ..
                }
                | Self::AwaitingSneakChar2 {
                    operator: Some(_),
                    ..
                }
        )
    }

    /// Get the pending operator if in an operator-pending state.
    ///
    /// Returns `Some(operator)` when awaiting a motion/text-object after
    /// an operator key (d, c, y, etc.), `None` otherwise.
    #[must_use]
    pub const fn pending_operator(&self) -> Option<Operator> {
        match self {
            Self::Operator { operator, .. }
            | Self::AwaitingTextObject { operator, .. }
            | Self::AwaitingTextObjectWithModifier { operator, .. } => Some(*operator),
            Self::AwaitingChar {
                operator: Some(op), ..
            }
            | Self::AwaitingComposingChars {
                operator: Some(op), ..
            }
            | Self::AwaitingPrefix {
                operator: Some(op), ..
            }
            | Self::AwaitingSneakChar1 {
                operator: Some(op), ..
            }
            | Self::AwaitingSneakChar2 {
                operator: Some(op), ..
            } => Some(*op),
            _ => None,
        }
    }

    /// Get the current count if any.
    #[must_use]
    pub const fn count(&self) -> Option<u32> {
        match self {
            Self::Ready { count, .. } => *count,
            Self::AwaitingRegister { count, .. } => *count,
            Self::Operator { count, .. } => *count,
            Self::AwaitingChar { count, .. } => *count,
            Self::AwaitingComposingChars { count, .. } => *count,
            Self::AwaitingTextObject { count, .. } => *count,
            Self::AwaitingTextObjectWithModifier { count, .. } => *count,
            Self::AwaitingPrefix { count, .. } => *count,
            Self::AwaitingMark { count, .. } => *count,
            Self::AwaitingInsertRegister
            | Self::AwaitingInsertExpression { .. }
            | Self::AwaitingInsertCtrlG
            | Self::AwaitingInsertDigraph1
            | Self::AwaitingInsertDigraph2 { .. }
            | Self::AwaitingInsertCtrlX
            | Self::InsertLiteral(_) => None,
            Self::AwaitingMacroRegister { count, .. } => *count,
            Self::AwaitingVisualTextObject { count, .. } => *count,
            Self::AwaitingVisualTextObjectWithModifier { count, .. } => *count,
            Self::AwaitingWindowCommand { count, .. } => *count,
            Self::AwaitingSneakChar1 { count, .. } => *count,
            Self::AwaitingSneakChar2 { count, .. } => *count,
            Self::AwaitingSurroundChar { count, .. } => *count,
            Self::AwaitingSurroundDeleteChar
            | Self::AwaitingSurroundOldChar
            | Self::AwaitingSurroundNewChar { .. } => None,
            Self::AwaitingVisualZPrefix { .. } => None,
            Self::AwaitingCtrlBackslashN => None,
        }
    }

    /// Get the current register if any.
    #[must_use]
    pub const fn register(&self) -> Option<RegisterName> {
        match self {
            Self::Ready { register, .. } => *register,
            Self::AwaitingRegister { .. } => None,
            Self::Operator { register, .. } => *register,
            Self::AwaitingChar { register, .. } => *register,
            Self::AwaitingComposingChars { register, .. } => *register,
            Self::AwaitingTextObject { register, .. } => *register,
            Self::AwaitingTextObjectWithModifier { register, .. } => *register,
            Self::AwaitingPrefix { register, .. } => *register,
            Self::AwaitingMark { register, .. } => *register,
            Self::AwaitingInsertRegister
            | Self::AwaitingInsertExpression { .. }
            | Self::AwaitingInsertCtrlG
            | Self::AwaitingInsertDigraph1
            | Self::AwaitingInsertDigraph2 { .. }
            | Self::AwaitingInsertCtrlX
            | Self::InsertLiteral(_) => None,
            Self::AwaitingMacroRegister { .. } => None,
            Self::AwaitingVisualTextObject { register, .. } => *register,
            Self::AwaitingVisualTextObjectWithModifier { register, .. } => *register,
            Self::AwaitingWindowCommand { register, .. } => *register,
            Self::AwaitingSneakChar1 { register, .. } => *register,
            Self::AwaitingSneakChar2 { register, .. } => *register,
            Self::AwaitingSurroundChar { .. }
            | Self::AwaitingSurroundDeleteChar
            | Self::AwaitingSurroundOldChar
            | Self::AwaitingSurroundNewChar { .. } => None,
            Self::AwaitingVisualZPrefix { register } => *register,
            Self::AwaitingCtrlBackslashN => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a RegisterName for tests.
    fn reg(c: char) -> RegisterName {
        RegisterName::new(c).unwrap()
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Ready
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn ready_empty() {
        let s = InputState::Ready {
            count: None,
            register: None,
        };
        assert_eq!(s.pending_display().as_str(), "");
    }

    #[test]
    fn ready_with_count() {
        let s = InputState::Ready {
            count: Some(3),
            register: None,
        };
        assert_eq!(s.pending_display().as_str(), "3");
    }

    #[test]
    fn ready_with_register() {
        let s = InputState::Ready {
            count: None,
            register: Some(reg('a')),
        };
        assert_eq!(s.pending_display().as_str(), "\"a");
    }

    #[test]
    fn ready_with_register_and_count() {
        let s = InputState::Ready {
            count: Some(3),
            register: Some(reg('a')),
        };
        assert_eq!(s.pending_display().as_str(), "\"a3");
    }

    #[test]
    fn ready_large_count() {
        let s = InputState::Ready {
            count: Some(999),
            register: None,
        };
        assert_eq!(s.pending_display().as_str(), "999");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // AwaitingRegister
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn awaiting_register_before_operator() {
        let s = InputState::AwaitingRegister {
            count: None,
            phase: RegisterPhase::BeforeOperator,
        };
        assert_eq!(s.pending_display().as_str(), "\"");
    }

    #[test]
    fn awaiting_register_before_operator_with_count() {
        let s = InputState::AwaitingRegister {
            count: Some(3),
            phase: RegisterPhase::BeforeOperator,
        };
        assert_eq!(s.pending_display().as_str(), "3\"");
    }

    #[test]
    fn awaiting_register_after_operator() {
        let s = InputState::AwaitingRegister {
            count: None,
            phase: RegisterPhase::AfterOperator {
                operator: Operator::Delete,
                count2: None,
            },
        };
        assert_eq!(s.pending_display().as_str(), "d\"");
    }

    #[test]
    fn awaiting_register_after_operator_with_counts() {
        let s = InputState::AwaitingRegister {
            count: Some(2),
            phase: RegisterPhase::AfterOperator {
                operator: Operator::Delete,
                count2: Some(3),
            },
        };
        assert_eq!(s.pending_display().as_str(), "2d3\"");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Operator
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn operator_delete() {
        let s = InputState::Operator {
            count: None,
            register: None,
            operator: Operator::Delete,
            count2: None,
            force_type: None,
        };
        assert_eq!(s.pending_display().as_str(), "d");
    }

    #[test]
    fn operator_delete_with_count() {
        let s = InputState::Operator {
            count: Some(3),
            register: None,
            operator: Operator::Delete,
            count2: None,
            force_type: None,
        };
        assert_eq!(s.pending_display().as_str(), "3d");
    }

    #[test]
    fn operator_delete_with_both_counts() {
        let s = InputState::Operator {
            count: Some(3),
            register: None,
            operator: Operator::Delete,
            count2: Some(2),
            force_type: None,
        };
        assert_eq!(s.pending_display().as_str(), "3d2");
    }

    #[test]
    fn operator_delete_with_register() {
        let s = InputState::Operator {
            count: None,
            register: Some(reg('a')),
            operator: Operator::Delete,
            count2: None,
            force_type: None,
        };
        assert_eq!(s.pending_display().as_str(), "\"ad");
    }

    #[test]
    fn operator_delete_full_prefix() {
        let s = InputState::Operator {
            count: Some(2),
            register: Some(reg('a')),
            operator: Operator::Delete,
            count2: Some(3),
            force_type: None,
        };
        assert_eq!(s.pending_display().as_str(), "\"a2d3");
    }

    #[test]
    fn operator_yank() {
        let s = InputState::Operator {
            count: None,
            register: None,
            operator: Operator::Yank,
            count2: None,
            force_type: None,
        };
        assert_eq!(s.pending_display().as_str(), "y");
    }

    #[test]
    fn operator_toggle_case() {
        let s = InputState::Operator {
            count: None,
            register: None,
            operator: Operator::ToggleCase,
            count2: None,
            force_type: None,
        };
        assert_eq!(s.pending_display().as_str(), "g~");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // AwaitingChar
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn awaiting_char_find_forward_with_operator() {
        let s = InputState::AwaitingChar {
            count: None,
            register: None,
            operator: Some(Operator::Delete),
            char_command: CharCommand::FindForward,
        };
        assert_eq!(s.pending_display().as_str(), "df");
    }

    #[test]
    fn awaiting_char_replace_no_operator() {
        let s = InputState::AwaitingChar {
            count: None,
            register: None,
            operator: None,
            char_command: CharCommand::Replace,
        };
        assert_eq!(s.pending_display().as_str(), "r");
    }

    #[test]
    fn awaiting_char_find_backward() {
        let s = InputState::AwaitingChar {
            count: None,
            register: None,
            operator: None,
            char_command: CharCommand::FindBackward,
        };
        assert_eq!(s.pending_display().as_str(), "F");
    }

    #[test]
    fn awaiting_char_till_forward() {
        let s = InputState::AwaitingChar {
            count: None,
            register: None,
            operator: None,
            char_command: CharCommand::TillForward,
        };
        assert_eq!(s.pending_display().as_str(), "t");
    }

    #[test]
    fn awaiting_char_till_backward() {
        let s = InputState::AwaitingChar {
            count: None,
            register: None,
            operator: None,
            char_command: CharCommand::TillBackward,
        };
        assert_eq!(s.pending_display().as_str(), "T");
    }

    #[test]
    fn awaiting_char_with_count_and_register() {
        let s = InputState::AwaitingChar {
            count: Some(2),
            register: Some(reg('b')),
            operator: Some(Operator::Yank),
            char_command: CharCommand::FindForward,
        };
        assert_eq!(s.pending_display().as_str(), "\"b2yf");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // AwaitingTextObject
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn awaiting_textobject_change_inner() {
        let s = InputState::AwaitingTextObject {
            count: None,
            register: None,
            operator: Operator::Change,
            scope: TextObjectScope::Inner,
        };
        assert_eq!(s.pending_display().as_str(), "ci");
    }

    #[test]
    fn awaiting_textobject_delete_around() {
        let s = InputState::AwaitingTextObject {
            count: None,
            register: None,
            operator: Operator::Delete,
            scope: TextObjectScope::Around,
        };
        assert_eq!(s.pending_display().as_str(), "da");
    }

    #[test]
    fn awaiting_textobject_with_count() {
        let s = InputState::AwaitingTextObject {
            count: Some(2),
            register: None,
            operator: Operator::Yank,
            scope: TextObjectScope::Inner,
        };
        assert_eq!(s.pending_display().as_str(), "2yi");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // AwaitingPrefix
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn awaiting_prefix_g_no_operator() {
        let s = InputState::AwaitingPrefix {
            count: None,
            register: None,
            prefix: 'g',
            operator: None,
            force_type: None,
        };
        assert_eq!(s.pending_display().as_str(), "g");
    }

    #[test]
    fn awaiting_prefix_g_with_operator() {
        let s = InputState::AwaitingPrefix {
            count: None,
            register: None,
            prefix: 'g',
            operator: Some(Operator::Delete),
            force_type: None,
        };
        assert_eq!(s.pending_display().as_str(), "dg");
    }

    #[test]
    fn awaiting_prefix_z() {
        let s = InputState::AwaitingPrefix {
            count: None,
            register: None,
            prefix: 'z',
            operator: None,
            force_type: None,
        };
        assert_eq!(s.pending_display().as_str(), "z");
    }

    #[test]
    fn awaiting_prefix_bracket() {
        let s = InputState::AwaitingPrefix {
            count: None,
            register: None,
            prefix: '[',
            operator: None,
            force_type: None,
        };
        assert_eq!(s.pending_display().as_str(), "[");
    }

    #[test]
    fn awaiting_prefix_with_count() {
        let s = InputState::AwaitingPrefix {
            count: Some(5),
            register: None,
            prefix: 'g',
            operator: None,
            force_type: None,
        };
        assert_eq!(s.pending_display().as_str(), "5g");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // AwaitingMark
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn awaiting_mark_set() {
        let s = InputState::AwaitingMark {
            count: None,
            mark_type: MarkType::Set,
            operator: None,
            register: None,
        };
        assert_eq!(s.pending_display().as_str(), "m");
    }

    #[test]
    fn awaiting_mark_yank_jump_line() {
        let s = InputState::AwaitingMark {
            count: None,
            mark_type: MarkType::JumpLine,
            operator: Some(Operator::Yank),
            register: None,
        };
        assert_eq!(s.pending_display().as_str(), "y'");
    }

    #[test]
    fn awaiting_mark_jump_exact() {
        let s = InputState::AwaitingMark {
            count: None,
            mark_type: MarkType::JumpExact,
            operator: None,
            register: None,
        };
        assert_eq!(s.pending_display().as_str(), "`");
    }

    #[test]
    fn awaiting_mark_with_operator_and_count() {
        let s = InputState::AwaitingMark {
            count: Some(2),
            mark_type: MarkType::JumpLine,
            operator: Some(Operator::Delete),
            register: Some(reg('a')),
        };
        assert_eq!(s.pending_display().as_str(), "\"a2d'");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // AwaitingMacroRegister
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn awaiting_macro_record() {
        let s = InputState::AwaitingMacroRegister {
            count: None,
            kind: MacroAwaitKind::Record,
        };
        assert_eq!(s.pending_display().as_str(), "q");
    }

    #[test]
    fn awaiting_macro_play() {
        let s = InputState::AwaitingMacroRegister {
            count: None,
            kind: MacroAwaitKind::Play,
        };
        assert_eq!(s.pending_display().as_str(), "@");
    }

    #[test]
    fn awaiting_macro_play_with_count() {
        let s = InputState::AwaitingMacroRegister {
            count: Some(3),
            kind: MacroAwaitKind::Play,
        };
        assert_eq!(s.pending_display().as_str(), "3@");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // AwaitingWindowCommand
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn awaiting_window_command() {
        let s = InputState::AwaitingWindowCommand {
            count: None,
            register: None,
        };
        assert_eq!(s.pending_display().as_str(), "^W");
    }

    #[test]
    fn awaiting_window_command_with_count() {
        let s = InputState::AwaitingWindowCommand {
            count: Some(3),
            register: None,
        };
        assert_eq!(s.pending_display().as_str(), "3^W");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // AwaitingVisualTextObject
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn awaiting_visual_textobject_inner() {
        let s = InputState::AwaitingVisualTextObject {
            count: None,
            scope: TextObjectScope::Inner,
            register: None,
        };
        assert_eq!(s.pending_display().as_str(), "i");
    }

    #[test]
    fn awaiting_visual_textobject_around() {
        let s = InputState::AwaitingVisualTextObject {
            count: None,
            scope: TextObjectScope::Around,
            register: None,
        };
        assert_eq!(s.pending_display().as_str(), "a");
    }

    #[test]
    fn awaiting_visual_textobject_with_count() {
        let s = InputState::AwaitingVisualTextObject {
            count: Some(2),
            scope: TextObjectScope::Inner,
            register: None,
        };
        assert_eq!(s.pending_display().as_str(), "2i");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn insert_register() {
        assert_eq!(
            InputState::AwaitingInsertRegister
                .pending_display()
                .as_str(),
            "^R"
        );
    }

    #[test]
    fn insert_ctrl_g() {
        assert_eq!(
            InputState::AwaitingInsertCtrlG.pending_display().as_str(),
            "^G"
        );
    }

    #[test]
    fn insert_digraph1() {
        assert_eq!(
            InputState::AwaitingInsertDigraph1
                .pending_display()
                .as_str(),
            "^K"
        );
    }

    #[test]
    fn insert_digraph2() {
        let s = InputState::AwaitingInsertDigraph2 { c1: 'e' };
        assert_eq!(s.pending_display().as_str(), "^Ke");
    }

    #[test]
    fn insert_digraph2_special_char() {
        let s = InputState::AwaitingInsertDigraph2 { c1: '!' };
        assert_eq!(s.pending_display().as_str(), "^K!");
    }

    #[test]
    fn insert_ctrl_x() {
        assert_eq!(
            InputState::AwaitingInsertCtrlX.pending_display().as_str(),
            "^X"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Combined / edge case tests
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn operator_with_force_type_ignored_in_display() {
        // force_type does not affect the display string
        let s = InputState::Operator {
            count: None,
            register: None,
            operator: Operator::Delete,
            count2: None,
            force_type: Some(MotionType::LineWise),
        };
        assert_eq!(s.pending_display().as_str(), "d");
    }

    #[test]
    fn operator_indent_with_count() {
        let s = InputState::Operator {
            count: Some(2),
            register: None,
            operator: Operator::Indent,
            count2: None,
            force_type: None,
        };
        assert_eq!(s.pending_display().as_str(), "2>");
    }

    #[test]
    fn default_input_state_is_empty_display() {
        let s = InputState::default();
        assert_eq!(s.pending_display().as_str(), "");
    }

    #[test]
    fn awaiting_textobject_uppercase_operator() {
        let s = InputState::AwaitingTextObject {
            count: None,
            register: None,
            operator: Operator::Uppercase,
            scope: TextObjectScope::Inner,
        };
        assert_eq!(s.pending_display().as_str(), "gUi");
    }

    #[test]
    fn awaiting_mark_register_displayed() {
        let s = InputState::AwaitingMark {
            count: None,
            mark_type: MarkType::JumpExact,
            operator: None,
            register: Some(reg('z')),
        };
        assert_eq!(s.pending_display().as_str(), "\"z`");
    }
}
