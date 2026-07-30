//! Composable effect middleware system.
//!
//! Provides a trait-based pipeline for transforming effects before they reach
//! the host. Hosts register middleware on the engine; effects pass through
//! the chain before reaching the host. This enables logging, batching,
//! deduplication, and custom transformations without modifying core code.
//!
//! # Built-in middleware
//!
//! - [`LoggingMiddleware`] — captures `Debug` representations for diagnostics
//! - [`DeduplicateMiddleware`] — wraps [`Effects::optimize_vec`] as middleware
//! - [`ComposeMiddleware`] — fuses adjacent compatible effects via [`compose_effects`](crate::effects::compose::compose_effects)
//! - [`SimplifyMiddleware`] — removes non-adjacent redundancies via [`simplify_effects`](crate::effects::compose::simplify_effects)
//!
//! # Example
//!
//! ```ignore
//! let mut pipeline = EffectPipeline::new();
//! pipeline.push(DeduplicateMiddleware);
//! pipeline.push(LoggingMiddleware::new());
//!
//! let mut effects = vec![Effect::set_cursor(Offset::new(5))];
//! pipeline.run(&mut effects);
//! ```

use std::sync::{Arc, Mutex};

use crate::effects::compose::{compose_effects, simplify_effects};
use crate::effects::effect::Effect;
use crate::effects::effects::Effects;
use crate::effects::invariants::verify_effects;
use smallvec::SmallVec;

/// Middleware that transforms effects before they reach the host.
///
/// Hosts register middleware on the engine; effects pass through the
/// chain before reaching the host. This enables logging, batching,
/// deduplication, and custom transformations without modifying core code.
pub trait EffectMiddleware: std::fmt::Debug + Send {
    /// Process and optionally transform the effects list.
    ///
    /// Implementations may add, remove, reorder, or replace effects.
    fn process(&self, effects: &mut SmallVec<[Effect; 4]>);
}

/// An ordered chain of middleware that effects pass through.
///
/// Effects are passed through each middleware in insertion order.
/// An empty pipeline is a no-op.
#[derive(Debug, Default)]
pub struct EffectPipeline {
    middleware: Vec<Box<dyn EffectMiddleware>>,
}

impl EffectPipeline {
    /// Create an empty pipeline with no middleware.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self {
            middleware: Vec::new(),
        }
    }

    /// Append a middleware to the end of the pipeline.
    ///
    /// Middleware runs in insertion order: the first pushed runs first.
    pub fn push(&mut self, mw: impl EffectMiddleware + 'static) {
        self.middleware.push(Box::new(mw));
    }

    /// Run all middleware in order on the given effects list.
    ///
    /// Each middleware's [`EffectMiddleware::process`] is called sequentially,
    /// passing the (possibly modified) effects list from one to the next.
    pub fn run(&self, effects: &mut SmallVec<[Effect; 4]>) {
        for mw in &self.middleware {
            mw.process(effects);
        }
    }

    /// Return the number of middleware in the pipeline.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.middleware.len()
    }

    /// Return `true` if the pipeline contains no middleware.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.middleware.is_empty()
    }
}

/// Middleware that logs each effect's `Debug` representation.
///
/// Stores log entries in a shared `Arc<Mutex<Vec<String>>>` so callers
/// can inspect what effects passed through the pipeline. Useful for
/// debugging and test assertions.
#[derive(Debug, Clone)]
pub struct LoggingMiddleware {
    log: Arc<Mutex<Vec<String>>>,
}

impl LoggingMiddleware {
    /// Create a new logging middleware with an empty log buffer.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self {
            log: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Return a clone of all log entries captured so far.
    #[must_use]
    pub fn log(&self) -> Vec<String> {
        self.log
            .lock()
            .map_or_else(|_| Vec::new(), |guard| guard.clone())
    }
}

