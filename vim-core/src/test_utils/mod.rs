//! Test utilities for vim-core.
//!
//! This module provides shared test infrastructure. It lives at the crate root
//! (not inside `document/`) so that bottom-layer modules don't need upward
//! imports in test code.
//!
//! # Layering
//!
//! Imports `document`, `primitives`, `commands`, `grammar`, `state`,
//! `effects` and `errors`; must not import `execution`, `dispatch`, `mode`
//! or `keymap`.

pub mod annotated_text;
mod simple_document;

pub use simple_document::SimpleDocument;
