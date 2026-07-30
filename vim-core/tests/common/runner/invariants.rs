//! Automatic invariant validation for fidelity tests.
//!
//! Two classes of checks:
//! - **Per-key**: run after every `engine.process()` call.
//! - **Per-test**: run once after all keys are processed and auto-escape is done.

use crate::common::document::TestDocument;
use vim_core::document::Document;
use vim_core::effects::{validate_ordering, Effect, EffectKind, EffectTier};
use vim_core::execution::VimEngine;
use vim_core::primitives::Mode;

/// Per-key invariant checks.
///
/// Called after effects have been applied (so cursor/doc state is updated)
/// but with a snapshot of the effects from the response.
pub fn check_per_key(engine: &VimEngine, doc: &TestDocument, effects: &[Effect], key_repr: &str) {
    let text = doc.text();
    let cursor = doc.cursor_offset();
    let mode = engine.mode();

    // 1. Undo group balance — skip during macro merge.
    //
    // Undo groups span multiple keypresses (open on Insert entry, close on
    // <Esc>). Within a single keypress, we can see:
    //   - BeginUndoGroup without EndUndoGroup (entering Insert/Replace)
    //   - EndUndoGroup without BeginUndoGroup (leaving Insert/Replace)
    // Both are valid. We check that the nesting depth never drops below -1
    // (at most one unmatched End per keypress, closing a prior group).
    if !engine.state().undo_tree().is_merging() {
        check_undo_depth(effects, key_repr);
    }

    // 2. Cursor within bounds.
    //
    // cursor == doc_len is valid when:
    //   - Insert/Replace/Visual mode (appending / selecting past last char)
    //   - Text ends with '\n' (cursor on the empty trailing line)
    let doc_len = text.len();
    let allows_past_end = matches!(
        mode,
        Mode::Insert | Mode::Replace | Mode::VirtualReplace | Mode::Visual(_)
    );
    let on_trailing_empty_line =
        cursor == doc_len && doc_len > 0 && text.as_bytes()[doc_len - 1] == b'\n';
    if doc_len == 0 {
        assert_eq!(
            cursor, 0,
            "INVARIANT VIOLATION: cursor={cursor} but document is empty (after key {key_repr:?})"
        );
    } else if cursor > doc_len {
        panic!(
            "INVARIANT VIOLATION: cursor={cursor} > doc_len={doc_len} \
             (after key {key_repr:?}, mode: {mode:?})"
        );
    } else if cursor == doc_len && !allows_past_end && !on_trailing_empty_line {
        // cursor == doc_len in Normal mode is a transient state that can occur
        // when exiting Visual mode. The engine's validate_clamped() fixes it
        // on the next key. Only warn, don't panic — cursor > doc_len is the
        // truly invariant-violating case.
        eprintln!(
            "INVARIANT[cursor-bounds] note: cursor={cursor} == doc_len={doc_len} in {mode:?} \
             (after key {key_repr:?}) — transient, clamped on next input"
        );
    }

    // 3. Cursor on char boundary
    if cursor < doc_len {
        assert!(
            text.is_char_boundary(cursor),
            "INVARIANT VIOLATION: cursor={cursor} is not on a char boundary \
             (after key {key_repr:?})"
        );
    }

    // 4. Effect ordering.
    //
    // `validate_ordering` internally checks undo group pairing, which returns
    // UndoGroupMismatch for cross-keypress groups (Insert/Replace entry/exit).
    // We tolerate UndoGroupMismatch since check #1 handles undo depth directly.
    // Other ordering errors (EditAfterFinalCursor, EmptyUndoGroup) are real bugs.
    if let Err(err) = validate_ordering(effects) {
        use vim_core::effects::OrderingError;
        if !matches!(err, OrderingError::UndoGroupMismatch) {
            panic!(
                "INVARIANT VIOLATION: effect ordering after key {key_repr:?}\n\
                 {err:?}\n\
                 effects: {effects:?}"
            );
        }
    }

    // 5. No pure-internal effects leaked to response.
    //
    // Internal-tier effects are consumed by the engine's effect processor.
    // However, some Internal effects are also passed through to hosts for
    // state replication (marks, jumplists, sticky column, etc.). These are
    // the "Internal+Passthrough" variants and are allowed.
    //
    // Pure-internal effects (recording, ext state, syntax selection)
    // should never appear in a response — their presence indicates the
    // effect processor failed to consume them.
    check_no_internal_leak(effects, key_repr);
}

