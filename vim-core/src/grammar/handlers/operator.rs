//! Operator state handler.
//!
//! Handles key processing when parser has an operator pending.

use crate::grammar::types::Operator;
use crate::keymap::{KeyClass, KeyEvent};
use crate::primitives::{Mode, MotionType, RegisterName};

use crate::grammar::command::{compute_count, count_or_default, Command};
use crate::grammar::command_line_intent::{CommandLineIntent, OperatorSearchIntent};
use crate::grammar::input_state::InputState;
use crate::grammar::parser::Parser;
use crate::grammar::result::GrammarResult;
use crate::grammar::types::{CharCommand, Motion, TextObject, TextObjectKind, TextObjectScope};
use crate::primitives::CommandLinePrompt;

/// Bundled operator-pending state passed to sub-handlers.
#[derive(Debug, Clone, Copy)]
pub(crate) struct OpCtx {
    pub count: Option<u32>,
    pub register: Option<RegisterName>,
    pub operator: Operator,
    pub count2: Option<u32>,
    pub force_type: Option<MotionType>,
}

impl Parser {
    /// Handle Operator state.
    ///
    /// We have an operator (d, c, y, etc.) and are waiting for:
    /// - Motion → execute operator+motion
    /// - `TextObjectTrigger` (i, a) → await text object type
    /// - Same operator → linewise (dd, yy, cc)
    /// - Digit → build count2
    /// - `CharMotion` (f, t) → await character
    /// - Prefix (g) → await continuation
    pub(crate) fn handle_operator(
        &mut self,
        op: OpCtx,
        key: KeyEvent,
        class: KeyClass,
    ) -> GrammarResult {
        // Sneak mode: intercept `s`/`S` before normal dispatch
        if self.sneak_mode() {
            if let Some(c) = key.as_char() {
                if c == 's' || c == 'S' {
                    use crate::grammar::command::compute_count;
                    let total = compute_count(op.count, op.count2);
                    return GrammarResult::Continue(InputState::AwaitingSneakChar1 {
                        count: Some(total.get()),
                        register: op.register,
                        operator: Some(op.operator),
                        forward: c == 's',
                    });
                }
            }
        }

        // v/V/Ctrl-V in operator-pending → set motion force override
        // s in operator-pending → surround (ys, ds, cs)
        if class == KeyClass::ModeSwitch {
            if let Some(force) = Self::motion_force_from_key(&key) {
                return GrammarResult::Continue(InputState::Operator {
                    count: op.count,
                    register: op.register,
                    operator: op.operator,
                    count2: op.count2,
                    force_type: Some(force),
                });
            }
            // Surround: `ys`, `ds`, `cs` — intercept `s` key
            if key.as_char() == Some('s') {
                return self.handle_surround_trigger(op);
            }
        }
        self.handle_operator_dispatch(op, key, class)
    }

    /// Dispatch operator key by class (extracted for line-limit compliance).
    fn handle_operator_dispatch(
        &mut self,
        op: OpCtx,
        key: KeyEvent,
        class: KeyClass,
    ) -> GrammarResult {
        let OpCtx {
            count,
            register,
            operator,
            count2,
            force_type,
        } = op;

        match class {
            KeyClass::Digit => {
                let digit = key.as_char().and_then(|c| c.to_digit(10)).unwrap_or(0);
                let new_count2 = count2.unwrap_or(0).saturating_mul(10).saturating_add(digit);
                GrammarResult::Continue(InputState::Operator {
                    count,
                    register,
                    operator,
                    count2: Some(new_count2),
                    force_type,
                })
            }
            KeyClass::Motion => Self::handle_operator_motion(
                OpCtx {
                    count,
                    register,
                    operator,
                    count2,
                    force_type,
                },
                key,
            ),
            KeyClass::TextObjectTrigger => {
                let scope = TextObjectScope::from_inner_flag(key.as_char() == Some('i'));
                let total = compute_count(count, count2);
                GrammarResult::Continue(InputState::AwaitingTextObject {
                    count: Some(total.get()),
                    register,
                    operator,
                    scope,
                })
            }
            KeyClass::Operator => {
                Self::handle_operator_same(count, register, operator, count2, key)
            }
            KeyClass::CharMotion => {
                let Some(cmd) = key.as_char().and_then(CharCommand::from_char) else {
                    return GrammarResult::Invalid;
                };
                let total = compute_count(count, count2);
                GrammarResult::Continue(InputState::AwaitingChar {
                    count: Some(total.get()),
                    register,
                    operator: Some(operator),
                    char_command: cmd,
                })
            }
            KeyClass::Prefix => {
                let Some(prefix) = key.as_char() else {
                    return GrammarResult::Invalid;
                };
                let total = compute_count(count, count2);
                GrammarResult::Continue(InputState::AwaitingPrefix {
                    count: Some(total.get()),
                    register,
                    prefix,
                    operator: Some(operator),
                    force_type,
                })
            }
            KeyClass::MarkTrigger => {
                Self::handle_operator_mark(count, register, operator, count2, key)
            }
            KeyClass::RegisterTrigger => GrammarResult::Continue(InputState::AwaitingRegister {
                count,
                phase: crate::grammar::input_state::RegisterPhase::AfterOperator {
                    operator,
                    count2,
                },
            }),
            KeyClass::SearchTrigger => {
                // g?? — Rot13 operator doubled via ? key (not via g prefix)
                if operator == Operator::Rot13 && key.as_char() == Some('?') {
                    let total = compute_count(count, count2);
                    return GrammarResult::Execute(Command::OperatorLine {
                        count: total,
                        register,
                        operator,
                    });
                }
                self.handle_operator_search(count, register, operator, count2, key)
            }
            _ => Self::handle_operator_case_doubled(count, register, operator, count2, key),
        }
    }

