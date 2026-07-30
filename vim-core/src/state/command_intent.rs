//! CommandIntent type for recording repeatable command intent.
//!
//! Captures the semantic intent of a command for dot-repeat tracking
//! and undo/history purposes.
//!
//! # Layering
//!
//! State holds pure data containers with no execution logic. Imports
//! `primitives`, `std`, and — as a narrow exception — `grammar::types` and
//! `grammar::Command`; must not import `commands`, `effects`, `execution`
//! or `dispatch`.

use compact_str::CompactString;
use std::num::NonZeroU32;

use crate::grammar::types::{Action, Motion, TextObject};
use crate::grammar::Command;
use crate::primitives::{InsertEntryType, Operator, RegisterName, VisualType};

/// The semantic intent of a repeatable command.
///
/// `CommandIntent` captures the high-level intent of a command that was
/// executed, enabling dot-repeat, undo grouping, and intent-based history.
///
/// Unlike [`crate::grammar::Command`] which is the raw parsed command,
/// `CommandIntent` is the normalized, repeatable form with the inserted
/// text baked in for insert-mode entries.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CommandIntent {
    /// Operator applied to a motion (e.g., `dw`, `3cj`, `y$`).
    OperatorMotion {
        /// The operator applied.
        operator: Operator,
        /// The motion target.
        motion: Motion,
        /// Optional count override.
        count: Option<NonZeroU32>,
        /// Text inserted during insert mode (for change-type operators).
        inserted_text: Option<CompactString>,
        /// Target register.
        register: Option<RegisterName>,
    },

    /// Operator applied to a text object (e.g., `diw`, `ca(`, `yap`).
    OperatorTextObject {
        /// The operator applied.
        operator: Operator,
        /// The text object target.
        text_object: TextObject,
        /// Optional count override.
        count: Option<NonZeroU32>,
        /// Text inserted during insert mode (for change-type operators).
        inserted_text: Option<CompactString>,
        /// Target register.
        register: Option<RegisterName>,
    },

    /// Linewise operator (e.g., `dd`, `yy`, `cc`, `3dd`).
    OperatorLine {
        /// The operator applied.
        operator: Operator,
        /// Optional count override.
        count: Option<NonZeroU32>,
        /// Text inserted during insert mode (for change-type operators).
        inserted_text: Option<CompactString>,
        /// Target register.
        register: Option<RegisterName>,
    },

    /// Operator applied to a visual selection.
    OperatorVisual {
        /// The operator applied.
        operator: Operator,
        /// The type of visual selection.
        visual_type: VisualType,
        /// Text inserted during insert mode (for change-type operators).
        inserted_text: Option<CompactString>,
        /// Target register.
        register: Option<RegisterName>,
    },

    /// Standalone action command (e.g., `x`, `3p`).
    Action {
        /// The action.
        action: Action,
        /// Optional count override.
        count: Option<NonZeroU32>,
    },

    /// Insert-mode entry with accumulated text (e.g., `i`, `a`, `o`).
    InsertEntry {
        /// How insert mode was entered.
        entry_type: InsertEntryType,
        /// The text that was typed/inserted during the insert session.
        inserted_text: CompactString,
    },
}

/// Capture the semantic intent of a command.
///
/// Maps a parsed [`Command`] to a [`CommandIntent`], extracting the
/// operator, motion/text-object/action, count, and register. Returns
/// `None` for command variants that don't have a meaningful repeatable intent
/// (e.g., pure motions, mode switches, macros).
///
/// `inserted_text` is left as `None` for insert-producing commands (change
/// operators, insert entries) — it is patched in later when insert mode exits.
///
/// `visual_type` is required for `OperatorSelection` commands, since the
/// command variant itself does not carry the visual type. Pass the current
/// visual type from state when available.
#[must_use]
pub fn capture_intent(command: &Command, visual_type: Option<VisualType>) -> Option<CommandIntent> {
    match command {
        Command::OperatorMotion {
            operator,
            motion,
            count,
            register,
            ..
        } => Some(CommandIntent::OperatorMotion {
            operator: *operator,
            motion: *motion,
            count: Some(*count),
            inserted_text: None,
            register: *register,
        }),

        Command::OperatorTextObject {
            operator,
            textobject,
            count,
            register,
        } => Some(CommandIntent::OperatorTextObject {
            operator: *operator,
            text_object: *textobject,
            count: Some(*count),
            inserted_text: None,
            register: *register,
        }),

        Command::OperatorLine {
            operator,
            count,
            register,
        } => Some(CommandIntent::OperatorLine {
            operator: *operator,
            count: Some(*count),
            inserted_text: None,
            register: *register,
        }),

        Command::OperatorSelection { operator, register } => Some(CommandIntent::OperatorVisual {
            operator: *operator,
            visual_type: visual_type.unwrap_or(VisualType::Char),
            inserted_text: None,
            register: *register,
        }),

        Command::Action { action, count, .. } => Some(CommandIntent::Action {
            action: *action,
            count: Some(*count),
        }),

        Command::InsertEntry { entry_type, .. } => Some(CommandIntent::InsertEntry {
            entry_type: *entry_type,
            inserted_text: CompactString::default(),
        }),

        // Other command variants don't have a meaningful repeatable intent.
        _ => None,
    }
}

