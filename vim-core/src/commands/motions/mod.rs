//! Motions module.
//!
//! Cursor movement implementations for all Vim motions.
//!
//! # Layering
//!
//! Imports `primitives`, `grammar`, `state`, `effects` and `document`; must
//! not import `operators`, `mode` or `execution`.
//!
//! Motion functions are pure: they compute positions and return
//! `MotionResult`. The one exception is `find::find_with_tracking`, a
//! commands-layer bridge that wraps a pure find motion with `SetLastFind` +
//! `SetCursor` effects. It lives here for proximity to the find motions it
//! delegates to, and is called exclusively from `dispatch/find.rs`.
//!
//! ```text
//! motions ──► primitives
//!    │
//!    ├──► grammar::Motion (enum definition only)
//!    ├──► effects (find_with_tracking bridge only)
//!    │
//!    ✗──► operators
//!    ✗──► mode, execution
//! ```
//!
//! # Architecture
//!
//! Plain functions, not trait methods. Each motion is a standalone
//! function called directly from dispatch.
//!
//! ```text
//! motions/
//! ├── mod.rs         (this file - exports)
//! ├── types.rs       (MotionResult, MotionInclusivity, MotionContext)
//! ├── char.rs        (h, l, 0, $, ^, g_)
//! ├── line.rs        (j, k, +, -, gj, gk)
//! ├── word.rs        (w, b, e, W, B, E, ge, gE)
//! ├── document.rs    (gg, G, %, H, M, L)
//! ├── find.rs        (f, F, t, T, ;, ,)
//! ├── search.rs      (/, ?, n, N, *, #)
//! ├── paragraph.rs   ({, })
//! ├── sentence.rs    ((, ))
//! ├── bracket.rs     (%, [{, ]})
//! ├── mark.rs        (', `)
//! ├── scroll.rs      (Ctrl-D/U/F/B, zz/zt/zb)
//! └── misc.rs        (|, gm, go)
//! ```

// Motion modules containing plain functions
pub mod boundary;
pub mod bracket;
pub mod char;
pub mod document;
pub mod find;
pub mod indent;
pub mod line;
pub mod mark;
pub mod misc;
pub mod paragraph;
pub mod search;
mod search_ci;
mod search_modifiers;
pub mod search_object;
mod search_regex;
pub mod section;
pub mod seek_textobject;
pub mod sentence;
pub mod subword;
pub mod types;
pub mod word;
pub mod word_boundary;

// Re-export core types
pub use crate::primitives::MotionInclusivity;
pub use find::{FindDirection, LastFind};
pub use types::{
    MiscMotion, MotionContext, MotionEffectsContext, MotionResult, SearchMotion, ViewportInfo,
    WordCharProvider,
};
pub use word_boundary::{word_under_cursor, word_under_cursor_or_next};
