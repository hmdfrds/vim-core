//! Operator dispatcher.
//!
//! Maps `Grammar::Operator` to `commands::operators` implementations.
//! This is the ONLY place to update when adding operators.
//!
//! # Design
//!
//! Grammar layer has `Operator` enum for parsing.
//! Commands layer has organized implementations (delete.rs, yank.rs, etc).
//! This dispatcher bridges them via exhaustive match.
//!
//! # Adding New Operators
//!
//! 1. Add variant to `grammar::Operator` enum
//! 2. Create implementation in `commands/operators/`
//! 3. Add match arm HERE in `dispatch_operator()`

pub use crate::commands::operators::block_visual;
use crate::commands::operators::inclusivity::motion_inclusivity;
pub use crate::commands::operators::range_helpers::{adjust_eof_range, empty_textobject_result};
pub use crate::commands::operators::OperatorContext;
use crate::commands::operators::{case, change, delete, format, indent, yank};
use crate::commands::CommandResult;
use crate::effects::Effects;
use crate::grammar::types::{Motion, Operator};
use crate::primitives::{next_char_boundary, prev_char_boundary};
use crate::primitives::{MotionInclusivity, MotionType, Offset};

/// Dispatch an Operator to the appropriate implementation.
///
/// This is the **exhaustive match** for all operators.
/// Adding a new Operator variant will cause a compile error here.
///
/// No dyn traits in the hot path: exhaustive match dispatch, which is
/// inlinable and allocation-free.
///
/// # Arguments
/// * `op` - The operator from Grammar
/// * `ctx` - Operator context with text, range, register, etc.
///
/// # Returns
/// * `CommandResult` with effects to apply
#[inline]
pub fn dispatch_operator(op: Operator, ctx: &OperatorContext<'_>) -> CommandResult {
    match op {
        Operator::Delete => delete::execute(ctx),
        Operator::Change => change::execute(ctx),
        Operator::Yank => yank::execute(ctx),
        Operator::Indent => indent::execute_indent(ctx),
        Operator::Outdent => indent::execute_outdent(ctx),
        Operator::ToggleCase => case::execute_toggle_case(ctx),
        Operator::Uppercase => case::execute_uppercase(ctx),
        Operator::Lowercase => case::execute_lowercase(ctx),
        Operator::Rot13 => case::execute_rot13(ctx),
        Operator::Rot47 => case::execute_rot47(ctx),
        Operator::Format => format::execute(ctx),
        Operator::FormatKeepCursor => format::execute_keep_cursor(ctx),
        Operator::CallOperatorFunc => CommandResult::effects_only(
            Effects::new().call_operator_func(ctx.range, ctx.motion_type),
        ),
        Operator::Filter => CommandResult::effects_only(Effects::new().operator_filter(
            ctx.range,
            ctx.motion_type,
            Some(ctx.register),
        )),
        Operator::Reindent => {
            // Neovim's op_reindent (indent.c:1052-1056) sets marks:
            //   b_op_start = oap->start  (min(cursor, motion_end))
            //   b_op_end   = oap->end    (max(cursor, motion_end))
            // These are (line, col) positions from before the reindent.
            // We carry the column values so the completion handler can
            // reconstruct the correct byte offsets in the post-reindent text.
            use crate::commands::helpers::column_of;
            let start_offset = ctx.cursor.get().min(ctx.motion_target.get());
            let end_offset = ctx.cursor.get().max(ctx.motion_target.get());
            let start_col = column_of(ctx.text, start_offset);
            let end_col = column_of(ctx.text, end_offset);

            // Compute `end_line_in_range`: the number of newlines from
            // `range.start()` to `oap->end` in the original text.
            //
            // For textobjects, `motion_target` is unset (== cursor).
            // We derive the line count from the range structure:
            //   - Linewise textobjects (i{, ip, etc.): Neovim applies the
            //     exclusive-end adjustment, placing `oap->end` on the last
            //     content line — skip trailing newlines when counting.
            //   - Charwise textobjects (it with CA_NO_ADJ_OP_END): `oap->end`
            //     is at `range.end()` — count all newlines in range.
            //
            // For motions, `oap->end = max(cursor, motion_target)`.
            // Neovim's exclusive-end adjustment (ops.c:3630) decrements
            // `oap->end.lnum` when the motion is charwise-exclusive with
            // col 0 end. We detect this by checking if `end_offset ==
            // range.end()` and `end_col == 0` — meaning the range was
            // NOT linewise-expanded (paragraph motions skip
            // extend_to_full_lines) and ends exactly at a line boundary.
            let range_start = ctx.range.start().get();
            let end_line_in_range =
                if ctx.origin == crate::commands::operators::OperatorOrigin::Motion {
                    // Motion case: end_offset is max(cursor, motion_target)
                    // which precisely tracks oap->end in the original text.
                    let clamped = end_offset.min(ctx.text.len());
                    let raw_count = if clamped >= range_start {
                        ctx.text
                            .get(range_start..clamped)
                            .map_or(0, |s| s.bytes().filter(|&b| b == b'\n').count())
                    } else {
                        0
                    };
                    // Neovim exclusive-end adjustment: when the motion's
                    // end_offset coincides with range.end() and is at col 0,
                    // the exclusive-end adjustment decremented oap->end.lnum.
                    // Paragraph / display motions hit this path because they
                    // skip extend_to_full_lines (exclusive_linewise_motion).
                    if end_offset == ctx.range.end().get() && end_col == 0 && raw_count > 0 {
                        raw_count - 1
                    } else {
                        raw_count
                    }
                } else {
                    // TextObject or Visual: motion_target is unreliable.
                    // Derive end_line_in_range from the range structure.
                    let range_text = ctx
                        .text
                        .get(range_start..ctx.range.end().get().min(ctx.text.len()))
                        .unwrap_or("");
                    let n = range_text.bytes().filter(|&b| b == b'\n').count();
                    if ctx.motion_type == crate::primitives::MotionType::LineWise {
                        // Linewise (visual V, linewise textobjects): oap->end
                        // is on the last content line. The trailing '\n' terminates
                        // that line, not starting a new one.
                        if range_text.ends_with('\n') {
                            n.saturating_sub(1)
                        } else {
                            n
                        }
                    } else {
                        // Charwise (e.g., `=it` with CA_NO_ADJ_OP_END):
                        // oap->end is at range.end() — count all newlines.
                        n
                    }
                };

            // For text objects and visual, oap->start comes from the
            // range itself (not min(cursor, motion_target)). For motions,
            // oap->start = min(cursor, motion_target). This distinction
            // matters for the EOF case where range.start() includes a
            // preceding newline: start_offset correctly points to the
            // content line, while range.start() is one byte earlier.
            let start_byte_offset =
                if ctx.origin == crate::commands::operators::OperatorOrigin::Motion {
                    start_offset
                } else {
                    ctx.range.start().get()
                };

            CommandResult::effects_only(Effects::new().operator_reindent(
                ctx.range,
                ctx.motion_type,
                start_col,
                end_col,
                end_line_in_range,
                start_byte_offset,
            ))
        }
        Operator::Custom(id) => {
            if let Some(provider) = ctx.custom_operators {
                if let Some(result) = provider.compute_operator(
                    id,
                    ctx.text,
                    (ctx.range.start().get(), ctx.range.end().get()),
                    ctx.count,
                ) {
                    return match result {
                        crate::document::CustomOperatorResult::Replace(text) => {
                            CommandResult::effects_only(Effects::new().replace(ctx.range, text))
                        }
                        crate::document::CustomOperatorResult::Delete => {
                            CommandResult::effects_only(Effects::new().delete(ctx.range))
                        }
                        crate::document::CustomOperatorResult::Defer => {
                            CommandResult::effects_only(
                                Effects::new().call_operator_func(ctx.range, ctx.motion_type),
                            )
                        }
                        crate::document::CustomOperatorResult::NoOp => {
                            CommandResult::effects_only(Effects::new())
                        }
                    };
                }
            }
            CommandResult::effects_only(
                Effects::new().call_operator_func(ctx.range, ctx.motion_type),
            )
        }

        // Composed operator: apply two operators to the same range.
        //
        // Ordering policy (to avoid data loss):
        // - Non-destructive first, destructive second: apply non-destructive first,
        //   then destructive (e.g., yank first, then delete).
        // - Destructive first, non-destructive second: apply non-destructive first
        //   so it sees the original text, then apply the destructive op.
        // - Both non-destructive: apply first then second.
        // - Both destructive: rejected in `compose()`, should not reach here.
        Operator::Composed(pair) => {
            use crate::effects::Effects;
            let first = pair.first();
            let second = pair.second();
            // Validate: both-destructive is an error (should have been caught at compose()).
            if first.is_destructive() && second.is_destructive() {
                debug_assert!(
                    false,
                    "Composed with two destructive operators should be rejected"
                );
                return CommandResult::effects_only(Effects::new());
            }
            // Determine application order: non-destructive op sees original text.
            let (op_first, op_second) = if second.is_destructive() {
                // Apply non-destructive (first) then destructive (second).
                (first, second)
            } else if first.is_destructive() {
                // Apply non-destructive (second) first to see original text, then destructive (first).
                (second, first)
            } else {
                // Both non-destructive: apply in declared order.
                (first, second)
            };
            // Apply first operation.
            let r1 = dispatch_operator(op_first, ctx);
            // Apply second operation to the same range/context.
            let r2 = dispatch_operator(op_second, ctx);
            // Merge effects.
            let mut merged = r1.effects;
            merged.extend(r2.effects);
            // Use cursor from second result if available, otherwise first.
            let cursor = r2.cursor.or(r1.cursor);
            CommandResult {
                effects: merged,
                cursor,
            }
        }
    }
}

