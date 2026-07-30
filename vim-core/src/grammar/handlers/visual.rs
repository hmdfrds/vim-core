//! Visual mode handler.
//!
//! Handles key processing when in visual mode.
//! Delegates motions/operators to normal mode handlers.

use crate::grammar::command::{count_or_default, Command, VisualKind};
use crate::grammar::input_state::InputState;
use crate::grammar::parser::Parser;
use crate::grammar::result::GrammarResult;
use crate::grammar::types::TextObjectScope;
use crate::keymap::{KeyClass, KeyEvent};
use crate::primitives::VisualType;

impl Parser {
    /// Handle key input in visual mode.
    ///
    /// Visual mode keys:
    /// - `Esc` → Exit visual mode
    /// - `v` → Exit if char-wise, else switch to char-wise
    /// - `V` → Exit if line-wise, else switch to line-wise
    /// - `Ctrl-V` → Exit if block-wise, else switch to block-wise
    /// - `o` → Swap selection anchor and head
    /// - `O` → Swap block corners (block mode only)
    /// - Motions/operators → Delegate to normal handlers (extend selection)
    pub(crate) fn handle_visual(
        &mut self,
        key: KeyEvent,
        class: KeyClass,
        current_type: VisualType,
    ) -> GrammarResult {
        // Handle Escape - exit visual mode
        if class == KeyClass::Escape {
            return GrammarResult::Execute(Command::Visual(VisualKind::Exit));
        }

        // v/V/Ctrl-V — visual mode switching (single source of truth: visual_type_from_key)
        if let Some(requested_type) = super::helpers::visual_type_from_key(key) {
            return if requested_type == current_type {
                GrammarResult::Execute(Command::Visual(VisualKind::Exit))
            } else {
                GrammarResult::Execute(Command::Visual(VisualKind::Switch {
                    visual_type: requested_type,
                }))
            };
        }

        // Ctrl-G — toggle between Visual and Select mode
        if key == KeyEvent::ctrl('g') {
            return GrammarResult::Execute(Command::Visual(VisualKind::ToggleSelect));
        }

        let char = key.as_char();

        match char {
            // o - swap selection ends
            Some('o') => GrammarResult::Execute(Command::Visual(VisualKind::SwapEnds)),

            // O - swap block corners (only in block mode, otherwise same as o)
            Some('O') => {
                if current_type == VisualType::Block {
                    GrammarResult::Execute(Command::Visual(VisualKind::SwapCorner))
                } else {
                    GrammarResult::Execute(Command::Visual(VisualKind::SwapEnds))
                }
            }

            // : in visual mode → command-line with '<,'> range pre-filled
            Some(':') => {
                use crate::grammar::command_line_intent::CommandLineIntent;
                use crate::primitives::CommandLinePrompt;
                // Set visual-range intent; engine will pre-fill "'<,'>" in command line
                self.set_command_line_intent(CommandLineIntent {
                    prompt: CommandLinePrompt::ExVisual,
                    operator_search: None,
                });
                GrammarResult::ModeChange(crate::primitives::Mode::CommandLine, None)
            }

            // z prefix in visual mode: zy → YankTrimmed
            Some('z') => GrammarResult::Continue(InputState::AwaitingVisualZPrefix {
                register: self.state().register(),
            }),

            // All other keys - handle operators and text objects specially, delegate rest
            _ => {
                // Sneak mode: intercept `s`/`S` before operator dispatch
                if self.sneak_mode() {
                    if let Some(c) = key.as_char() {
                        if c == 's' || c == 'S' {
                            return GrammarResult::Continue(InputState::AwaitingSneakChar1 {
                                count: self.state().count(),
                                register: self.state().register(),
                                operator: None,
                                forward: c == 's',
                            });
                        }
                    }
                }

                // Operators in visual mode apply directly to selection
                if class == KeyClass::Operator {
                    // Visual S → surround (awaits delimiter char, selection is the range)
                    if key.as_char() == Some('S') {
                        return GrammarResult::Continue(InputState::AwaitingSurroundChar {
                            count: self.state().count(),
                            motion: None,
                            textobject: None,
                        });
                    }
                    let Some(operator) = super::helpers::operator_from_key(key) else {
                        return GrammarResult::Invalid;
                    };
                    return GrammarResult::Execute(Command::OperatorSelection {
                        register: self.state().register(),
                        operator,
                    });
                }

                // Case operators in visual mode: U, u, ~ apply to selection directly
                if let Some(c) = char {
                    let case_op = match c {
                        'U' => Some(crate::grammar::types::Operator::Uppercase),
                        'u' => Some(crate::grammar::types::Operator::Lowercase),
                        '~' => Some(crate::grammar::types::Operator::ToggleCase),
                        _ => None,
                    };
                    if let Some(operator) = case_op {
                        return GrammarResult::Execute(Command::OperatorSelection {
                            register: self.state().register(),
                            operator,
                        });
                    }
                }

                // Text object triggers (i, a) in visual mode - transition to await text object type
                // This will SET the selection to the text object range
                if class == KeyClass::TextObjectTrigger {
                    let scope = TextObjectScope::from_inner_flag(key.as_char() == Some('i'));
                    return GrammarResult::Continue(InputState::AwaitingVisualTextObject {
                        count: self.state().count(),
                        scope,
                        register: self.state().register(),
                    });
                }

                // Motions extend the selection - delegate to ready handler
                // Pass the parser state's count and register so counts work in visual mode
                self.handle_ready(self.state().count(), self.state().register(), key, class)
            }
        }
    }

