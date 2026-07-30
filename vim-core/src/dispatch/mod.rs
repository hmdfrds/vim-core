//! Command dispatchers.
//!
//! Bridge between Grammar layer (flat enums) and Commands layer (organized structs).
//! Each dispatcher maps an enum variant to its implementation.
//!
//! # Layering
//!
//! Imports `grammar` (enums), `commands` (implementations) and `effects`
//! (lightweight results); must not import `mode` or `execution`.
//!
//! Dispatch is a bridge: dispatchers may create lightweight `Effect` values
//! (e.g. `ShowError`, `OperatorFilter`) for commands that return results
//! directly without requiring document access. Full execution logic belongs
//! in `execution/`.
//!
//! # Architecture
//!
//! ```text
//! Grammar Layer          Dispatch Layer        Commands Layer
//! ─────────────          ──────────────        ──────────────
//! Motion::Left    ──────► motion.rs ────────► CharMotion::Left
//! Operator::Delete ─────► operator.rs ──────► DeleteOperator
//! TextObject::Word ─────► textobject.rs ────► WordTextObject
//! ```
//!
//! # Adding New Dispatchers
//!
//! 1. Add variant to Grammar enum
//! 2. Implement in commands/ module
//! 3. Add dispatch case here (exhaustive match forces this)

mod action;
mod find;
mod insert;
mod mark;
mod motion;
mod operator;
mod select;
mod textobject;
mod visual;

pub use action::{dispatch_action, dispatch_join_no_space, ActionContext};
pub use find::{dispatch_char_command, dispatch_find, CharCommandInput};
pub use insert::{
    build_insert_exit_effects, compute_newline_insert, compute_tab_spaces, dispatch_insert,
    dispatch_insert_entry, enter_insert_at, InsertExitParams, InsertPrecomputed,
    ReplaceRestoreAction,
};
pub use mark::{dispatch_mark, MarkContext};
pub use motion::{
    dispatch_motion, dispatch_motion_with_effects, MotionContext, MotionEffectsContext,
    ViewportInfo,
};

pub use operator::{
    adjust_eof_range, block_visual, dispatch_operator, dispatch_operator_find,
    dispatch_operator_line, dispatch_operator_mark, dispatch_operator_selection,
    dispatch_operator_textobject, dispatch_operator_with_motion, empty_textobject_result,
    OperatorContext, OperatorFindInput, OperatorLineInput, OperatorMarkInput, OperatorMotionInput,
    OperatorTextObjectInput, SelectionOperatorContext,
};

pub use select::dispatch_select_enter;
pub use textobject::{
    dispatch_textobject, dispatch_textobject_with_count, dispatch_visual_textobject,
    TextObjectContext, TextObjectRange,
};
pub use visual::{
    dispatch_visual, expand_selection_to_lines, reconstruct_from_last_visual,
    resolve_live_selection, save_last_visual_effects, VisualContext,
};

mod ex;
pub use ex::dispatch_ex_core;
pub(crate) use ex::dispatch_resolve_ex_range;