pub use crate::commands::operators::OperatorMotionInput;

/// Dispatch an operator applied via a motion (e.g., `dw`, `cw`, `y3j`).
///
/// Encapsulates the full flow:
/// 1. Build motion context and resolve range via `compute_motion_range`
/// 2. Apply Vim special cases (`cw→ce`, `yw` cross-line trim)
/// 3. Force numbered registers for jump motions
/// 4. Dispatch operator
///
/// This moves business logic that was previously in executor.rs into the
/// dispatch/commands layer where it belongs.
#[inline]
pub fn dispatch_operator_with_motion(input: &OperatorMotionInput<'_>) -> CommandResult {
    use crate::commands::operators::range::{
        adjust_change_word_range, adjust_yank_word_range, compute_motion_range_with_sticky,
    };
    use crate::dispatch::dispatch_motion;

    let operator = input.operator;
    let OperatorMotionInput {
        motion,
        count,
        register,
        text,
        cursor,
        search,
        last_find,
        options,
        ..
    } = *input;

    let Some(mut range_result) = compute_motion_range_with_sticky(
        text,
        cursor.get(),
        motion,
        count,
        search,
        last_find,
        options,
        dispatch_motion,
        input.viewport,
        input.sticky_column,
    ) else {
        return CommandResult::effects_only(crate::effects::Effects::new());
    };

    // Vim cw→ce special case: strip trailing whitespace for change + word-forward.
    // If was promoted to linewise and trimming removed the newline, revert to charwise.
    if operator == Operator::Change && matches!(motion, Motion::WordForward | Motion::WORDForward) {
        let old_range = range_result.range;
        range_result.range = adjust_change_word_range(text, range_result.range, cursor.get());
        if range_result.range != old_range && range_result.was_promoted {
            range_result.motion_type = MotionType::CharWise;
        }
    }

    // Vim yw/dw special case: when word motion crosses a newline, yank/delete
    // should NOT include the trailing whitespace/newline.
    // If the range was promoted to linewise because w landed at col 0 of the next line,
    // and we then trim it back (removing the newline), revert to charwise.
    if matches!(operator, Operator::Yank | Operator::Delete)
        && matches!(motion, Motion::WordForward | Motion::WORDForward)
    {
        let old_range = range_result.range;
        range_result.range = adjust_yank_word_range(text, range_result.range);
        if range_result.range != old_range && range_result.was_promoted {
            range_result.motion_type = MotionType::CharWise;
        }
    }

    // Fold-aware range expansion: if either end of the range falls inside a
    // closed fold, expand to include the entire fold.
    if let Some(fold) = input.fold_provider {
        use crate::commands::helpers::fold_snap;
        use crate::primitives::Direction;
        let start = fold_snap(
            text,
            range_result.range.start().get(),
            Direction::Backward,
            fold,
        );
        let end = fold_snap(
            text,
            range_result.range.end().get(),
            Direction::Forward,
            fold,
        );
        range_result.range = crate::primitives::Range::from_raw(start, end);
    }

    // Apply motion force override (dv$, dVj, d<C-v>j)
    let motion_type = match input.force_type {
        Some(MotionType::CharWise) => {
            // v-force: toggle inclusive/exclusive per Neovim spec
            let original_inclusivity = motion_inclusivity(motion);
            match original_inclusivity {
                MotionInclusivity::Linewise => {
                    // Linewise → CharWise, no range adjustment
                    MotionType::CharWise
                }
                MotionInclusivity::Exclusive => {
                    // Exclusive → Inclusive: extend range end by 1 char
                    let extended =
                        next_char_boundary(text, range_result.range.end().get()).min(text.len());
                    range_result.range = range_result.range.with_end(Offset::new(extended));
                    MotionType::CharWise
                }
                MotionInclusivity::Inclusive => {
                    // Inclusive → Exclusive: shrink range end by 1 char
                    if range_result.range.end().get() > range_result.range.start().get() {
                        let shrunk = prev_char_boundary(text, range_result.range.end().get());
                        range_result.range = range_result.range.with_end(Offset::new(shrunk));
                    }
                    MotionType::CharWise
                }
            }
        }
        Some(force) => force, // V-force or Ctrl-V force: simple override
        None => range_result.motion_type, // No force: use motion's natural type
    };

    // Vi blank-line delete promotion (Neovim ops.c:742-757): when a charwise
    // delete spans multiple lines and remaining text after start is all
    // whitespace with cursor in indent region, promote to linewise.
    let (final_range, final_motion_type) = {
        use crate::commands::helpers::{
            first_non_blank_in_line, line_end_for_offset, line_of, line_start_for_offset,
        };
        use crate::commands::operators::range_helpers::extend_to_full_lines;
        // This function (dispatch_operator_with_motion) is always called for
        // motion-based operators, never visual. So origin is always Motion.
        if operator == Operator::Delete
            && motion_type == MotionType::CharWise
            && input.force_type.is_none()
        {
            let start = range_result.range.start().get();
            let end = range_result.range.end().get();

            if line_of(text, end) > line_of(text, start) {
                let ls = line_start_for_offset(text, start);
                let le = line_end_for_offset(text, start);
                let after_start = &text[start..le];
                let in_indent = first_non_blank_in_line(&text[ls..le]) >= (start - ls);

                if after_start.trim().is_empty() && in_indent {
                    let (ls, le) = extend_to_full_lines(text, start, end);
                    (
                        crate::primitives::Range::from_raw(ls, le),
                        MotionType::LineWise,
                    )
                } else {
                    (range_result.range, motion_type)
                }
            } else {
                (range_result.range, motion_type)
            }
        } else {
            (range_result.range, motion_type)
        }
    };

    let mut op_ctx = OperatorContext::new(
        text,
        final_range,
        final_motion_type,
        register,
        count,
        cursor,
    )
    .with_motion_target(Offset::new(range_result.motion_target))
    .with_shiftwidth(input.shiftwidth)
    .with_tabstop(input.options.tabstop())
    .with_expandtab(input.options.expandtab())
    .with_textwidth(input.textwidth)
    .with_format_options(input.options)
    .with_commentstring(input.options.commentstring())
    .with_sticky_column(input.sticky_column)
    .with_force_applied(input.force_type.is_some());

    if let Some(provider) = input.custom_operators {
        op_ctx = op_ctx.with_custom_operators(provider);
    }

    // Jump motions (%, G, gg, *, #, (, ), {, }, H/M/L) always use numbered
    // registers for deletes, even for single-line results (per :help registers)
    if motion.is_jump_motion() || matches!(motion, Motion::SearchNext | Motion::SearchPrev) {
        op_ctx = op_ctx.with_force_numbered();
    }

    // Ctrl-V force: route through block visual operator path.
    //
    // When the motion force override is BlockWise (d<C-v>j, y<C-v>k, etc.),
    // we need to treat the operation as a block rectangle, not a linear range.
    // Synthesize a SelectionRange from cursor (anchor) and motion_target (head),
    // then delegate to execute_block_operator which handles block geometry,
    // per-line column extraction, and register routing.
    if final_motion_type == MotionType::BlockWise {
        let sel =
            crate::primitives::SelectionRange::new(cursor, Offset::new(range_result.motion_target));
        return block_visual::execute_block_operator(operator, &op_ctx, &sel);
    }

    let mut result = dispatch_operator(operator, &op_ctx);

    // gn/gN operator motions: Neovim internally enters visual mode to select the
    // search match, so `<` and `>` marks are set to the match boundaries.
    // Emit SetMark effects so our engine matches Neovim behavior.
    if matches!(
        motion,
        Motion::SearchObjectForward | Motion::SearchObjectBackward
    ) {
        let mark_start = range_result.range.start();
        // Neovim's visual end mark for gn is the last included byte (inclusive),
        // which in gap-indexing is end - 1.
        let mark_end_raw = range_result.range.end().get();
        let mark_end = if mark_end_raw > 0 {
            Offset::new(prev_char_boundary(text, mark_end_raw))
        } else {
            Offset::new(0)
        };
        let mut mark_effects = Effects::new()
            .set_mark(crate::primitives::MarkName::VISUAL_START, mark_start, None)
            .set_mark(crate::primitives::MarkName::VISUAL_END, mark_end, None);
        mark_effects.extend(result.effects);
        result.effects = mark_effects;
    }

    result
}