    /// Handle `AwaitingVisualTextObject` state.
    ///
    /// We have 'i' or 'a' in visual mode, waiting for text object type (w, (, ", etc.)
    /// Unlike operator pending mode, this emits `VisualTextObject` which sets the
    /// selection range rather than applying an operator.
    /// Also intercepts `n`/`l` for targets.vim next/last seeking.
    pub(crate) fn handle_awaiting_visual_textobject(
        count: Option<u32>,
        scope: TextObjectScope,
        register: Option<crate::primitives::RegisterName>,
        key: KeyEvent,
    ) -> GrammarResult {
        use crate::grammar::types::{SeekDirection, TextObject, TextObjectKind};

        if key == KeyEvent::escape() {
            return GrammarResult::Execute(Command::Visual(VisualKind::Exit));
        }

        if let Some(c) = key.as_char() {
            // targets.vim: `n` → seek next, `l` → seek last.
            match c {
                'n' => {
                    return GrammarResult::Continue(
                        InputState::AwaitingVisualTextObjectWithModifier {
                            count,
                            scope,
                            register,
                            seek: SeekDirection::Next,
                        },
                    );
                }
                'l' => {
                    return GrammarResult::Continue(
                        InputState::AwaitingVisualTextObjectWithModifier {
                            count,
                            scope,
                            register,
                            seek: SeekDirection::Last,
                        },
                    );
                }
                _ => {}
            }

            // Try built-in text objects first, then semantic.
            if let Some(kind) = TextObjectKind::from_char(c) {
                let textobject = TextObject {
                    scope,
                    kind,
                    seek: None,
                };
                return GrammarResult::Execute(Command::VisualTextObject {
                    count: count_or_default(count),
                    textobject,
                    register,
                });
            }
            if let Some(kind) = TextObjectKind::from_semantic_char(c) {
                let textobject = TextObject {
                    scope,
                    kind,
                    seek: None,
                };
                return GrammarResult::Execute(Command::VisualTextObject {
                    count: count_or_default(count),
                    textobject,
                    register,
                });
            }
        }
        // Unrecognized text object character cancels the pending text object
        // selection — matching Neovim's behavior where invalid text objects
        // abort the command rather than leaving the parser in an intermediate state.
        GrammarResult::Cancel
    }

