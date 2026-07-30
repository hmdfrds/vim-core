//! Prefix state handler.
//!
//! Handles key processing after prefix keys like `g`, `z`, `[`, `]`.

use crate::grammar::types::Operator;
use crate::keymap::KeyEvent;
use crate::primitives::{Mode, MotionType, RegisterName};

use crate::grammar::command::{count_or_default, Command, PrefixCommand, VisualKind};
use crate::grammar::input_state::InputState;
use crate::grammar::parser::Parser;
use crate::grammar::result::GrammarResult;
use crate::grammar::types::Action;
use crate::grammar::types::Motion;
use crate::primitives::VisualType;

impl Parser {
    /// Handle `AwaitingPrefix` state.
    ///
    /// After g, z, [, ] - we need the continuation key.
    pub(crate) fn handle_awaiting_prefix(
        &self,
        count: Option<u32>,
        register: Option<RegisterName>,
        prefix: char,
        operator: Option<Operator>,
        force_type: Option<MotionType>,
        key: KeyEvent,
    ) -> GrammarResult {
        // Handle arrow keys in g-prefix (g<Up> = gk, g<Down> = gj)
        if prefix == 'g' {
            if let Some(motion) = Self::g_prefix_from_key(&key) {
                return Self::motion_or_op_motion(count, register, operator, force_type, motion);
            }
            // g Ctrl-A / g Ctrl-X — sequential increment/decrement
            if let Some(cmd) = Self::g_prefix_command_from_key(&key) {
                return GrammarResult::Execute(Command::Prefix {
                    count: count_or_default(count),
                    register,
                    command: cmd,
                });
            }
            // g Ctrl-H — enter Select mode (blockwise)
            if let Some(cmd) = Self::g_prefix_select_from_key(&key) {
                return GrammarResult::Execute(cmd);
            }
        }

        // z<Enter> → first non-blank + scroll to top
        // Must be checked before as_char() since Key::Enter isn't a char
        if prefix == 'z' && key.key == crate::keymap::Key::Enter {
            return GrammarResult::Execute(Command::Prefix {
                count: count_or_default(count),
                register,
                command: PrefixCommand::FirstNonBlankTop,
            });
        }

        let Some(c) = key.as_char() else {
            return GrammarResult::Invalid;
        };

        // Handle 'g' prefix specially
        if prefix == 'g' {
            return self.handle_g_prefix(count, register, operator, force_type, c);
        }

        // Handle '[' and ']' prefix bracket motions and actions
        if prefix == '[' || prefix == ']' {
            if let Some(motion) = Self::bracket_motion_from_prefix(prefix, c) {
                return Self::motion_or_op_motion(count, register, operator, force_type, motion);
            }
            // ]p/[p — put with indent adjustment (not a motion)
            if c == 'p' || c == 'P' {
                let action = if prefix == ']' {
                    Action::PutIndentAfter
                } else {
                    Action::PutIndentBefore
                };
                return GrammarResult::Execute(Command::Action {
                    count: count_or_default(count),
                    register,
                    action,
                });
            }
            // vim-unimpaired bracket commands
            if let Some(cmd) = Self::unimpaired_bracket_command(prefix, c) {
                return GrammarResult::Execute(Command::Prefix {
                    count: count_or_default(count),
                    register,
                    command: cmd,
                });
            }
            // Fallback: text object seeking — ]x/[x navigates to next/previous
            // instance of text object kind x. Only fires for keys not already
            // handled by specific bracket motions above.
            if let Some(kind) = crate::grammar::types::TextObjectKind::from_char(c) {
                let direction = if prefix == ']' {
                    crate::primitives::Direction::Forward
                } else {
                    crate::primitives::Direction::Backward
                };
                let motion = Motion::SeekTextObject { kind, direction };
                return Self::motion_or_op_motion(count, register, operator, force_type, motion);
            }
        }

        // Handle 'Z' prefix commands
        if prefix == 'Z' {
            if let Some(cmd) = Self::z_upper_prefix_command(c) {
                return GrammarResult::Execute(Command::Prefix {
                    count: count_or_default(count),
                    register,
                    command: cmd,
                });
            }
        }

        // Handle 'z' prefix commands
        if prefix == 'z' {
            if let Some(cmd) = Self::z_prefix_command(c) {
                return GrammarResult::Execute(Command::Prefix {
                    count: count_or_default(count),
                    register,
                    command: cmd,
                });
            }
        }

        // Unrecognized prefix combination
        GrammarResult::Invalid
    }

