//! Ready state handler.
//!
//! Handles key processing when parser is in Ready state.

use crate::keymap::{KeyClass, KeyEvent};
use crate::primitives::Mode;
use crate::primitives::RegisterName;
use std::num::NonZeroU32;

use crate::grammar::command::{count_or_default, Command, MacroKind, VisualKind};
use crate::grammar::command_line_intent::CommandLineIntent;
use crate::grammar::input_state::{InputState, MacroAwaitKind};
use crate::grammar::parser::Parser;
use crate::grammar::result::GrammarResult;
use crate::grammar::types::{Action, CharCommand, MarkType, Motion};
use crate::primitives::CommandLinePrompt;

impl Parser {
    /// Handle Ready state.
    ///
    /// Ready is the initial state. From here we can accept:
    /// - Digits (1-9) → build count
    /// - `RegisterTrigger` (") → await register name
    /// - Operator (d,c,y) → await motion/textobj
    /// - Motion (j,w,$) → execute immediately
    /// - `CharMotion` (f,t) → await character
    /// - Action (x,p,.) → execute immediately
    /// - `ModeSwitch` (i,v) → change mode
    /// - Prefix (g,z) → await continuation
    /// - MarkTrigger (m,',`) → await mark name
    pub(crate) fn handle_ready(
        &mut self,
        count: Option<u32>,
        register: Option<RegisterName>,
        key: KeyEvent,
        class: KeyClass,
    ) -> GrammarResult {
        match class {
            KeyClass::Digit => Self::handle_ready_digit(count, register, key),
            KeyClass::RegisterTrigger => GrammarResult::Continue(InputState::AwaitingRegister {
                count,
                phase: crate::grammar::input_state::RegisterPhase::BeforeOperator,
            }),
            KeyClass::Operator => Self::handle_ready_operator(count, register, key),
            KeyClass::Motion => Self::handle_ready_motion(count, register, key),
            KeyClass::CharMotion => Self::handle_ready_char_motion(count, register, key),
            KeyClass::Action => self.handle_ready_action(count, register, key),
            KeyClass::ModeSwitch => self.handle_ready_mode_switch(count, register, key),
            KeyClass::Prefix => {
                let Some(prefix) = key.as_char() else {
                    return GrammarResult::Invalid;
                };
                GrammarResult::Continue(InputState::AwaitingPrefix {
                    count,
                    register,
                    prefix,
                    operator: None,
                    force_type: None,
                })
            }
            KeyClass::MarkTrigger => {
                let Some(mt) = key.as_char().and_then(MarkType::from_char) else {
                    return GrammarResult::Invalid;
                };
                GrammarResult::Continue(InputState::AwaitingMark {
                    count,
                    mark_type: mt,
                    operator: None,
                    register,
                })
            }
            KeyClass::SearchTrigger => self.handle_ready_search(count, key),
            KeyClass::MacroTrigger => Self::handle_ready_macro(self.is_recording(), count, key),
            _ => GrammarResult::Invalid,
        }
    }

    /// Handle digit in Ready state.
    fn handle_ready_digit(
        count: Option<u32>,
        register: Option<RegisterName>,
        key: KeyEvent,
    ) -> GrammarResult {
        // KeyClass::Digit guarantees this is a digit char
        let digit = key.as_char().and_then(|c| c.to_digit(10)).unwrap_or(0);

        // Special case: 0 at start is motion, not digit
        if count.is_none() && digit == 0 {
            return GrammarResult::Execute(Command::Motion {
                count: NonZeroU32::MIN,
                motion: Motion::LineStart,
                explicit_count: false,
            });
        }

        let new_count = count.unwrap_or(0).saturating_mul(10).saturating_add(digit);
        GrammarResult::Continue(InputState::Ready {
            count: Some(new_count),
            register,
        })
    }

