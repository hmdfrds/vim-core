//! Canonical OperatorToMark effect handler.
//!
//! Both the orchestrator and the effect_applier need to handle the
//! `Effect::OperatorToMark` variant.  The orchestrator's version is more
//! complete (it has `with_force_numbered()` and Neovim's exclusive-to-linewise
//! promotion logic) and is used here as the canonical implementation.
//!
//! # Engine sync
//!
//! The orchestrator has access to a `VimEngine` reference and must sync the
//! engine's internal state (registers, mode) after the sub-effects are applied.
//! The effect_applier operates without engine access.  Pass `None` for the
//! engine argument when engine sync is not available or not needed.

use crate::common::document::TestDocument;
use vim_core::effects::Effect;
use vim_core::primitives::{MarkName, Offset, Operator, RegisterName};

/// Apply an `OperatorToMark` effect to `doc`, optionally syncing `engine`.
///
/// This is the single canonical implementation used by both `orchestrator.rs`
/// and `effect_applier.rs`.
///
/// # Parameters
///
/// - `doc`      – mutable reference to the test document.
/// - `engine`   – optional engine reference for state sync (registers, mode).
///   Pass `None` when engine access is unavailable.
/// - `operator` – the vim operator to execute (yank, delete, change, …).
/// - `mark`     – the mark name that serves as the motion target.
/// - `linewise` – `true` for `'a`-style (linewise), `false` for `` `a ``-style
///   (charwise / exclusive).
/// - `register` – target register, or `None` for the unnamed register.
/// - `cursor`   – byte offset of the cursor at the time the command was issued.
pub fn apply_operator_to_mark(
    doc: &mut TestDocument,
    engine: Option<&mut vim_core::execution::VimEngine>,
    operator: Operator,
    mark: MarkName,
    linewise: bool,
    register: Option<RegisterName>,
    cursor: Offset,
) {
    use super::apply_effect;
    use vim_core::commands::operators::OperatorContext;
    use vim_core::dispatch::dispatch_operator;
    use vim_core::document::Document as _;
    use vim_core::primitives::Mode;
    use vim_core::primitives::{MotionType, Range};

    let Some(mark_offset) = doc.get_mark(mark.char()) else {
        return;
    };

    let text = doc.text();

    let (start, end) = if cursor.get() < mark_offset {
        (cursor.get(), mark_offset)
    } else {
        (mark_offset, cursor.get())
    };

    let (range, motion_type) = if linewise {
        // Extend start and end to full line boundaries.
        let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
        let line_end = text[end..].find('\n').map_or(text.len(), |i| end + i + 1);
        (Range::from_raw(line_start, line_end), MotionType::LineWise)
    } else {
        // Apply Neovim's exclusive-to-linewise promotion:
        // backtick marks are exclusive, so when `end` lands at column 0
        // and `start` is at or before the first non-blank of its line,
        // Neovim promotes the motion to linewise.
        let end_at_col0 = end > start
            && (end == 0
                || text.as_bytes().get(end.wrapping_sub(1)) == Some(&b'\n')
                || end == text.len() && start == 0);
        let promote = if end_at_col0 && end > 0 && text.as_bytes().get(end - 1) == Some(&b'\n') {
            let line_start_off = text[..start].rfind('\n').map_or(0, |i| i + 1);
            let line_text = &text[line_start_off
                ..text[line_start_off..]
                    .find('\n')
                    .map_or(text.len(), |i| line_start_off + i)];
            let fnb = line_text.chars().take_while(|c| c.is_whitespace()).count();
            start <= line_start_off + fnb
        } else {
            false
        };

        if promote {
            let line_start_off = text[..start].rfind('\n').map_or(0, |i| i + 1);
            let backed_up_end = end - 1;
            let le = text[backed_up_end..]
                .find('\n')
                .map_or(text.len(), |i| backed_up_end + i + 1);
            (Range::from_raw(line_start_off, le), MotionType::LineWise)
        } else {
            (Range::from_raw(start, end), MotionType::CharWise)
        }
    };

    let op_ctx =
        OperatorContext::new(text, range, motion_type, register, 1, cursor).with_force_numbered();

    let result = dispatch_operator(operator, &op_ctx);

    if let Some(eng) = engine {
        // Orchestrator path: sync engine state in addition to applying to doc.
        let mut pending_mode: Option<Mode> = None;
        for sub_effect in result.effects {
            eng.apply_effect(&sub_effect);
            if let Effect::SetMode { mode, .. } = sub_effect {
                pending_mode = Some(mode);
                apply_effect(doc, sub_effect);
            } else {
                apply_effect(doc, sub_effect);
            }
        }
        if let Some(mode) = pending_mode {
            eng.set_mode(mode);
        }
    } else {
        // effect_applier path: doc-only application.
        for sub_effect in result.effects {
            apply_effect(doc, sub_effect);
        }
    }
}