/// Input context for [`dispatch_operator_line`].
///
/// Bundles the parameters needed to apply an operator to entire lines
/// (e.g., `dd`, `yy`, `cc`).
pub struct OperatorLineInput<'a> {
    /// The operator to apply (e.g., Delete, Yank, Change).
    pub operator: Operator,
    /// Repeat count (number of lines).
    pub count: u32,
    /// Target register, if specified.
    pub register: Option<crate::primitives::RegisterName>,
    /// Document text.
    pub text: &'a str,
    /// Cursor byte offset.
    pub cursor: usize,
    /// Indentation width for indent/outdent operators.
    pub shiftwidth: usize,
    /// Tab stop width for indent operators.
    pub tabstop: usize,
    /// Whether to expand tabs to spaces for indent operators.
    pub expandtab: bool,
    /// Textwidth from VimOptions. The format operators read `options`
    /// instead, so this matters only to a context built without them.
    pub textwidth: usize,
    /// Engine options, for the format operators.
    pub options: &'a crate::primitives::VimOptions,
    /// Comment string for commentary operator (e.g., `"# %s"` for GDScript).
    pub commentstring: &'a str,
    /// Custom operator provider for `Operator::Custom(id)`.
    pub custom_operators: Option<&'a dyn crate::document::CustomOperatorProvider>,
    /// Sticky column (curswant) for linewise delete cursor placement.
    ///
    /// After a linewise delete (`dd`), Neovim uses `coladvance(curswant)` to
    /// position the cursor on the surviving line, preserving the virtual column.
    /// Without this, the cursor falls back to the range start column.
    pub sticky_column: Option<crate::primitives::VirtualColumn>,
}

