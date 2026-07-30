//! Vim mode tests.
//!
//! Mode and VisualType are canonically defined in `primitives`.
//! Tests for them live here alongside other state-level tests.
//!
//! # Layering
//!
//! Imports `primitives` and `std`; must not import `commands`, `effects`,
//! `execution` or `dispatch`. State modules are pure data containers with
//! no execution logic.

#[cfg(test)]
mod tests {
    use crate::primitives::{Mode, Operator, VisualType};

    #[test]
    fn test_mode_predicates() {
        assert!(Mode::Normal.is_normal());
        assert!(!Mode::Normal.is_insert());
        assert!(!Mode::Normal.is_visual());

        assert!(Mode::Insert.is_insert());
        assert!(!Mode::Insert.is_normal());

        assert!(Mode::Visual(VisualType::Char).is_visual());
        assert!(Mode::Visual(VisualType::Line).is_visual());
        assert!(Mode::Visual(VisualType::Block).is_visual());

        assert!(Mode::Replace.is_replace());
        assert!(Mode::CommandLine.is_command_line());
        assert!(Mode::OperatorPending(Operator::Delete).is_operator_pending());
    }

    #[test]
    fn test_visual_type_predicates() {
        assert!(VisualType::Char.is_char());
        assert!(!VisualType::Char.is_line());
        assert!(!VisualType::Char.is_block());

        assert!(VisualType::Line.is_line());
        assert!(VisualType::Block.is_block());
    }

    #[test]
    fn test_visual_type_extraction() {
        assert_eq!(
            Mode::Visual(VisualType::Char).visual_type(),
            Some(VisualType::Char)
        );
        assert_eq!(
            Mode::Visual(VisualType::Line).visual_type(),
            Some(VisualType::Line)
        );
        assert_eq!(Mode::Normal.visual_type(), None);
    }

    #[test]
    fn test_pending_operator_extraction() {
        assert_eq!(
            Mode::OperatorPending(Operator::Delete).pending_operator(),
            Some(Operator::Delete)
        );
        assert_eq!(Mode::Normal.pending_operator(), None);
    }

    #[test]
    fn test_short_name() {
        assert_eq!(Mode::Normal.short_name(), "NORMAL");
        assert_eq!(Mode::Insert.short_name(), "INSERT");
        assert_eq!(Mode::Visual(VisualType::Char).short_name(), "VISUAL");
        assert_eq!(Mode::Visual(VisualType::Line).short_name(), "V-LINE");
        assert_eq!(Mode::Visual(VisualType::Block).short_name(), "V-BLOCK");
        assert_eq!(Mode::Replace.short_name(), "REPLACE");
        assert_eq!(Mode::CommandLine.short_name(), "COMMAND");
        assert_eq!(
            Mode::OperatorPending(Operator::Delete).short_name(),
            "OP-PENDING"
        );
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", Mode::Normal), "Normal");
        assert_eq!(format!("{}", Mode::Visual(VisualType::Block)), "V-Block");
    }

    #[test]
    fn test_default() {
        assert_eq!(Mode::default(), Mode::Normal);
        assert_eq!(VisualType::default(), VisualType::Char);
    }
}
