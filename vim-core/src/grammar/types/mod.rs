//! Grammar types.
//!
//! Supporting types for the grammar state machine.
//! Split into separate files for maintainability.
//!
//! # Layering
//!
//! Imports `std`, `strum` (derives only) and `primitives`; must not import
//! `commands`, `effects` or `execution`.
//!
//! Grammar types are enums parsed from keystrokes. `Operator` and
//! `CommandLineEdit` are re-exported from `primitives`, where they
//! canonically live as pure domain types with zero dependencies.

mod action;
mod char_command;
mod command_modifiers;
mod ex_command;
mod ex_range;
mod mark;
mod motion;
mod textobject;

pub use action::Action;
pub use char_command::CharCommand;
pub use command_modifiers::ModifierFlags;
pub use ex_command::{
    ExCommand, MapModePrefix, SetAssignment, SortOptions, TimeAmount, ZWindowStyle,
};
pub use ex_range::{ExRange, LineSpec, RangeSeparator};
pub use mark::MarkType;
pub use motion::Motion;
pub use textobject::{SeekDirection, TextObject, TextObjectKind, TextObjectScope};

// Re-exported from primitives (canonical location).
//
// `SubFlags`/`CaseSensitivity` were relocated to `primitives` to fix audit
// Defect #1 (effects layer importing from grammar layer). The re-export here
// preserves back-compat for `use crate::grammar::types::SubFlags` callers
// during the transition; new code should import directly from `primitives`.
pub use crate::primitives::{CaseSensitivity, CommandLineEdit, Operator, SubFlags};