    /// Handle motion key in operator-pending mode.
    ///
    /// Special-cases `0`: when count2 is already being built it acts as a digit
    /// (e.g., `d10w`), otherwise it is the `0` motion (go to column 0).
    fn handle_operator_motion(op: OpCtx, key: KeyEvent) -> GrammarResult {
        let OpCtx {
            count,
            register,
            operator,
            count2,
            force_type,
        } = op;
        // `0` after count2 acts as a digit (e.g., `d10w`).
        if key.as_char() == Some('0') {
            if let Some(c2) = count2 {
                let new_count2 = c2.saturating_mul(10);
                return GrammarResult::Continue(InputState::Operator {
                    count,
                    register,
                    operator,
                    count2: Some(new_count2),
                    force_type,
                });
            }
        }
        if let Some(motion) = Motion::from_key_event(&key) {
            let total = compute_count(count, count2);
            GrammarResult::Execute(Command::OperatorMotion {
                count: total,
                register,
                operator,
                motion,
                force_type,
            })
        } else {
            GrammarResult::Invalid
        }
    }

    /// Handle same-operator doubling for linewise commands (dd, yy, cc).
    ///
    /// When `composed-operators` is enabled, pressing a DIFFERENT operator key
    /// during OP composes the two operators instead of returning Invalid.
    /// Pressing the SAME operator key still produces a linewise operation.
    fn handle_operator_same(
        count: Option<u32>,
        register: Option<RegisterName>,
        operator: Operator,
        count2: Option<u32>,
        key: KeyEvent,
    ) -> GrammarResult {
        let Some(key_op) = super::helpers::operator_from_key(key) else {
            return GrammarResult::Invalid;
        };
        if key_op == operator {
            // Same operator doubled (dd, yy, >>): linewise operation.
            let total = compute_count(count, count2);
            GrammarResult::Execute(Command::OperatorLine {
                count: total,
                register,
                operator,
            })
        } else {
            // Different operator: attempt composition (composed-operators feature).
            {
                let total = compute_count(count, count2);
                if let Some(composed) = operator.compose(key_op) {
                    return GrammarResult::Continue(InputState::Operator {
                        count: Some(total.get()),
                        register,
                        operator: composed,
                        count2: None,
                        force_type: None,
                    });
                }
            }
            GrammarResult::Invalid
        }
    }

    /// Handle mark trigger in operator-pending mode (e.g., `y'a`, `d`b`).
    fn handle_operator_mark(
        count: Option<u32>,
        register: Option<RegisterName>,
        operator: Operator,
        count2: Option<u32>,
        key: KeyEvent,
    ) -> GrammarResult {
        let Some(mt) = key
            .as_char()
            .and_then(crate::grammar::types::MarkType::from_char)
        else {
            return GrammarResult::Invalid;
        };
        let total = compute_count(count, count2);
        GrammarResult::Continue(InputState::AwaitingMark {
            count: Some(total.get()),
            mark_type: mt,
            operator: Some(operator),
            register,
        })
    }