    /// Map non-char keys to g-prefix motions (arrow keys).
    const fn g_prefix_from_key(key: &KeyEvent) -> Option<Motion> {
        use crate::keymap::Key;
        match key.key {
            Key::Up => Some(Motion::DisplayUp),
            Key::Down => Some(Motion::DisplayDown),
            // drift: g-prefix only maps Up/Down arrows to display-line motions; all other named keys are unrecognised in this sub-dispatch
            _ => None,
        }
    }

    /// Map non-char keys to g-prefix PrefixCommands (ctrl combos).
    ///
    /// Returns `Some(PrefixCommand)` for commands that use the Prefix wrapper,
    /// or `None` to fall through to other g-prefix handling (including
    /// `g_prefix_select_from_key` for Ctrl-H).
    const fn g_prefix_command_from_key(key: &KeyEvent) -> Option<PrefixCommand> {
        use crate::keymap::{Key, Modifiers};
        if !key.modifiers.contains(Modifiers::CTRL) {
            return None;
        }
        match key.key {
            Key::Char('a' | 'A') => Some(PrefixCommand::SequentialIncrement),
            Key::Char('x' | 'X') => Some(PrefixCommand::SequentialDecrement),
            // drift: only Ctrl-A and Ctrl-X are recognised g-prefix commands; other Ctrl+key combos yield None
            _ => None,
        }
    }

    /// Map `g Ctrl-H` to blockwise select entry.
    ///
    /// Returns `Some(Command)` for `g<C-H>`, `None` otherwise.
    /// Separated from `g_prefix_command_from_key` because SelectEnter is not
    /// a PrefixCommand — it's a top-level Command variant.
    const fn g_prefix_select_from_key(key: &KeyEvent) -> Option<Command> {
        use crate::keymap::{Key, Modifiers};
        if key.modifiers.contains(Modifiers::CTRL) {
            if let Key::Char('h' | 'H') = key.key {
                return Some(Command::SelectEnter {
                    visual_type: VisualType::Block,
                });
            }
        }
        None
    }