impl std::fmt::Display for CommandIntent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OperatorMotion {
                operator, motion, ..
            } => {
                write!(f, "{operator:?}+{motion:?}")
            }
            Self::OperatorTextObject {
                operator,
                text_object,
                ..
            } => {
                write!(f, "{operator:?}+{text_object:?}")
            }
            Self::OperatorLine { operator, .. } => {
                write!(f, "{operator:?}+Line")
            }
            Self::OperatorVisual {
                operator,
                visual_type,
                ..
            } => {
                write!(f, "{operator:?}+Visual({visual_type:?})")
            }
            Self::Action { action, .. } => {
                write!(f, "Action({action:?})")
            }
            Self::InsertEntry { entry_type, .. } => {
                write!(f, "Insert({entry_type:?})")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::types::{Motion, Operator};
    use std::num::NonZeroU32;

    #[test]
    fn capture_operator_motion() {
        let cmd = Command::OperatorMotion {
            count: NonZeroU32::new(2).unwrap(),
            register: None,
            operator: Operator::Delete,
            motion: Motion::WordForward,
            force_type: None,
        };
        let intent = capture_intent(&cmd, None).unwrap();
        assert!(matches!(
            intent,
            CommandIntent::OperatorMotion {
                operator: Operator::Delete,
                motion: Motion::WordForward,
                count: Some(c),
                inserted_text: None,
                register: None,
            } if c == NonZeroU32::new(2).unwrap()
        ));
    }

    #[test]
    fn capture_operator_textobject() {
        use crate::grammar::types::{TextObjectKind, TextObjectScope};
        let inner_word = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Word,
            seek: None,
        };
        let cmd = Command::OperatorTextObject {
            count: NonZeroU32::MIN,
            register: None,
            operator: Operator::Change,
            textobject: inner_word,
        };
        let intent = capture_intent(&cmd, None).unwrap();
        match intent {
            CommandIntent::OperatorTextObject {
                operator,
                text_object,
                count,
                inserted_text,
                register,
            } => {
                assert_eq!(operator, Operator::Change);
                assert_eq!(text_object.scope, TextObjectScope::Inner);
                assert_eq!(text_object.kind, TextObjectKind::Word);
                assert_eq!(count, Some(NonZeroU32::MIN));
                assert!(inserted_text.is_none());
                assert!(register.is_none());
            }
            _ => panic!("expected OperatorTextObject"),
        }
    }

    #[test]
    fn capture_operator_line() {
        let cmd = Command::OperatorLine {
            count: NonZeroU32::new(3).unwrap(),
            register: None,
            operator: Operator::Delete,
        };
        let intent = capture_intent(&cmd, None).unwrap();
        assert!(matches!(
            intent,
            CommandIntent::OperatorLine {
                operator: Operator::Delete,
                count: Some(c),
                inserted_text: None,
                register: None,
            } if c == NonZeroU32::new(3).unwrap()
        ));
    }

    #[test]
    fn capture_operator_selection_with_visual_type() {
        let cmd = Command::OperatorSelection {
            register: None,
            operator: Operator::Delete,
        };
        let intent = capture_intent(&cmd, Some(VisualType::Line)).unwrap();
        assert!(matches!(
            intent,
            CommandIntent::OperatorVisual {
                operator: Operator::Delete,
                visual_type: VisualType::Line,
                inserted_text: None,
                register: None,
            }
        ));
    }

    #[test]
    fn capture_action() {
        let cmd = Command::Action {
            count: NonZeroU32::MIN,
            register: None,
            action: Action::DeleteChar,
        };
        let intent = capture_intent(&cmd, None).unwrap();
        assert!(matches!(
            intent,
            CommandIntent::Action {
                action: Action::DeleteChar,
                count: Some(NonZeroU32::MIN),
            }
        ));
    }

    #[test]
    fn capture_insert_entry() {
        let cmd = Command::InsertEntry {
            count: NonZeroU32::MIN,
            entry_type: InsertEntryType::BeforeCursor,
            register: None,
        };
        let intent = capture_intent(&cmd, None).unwrap();
        assert!(matches!(intent, CommandIntent::InsertEntry { .. }));
    }

    #[test]
    fn capture_motion_returns_none() {
        let cmd = Command::Motion {
            count: NonZeroU32::MIN,
            motion: Motion::WordForward,
            explicit_count: false,
        };
        assert!(capture_intent(&cmd, None).is_none());
    }

    #[test]
    fn display_formats_nicely() {
        let intent = CommandIntent::OperatorMotion {
            operator: Operator::Delete,
            motion: Motion::WordForward,
            count: Some(NonZeroU32::MIN),
            inserted_text: None,
            register: None,
        };
        let s = format!("{intent}");
        assert!(s.contains("Delete"));
        assert!(s.contains("WordForward"));
    }
}