    /// Handle search trigger in operator-pending mode (`d/pattern`, `c?pattern`).
    ///
    /// Switches to `CommandLine` mode with the operator search intent stored.
    fn handle_operator_search(
        &mut self,
        count: Option<u32>,
        register: Option<RegisterName>,
        operator: Operator,
        count2: Option<u32>,
        key: KeyEvent,
    ) -> GrammarResult {
        let prompt = if key.as_char() == Some('?') {
            CommandLinePrompt::SearchBackward
        } else {
            CommandLinePrompt::SearchForward
        };
        let total = compute_count(count, count2);
        self.set_command_line_intent(CommandLineIntent {
            prompt,
            operator_search: Some(OperatorSearchIntent {
                operator,
                count: total,
                register,
            }),
        });
        GrammarResult::ModeChange(Mode::CommandLine, None)
    }

    /// Handle case-operator doubling (`guu`, `gUU`, `g~~`) for linewise execution.
    ///
    /// These keys are not classified as `KeyClass::Operator` (to avoid conflicting
    /// with normal mode), so we detect them by matching the char against the active
    /// case operator's source key.
    const fn handle_operator_case_doubled(
        count: Option<u32>,
        register: Option<RegisterName>,
        operator: Operator,
        count2: Option<u32>,
        key: KeyEvent,
    ) -> GrammarResult {
        if let Some(c) = key.as_char() {
            let is_doubled = matches!(
                (c, &operator),
                ('u', Operator::Lowercase)
                    | ('U', Operator::Uppercase)
                    | ('~', Operator::ToggleCase)
                    | ('q', Operator::Format)
                    | ('w', Operator::FormatKeepCursor)
            );
            if is_doubled {
                let total = compute_count(count, count2);
                return GrammarResult::Execute(Command::OperatorLine {
                    count: total,
                    register,
                    operator,
                });
            }
        }
        // Unrecognized key in operator-pending mode cancels the pending operator.
        // This matches Neovim where pressing an invalid key after an operator
        // (e.g., `d\`, `y\`) aborts the command rather than leaving the parser
        // stuck in Operator state.
        GrammarResult::Cancel
    }

    /// Handle `s` in operator-pending mode for surround operations.
    ///
    /// `ys` → surround add (needs motion then char)
    /// `ds` → surround delete (needs char to find)
    /// `cs` → surround change (needs old char then new char)
    const fn handle_surround_trigger(&mut self, op: OpCtx) -> GrammarResult {
        match op.operator {
            Operator::Yank => {
                // `ys` — surround add. Stay in Operator state with Yank so normal
                // motion/textobject parsing works. Set `surround_pending` flag so
                // that when the motion resolves (producing OperatorMotion/TextObject),
                // `maybe_intercept_surround` in parser.rs converts the Execute result
                // to Continue(AwaitingSurroundChar) for delimiter char collection.
                self.set_surround_pending(true);
                let total = compute_count(op.count, op.count2);
                GrammarResult::Continue(InputState::Operator {
                    count: Some(total.get()),
                    register: op.register,
                    operator: Operator::Yank,
                    count2: None,
                    force_type: None,
                })
            }
            Operator::Delete => {
                // `ds` → surround delete (no motion needed — finds the pair itself)
                GrammarResult::Continue(InputState::AwaitingSurroundDeleteChar)
            }
            Operator::Change => {
                // `cs` → surround change (no motion needed)
                GrammarResult::Continue(InputState::AwaitingSurroundOldChar)
            }
            _ => {
                // Other operators + s: not a valid surround command → cancel
                GrammarResult::Cancel
            }
        }
    }

    /// Map v/V/Ctrl-V keys to motion force override type.
    fn motion_force_from_key(key: &KeyEvent) -> Option<MotionType> {
        use crate::keymap::{Key, Modifiers};
        match key {
            k if k.key == Key::Char('v') && k.modifiers.is_empty() => Some(MotionType::CharWise),
            k if k.key == Key::Char('V') && k.modifiers.is_empty() => Some(MotionType::LineWise),
            k if k.key == Key::Char('v') && k.modifiers.contains(Modifiers::CTRL) => {
                Some(MotionType::BlockWise)
            }
            _ => None,
        }
    }

