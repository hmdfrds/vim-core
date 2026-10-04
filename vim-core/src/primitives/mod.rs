//! Primitive types for vim-core.
//!
//! Position, Range, Direction, and other fundamental types.
//!
//! # Layering
//!
//! This module is the bottom of the dependency graph. It imports only `std`
//! and external crates; it must not import any other module of this crate
//! (`commands`, `grammar`, `effects`, `state`, `execution`, `mode`,
//! `keymap`).
//!
//! ```text
//! primitives ← (everything else imports from here)
//! ```
//!
//! Dependencies flow downward only; no cycles.

/// Abbreviation table and types.
pub mod abbreviation;
pub use abbreviation::{AbbrevEntry, AbbrevKind, AbbrevMode, AbbrevTable};

mod api_types;
pub use api_types::{AutocmdId, CallerId, CapabilityTier, TextEdit, VarScope, VimValue};
mod buffer_id;
pub(crate) mod byte_delta;
pub mod changeset;
mod clipboard_metadata;
mod clipboard_mode;
mod command_line_edit;
mod command_line_prompt;
mod command_properties;
mod completion_kind;
mod cursor_style;
pub mod digraph;
mod direction;
mod find_direction;
mod format_flags;
mod host_settings;
mod insert_entry_type;
mod join_style;
mod key_hints;
mod last_visual_info;
mod limits;
mod linewise_text;
mod map_mode_prefix;
mod mark;
mod mark_type;
mod mode;
mod mode_appearance;
mod motion_inclusivity;
mod motion_type;
mod operator;
mod option_scope;
mod position;
mod range;
mod range_transform;
mod register_type;
mod replaced_char;
mod return_to;
mod savepoint;
mod search_direction;
mod search_flags;
mod selection;
mod selection_shape;
mod selections;
mod sticky_target;
mod structural_flags;
mod sub_flags;
mod substitute_confirm_payload;
mod substitute_preview;
mod subword_config;
mod text_diff;
mod text_range;
pub mod text_util;
mod undo_cursor_strategy;
mod undo_nav_step;
mod undo_tree_snapshot;
mod vim_context;
mod vim_event;
mod vim_options;
mod virtual_column;
mod virtual_text;
mod word_char_set;
mod word_kind;

pub use buffer_id::BufferId;
pub use clipboard_metadata::ClipboardMetadata;
pub use clipboard_mode::UseSystemClipboard;
pub use command_line_edit::CommandLineEdit;
pub use command_line_prompt::CommandLinePrompt;
pub use command_properties::{CommandProperties, RepeatBehavior};
pub use completion_kind::CompletionKind;
pub use cursor_style::{mode_to_override_index, CursorShape, CursorStyle, CURSOR_OVERRIDE_COUNT};
pub use digraph::{lookup_digraph, DigraphRegistry};
pub use direction::Direction;
pub use find_direction::{FindDirection, LastFind};
pub use format_flags::FormatFlags;
pub use host_settings::HostSettings;
pub use insert_entry_type::InsertEntryType;
pub use join_style::JoinStyle;
pub use key_hints::{KeyHint, KeyHintsInfo};
pub use last_visual_info::LastVisualInfo;
pub use linewise_text::LinewiseText;
pub use map_mode_prefix::MapModePrefix;
pub use mark::Mark;
pub use mark_type::MarkName;
pub use mode::{Mode, ModeKind, VisualType};
pub use mode_appearance::ModeAppearance;
pub use motion_inclusivity::MotionInclusivity;
pub use motion_type::MotionType;
pub use operator::ComposedPair;
pub use operator::Operator;
pub use operator::OperatorKind;
pub use option_scope::{
    is_sentinel, resolve_option, OptionId, OptionOverrides, OptionScope, OptionValue,
};
pub use position::{Column, LineNumber, Offset, Position};
pub use range::{LineRange, Range};
pub use range_transform::{MotionRange, NormalizedRange};
pub use register_type::{RegisterCategory, RegisterContent, RegisterName};
pub use replaced_char::ReplacedChar;
pub use return_to::ReturnTo;
pub use search_direction::SearchDirection;
pub use search_flags::SearchFlags;
pub use selection::{Selection, SelectionRange};
pub use selection_shape::SelectionShape;
pub use selections::CursorMode;
pub use selections::Selections;
pub use sticky_target::StickyTarget;
pub use structural_flags::StructuralFlags;
pub use sub_flags::{CaseSensitivity, SubFlags};
pub use substitute_confirm_payload::{ConfirmMatchPayload, SubstituteConfirmPayload};
pub use substitute_preview::SubstitutePreviewMatch;
pub use subword_config::SubwordConfig;
pub use text_diff::diff_texts;
pub use text_range::{RangeEnd, TextRange};
pub use undo_cursor_strategy::UndoCursorStrategy;
pub use undo_nav_step::UndoNavStep;
pub use vim_context::VimContext;
pub use vim_event::{OptionSetPayload, UndoDirection, VimEvent};
pub use vim_options::{AutoPairs, IncCommandMode, Pair, SelectionMode, VimOptions, WordEraseStyle};
pub use vim_regex::MagicMode;
pub use virtual_column::VirtualColumn;
pub use virtual_text::{Diagnostic, DiagnosticSeverity, VirtualTextPosition};
pub use word_char_set::WordCharSet;
pub use word_kind::WordKind;

mod semantic_object;
pub use semantic_object::SemanticObject;
pub use undo_tree_snapshot::{NodeId, UndoTreeNodeView, UndoTreeSnapshot};

// Changeset types
pub use changeset::{Assoc, ChangeIter, ChangeSet, ChangeSetError, ChangeSetRecorder, TextOp};
pub use limits::MAX_BRACKET_TRAVEL;
pub use savepoint::{SavePoint, SavePointRestore};
pub use text_util::{is_combining_mark, is_word_char, next_char_boundary, prev_char_boundary};
