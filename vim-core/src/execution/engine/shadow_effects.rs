//! Effect classification for shadow execution.
//!
//! Maps every [`Effect`] variant to an [`EffectCategory`] that the shadow
//! execution loop uses to route effects. The match is exhaustive with no
//! wildcard catch-all, so adding a new `Effect` variant without classifying
//! it produces a compile error.

use ahash::AHashMap as HashMap;

use crate::effects::Effect;

/// How the shadow execution loop should route an [`Effect`].
///
/// - `ShadowApply` — text mutation, applied to `OwnedDocument`.
/// - `CursorUpdate` — cursor/selection change, tracked by shadow loop.
/// - `PassThrough` — accumulated and delivered to host after replay.
/// - `Intercepted` — consumed by `process_effects` before reaching loop.
/// - `HostRequired` — needs host round-trip, aborts shadow execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::execution::engine) enum EffectCategory {
    /// Text mutation — apply to `OwnedDocument`.
    ShadowApply,
    /// Cursor/selection change — update shadow cursor tracker.
    CursorUpdate,
    /// Safe to accumulate — deliver to host after shadow execution.
    PassThrough,
    /// Already consumed by `process_effects` — never reaches shadow loop.
    Intercepted,
    /// Requires host round-trip — abort shadow execution.
    HostRequired,
}