    /// Handle `AwaitingTextObject` state.
    ///
    /// We have operator + i/a, waiting for text object type (w, (, ", etc.)
    /// Also intercepts `n`/`l` for targets.vim next/last seeking.
    pub(crate) const fn handle_awaiting_textobject(
        count: Option<u32>,
        register: Option<RegisterName>,
        operator: Operator,
        scope: TextObjectScope,
        key: KeyEvent,
    ) -> GrammarResult {
        use crate::grammar::types::SeekDirection;

        if let Some(c) = key.as_char() {
            // targets.vim: `n` → seek next, `l` → seek last.
            // These transition to AwaitingTextObjectWithModifier for the third key.
            match c {
                'n' => {
                    return GrammarResult::Continue(InputState::AwaitingTextObjectWithModifier {
                        count,
                        register,
                        operator,
                        scope,
                        seek: SeekDirection::Next,
                    });
                }
                'l' => {
                    return GrammarResult::Continue(InputState::AwaitingTextObjectWithModifier {
                        count,
                        register,
                        operator,
                        scope,
                        seek: SeekDirection::Last,
                    });
                }
                _ => {}
            }

            // Try built-in text objects first (subword etc.), then semantic.
            // This ensures keys like 'S' resolve to Subword (always available)
            // rather than Semantic(Scope) which requires a provider.
            if let Some(kind) = TextObjectKind::from_char(c) {
                let textobject = TextObject {
                    scope,
                    kind,
                    seek: None,
                };
                return GrammarResult::Execute(Command::OperatorTextObject {
                    count: count_or_default(count),
                    register,
                    operator,
                    textobject,
                });
            }
            if let Some(kind) = TextObjectKind::from_semantic_char(c) {
                let textobject = TextObject {
                    scope,
                    kind,
                    seek: None,
                };
                return GrammarResult::Execute(Command::OperatorTextObject {
                    count: count_or_default(count),
                    register,
                    operator,
                    textobject,
                });
            }
        }
        // Unrecognized text object character (e.g., `\`, `z`, `x`) cancels the
        // pending operator — matching Neovim's behavior where invalid text objects
        // after `i`/`a` abort the entire command rather than keeping the parser
        // stuck in AwaitingTextObject state.
        GrammarResult::Cancel
    }

    /// Handle `AwaitingTextObjectWithModifier` state.
    ///
    /// We have operator + i/a + n/l, waiting for delimiter key (", (, {, etc.)
    /// Only delimiter-based text objects are valid here.
    pub(crate) const fn handle_awaiting_textobject_with_modifier(
        count: Option<u32>,
        register: Option<RegisterName>,
        operator: Operator,
        scope: TextObjectScope,
        seek: crate::grammar::types::SeekDirection,
        key: KeyEvent,
    ) -> GrammarResult {
        if let Some(c) = key.as_char() {
            if let Some(kind) = TextObjectKind::from_char(c) {
                if kind.supports_seek() {
                    let textobject = TextObject {
                        scope,
                        kind,
                        seek: Some(seek),
                    };
                    return GrammarResult::Execute(Command::OperatorTextObject {
                        count: count_or_default(count),
                        register,
                        operator,
                        textobject,
                    });
                }
            }
        }
        // Invalid key after seek modifier cancels the entire command.
        GrammarResult::Cancel
    }
}

#[cfg(test)]
mod tests {
    use crate::grammar::command::Command;
    use crate::grammar::input_state::InputState;
    use crate::grammar::parser::Parser;
    use crate::grammar::result::GrammarResult;
    use crate::grammar::types::{Motion, Operator, TextObjectKind, TextObjectScope};
    use crate::keymap::{KeyEvent, Keymap};
    use crate::primitives::Mode;
    use std::num::NonZeroU32;

    fn keymap() -> Keymap {
        Keymap::default()
    }

    // ═══════════════════════════════════════════════════════════════════
    // Composed operator grammar tests
    // ═══════════════════════════════════════════════════════════════════