/// Extend a linewise delete range to include the preceding newline when deleting at EOF.
///
/// When deleting the last line(s) of a document, the range needs to include the
/// preceding newline so that no trailing `\n` is left behind. This logic is shared
/// between `dispatch_operator_line` (e.g., `dd` at EOF) and `dispatch_operator_mark`
/// (e.g., `d'a` targeting EOF).
///
/// Returns the range unchanged if the conditions are not met (not at EOF, no
/// preceding newline, or not a Delete operator).
#[inline]
fn extend_range_for_eof_delete(
    text: &str,
    range: crate::primitives::Range,
    operator: Operator,
) -> crate::primitives::Range {
    if !matches!(operator, Operator::Delete) {
        return range;
    }
    let start = range.start().get();
    let end = range.end().get();
    if end >= text.len() && start > 0 && text.as_bytes().get(start - 1) == Some(&b'\n') {
        crate::primitives::Range::from_raw(start - 1, end)
    } else {
        range
    }
}

/// Dispatch an operator applied to lines (e.g., `dd`, `yy`, `cc`).
///
/// Handles the empty-buffer special case and EOF range extension for delete.
#[inline]
pub fn dispatch_operator_line(input: &OperatorLineInput<'_>) -> CommandResult {
    use crate::commands::operators::range::compute_linewise_range;
    use crate::commands::operators::registers::route_empty_buffer_registers;
    use crate::primitives::Offset;

    let operator = input.operator;
    let OperatorLineInput {
        count,
        register,
        text,
        cursor,
        shiftwidth,
        textwidth,
        options,
        commentstring,
        ..
    } = *input;

    // Special case: dd/yy on empty buffer
    if text.is_empty() && matches!(operator, Operator::Delete | Operator::Yank) {
        return CommandResult::effects_only(route_empty_buffer_registers(operator, register));
    }

    let range_result = compute_linewise_range(text, cursor, count);

    // Neovim rule: when N>> or Ndd with count > 1 and cursor is on the
    // last line, cursor_down(count-1) fails and the operation is a no-op.
    // This prevents `3>>` on the last line from indenting that single line.
    if count > 1 {
        let cursor_line = crate::commands::helpers::line_of(text, cursor);
        let total_lines = crate::commands::helpers::line_count(text);
        if cursor_line + 1 >= total_lines {
            // On the last line with count > 1: no-op
            return CommandResult::empty(Offset::new(cursor));
        }
    }

    // For delete at EOF, extend range to include preceding newline.
    // Change does NOT extend — it replaces line content in-place.
    let range = extend_range_for_eof_delete(text, range_result.range, operator);

    // For case operators (gUU, guu, g~~, g??), Neovim moves cursor to the
    // first non-blank character of the line (beginline(BL_WHITE|BL_FIX)).
    // We compute that offset and pass it as motion_target so that the
    // `min(cursor, motion_target)` in case.rs produces the correct result.
    //
    // For Reindent (`==`), Neovim's `nv_lineop` calls
    //   `beginline(BL_WHITE | BL_FIX)` on the TARGET line (the last line
    //   in the count range). This sets oap->end.col to the first non-blank
    //   of that line, which is used for mark `]` (indent.c:1055).
    let motion_target = if matches!(
        operator,
        Operator::Uppercase
            | Operator::Lowercase
            | Operator::ToggleCase
            | Operator::Rot13
            | Operator::Rot47
    ) {
        let line_start = crate::commands::helpers::line_start_for_offset(text, cursor);
        let line_end = text[line_start..]
            .find('\n')
            .map_or(text.len(), |i| line_start + i);
        let line = &text[line_start..line_end];
        let fnb = crate::commands::helpers::first_non_blank_in_line(line);
        Offset::new(line_start + fnb)
    } else if matches!(operator, Operator::Reindent) {
        // Neovim: `_` motion with count moves cursor_down(count-1), then
        // beginline(BL_WHITE|BL_FIX) on the target line. The motion_target
        // is the first non-blank of the LAST line in the operated range.
        let end_offset = range.end().get().min(text.len());
        // Find the last line in the range. range.end() may be at or past
        // the newline of the last line, so back up to find the line start.
        let last_line_start = if end_offset > 0 {
            let search_from = end_offset
                .saturating_sub(1)
                .min(text.len().saturating_sub(1));
            crate::commands::helpers::line_start_for_offset(text, search_from)
        } else {
            0
        };
        let last_line_end = text[last_line_start..]
            .find('\n')
            .map_or(text.len(), |i| last_line_start + i);
        let last_line = &text[last_line_start..last_line_end];
        let fnb = crate::commands::helpers::first_non_blank_in_line(last_line);
        Offset::new(last_line_start + fnb)
    } else {
        Offset::new(cursor)
    };

    let mut op_ctx = OperatorContext::new(
        text,
        range,
        range_result.motion_type,
        register,
        count,
        Offset::new(cursor),
    )
    .with_motion_target(motion_target)
    .with_shiftwidth(shiftwidth)
    .with_tabstop(input.tabstop)
    .with_expandtab(input.expandtab)
    .with_textwidth(textwidth)
    .with_format_options(options)
    .with_commentstring(commentstring)
    .with_sticky_column(input.sticky_column);

    // Format operators (gqq, gww) use text-object cursor placement:
    // cursor goes to start of range rather than preserving column.
    // Other line operators (dd, yy, >>, gUU) preserve the column via
    // cursor_after_delete/standard cursor logic and must NOT use this flag.
    if matches!(
        operator,
        Operator::Format | Operator::FormatKeepCursor | Operator::Reindent
    ) {
        op_ctx = op_ctx.with_textobject_flag();
    }

    if let Some(provider) = input.custom_operators {
        op_ctx = op_ctx.with_custom_operators(provider);
    }

    dispatch_operator(operator, &op_ctx)
}

