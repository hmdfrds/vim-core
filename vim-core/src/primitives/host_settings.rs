//! Snapshot of host-recommended editor settings for the current engine state.
//!
//! `HostSettings` bundles every piece of information a host needs to
//! configure its editor chrome (cursor shape, line highlight, input
//! routing, line-number style, mode indicator) into a single struct
//! so that the host can query it in one call rather than deriving each
//! value independently.

use super::cursor_style::CursorStyle;
use super::mode::{Mode, VisualType};
use super::mode_appearance::ModeAppearance;

/// Recommended editor settings derived from the current vim engine state.
///
/// Returned by [`VimEngine::host_settings()`](crate::execution::VimEngine::host_settings).
/// All fields are pure functions of the current mode and options — no
/// side-effects, no allocations.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HostSettings {
    /// Cursor shape and blink style for the current mode.
    pub cursor_style: CursorStyle,
    /// Whether the host should highlight the entire current line
    /// (true when in Visual Line mode).
    pub line_mode: bool,
    /// Whether the cursor should be clipped before the last character
    /// on the line (true in all modes except Insert, Replace, and
    /// VirtualReplace).
    pub clip_at_eol: bool,
    /// Whether the host should route raw character input to the engine
    /// (true in Insert, Replace, VirtualReplace, and CommandLine modes).
    pub input_enabled: bool,
    /// Whether relative line numbers should be displayed.
    pub relative_line_numbers: bool,
    /// Display name and optional color hints for the current mode.
    pub mode_appearance: ModeAppearance,
}

impl HostSettings {
    /// Build host settings from a mode and the `relativenumber` option.
    ///
    /// This is a pure function — no engine reference needed — so it can
    /// be used in contexts where only the mode is available.
    #[must_use]
    pub const fn from_mode(mode: Mode, relativenumber: bool) -> Self {
        let input_enabled = matches!(
            mode,
            Mode::Insert | Mode::Replace | Mode::VirtualReplace | Mode::CommandLine
        );
        Self {
            cursor_style: CursorStyle::for_mode(mode),
            line_mode: matches!(mode, Mode::Visual(VisualType::Line)),
            clip_at_eol: !input_enabled || matches!(mode, Mode::CommandLine),
            input_enabled,
            relative_line_numbers: relativenumber,
            mode_appearance: ModeAppearance::for_mode(mode),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{CursorShape, Operator};

    #[test]
    fn normal_mode_defaults() {
        let s = HostSettings::from_mode(Mode::Normal, false);
        assert_eq!(s.cursor_style.shape, CursorShape::Block);
        assert!(!s.cursor_style.blink);
        assert!(!s.line_mode);
        assert!(s.clip_at_eol);
        assert!(!s.input_enabled);
        assert!(!s.relative_line_numbers);
        assert_eq!(s.mode_appearance.name, "NORMAL");
    }

    #[test]
    fn insert_mode_enables_input() {
        let s = HostSettings::from_mode(Mode::Insert, false);
        assert_eq!(s.cursor_style.shape, CursorShape::VerticalBar);
        assert!(s.cursor_style.blink);
        assert!(!s.line_mode);
        assert!(!s.clip_at_eol);
        assert!(s.input_enabled);
        assert_eq!(s.mode_appearance.name, "INSERT");
    }

    #[test]
    fn replace_mode_enables_input() {
        let s = HostSettings::from_mode(Mode::Replace, false);
        assert_eq!(s.cursor_style.shape, CursorShape::HorizontalBar);
        assert!(!s.clip_at_eol);
        assert!(s.input_enabled);
    }

    #[test]
    fn virtual_replace_mode_enables_input() {
        let s = HostSettings::from_mode(Mode::VirtualReplace, false);
        assert_eq!(s.cursor_style.shape, CursorShape::HorizontalBar);
        assert!(!s.clip_at_eol);
        assert!(s.input_enabled);
    }

    #[test]
    fn command_line_mode_input_but_clips() {
        let s = HostSettings::from_mode(Mode::CommandLine, false);
        assert!(s.input_enabled);
        // CommandLine accepts text input but the *document* cursor clips at EOL.
        assert!(s.clip_at_eol);
        assert_eq!(s.mode_appearance.name, "COMMAND");
    }

    #[test]
    fn visual_line_sets_line_mode() {
        let s = HostSettings::from_mode(Mode::Visual(VisualType::Line), false);
        assert!(s.line_mode);
        assert!(s.clip_at_eol);
        assert!(!s.input_enabled);
    }

    #[test]
    fn visual_char_no_line_mode() {
        let s = HostSettings::from_mode(Mode::Visual(VisualType::Char), false);
        assert!(!s.line_mode);
    }

    #[test]
    fn visual_block_no_line_mode() {
        let s = HostSettings::from_mode(Mode::Visual(VisualType::Block), false);
        assert!(!s.line_mode);
    }

    #[test]
    fn relativenumber_propagates() {
        let s = HostSettings::from_mode(Mode::Normal, true);
        assert!(s.relative_line_numbers);

        let s = HostSettings::from_mode(Mode::Normal, false);
        assert!(!s.relative_line_numbers);
    }

    #[test]
    fn operator_pending_clips_no_input() {
        let s = HostSettings::from_mode(Mode::OperatorPending(Operator::Delete), false);
        assert!(s.clip_at_eol);
        assert!(!s.input_enabled);
        // Delete is a modification operator → HalfBlock cursor
        assert_eq!(s.cursor_style.shape, CursorShape::HalfBlock);
    }

    #[test]
    fn select_mode_clips_no_input() {
        let s = HostSettings::from_mode(Mode::Select(VisualType::Char), false);
        assert!(s.clip_at_eol);
        assert!(!s.input_enabled);
    }
}