    /// `y>w` composes Yank+Indent into a Composed operator, then resolves with
    /// `WordForward` motion to produce an `OperatorMotion { operator: Composed(..), .. }`.
    #[test]
    fn y_indent_w_composes_and_produces_operator_motion() {
        let mut p = Parser::new();
        let km = keymap();

        // 'y' → Operator { operator: Yank }
        let r = p.process(KeyEvent::char('y'), &km, Mode::Normal);
        assert!(
            matches!(
                r,
                GrammarResult::Continue(InputState::Operator {
                    operator: Operator::Yank,
                    ..
                })
            ),
            "expected Yank operator pending"
        );

        // '>' (Indent operator key) → should compose Yank + Indent → Composed pending
        let r = p.process(KeyEvent::char('>'), &km, Mode::Normal);
        assert!(
            matches!(
                r,
                GrammarResult::Continue(InputState::Operator {
                    operator: Operator::Composed(_),
                    ..
                })
            ),
            "expected Composed operator pending after y>, got {r:?}"
        );

        // 'w' → OperatorMotion with the Composed operator
        let r = p.process(KeyEvent::char('w'), &km, Mode::Normal);
        match r {
            GrammarResult::Execute(Command::OperatorMotion {
                operator: Operator::Composed(pair),
                motion: Motion::WordForward,
                count: NonZeroU32::MIN,
                ..
            }) => {
                assert_eq!(pair.first(), Operator::Yank, "first should be Yank");
                assert_eq!(pair.second(), Operator::Indent, "second should be Indent");
            }
            other => panic!("expected OperatorMotion with Composed(Yank, Indent), got {other:?}"),
        }
    }

    /// `dd` still produces `OperatorLine { operator: Delete }` — same-key doubling
    /// is NOT affected by the composed-operators feature.
    #[test]
    fn dd_still_produces_linewise_delete() {
        let mut p = Parser::new();
        let km = keymap();

        p.process(KeyEvent::char('d'), &km, Mode::Normal);
        let r = p.process(KeyEvent::char('d'), &km, Mode::Normal);

        assert!(
            matches!(
                r,
                GrammarResult::Execute(Command::OperatorLine {
                    operator: Operator::Delete,
                    count: NonZeroU32::MIN,
                    ..
                })
            ),
            "dd must still produce linewise delete, got {r:?}"
        );
    }

    /// `Operator::Delete.compose(Operator::Delete)` returns `None` — both-destructive
    /// combinations are rejected at the type level.
    #[test]
    fn compose_delete_plus_delete_returns_none() {
        assert!(
            Operator::Delete.compose(Operator::Delete).is_none(),
            "Delete+Delete must be rejected by compose()"
        );
    }

    /// `Operator::Change.compose(anything)` returns `None` — Change is not composable.
    #[test]
    fn compose_change_plus_anything_returns_none() {
        assert!(
            Operator::Change.compose(Operator::Yank).is_none(),
            "Change+Yank must be None"
        );
        assert!(
            Operator::Change.compose(Operator::Indent).is_none(),
            "Change+Indent must be None"
        );
        assert!(
            Operator::Change.compose(Operator::Delete).is_none(),
            "Change+Delete must be None"
        );
        assert!(
            Operator::Yank.compose(Operator::Change).is_none(),
            "Yank+Change must be None"
        );
        assert!(
            Operator::Indent.compose(Operator::Change).is_none(),
            "Indent+Change must be None"
        );
    }

    // ═══════════════════════════════════════════════════════════════════
    // targets.vim next/last text object grammar tests
    // ═══════════════════════════════════════════════════════════════════

    /// `din"` → OperatorTextObject with seek: Some(Next), kind: DoubleQuote
    #[test]
    fn din_quote_produces_operator_textobject_with_next_seek() {
        use crate::grammar::types::SeekDirection;
        let mut p = Parser::default();
        let km = keymap();

        // 'd' → Operator { Delete }
        let r = p.process(KeyEvent::char('d'), &km, Mode::Normal);
        assert!(matches!(
            r,
            GrammarResult::Continue(InputState::Operator {
                operator: Operator::Delete,
                ..
            })
        ));

        // 'i' → AwaitingTextObject { scope: Inner }
        let r = p.process(KeyEvent::char('i'), &km, Mode::Normal);
        assert!(matches!(
            r,
            GrammarResult::Continue(InputState::AwaitingTextObject {
                operator: Operator::Delete,
                scope: TextObjectScope::Inner,
                ..
            })
        ));

        // 'n' → AwaitingTextObjectWithModifier { seek: Next }
        let r = p.process(KeyEvent::char('n'), &km, Mode::Normal);
        assert!(matches!(
            r,
            GrammarResult::Continue(InputState::AwaitingTextObjectWithModifier {
                operator: Operator::Delete,
                scope: TextObjectScope::Inner,
                seek: SeekDirection::Next,
                ..
            })
        ));

        // '"' → Execute OperatorTextObject with seek
        let r = p.process(KeyEvent::char('"'), &km, Mode::Normal);
        match r {
            GrammarResult::Execute(Command::OperatorTextObject {
                operator,
                textobject,
                ..
            }) => {
                assert_eq!(operator, Operator::Delete);
                assert_eq!(textobject.scope, TextObjectScope::Inner);
                assert_eq!(textobject.kind, TextObjectKind::DoubleQuote);
                assert_eq!(textobject.seek, Some(SeekDirection::Next));
            }
            other => panic!("expected Execute(OperatorTextObject), got {other:?}"),
        }
    }

