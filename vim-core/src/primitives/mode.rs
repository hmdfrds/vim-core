//! Vim editing mode — pure domain enums.
//!
//! `Mode` and `VisualType` live at the `primitives` layer because they
//! have no dependencies beyond other primitives (`Operator`) and are
//! consumed by every layer (state, grammar, effects, commands, execution).

use super::Operator;
use smart_default::SmartDefault;

/// Visual mode type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SmartDefault)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum VisualType {
    /// Character-wise visual mode (`v`).
    #[default]
    Char,
    /// Line-wise visual mode (`V`).
    Line,
    /// Block visual mode (`Ctrl-V`).
    Block,
}

impl VisualType {
    /// All variants, in declaration order.
    pub const ALL: [Self; 3] = [Self::Char, Self::Line, Self::Block];

    /// Check if character-wise.
    #[inline]
    #[must_use]
    pub const fn is_char(self) -> bool {
        matches!(self, Self::Char)
    }

    /// Check if line-wise.
    #[inline]
    #[must_use]
    pub const fn is_line(self) -> bool {
        matches!(self, Self::Line)
    }

    /// Check if block-wise.
    #[inline]
    #[must_use]
    pub const fn is_block(self) -> bool {
        matches!(self, Self::Block)
    }
}

/// Discriminator for [`Mode`] — strips payload variants to a plain enum.
///
/// Follows the `Effect → EffectKind` pattern. Use this when you need to match
/// on mode category without caring about the inner `VisualType` or `Operator`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ModeKind {
    /// Normal mode — command mode.
    Normal,
    /// Insert mode — text input.
    Insert,
    /// Visual mode — selection active.
    Visual,
    /// Select mode — GUI-like selection (typing replaces selection).
    Select,
    /// Replace mode — overwrite characters.
    Replace,
    /// Virtual replace mode (`gR`) — overwrite by screen columns, not bytes.
    VirtualReplace,
    /// Command-line mode — `:` command entry.
    CommandLine,
    /// Operator-pending mode — waiting for motion/text-object.
    OperatorPending,
}

impl ModeKind {
    /// All variants in declaration order.
    pub const ALL: [Self; 8] = [
        Self::Normal,
        Self::Insert,
        Self::Visual,
        Self::Select,
        Self::Replace,
        Self::VirtualReplace,
        Self::CommandLine,
        Self::OperatorPending,
    ];
}

/// Vim editing mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SmartDefault)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Mode {
    /// Normal mode - command mode.
    #[default]
    Normal,
    /// Insert mode - text input.
    Insert,
    /// Visual mode - selection active.
    Visual(VisualType),
    /// Select mode — GUI-like selection (typing replaces selection).
    Select(VisualType),
    /// Replace mode - overwrite characters.
    Replace,
    /// Virtual replace mode (gR) — overwrite by screen columns, not bytes.
    VirtualReplace,
    /// Command-line mode - `:` command entry.
    CommandLine,
    /// Operator-pending mode - waiting for motion/text-object.
    OperatorPending(Operator),
}

impl Mode {
    /// Check if this is Normal mode.
    #[inline]
    #[must_use]
    pub const fn is_normal(self) -> bool {
        matches!(self, Self::Normal)
    }

    /// Check if this is Insert mode.
    #[inline]
    #[must_use]
    pub const fn is_insert(self) -> bool {
        matches!(self, Self::Insert)
    }

    /// Check if this is any Visual mode.
    #[inline]
    #[must_use]
    pub const fn is_visual(self) -> bool {
        matches!(self, Self::Visual(_))
    }

    /// Check if this is any Select mode.
    #[inline]
    #[must_use]
    pub const fn is_select(self) -> bool {
        matches!(self, Self::Select(_))
    }

    /// Check if this is any Visual or Select mode (both maintain a selection).
    #[inline]
    #[must_use]
    pub const fn is_visual_or_select(self) -> bool {
        matches!(self, Self::Visual(_) | Self::Select(_))
    }

    /// Check if this is Replace or Virtual Replace mode.
    #[inline]
    #[must_use]
    pub const fn is_replace(self) -> bool {
        matches!(self, Self::Replace | Self::VirtualReplace)
    }

    /// Check if this is `CommandLine` mode.
    #[inline]
    #[must_use]
    pub const fn is_command_line(self) -> bool {
        matches!(self, Self::CommandLine)
    }

    /// Check if this is `OperatorPending` mode.
    #[inline]
    #[must_use]
    pub const fn is_operator_pending(self) -> bool {
        matches!(self, Self::OperatorPending(_))
    }

