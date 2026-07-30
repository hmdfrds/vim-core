//! Common test utilities for vim-core tests.
//!
//! This module provides:
//! - document/: In-memory Document implementation (TestDocument)
//! - golden: re-export of [`vim_test::golden`] (expected results from Neovim)
//! - neovim_oracle: re-export of [`vim_test::neovim_oracle`]
//! - runner/: Test orchestration and utilities
//! - macros: vim_test! macro for one-liner test definitions
//!
//! # Structure
//!
//! ```text
//! common/
//! ├── mod.rs           (this file)
//! ├── document/        (TestDocument implementation)
//! │   ├── mod.rs
//! │   ├── test_document.rs
//! │   ├── cursor.rs
//! │   └── edits.rs
//! ├── runner/          (test execution)
//! │   ├── mod.rs
//! │   ├── orchestrator.rs
//! │   ├── key_parser.rs
//! │   ├── effect_applier.rs
//! │   └── state_diff.rs
//! └── macros.rs        (vim_test! macro)
//! ```
//!
//! `golden` and `neovim_oracle` used to be forked copies of the same modules
//! in the `vim-test` crate. They are now re-exported from `vim_test` so that
//! existing paths (`common::golden::GoldenState`, …) keep resolving.

pub mod document;
pub mod macros;
pub mod runner;

#[allow(unused_imports)]
pub use vim_test::{golden, neovim_oracle};

#[allow(unused_imports)]
pub use document::TestDocument;
#[allow(unused_imports)]
pub use runner::*;