    /// Handle action keys in Ready state.
    ///
    /// Processes dot-repeat (`.`) as a special case, then falls back to
    /// regular action lookup via `Action::from_char` and `Action::from_ctrl_char`.
    fn handle_ready_action(
        &mut self,
        count: Option<u32>,
        register: Option<RegisterName>,
        key: KeyEvent,
    ) -> GrammarResult {
        // Special case: dot repeat (.) — handled before Action lookup
        // since '.' is not an Action variant.
        if key.as_char() == Some('.') {
            if let Some(cmd) = self.last_command() {
                // If user supplied an explicit count (even `1.`), override
                // the original command's count. If no count, replay as-is.
                let mut replayed_cmd = if let Some(c) = count.and_then(std::num::NonZeroU32::new) {
                    cmd.with_count(c)
                } else {
                    cmd.clone()
                };

                // Auto-increment numbered register per `:help .`:
                // "If the command included a specification of a numbered register,
                // the register number will be incremented."
                // e.g., `"1p` then `.` → pastes register 2, then 3, etc.
                // Register 9 stays at 9 (no wrap-around).
                if let Some(reg) = replayed_cmd.register() {
                    if let Some(next) = reg.next_numbered() {
                        replayed_cmd = replayed_cmd.with_register(Some(next));
                    }
                }

                self.set_repeat();
                return GrammarResult::Execute(replayed_cmd);
            }
            return GrammarResult::Invalid;
        }

        // Ctrl-W — window command prefix (two-key sequence)
        if key
            == KeyEvent::new(
                crate::keymap::Key::Char('w'),
                crate::keymap::Modifiers::CTRL,
            )
        {
            return GrammarResult::Continue(InputState::AwaitingWindowCommand { count, register });
        }

        // Try regular char first, then ctrl+char
        // For regular chars, use as_char() which only returns Some for unmodified keys
        // For ctrl keys, we need to get the base character from key.key directly
        let action = key.as_char().and_then(Action::from_char).or_else(|| {
            // Check for control key actions (Ctrl-R, Ctrl-O, Ctrl-I)
            use crate::keymap::Modifiers;
            if key.modifiers.contains(Modifiers::CTRL) {
                key.key.as_char().and_then(Action::from_ctrl_char)
            } else {
                None
            }
        });

        if let Some(action) = action {
            GrammarResult::Execute(Command::Action {
                count: count_or_default(count),
                register,
                action,
            })
        } else {
            match key.key() {
                crate::keymap::Key::Delete if count.is_some() => {
                    // Delete key during count accumulation: remove last digit.
                    // 123<Del> → 12, 12<Del> → 1, 1<Del> → no count.
                    let new_count = count.unwrap_or(0) / 10;
                    if new_count == 0 {
                        GrammarResult::Continue(InputState::Ready {
                            count: None,
                            register,
                        })
                    } else {
                        GrammarResult::Continue(InputState::Ready {
                            count: Some(new_count),
                            register,
                        })
                    }
                }
                crate::keymap::Key::Delete => GrammarResult::Execute(Command::Action {
                    count: count_or_default(count),
                    register,
                    action: Action::DeleteChar,
                }),
                crate::keymap::Key::Tab => GrammarResult::Execute(Command::Action {
                    count: count_or_default(count),
                    register,
                    action: Action::JumpNewer,
                }),
                // drift: named keys other than Delete and Tab have no normal-mode binding and produce an invalid sequence
                _ => GrammarResult::Invalid,
            }
        }
    }

    /// Handle mode switch keys in Ready state.
    ///
    /// Dispatches insert entry (i/I/a/A/o/O/s/S), visual entry (v/V/Ctrl-V),
    /// command-line mode (:), and other mode switches (R).
    fn handle_ready_mode_switch(
        &mut self,
        count: Option<u32>,
        register: Option<RegisterName>,
        key: KeyEvent,
    ) -> GrammarResult {
        // Sneak mode: `s`/`S` become two-character find motions
        if self.sneak_mode() {
            if let Some(c) = key.as_char() {
                if c == 's' || c == 'S' {
                    return GrammarResult::Continue(InputState::AwaitingSneakChar1 {
                        count,
                        register,
                        operator: None,
                        forward: c == 's',
                    });
                }
            }
        }

        // Insert entry keys (i/I/a/A/o/O/s/S) return InsertEntry command
        if let Some(entry_type) = super::helpers::entry_type_from_key(key) {
            return GrammarResult::Execute(Command::InsertEntry {
                count: count_or_default(count),
                entry_type,
                register,
            });
        }
        // <Insert> key enters Insert mode (same as 'i')
        if key.key() == crate::keymap::Key::Insert {
            return GrammarResult::Execute(Command::InsertEntry {
                count: count_or_default(count),
                entry_type: crate::primitives::InsertEntryType::BeforeCursor,
                register,
            });
        }
        // Visual mode entry (v/V/Ctrl-V) should use VisualEnter command
        // to properly set selection anchor
        if let Some(visual_type) = super::helpers::visual_type_from_key(key) {
            return GrammarResult::Execute(Command::Visual(VisualKind::Enter {
                visual_type,
                count,
            }));
        }
        if key.as_char() == Some(':') {
            self.set_command_line_intent(CommandLineIntent {
                prompt: CommandLinePrompt::Ex,
                operator_search: None,
            });
            return GrammarResult::ModeChange(Mode::CommandLine, None);
        }
        // Other mode switches (R) return ModeChange with count for repeat-on-exit
        let Some(mode) = super::helpers::mode_from_key(key) else {
            return GrammarResult::Invalid;
        };
        GrammarResult::ModeChange(mode, count)
    }

