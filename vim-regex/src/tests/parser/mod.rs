//! Tests for the Vim regex parser.
//!
//! Included via `#[path = "../tests/parser/mod.rs"] mod tests;` in `parser/mod.rs`.
//! Split into submodules to respect the 700-line-per-file limit.

mod basic;

mod groups;

mod advanced;

mod branch_and;

mod anywhere_anchors;

mod composing;

mod diagnostics;

mod collections_posix;
