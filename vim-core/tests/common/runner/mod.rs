//! Test runner infrastructure.
//!
//! Split into separate concerns:
//! - `key_parser.rs` - Parse key notation strings
//! - `effect_applier.rs` - Apply effects to test documents
//! - `operator_to_mark.rs` - Canonical OperatorToMark handler (shared)
//! - `state_diff.rs` - Compare and diff states
//! - `orchestrator.rs` - Main test orchestration

mod effect_applier;
pub mod invariants;
mod key_parser;
pub(super) mod operator_to_mark;
mod orchestrator;
mod state_diff;

pub use effect_applier::apply_effect;
pub use key_parser::parse_keys;
pub use orchestrator::{run_fidelity_test, run_vim_commands};
pub use state_diff::compare_states;