    /// `dal(` → OperatorTextObject with seek: Some(Last), kind: Paren
    #[test]
    fn dal_paren_produces_operator_textobject_with_last_seek() {
        use crate::grammar::types::SeekDirection;
        let mut p = Parser::default();
        let km = keymap();

        p.process(KeyEvent::char('d'), &km, Mode::Normal);
        p.process(KeyEvent::char('a'), &km, Mode::Normal);

        // 'l' → AwaitingTextObjectWithModifier { seek: Last }
        let r = p.process(KeyEvent::char('l'), &km, Mode::Normal);
        assert!(matches!(
            r,
            GrammarResult::Continue(InputState::AwaitingTextObjectWithModifier {
                scope: TextObjectScope::Around,
                seek: SeekDirection::Last,
                ..
            })
        ));

        // '(' → Execute OperatorTextObject
        let r = p.process(KeyEvent::char('('), &km, Mode::Normal);
        match r {
            GrammarResult::Execute(Command::OperatorTextObject {
                operator,
                textobject,
                ..
            }) => {
                assert_eq!(operator, Operator::Delete);
                assert_eq!(textobject.scope, TextObjectScope::Around);
                assert_eq!(textobject.kind, TextObjectKind::Paren);
                assert_eq!(textobject.seek, Some(SeekDirection::Last));
            }
            other => panic!("expected Execute(OperatorTextObject), got {other:?}"),
        }
    }

    /// `cin{` → OperatorTextObject with Change, seek: Next, Brace
    #[test]
    fn cin_brace_produces_change_next_brace() {
        use crate::grammar::types::SeekDirection;
        let mut p = Parser::default();
        let km = keymap();

        p.process(KeyEvent::char('c'), &km, Mode::Normal);
        p.process(KeyEvent::char('i'), &km, Mode::Normal);
        p.process(KeyEvent::char('n'), &km, Mode::Normal);
        let r = p.process(KeyEvent::char('{'), &km, Mode::Normal);
        match r {
            GrammarResult::Execute(Command::OperatorTextObject {
                operator,
                textobject,
                ..
            }) => {
                assert_eq!(operator, Operator::Change);
                assert_eq!(textobject.kind, TextObjectKind::Brace);
                assert_eq!(textobject.seek, Some(SeekDirection::Next));
            }
            other => panic!("expected Execute(OperatorTextObject), got {other:?}"),
        }
    }

    /// `yil[` → OperatorTextObject with Yank, seek: Last, Bracket
    #[test]
    fn yil_bracket_produces_yank_last_bracket() {
        use crate::grammar::types::SeekDirection;
        let mut p = Parser::default();
        let km = keymap();

        p.process(KeyEvent::char('y'), &km, Mode::Normal);
        p.process(KeyEvent::char('i'), &km, Mode::Normal);
        p.process(KeyEvent::char('l'), &km, Mode::Normal);
        let r = p.process(KeyEvent::char('['), &km, Mode::Normal);
        match r {
            GrammarResult::Execute(Command::OperatorTextObject {
                operator,
                textobject,
                ..
            }) => {
                assert_eq!(operator, Operator::Yank);
                assert_eq!(textobject.kind, TextObjectKind::Bracket);
                assert_eq!(textobject.seek, Some(SeekDirection::Last));
            }
            other => panic!("expected Execute(OperatorTextObject), got {other:?}"),
        }
    }