    /// Get the visual type if in visual mode.
    #[inline]
    #[must_use]
    pub const fn visual_type(self) -> Option<VisualType> {
        match self {
            Self::Visual(vt) => Some(vt),
            _ => None,
        }
    }

    /// Get the select type if in select mode.
    #[inline]
    #[must_use]
    pub const fn select_type(self) -> Option<VisualType> {
        match self {
            Self::Select(vt) => Some(vt),
            _ => None,
        }
    }

    /// Get the pending operator if in operator-pending mode.
    #[inline]
    #[must_use]
    pub const fn pending_operator(self) -> Option<Operator> {
        match self {
            Self::OperatorPending(op) => Some(op),
            _ => None,
        }
    }

    /// Return the [`ModeKind`] discriminator for this mode, stripping any payload.
    #[inline]
    #[must_use]
    pub const fn kind(&self) -> ModeKind {
        match self {
            Mode::Normal => ModeKind::Normal,
            Mode::Insert => ModeKind::Insert,
            Mode::Visual(_) => ModeKind::Visual,
            Mode::Select(_) => ModeKind::Select,
            Mode::Replace => ModeKind::Replace,
            Mode::VirtualReplace => ModeKind::VirtualReplace,
            Mode::CommandLine => ModeKind::CommandLine,
            Mode::OperatorPending(_) => ModeKind::OperatorPending,
        }
    }