    /// Handle macro trigger keys in Ready state.
    ///
    /// Processes `q` (start/stop recording) and `@` (play macro).
    /// Takes `is_recording` by value to avoid borrowing `self`.
    const fn handle_ready_macro(
        is_recording: bool,
        count: Option<u32>,
        key: KeyEvent,
    ) -> GrammarResult {
        if let Some(c) = key.as_char() {
            if c == 'q' {
                // Check if currently recording
                if is_recording {
                    // Stop recording
                    return GrammarResult::Execute(Command::Macro(MacroKind::Stop));
                }
                // Start recording - await register
                return GrammarResult::Continue(InputState::AwaitingMacroRegister {
                    count,
                    kind: MacroAwaitKind::Record,
                });
            }
            if c == '@' {
                // Play macro - await register
                return GrammarResult::Continue(InputState::AwaitingMacroRegister {
                    count,
                    kind: MacroAwaitKind::Play,
                });
            }
        }
        GrammarResult::Invalid
    }

    /// Handle operator key in Ready state.
    ///
    /// Looks up the operator from the key and transitions to `Operator` input state
    /// to await a motion or text object.
    const fn handle_ready_operator(
        count: Option<u32>,
        register: Option<RegisterName>,
        key: KeyEvent,
    ) -> GrammarResult {
        let Some(op) = super::helpers::operator_from_key(key) else {
            return GrammarResult::Invalid;
        };
        GrammarResult::Continue(InputState::Operator {
            count,
            register,
            operator: op,
            count2: None,
            force_type: None,
        })
    }

    /// Handle character motion key in Ready state.
    ///
    /// Parses the char-motion command (f/F/t/T) and transitions to `AwaitingChar`
    /// to receive the target character.
    fn handle_ready_char_motion(
        count: Option<u32>,
        register: Option<RegisterName>,
        key: KeyEvent,
    ) -> GrammarResult {
        let Some(char_command) = key.as_char().and_then(CharCommand::from_char) else {
            return GrammarResult::Invalid;
        };
        GrammarResult::Continue(InputState::AwaitingChar {
            count,
            register,
            operator: None,
            char_command,
        })
    }

    /// Handle search trigger key in Ready state.
    ///
    /// Determines search direction from the key (`/` or `?`) and transitions to
    /// command-line mode with the appropriate search prompt.
    fn handle_ready_search(&mut self, count: Option<u32>, key: KeyEvent) -> GrammarResult {
        let prompt = if key.as_char() == Some('?') {
            CommandLinePrompt::SearchBackward
        } else {
            CommandLinePrompt::SearchForward
        };
        self.set_command_line_intent(CommandLineIntent {
            prompt,
            operator_search: None,
        });
        GrammarResult::ModeChange(Mode::CommandLine, count)
    }

    /// Handle motion in Ready state.
    fn handle_ready_motion(
        count: Option<u32>,
        register: Option<RegisterName>,
        key: KeyEvent,
    ) -> GrammarResult {
        // NOTE: `0` is classified as KeyClass::Motion in core.rs, but when
        // a count is already being built it acts as a digit (appending 0).
        // See also: core.rs normal mode map, operator.rs::KeyClass::Motion arm.
        if key.as_char() == Some('0') {
            if let Some(c) = count {
                let new_count = c.saturating_mul(10);
                return GrammarResult::Continue(InputState::Ready {
                    count: Some(new_count),
                    register,
                });
            }
        }

        if let Some(motion) = Motion::from_key_event(&key) {
            GrammarResult::Execute(Command::Motion {
                count: count_or_default(count),
                motion,
                explicit_count: count.is_some(),
            })
        } else {
            GrammarResult::Invalid
        }
    }
}
