// ═══════════════════════════════════════════════════════════════════════════════
// Crate-level lints
// ═══════════════════════════════════════════════════════════════════════════════
//
// The attributes below are the crate's lint policy: no unsafe code, no
// undocumented public items, and no panicking or debug-output constructs on
// production paths. Tests relax the production-only restrictions via the
// `#[cfg_attr(test, allow(...))]` block further down.

// vim-core contains no unsafe code.
#![forbid(unsafe_code)]
// Warnings are errors, so lint regressions cannot accumulate.
#![deny(warnings)]
// Documentation: required for all public items.
#![deny(missing_docs)]
// Clippy hardening — pedantic (deny, not warn)
#![deny(clippy::pedantic)]
// Clippy nursery (warn — unstable lints may have false positives)
#![warn(clippy::nursery)]
// Allow noisy pedantic lints (style preferences, not safety)
#![allow(clippy::module_name_repetitions)]
// We use module::ModuleFoo pattern
// clippy::must_use_candidate — convention: #[must_use] on all pure functions
#![allow(clippy::match_same_arms)] // Sometimes clearer to be explicit
#![allow(clippy::unused_self)] // Common in trait implementations
#![allow(clippy::return_self_not_must_use)] // Self-returning methods are common
#![allow(clippy::doc_markdown)] // Fix incrementally
#![allow(clippy::trivially_copy_pass_by_ref)] // Minor optimization, not critical
#![allow(clippy::struct_field_names)] // Field naming conventions vary
#![allow(clippy::too_many_lines)] // Function length is a review judgement
#![allow(clippy::redundant_else)] // Style preference
#![allow(clippy::manual_let_else)]
// Style preference
// cast_possible_truncation, cast_possible_wrap, cast_sign_loss:
// Handled per-site with targeted #[allow(reason = "...")] annotations.
// No crate-level suppression — every cast is audited individually.
// Allow noisy nursery lints
#![allow(clippy::option_if_let_else)] // Often less readable than match
#![allow(clippy::redundant_pub_crate)] // pub(crate) is intentional documentation
#![allow(clippy::significant_drop_tightening)]
// False positives with RAII guards
// clippy::use_self — convention: Self:: used consistently in impl blocks
// clippy::missing_const_for_fn — convention: const fn on all eligible methods
// Enum and type safety
#![deny(clippy::large_enum_variant)]
#![deny(clippy::enum_glob_use)]
// Restriction lints — no debug or placeholder code in production
#![deny(clippy::dbg_macro)]
#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]
#![deny(clippy::todo)]
#![deny(clippy::unimplemented)]
// Restriction lints — string allocation hygiene
#![deny(clippy::str_to_string)]
// Restriction lints — explicitness
#![deny(clippy::rest_pat_in_fully_bound_structs)]
// Data scope: declarations at the top of a block, not among its statements
#![deny(clippy::items_after_statements)]
// Error handling restrictions (no panic in production paths)
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::indexing_slicing)]
// Test naming: allow Vim key names in test functions (test_V_toggles_visual, etc.)
#![cfg_attr(test, allow(non_snake_case))]
// Test convenience: allow unused bindings in test setup code
#![cfg_attr(test, allow(unused_variables))]
// Test convenience: allow ignoring Result/must_use values in assertions
#![cfg_attr(test, allow(unused_must_use))]
// Test convenience: tests use `.unwrap()`/`.expect()`/`panic!()` and direct
// indexing freely — they are *expected* to panic on assertion failure, and
// the production-only restrictions above (indexing_slicing, unwrap_used,
// expect_used, panic) are explicitly scoped to non-test code via these
// `#[cfg_attr(test, allow(...))]` attributes. This mirrors the existing
// `unused_must_use` pattern above and is the conventional Rust idiom for
// crates that enforce strict panic-discipline on production code.
#![cfg_attr(test, allow(clippy::unwrap_used))]
#![cfg_attr(test, allow(clippy::expect_used))]
#![cfg_attr(test, allow(clippy::panic))]
#![cfg_attr(test, allow(clippy::indexing_slicing))]
// Test convenience: tests sometimes use eprintln!/print! for diagnostic output
// during failure, and freely use String::to_string + non-inlined format args.
// Tests are not subject to the production restriction on these idioms.
#![cfg_attr(test, allow(clippy::print_stderr))]
#![cfg_attr(test, allow(clippy::print_stdout))]
#![cfg_attr(test, allow(clippy::str_to_string))]
#![cfg_attr(test, allow(clippy::uninlined_format_args))]
// Test convenience: explicit clones in tests document intent (e.g.,
// "this fixture is reused below"); the redundant_clone lint is too
// strict for tests.
#![cfg_attr(test, allow(clippy::redundant_clone))]
// Test convenience: tests freely use these idiomatic patterns that
// would be discouraged in production code but are not bugs in tests:
//   - field_reassign_with_default: `let mut x = T::default(); x.f = ...`
//     is clearer in tests than wrapping the construction in a builder.
//   - needless_collect: `let v: Vec<_> = it.collect()` then asserts on
//     `v.len()` and `v.contains(...)` is more readable than chained
//     iterator combinators.
//   - clone_on_copy: explicit `.clone()` in tests documents that the
//     value is logically copied (vs accidentally shared).
//   - useless_conversion / useless_format / single_char_pattern /
//     needless_borrow / needless_range_loop / explicit_iter_loop /
//     stable_sort_primitive / similar_names / no_effect_underscore_binding
//     / unnecessary_min_or_max / cast_possible_truncation / cast_possible_wrap
//     / cast_lossless / cast_sign_loss / needless_raw_string_hashes /
//     unnecessary_trailing_comma / borrow_deref_ref / double_ended_iterator_last:
//     all are stylistic nudges that hurt test readability without changing behavior.
//   - missing_const_for_fn / must_use_candidate / no_effect_underscore_binding /
//     items_after_statements: tests have looser scope discipline; these
//     are production conventions, not test conventions.
//   - match_wildcard_for_single_variants: the `_ => panic!("expected X")`
//     pattern is the idiom for negative-case test assertions.
//   - manual_contains: the explicit-loop equivalent of `.contains(...)`
//     is sometimes clearer in tests when the comparison is non-trivial.
//   - too_many_arguments / needless_pass_by_value: test fixtures and
//     helpers are intentionally explicit.
//   - missing_errors_doc / missing_panics_doc: tests aren't part of the
//     public API contract; doc requirements would just be noise.
#![cfg_attr(test, allow(clippy::field_reassign_with_default))]
#![cfg_attr(test, allow(clippy::needless_collect))]
#![cfg_attr(test, allow(clippy::clone_on_copy))]
#![cfg_attr(test, allow(clippy::useless_conversion))]
#![cfg_attr(test, allow(clippy::useless_format))]
#![cfg_attr(test, allow(clippy::single_char_pattern))]
#![cfg_attr(test, allow(clippy::needless_borrow))]
#![cfg_attr(test, allow(clippy::needless_range_loop))]
#![cfg_attr(test, allow(clippy::explicit_iter_loop))]
#![cfg_attr(test, allow(clippy::stable_sort_primitive))]
#![cfg_attr(test, allow(clippy::similar_names))]
#![cfg_attr(test, allow(clippy::no_effect_underscore_binding))]
#![cfg_attr(test, allow(clippy::unnecessary_min_or_max))]
#![cfg_attr(test, allow(clippy::cast_possible_truncation))]
#![cfg_attr(test, allow(clippy::cast_possible_wrap))]
#![cfg_attr(test, allow(clippy::cast_lossless))]
#![cfg_attr(test, allow(clippy::cast_sign_loss))]
#![cfg_attr(test, allow(clippy::needless_raw_string_hashes))]
#![cfg_attr(test, allow(clippy::unnecessary_trailing_comma))]
#![cfg_attr(test, allow(clippy::borrow_deref_ref))]
#![cfg_attr(test, allow(clippy::double_ended_iterator_last))]
#![cfg_attr(test, allow(clippy::missing_const_for_fn))]
#![cfg_attr(test, allow(clippy::must_use_candidate))]
#![cfg_attr(test, allow(clippy::items_after_statements))]
#![cfg_attr(test, allow(clippy::match_wildcard_for_single_variants))]
#![cfg_attr(test, allow(clippy::manual_contains))]
#![cfg_attr(test, allow(clippy::too_many_arguments))]
#![cfg_attr(test, allow(clippy::needless_pass_by_value))]
#![cfg_attr(test, allow(clippy::missing_errors_doc))]
#![cfg_attr(test, allow(clippy::missing_panics_doc))]
#![cfg_attr(test, allow(clippy::redundant_closure_for_method_calls))]
#![cfg_attr(test, allow(clippy::unnecessary_map_or))]
#![cfg_attr(test, allow(clippy::unnecessary_sort_by))]
#![cfg_attr(test, allow(clippy::semicolon_if_nothing_returned))]
#![cfg_attr(test, allow(clippy::single_element_loop))]
#![cfg_attr(test, allow(clippy::collapsible_if))]
#![cfg_attr(test, allow(clippy::collapsible_match))]
#![cfg_attr(test, allow(clippy::if_same_then_else))]
#![cfg_attr(test, allow(clippy::if_not_else))]
#![cfg_attr(test, allow(clippy::comparison_chain))]
#![cfg_attr(test, allow(clippy::range_plus_one))]
#![cfg_attr(test, allow(clippy::needless_borrows_for_generic_args))]
#![cfg_attr(test, allow(clippy::or_fun_call))]
#![cfg_attr(test, allow(clippy::format_in_format_args))]
#![cfg_attr(test, allow(clippy::format_push_string))]
#![cfg_attr(test, allow(clippy::manual_is_multiple_of))]
#![cfg_attr(test, allow(clippy::manual_checked_ops))]
#![cfg_attr(test, allow(clippy::len_zero))]
#![cfg_attr(test, allow(clippy::struct_excessive_bools))]
#![cfg_attr(test, allow(clippy::too_long_first_doc_paragraph))]
#![cfg_attr(test, allow(clippy::doc_lazy_continuation))]
#![cfg_attr(test, allow(clippy::doc_link_with_quotes))]
#![cfg_attr(test, allow(clippy::derive_partial_eq_without_eq))]
#![cfg_attr(test, allow(clippy::derivable_impls))]
#![cfg_attr(test, allow(clippy::debug_assert_with_mut_call))]
#![cfg_attr(test, allow(clippy::branches_sharing_code))]
#![cfg_attr(test, allow(clippy::while_let_loop))]
#![cfg_attr(test, allow(clippy::needless_continue))]
#![cfg_attr(test, allow(clippy::map_unwrap_or))]
#![cfg_attr(test, allow(clippy::needless_lifetimes))]
#![cfg_attr(test, allow(clippy::elidable_lifetime_names))]
#![cfg_attr(test, allow(clippy::double_must_use))]
#![cfg_attr(test, allow(clippy::use_self))]
#![cfg_attr(test, allow(clippy::items_after_test_module))]