    /// Short name for display.
    #[must_use]
    pub const fn short_name(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
            Self::Visual(VisualType::Char) => "VISUAL",
            Self::Visual(VisualType::Line) => "V-LINE",
            Self::Visual(VisualType::Block) => "V-BLOCK",
            Self::Select(VisualType::Char) => "SELECT",
            Self::Select(VisualType::Line) => "S-LINE",
            Self::Select(VisualType::Block) => "S-BLOCK",
            Self::Replace => "REPLACE",
            Self::VirtualReplace => "V-REPLACE",
            Self::CommandLine => "COMMAND",
            Self::OperatorPending(_) => "OP-PENDING",
        }
    }

    /// Canonical single-character mode string, matching Vim's `mode()` function.
    #[must_use]
    pub const fn vim_string(self) -> &'static str {
        match self {
            Self::Normal => "n",
            Self::Insert => "i",
            Self::Visual(VisualType::Char) => "v",
            Self::Visual(VisualType::Line) => "V",
            Self::Visual(VisualType::Block) => "\x16",
            Self::Select(VisualType::Char) => "s",
            Self::Select(VisualType::Line) => "S",
            Self::Select(VisualType::Block) => "\x13",
            Self::Replace => "R",
            Self::VirtualReplace => "Rv",
            Self::CommandLine => "c",
            Self::OperatorPending(_) => "no",
        }
    }

    /// Human-readable display name for status bars.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
            Self::Visual(VisualType::Char) => "VISUAL",
            Self::Visual(VisualType::Line) => "V-LINE",
            Self::Visual(VisualType::Block) => "V-BLOCK",
            Self::Select(VisualType::Char) => "SELECT",
            Self::Select(VisualType::Line) => "S-LINE",
            Self::Select(VisualType::Block) => "S-BLOCK",
            Self::Replace | Self::VirtualReplace => "REPLACE",
            Self::CommandLine => "COMMAND",
            Self::OperatorPending(_) => "OP-PENDING",
        }
    }
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normal => write!(f, "Normal"),
            Self::Insert => write!(f, "Insert"),
            Self::Visual(VisualType::Char) => write!(f, "Visual"),
            Self::Visual(VisualType::Line) => write!(f, "V-Line"),
            Self::Visual(VisualType::Block) => write!(f, "V-Block"),
            Self::Select(VisualType::Char) => write!(f, "Select"),
            Self::Select(VisualType::Line) => write!(f, "S-Line"),
            Self::Select(VisualType::Block) => write!(f, "S-Block"),
            Self::Replace => write!(f, "Replace"),
            Self::VirtualReplace => write!(f, "V-Replace"),
            Self::CommandLine => write!(f, "Command"),
            Self::OperatorPending(_) => write!(f, "Op-Pending"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_select_true_for_select_variants() {
        assert!(Mode::Select(VisualType::Char).is_select());
        assert!(Mode::Select(VisualType::Line).is_select());
        assert!(Mode::Select(VisualType::Block).is_select());
    }

    #[test]
    fn is_select_false_for_non_select() {
        assert!(!Mode::Normal.is_select());
        assert!(!Mode::Insert.is_select());
        assert!(!Mode::Visual(VisualType::Char).is_select());
        assert!(!Mode::Replace.is_select());
        assert!(!Mode::VirtualReplace.is_select());
        assert!(!Mode::CommandLine.is_select());
    }

    #[test]
    fn is_visual_or_select_true_for_both() {
        assert!(Mode::Visual(VisualType::Char).is_visual_or_select());
        assert!(Mode::Visual(VisualType::Line).is_visual_or_select());
        assert!(Mode::Visual(VisualType::Block).is_visual_or_select());
        assert!(Mode::Select(VisualType::Char).is_visual_or_select());
        assert!(Mode::Select(VisualType::Line).is_visual_or_select());
        assert!(Mode::Select(VisualType::Block).is_visual_or_select());
    }

    #[test]
    fn is_visual_or_select_false_for_others() {
        assert!(!Mode::Normal.is_visual_or_select());
        assert!(!Mode::Insert.is_visual_or_select());
        assert!(!Mode::Replace.is_visual_or_select());
        assert!(!Mode::VirtualReplace.is_visual_or_select());
        assert!(!Mode::CommandLine.is_visual_or_select());
    }

    #[test]
    fn select_type_returns_some_for_select() {
        assert_eq!(
            Mode::Select(VisualType::Char).select_type(),
            Some(VisualType::Char)
        );
        assert_eq!(
            Mode::Select(VisualType::Line).select_type(),
            Some(VisualType::Line)
        );
        assert_eq!(
            Mode::Select(VisualType::Block).select_type(),
            Some(VisualType::Block)
        );
    }

    #[test]
    fn select_type_returns_none_for_others() {
        assert_eq!(Mode::Normal.select_type(), None);
        assert_eq!(Mode::Insert.select_type(), None);
        assert_eq!(Mode::Visual(VisualType::Char).select_type(), None);
        assert_eq!(Mode::Replace.select_type(), None);
    }

    #[test]
    fn short_name_select_variants() {
        assert_eq!(Mode::Select(VisualType::Char).short_name(), "SELECT");
        assert_eq!(Mode::Select(VisualType::Line).short_name(), "S-LINE");
        assert_eq!(Mode::Select(VisualType::Block).short_name(), "S-BLOCK");
    }

    #[test]
    fn display_select_variants() {
        assert_eq!(format!("{}", Mode::Select(VisualType::Char)), "Select");
        assert_eq!(format!("{}", Mode::Select(VisualType::Line)), "S-Line");
        assert_eq!(format!("{}", Mode::Select(VisualType::Block)), "S-Block");
    }

    #[test]
    fn display_normal() {
        assert_eq!(format!("{}", Mode::Normal), "Normal");
    }

    #[test]
    fn display_insert() {
        assert_eq!(format!("{}", Mode::Insert), "Insert");
    }

    #[test]
    fn display_visual_char() {
        assert_eq!(format!("{}", Mode::Visual(VisualType::Char)), "Visual");
    }

    #[test]
    fn display_visual_line() {
        assert_eq!(format!("{}", Mode::Visual(VisualType::Line)), "V-Line");
    }

    #[test]
    fn display_visual_block() {
        assert_eq!(format!("{}", Mode::Visual(VisualType::Block)), "V-Block");
    }

    #[test]
    fn display_replace() {
        assert_eq!(format!("{}", Mode::Replace), "Replace");
    }

    #[test]
    fn display_virtual_replace() {
        assert_eq!(format!("{}", Mode::VirtualReplace), "V-Replace");
    }

    #[test]
    fn display_command_line() {
        assert_eq!(format!("{}", Mode::CommandLine), "Command");
    }

    #[test]
    fn display_operator_pending() {
        assert_eq!(
            format!("{}", Mode::OperatorPending(Operator::Delete)),
            "Op-Pending"
        );
        assert_eq!(
            format!("{}", Mode::OperatorPending(Operator::Yank)),
            "Op-Pending"
        );
    }

    // ── vim_string tests ──────────────────────────────────────────────

    #[test]
    fn vim_string_normal() {
        assert_eq!(Mode::Normal.vim_string(), "n");
    }

    #[test]
    fn vim_string_insert() {
        assert_eq!(Mode::Insert.vim_string(), "i");
    }

    #[test]
    fn vim_string_visual_char() {
        assert_eq!(Mode::Visual(VisualType::Char).vim_string(), "v");
    }

    #[test]
    fn vim_string_visual_line() {
        assert_eq!(Mode::Visual(VisualType::Line).vim_string(), "V");
    }

    #[test]
    fn vim_string_visual_block() {
        // Ctrl-V = 0x16
        assert_eq!(Mode::Visual(VisualType::Block).vim_string(), "\x16");
    }

    #[test]
    fn vim_string_select_char() {
        assert_eq!(Mode::Select(VisualType::Char).vim_string(), "s");
    }

    #[test]
    fn vim_string_select_line() {
        assert_eq!(Mode::Select(VisualType::Line).vim_string(), "S");
    }

    #[test]
    fn vim_string_select_block() {
        // Ctrl-S = 0x13
        assert_eq!(Mode::Select(VisualType::Block).vim_string(), "\x13");
    }

    #[test]
    fn vim_string_replace() {
        assert_eq!(Mode::Replace.vim_string(), "R");
    }

    #[test]
    fn vim_string_virtual_replace() {
        assert_eq!(Mode::VirtualReplace.vim_string(), "Rv");
    }

    #[test]
    fn vim_string_command_line() {
        assert_eq!(Mode::CommandLine.vim_string(), "c");
    }

    #[test]
    fn vim_string_operator_pending() {
        assert_eq!(Mode::OperatorPending(Operator::Delete).vim_string(), "no");
        assert_eq!(Mode::OperatorPending(Operator::Yank).vim_string(), "no");
    }

    // ── display_name tests ───────────────────────────────────────────

    #[test]
    fn display_name_all_modes() {
        assert_eq!(Mode::Normal.display_name(), "NORMAL");
        assert_eq!(Mode::Insert.display_name(), "INSERT");
        assert_eq!(Mode::Visual(VisualType::Char).display_name(), "VISUAL");
        assert_eq!(Mode::Visual(VisualType::Line).display_name(), "V-LINE");
        assert_eq!(Mode::Visual(VisualType::Block).display_name(), "V-BLOCK");
        assert_eq!(Mode::Select(VisualType::Char).display_name(), "SELECT");
        assert_eq!(Mode::Select(VisualType::Line).display_name(), "S-LINE");
        assert_eq!(Mode::Select(VisualType::Block).display_name(), "S-BLOCK");
        assert_eq!(Mode::Replace.display_name(), "REPLACE");
        assert_eq!(Mode::VirtualReplace.display_name(), "REPLACE");
        assert_eq!(Mode::CommandLine.display_name(), "COMMAND");
        assert_eq!(
            Mode::OperatorPending(Operator::Delete).display_name(),
            "OP-PENDING"
        );
    }

    #[test]
    fn display_name_replace_and_virtual_replace_are_same() {
        assert_eq!(
            Mode::Replace.display_name(),
            Mode::VirtualReplace.display_name()
        );
    }

    #[test]
    fn visual_type_all_no_duplicates() {
        use std::collections::HashSet;
        let unique: HashSet<VisualType> = VisualType::ALL.iter().copied().collect();
        assert_eq!(
            unique.len(),
            VisualType::ALL.len(),
            "Duplicate in VisualType::ALL"
        );
    }

    #[test]
    fn mode_kind_all_no_duplicates() {
        use std::collections::HashSet;
        let unique: HashSet<ModeKind> = ModeKind::ALL.iter().copied().collect();
        assert_eq!(
            unique.len(),
            ModeKind::ALL.len(),
            "Duplicate in ModeKind::ALL"
        );
    }

    #[test]
    fn mode_kind_covers_every_mode_variant() {
        use std::collections::HashSet;
        let modes = [
            Mode::Normal,
            Mode::Insert,
            Mode::Visual(VisualType::Char),
            Mode::Select(VisualType::Char),
            Mode::Replace,
            Mode::VirtualReplace,
            Mode::CommandLine,
            Mode::OperatorPending(Operator::Delete),
        ];
        let kinds: HashSet<ModeKind> = modes.iter().map(|m| m.kind()).collect();
        let all: HashSet<ModeKind> = ModeKind::ALL.iter().copied().collect();
        let missing: Vec<_> = all.difference(&kinds).collect();
        assert!(
            missing.is_empty(),
            "ModeKind variants not covered by Mode::kind(): {:?}",
            missing
        );
    }

    #[test]
    fn display_all_variants_exhaustive() {
        // Verify every variant produces a non-empty Display string.
        let all_modes = [
            Mode::Normal,
            Mode::Insert,
            Mode::Visual(VisualType::Char),
            Mode::Visual(VisualType::Line),
            Mode::Visual(VisualType::Block),
            Mode::Select(VisualType::Char),
            Mode::Select(VisualType::Line),
            Mode::Select(VisualType::Block),
            Mode::Replace,
            Mode::VirtualReplace,
            Mode::CommandLine,
            Mode::OperatorPending(Operator::Delete),
        ];
        for mode in &all_modes {
            let display = format!("{mode}");
            assert!(
                !display.is_empty(),
                "Display for {mode:?} must not be empty"
            );
            // Title-case: first char is uppercase.
            assert!(
                display.starts_with(|c: char| c.is_uppercase()),
                "Display for {mode:?} should start with uppercase, got {display:?}"
            );
        }
    }
}
