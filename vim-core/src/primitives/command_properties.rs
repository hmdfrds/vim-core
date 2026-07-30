//! Command properties for vim-core.
//!
//! Describes how a command interacts with repeat, jumps, and operator logic.

/// How a command interacts with the repeat (`.`) mechanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum RepeatBehavior {
    /// Record the command for later repetition with `.`.
    Record,
    /// Do not record; skip this command during repeat.
    #[default]
    Skip,
    /// Record only the motion portion for repeat.
    MotionOnly,
    /// Abort any in-progress repeat sequence.
    Abort,
}

/// Properties that govern how a command behaves in the editor engine.
///
/// Used to configure repeat behavior, jump list integration, visual mode
/// persistence, cursor movement, operator suppression, and idempotency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[expect(
    clippy::struct_excessive_bools,
    reason = "five orthogonal binary attributes documented at field level; bitflags would obscure the per-attribute doc-comment surface"
)]
pub struct CommandProperties {
    /// How the command interacts with the `.` repeat mechanism.
    pub repeat: RepeatBehavior,
    /// Whether the command adds an entry to the jump list.
    pub jump: bool,
    /// Whether the command preserves the current visual selection.
    pub keep_visual: bool,
    /// Whether the command moves the cursor.
    pub move_point: bool,
    /// Whether the command suppresses a pending operator.
    pub suppress_operator: bool,
    /// Whether applying the command twice has the same effect as applying it once.
    pub idempotent: bool,
}

impl Default for CommandProperties {
    fn default() -> Self {
        Self {
            repeat: RepeatBehavior::Skip,
            jump: false,
            keep_visual: false,
            move_point: false,
            suppress_operator: false,
            idempotent: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_repeat_is_skip() {
        assert_eq!(RepeatBehavior::default(), RepeatBehavior::Skip);
    }

    #[test]
    fn default_command_properties() {
        let props = CommandProperties::default();
        assert_eq!(props.repeat, RepeatBehavior::Skip);
        assert!(!props.jump);
        assert!(!props.keep_visual);
        assert!(!props.move_point);
        assert!(!props.suppress_operator);
        assert!(props.idempotent);
    }

    #[test]
    fn command_properties_is_copy() {
        let props = CommandProperties::default();
        let _copy = props;
        let _original = props;
    }
}
