//! Mode handlers — trait-based key routing per Vim mode.
//!
//! Each mode (Normal, Insert, Visual, Select, Replace, CommandLine, OperatorPending)
//! has its own handler implementing `ModeHandler`. The trait signature acts
//! as a **compiler-enforced contract**: handlers can only see what's passed
//! through `ModeContext`, which prevents access to engine internals.
//!
//! # Layering
//!
//! Mode sits high in the graph — it coordinates grammar and commands.
//! Imports `primitives`, `grammar`, `state`, `commands` and `keymap`; must
//! not import `execution`, which would create a cycle.
//!
//! ```text
//! mode ──► grammar ──► effects ──► primitives
//!   │         │
//!   │         └──► state
//!   │
//!   └──► commands
//!   └──► keymap
//!   ✗
//!   └──► execution (not imported — would create a cycle)
//! ```
//!
//! # Strategy Pattern
//!
//! ```text
//! ModeDispatcher (zero-cost, 0 bytes)
//!   ├── NormalModeHandler          (Normal)
//!   ├── InsertModeHandler          (Insert)
//!   ├── VisualModeHandler          (Visual char/line/block)
//!   ├── SelectModeHandler          (Select char/line/block)
//!   ├── ReplaceModeHandler         (Replace)
//!   ├── CommandLineModeHandler     (Command-line :/?!)
//!   └── OperatorPendingModeHandler (Operator-pending d/c/y…)
//! ```

pub mod capabilities;
mod command_line;
mod insert;
mod normal;
mod operator_pending;
mod replace;
mod select;
mod types;
mod visual;

/// Sealed trait — prevents external implementations of `ModeHandler`.
///
/// Only the 7 handler types defined in this module may implement
/// `ModeHandler`; external crates cannot.
mod sealed {
    pub trait Sealed {}

    impl Sealed for super::NormalModeHandler {}
    impl Sealed for super::VisualModeHandler {}
    impl Sealed for super::OperatorPendingModeHandler {}
    impl Sealed for super::InsertModeHandler {}
    impl Sealed for super::ReplaceModeHandler {}
    impl Sealed for super::CommandLineModeHandler {}
    impl Sealed for super::SelectModeHandler {}
}

// Re-export handler types from submodules.
pub use command_line::CommandLineModeHandler;
pub use insert::InsertModeHandler;
pub use normal::NormalModeHandler;
pub use operator_pending::OperatorPendingModeHandler;
pub use replace::ReplaceModeHandler;
pub use select::SelectModeHandler;
pub use visual::VisualModeHandler;

// Re-export core types from types.rs.
pub use types::{
    CommandLineResult, InsertMode, ModeAction, ModeContext, ModeDispatcher, ModeHandler,
};

// Re-export capability types.
pub use capabilities::{
    route_through_capabilities, Capability, CapabilityResult, CapabilitySet, ModeProfile,
};

#[cfg(test)]
mod mode_tests;
