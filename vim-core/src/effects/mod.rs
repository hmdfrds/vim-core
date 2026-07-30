#![allow(
    clippy::module_inception,
    reason = "effects::effects re-export is the canonical pattern"
)]
//! Effects system.
//!
//! Pure effects that the shell applies to the editor.
//! The engine produces `Effect` values, the shell applies them.
//!
//! # Layering
//!
//! Imports:
//!
//! | Dependency    | Used by                                           | Justification                                        |
//! |---------------|---------------------------------------------------|------------------------------------------------------|
//! | `primitives`  | `Offset`, `LineNumber`, `Range`, `RegisterName`,  | All effect fields are primitive types                 |
//! |               | `Mode`, `InsertEntryType`, `CommandLineEdit`,     |                                                      |
//! |               | `Operator`, `FindDirection`, `LastFind`,           |                                                      |
//! |               | `LastVisualInfo`, etc.                             |                                                      |
//! | `errors`      | `VimError` (in `ShowError`)                       | Typed errors with Vim error codes for display         |
//!
//! Must not import `state`, `commands`, `execution`, `dispatch`, `mode`,
//! `keymap`, `document` or `grammar`.
//!
//! ```text
//! effects ──► primitives  (Offset, LineNumber, Mode, InsertEntryType, FindDirection, LastVisualInfo, ...)
//!    ├──► errors     (VimError, for ShowError payload)
//!    ✗
//!    └──✗ state, commands, execution, dispatch, mode, keymap, document, grammar (forbidden)
//! ```

/// Algebraic inverses for undo/redo without an undo stack.
pub mod algebra;
/// Bidirectional conversion between [`Effect`] and [`ChangeSet`](crate::primitives::ChangeSet).
pub mod bridge;
mod builder_macros;
/// Machine-verified effect commutativity classification.
pub mod commutativity;
/// Effect composition — fusing adjacent compatible effects.
pub mod compose;
mod effect;
mod effects;
pub mod host_event;
mod info_message;
/// Precondition invariant checks for effects.
pub mod invariants;
/// Composable middleware for the effect pipeline.
pub mod middleware;
mod provenance;
/// Undo lifecycle intent for multi-cursor effect wrapping.
pub mod undo_intent;

pub use algebra::{inverse, make_reversible, InverseContext, ReversibleEffect};
pub use bridge::{changeset_to_effects, effects_to_changeset};
pub use compose::{compose_effects, simplify_effects, try_compose};
pub use effect::{
    Effect, EffectKind, EffectTier, HighlightStyle, SelectionTag, SourceContext,
    HIGHLIGHT_OWNER_YANK,
};
pub use effects::{
    undo_state, validate_ordering, validate_undo_groups_slice, Effects, OrderingError,
};
pub use host_event::HostEvent;
pub use info_message::{InfoMessage, LineModCounts, MessageKind};
pub use invariants::{bounds_check_and_clamp, precondition, verify_effects, violation_message};
pub use middleware::{
    ComposeMiddleware, DeduplicateMiddleware, EffectMiddleware, EffectPipeline, LoggingMiddleware,
    SimplifyMiddleware, VerifyMiddleware,
};
pub use provenance::EffectProvenance;
