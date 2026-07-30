//! Key classification for grammatical parsing.
//!
//! Classifies keys by their role in Vim's command grammar.

use strum::{Display, EnumIter};

/// Classification of a key's grammatical role.
///
/// Keys have different meanings depending on context:
/// - `w` is a Motion in Normal mode
/// - `w` is a `TextObject` after `i` or `a`
/// - `i` is a `ModeSwitch` in Normal mode
/// - `i` is a `TextObjectTrigger` after an operator
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter, Display)]
#[non_exhaustive]
pub enum KeyClass {
    // === Number prefix ===
    /// Numeric prefix (1-9, or 0 after another digit).
    Digit,

    // === Register ===
    /// Register trigger (`"` key).
    RegisterTrigger,
    /// Valid register name (a-z, 0-9, special registers).
    RegisterName,

    // === Operators ===
    /// Operator command (d, c, y, >, <, g~, gu, gU).
    Operator,

    // === Motions ===
    /// Motion command (h, j, k, l, w, b, e, $, 0, G, gg).
    Motion,
    /// Character motion (f, F, t, T) - awaits character argument.
    CharMotion,

    // === Text Objects ===
    /// Text object trigger (i, a) - after operator.
    TextObjectTrigger,
    /// Text object type (w, p, (, ", etc.) - after i/a.
    TextObject,

    // === Mode Changes ===
    /// Mode switch command (i, I, a, A, o, O, v, V, :, R).
    ModeSwitch,

    // === Actions ===
    /// Standalone action (x, X, p, P, u, U, ., J).
    Action,

    // === Prefixes ===
    /// Prefix key for compound commands (`g`, `z`, `[`, `]`).
    Prefix,

    // === Marks ===
    /// Mark trigger (m, ', `).
    MarkTrigger,

    // === Search ===
    /// Search trigger (/, ?).
    SearchTrigger,

    // === Macro ===
    /// Macro trigger (`q` for record/stop, `@` for play).
    MacroTrigger,

    // === Special ===
    /// Escape key.
    Escape,
    /// Unrecognized / unmapped key.
    Unknown,
}

impl KeyClass {
    /// Check if this class represents an operator.
    #[must_use]
    pub const fn is_operator(&self) -> bool {
        matches!(self, Self::Operator)
    }

    /// Check if this class represents a motion.
    #[must_use]
    pub const fn is_motion(&self) -> bool {
        matches!(self, Self::Motion | Self::CharMotion)
    }

    /// Check if this class represents a text object component.
    #[must_use]
    pub const fn is_text_object_related(&self) -> bool {
        matches!(self, Self::TextObjectTrigger | Self::TextObject)
    }

    /// Check if this class starts a command.
    #[must_use]
    pub const fn starts_command(&self) -> bool {
        matches!(
            self,
            Self::Digit
                | Self::RegisterTrigger
                | Self::Operator
                | Self::Motion
                | Self::CharMotion
                | Self::ModeSwitch
                | Self::Action
                | Self::Prefix
                | Self::MarkTrigger
                | Self::SearchTrigger
                | Self::MacroTrigger
        )
    }

    /// Check if this class can follow an operator.
    #[must_use]
    pub const fn follows_operator(&self) -> bool {
        matches!(
            self,
            Self::Motion
                | Self::CharMotion
                | Self::TextObjectTrigger
                | Self::Digit
                | Self::Prefix
                | Self::MarkTrigger
                | Self::SearchTrigger
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_operator() {
        assert!(KeyClass::Operator.is_operator());
        assert!(!KeyClass::Motion.is_operator());
        assert!(!KeyClass::Action.is_operator());
    }

    #[test]
    fn is_motion() {
        assert!(KeyClass::Motion.is_motion());
        assert!(KeyClass::CharMotion.is_motion());
        assert!(!KeyClass::Operator.is_motion());
    }

    #[test]
    fn is_text_object_related() {
        assert!(KeyClass::TextObjectTrigger.is_text_object_related());
        assert!(KeyClass::TextObject.is_text_object_related());
        assert!(!KeyClass::Motion.is_text_object_related());
    }

    #[test]
    fn starts_command() {
        // Should start a command
        let starters = [
            KeyClass::Digit,
            KeyClass::RegisterTrigger,
            KeyClass::Operator,
            KeyClass::Motion,
            KeyClass::CharMotion,
            KeyClass::ModeSwitch,
            KeyClass::Action,
            KeyClass::Prefix,
            KeyClass::MarkTrigger,
            KeyClass::SearchTrigger,
            KeyClass::MacroTrigger,
        ];
        for class in starters {
            assert!(class.starts_command(), "{class} should start a command");
        }

        // Should NOT start a command
        assert!(!KeyClass::TextObjectTrigger.starts_command());
        assert!(!KeyClass::TextObject.starts_command());
        assert!(!KeyClass::RegisterName.starts_command());
        assert!(!KeyClass::Escape.starts_command());
        assert!(!KeyClass::Unknown.starts_command());
    }

    #[test]
    fn follows_operator() {
        let followers = [
            KeyClass::Motion,
            KeyClass::CharMotion,
            KeyClass::TextObjectTrigger,
            KeyClass::Digit,
            KeyClass::Prefix,
            KeyClass::MarkTrigger,
            KeyClass::SearchTrigger,
        ];
        for class in followers {
            assert!(class.follows_operator(), "{class} should follow operator");
        }

        assert!(!KeyClass::Action.follows_operator());
        assert!(!KeyClass::Operator.follows_operator());
        assert!(!KeyClass::Unknown.follows_operator());
    }

    #[test]
    fn unknown_fails_all_checks() {
        assert!(!KeyClass::Unknown.is_operator());
        assert!(!KeyClass::Unknown.is_motion());
        assert!(!KeyClass::Unknown.is_text_object_related());
        assert!(!KeyClass::Unknown.starts_command());
        assert!(!KeyClass::Unknown.follows_operator());
    }
}
