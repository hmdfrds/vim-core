//! Handler modules for grammar parser.
//!
//! Each state has its own handler module for clean separation of concerns.
//! Handler modules add `impl` blocks to the `Parser` struct.
//!
//! # Layering
//!
//! Imports the parent `grammar` module, `keymap` and `primitives`; must not
//! import `commands`. Handlers produce `Command` values; they never call
//! command implementations.

pub mod awaiting;
pub mod helpers;
pub mod insert;
pub mod operator;
pub mod prefix;
pub mod ready;
pub mod surround;
pub mod visual;