/// Input context for [`dispatch_operator_find`].
///
/// Bundles the parameters needed to apply an operator via a find motion
/// (e.g., `dfo`, `ct;`).
pub struct OperatorFindInput<'a> {
    /// The operator to apply (e.g., Delete, Change).
    pub operator: Operator,
    /// The find command variant (f/F/t/T).
    pub cmd: crate::grammar::types::CharCommand,
    /// The target character for the find motion.
    pub char: char,
    /// Repeat count.
    pub count: u32,
    /// Target register, if specified.
    pub register: Option<crate::primitives::RegisterName>,
    /// Document text.
    pub text: &'a str,
    /// Cursor byte offset.
    pub cursor: usize,
    /// Engine options (search flags, word boundaries, etc.).
    pub options: &'a crate::primitives::VimOptions,
    /// Custom operator provider for `Operator::Custom(id)`.
    pub custom_operators: Option<&'a dyn crate::document::CustomOperatorProvider>,
}

/// Dispatch an operator applied via a find motion (e.g., `dfo`, `ct;`).
#[inline]
pub fn dispatch_operator_find(input: &OperatorFindInput<'_>) -> CommandResult {
    use crate::commands::motions::{MotionContext, MotionResult};
    use crate::commands::operators::range::compute_find_range;
    use crate::dispatch::dispatch_find;
    use crate::primitives::Offset;

    let operator = input.operator;
    let OperatorFindInput {
        cmd,
        char,
        count,
        register,
        text,
        cursor,
        options,
        ..
    } = *input;

    // Replace cannot take an operator
    if cmd == crate::grammar::types::CharCommand::Replace {
        return CommandResult::effects_only(crate::effects::Effects::new());
    }

    let motion_ctx =
        MotionContext::new(text, crate::primitives::Offset::new(cursor), count, options)
            .with_target_char(char);

    match dispatch_find(cmd, &motion_ctx) {
        Some(MotionResult::Position(target)) => {
            let range_result = compute_find_range(text, cursor, target.get());
            let mut op_ctx = OperatorContext::new(
                text,
                range_result.range,
                range_result.motion_type,
                register,
                count,
                Offset::new(cursor),
            )
            .with_motion_target(target)
            .with_textwidth(options.textwidth())
            .with_format_options(options)
            .with_commentstring(options.commentstring());
            if let Some(provider) = input.custom_operators {
                op_ctx = op_ctx.with_custom_operators(provider);
            }
            dispatch_operator(operator, &op_ctx)
        }
        _ => CommandResult::effects_only(crate::effects::Effects::new()),
    }
}

