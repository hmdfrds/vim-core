//! Visual mode state tests.
//!
//! LastVisualInfo is canonically defined in `primitives`.
//! Tests for them live here alongside other state-level tests.

#[cfg(test)]
mod tests {
    use crate::primitives::{LastVisualInfo, VisualType};

    #[test]
    fn test_char_wise_creation() {
        let info = LastVisualInfo::char_wise(5);
        assert_eq!(info.visual_type(), VisualType::Char);
        assert_eq!(info.lines(), 5);
        assert_eq!(info.columns(), 0);
    }

    #[test]
    fn test_line_wise_creation() {
        let info = LastVisualInfo::line_wise(10);
        assert_eq!(info.visual_type(), VisualType::Line);
        assert_eq!(info.lines(), 10);
        assert_eq!(info.columns(), 0, "line-wise should not track columns");
    }

    #[test]
    fn test_block_wise_creation() {
        let info = LastVisualInfo::block_wise(3, 10);
        assert_eq!(info.visual_type(), VisualType::Block);
        assert_eq!(info.lines(), 3);
        assert_eq!(info.columns(), 10);
    }

    #[test]
    fn test_new_with_custom_params() {
        let info = LastVisualInfo::new(VisualType::Line, 42, 7);
        assert_eq!(info.visual_type(), VisualType::Line);
        assert_eq!(info.lines(), 42);
        assert_eq!(info.columns(), 7);
    }

    /// Edge case: single-line visual selection
    #[test]
    fn test_single_line_selection() {
        let info = LastVisualInfo::char_wise(1);
        assert_eq!(info.lines(), 1);

        let info = LastVisualInfo::line_wise(1);
        assert_eq!(info.lines(), 1);

        let info = LastVisualInfo::block_wise(1, 1);
        assert_eq!(info.lines(), 1);
        assert_eq!(info.columns(), 1);
    }

    /// Each convenience constructor must produce the correct VisualType
    #[test]
    fn test_constructors_produce_distinct_types() {
        let c = LastVisualInfo::char_wise(1);
        let l = LastVisualInfo::line_wise(1);
        let b = LastVisualInfo::block_wise(1, 1);

        assert!(c.visual_type().is_char());
        assert!(l.visual_type().is_line());
        assert!(b.visual_type().is_block());
        assert_ne!(c, l);
        assert_ne!(l, b);
    }
}