/// Per-test invariant checks.
///
/// Called once after all keys have been processed and auto-escape is done,
/// before `doc.to_golden_state()`.
pub fn check_per_test(engine: &VimEngine, doc: &TestDocument) {
    let text = doc.text();
    let cursor = doc.cursor_offset();
    let mode = engine.mode();

    // 5. Cursor not on '\n' — unless the line is empty or we're in Visual mode.
    //
    // In Visual mode (especially Visual Block with `$`), the cursor can
    // legitimately sit on '\n' to indicate "end of line" selection.
    // Only check this in Normal mode.
    if mode == Mode::Normal
        && !text.is_empty()
        && cursor < text.len()
        && text.as_bytes()[cursor] == b'\n'
    {
        // Find the start of the current line
        let line_start = text[..cursor].rfind('\n').map(|pos| pos + 1).unwrap_or(0);
        let line_content = &text[line_start..cursor];
        if !line_content.is_empty() {
            panic!(
                "INVARIANT VIOLATION: cursor on '\\n' at offset {cursor} \
                 but line is not empty (line content: {line_content:?}, mode: {mode:?})"
            );
        }
    }

    // 6. Mode/selection consistency
    let has_selection = doc.selection().is_some();
    match mode {
        Mode::Visual(_) => {
            // Visual mode must have selection
            assert!(
                has_selection,
                "INVARIANT VIOLATION: Visual mode but no selection set"
            );
        }
        Mode::Normal => {
            // Normal mode must not have selection
            assert!(
                !has_selection,
                "INVARIANT VIOLATION: Normal mode but selection is present \
                 (anchor={:?}, head={:?})",
                doc.selection().map(|s| s.anchor().get()),
                doc.selection().map(|s| s.head().get()),
            );
        }
        // Other modes: no constraint
        _ => {}
    }
}

/// Check undo group depth within a single keypress's effects.
///
/// Undo groups span multiple keypresses (e.g. Insert mode entry opens,
/// `<Esc>` closes). Within one keypress:
/// - depth can go to +N (N unclosed opens): entering Insert/Replace
/// - depth can go to -1 (one unmatched close): leaving Insert/Replace
/// - depth should never drop below -1 (multiple unmatched closes is a bug)
fn check_undo_depth(effects: &[Effect], key_repr: &str) {
    let mut depth: i32 = 0;
    let mut min_depth: i32 = 0;
    for effect in effects {
        match effect {
            Effect::BeginUndoGroup { .. } => depth += 1,
            Effect::EndUndoGroup { .. } => {
                depth -= 1;
                min_depth = min_depth.min(depth);
            }
            _ => {}
        }
    }
    if min_depth < -1 {
        panic!(
            "INVARIANT VIOLATION: undo group depth dropped to {min_depth} \
             after key {key_repr:?} (multiple unmatched EndUndoGroup)\n\
             effects: {effects:?}"
        );
    }
}

/// Internal+Passthrough whitelist: Internal-tier effects that are intentionally
/// forwarded to hosts for state replication / UI updates.
const INTERNAL_PASSTHROUGH: &[EffectKind] = &[
    EffectKind::SaveLastVisual,
    EffectKind::SetLastFind,
    EffectKind::SetLastSubstitute,
    EffectKind::SetLastSubstituteFlags,
    EffectKind::SetSubstitutePattern,
    EffectKind::PushJumpList,
    EffectKind::JumpOlder,
    EffectKind::JumpNewer,
    EffectKind::ChangelistOlder,
    EffectKind::ChangelistNewer,
    EffectKind::SetMark,
    EffectKind::ClearMark,
    EffectKind::SetStickyColumn,
    EffectKind::SetSubstituteConfirmState,
    EffectKind::ClearSubstituteConfirmState,
    EffectKind::Noop,
];

/// Check that no pure-internal effects leaked into the response.
///
/// Internal-tier effects that are NOT in the passthrough whitelist should
/// have been consumed by the engine's effect processor and must not appear
/// in the response visible to hosts.
fn check_no_internal_leak(effects: &[Effect], key_repr: &str) {
    for effect in effects {
        let kind = effect.kind();
        if kind.tier() == EffectTier::Internal && !INTERNAL_PASSTHROUGH.contains(&kind) {
            panic!(
                "INVARIANT VIOLATION: pure-internal effect {kind:?} leaked to response \
                 after key {key_repr:?}\n\
                 This effect should have been consumed by the effect processor.\n\
                 effects: {effects:?}"
            );
        }
    }
}
