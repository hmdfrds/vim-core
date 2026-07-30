//! Vim state management.
//!
//! Pure data containers for all mutable state that persists across commands.
//! Each sub-module owns one state domain (registers, marks, search, etc.)
//! and the [`VimState`] struct composes them all.
//!
//! # Architecture
//!
//! ```text
//! execution ──► dispatch ──► commands ──► effects
//!     │                         │
//!     └──────► state ◄──────────┘
//! ```
//!
//! # Layering
//!
//! Imports `primitives`, `std` and `grammar::Command` (repeat state only);
//! must not import `commands`, `effects`, `execution` or `dispatch`. State
//! modules are pure data containers with no execution logic.

mod changelist;
mod command_intent;
mod command_line;
/// State diffing for reactive UIs.
pub mod diff;
pub mod federation;
mod find;
mod insert;
mod jumplist;
mod macros;
pub(crate) mod mark_snapshot;
mod marks;
mod message_history;
mod mode;
mod multi_cursor;
mod offset_adjust;
mod position_tracker;
mod registers;
mod remap;
mod repeat;
mod search;
pub(crate) mod substitute_confirm;
mod syntax_selection;
mod transient;
mod undo_tree;
mod variable_store;
mod vim_state;
mod visual;

// === Change List ===
pub use changelist::ChangeList;

// === Command Intent ===
pub use command_intent::{capture_intent, CommandIntent};

// === Command-Line ===
pub use command_line::{CommandLinePrompt, CommandLineState, CompletionCandidate};

// === Insert ===
pub use insert::{BlockInsertContext, InsertState};

// === Jump List ===
pub use jumplist::{JumpEntry, JumpList};

// === Macros ===
pub use macros::{MacroRecursionError, MacroState, MAX_MACRO_DEPTH};

// === Marks ===
pub use marks::BufferMarks;
pub use marks::GlobalMarkEntry;
pub use marks::Marks;
#[cfg(feature = "serde")]
pub use marks::SerializedMarks;

// === Registers ===
pub use registers::Registers;

// === Repeat ===
pub use repeat::RepeatState;

// === Search ===
pub use search::{LastPatternKind, SearchOffset, SearchState};

// === Substitute Confirm ===
pub use substitute_confirm::{SubstituteConfirmMatch, SubstituteConfirmState};

// === Syntax Selection ===
pub use syntax_selection::{selections_contained_by, SyntaxSelectionHistory};

// === Transient ===
pub use transient::{ScrollHint, StatusMessage};

// === Message History ===
pub use message_history::{MessageEntry, MessageHistory, MessageKind};

// === State Diffing ===
pub use diff::{StateDiff, StateSnapshot};

// === Position Tracker ===
pub use position_tracker::{PositionTracker, TrackedId};

// === Undo Tree ===
pub use undo_tree::{
    LeafInfo, NodeId, NodeInfo, UndoStep, UndoTree, UndoTreeNodeView, UndoTreeSnapshot,
};

// === Variable Store ===
pub use variable_store::VariableStore;

// === VimState ===
pub use vim_state::{SearchCountCache, VimState};

// === Multi-Cursor ===
pub use multi_cursor::{MatchSearchState, MultiCursorCommand, MultiCursorState};