    /// Handle 'g' prefix commands.
    fn handle_g_prefix(
        &self,
        count: Option<u32>,
        register: Option<RegisterName>,
        operator: Option<Operator>,
        force_type: Option<MotionType>,
        c: char,
    ) -> GrammarResult {
        // Try motion-only g-prefix keys first.
        if let Some(motion) = Self::g_prefix_motion(c) {
            return Self::motion_or_op_motion(count, register, operator, force_type, motion);
        }
        match c {
            // gv — reselect last visual selection.
            // If an operator is pending (e.g., `ygv`), gv is not a valid motion
            // so the operator is cancelled and gv is consumed (Vim behavior).
            'v' if operator.is_some() => GrammarResult::Cancel,
            'v' => GrammarResult::Execute(Command::Visual(VisualKind::Reselect)),
            'u' | 'U' | '~' | 'q' | 'w' | '@' | '?' => {
                let op = match c {
                    'u' => Operator::Lowercase,
                    'U' => Operator::Uppercase,
                    '~' => Operator::ToggleCase,
                    'w' => Operator::FormatKeepCursor,
                    '@' => Operator::CallOperatorFunc,
                    '?' => Operator::Rot13,
                    _ => Operator::Format,
                };
                // If the same g-prefix operator is already pending, this is
                // a doubled-operator = line operation (e.g., g?g? = rot13 line).
                if operator == Some(op) {
                    GrammarResult::Execute(Command::OperatorLine {
                        count: count_or_default(count),
                        register,
                        operator: op,
                    })
                } else {
                    GrammarResult::Continue(InputState::Operator {
                        count,
                        register,
                        operator: op,
                        count2: None,
                        force_type: None,
                    })
                }
            }
            'i' => GrammarResult::Execute(Command::Prefix {
                count: count_or_default(count),
                register,
                command: PrefixCommand::GotoInsertStop,
            }),
            // gI — insert at column 0 of current line
            'I' => GrammarResult::Execute(Command::InsertEntry {
                count: count_or_default(count),
                register,
                entry_type: crate::primitives::InsertEntryType::Column0,
            }),
            'J' => GrammarResult::Execute(Command::Prefix {
                count: count_or_default(count),
                register,
                command: PrefixCommand::JoinNoSpace,
            }),
            // gp/gP — put with cursor after pasted text
            'p' => GrammarResult::Execute(Command::Action {
                count: count_or_default(count),
                register,
                action: Action::PutAfterCursorAfter,
            }),
            'P' => GrammarResult::Execute(Command::Action {
                count: count_or_default(count),
                register,
                action: Action::PutBeforeCursorAfter,
            }),
            // gR — virtual replace mode
            'R' => GrammarResult::ModeChange(Mode::VirtualReplace, count),
            // ga — show ASCII value of char under cursor
            'a' => GrammarResult::Execute(Command::Prefix {
                count: count_or_default(count),
                register,
                command: PrefixCommand::ShowAscii,
            }),
            // g8 — show UTF-8 bytes of char under cursor
            '8' => GrammarResult::Execute(Command::Prefix {
                count: count_or_default(count),
                register,
                command: PrefixCommand::ShowUtf8,
            }),
            // gd — go to definition
            'd' => GrammarResult::Execute(Command::Prefix {
                count: count_or_default(count),
                register,
                command: PrefixCommand::GotoDefinition,
            }),
            // gh — enter Select mode (charwise)
            'h' => GrammarResult::Execute(Command::SelectEnter {
                visual_type: VisualType::Char,
            }),
            // gH — enter Select mode (linewise)
            'H' => GrammarResult::Execute(Command::SelectEnter {
                visual_type: VisualType::Line,
            }),
            // g[ — expand selection to parent syntax node
            '[' => GrammarResult::Execute(Command::Prefix {
                count: count_or_default(count),
                register,
                command: PrefixCommand::SelectParentNode,
            }),
            // g] — shrink selection to child syntax node (or pop history)
            ']' => GrammarResult::Execute(Command::Prefix {
                count: count_or_default(count),
                register,
                command: PrefixCommand::SelectChildNode,
            }),
            // g{ — select previous sibling syntax node
            '{' => GrammarResult::Execute(Command::Prefix {
                count: count_or_default(count),
                register,
                command: PrefixCommand::SelectPrevSibling,
            }),
            // g} — select next sibling syntax node
            '}' => GrammarResult::Execute(Command::Prefix {
                count: count_or_default(count),
                register,
                command: PrefixCommand::SelectNextSibling,
            }),
            // g( -- select all sibling syntax nodes
            '(' => GrammarResult::Execute(Command::Prefix {
                count: count_or_default(count),
                register,
                command: PrefixCommand::SelectAllSiblings,
            }),
            // g) -- select all child syntax nodes
            ')' => GrammarResult::Execute(Command::Prefix {
                count: count_or_default(count),
                register,
                command: PrefixCommand::SelectAllChildren,
            }),
            // g. — intent-aware repeat
            '.' => GrammarResult::Execute(Command::Action {
                count: count_or_default(count),
                register,
                action: Action::IntentRepeat,
            }),
            // g& — repeat last substitution globally on all lines
            '&' => GrammarResult::Execute(Command::Action {
                count: count_or_default(count),
                register,
                action: Action::RepeatSubstituteGlobal,
            }),
            // g- — navigate to earlier undo state (:earlier N)
            '-' => GrammarResult::Execute(Command::Prefix {
                count: count_or_default(count),
                register,
                command: PrefixCommand::UndoEarlier,
            }),
            // g+ — navigate to later undo state (:later N)
            '+' => GrammarResult::Execute(Command::Prefix {
                count: count_or_default(count),
                register,
                command: PrefixCommand::UndoLater,
            }),
            // gb — add cursor at next match
            'b' => GrammarResult::Execute(Command::Action {
                count: count_or_default(count),
                register,
                action: Action::AddNextMatchCursor,
            }),
            // gB — add cursor at previous match
            'B' => GrammarResult::Execute(Command::Action {
                count: count_or_default(count),
                register,
                action: Action::AddPrevMatchCursor,
            }),
            // gs — skip current match
            's' => GrammarResult::Execute(Command::Action {
                count: count_or_default(count),
                register,
                action: Action::SkipMatchCursor,
            }),
            _ => GrammarResult::Invalid,
        }
    }

