//! Ex commands (:s, :g, :d, etc).
//!
//! # Layering
//!
//! Imports `primitives`, `effects` and `grammar`; must not import `mode` or
//! `execution`. Ex commands produce `Effects` and know nothing about mode
//! handlers.
//!
//! ```text
//! ex ──► effects ──► primitives
//!    │
//!    ├──► grammar (for types)
//!    │
//!    ✗──► mode, execution
//! ```
//!
//! # Structure
//!
//! - `types.rs`: ExContext, ExResult
//! - `range.rs`: Range resolution (LineSpec → line number)
//! - `line_ops.rs`: :d, :y
//! - `substitute.rs`: :s/pat/rep/flags
//! - `global.rs`: :g/pat/cmd, :v/pat/cmd
//! - `sort.rs`: :sort, :m, :t

pub mod abbreviation;
pub mod completion;
pub mod display;
pub mod effects;
pub mod global;
pub mod line_ops;
pub mod range;
pub mod shell;
pub mod sort;
pub mod substitute;
pub mod text_ops;
pub mod types;

pub mod structural;

pub use display::{print_lines, z_window};
pub use global::global;
pub use line_ops::{delete, goto_line, join, yank};
pub use range::resolve_range;
pub use shell::nohighlight;
pub use sort::{copy_lines, move_lines, sort};
pub use substitute::{compute_preview_matches, substitute};
pub use text_ops::{center, left, put, retab, right};
pub use types::{ExContext, ExResult, ResolvedRange};