impl Default for LoggingMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl EffectMiddleware for LoggingMiddleware {
    fn process(&self, effects: &mut SmallVec<[Effect; 4]>) {
        if let Ok(mut guard) = self.log.lock() {
            for effect in effects.iter() {
                guard.push(format!("{effect:?}"));
            }
        }
    }
}

/// Middleware that removes redundant effects using [`Effects::optimize_vec`].
///
/// This is a thin wrapper that makes the existing optimization logic
/// available as a composable middleware in the pipeline.
///
/// Optimizations performed (delegated to `Effects::optimize_vec`):
/// 1. Deduplicate trailing `SetCursor` effects, keeping only the last.
/// 2. Remove `ClearMessage` when immediately followed by `ShowInfo`.
/// 3. Coalesce consecutive `ShowInfo` effects, keeping only the last in each run.
#[derive(Debug, Clone, Copy, Default)]
pub struct DeduplicateMiddleware;

impl EffectMiddleware for DeduplicateMiddleware {
    fn process(&self, effects: &mut SmallVec<[Effect; 4]>) {
        Effects::<crate::effects::undo_state::Closed>::optimize_vec(effects);
    }
}

/// Middleware that fuses adjacent compatible effects into single effects.
///
/// This is a thin wrapper around [`compose_effects`] that makes the
/// composition logic available as a composable middleware in the pipeline.
///
/// Composition rules (see [`crate::effects::compose`] for details):
/// 1. Adjacent forward inserts fuse into one `Insert`.
/// 2. Adjacent same-start deletes fuse into one `Delete`.
/// 3. Consecutive `SetCursor` effects collapse to the last.
/// 4. Consecutive `SetMode` effects collapse to the last.
#[derive(Debug, Clone, Copy, Default)]
pub struct ComposeMiddleware;

impl EffectMiddleware for ComposeMiddleware {
    fn process(&self, effects: &mut SmallVec<[Effect; 4]>) {
        compose_effects(effects);
    }
}

/// Middleware that performs non-local simplification of effects.
///
/// This is a thin wrapper around [`simplify_effects`] that makes the
/// simplification logic available as a composable middleware in the pipeline.
///
/// Simplification rules (see [`crate::effects::compose::simplify_effects`] for details):
/// 1. Only the last `SetCursor` is kept — all earlier ones are removed.
/// 2. Only the last `SetMode` is kept — all earlier ones are removed.
/// 3. Empty undo groups (`BeginUndoGroup` ... `EndUndoGroup` with no text
///    mutations between) are removed entirely.
#[derive(Debug, Clone, Copy, Default)]
pub struct SimplifyMiddleware;

impl EffectMiddleware for SimplifyMiddleware {
    fn process(&self, effects: &mut SmallVec<[Effect; 4]>) {
        simplify_effects(effects);
    }
}

/// Middleware that verifies effect invariants in debug builds.
///
/// Checks every effect against its precondition (e.g., offsets within document
/// bounds) and panics with a detailed diagnostic on the first violation.
///
/// In release builds (`cfg!(debug_assertions)` is `false`), this is a no-op.
/// Designed to be placed early in the pipeline so invalid effects are caught
/// before any transformation or delivery to the host.
///
/// # Example
///
/// ```ignore
/// let mut pipeline = EffectPipeline::new();
/// pipeline.push(VerifyMiddleware::new(doc_len));
/// pipeline.push(DeduplicateMiddleware);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct VerifyMiddleware {
    doc_len: usize,
}

impl VerifyMiddleware {
    /// Create a new verify middleware for a document of `doc_len` bytes.
    #[inline]
    #[must_use]
    pub const fn new(doc_len: usize) -> Self {
        Self { doc_len }
    }
}