    /// Map g-prefix char to a Motion (motion-only g-commands).
    const fn g_prefix_motion(c: char) -> Option<Motion> {
        match c {
            'g' => Some(Motion::GotoFirstLine),
            'j' => Some(Motion::DisplayDown),
            'k' => Some(Motion::DisplayUp),
            '_' => Some(Motion::LastNonBlank),
            'e' => Some(Motion::WordEndBackward),
            'E' => Some(Motion::WORDEndBackward),
            '0' => Some(Motion::ScreenLineStart),
            '$' => Some(Motion::ScreenLineEnd),
            '^' => Some(Motion::ScreenFirstNonBlank),
            'm' => Some(Motion::MiddleOfScreenLine),
            'M' => Some(Motion::MiddleOfTextLine),
            'o' => Some(Motion::GotoByte),
            ';' => Some(Motion::ChangelistOlder),
            ',' => Some(Motion::ChangelistNewer),
            'n' => Some(Motion::SearchObjectForward),
            'N' => Some(Motion::SearchObjectBackward),
            '*' => Some(Motion::PartialWordSearchForward),
            '#' => Some(Motion::PartialWordSearchBackward),
            _ => None,
        }
    }

    /// Map z-prefix key to a typed `PrefixCommand`.
    const fn z_prefix_command(c: char) -> Option<PrefixCommand> {
        match c {
            'z' => Some(PrefixCommand::ScrollCenter),
            't' => Some(PrefixCommand::ScrollTop),
            'b' => Some(PrefixCommand::ScrollBottom),
            '\r' | '\n' => Some(PrefixCommand::FirstNonBlankTop),
            '.' => Some(PrefixCommand::FirstNonBlankCenter),
            '-' => Some(PrefixCommand::FirstNonBlankBottom),
            // Horizontal scroll
            'h' => Some(PrefixCommand::ScrollColumnLeft),
            'l' => Some(PrefixCommand::ScrollColumnRight),
            'H' => Some(PrefixCommand::ScrollHalfScreenLeft),
            'L' => Some(PrefixCommand::ScrollHalfScreenRight),
            's' => Some(PrefixCommand::ScrollCursorToLeft),
            'e' => Some(PrefixCommand::ScrollCursorToRight),
            // Fold
            'o' => Some(PrefixCommand::FoldOpen),
            'O' => Some(PrefixCommand::FoldOpenRecursive),
            'c' => Some(PrefixCommand::FoldClose),
            'C' => Some(PrefixCommand::FoldCloseRecursive),
            'a' => Some(PrefixCommand::FoldToggle),
            'A' => Some(PrefixCommand::FoldToggleRecursive),
            'R' => Some(PrefixCommand::FoldOpenAll),
            'M' => Some(PrefixCommand::FoldCloseAll),
            'd' => Some(PrefixCommand::FoldDelete),
            'D' => Some(PrefixCommand::FoldDeleteRecursive),
            'E' => Some(PrefixCommand::FoldEliminateAll),
            'i' => Some(PrefixCommand::FoldToggleEnable),
            'n' => Some(PrefixCommand::FoldDisable),
            'N' => Some(PrefixCommand::FoldEnable),
            _ => None,
        }
    }

    /// Map Z-prefix key to a typed `PrefixCommand`.
    const fn z_upper_prefix_command(c: char) -> Option<PrefixCommand> {
        match c {
            'Z' => Some(PrefixCommand::WriteQuit),
            'Q' => Some(PrefixCommand::ForceQuit),
            _ => None,
        }
    }

