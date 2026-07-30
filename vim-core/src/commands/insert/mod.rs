//! Insert mode commands.
//!
//! Handles all insert mode operations (entry, exit, deletion).
//!
//! # Layering
//!
//! Imports `primitives`, `effects`, `state`, `grammar` and `document`; must
//! not import `execution`, `dispatch`, `mode` or `keymap`. Insert commands
//! produce `Effects` and do not handle mode transitions.
//!
//! # Architecture
//!
//! Submodules are organized by function:
//! - `entry.rs`: Mode entry commands (i, a, I, A, o, O, s, S)
//! - `delete.rs`: Delete commands (BS, C-W, C-U, Del)
//! - `autoindent.rs`: Autoindent and tab expansion (pure computation)
//! - `exit.rs`: Exit cursor, repeat text, block insert (pure computation)
//! - `types.rs`: Context and result types

pub mod auto_pairs;
pub mod autoindent;
pub mod delete;
pub mod effects;
pub mod entry;
pub mod exit;
pub mod indent;
/// Smartindent adjustments for `{`, `}`, `#` characters.
pub mod smartindent;
pub mod types;
/// Auto-wrap utilities for textwidth enforcement.
pub mod wrap;

pub use autoindent::{compute_newline_insert, compute_tab_spaces};
pub use entry::enter_insert_at;
pub use exit::{
    build_insert_exit_effects, compute_block_insert_offsets, compute_exit_cursor,
    compute_repeat_text,
};
pub use types::{
    InsertContext, InsertExitContext, InsertExitParams, InsertPrecomputed, NewlineInsert,
    ReplaceRestoreAction,
};
