//! Vim commands.
//!
//! Operators, motions, text objects, actions, insert, visual, and ex commands.
//!
//! # Layering
//!
//! Imports `primitives`, `effects` and `grammar` (enums only); must not
//! import `mode` or `execution`. Commands produce `Effects` and know
//! nothing about mode handlers or execution.
//!
//! ```text
//! commands ──► effects ──► primitives
//!    │
//!    ├──► grammar::Motion, grammar::TextObject (read-only enums)
//!    │
//!    ✗──► mode, execution
//! ```
//!
//! # File Layout
//!
//! A submodule is either a single file (`helpers.rs`, `line_index.rs`,
//! `result.rs`) or a directory. Directory submodules hold some mix of:
//!
//! - `mod.rs` — re-exports plus that submodule's own layering doc.
//! - `types.rs` — context and result structs for the domain. Present in
//!   `actions/`, `ex/`, `insert/`, `motions/`, `operators/`, `textobjects/`
//!   and `visual/`; absent from `command_line/` and `selections/`.
//! - Implementation files, one per command family (`word.rs`, `put.rs`, …).
//! - `effects.rs` — stateless effect builders. Only `actions/`,
//!   `command_line/`, `ex/` and `insert/` have one; `motions/`, `operators/`,
//!   `selections/`, `textobjects/` and `visual/` do not.
//!
//! Implementation signatures are per-domain, not uniform across the tree:
//!
//! - `actions/`, `operators/`: `fn execute_x(&XContext) -> CommandResult`
//!   (effects plus an optional cursor).
//! - `motions/`: plain functions named after the key — `w`, `b`, `dollar` —
//!   taking `&MotionContext` and returning `MotionResult`.
//! - `textobjects/`: `fn compute_x_object(..)` returning the object's range.
//!
//! `effects.rs` builders take primitives only — no document, no `VimState` —
//! and usually return `Effects`. Some return `(Effects, usize)` where the
//! caller needs the resulting offset (`insert::effects::repeat_text`,
//! `insert::effects::strip_indent`), and a few are pure analysis helpers that
//! return no effects at all (`insert::effects::analyze_replace_target`).
//!
//! ## Effect-module Import Alias
//!
//! Effect modules are aliased to a short `*_effects` name at the `use` site,
//! not re-exported anywhere central:
//!
//! ```text
//! use crate::commands::actions::effects as action_effects;
//! use crate::commands::insert::effects as insert_effects;
//! use crate::commands::ex::effects as ex_effects;
//! ```
//!
//! `command_line::effects` has one caller and is used fully qualified.

pub mod actions;
pub mod command_line;
pub mod ex;
pub mod helpers;
pub mod insert;
pub mod line_index;
pub mod motions;
pub mod operators;
pub mod result;
pub mod selections;
pub mod textobjects;
pub mod visual;

pub use result::CommandResult;