    /// Map `[`/`]` prefix + key to bracket motion variants.
    const fn bracket_motion_from_prefix(prefix: char, key: char) -> Option<Motion> {
        match (prefix, key) {
            // Section motions ([[, ]], ][, [])
            ('[', '[') => Some(Motion::SectionBackwardStart),
            (']', ']') => Some(Motion::SectionForwardStart),
            (']', '[') => Some(Motion::SectionForwardEnd),
            ('[', ']') => Some(Motion::SectionBackwardEnd),
            ('[', '{') => Some(Motion::PrevUnmatchedBrace),
            (']', '}') => Some(Motion::NextUnmatchedBrace),
            ('[', '(') => Some(Motion::PrevUnmatchedParen),
            (']', ')') => Some(Motion::NextUnmatchedParen),
            // Method boundary motions
            ('[', 'm') => Some(Motion::PrevMethodStart),
            (']', 'm') => Some(Motion::NextMethodStart),
            ('[', 'M') => Some(Motion::PrevMethodEnd),
            (']', 'M') => Some(Motion::NextMethodEnd),
            // Comment navigation
            ('[', '/') => Some(Motion::PrevCommentStart),
            (']', '/') => Some(Motion::NextCommentEnd),
            // Bracket/quote pair navigation
            (']', 'b') => Some(Motion::NextBracketPair),
            ('[', 'b') => Some(Motion::PrevBracketPair),
            (']', 'q') => Some(Motion::NextQuotePair),
            ('[', 'q') => Some(Motion::PrevQuotePair),
            // Indent navigation
            ('[', 'i') => Some(Motion::PrevSameIndent),
            (']', 'i') => Some(Motion::NextSameIndent),
            ('[', '-') => Some(Motion::PrevLesserIndent),
            (']', '-') => Some(Motion::NextLesserIndent),
            ('[', '+') => Some(Motion::PrevGreaterIndent),
            (']', '+') => Some(Motion::NextGreaterIndent),
            // Mark navigation
            (']', '\'') => Some(Motion::NextMark),
            ('[', '\'') => Some(Motion::PreviousMark),
            _ => None,
        }
    }

    /// Map bracket prefix to vim-unimpaired commands.
    const fn unimpaired_bracket_command(prefix: char, key: char) -> Option<PrefixCommand> {
        match (prefix, key) {
            ('[', ' ') => Some(PrefixCommand::InsertBlankAbove),
            (']', ' ') => Some(PrefixCommand::InsertBlankBelow),
            _ => None,
        }
    }

    /// Handle `AwaitingWindowCommand` state (Ctrl-W sub-commands).
    pub(crate) const fn handle_window_command(
        count: Option<u32>,
        register: Option<RegisterName>,
        key: KeyEvent,
    ) -> GrammarResult {
        let Some(cmd) = Self::window_sub_command(&key) else {
            return GrammarResult::Invalid;
        };
        GrammarResult::Execute(Command::Prefix {
            count: count_or_default(count),
            register,
            command: cmd,
        })
    }

    /// Map Ctrl-W sub-command key to a `PrefixCommand`.
    const fn window_sub_command(key: &KeyEvent) -> Option<PrefixCommand> {
        // Accept both plain chars and Ctrl-modified versions
        // (e.g., Ctrl-W Ctrl-S is same as Ctrl-W s)
        let c = match key.key {
            crate::keymap::Key::Char(c) => c,
            // drift: Ctrl-W sub-commands are all character-based; named keys (arrows, F-keys) are not valid Ctrl-W sub-commands
            _ => return None,
        };
        match c {
            's' | 'S' => Some(PrefixCommand::WindowSplit),
            'v' => Some(PrefixCommand::WindowVSplit),
            'c' => Some(PrefixCommand::WindowClose),
            'o' => Some(PrefixCommand::WindowOnly),
            'w' => Some(PrefixCommand::WindowNext),
            'W' => Some(PrefixCommand::WindowPrev),
            'h' => Some(PrefixCommand::WindowMoveLeft),
            'l' => Some(PrefixCommand::WindowMoveRight),
            'k' => Some(PrefixCommand::WindowMoveUp),
            'j' => Some(PrefixCommand::WindowMoveDown),
            '=' => Some(PrefixCommand::WindowEqualSize),
            '+' => Some(PrefixCommand::WindowIncreaseHeight),
            '-' => Some(PrefixCommand::WindowDecreaseHeight),
            '>' => Some(PrefixCommand::WindowIncreaseWidth),
            '<' => Some(PrefixCommand::WindowDecreaseWidth),
            'r' => Some(PrefixCommand::WindowRotateDown),
            'R' => Some(PrefixCommand::WindowRotateUp),
            'n' => Some(PrefixCommand::WindowNew),
            'q' => Some(PrefixCommand::WindowClose), // alias
            _ => None,
        }
    }

