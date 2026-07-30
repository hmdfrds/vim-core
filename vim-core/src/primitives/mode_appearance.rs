//! Per-mode theme color suggestions.
//!
//! `ModeAppearance` provides a display name and optional foreground/background
//! color hints for each editing mode. Hosts can use these as defaults when no
//! user theme override is configured.

use compact_str::CompactString;

use super::mode::{Mode, VisualType};

/// Theme color suggestion for a vim editing mode.
///
/// The `fg` and `bg` fields are optional RGB triples. Hosts should treat them
/// as *suggestions* — user theme overrides take precedence.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ModeAppearance {
    /// Short display name (e.g. "NORMAL", "INSERT").
    pub name: CompactString,
    /// Optional foreground color `[r, g, b]`.
    pub fg: Option<[u8; 3]>,
    /// Optional background color `[r, g, b]`.
    pub bg: Option<[u8; 3]>,
}

impl ModeAppearance {
    /// Compute the default appearance for a given mode.
    #[must_use]
    pub const fn for_mode(mode: Mode) -> Self {
        match mode {
            Mode::Normal => Self {
                name: CompactString::new_inline("NORMAL"),
                fg: None,
                bg: None,
            },
            Mode::Insert => Self {
                name: CompactString::new_inline("INSERT"),
                fg: None,
                bg: Some([80, 161, 79]), // green-ish
            },
            Mode::Visual(VisualType::Char) => Self {
                name: CompactString::new_inline("VISUAL"),
                fg: None,
                bg: Some([64, 120, 199]), // blue-ish
            },
            Mode::Visual(VisualType::Line) => Self {
                name: CompactString::new_inline("V-LINE"),
                fg: None,
                bg: Some([64, 120, 199]),
            },
            Mode::Visual(VisualType::Block) => Self {
                name: CompactString::new_inline("V-BLOCK"),
                fg: None,
                bg: Some([64, 120, 199]),
            },
            Mode::Select(VisualType::Char) => Self {
                name: CompactString::new_inline("SELECT"),
                fg: None,
                bg: Some([64, 120, 199]),
            },
            Mode::Select(VisualType::Line) => Self {
                name: CompactString::new_inline("S-LINE"),
                fg: None,
                bg: Some([64, 120, 199]),
            },
            Mode::Select(VisualType::Block) => Self {
                name: CompactString::new_inline("S-BLOCK"),
                fg: None,
                bg: Some([64, 120, 199]),
            },
            Mode::Replace | Mode::VirtualReplace => Self {
                name: CompactString::new_inline("REPLACE"),
                fg: None,
                bg: Some([204, 51, 51]), // red-ish
            },
            Mode::CommandLine => Self {
                name: CompactString::new_inline("COMMAND"),
                fg: None,
                bg: None,
            },
            Mode::OperatorPending(_) => Self {
                name: CompactString::new_inline("OP-PENDING"),
                fg: None,
                bg: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Operator;

    #[test]
    fn normal_mode_name() {
        let a = ModeAppearance::for_mode(Mode::Normal);
        assert_eq!(a.name, "NORMAL");
        assert_eq!(a.fg, None);
        assert_eq!(a.bg, None);
    }

    #[test]
    fn insert_mode_has_green_bg() {
        let a = ModeAppearance::for_mode(Mode::Insert);
        assert_eq!(a.name, "INSERT");
        assert!(a.bg.is_some(), "Insert should have a background color");
    }

    #[test]
    fn visual_char_mode_name() {
        let a = ModeAppearance::for_mode(Mode::Visual(VisualType::Char));
        assert_eq!(a.name, "VISUAL");
        assert!(a.bg.is_some());
    }

    #[test]
    fn visual_line_mode_name() {
        let a = ModeAppearance::for_mode(Mode::Visual(VisualType::Line));
        assert_eq!(a.name, "V-LINE");
    }

    #[test]
    fn visual_block_mode_name() {
        let a = ModeAppearance::for_mode(Mode::Visual(VisualType::Block));
        assert_eq!(a.name, "V-BLOCK");
    }

    #[test]
    fn select_char_mode_name() {
        let a = ModeAppearance::for_mode(Mode::Select(VisualType::Char));
        assert_eq!(a.name, "SELECT");
    }

    #[test]
    fn select_line_mode_name() {
        let a = ModeAppearance::for_mode(Mode::Select(VisualType::Line));
        assert_eq!(a.name, "S-LINE");
    }

    #[test]
    fn select_block_mode_name() {
        let a = ModeAppearance::for_mode(Mode::Select(VisualType::Block));
        assert_eq!(a.name, "S-BLOCK");
    }

    #[test]
    fn replace_mode_has_red_bg() {
        let a = ModeAppearance::for_mode(Mode::Replace);
        assert_eq!(a.name, "REPLACE");
        assert!(a.bg.is_some(), "Replace should have a background color");
    }

    #[test]
    fn virtual_replace_matches_replace() {
        let a = ModeAppearance::for_mode(Mode::VirtualReplace);
        assert_eq!(a.name, "REPLACE");
        assert!(a.bg.is_some());
    }

    #[test]
    fn command_line_mode_name() {
        let a = ModeAppearance::for_mode(Mode::CommandLine);
        assert_eq!(a.name, "COMMAND");
        assert_eq!(a.bg, None);
    }

    #[test]
    fn operator_pending_mode_name() {
        let a = ModeAppearance::for_mode(Mode::OperatorPending(Operator::Delete));
        assert_eq!(a.name, "OP-PENDING");
        assert_eq!(a.bg, None);
    }

    #[test]
    fn names_match_mode_short_name() {
        // ModeAppearance names should agree with Mode::short_name()
        let modes = [
            Mode::Normal,
            Mode::Insert,
            Mode::Visual(VisualType::Char),
            Mode::Visual(VisualType::Line),
            Mode::Visual(VisualType::Block),
            Mode::Select(VisualType::Char),
            Mode::Select(VisualType::Line),
            Mode::Select(VisualType::Block),
            Mode::Replace,
            Mode::CommandLine,
            Mode::OperatorPending(Operator::Delete),
        ];
        for mode in modes {
            let appearance = ModeAppearance::for_mode(mode);
            assert_eq!(
                appearance.name,
                mode.short_name(),
                "ModeAppearance name for {mode:?} should match Mode::short_name()"
            );
        }
    }
}
