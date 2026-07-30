//! Text objects (iw, aw, i(, a(, etc).
//!
//! # Layering
//!
//! Imports `primitives` and `grammar::TextObject`; must not import
//! `operators`, `effects`, `mode` or `execution`. Text objects are pure:
//! they compute ranges and return `Option<TextObjectRange>`, producing no
//! effects and no other side effects.
//!
//! ```text
//! textobjects ──► primitives (Range only)
//!    │
//!    ├──► grammar::TextObject (for dispatch)
//!    │
//!    ✗──► operators, effects (textobjects don't produce effects)
//!    ✗──► mode, execution
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
//! dispatch_textobject() ───► word::compute_word_object(...)
//!  (in dispatch/)            brackets::compute_bracket_object(...)
//!                            quotes::compute_quote_object(...)
//! ```
//!
//! # Usage
//!
//! ```ignore
//! use vim_core::dispatch::dispatch_textobject;
//! use vim_core::commands::textobjects::TextObjectContext;
//! use vim_core::grammar::types::TextObject;
//!
//! let ctx = TextObjectContext::new(text, cursor);
//! let result = dispatch_textobject(object, &ctx);
//! ```

pub mod aggregate;
pub mod argument;
pub mod brackets;
pub mod entire;
pub mod helpers;
pub mod indent;
pub mod paragraph;
pub mod quotes;
pub mod sentence;
pub mod subword;
pub mod symbol;
pub mod syntax;
pub mod tag;
mod types;
pub mod word;

// Re-exports
pub use helpers::{char_at, class_at, line_end, line_start, CharClass};
pub use types::{BracketType, TextObjectContext, TextObjectRange};
pub use word::compute_word_object;

// NOTE: compute_text_object() / dispatch_textobject() is in dispatch/textobject.rs, not here.
// This keeps commands/ as pure implementations without cross-references.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_object_context() {
        let ctx = TextObjectContext::new("hello world", 5);
        assert_eq!(ctx.text, "hello world");
        assert_eq!(ctx.cursor.get(), 5);
    }
}