/// Input context for [`dispatch_operator_mark`].
///
/// Bundles the parameters needed to apply an operator to a mark range
/// (e.g., `y'a`, `` d`b ``).
pub struct OperatorMarkInput<'a> {
    /// The operator to apply (e.g., Delete, Yank, Change).
    pub operator: Operator,
    /// Document text.
    pub text: &'a str,
    /// Cursor byte offset.
    pub cursor: usize,
    /// Byte offset of the target mark.
    pub mark_offset: usize,
    /// Whether the mark jump is line-wise or exact.
    pub mark_type: crate::grammar::types::MarkType,
    /// Target register, if specified.
    pub register: Option<crate::primitives::RegisterName>,
    /// Engine options, for the format operators.
    pub options: &'a crate::primitives::VimOptions,
    /// Custom operator provider for `Operator::Custom(id)`.
    pub custom_operators: Option<&'a dyn crate::document::CustomOperatorProvider>,
}

/// Dispatch an operator applied to a mark range (e.g., `y'a`, `` d`b ``).
///
/// Derives `linewise` from `mark_type` internally — callers pass the raw
/// grammar type and dispatch owns the mapping policy.
#[inline]
pub fn dispatch_operator_mark(input: &OperatorMarkInput<'_>) -> CommandResult {
    use crate::commands::operators::range::compute_mark_range;
    use crate::primitives::Offset;

    let operator = input.operator;
    let OperatorMarkInput {
        text,
        cursor,
        mark_offset,
        mark_type,
        register,
        options,
        ..
    } = *input;

    let linewise = mark_type == crate::grammar::types::MarkType::JumpLine;
    let mark_result = compute_mark_range(text, cursor, mark_offset, linewise);

    // For linewise delete at EOF, extend range to include preceding newline
    // (same logic as dispatch_operator_line). Prevents trailing \n in result.
    let range = if linewise {
        extend_range_for_eof_delete(text, mark_result.range, operator)
    } else {
        mark_result.range
    };

    // Per Vim docs, ` and ' motions always force numbered register usage
    // (register 1 even for sub-line deletes). See :help quote_number.
    let mut op_ctx = OperatorContext::new(
        text,
        range,
        mark_result.motion_type,
        register,
        1,
        Offset::new(cursor),
    )
    .with_motion_target({
        if linewise {
            // For 'a (linewise mark), motion goes to first non-blank of mark's line
            let mark_line_start =
                crate::commands::helpers::line_start_for_offset(text, mark_offset);
            let mark_line_text = text
                .get(mark_line_start..)
                .and_then(|s| s.split('\n').next())
                .unwrap_or("");
            let fnb = crate::commands::helpers::first_non_blank_in_line(mark_line_text);
            Offset::new(mark_line_start + fnb)
        } else {
            // For `a (exact mark), motion goes to exact mark position
            Offset::new(mark_offset)
        }
    })
    .with_textwidth(options.textwidth())
    .with_format_options(options)
    .with_force_numbered();
    if let Some(provider) = input.custom_operators {
        op_ctx = op_ctx.with_custom_operators(provider);
    }
    dispatch_operator(operator, &op_ctx)
}

