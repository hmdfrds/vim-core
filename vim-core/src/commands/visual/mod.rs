//! Visual mode commands.
//!
//! Handles visual mode operations (enter, exit, switch, selection manipulation).
//!
//! # Layering
//!
//! Imports `primitives`, `effects`, `state`, `grammar` and `document`; must
//! not import `execution`, `dispatch`, `mode` or `keymap`. Visual commands
//! produce `Effects` and do not carry out execution themselves.
//!
//! # Architecture
//!
//! Organized by function:
//! - `mode.rs`: Mode entry/exit commands (v, V, C-v, Esc, gv)
//! - `selection.rs`: Selection manipulation helpers
//! - `types.rs`: Context and result types

pub mod mode;
pub mod selection;
pub mod textobject;
pub mod types;

pub use selection::{
    expand_selection_to_lines, extend_selection, move_cursor, selection_to_operator_range,
    visual_exit_effects,
};
pub use types::VisualContext;