/// Classify an [`Effect`] for the shadow execution loop.
///
/// The match is exhaustive over all 97 `Effect` variants. Each arm uses
/// `{ .. }` field patterns for forward-compatibility with field additions.
/// There is intentionally **no** wildcard `_` catch-all — the compiler
/// enforces that every variant is explicitly classified.
pub(in crate::execution::engine) const fn classify_effect(effect: &Effect) -> EffectCategory {
    match effect {
        // ── ShadowApply: text mutations ──────────────────────────────────
        Effect::Insert { .. } => EffectCategory::ShadowApply,
        Effect::Delete { .. } => EffectCategory::ShadowApply,
        Effect::Replace { .. } => EffectCategory::ShadowApply,

        // ── CursorUpdate: cursor / selection state ───────────────────────
        Effect::SetCursor { .. } => EffectCategory::CursorUpdate,
        Effect::SetSelection { .. } => EffectCategory::CursorUpdate,
        Effect::ClearSelection => EffectCategory::CursorUpdate,

        // ── PassThrough: safe to accumulate ──────────────────────────────
        // Register / mark / mode
        Effect::SetRegister { .. } => EffectCategory::PassThrough,
        Effect::SetMark { .. } => EffectCategory::PassThrough,
        Effect::SetMode { .. } => EffectCategory::PassThrough,
        Effect::BeginInsert { .. } => EffectCategory::PassThrough,
        Effect::CommandLineEdit(_) => EffectCategory::PassThrough,
        Effect::SaveLastVisual { .. } => EffectCategory::PassThrough,
        Effect::SetBlockInsert { .. } => EffectCategory::PassThrough,
        // Changelist navigation
        Effect::ChangelistOlder { .. } => EffectCategory::PassThrough,
        Effect::ChangelistNewer { .. } => EffectCategory::PassThrough,
        // Last-command state
        Effect::SetLastSubstitute { .. } => EffectCategory::PassThrough,
        Effect::SetLastSubstituteFlags { .. } => EffectCategory::PassThrough,
        Effect::SetSubstitutePattern { .. } => EffectCategory::PassThrough,
        Effect::SetLastFind { .. } => EffectCategory::PassThrough,
        Effect::SetScrollHalfCount { .. } => EffectCategory::PassThrough,
        Effect::SetStickyColumn { .. } => EffectCategory::PassThrough,
        // Messages / UI
        Effect::Bell => EffectCategory::PassThrough,
        Effect::ShowInfo { .. } => EffectCategory::PassThrough,
        Effect::ShowWarning { .. } => EffectCategory::PassThrough,
        Effect::ShowError { .. } => EffectCategory::PassThrough,
        Effect::ClearMessage => EffectCategory::PassThrough,
        // Scroll / viewport
        Effect::ScrollTo { .. } => EffectCategory::PassThrough,
        Effect::CenterCursor => EffectCategory::PassThrough,
        Effect::CursorToTop => EffectCategory::PassThrough,
        Effect::CursorToBottom => EffectCategory::PassThrough,
        Effect::ScrollLeft { .. } => EffectCategory::PassThrough,
        Effect::ScrollRight { .. } => EffectCategory::PassThrough,
        Effect::ScrollHalfScreenLeft { .. } => EffectCategory::PassThrough,
        Effect::ScrollHalfScreenRight { .. } => EffectCategory::PassThrough,
        Effect::ScrollCursorToLeftEdge => EffectCategory::PassThrough,
        Effect::ScrollCursorToRightEdge => EffectCategory::PassThrough,
        // Jump list
        Effect::PushJumpList { .. } => EffectCategory::PassThrough,
        // Search / highlights
        Effect::SetSearchPattern { .. } => EffectCategory::PassThrough,
        Effect::HighlightMatches { .. } => EffectCategory::PassThrough,
        Effect::ClearHighlights => EffectCategory::PassThrough,
        Effect::SearchMatchInfo { .. } => EffectCategory::PassThrough,
        // Highlight range effects
        Effect::SetHighlightRange { .. } => EffectCategory::PassThrough,
        Effect::ClearHighlightRange { .. } => EffectCategory::PassThrough,
        // Events
        Effect::Event { .. } => EffectCategory::PassThrough,
        // Undo grouping markers
        Effect::BeginUndoGroup { .. } => EffectCategory::PassThrough,
        Effect::EndUndoGroup { .. } => EffectCategory::PassThrough,
        // Window management
        Effect::WindowSplit => EffectCategory::PassThrough,
        Effect::WindowNew => EffectCategory::PassThrough,
        Effect::WindowVSplit => EffectCategory::PassThrough,
        Effect::WindowClose => EffectCategory::PassThrough,
        Effect::WindowOnly => EffectCategory::PassThrough,
        Effect::WindowNext => EffectCategory::PassThrough,
        Effect::WindowPrev => EffectCategory::PassThrough,
        Effect::WindowMoveLeft => EffectCategory::PassThrough,
        Effect::WindowMoveRight => EffectCategory::PassThrough,
        Effect::WindowMoveUp => EffectCategory::PassThrough,
        Effect::WindowMoveDown => EffectCategory::PassThrough,
        Effect::WindowEqualSize => EffectCategory::PassThrough,
        Effect::WindowIncreaseHeight { .. } => EffectCategory::PassThrough,
        Effect::WindowDecreaseHeight { .. } => EffectCategory::PassThrough,
        Effect::WindowIncreaseWidth { .. } => EffectCategory::PassThrough,
        Effect::WindowDecreaseWidth { .. } => EffectCategory::PassThrough,
        Effect::WindowRotateDown => EffectCategory::PassThrough,
        Effect::WindowRotateUp => EffectCategory::PassThrough,
        // Fold operations
        Effect::FoldLine { .. } => EffectCategory::PassThrough,
        Effect::UnfoldLine { .. } => EffectCategory::PassThrough,
        Effect::ToggleFold { .. } => EffectCategory::PassThrough,
        Effect::ToggleFoldRecursive { .. } => EffectCategory::PassThrough,
        Effect::FoldAll => EffectCategory::PassThrough,
        Effect::UnfoldAll => EffectCategory::PassThrough,
        Effect::FoldLineRecursive { .. } => EffectCategory::PassThrough,
        Effect::UnfoldLineRecursive { .. } => EffectCategory::PassThrough,
        Effect::DeleteFold { .. } => EffectCategory::PassThrough,
        Effect::DeleteFoldRecursive { .. } => EffectCategory::PassThrough,
        Effect::EliminateAllFolds => EffectCategory::PassThrough,
        Effect::ToggleFoldEnable => EffectCategory::PassThrough,
        Effect::SetFoldEnable { .. } => EffectCategory::PassThrough,

        // Substitute preview
        Effect::SubstitutePreview { .. } => EffectCategory::PassThrough,
        Effect::ClearSubstitutePreview => EffectCategory::PassThrough,
        // Substitute confirm UI
        Effect::SubstituteConfirmShow { .. } => EffectCategory::PassThrough,
        Effect::SubstituteConfirmEnd => EffectCategory::PassThrough,
        // Virtual text / decoration
        Effect::SetVirtualText { .. } => EffectCategory::PassThrough,
        Effect::ClearVirtualText { .. } => EffectCategory::PassThrough,
        Effect::SetDiagnostics { .. } => EffectCategory::PassThrough,
        // Fold range sync
        Effect::SyncFoldRanges { .. } => EffectCategory::PassThrough,
        // Undo tree visualization
        Effect::UndoTreeSnapshot { .. } => EffectCategory::PassThrough,

        // ── Intercepted: consumed by process_effects ─────────────────────
        Effect::PlayMacro { .. } => EffectCategory::Intercepted,
        Effect::StartRecording { .. } => EffectCategory::Intercepted,
        Effect::StopRecording => EffectCategory::Intercepted,
        // Extension state (engine-internal, consumed by effect processor)
        Effect::SetExtState { .. } => EffectCategory::Intercepted,
        Effect::ClearExtState { .. } => EffectCategory::Intercepted,
        // Syntax selection history (engine-internal, consumed by effect processor)
        Effect::SyntaxSelectionPush { .. } => EffectCategory::Intercepted,
        Effect::SyntaxSelectionPop => EffectCategory::Intercepted,
        Effect::SyntaxHistoryClear => EffectCategory::Intercepted,
        Effect::SetSyntaxSelections { .. } => EffectCategory::Intercepted,
        // Substitute confirm state (engine-internal, consumed by effect processor)
        Effect::SetSubstituteConfirmState { .. } => EffectCategory::Intercepted,
        Effect::ClearSubstituteConfirmState => EffectCategory::Intercepted,

        // Cursor style (emitted alongside mode transitions, safe to accumulate)
        Effect::SetCursorStyle { .. } => EffectCategory::PassThrough,
        // Operator-pending cursor shape hint (pure UI hint, safe to accumulate)
        Effect::CursorShapeHint { .. } => EffectCategory::PassThrough,
        // Insert-mode bracket match flash (pure UI hint, safe to accumulate)
        Effect::ShowMatch { .. } => EffectCategory::PassThrough,

        // ── HostRequired: need host round-trip ───────────────────────────
        Effect::OperatorFilter { .. } => EffectCategory::HostRequired,
        Effect::OperatorReindent { .. } => EffectCategory::HostRequired,
        Effect::HostAction { .. } => EffectCategory::HostRequired,
        Effect::NormCommand { .. } => EffectCategory::HostRequired,
        Effect::CopyToClipboard { .. } => EffectCategory::HostRequired,
        Effect::GotoDefinition => EffectCategory::HostRequired,
        Effect::ShowDocumentation => EffectCategory::HostRequired,
        Effect::Undo { .. } => EffectCategory::HostRequired,
        Effect::Redo { .. } => EffectCategory::HostRequired,
        Effect::UndoLine { .. } => EffectCategory::HostRequired,
        Effect::OperatorToMark { .. } => EffectCategory::HostRequired,
        Effect::JumpOlder { .. } => EffectCategory::PassThrough,
        Effect::JumpNewer { .. } => EffectCategory::PassThrough,
        Effect::JumpToBuffer { .. } => EffectCategory::HostRequired,
        Effect::OpenCommandWindow { .. } => EffectCategory::HostRequired,
        Effect::CallOperatorFunc { .. } => EffectCategory::HostRequired,

        // Multi-cursor & syntax selection — safe to accumulate
        Effect::HighlightRows { .. } => EffectCategory::PassThrough,
        Effect::SetBlockSelections { .. } => EffectCategory::PassThrough,
        Effect::SaveSelections { .. } => EffectCategory::PassThrough,
        Effect::RestoreSelections { .. } => EffectCategory::PassThrough,
        Effect::SelectNextMatch { .. } => EffectCategory::PassThrough,
        Effect::SelectPreviousMatch { .. } => EffectCategory::PassThrough,

        // No-op / register and mark clearing — safe to pass through
        Effect::Noop => EffectCategory::PassThrough,
        Effect::ClearNamedRegister { .. } => EffectCategory::PassThrough,
        Effect::ClearMark { .. } => EffectCategory::PassThrough,

        // Variable store — consumed by effect processor, never reaches host
        Effect::SetVariable { .. } | Effect::DeleteVariable { .. } => EffectCategory::Intercepted,

        // Cross-buffer edit — requires host to route to target buffer
        Effect::CrossBufferEdit { .. } => EffectCategory::HostRequired,

        // Atomic mode transition — pass through (contains mode + cursor info)
        Effect::ModeTransition { .. } => EffectCategory::PassThrough,

        // Timer request — pass through (host manages timers)
        Effect::RequestTimer { .. } => EffectCategory::PassThrough,
    }
}

