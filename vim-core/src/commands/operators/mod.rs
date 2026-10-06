//! Operators (d, c, y, >, <, gu, gU, g~, gq).
//!
//! Operators act on a range of text (from motion or text object).
//!
//! # Layering
//!
//! Imports `primitives`, `effects` and `grammar` (Motion enum only); must
//! not import `mode` or `execution`. Operators produce `Effects` and know
//! nothing about mode handlers.
//!
//! ```text
//! operators ──► effects ──► primitives
//!    │
//!    ├──► grammar::Motion (read-only for inclusivity)
//!    │
//!    ✗──► mode
//! ```
//!
//! # Architecture
//!
//! No dyn traits in the hot path:
//! - We use **enum dispatch** not trait objects
//! - Plain functions, not trait methods
//! - Exhaustive match in **dispatch layer** for compiler safety
//!
//! ```text
//! dispatch_operator() ───► delete::execute(ctx, ...)
//!  (in dispatch/)          change::execute(ctx, ...)
//!                          yank::execute(ctx, ...)
//! ```
//!
//! # Usage
//!
//! ```ignore
//! use vim_core::dispatch::dispatch_operator;
//! use vim_core::commands::operators::OperatorContext;
//! use vim_core::commands::Operator;
//!
//! let ctx = OperatorContext::new(text, range, motion_type, register, count, cursor);
//! let result = dispatch_operator(Operator::Delete, &ctx);
//! // result.effects is Effects (SmallVec-backed, #[must_use])
//! ```

pub mod block_visual;
pub mod case;
pub mod change;
pub mod delete;
pub mod format;
mod format_cancel;
mod format_legacy;
pub mod inclusivity;
pub mod indent;
pub mod range;
pub mod range_helpers;
pub mod registers;
pub mod surround;
pub mod types;
pub mod yank;

// Re-exports for convenience
pub use crate::primitives::MotionInclusivity;
pub use inclusivity::{motion_inclusivity, motion_inclusivity_with_find};
pub use range::{compute_linewise_range, compute_motion_range, RangeResult};
pub use range_helpers::{adjust_eof_range, empty_textobject_result, maybe_promote_to_linewise};
pub use types::{cursor_after_delete, extract_range_text, is_full_line_delete};
pub use types::{
    CaseTransform, OperatorContext, OperatorMotionInput, OperatorOrigin, SelectionOperatorContext,
    ShiftDirection,
};

// NOTE: execute_operator() is in dispatch/operator.rs, not here.
// This keeps commands/ as pure implementations without cross-references.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{MotionType, Offset, Range};

    #[test]
    fn test_operator_context() {
        let ctx = OperatorContext::new(
            "hello world",
            Range::from_raw(0, 5),
            MotionType::CharWise,
            None,
            1,
            Offset::new(0),
        );

        assert_eq!(ctx.text, "hello world");
        assert_eq!(ctx.range.start().get(), 0);
        assert_eq!(ctx.range.end().get(), 5);
    }
}