/// Input context for [`dispatch_operator_textobject`].
///
/// Bundles the parameters needed to apply an operator to a text object
/// (e.g., `diw`, `ci(`, `yap`).
pub struct OperatorTextObjectInput<'a> {
    /// The operator to apply (e.g., Delete, Yank, Change).
    pub operator: Operator,
    /// The text object to select (e.g., Word, Sentence, Paragraph).
    pub textobject: crate::grammar::types::TextObject,
    /// Repeat count.
    pub count: u32,
    /// Target register, if specified.
    pub register: Option<crate::primitives::RegisterName>,
    /// Document text.
    pub text: &'a str,
    /// Engine options (for word boundary classification, etc.).
    pub options: &'a crate::primitives::VimOptions,
    /// Cursor byte offset.
    pub cursor: usize,
    /// Indentation width for indent/outdent operators.
    pub shiftwidth: usize,
    /// Tab stop width for indent operators.
    pub tabstop: usize,
    /// Whether to expand tabs to spaces for indent operators.
    pub expandtab: bool,
    /// Textwidth from VimOptions. The format operators read `options`
    /// instead, so this matters only to a context built without them.
    pub textwidth: usize,
    /// Comment string for commentary operator.
    pub commentstring: &'a str,
    /// Capability providers (syntax, custom text objects, etc.).
    pub providers: crate::document::Providers<'a>,
    /// Custom operator provider for `Operator::Custom(id)`.
    pub custom_operators: Option<&'a dyn crate::document::CustomOperatorProvider>,
}