    /// Handle `AwaitingVisualTextObjectWithModifier` state.
    ///
    /// We have i/a + n/l in visual mode, waiting for delimiter key.
    pub(crate) fn handle_awaiting_visual_textobject_with_modifier(
        count: Option<u32>,
        scope: TextObjectScope,
        register: Option<crate::primitives::RegisterName>,
        seek: crate::grammar::types::SeekDirection,
        key: KeyEvent,
    ) -> GrammarResult {
        use crate::grammar::types::{TextObject, TextObjectKind};

        if key == KeyEvent::escape() {
            return GrammarResult::Execute(Command::Visual(VisualKind::Exit));
        }

        if let Some(c) = key.as_char() {
            if let Some(kind) = TextObjectKind::from_char(c) {
                if kind.supports_seek() {
                    let textobject = TextObject {
                        scope,
                        kind,
                        seek: Some(seek),
                    };
                    return GrammarResult::Execute(Command::VisualTextObject {
                        count: count_or_default(count),
                        textobject,
                        register,
                    });
                }
            }
        }
        GrammarResult::Cancel
    }
    /// Handle `AwaitingVisualZPrefix` state.
    ///
    /// After `z` in visual mode: `y` → YankTrimmed, anything else falls through
    /// to normal z-prefix handling via `handle_ready`.
    pub(crate) fn handle_awaiting_visual_z_prefix(
        register: Option<crate::primitives::RegisterName>,
        key: KeyEvent,
    ) -> GrammarResult {
        if key == KeyEvent::escape() {
            return GrammarResult::Cancel;
        }
        if key.as_char() == Some('y') {
            return GrammarResult::Execute(Command::YankTrimmed { register });
        }
        // Not zy — fall through to cancel (the z-prefix normal commands like
        // zt/zz/zb are handled by handle_ready which we can't call from here
        // as a static method; the user can just press z again in normal mode).
        GrammarResult::Cancel
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::KeyEvent;

    fn parser() -> Parser {
        Parser::new()
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::char(c)
    }

    fn ctrl_key(c: char) -> KeyEvent {
        KeyEvent::ctrl(c)
    }

    fn esc() -> KeyEvent {
        KeyEvent::escape()
    }

    #[test]
    fn test_escape_exits_visual() {
        let mut p = parser();
        let result = p.handle_visual(esc(), KeyClass::Escape, VisualType::Char);
        assert!(matches!(
            result,
            GrammarResult::Execute(Command::Visual(VisualKind::Exit))
        ));
    }

    #[test]
    fn test_v_toggles_char_visual() {
        let mut p = parser();

        // v in char mode -> exit
        let result = p.handle_visual(key('v'), KeyClass::ModeSwitch, VisualType::Char);
        assert!(matches!(
            result,
            GrammarResult::Execute(Command::Visual(VisualKind::Exit))
        ));

        // v in line mode -> switch to char
        let result = p.handle_visual(key('v'), KeyClass::ModeSwitch, VisualType::Line);
        assert!(matches!(
            result,
            GrammarResult::Execute(Command::Visual(VisualKind::Switch {
                visual_type: VisualType::Char
            }))
        ));
    }

    #[test]
    fn test_V_toggles_line_visual() {
        let mut p = parser();

        // V in line mode -> exit
        let result = p.handle_visual(key('V'), KeyClass::ModeSwitch, VisualType::Line);
        assert!(matches!(
            result,
            GrammarResult::Execute(Command::Visual(VisualKind::Exit))
        ));

        // V in char mode -> switch to line
        let result = p.handle_visual(key('V'), KeyClass::ModeSwitch, VisualType::Char);
        assert!(matches!(
            result,
            GrammarResult::Execute(Command::Visual(VisualKind::Switch {
                visual_type: VisualType::Line
            }))
        ));
    }

    #[test]
    fn test_ctrl_v_toggles_block_visual() {
        let mut p = parser();

        // Ctrl-V in block mode -> exit
        let result = p.handle_visual(ctrl_key('v'), KeyClass::ModeSwitch, VisualType::Block);
        assert!(matches!(
            result,
            GrammarResult::Execute(Command::Visual(VisualKind::Exit))
        ));

        // Ctrl-V in char mode -> switch to block
        let result = p.handle_visual(ctrl_key('v'), KeyClass::ModeSwitch, VisualType::Char);
        assert!(matches!(
            result,
            GrammarResult::Execute(Command::Visual(VisualKind::Switch {
                visual_type: VisualType::Block
            }))
        ));
    }

    #[test]
    fn test_o_swaps_ends() {
        let mut p = parser();
        let result = p.handle_visual(key('o'), KeyClass::ModeSwitch, VisualType::Char);
        assert!(matches!(
            result,
            GrammarResult::Execute(Command::Visual(VisualKind::SwapEnds))
        ));
    }

    #[test]
    fn test_O_in_block_mode_swaps_corners() {
        let mut p = parser();

        // O in block mode -> swap corners
        let result = p.handle_visual(key('O'), KeyClass::ModeSwitch, VisualType::Block);
        assert!(matches!(
            result,
            GrammarResult::Execute(Command::Visual(VisualKind::SwapCorner))
        ));

        // O in char mode -> just swap ends
        let result = p.handle_visual(key('O'), KeyClass::ModeSwitch, VisualType::Char);
        assert!(matches!(
            result,
            GrammarResult::Execute(Command::Visual(VisualKind::SwapEnds))
        ));
    }

    #[test]
    fn test_motion_delegates_to_ready() {
        let mut p = parser();

        // j motion -> should produce Motion command
        let result = p.handle_visual(key('j'), KeyClass::Motion, VisualType::Char);
        assert!(matches!(
            result,
            GrammarResult::Execute(Command::Motion { .. })
        ));

        // w motion
        let result = p.handle_visual(key('w'), KeyClass::Motion, VisualType::Char);
        assert!(matches!(
            result,
            GrammarResult::Execute(Command::Motion { .. })
        ));
    }

    #[test]
    fn test_ctrl_g_toggle_select() {
        let mut p = parser();
        let result = p.handle_visual(ctrl_key('g'), KeyClass::Motion, VisualType::Char);
        assert!(matches!(
            result,
            GrammarResult::Execute(Command::Visual(VisualKind::ToggleSelect))
        ));
        // Also works from line-visual
        let result = p.handle_visual(ctrl_key('g'), KeyClass::Motion, VisualType::Line);
        assert!(matches!(
            result,
            GrammarResult::Execute(Command::Visual(VisualKind::ToggleSelect))
        ));
        // Also works from block-visual
        let result = p.handle_visual(ctrl_key('g'), KeyClass::Motion, VisualType::Block);
        assert!(matches!(
            result,
            GrammarResult::Execute(Command::Visual(VisualKind::ToggleSelect))
        ));
    }

    #[test]
    fn test_escape_exits_awaiting_visual_textobject() {
        let result =
            Parser::handle_awaiting_visual_textobject(None, TextObjectScope::Inner, None, esc());
        assert!(matches!(
            result,
            GrammarResult::Execute(Command::Visual(VisualKind::Exit))
        ));
    }
}