//! # vim-core
//!
//! Pure Rust Vim engine.
//!
//! ## Philosophy
//!
//! - **Pure**: No side effects, only effects as output
//! - **Fidelity**: Every keystroke matches Neovim exactly
//! - **Embeddable**: Works in any editor via FFI
//!
//! ## Architecture
//!
//! Layer dependencies flow downward only. The DAG below is the authoritative
//! statement of what may import what.
//!
//! ```text
//! Leaf data (zero deps):       primitives           grammar/types
//!                                   ^                     ^
//! Logic leaves:                  errors   document   keymap
//!                                   ^       ^        ^         ^
//! Middle data:                ──────────── state ───────────────
//!                                              ^
//! Middle logic:                  effects     grammar (handlers)
//!                                    ^             ^
//!                                ────────  commands  ────────
//!                                              ^
//!                                          dispatch
//!                                              ^
//!                                            mode
//!                                              ^
//!                                         execution
//!                                              ^
//!                                    lib.rs / bridge / test_utils
//! ```
//!
//! External crates:               vim-regex (Vim regex engine)
//!
//! Rules:
//! - `primitives` and `grammar/types` have no internal imports
//! - `dispatch` bridges grammar enums to command implementations
//! - `mode` coordinates grammar + commands
//! - `effects` is data-only — it must not depend on `grammar`
//! - `errors` is data-only — it must not depend on `state`
//!
//! ## Usage
//!
//! ```ignore
//! use vim_core::execution::{InputContext, VimEngine};
//! use vim_core::keymap::KeyEvent;
//!
//! let mut engine = VimEngine::new();
//! let ctx = InputContext::new(&document, cursor_offset).validate()?;
//! let response = engine.process(KeyEvent::char('l'), ctx);
//! // Apply `response.effects` to your editor
//! ```