    /// Helper: Create Motion or OperatorMotion command.
    #[inline]
    const fn motion_or_op_motion(
        count: Option<u32>,
        register: Option<RegisterName>,
        operator: Option<Operator>,
        force_type: Option<MotionType>,
        motion: Motion,
    ) -> GrammarResult {
        if let Some(op) = operator {
            GrammarResult::Execute(Command::OperatorMotion {
                count: count_or_default(count),
                register,
                operator: op,
                motion,
                force_type,
            })
        } else {
            GrammarResult::Execute(Command::Motion {
                count: count_or_default(count),
                motion,
                explicit_count: count.is_some(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::KeyEvent;
    use crate::primitives::VisualType;
    use std::num::NonZeroU32;

    fn parser() -> Parser {
        Parser::new()
    }

    #[test]
    fn test_gh_enters_select_char() {
        let p = parser();
        let result = p.handle_awaiting_prefix(None, None, 'g', None, None, KeyEvent::char('h'));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::SelectEnter {
                visual_type: VisualType::Char,
            })
        );
    }

    #[test]
    fn test_g_upper_h_enters_select_line() {
        let p = parser();
        let result = p.handle_awaiting_prefix(None, None, 'g', None, None, KeyEvent::char('H'));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::SelectEnter {
                visual_type: VisualType::Line,
            })
        );
    }

    #[test]
    fn test_g_ctrl_h_enters_select_block() {
        let p = parser();
        let result = p.handle_awaiting_prefix(None, None, 'g', None, None, KeyEvent::ctrl('h'));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::SelectEnter {
                visual_type: VisualType::Block,
            })
        );
    }

    #[test]
    fn test_g_ctrl_upper_h_enters_select_block() {
        // Ctrl-H with uppercase also triggers blockwise select
        let p = parser();
        let result = p.handle_awaiting_prefix(None, None, 'g', None, None, KeyEvent::ctrl('H'));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::SelectEnter {
                visual_type: VisualType::Block,
            })
        );
    }

    // ── Incremental syntax selection key bindings ─────────────────────

    fn assert_syntax_prefix(key: char, expected_cmd: PrefixCommand) {
        let p = parser();
        let result = p.handle_awaiting_prefix(None, None, 'g', None, None, KeyEvent::char(key));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Prefix {
                count: NonZeroU32::MIN,
                register: None,
                command: expected_cmd,
            })
        );
    }

    #[test]
    fn g_open_bracket_selects_parent_node() {
        assert_syntax_prefix('[', PrefixCommand::SelectParentNode);
    }

    #[test]
    fn g_close_bracket_selects_child_node() {
        assert_syntax_prefix(']', PrefixCommand::SelectChildNode);
    }

    #[test]
    fn g_open_brace_selects_prev_sibling() {
        assert_syntax_prefix('{', PrefixCommand::SelectPrevSibling);
    }

    #[test]
    fn g_close_brace_selects_next_sibling() {
        assert_syntax_prefix('}', PrefixCommand::SelectNextSibling);
    }

    #[test]
    fn g_open_paren_selects_all_siblings() {
        assert_syntax_prefix('(', PrefixCommand::SelectAllSiblings);
    }

    #[test]
    fn g_close_paren_selects_all_children() {
        assert_syntax_prefix(')', PrefixCommand::SelectAllChildren);
    }

    #[test]
    fn g_open_bracket_with_count() {
        let p = parser();
        let result = p.handle_awaiting_prefix(Some(3), None, 'g', None, None, KeyEvent::char('['));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Prefix {
                count: NonZeroU32::new(3).unwrap(),
                register: None,
                command: PrefixCommand::SelectParentNode,
            })
        );
    }

    #[test]
    fn existing_g_prefix_keys_still_work() {
        // gv should still produce Reselect
        let p = parser();
        let result = p.handle_awaiting_prefix(None, None, 'g', None, None, KeyEvent::char('v'));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Visual(VisualKind::Reselect))
        );

        // gd should still produce GotoDefinition
        let result = p.handle_awaiting_prefix(None, None, 'g', None, None, KeyEvent::char('d'));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Prefix {
                count: NonZeroU32::MIN,
                register: None,
                command: PrefixCommand::GotoDefinition,
            })
        );
    }

    // ── Bracket/quote pair navigation key bindings ────────────────────

    fn assert_bracket_motion(prefix: char, key: char, expected: Motion) {
        let p = parser();
        let result = p.handle_awaiting_prefix(None, None, prefix, None, None, KeyEvent::char(key));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Motion {
                count: NonZeroU32::MIN,
                motion: expected,
                explicit_count: false,
            })
        );
    }

    #[test]
    fn close_bracket_b_parses_to_next_bracket_pair() {
        assert_bracket_motion(']', 'b', Motion::NextBracketPair);
    }

    #[test]
    fn open_bracket_b_parses_to_prev_bracket_pair() {
        assert_bracket_motion('[', 'b', Motion::PrevBracketPair);
    }

    #[test]
    fn close_bracket_q_parses_to_next_quote_pair() {
        assert_bracket_motion(']', 'q', Motion::NextQuotePair);
    }

    #[test]
    fn open_bracket_q_parses_to_prev_quote_pair() {
        assert_bracket_motion('[', 'q', Motion::PrevQuotePair);
    }

    #[test]
    fn bracket_pair_with_count() {
        let p = parser();
        let result = p.handle_awaiting_prefix(Some(3), None, ']', None, None, KeyEvent::char('b'));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Motion {
                count: NonZeroU32::new(3).unwrap(),
                motion: Motion::NextBracketPair,
                explicit_count: true,
            })
        );
    }

    #[test]
    fn bracket_pair_as_operator_motion() {
        let p = parser();
        let result = p.handle_awaiting_prefix(
            None,
            None,
            ']',
            Some(Operator::Delete),
            None,
            KeyEvent::char('b'),
        );
        assert_eq!(
            result,
            GrammarResult::Execute(Command::OperatorMotion {
                count: NonZeroU32::MIN,
                register: None,
                operator: Operator::Delete,
                motion: Motion::NextBracketPair,
                force_type: None,
            })
        );
    }

    // ── g-/g+ undo branch navigation ────────────────────────────────

    #[test]
    fn g_minus_produces_undo_earlier() {
        let p = parser();
        let result = p.handle_awaiting_prefix(None, None, 'g', None, None, KeyEvent::char('-'));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Prefix {
                count: NonZeroU32::MIN,
                register: None,
                command: PrefixCommand::UndoEarlier,
            })
        );
    }

    #[test]
    fn g_plus_produces_undo_later() {
        let p = parser();
        let result = p.handle_awaiting_prefix(None, None, 'g', None, None, KeyEvent::char('+'));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Prefix {
                count: NonZeroU32::MIN,
                register: None,
                command: PrefixCommand::UndoLater,
            })
        );
    }

    #[test]
    fn count_3_g_minus_produces_undo_earlier_with_count() {
        let p = parser();
        let result = p.handle_awaiting_prefix(Some(3), None, 'g', None, None, KeyEvent::char('-'));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Prefix {
                count: NonZeroU32::new(3).unwrap(),
                register: None,
                command: PrefixCommand::UndoEarlier,
            })
        );
    }

    #[test]
    fn count_5_g_plus_produces_undo_later_with_count() {
        let p = parser();
        let result = p.handle_awaiting_prefix(Some(5), None, 'g', None, None, KeyEvent::char('+'));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Prefix {
                count: NonZeroU32::new(5).unwrap(),
                register: None,
                command: PrefixCommand::UndoLater,
            })
        );
    }

    // ── gb/gB/gs multi-cursor grammar commands ─────────────────────────

    #[test]
    fn gb_produces_add_next_match_cursor() {
        let p = parser();
        let result = p.handle_awaiting_prefix(None, None, 'g', None, None, KeyEvent::char('b'));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Action {
                count: NonZeroU32::MIN,
                register: None,
                action: Action::AddNextMatchCursor,
            })
        );
    }

    #[test]
    fn g_upper_b_produces_add_prev_match_cursor() {
        let p = parser();
        let result = p.handle_awaiting_prefix(None, None, 'g', None, None, KeyEvent::char('B'));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Action {
                count: NonZeroU32::MIN,
                register: None,
                action: Action::AddPrevMatchCursor,
            })
        );
    }

    #[test]
    fn gs_produces_skip_match_cursor() {
        let p = parser();
        let result = p.handle_awaiting_prefix(None, None, 'g', None, None, KeyEvent::char('s'));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Action {
                count: NonZeroU32::MIN,
                register: None,
                action: Action::SkipMatchCursor,
            })
        );
    }

    #[test]
    fn gb_with_count_3() {
        let p = parser();
        let result = p.handle_awaiting_prefix(Some(3), None, 'g', None, None, KeyEvent::char('b'));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Action {
                count: NonZeroU32::new(3).unwrap(),
                register: None,
                action: Action::AddNextMatchCursor,
            })
        );
    }
}
