//! Command execution.
//!
//! `VimEngine` and execution infrastructure.
//!
//! # Architecture
//!
//! ```text
//! execution (TOP) ──► dispatch (bridge) ──► commands (logic) ──► effects
//! ```
//!
//! ## Architecture Notes
//!
//! - **`executor.rs`**: Routing layer. `execute()` maps `Command` variants to
//!   dispatch functions — overwhelmingly context construction and delegation.
//!   A few local helpers post-process the effects a dispatcher returned (e.g.
//!   `zy` trims trailing whitespace out of a block-yank register).
//! - **`executor_ex.rs`**: Ex-command routing layer — `executor.rs`'s counterpart
//!   for the `:` command line. Maps `ExCommand` variants to `commands::ex`
//!   implementations, or emits a `HostRequest` for shell-only operations.
//! - **`engine.rs`** (plus the `engine/` submodules): Top-level orchestrator.
//!   Owns the `Parser`, `Keymap`, `VimState` and `ModeDispatcher`. There is no
//!   `Executor` type — `executor.rs` exposes free functions the engine calls.
//!   Routes keystrokes through mapping expansion → mode dispatch → pipeline.
//! - **`insert_handler.rs`**: Insert/Replace mode *exit* orchestration only. One
//!   function, `handle_insert_exit`, which applies the entry count repeat
//!   (`3iX<Esc>`), strips auto-indent left by `o`/`O`, replicates block-visual
//!   inserts and fixes up the `[`/`]` change marks. Insert session state
//!   (accumulated text, replace stack) lives in `state::insert::InsertState`;
//!   per-keystroke insert handling goes through `dispatch::dispatch_insert`.
//! - **`effect_processor.rs`**: Post-execution effect scanning. Syncs effects to
//!   engine state and handles dot-repeat interception.
//! - **`pipeline.rs`**: Parse → Resolve → Execute pipeline types and resolver.
//! - **`context.rs`**: Type-state validated `InputContext` and `ExecutionContext`.
//! - **`host.rs`**: Host boundary contracts (`HostRequest`/`HostResult`).
//!
//! # Layering
//!
//! Execution is the top layer: it may import any internal module, and no
//! internal module below it may import execution. It coordinates mode,
//! grammar and commands to produce `Effects`.
//!
//! Files here have a soft target of 700 lines; split rather than grow past it.

mod event_registry;
pub use event_registry::{
    AutocmdFilter, AutocmdRegistration, EventContext, EventRegistry, StoredAutocmdHandler,
};
#[allow(dead_code)] // Public API consumed by host integration layers.
mod api;
pub use api::{InvocationContext, VimApi};
mod api_emitter;
pub use api_emitter::EffectEmitter;
mod api_error;
pub use api_error::ApiError;
mod flat_api;
pub use flat_api::{FlatApi, FlatResult};
mod api_views;
#[allow(dead_code)] // Public API consumed by host integration layers.
mod expr_eval;
pub use expr_eval::ExpressionEval;
mod expr_engine;
pub use api_views::{
    BufferView, CursorView, MarkView, MultiBufferView, OffsetSelectionInfo, OptionView, RegexMatch,
    RegisterView, StateView, TagMatch, VariableView,
};
pub use expr_engine::{ExprContext, ExprEngine, RuntimeError, SimpleExprEval};
mod context;
mod dirty_tracker;
pub use dirty_tracker::{DirtyFlags, DirtyInfo, DirtyRange, DirtyTracker};
mod effect_processor;
mod federation_extractor;
pub(crate) mod modifier_filter;
mod undo_stack;
pub use undo_stack::UndoStore;
mod engine;
mod executor;
mod executor_ex;
mod external_edit;
mod host;
pub mod host_api;
mod host_callbacks;
pub mod host_defaults;
mod host_notification;
mod host_response;
mod host_session;
mod key_notation;
pub mod multi_cursor;
mod multi_cursor_executor;
pub mod session_host;
pub use host_callbacks::HostCallbacks;
pub use host_response::{
    CommandLineInfo, CursorShape, EditOp, HostResponse, MarkChangeInfo, ScrollInfo,
    ScrollPlacement, SearchMatchInfo, SelectionInfo, SubstitutePreviewState,
};
pub use host_session::{HostSession, UnknownRequestError};
pub use multi_cursor_executor::MultiCursorContext;
pub use session_host::SessionHost;
mod insert_handler;
mod pipeline;
pub mod predictive;
mod property_overlay;
#[allow(dead_code)] // Public API consumed by host integration layers.
pub(crate) mod safety_harness;
pub use property_overlay::PropertyOverlay;
pub mod replay;
mod response;
mod set_operator;
mod shell_expand;
pub mod utilities;

pub use context::{
    ContextError, ContextResult, ExecutionContext, InputContext, Unvalidated, Validated,
};
pub use engine::buffer_state::BufferLocalState;
pub use engine::fork::{ForkError, ScopedFork};
pub use engine::{
    parse_keys_from_string, HookAction, HookContext, HookHandler, HookId, HookPoint, HostMapping,
    KeyInterestSet, MacroOutput, VimEngine,
};
pub use external_edit::{ExternalEdit, ExternalEditKind};
pub use host::{
    CmdlineCompletionEntry, CmdlineCompletionKind, HostRequest, HostRequestId, HostRequestKind,
    HostRequestMeta, HostResult, RequestDisposition, SplitDirection,
};
pub use host_api::{
    fallback_effect, filter_effects_for_host, required_capability, should_deliver,
    simple_offset_to_pos, simple_pos_to_offset, DeferredAction, DeferredActionKind, HostCapability,
    HostCapabilitySet, ProcessResult, VimHost, VimSession, WindowNavAction,
};
pub use host_defaults::ExpectedResultKind;
pub use host_notification::HostNotification;
pub use pipeline::{PipelineError, PlannedAction};
pub use replay::{ReplayEntry, SessionRecorder, SessionReplayer};
pub use response::{Response, ResponseKind};
pub mod trace;

// Shadow document — re-exported for embedding shells that keep their own
// copy of the buffer text.
pub use engine::shadow_document::{LineIndex, OwnedDocument, ShadowDocument};

pub use engine::vim_text_document::VimTextDocument;

// Predictive pre-computation API.
pub use predictive::{
    Prediction, PredictionCategories, PredictionConfig, PredictionTrigger, PredictionWeights,
};

// Integration utilities for direct VimEngine consumers.
pub use crate::dispatch::ViewportInfo;
pub use utilities::{drain_pending_keys, process_host_requests, CycleTextCache, DrainResult};