#[cfg(not(feature = "std"))]
compile_error!("vim-core is std-only. Enable the `std` feature.");

// Compile-time guarantee: all `u32 as usize` casts in this crate are lossless.
// This would fail to compile on 16-bit targets (if Rust ever supported them).
const _: () = assert!(std::mem::size_of::<usize>() >= std::mem::size_of::<u32>());
// Feature flag handling
cfg_if::cfg_if! {
    if #[cfg(feature = "logging")] {
        #[allow(unused_macros, reason = "cfg_if defines both branches; only one is active")]
        macro_rules! debug { ($($tt:tt)*) => { tracing::debug!($($tt)*) } }
        #[allow(unused_macros, reason = "cfg_if defines both branches; only one is active")]
        macro_rules! trace { ($($tt:tt)*) => { tracing::trace!($($tt)*) } }
        #[allow(unused_macros, reason = "cfg_if defines both branches; only one is active")]
        macro_rules! info { ($($tt:tt)*) => { tracing::info!($($tt)*) } }
        #[allow(unused_macros, reason = "cfg_if defines both branches; only one is active")]
        macro_rules! warn { ($($tt:tt)*) => { tracing::warn!($($tt)*) } }
        #[allow(unused_macros, reason = "cfg_if defines both branches; only one is active")]
        macro_rules! error { ($($tt:tt)*) => { tracing::error!($($tt)*) } }
    } else {
        #[allow(unused_macros, reason = "cfg_if defines both branches; only one is active")]
        macro_rules! debug { ($($tt:tt)*) => {} }
        #[allow(unused_macros, reason = "cfg_if defines both branches; only one is active")]
        macro_rules! trace { ($($tt:tt)*) => {} }
        #[allow(unused_macros, reason = "cfg_if defines both branches; only one is active")]
        macro_rules! info { ($($tt:tt)*) => {} }
        #[allow(unused_macros, reason = "cfg_if defines both branches; only one is active")]
        macro_rules! warn { ($($tt:tt)*) => {} }
        #[allow(unused_macros, reason = "cfg_if defines both branches; only one is active")]
        macro_rules! error { ($($tt:tt)*) => {} }
    }
}

// Module declarations
pub mod commands;
pub mod dispatch; // Command dispatchers (motion, operator, textobject)
pub mod document;
pub mod effects;
pub mod errors;
pub mod execution;
pub mod grammar;
pub mod keymap;
pub mod mode;
pub mod primitives;
#[cfg(test)]
mod proof_tests; // Proof tests for deep audit fixes
pub use vim_regex as regex;
pub mod state;
mod static_checks; // Compile-time size guards
#[cfg(any(test, feature = "testing"))]
pub mod test_utils; // Shared test infrastructure (SimpleDocument, etc.)

// Top-level re-exports for external crates.
pub use primitives::{IncCommandMode, MagicMode, SearchFlags, SelectionMode, VimOptions};

/// Prelude for convenient imports.
///
/// Re-exports commonly used types for ergonomic `use vim_core::prelude::*;`.
pub mod prelude {
    pub use crate::effects::{Effect, Effects};
    pub use crate::errors::{ErrorSeverity, VimError};
    pub use crate::execution::{InputContext, Response, Validated, VimEngine};
    pub use crate::keymap::KeyEvent;
    pub use crate::primitives::Mode;
    pub use crate::primitives::VimOptions;
}
