//! Tests for `matchers.rs` — `CharMatcher` and `ZeroWidthMatcher`.
//!
//! Included via `#[path = "tests/matchers/mod.rs"] mod tests;` in `matchers.rs`.
//! Split into submodules to respect the 700-line-per-file limit.

#[path = "char.rs"]
mod char_matcher;

#[path = "zero_width.rs"]
mod zero_width;

#[path = "builder.rs"]
mod builder;
