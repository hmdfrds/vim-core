//! Actions (x, p, u, ., etc).
//!
//! # Layering
//!
//! Imports `primitives`, `effects`, `grammar` and `state`; must not import
//! `mode` or `execution`. Actions produce `Effects` and know nothing about
//! mode handlers.
//!
//! ```text
//! actions ──► effects ──► primitives
//!    │
//!    ├──► grammar (enums only)
//!    ├──► state (for RegisterContent, Mark, etc.)
//!    │
//!    ✗──► mode, execution
//! ```

pub mod case;
pub mod delete_char;
pub mod delete_to_end;
pub mod effects;
pub mod indent;
pub mod info;
pub mod intent_repeat;
pub mod join;
pub mod jump;
pub mod mark;
pub mod number;
pub mod put;
pub mod replace_char;
pub mod substitute;
pub mod types;
pub mod undo;
pub mod visual_block;
pub mod visual_put;

// Re-exports for convenience
pub use case::{execute_toggle_case_char, lowercase_range, toggle_case_range, uppercase_range};
pub use indent::{
    execute_indent_lines, execute_outdent_lines, indent_lines_raw, outdent_lines_raw,
};
pub use join::{execute_join, execute_join_no_space};
pub use number::{execute_decrement_number, execute_increment_number};
pub use put::{put_after, put_before};

pub use types::{ActionContext, BlockGeometry, MarkContext, ReplaceCharContext};

// NOTE: dispatch_action() is in dispatch/action.rs, not here.
// This keeps commands/ as pure implementations without cross-references.