/// Dispatch an operator applied to a text object (e.g., `diw`, `ci(`, `yap`).
///
/// Owns the complete textobject-to-operator pipeline:
/// 1. Resolve text object with count expansion (via `dispatch_textobject_with_count`)
/// 2. Handle empty range (e.g., `di(` on `()`) → cursor-only result
/// 3. Map `linewise` flag → `MotionType`
/// 4. Adjust range for EOF (delete past end)
/// 5. Build `OperatorContext` and dispatch the operator
///
/// Returns effects-only `CommandResult`.
#[inline]
pub fn dispatch_operator_textobject(input: &OperatorTextObjectInput<'_>) -> CommandResult {
    use super::textobject::{dispatch_textobject_with_count, TextObjectContext};
    use crate::commands::operators::range_helpers::{adjust_eof_range, empty_textobject_result};
    use crate::primitives::Offset;

    let operator = input.operator;
    let OperatorTextObjectInput {
        textobject,
        count,
        register,
        text,
        options,
        cursor,
        shiftwidth,
        textwidth,
        commentstring,
        ref providers,
        ..
    } = *input;

    let text_ctx = TextObjectContext::new(text, cursor)
        .with_providers(*providers)
        .with_options(options);
    let Some(text_obj_range) = dispatch_textobject_with_count(textobject, &text_ctx, count) else {
        if count > 1 {
            // Neovim: when a counted text object can't expand enough, the operation
            // is canceled (no text change) but cursor moves to end of current line.
            // Only applies when count > 1 (not when the base text object simply
            // doesn't match, e.g. `di(` with no parens).
            let base_ctx = TextObjectContext::new(text, cursor)
                .with_providers(*providers)
                .with_options(options);
            if super::textobject::dispatch_textobject(textobject, &base_ctx).is_some() {
                let eol = crate::commands::helpers::line_end_for_offset(text, cursor);
                let last_char = if eol > 0 && !text.is_empty() {
                    crate::primitives::text_util::prev_char_boundary(text, eol)
                } else {
                    0
                };
                let new_cursor = Offset::new(last_char);
                return CommandResult::new(
                    crate::effects::Effects::new().set_cursor(new_cursor),
                    new_cursor,
                );
            }
        }
        return CommandResult::effects_only(crate::effects::Effects::new());
    };

    // Empty text object range (e.g., `di(` on `()`)
    if text_obj_range.range.is_empty() {
        return empty_textobject_result(text, operator, register, &text_obj_range);
    }

    // Map linewise flag → MotionType
    let motion_type = if text_obj_range.linewise {
        crate::primitives::MotionType::LineWise
    } else {
        crate::primitives::MotionType::CharWise
    };

    let range = adjust_eof_range(text, operator, &text_obj_range);
    let mut op_ctx = OperatorContext::new(
        text,
        range,
        motion_type,
        register,
        count,
        Offset::new(cursor),
    )
    .with_shiftwidth(shiftwidth)
    .with_tabstop(input.tabstop)
    .with_expandtab(input.expandtab)
    .with_textwidth(textwidth)
    .with_format_options(options)
    .with_commentstring(commentstring)
    .with_textobject_flag();
    if let Some(provider) = input.custom_operators {
        op_ctx = op_ctx.with_custom_operators(provider);
    }
    dispatch_operator(operator, &op_ctx)
}

pub use crate::commands::operators::SelectionOperatorContext;

/// Dispatch an operator on a visual selection (or dot-repeated visual range).
///
/// This is the complete visual operator pipeline:
/// 1. Dispatch the operator with the resolved range
/// 2. If live visual mode: append visual exit effects (SaveLastVisual, marks, ClearSelection)
/// 3. Set mode (Normal for non-Change operators, stay for Change)
/// 4. Set final cursor position
///
/// The execution layer should call this after resolving the selection range —
/// either from live selection or from LastVisualInfo for dot-repeat.
#[inline]
pub fn dispatch_operator_selection(ctx: &SelectionOperatorContext<'_>) -> CommandResult {
    use crate::commands::visual::selection;
    use crate::effects::Effects;
    use crate::primitives::Mode;

    // 1. Pre-set visual marks BEFORE operator effects, so that text-modifying
    //    operators (indent, etc.) trigger adjust_offsets and shift marks correctly.
    //    If marks are set AFTER the operator, they'd use stale pre-edit positions.
    let mut effects = Effects::new();
    if let Some((visual_type, ref sel)) = ctx.visual_exit {
        let exit = selection::visual_exit_with_marks(ctx.text, visual_type, sel, ctx.tabstop);
        effects.extend(exit);
    }

    // 2. Dispatch the operator (its Insert/Delete effects will adjust marks via effect_processor)
    let mut op_ctx = OperatorContext::new(
        ctx.text,
        ctx.range,
        ctx.motion_type,
        ctx.register,
        1,
        ctx.cursor_pos,
    )
    .with_shiftwidth(ctx.shiftwidth)
    .with_tabstop(ctx.tabstop)
    .with_expandtab(ctx.expandtab)
    .with_textwidth(ctx.options.textwidth())
    .with_format_options(ctx.options)
    .with_commentstring(ctx.commentstring)
    .with_sticky_column(ctx.sticky_column)
    .with_from_visual();
    if let Some(provider) = ctx.custom_operators {
        op_ctx = op_ctx.with_custom_operators(provider);
    }
    let op_result = dispatch_operator(ctx.operator, &op_ctx);
    let final_cursor = op_result.cursor;
    effects.extend(op_result.effects);

    // On empty buffer, the delete operator returns without marks.
    // Neovim still sets [/] to 0 for visual delete on empty buffer.
    if ctx.text.is_empty()
        && matches!(
            ctx.operator,
            Operator::Delete | Operator::Yank | Operator::Change
        )
    {
        let zero = crate::primitives::Offset::new(0);
        effects = effects
            .set_mark(crate::primitives::MarkName::CHANGE_START, zero, None)
            .set_mark(crate::primitives::MarkName::CHANGE_END, zero, None);
    }

    // 3. Mode transition: Normal for non-Change, no mode change for Change
    let mut tail = if ctx.operator == Operator::Change {
        Effects::new()
    } else {
        Effects::new().set_mode(Mode::Normal)
    };

    // 4. Final cursor position
    if let Some(cursor) = final_cursor {
        tail = tail.set_cursor(cursor);
    }

    effects.extend(tail);
    CommandResult::effects_only(effects)
}

#[cfg(test)]
#[path = "operator_tests.rs"]
mod tests;