    /// Normal `diw` still works without seek modifier.
    #[test]
    fn diw_without_modifier_has_no_seek() {
        let mut p = Parser::default();
        let km = keymap();

        p.process(KeyEvent::char('d'), &km, Mode::Normal);
        p.process(KeyEvent::char('i'), &km, Mode::Normal);
        let r = p.process(KeyEvent::char('w'), &km, Mode::Normal);
        match r {
            GrammarResult::Execute(Command::OperatorTextObject { textobject, .. }) => {
                assert_eq!(textobject.kind, TextObjectKind::Word);
                assert_eq!(textobject.seek, None);
            }
            other => panic!("expected Execute(OperatorTextObject), got {other:?}"),
        }
    }

    /// Seek modifier + non-seekable text object → Cancel.
    /// `dinw` should cancel because Word doesn't support seeking.
    #[test]
    fn seek_modifier_with_non_seekable_cancels() {
        let mut p = Parser::default();
        let km = keymap();

        p.process(KeyEvent::char('d'), &km, Mode::Normal);
        p.process(KeyEvent::char('i'), &km, Mode::Normal);
        p.process(KeyEvent::char('n'), &km, Mode::Normal);
        let r = p.process(KeyEvent::char('w'), &km, Mode::Normal);
        assert!(matches!(r, GrammarResult::Cancel));
    }

    /// `dint` → seek next tag
    #[test]
    fn din_tag_produces_next_tag() {
        use crate::grammar::types::SeekDirection;
        let mut p = Parser::default();
        let km = keymap();

        p.process(KeyEvent::char('d'), &km, Mode::Normal);
        p.process(KeyEvent::char('i'), &km, Mode::Normal);
        p.process(KeyEvent::char('n'), &km, Mode::Normal);
        let r = p.process(KeyEvent::char('t'), &km, Mode::Normal);
        match r {
            GrammarResult::Execute(Command::OperatorTextObject { textobject, .. }) => {
                assert_eq!(textobject.kind, TextObjectKind::Tag);
                assert_eq!(textobject.seek, Some(SeekDirection::Next));
            }
            other => panic!("expected Execute(OperatorTextObject), got {other:?}"),
        }
    }

    /// `din'` → seek next single quote
    #[test]
    fn din_single_quote_produces_next() {
        use crate::grammar::types::SeekDirection;
        let mut p = Parser::default();
        let km = keymap();

        p.process(KeyEvent::char('d'), &km, Mode::Normal);
        p.process(KeyEvent::char('i'), &km, Mode::Normal);
        p.process(KeyEvent::char('n'), &km, Mode::Normal);
        let r = p.process(KeyEvent::char('\''), &km, Mode::Normal);
        match r {
            GrammarResult::Execute(Command::OperatorTextObject { textobject, .. }) => {
                assert_eq!(textobject.kind, TextObjectKind::SingleQuote);
                assert_eq!(textobject.seek, Some(SeekDirection::Next));
            }
            other => panic!("expected Execute(OperatorTextObject), got {other:?}"),
        }
    }

    /// `dinb` → seek next AnyBracket
    #[test]
    fn din_any_bracket_produces_next() {
        use crate::grammar::types::SeekDirection;
        let mut p = Parser::default();
        let km = keymap();

        p.process(KeyEvent::char('d'), &km, Mode::Normal);
        p.process(KeyEvent::char('i'), &km, Mode::Normal);
        p.process(KeyEvent::char('n'), &km, Mode::Normal);
        let r = p.process(KeyEvent::char('b'), &km, Mode::Normal);
        match r {
            GrammarResult::Execute(Command::OperatorTextObject { textobject, .. }) => {
                assert_eq!(textobject.kind, TextObjectKind::AnyBracket);
                assert_eq!(textobject.seek, Some(SeekDirection::Next));
            }
            other => panic!("expected Execute(OperatorTextObject), got {other:?}"),
        }
    }

    /// Seek modifier + invalid char → Cancel.
    #[test]
    fn seek_modifier_with_invalid_char_cancels() {
        let mut p = Parser::default();
        let km = keymap();

        p.process(KeyEvent::char('d'), &km, Mode::Normal);
        p.process(KeyEvent::char('i'), &km, Mode::Normal);
        p.process(KeyEvent::char('n'), &km, Mode::Normal);
        // 'z' is not any text object
        let r = p.process(KeyEvent::char('z'), &km, Mode::Normal);
        assert!(matches!(r, GrammarResult::Cancel));
    }
}
