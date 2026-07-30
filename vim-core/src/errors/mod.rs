//! Error types.
//!
//! `VimError` is the typed single source of truth for all user-facing
//! Vim error messages.
//!
//! # Layering
//!
//! Imports `primitives` and `state` (for `From` impls only); must not import
//! `commands`, `dispatch`, `execution`, `mode` or `keymap`.
mod vim_error;

pub use vim_error::{ErrorSeverity, VimError};