impl EffectMiddleware for VerifyMiddleware {
    fn process(&self, effects: &mut SmallVec<[Effect; 4]>) {
        if cfg!(debug_assertions) {
            verify_effects(effects, self.doc_len);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ComposeMiddleware, DeduplicateMiddleware, EffectMiddleware, EffectPipeline,
        LoggingMiddleware, SimplifyMiddleware, VerifyMiddleware,
    };
    use crate::effects::effect::Effect;
    use crate::primitives::{Mode, Offset, Range};
    use compact_str::CompactString;
    use smallvec::SmallVec;

    /// Helper: build a `SmallVec<[Effect; 4]>` from a vec literal.
    fn sv(v: Vec<Effect>) -> SmallVec<[Effect; 4]> {
        SmallVec::from_vec(v)
    }

    // ── LoggingMiddleware ──────────────────────────────────────────────

    #[test]
    fn test_logging_middleware_captures() {
        let mw = LoggingMiddleware::new();
        let mut effects = sv(vec![
            Effect::set_cursor(Offset::new(10)),
            Effect::set_mode(Mode::Insert),
        ]);

        mw.process(&mut effects);

        let log = mw.log();
        assert_eq!(log.len(), 2);
        assert!(log.first().is_some_and(|s| s.contains("SetCursor")));
        assert!(log.get(1).is_some_and(|s| s.contains("SetMode")));

        // Effects are not modified by logging.
        assert_eq!(effects.len(), 2);
    }

    // ── DeduplicateMiddleware ──────────────────────────────────────────

    #[test]
    fn test_deduplicate_middleware() {
        let mw = DeduplicateMiddleware;

        // Two trailing SetCursor effects — the first should be removed.
        let mut effects = sv(vec![
            Effect::set_cursor(Offset::new(5)),
            Effect::set_cursor(Offset::new(10)),
        ]);

        mw.process(&mut effects);

        // Only the last SetCursor should remain.
        assert_eq!(effects.len(), 1);
        assert_eq!(effects.first(), Some(&Effect::set_cursor(Offset::new(10))));
    }

    // ── EffectPipeline ─────────────────────────────────────────────────

    #[test]
    fn test_pipeline_runs_in_order() {
        // Build a pipeline: deduplicate first, then log.
        // If dedup runs first, it removes the redundant SetCursor.
        // Then logging should only see the surviving effect.
        let logger = LoggingMiddleware::new();
        let logger_clone = logger.clone();

        let mut pipeline = EffectPipeline::new();
        pipeline.push(DeduplicateMiddleware);
        pipeline.push(logger);

        let mut effects = sv(vec![
            Effect::set_cursor(Offset::new(5)),
            Effect::set_cursor(Offset::new(10)),
        ]);

        pipeline.run(&mut effects);

        // Dedup ran first, removing the redundant SetCursor.
        assert_eq!(effects.len(), 1);
        // Logger ran second, so it only captured the single surviving effect.
        let log = logger_clone.log();
        assert_eq!(log.len(), 1);
        assert!(log.first().is_some_and(|s| s.contains("SetCursor")));
    }

    #[test]
    fn test_empty_pipeline_is_noop() {
        let pipeline = EffectPipeline::new();
        assert!(pipeline.is_empty());
        assert_eq!(pipeline.len(), 0);

        let mut effects = sv(vec![
            Effect::set_cursor(Offset::new(5)),
            Effect::set_mode(Mode::Normal),
            Effect::ClearMessage,
        ]);
        let original = effects.clone();

        pipeline.run(&mut effects);

        assert_eq!(effects, original);
    }

    // ── Supplementary tests ────────────────────────────────────────────

    #[test]
    fn test_logging_middleware_default() {
        let mw = LoggingMiddleware::default();
        assert!(mw.log().is_empty());
    }

    #[test]
    fn test_pipeline_len_tracks_pushes() {
        let mut pipeline = EffectPipeline::new();
        assert_eq!(pipeline.len(), 0);
        assert!(pipeline.is_empty());

        pipeline.push(DeduplicateMiddleware);
        assert_eq!(pipeline.len(), 1);
        assert!(!pipeline.is_empty());

        pipeline.push(LoggingMiddleware::new());
        assert_eq!(pipeline.len(), 2);
    }

    #[test]
    fn test_deduplicate_message_coalescing() {
        let mw = DeduplicateMiddleware;
        let mut effects = sv(vec![Effect::ClearMessage, Effect::show_message("hello")]);

        mw.process(&mut effects);

        // ClearMessage before ShowInfo should be removed.
        assert_eq!(effects.len(), 1);
        assert!(matches!(effects.first(), Some(Effect::ShowInfo { .. })));
    }

    #[test]
    fn test_logging_accumulates_across_calls() {
        let mw = LoggingMiddleware::new();

        let mut effects1 = sv(vec![Effect::ClearMessage]);
        mw.process(&mut effects1);

        let mut effects2 = sv(vec![Effect::set_cursor(Offset::new(0))]);
        mw.process(&mut effects2);

        let log = mw.log();
        assert_eq!(log.len(), 2);
        assert!(log.first().is_some_and(|s| s.contains("ClearMessage")));
        assert!(log.get(1).is_some_and(|s| s.contains("SetCursor")));
    }

    // ── ComposeMiddleware ────────────────────────────────────────────────

    #[test]
    fn test_compose_middleware_fuses_cursors() {
        let mw = ComposeMiddleware;
        let mut effects = sv(vec![
            Effect::set_cursor(Offset::new(5)),
            Effect::set_cursor(Offset::new(10)),
            Effect::set_cursor(Offset::new(15)),
        ]);

        mw.process(&mut effects);

        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0], Effect::set_cursor(Offset::new(15)));
    }

    #[test]
    fn test_compose_middleware_fuses_modes() {
        let mw = ComposeMiddleware;
        let mut effects = sv(vec![
            Effect::set_mode(Mode::Normal),
            Effect::set_mode(Mode::Insert),
        ]);

        mw.process(&mut effects);

        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0], Effect::set_mode(Mode::Insert));
    }

    #[test]
    fn test_compose_middleware_no_change_for_non_composable() {
        let mw = ComposeMiddleware;
        let mut effects = sv(vec![
            Effect::set_cursor(Offset::new(5)),
            Effect::set_mode(Mode::Normal),
            Effect::ClearMessage,
        ]);
        let original = effects.clone();

        mw.process(&mut effects);

        assert_eq!(effects, original);
    }

    #[test]
    fn test_compose_middleware_in_pipeline() {
        // Compose first (fuse cursors), then log.
        let logger = LoggingMiddleware::new();
        let logger_clone = logger.clone();

        let mut pipeline = EffectPipeline::new();
        pipeline.push(ComposeMiddleware);
        pipeline.push(logger);

        let mut effects = sv(vec![
            Effect::set_cursor(Offset::new(1)),
            Effect::set_cursor(Offset::new(2)),
            Effect::set_cursor(Offset::new(3)),
        ]);

        pipeline.run(&mut effects);

        // Compose fused 3 cursors into 1.
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0], Effect::set_cursor(Offset::new(3)));

        // Logger only saw the single fused effect.
        let log = logger_clone.log();
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn test_compose_then_deduplicate_pipeline() {
        // Compose fuses adjacent same-type, dedup handles trailing cursor optimization.
        let mut pipeline = EffectPipeline::new();
        pipeline.push(ComposeMiddleware);
        pipeline.push(DeduplicateMiddleware);

        let mut effects = sv(vec![
            Effect::set_cursor(Offset::new(1)),
            Effect::set_cursor(Offset::new(2)),
            Effect::set_mode(Mode::Normal),
            Effect::set_mode(Mode::Insert),
        ]);

        pipeline.run(&mut effects);

        // Compose: [SetCursor(2), SetMode(Insert)]
        // Dedup: no further changes (no trailing cursor duplication)
        assert_eq!(effects.len(), 2);
        assert_eq!(effects[0], Effect::set_cursor(Offset::new(2)));
        assert_eq!(effects[1], Effect::set_mode(Mode::Insert));
    }

    // ── SimplifyMiddleware ──────────────────────────────────────────────

    #[test]
    fn test_simplify_middleware_removes_earlier_cursors() {
        let mw = SimplifyMiddleware;
        let mut effects = sv(vec![
            Effect::set_cursor(Offset::new(5)),
            Effect::ClearMessage,
            Effect::set_cursor(Offset::new(10)),
        ]);

        mw.process(&mut effects);

        assert_eq!(effects.len(), 2);
        assert_eq!(effects[0], Effect::ClearMessage);
        assert_eq!(effects[1], Effect::set_cursor(Offset::new(10)));
    }

    #[test]
    fn test_simplify_middleware_removes_earlier_modes() {
        let mw = SimplifyMiddleware;
        let mut effects = sv(vec![
            Effect::set_mode(Mode::Insert),
            Effect::ClearMessage,
            Effect::set_mode(Mode::Normal),
        ]);

        mw.process(&mut effects);

        assert_eq!(effects.len(), 2);
        assert_eq!(effects[0], Effect::ClearMessage);
        assert_eq!(effects[1], Effect::set_mode(Mode::Normal));
    }

    #[test]
    fn test_simplify_middleware_removes_empty_undo_group() {
        let mw = SimplifyMiddleware;
        let mut effects = sv(vec![
            Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            },
            Effect::EndUndoGroup { node_id: None },
        ]);

        mw.process(&mut effects);

        assert!(effects.is_empty());
    }

    #[test]
    fn test_simplify_middleware_no_change_for_non_redundant() {
        let mw = SimplifyMiddleware;
        let mut effects = sv(vec![
            Effect::set_cursor(Offset::new(5)),
            Effect::set_mode(Mode::Normal),
            Effect::ClearMessage,
        ]);
        let original = effects.clone();

        mw.process(&mut effects);

        assert_eq!(effects, original);
    }

    #[test]
    fn test_simplify_middleware_in_pipeline() {
        // Simplify first (non-local), then compose (local), then log.
        let logger = LoggingMiddleware::new();
        let logger_clone = logger.clone();

        let mut pipeline = EffectPipeline::new();
        pipeline.push(SimplifyMiddleware);
        pipeline.push(ComposeMiddleware);
        pipeline.push(logger);

        let mut effects = sv(vec![
            Effect::set_cursor(Offset::new(1)),
            Effect::ClearMessage,
            Effect::set_cursor(Offset::new(2)),
            Effect::set_cursor(Offset::new(3)),
        ]);

        pipeline.run(&mut effects);

        // Simplify: removes SetCursor(1) and SetCursor(2), keeps SetCursor(3).
        // Compose: ClearMessage + SetCursor(3) don't compose — stays as is.
        assert_eq!(effects.len(), 2);
        assert_eq!(effects[0], Effect::ClearMessage);
        assert_eq!(effects[1], Effect::set_cursor(Offset::new(3)));

        let log = logger_clone.log();
        assert_eq!(log.len(), 2);
    }

    #[test]
    fn test_full_pipeline_compose_simplify_deduplicate() {
        // All three middlewares together.
        let mut pipeline = EffectPipeline::new();
        pipeline.push(ComposeMiddleware);
        pipeline.push(SimplifyMiddleware);
        pipeline.push(DeduplicateMiddleware);

        let mut effects = sv(vec![
            Effect::set_cursor(Offset::new(1)),
            Effect::set_cursor(Offset::new(2)), // Compose fuses with previous
            Effect::ClearMessage,
            Effect::set_cursor(Offset::new(3)), // Now non-adjacent with composed SetCursor(2)
            Effect::set_mode(Mode::Normal),
            Effect::set_mode(Mode::Insert), // Compose fuses with previous
        ]);

        pipeline.run(&mut effects);

        // After Compose: [SetCursor(2), ClearMessage, SetCursor(3), SetMode(Insert)]
        // After Simplify: SetCursor(2) removed (last SetCursor is 3).
        //   → [ClearMessage, SetCursor(3), SetMode(Insert)]
        // After Dedup: no further changes.
        assert_eq!(effects.len(), 3);
        assert_eq!(effects[0], Effect::ClearMessage);
        assert_eq!(effects[1], Effect::set_cursor(Offset::new(3)));
        assert_eq!(effects[2], Effect::set_mode(Mode::Insert));
    }

    // ── VerifyMiddleware ──────────────────────────────────────────────────

    #[test]
    fn test_verify_middleware_passes_valid_effects() {
        let mw = VerifyMiddleware::new(10);
        let mut effects = sv(vec![
            Effect::Insert {
                offset: Offset::new(5),
                text: CompactString::new("hello"),
            },
            Effect::SetCursor {
                offset: Offset::new(9),
            },
            Effect::set_mode(Mode::Normal),
        ]);

        // Should not panic.
        mw.process(&mut effects);

        // VerifyMiddleware does not modify effects.
        assert_eq!(effects.len(), 3);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Effect invariant violation")]
    fn test_verify_middleware_panics_on_violation() {
        let mw = VerifyMiddleware::new(10);
        let mut effects = sv(vec![Effect::Insert {
            offset: Offset::new(100),
            text: CompactString::new("bad"),
        }]);

        mw.process(&mut effects);
    }

    #[test]
    fn test_verify_middleware_empty_effects() {
        let mw = VerifyMiddleware::new(0);
        let mut effects: SmallVec<[Effect; 4]> = SmallVec::new();

        // Empty stream should not panic.
        mw.process(&mut effects);
        assert!(effects.is_empty());
    }

    #[test]
    fn test_verify_middleware_does_not_modify_effects() {
        let mw = VerifyMiddleware::new(20);
        let mut effects = sv(vec![
            Effect::Delete {
                range: Range::from_raw(5, 10),
            },
            Effect::ClearMessage,
        ]);
        let original = effects.clone();

        mw.process(&mut effects);

        assert_eq!(effects, original);
    }

    #[test]
    fn test_verify_middleware_in_pipeline() {
        // Verify first, then deduplicate.
        let mut pipeline = EffectPipeline::new();
        pipeline.push(VerifyMiddleware::new(20));
        pipeline.push(DeduplicateMiddleware);

        let mut effects = sv(vec![
            Effect::set_cursor(Offset::new(5)),
            Effect::set_cursor(Offset::new(10)),
        ]);

        pipeline.run(&mut effects);

        // Verify passed, then dedup removed the first cursor.
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0], Effect::set_cursor(Offset::new(10)));
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Effect invariant violation")]
    fn test_verify_middleware_in_pipeline_catches_violation() {
        let mut pipeline = EffectPipeline::new();
        pipeline.push(VerifyMiddleware::new(5));
        pipeline.push(DeduplicateMiddleware);

        let mut effects = sv(vec![
            Effect::set_cursor(Offset::new(3)),
            // Invalid: offset 5 is out of bounds for doc_len 5 (must be < 5).
            Effect::set_cursor(Offset::new(5)),
        ]);

        pipeline.run(&mut effects);
    }

    #[test]
    fn test_verify_middleware_debug_trait() {
        let mw = VerifyMiddleware::new(42);
        let debug = format!("{mw:?}");
        assert!(
            debug.contains("VerifyMiddleware"),
            "Debug should contain type name: {debug}",
        );
        assert!(
            debug.contains("42"),
            "Debug should contain doc_len: {debug}",
        );
    }

    #[test]
    fn test_verify_middleware_clone_copy() {
        let mw = VerifyMiddleware::new(100);
        let cloned = mw;
        let _ = cloned; // Test that Copy works.
    }
}