/// Coalesce key for deduplication grouping.
///
/// Effects that share the same key are deduplicated by keeping only the last
/// occurrence. Effects that return `None` from [`coalesce_key`] are always
/// kept (they cannot be safely merged).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum CoalesceKey {
    Message,
    ScrollPosition,
    HorizontalScroll,
    Highlight,
    SearchInfo,
    SearchPattern,
    Mode,
    ScrollHalf,
    StickyColumn,
    SubstitutePreview,
    VirtualText(u32),
    Diagnostics(u32),
    FoldRanges,
    UndoTreeSnapshot,
    CursorStyle,
}

/// Return the coalesce key for an effect, or `None` if it cannot be deduped.
///
/// Effects sharing a key are subject to "last wins" deduplication.
const fn coalesce_key(effect: &Effect) -> Option<CoalesceKey> {
    match effect {
        Effect::ShowInfo { .. }
        | Effect::ShowWarning { .. }
        | Effect::ShowError { .. }
        | Effect::ClearMessage => Some(CoalesceKey::Message),

        Effect::ScrollTo { .. }
        | Effect::CenterCursor
        | Effect::CursorToTop
        | Effect::CursorToBottom => Some(CoalesceKey::ScrollPosition),

        Effect::ScrollLeft { .. } | Effect::ScrollRight { .. } => {
            Some(CoalesceKey::HorizontalScroll)
        }

        Effect::HighlightMatches { .. } | Effect::ClearHighlights => Some(CoalesceKey::Highlight),

        Effect::SearchMatchInfo { .. } => Some(CoalesceKey::SearchInfo),
        Effect::SetSearchPattern { .. } => Some(CoalesceKey::SearchPattern),
        Effect::SetMode { .. } => Some(CoalesceKey::Mode),
        Effect::SetScrollHalfCount { .. } => Some(CoalesceKey::ScrollHalf),
        Effect::SetStickyColumn { .. } => Some(CoalesceKey::StickyColumn),

        Effect::SubstitutePreview { .. } | Effect::ClearSubstitutePreview => {
            Some(CoalesceKey::SubstitutePreview)
        }

        Effect::SetVirtualText { namespace, .. } | Effect::ClearVirtualText { namespace, .. } => {
            Some(CoalesceKey::VirtualText(*namespace))
        }

        Effect::SetDiagnostics { namespace, .. } => Some(CoalesceKey::Diagnostics(*namespace)),

        Effect::SyncFoldRanges { .. } => Some(CoalesceKey::FoldRanges),

        Effect::UndoTreeSnapshot { .. } => Some(CoalesceKey::UndoTreeSnapshot),

        Effect::SetCursorStyle { .. } => Some(CoalesceKey::CursorStyle),

        _ => None,
    }
}

/// Reduce a vec of effects by merging redundant entries.
///
/// For effects that share a [`CoalesceKey`], only the last occurrence is
/// kept. Effects without a coalesce key are always preserved. Relative
/// ordering of surviving effects is maintained.
///
/// This is the final pass over the deferred buffer accumulated during
/// shadow execution, reducing thousands of redundant effects (e.g. 100
/// `ShowInfo` from `100@q`) down to the minimal set the host needs.
pub(in crate::execution::engine) fn coalesce_effects(effects: Vec<Effect>) -> Vec<Effect> {
    // Track the last-seen index for each coalesce key.
    let mut last_seen: HashMap<CoalesceKey, usize> = HashMap::new();
    for (idx, effect) in effects.iter().enumerate() {
        if let Some(key) = coalesce_key(effect) {
            last_seen.insert(key, idx);
        }
    }

    // Keep effects where either they have no key or their index is the last.
    effects
        .into_iter()
        .enumerate()
        .filter(|(idx, effect)| match coalesce_key(effect) {
            None => true,
            Some(key) => last_seen.get(&key) == Some(idx),
        })
        .map(|(_, effect)| effect)
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
#[path = "shadow_effects_tests.rs"]
mod tests;
