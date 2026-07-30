//! Global command (`:g/pattern/cmd`, `:v/pattern/cmd`).
//!
//! Execute a command on all lines matching (or not matching) a pattern.

use super::range::{resolve_line_spec, resolve_range};
use super::types::{ExContext, ExResult};
use crate::effects::{Effect, Effects};
use crate::errors::VimError;
use crate::grammar::types::{ExCommand, ExRange};
use crate::primitives::byte_delta;
use crate::primitives::{MarkName, MotionType, Offset, Range, RegisterName};
use crate::regex::{MatchContext, VimRegex};

/// Maximum number of matching lines `:g` will process.
///
/// Follows the pattern of `MAX_DRAIN_ITERATIONS` and `MAX_MACRO_DEPTH`.
/// The regex engine is already O(n·m) (Pike VM), so this caps the only
/// remaining unbounded loop vector.
const MAX_GLOBAL_CMD_LINES: usize = 100_000;

/// Maximum recursion depth for chained `:g/pat1/g/pat2/.../cmd`.
const MAX_GLOBAL_RECURSION_DEPTH: u8 = 10;

/// Execute global command.
///
/// For each line matching the pattern, executes the specified command.
/// With `invert=true`, executes on lines NOT matching (`:v`).
///
/// # Effect ordering
///
/// For text-mutating commands (`:g/pat/d`, `:g/pat/s`), effects are emitted
/// in **reverse line order** so that byte offsets from the original text remain
/// valid when the host applies them sequentially. The host MUST apply effects
/// in emission order (first emitted → first applied) for correct results.
///
/// # Errors
///
/// Returns `VimError::PatternNotFound` for bad regex or `VimError::NotEditorCommand` for unsupported commands.
pub fn global(
    range: &ExRange,
    pattern: &str,
    command: &ExCommand,
    invert: bool,
    ctx: &ExContext,
) -> ExResult {
    global_inner(range, pattern, command, invert, ctx, 0, None)
}

/// Inner implementation with depth tracking and optional line subset for chained `:g`.
///
/// When `line_subset` is `Some`, the inner `:g` filters within that subset
/// rather than scanning the full range (AND semantics for chained globals).
fn global_inner(
    range: &ExRange,
    pattern: &str,
    command: &ExCommand,
    invert: bool,
    ctx: &ExContext,
    depth: u8,
    line_subset: Option<&[usize]>,
) -> ExResult {
    if depth > MAX_GLOBAL_RECURSION_DEPTH {
        return Err(VimError::GlobalRecursionLimitExceeded {
            limit: MAX_GLOBAL_RECURSION_DEPTH,
        });
    }

    let resolved = resolve_range(range, ctx)?;

    let re =
        VimRegex::new(pattern).map_err(|e| VimError::PatternNotFound(format!("{e}").into()))?;

    // Collect matching line indices.
    // One no-captures cache reused for the entire matching loop, so the
    // per-line match does not reallocate engine scratch space.
    let mut is_match_cache = re.create_cache_no_captures();
    let mut matching_lines: Vec<usize> = Vec::new();

    if let Some(subset) = line_subset {
        // Chained :g — filter within the already-matched subset (AND semantics).
        for &line_idx in subset {
            if let Some(line_text) = ctx.line_text(line_idx) {
                let line_ctx = MatchContext::simple(line_text);
                let matches = re
                    .is_match_with_cache(&mut is_match_cache, &line_ctx)
                    .unwrap_or(false);
                if matches != invert {
                    matching_lines.push(line_idx);
                }
            }
        }
    } else {
        // Top-level :g — scan the full range with optional bloom pre-filter.
        let bloom_candidates: Option<Vec<usize>> = ctx.tree.and_then(|tree| {
            re.bloom_literal().map(|literal| {
                tree.find_matching_lines(literal, resolved.start()..resolved.end() + 1)
            })
        });

        let mut bloom_iter_idx = 0usize;
        for line_idx in resolved.start()..=resolved.end() {
            if let Some(ref candidates) = bloom_candidates {
                while bloom_iter_idx < candidates.len() && candidates[bloom_iter_idx] < line_idx {
                    bloom_iter_idx += 1;
                }
                let is_candidate =
                    bloom_iter_idx < candidates.len() && candidates[bloom_iter_idx] == line_idx;
                if !is_candidate {
                    if invert {
                        matching_lines.push(line_idx);
                    }
                    continue;
                }
            }

            if let Some(line_text) = ctx.line_text(line_idx) {
                let line_ctx = MatchContext::simple(line_text);
                let matches = re
                    .is_match_with_cache(&mut is_match_cache, &line_ctx)
                    .unwrap_or(false);
                if matches != invert {
                    matching_lines.push(line_idx);
                }
            }
        }
    }

    if matching_lines.len() > MAX_GLOBAL_CMD_LINES {
        return Err(VimError::GlobalLineLimitExceeded {
            limit: MAX_GLOBAL_CMD_LINES,
        });
    }

    if matching_lines.is_empty() {
        // Even with no matches, `:g` sets the search register
        return Ok(Effects::new()
            .set_search_pattern(pattern, crate::primitives::Direction::Forward)
            .show_error(VimError::PatternNotFound(pattern.into())));
    }

    // Set search register from the pattern (Vim behavior: :g/pat/ sets @/ to pat)
    let mut all_effects =
        Effects::new().set_search_pattern(pattern, crate::primitives::Direction::Forward);
    let lines_affected = matching_lines.len();

    let mut error_count = 0usize;

    match command {
        ExCommand::Delete { register, .. } => {
            // Neovim processes :g/pat/d matches top-to-bottom. Each :d
            // triggers register rotation (shift numbered 1-9). We emit
            // register effects in forward order for correct rotation, and
            // delete effects in reverse order for byte-offset correctness.

            let reg = register.unwrap_or(RegisterName::UNNAMED);
            let use_blackhole = reg.is_blackhole();

            // ── Phase 1: Register rotation (forward order) ─────────
            // Emit SetRegister effects that will trigger on_delete
            // routing in the effect processor, shifting 1→2→...→9.
            if !use_blackhole {
                for &line_idx in &matching_lines {
                    if let Some(line_text) = ctx.line_text(line_idx) {
                        let mut text_with_nl = String::from(line_text);
                        if !text_with_nl.ends_with('\n') {
                            text_with_nl.push('\n');
                        }
                        if reg != RegisterName::UNNAMED {
                            // Explicit register: set that register AND rotate
                            all_effects = all_effects.set_register(
                                reg,
                                text_with_nl.as_str(),
                                MotionType::LineWise,
                            );
                        }
                        // Route through NUMBERED_1 to trigger shift_numbered
                        all_effects = all_effects.set_register(
                            RegisterName::NUMBERED_1,
                            text_with_nl.as_str(),
                            MotionType::LineWise,
                        );
                    }
                }
            }

            // ── Phase 2: Delete effects (reverse order) ────────────
            // Build delete ranges from the original text, emitted in
            // reverse so that byte offsets remain valid when the host
            // applies them sequentially.
            for &line_idx in matching_lines.iter().rev() {
                if let Some((start_offset, end_offset)) = ctx.lines_range(line_idx, line_idx) {
                    // If deleting the last line(s) and there's a preceding
                    // newline, extend backward to avoid trailing newline —
                    // but only if the preceding line is NOT also being deleted
                    // (which would already cover that newline).
                    let prev_also_deleted =
                        line_idx > 0 && matching_lines.binary_search(&(line_idx - 1)).is_ok();
                    let (actual_start, actual_end) =
                        if line_idx + 1 >= ctx.total_lines && line_idx > 0 && !prev_also_deleted {
                            (start_offset.prev(), end_offset)
                        } else {
                            (start_offset, end_offset)
                        };
                    all_effects = all_effects.delete(Range::new(actual_start, actual_end));
                }
            }

            // ── Phase 3: Marks and cursor ──────────────────────────
            // Neovim sets [, ], . marks and cursor to the position of
            // the last matched line in the resulting document.
            let last_match = *matching_lines.last().unwrap_or(&0);
            let includes_last_line = last_match + 1 >= ctx.total_lines;

            // Compute total bytes deleted to determine remaining length.
            let mut total_deleted: usize = 0;
            for &line_idx in &matching_lines {
                if let Some((start, end)) = ctx.lines_range(line_idx, line_idx) {
                    total_deleted += start.distance(end);
                }
            }
            // When the last matching line is the last document line and
            // there's a preceding line that is NOT also being deleted,
            // the delete extends backward by one byte (the \n separator).
            let prev_of_last_also_deleted =
                last_match > 0 && matching_lines.binary_search(&(last_match - 1)).is_ok();
            if includes_last_line && last_match > 0 && !prev_of_last_also_deleted {
                total_deleted += 1;
            }
            let remaining_len = ctx.text.len().saturating_sub(total_deleted);

            let mark_pos = if includes_last_line {
                Offset::new(remaining_len + 1)
            } else {
                // Last matching line's start adjusted for prior deletions.
                let last_line_start = ctx
                    .line_start_offset(last_match)
                    .unwrap_or_else(|| Offset::new(0));
                let mut prior_deleted: usize = 0;
                for &line_idx in &matching_lines {
                    if line_idx >= last_match {
                        break;
                    }
                    if let Some((s, e)) = ctx.lines_range(line_idx, line_idx) {
                        prior_deleted += s.distance(e);
                    }
                }
                Offset::new(last_line_start.get().saturating_sub(prior_deleted))
            };
            all_effects = all_effects
                .set_mark(MarkName::CHANGE_START, mark_pos, None)
                .set_mark(MarkName::CHANGE_END, mark_pos, None)
                .set_mark(MarkName::LAST_CHANGE, mark_pos, None);
        }
        ExCommand::Yank { register, .. } => {
            // Yank doesn't modify, so order doesn't matter
            // Collect all text into one register
            let mut yanked_text = String::new();
            for &line_idx in &matching_lines {
                if let Some(line_text) = ctx.line_text(line_idx) {
                    yanked_text.push_str(line_text);
                    yanked_text.push('\n');
                }
            }
            let reg = register.unwrap_or(RegisterName::UNNAMED);
            all_effects = all_effects.set_register(
                reg,
                yanked_text.as_str(),
                crate::primitives::MotionType::LineWise,
            );
        }
        ExCommand::Substitute {
            pattern: sub_pat,
            replacement,
            flags,
            ..
        } => {
            // Iterate in reverse so that substitutions on later lines don't
            // invalidate byte offsets of earlier lines (same reason as delete).
            for &line_idx in matching_lines.iter().rev() {
                let line_range = ExRange::single_line(byte_delta::to_u32(line_idx + 1));
                match super::substitute::substitute(&line_range, sub_pat, replacement, *flags, ctx)
                {
                    Ok(effects) => all_effects.extend(effects),
                    Err(_) => error_count += 1,
                }
            }
        }
        ExCommand::Move { target, .. } => {
            // Resolve target once from the original context.
            let target_line = resolve_line_spec(target, ctx)?;
            for &line_idx in matching_lines.iter().rev() {
                let line_range = ExRange::single_line(byte_delta::to_u32(line_idx + 1));
                match super::sort::move_lines(&line_range, Some(target_line), ctx) {
                    Ok(effects) => all_effects.extend(effects),
                    Err(_) => error_count += 1,
                }
            }
        }
        ExCommand::Copy { target, .. } => {
            let target_line = resolve_line_spec(target, ctx)?;
            for &line_idx in matching_lines.iter().rev() {
                let line_range = ExRange::single_line(byte_delta::to_u32(line_idx + 1));
                match super::sort::copy_lines(&line_range, Some(target_line), ctx) {
                    Ok(effects) => all_effects.extend(effects),
                    Err(_) => error_count += 1,
                }
            }
        }
        ExCommand::Join { bang, .. } => {
            // Track the marks from the first effective join in reverse order
            // (= last effective join in forward order). These will override
            // the marks from later-processed (lower line) joins.
            let mut first_effective_join_marks: Option<(Offset, Offset, Offset)> = None;
            for &line_idx in matching_lines.iter().rev() {
                let line_range = ExRange::single_line(byte_delta::to_u32(line_idx + 1));
                match super::line_ops::join(&line_range, *bang, ctx) {
                    Ok(effects) => {
                        // Capture marks from the first effective join (last in
                        // forward order). Look for SetMark effects in the result.
                        if first_effective_join_marks.is_none() {
                            let mut mark_start = None;
                            let mut mark_end = None;
                            let mut mark_dot = None;
                            for effect in effects.iter() {
                                if let crate::effects::Effect::SetMark { name, offset, .. } = effect
                                {
                                    if *name == MarkName::CHANGE_START {
                                        mark_start = Some(*offset);
                                    } else if *name == MarkName::CHANGE_END {
                                        mark_end = Some(*offset);
                                    } else if *name == MarkName::LAST_CHANGE {
                                        mark_dot = Some(*offset);
                                    }
                                }
                            }
                            if let (Some(s), Some(e), Some(d)) = (mark_start, mark_end, mark_dot) {
                                first_effective_join_marks = Some((s, e, d));
                            }
                        }
                        all_effects.extend(effects);
                    }
                    Err(_) => error_count += 1,
                }
            }
            // Store the captured marks for the final override section below.
            // We use a local variable that the Join arm of the override match
            // can reference. Since this is inside the same function, we move
            // the override logic here instead.
            if let Some((mark_start, mark_end, mark_dot)) = first_effective_join_marks {
                all_effects = all_effects
                    .set_mark(MarkName::CHANGE_START, mark_start, None)
                    .set_mark(MarkName::CHANGE_END, mark_end, None)
                    .set_mark(MarkName::LAST_CHANGE, mark_dot, None);
            }
        }
        ExCommand::Sort { options, .. } => {
            for &line_idx in matching_lines.iter().rev() {
                let line_range = ExRange::single_line(byte_delta::to_u32(line_idx + 1));
                match super::sort::sort(&line_range, options, ctx) {
                    Ok(effects) => all_effects.extend(effects),
                    Err(_) => error_count += 1,
                }
            }
        }
        ExCommand::Norm { keys, remap, .. } => {
            for &line_idx in &matching_lines {
                let effects =
                    super::effects::norm_command(line_idx, line_idx, keys.clone(), *remap);
                all_effects.extend(effects);
            }
        }
        ExCommand::Left { indent, .. } => {
            for &line_idx in matching_lines.iter().rev() {
                let line_range = ExRange::single_line(byte_delta::to_u32(line_idx + 1));
                match super::text_ops::left(&line_range, *indent, ctx) {
                    Ok(effects) => all_effects.extend(effects),
                    Err(_) => error_count += 1,
                }
            }
        }
        ExCommand::Right { width, .. } => {
            let w = width.unwrap_or(80);
            for &line_idx in matching_lines.iter().rev() {
                let line_range = ExRange::single_line(byte_delta::to_u32(line_idx + 1));
                match super::text_ops::right(&line_range, w, ctx) {
                    Ok(effects) => all_effects.extend(effects),
                    Err(_) => error_count += 1,
                }
            }
        }
        ExCommand::Center { width, .. } => {
            let w = width.unwrap_or(80);
            for &line_idx in matching_lines.iter().rev() {
                let line_range = ExRange::single_line(byte_delta::to_u32(line_idx + 1));
                match super::text_ops::center(&line_range, w, ctx) {
                    Ok(effects) => all_effects.extend(effects),
                    Err(_) => error_count += 1,
                }
            }
        }
        ExCommand::Retab {
            new_tabstop,
            to_tabs,
            ..
        } => {
            let tabstop = new_tabstop.unwrap_or(8);
            for &line_idx in matching_lines.iter().rev() {
                let line_range = ExRange::single_line(byte_delta::to_u32(line_idx + 1));
                match super::text_ops::retab(&line_range, tabstop, *to_tabs, ctx) {
                    Ok(effects) => all_effects.extend(effects),
                    Err(_) => error_count += 1,
                }
            }
        }
        ExCommand::Global {
            range: inner_range,
            pattern: inner_pattern,
            command: inner_cmd,
            invert: inner_invert,
        } => {
            // Chained :g — recurse with AND semantics on the matched subset.
            let inner_effects = global_inner(
                inner_range,
                inner_pattern,
                inner_cmd,
                *inner_invert,
                ctx,
                depth + 1,
                Some(&matching_lines),
            )?;
            all_effects.extend(inner_effects);
        }
        _ => {
            return Err(VimError::NotEditorCommand(
                "Unsupported sub-command in :g".into(),
            ));
        }
    }

    // ── Final mark overrides ────────────────────────────────────────────
    // For sub-commands that emit per-line SetMark effects (substitute, join),
    // the last-processed line (lowest line number in reverse order) overrides
    // the marks from the first-processed line (highest line number). Neovim's
    // marks should reflect the LAST matching line in forward order.
    //
    // For substitute: line count is unchanged, so the original offset of the
    // last matching line is still valid in the result text.
    // For join: each join reduces line count; we'd need cumulative offset
    // tracking which is complex. For now, handle substitute.
    match command {
        ExCommand::Substitute { .. } => {
            if let Some(&last_line) = matching_lines.last() {
                if let Some(orig_start) = crate::commands::helpers::line_start(ctx.text, last_line)
                {
                    let shift = byte_shift_before(&all_effects, orig_start);
                    let mark = Offset::new(apply_shift(orig_start, shift));
                    all_effects = all_effects
                        .set_mark(MarkName::CHANGE_START, mark, None)
                        .set_mark(MarkName::CHANGE_END, mark, None)
                        .set_mark(MarkName::LAST_CHANGE, mark, None);
                }
            }
        }
        ExCommand::Join { .. } => {
            // :g/pat/j join marks are complex due to cumulative offset shifts.
            // Each join reduces the line count; the marks should reflect the
            // LAST effective join in forward order. Left for future work.
        }
        ExCommand::Global { .. } => {
            // Chained :g — the recursive call already set marks internally.
        }
        _ => {}
    }

    // Cursor placement after :g
    //
    // For :g/pat/d, cursor goes to the last matching line's position in
    // the post-delete document (= mark_pos computed above). We emit the
    // cursor here instead of relying on per-line delete() calls.
    //
    // For :move/:copy, the sub-command already emitted its own SetCursor.
    //
    // For non-line-changing commands (substitute, join, etc.), cursor goes
    // to the start of the last matching line in the original text.
    //
    // :norm is handled by the orchestrator which sets cursor after running
    // the keys, so we skip it here.
    match command {
        ExCommand::Delete { .. } => {
            // Use mark_pos computed in the Delete branch above — it's
            // the last matched line's post-delete position. For
            // end-of-file deletes, clamp to remaining_len.
            // Re-compute since mark_pos was consumed above.
            let last_match = *matching_lines.last().unwrap_or(&0);
            let includes_last = last_match + 1 >= ctx.total_lines;
            let mut tot_del: usize = 0;
            for &line_idx in &matching_lines {
                if let Some((s, e)) = ctx.lines_range(line_idx, line_idx) {
                    tot_del += s.distance(e);
                }
            }
            let prev_of_last_deleted =
                last_match > 0 && matching_lines.binary_search(&(last_match - 1)).is_ok();
            if includes_last && last_match > 0 && !prev_of_last_deleted {
                tot_del += 1;
            }
            let rem_len = ctx.text.len().saturating_sub(tot_del);
            let cursor = if rem_len == 0 {
                Offset::new(0)
            } else if includes_last {
                // The last line of the buffer was deleted. Cursor goes to the
                // start of the new last surviving line. Walk backwards from
                // the end to find the last non-deleted line.
                let match_set: std::collections::HashSet<usize> =
                    matching_lines.iter().copied().collect();
                let last_surviving_line = (0..ctx.total_lines)
                    .rev()
                    .find(|li| !match_set.contains(li));
                if let Some(surv_line) = last_surviving_line {
                    let surv_start = ctx
                        .line_start_offset(surv_line)
                        .unwrap_or_else(|| Offset::new(0));
                    let prior_del: usize = matching_lines
                        .iter()
                        .filter(|&&li| li < surv_line)
                        .filter_map(|&li| ctx.lines_range(li, li))
                        .map(|(s, e)| s.distance(e))
                        .sum();
                    Offset::new(surv_start.get().saturating_sub(prior_del))
                } else {
                    Offset::new(0)
                }
            } else {
                // Normal case: cursor goes to the position where the last
                // matched line was (now occupied by the next surviving line).
                let last_line_start = ctx
                    .line_start_offset(last_match)
                    .unwrap_or_else(|| Offset::new(0));
                let mut prior_del: usize = 0;
                for &li in &matching_lines {
                    if li >= last_match {
                        break;
                    }
                    if let Some((s, e)) = ctx.lines_range(li, li) {
                        prior_del += s.distance(e);
                    }
                }
                Offset::new(last_line_start.get().saturating_sub(prior_del))
            };
            all_effects = all_effects.set_cursor(cursor);
        }
        ExCommand::Move { .. }
        | ExCommand::Copy { .. }
        | ExCommand::Norm { .. }
        | ExCommand::Global { .. } => {
            // These commands already emitted their own SetCursor effects.
        }
        _ => {
            // Non-line-changing commands: cursor at last matching line,
            // adjusted for byte shifts from substitutions on earlier lines.
            if let Some(&last_line) = matching_lines.last() {
                if let Some(orig_start) = crate::commands::helpers::line_start(ctx.text, last_line)
                {
                    let shift = byte_shift_before(&all_effects, orig_start);
                    all_effects = all_effects.set_cursor(crate::primitives::Offset::new(
                        apply_shift(orig_start, shift),
                    ));
                }
            }
        }
    }

    let msg = if error_count > 0 {
        format!("{lines_affected} lines affected ({error_count} errors)")
    } else {
        format!("{lines_affected} lines affected")
    };
    all_effects = all_effects.show_message(msg);

    Ok(all_effects)
}

/// Compute the net byte shift from mutation effects whose range starts
/// strictly before `threshold` (an original-text byte offset).
///
/// Used by `:g/pat/s` to adjust cursor/mark positions: substitutions on
/// earlier lines change byte offsets of later lines.
fn byte_shift_before(effects: &Effects, threshold: usize) -> isize {
    let mut shift: isize = 0;
    for effect in effects.iter() {
        match effect {
            Effect::Replace { range, text } => {
                if range.start().get() < threshold {
                    shift += byte_delta::delta(text.len(), range.len());
                }
            }
            Effect::Delete { range } => {
                if range.start().get() < threshold {
                    shift -= byte_delta::to_isize(range.len());
                }
            }
            Effect::Insert { offset, text } => {
                if offset.get() < threshold {
                    shift += byte_delta::to_isize(text.len());
                }
            }
            _ => {}
        }
    }
    shift
}

/// Apply a signed byte shift to an offset, clamping to zero.
const fn apply_shift(offset: usize, shift: isize) -> usize {
    offset.saturating_add_signed(shift)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;

    fn ctx() -> ExContext<'static> {
        ExContext::new("line one\nTODO fix this\nline three\nTODO another", 0)
    }

    #[test]
    fn test_global_delete() {
        let range = ExRange::entire_file();
        let cmd = ExCommand::Delete {
            range: ExRange::current_line(),
            register: None,
        };
        let result = global(&range, "TODO", &cmd, false, &ctx()).unwrap();

        // Should have effects for deleting matching lines
        assert!(result
            .as_slice()
            .iter()
            .any(|e| matches!(e, Effect::Delete { .. })));
    }

    #[test]
    fn test_global_invert() {
        let range = ExRange::entire_file();
        let cmd = ExCommand::Yank {
            range: ExRange::current_line(),
            register: Some(crate::primitives::RegisterName::new_unchecked('a')),
        };
        // :v/TODO/y - yank lines NOT matching TODO
        let result = global(&range, "TODO", &cmd, true, &ctx()).unwrap();

        assert!(result
            .as_slice()
            .iter()
            .any(|e| matches!(e, Effect::SetRegister { name, .. } if *name == crate::primitives::RegisterName::new_unchecked('a'))));
    }

    // ── Chained :g tests ──────────────────────────────────────────────────

    #[test]
    fn test_chained_global_delete_and_semantics() {
        // :g/foo/g/bar/d — only lines matching BOTH "foo" AND "bar" are deleted
        let text = "foo only\nbar only\nfoo bar both\nneither\nfoo bar again\n";
        let ctx = ExContext::new(text, 0);
        let range = ExRange::entire_file();

        let inner_cmd = ExCommand::Global {
            range: ExRange::entire_file(),
            pattern: "bar".into(),
            command: Box::new(ExCommand::Delete {
                range: ExRange::current_line(),
                register: None,
            }),
            invert: false,
        };

        let result = global(&range, "foo", &inner_cmd, false, &ctx).unwrap();

        // Count delete effects — should delete lines 2 ("foo bar both") and 4 ("foo bar again")
        let delete_count = result
            .as_slice()
            .iter()
            .filter(|e| matches!(e, Effect::Delete { .. }))
            .count();
        assert_eq!(
            delete_count, 2,
            "expected 2 delete effects for 'foo bar both' and 'foo bar again'"
        );
    }

    #[test]
    fn test_chained_global_triple_chain() {
        // :g/a/g/b/g/c/d — only lines containing all three: "a", "b", and "c"
        let text = "abc\nab\nac\nbc\na\nb\nc\nabc again\n";
        let ctx = ExContext::new(text, 0);
        let range = ExRange::entire_file();

        let innermost_cmd = ExCommand::Delete {
            range: ExRange::current_line(),
            register: None,
        };
        let inner_cmd = ExCommand::Global {
            range: ExRange::entire_file(),
            pattern: "c".into(),
            command: Box::new(innermost_cmd),
            invert: false,
        };
        let outer_cmd = ExCommand::Global {
            range: ExRange::entire_file(),
            pattern: "b".into(),
            command: Box::new(inner_cmd),
            invert: false,
        };

        let result = global(&range, "a", &outer_cmd, false, &ctx).unwrap();

        // Only "abc" (line 0) and "abc again" (line 7) match all three
        let delete_count = result
            .as_slice()
            .iter()
            .filter(|e| matches!(e, Effect::Delete { .. }))
            .count();
        assert_eq!(delete_count, 2, "expected 2 deletes for lines with a+b+c");
    }

    #[test]
    fn test_chained_global_recursion_depth_limit() {
        // Build a chain deeper than MAX_GLOBAL_RECURSION_DEPTH (10)
        let text = "test line\n";
        let ctx = ExContext::new(text, 0);
        let range = ExRange::entire_file();

        // Build a chain of 12 nested :g commands
        let mut cmd: ExCommand = ExCommand::Delete {
            range: ExRange::current_line(),
            register: None,
        };
        for _ in 0..12 {
            cmd = ExCommand::Global {
                range: ExRange::entire_file(),
                pattern: "test".into(),
                command: Box::new(cmd),
                invert: false,
            };
        }

        let result = global(&range, "test", &cmd, false, &ctx);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            VimError::GlobalRecursionLimitExceeded { .. }
        ));
    }

    #[test]
    fn test_chained_global_empty_result_no_error() {
        // :g/foo/g/impossible/d — no lines match both, no error (just PatternNotFound message)
        let text = "foo here\nfoo there\nbar\n";
        let ctx = ExContext::new(text, 0);
        let range = ExRange::entire_file();

        let inner_cmd = ExCommand::Global {
            range: ExRange::entire_file(),
            pattern: "impossible".into(),
            command: Box::new(ExCommand::Delete {
                range: ExRange::current_line(),
                register: None,
            }),
            invert: false,
        };

        let result = global(&range, "foo", &inner_cmd, false, &ctx).unwrap();

        // No delete effects should be emitted
        let delete_count = result
            .as_slice()
            .iter()
            .filter(|e| matches!(e, Effect::Delete { .. }))
            .count();
        assert_eq!(delete_count, 0, "no lines should be deleted");
    }

    // ── Additional chained :g validation tests ───────────────────────────

    #[test]
    fn test_chained_global_with_substitution() {
        // :g/foo/g/bar/s/x/y/g — substitute 'x' with 'y' only on lines matching BOTH foo AND bar
        let text = "foo x bar x\nfoo x only\nbar x only\nfoo bar no_x\nneither x\n";
        let ctx = ExContext::new(text, 0);
        let range = ExRange::entire_file();

        let inner_cmd = ExCommand::Global {
            range: ExRange::entire_file(),
            pattern: "bar".into(),
            command: Box::new(ExCommand::Substitute {
                range: ExRange::current_line(),
                pattern: "x".into(),
                replacement: "y".into(),
                flags: crate::primitives::SubFlags::parse("g"),
            }),
            invert: false,
        };

        let result = global(&range, "foo", &inner_cmd, false, &ctx).unwrap();

        // Only "foo x bar x" (line 0) and "foo bar no_x" (line 3) match both foo AND bar.
        // Of those, only line 0 has 'x' to replace (line 3 has "no_x" which also contains x).
        // So we expect Replace effects for lines 0 and 3.
        let replace_count = result
            .as_slice()
            .iter()
            .filter(|e| matches!(e, Effect::Replace { .. }))
            .count();
        // Line 0 has 2 occurrences of 'x', line 3 has 1 occurrence ('no_x' → 'no_y')
        // With 'g' flag, all occurrences on each line are replaced
        assert!(
            replace_count > 0,
            "expected at least one Replace effect, got {replace_count}"
        );
    }

    #[test]
    fn test_chained_inverted_outer_normal_inner() {
        // :v/foo/g/bar/d — delete lines that DON'T match foo but DO match bar
        let text = "foo bar\nbar only\nfoo only\nbar also\nneither\n";
        let ctx = ExContext::new(text, 0);
        let range = ExRange::entire_file();

        // Outer: :v/foo/ (invert=true) selects lines NOT matching foo → "bar only", "bar also", "neither"
        // Inner: :g/bar/d selects lines matching bar within that subset → "bar only", "bar also"
        let inner_cmd = ExCommand::Global {
            range: ExRange::entire_file(),
            pattern: "bar".into(),
            command: Box::new(ExCommand::Delete {
                range: ExRange::current_line(),
                register: None,
            }),
            invert: false,
        };

        let result = global(&range, "foo", &inner_cmd, true, &ctx).unwrap();

        let delete_count = result
            .as_slice()
            .iter()
            .filter(|e| matches!(e, Effect::Delete { .. }))
            .count();
        assert_eq!(
            delete_count, 2,
            "expected 2 deletes for 'bar only' and 'bar also'"
        );
    }

    #[test]
    fn test_chained_normal_outer_inverted_inner() {
        // :g/foo/v/bar/d — delete lines with foo but WITHOUT bar
        let text = "foo bar\nfoo only\nbar only\nfoo also\nneither\n";
        let ctx = ExContext::new(text, 0);
        let range = ExRange::entire_file();

        // Outer: :g/foo/ selects "foo bar", "foo only", "foo also"
        // Inner: :v/bar/d (invert=true) selects lines NOT matching bar → "foo only", "foo also"
        let inner_cmd = ExCommand::Global {
            range: ExRange::entire_file(),
            pattern: "bar".into(),
            command: Box::new(ExCommand::Delete {
                range: ExRange::current_line(),
                register: None,
            }),
            invert: true,
        };

        let result = global(&range, "foo", &inner_cmd, false, &ctx).unwrap();

        let delete_count = result
            .as_slice()
            .iter()
            .filter(|e| matches!(e, Effect::Delete { .. }))
            .count();
        assert_eq!(
            delete_count, 2,
            "expected 2 deletes for 'foo only' and 'foo also'"
        );
    }

    #[test]
    fn test_chained_global_degenerate_all_match() {
        // :g/./g/./d — every line matches '.', all should be deleted
        let text = "a\nb\nc\n";
        let ctx = ExContext::new(text, 0);
        let range = ExRange::entire_file();

        let inner_cmd = ExCommand::Global {
            range: ExRange::entire_file(),
            pattern: ".".into(),
            command: Box::new(ExCommand::Delete {
                range: ExRange::current_line(),
                register: None,
            }),
            invert: false,
        };

        let result = global(&range, ".", &inner_cmd, false, &ctx).unwrap();

        let delete_count = result
            .as_slice()
            .iter()
            .filter(|e| matches!(e, Effect::Delete { .. }))
            .count();
        // 3 lines in the buffer (a, b, c), all match '.'
        assert_eq!(delete_count, 3, "expected all 3 lines to be deleted");
    }

    #[test]
    fn test_chained_global_single_line_no_crash() {
        // :g/x/g/x/d — single-line buffer, verify no crash
        let text = "x";
        let ctx = ExContext::new(text, 0);
        let range = ExRange::entire_file();

        let inner_cmd = ExCommand::Global {
            range: ExRange::entire_file(),
            pattern: "x".into(),
            command: Box::new(ExCommand::Delete {
                range: ExRange::current_line(),
                register: None,
            }),
            invert: false,
        };

        let result = global(&range, "x", &inner_cmd, false, &ctx);
        // Should not panic — either succeeds with a delete or handles gracefully
        assert!(result.is_ok(), "single-line :g/x/g/x/d should not crash");
        let effects = result.unwrap();
        let delete_count = effects
            .as_slice()
            .iter()
            .filter(|e| matches!(e, Effect::Delete { .. }))
            .count();
        assert_eq!(
            delete_count, 1,
            "the single line matching 'x' should be deleted"
        );
    }

    #[test]
    fn test_chained_global_at_exact_depth_limit() {
        // Build a chain of exactly MAX_GLOBAL_RECURSION_DEPTH (10) nested :g commands.
        // The outer call starts at depth=0, so 10 inner levels means depth goes 1..10.
        // depth=10 should still succeed (since check is `depth > 10`).
        let text = "test line\n";
        let ctx = ExContext::new(text, 0);
        let range = ExRange::entire_file();

        // Build exactly 10 nested :g commands (depth 1 through 10)
        let mut cmd: ExCommand = ExCommand::Delete {
            range: ExRange::current_line(),
            register: None,
        };
        for _ in 0..10 {
            cmd = ExCommand::Global {
                range: ExRange::entire_file(),
                pattern: "test".into(),
                command: Box::new(cmd),
                invert: false,
            };
        }

        // The outer `global()` starts at depth=0, then each nested Global increments.
        // 10 nested Globals → depth goes 1, 2, ..., 10.
        // Check is `depth > MAX_GLOBAL_RECURSION_DEPTH` (i.e. depth > 10) so depth=10 passes.
        // BUT wait — the outer global() also processes the first Global as a command,
        // so the recursion is: global(depth=0) sees Global cmd → global_inner(depth=1)
        // sees Global cmd → global_inner(depth=2) ... → global_inner(depth=10) sees Delete.
        // depth=10 is NOT > 10, so it should succeed.
        let result = global(&range, "test", &cmd, false, &ctx);
        assert!(
            result.is_ok(),
            "depth=10 (exactly at limit) should succeed, got: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_chained_global_one_past_depth_limit() {
        // Build 11 nested :g commands — the 11th recursion hits depth=11 > 10, should fail.
        let text = "test line\n";
        let ctx = ExContext::new(text, 0);
        let range = ExRange::entire_file();

        let mut cmd: ExCommand = ExCommand::Delete {
            range: ExRange::current_line(),
            register: None,
        };
        for _ in 0..11 {
            cmd = ExCommand::Global {
                range: ExRange::entire_file(),
                pattern: "test".into(),
                command: Box::new(cmd),
                invert: false,
            };
        }

        let result = global(&range, "test", &cmd, false, &ctx);
        assert!(result.is_err(), "depth=11 should exceed the limit");
        assert!(matches!(
            result.unwrap_err(),
            VimError::GlobalRecursionLimitExceeded { limit: 10 }
        ));
    }

    #[test]
    fn test_chained_global_same_pattern_twice() {
        // :g/foo/g/foo/d — same pattern AND itself = same set of lines
        let text = "foo here\nbar there\nfoo again\n";
        let ctx = ExContext::new(text, 0);
        let range = ExRange::entire_file();

        let inner_cmd = ExCommand::Global {
            range: ExRange::entire_file(),
            pattern: "foo".into(),
            command: Box::new(ExCommand::Delete {
                range: ExRange::current_line(),
                register: None,
            }),
            invert: false,
        };

        let result = global(&range, "foo", &inner_cmd, false, &ctx).unwrap();

        // Lines 0 and 2 match "foo"; inner also matches "foo" => same 2 lines deleted
        let delete_count = result
            .as_slice()
            .iter()
            .filter(|e| matches!(e, Effect::Delete { .. }))
            .count();
        assert_eq!(
            delete_count, 2,
            "same pattern twice should still delete matching lines"
        );
    }

    #[test]
    fn test_chained_global_large_buffer() {
        // 200 lines, chained :g to verify linear performance (no quadratic blowup)
        let mut text = String::new();
        for i in 0..200 {
            if i % 2 == 0 {
                text.push_str(&format!("foo bar line {i}\n"));
            } else {
                text.push_str(&format!("baz only line {i}\n"));
            }
        }
        let ctx = ExContext::new(&text, 0);
        let range = ExRange::entire_file();

        let inner_cmd = ExCommand::Global {
            range: ExRange::entire_file(),
            pattern: "bar".into(),
            command: Box::new(ExCommand::Delete {
                range: ExRange::current_line(),
                register: None,
            }),
            invert: false,
        };

        let result = global(&range, "foo", &inner_cmd, false, &ctx).unwrap();

        // All even lines (0, 2, 4, ..., 198) have "foo bar" => 100 lines match both
        let delete_count = result
            .as_slice()
            .iter()
            .filter(|e| matches!(e, Effect::Delete { .. }))
            .count();
        assert_eq!(
            delete_count, 100,
            "expected 100 deletes for lines with foo+bar"
        );
    }

    #[test]
    fn test_chained_global_empty_lines() {
        // :g/^$/g/^$/d — chain on empty lines (edge case for line matching)
        // Text: "first\n\nsecond\n\n\nthird\n"
        // Lines: 0="first", 1="", 2="second", 3="", 4="", 5="third", 6=""
        // Empty lines: 1, 3, 4, 6 (trailing \n creates empty line 6)
        let text = "first\n\nsecond\n\n\nthird\n";
        let ctx = ExContext::new(text, 0);
        let range = ExRange::entire_file();

        let inner_cmd = ExCommand::Global {
            range: ExRange::entire_file(),
            pattern: "^$".into(),
            command: Box::new(ExCommand::Delete {
                range: ExRange::current_line(),
                register: None,
            }),
            invert: false,
        };

        let result = global(&range, "^$", &inner_cmd, false, &ctx).unwrap();

        // Lines 1, 3, 4, 6 are empty => inner matches same empty lines => 4 deletes
        let delete_count = result
            .as_slice()
            .iter()
            .filter(|e| matches!(e, Effect::Delete { .. }))
            .count();
        assert_eq!(delete_count, 4, "expected 4 empty lines deleted");
    }

    #[test]
    fn test_chained_global_with_move() {
        // :g/foo/g/bar/m 0 — move lines matching both foo and bar to top
        let text = "first\nfoo bar\nsecond\nfoo bar again\nlast\n";
        let ctx = ExContext::new(text, 0);
        let range = ExRange::entire_file();

        let inner_cmd = ExCommand::Global {
            range: ExRange::entire_file(),
            pattern: "bar".into(),
            command: Box::new(ExCommand::Move {
                range: ExRange::current_line(),
                target: crate::grammar::types::LineSpec::Absolute(0),
            }),
            invert: false,
        };

        let result = global(&range, "foo", &inner_cmd, false, &ctx);
        // Move is supported — should produce effects without error
        assert!(
            result.is_ok(),
            "chained :g with :m should not error: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_chained_global_line_subset_preserves_document_indices() {
        // Verify inner :g filters using document-relative indices, not 0-based subset indices.
        // If indices were re-indexed, the inner match would fail.
        let text = "alpha\nfoo xxx\nbeta\nfoo yyy\ngamma\n";
        let ctx = ExContext::new(text, 0);
        let range = ExRange::entire_file();

        // Outer :g/foo/ matches lines 1 ("foo xxx") and 3 ("foo yyy")
        // Inner :g/yyy/ should match line 3 (document index), NOT index 1 of subset
        let inner_cmd = ExCommand::Global {
            range: ExRange::entire_file(),
            pattern: "yyy".into(),
            command: Box::new(ExCommand::Delete {
                range: ExRange::current_line(),
                register: None,
            }),
            invert: false,
        };

        let result = global(&range, "foo", &inner_cmd, false, &ctx).unwrap();

        let delete_count = result
            .as_slice()
            .iter()
            .filter(|e| matches!(e, Effect::Delete { .. }))
            .count();
        assert_eq!(delete_count, 1, "only line 3 ('foo yyy') should be deleted");
    }

    #[test]
    fn test_global_recursion_limit_error_format() {
        // Verify the error message format matches E-code conventions
        let err = VimError::GlobalRecursionLimitExceeded { limit: 10 };
        let msg = err.to_string();
        assert_eq!(msg, "E5101: :global recursion depth limit exceeded (10)");
        // Verify it has Normal severity (not Brief or System)
        assert_eq!(err.severity(), crate::errors::ErrorSeverity::Normal);
    }

    // ── Vim behaviour conformance & regression probes ────────────────────

    #[test]
    fn test_chained_global_yank_exact_content() {
        // :g/a/g/b/y — verify yanked text is exactly the matching lines.
        let text = "cat\ndog\ncab\nbat\n";
        // Lines containing 'a': "cat"(0), "cab"(2), "bat"(3)
        // Of those, containing 'b': "cab"(2), "bat"(3)
        let ctx = ExContext::new(text, 0);
        let range = ExRange::entire_file();

        let inner_cmd = ExCommand::Global {
            range: ExRange::entire_file(),
            pattern: "b".into(),
            command: Box::new(ExCommand::Yank {
                range: ExRange::current_line(),
                register: None,
            }),
            invert: false,
        };

        let result = global(&range, "a", &inner_cmd, false, &ctx).unwrap();

        // Extract the last UNNAMED register value (the inner yank result)
        let yanked_texts: Vec<&str> = result
            .as_slice()
            .iter()
            .filter_map(|e| {
                if let Effect::SetRegister { name, text, .. } = e {
                    if *name == RegisterName::UNNAMED {
                        Some(text.as_str())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        assert!(!yanked_texts.is_empty(), "expected yank register effect");
        let yanked = *yanked_texts.last().unwrap();
        assert_eq!(
            yanked, "cab\nbat\n",
            "yank should contain exactly 'cab' and 'bat' (lines matching both /a/ and /b/)"
        );
    }

    #[test]
    fn test_chained_global_delete_byte_offsets_original_buffer() {
        // Verify delete byte offsets reference the ORIGINAL buffer, not a re-indexed subset.
        let text = "aaa\nbbb\nabc\nddd\n";
        // Byte layout: "aaa\n"=0..4, "bbb\n"=4..8, "abc\n"=8..12, "ddd\n"=12..16
        // :g/a/g/b/d — lines with both 'a' and 'b': only "abc"(line 2, bytes 8..12)
        let ctx = ExContext::new(text, 0);
        let range = ExRange::entire_file();

        let inner_cmd = ExCommand::Global {
            range: ExRange::entire_file(),
            pattern: "b".into(),
            command: Box::new(ExCommand::Delete {
                range: ExRange::current_line(),
                register: None,
            }),
            invert: false,
        };

        let result = global(&range, "a", &inner_cmd, false, &ctx).unwrap();

        let delete_ranges: Vec<(usize, usize)> = result
            .as_slice()
            .iter()
            .filter_map(|e| {
                if let Effect::Delete { range } = e {
                    Some((range.start().get(), range.end().get()))
                } else {
                    None
                }
            })
            .collect();

        assert_eq!(
            delete_ranges.len(),
            1,
            "expected exactly 1 delete for 'abc'"
        );
        assert_eq!(
            delete_ranges[0],
            (8, 12),
            "delete range must reference original buffer bytes (8..12 for 'abc\\n')"
        );
    }

    #[test]
    fn test_chained_global_interleaved_with_byte_verification() {
        // :g/foo/g/bar/d — "foo", "bar", "foo bar", "bar foo"
        // Byte layout: "foo\n"=0..4, "bar\n"=4..8, "foo bar\n"=8..16, "bar foo\n"=16..24
        // Lines matching "foo": "foo"(0), "foo bar"(2), "bar foo"(3)
        // Of those, matching "bar": "foo bar"(2), "bar foo"(3)
        let text = "foo\nbar\nfoo bar\nbar foo\n";
        let ctx = ExContext::new(text, 0);
        let range = ExRange::entire_file();

        let inner_cmd = ExCommand::Global {
            range: ExRange::entire_file(),
            pattern: "bar".into(),
            command: Box::new(ExCommand::Delete {
                range: ExRange::current_line(),
                register: None,
            }),
            invert: false,
        };

        let result = global(&range, "foo", &inner_cmd, false, &ctx).unwrap();

        let delete_ranges: Vec<(usize, usize)> = result
            .as_slice()
            .iter()
            .filter_map(|e| {
                if let Effect::Delete { range } = e {
                    Some((range.start().get(), range.end().get()))
                } else {
                    None
                }
            })
            .collect();

        assert_eq!(delete_ranges.len(), 2);
        // Reverse order emission for byte-offset correctness
        assert_eq!(
            delete_ranges[0],
            (16, 24),
            "first emitted delete should be 'bar foo\\n' (later line, reverse order)"
        );
        assert_eq!(
            delete_ranges[1],
            (8, 16),
            "second emitted delete should be 'foo bar\\n'"
        );
    }

    #[test]
    fn test_chained_global_regex_anchor_hash_todo() {
        // :g/^#/g/TODO/d — delete comment lines (starting with #) that contain TODO.
        let text = "# TODO: fix\ncode\n# note\n# TODO: clean\nTODO not comment\n";
        // Lines starting with '#': line 0 "# TODO: fix", line 2 "# note", line 3 "# TODO: clean"
        // Of those, containing 'TODO': line 0, line 3
        // "TODO not comment" does NOT start with '#', so it's excluded by outer.
        let ctx = ExContext::new(text, 0);
        let range = ExRange::entire_file();

        let inner_cmd = ExCommand::Global {
            range: ExRange::entire_file(),
            pattern: "TODO".into(),
            command: Box::new(ExCommand::Delete {
                range: ExRange::current_line(),
                register: None,
            }),
            invert: false,
        };

        let result = global(&range, "^#", &inner_cmd, false, &ctx).unwrap();

        let delete_count = result
            .as_slice()
            .iter()
            .filter(|e| matches!(e, Effect::Delete { .. }))
            .count();
        assert_eq!(
            delete_count, 2,
            "only '# TODO: fix' and '# TODO: clean' should be deleted (not 'TODO not comment')"
        );
    }

    #[test]
    fn test_chained_global_all_match_outer_none_match_inner_zero_deletes() {
        // :g/foo/g/bar/d — all lines match "foo" but none match "bar"
        let text = "foo one\nfoo two\nfoo three\n";
        let ctx = ExContext::new(text, 0);
        let range = ExRange::entire_file();

        let inner_cmd = ExCommand::Global {
            range: ExRange::entire_file(),
            pattern: "bar".into(),
            command: Box::new(ExCommand::Delete {
                range: ExRange::current_line(),
                register: None,
            }),
            invert: false,
        };

        let result = global(&range, "foo", &inner_cmd, false, &ctx).unwrap();

        let delete_count = result
            .as_slice()
            .iter()
            .filter(|e| matches!(e, Effect::Delete { .. }))
            .count();
        assert_eq!(
            delete_count, 0,
            "0 deletions when all match outer but none match inner"
        );
    }

    #[test]
    fn test_chained_global_snapshot_invariant_no_mutation_leakage() {
        // Critical: inner :g MUST use the same text snapshot as outer.
        // Proof: if mutations leaked, the second matching line's text would differ.
        //
        // Text: "ax\nbx\nab\ncx\n"
        // :g/a/g/x/d
        // Lines matching 'a': "ax"(0), "ab"(2)
        // Of those, matching 'x': "ax"(0) only
        // => Exactly 1 delete.
        //
        // Bug scenario: if inner :g saw mutated text after deleting "ax",
        // line indices would shift and "ab" would move to index 1, but
        // the implementation passes `matching_lines` (original indices)
        // to the recursive call, so this can't happen.
        let text = "ax\nbx\nab\ncx\n";
        let ctx = ExContext::new(text, 0);
        let range = ExRange::entire_file();

        let inner_cmd = ExCommand::Global {
            range: ExRange::entire_file(),
            pattern: "x".into(),
            command: Box::new(ExCommand::Delete {
                range: ExRange::current_line(),
                register: None,
            }),
            invert: false,
        };

        let result = global(&range, "a", &inner_cmd, false, &ctx).unwrap();

        let delete_count = result
            .as_slice()
            .iter()
            .filter(|e| matches!(e, Effect::Delete { .. }))
            .count();
        assert_eq!(
            delete_count, 1,
            "snapshot invariant: only 'ax' deleted, 'ab' does not match /x/"
        );
    }

    #[test]
    fn test_chained_global_empty_outer_short_circuits() {
        // :g/nomatch/g/anything/d — outer pattern has zero matches.
        // Should short-circuit with PatternNotFound, inner never runs.
        let text = "hello\nworld\n";
        let ctx = ExContext::new(text, 0);
        let range = ExRange::entire_file();

        let inner_cmd = ExCommand::Global {
            range: ExRange::entire_file(),
            pattern: "anything".into(),
            command: Box::new(ExCommand::Delete {
                range: ExRange::current_line(),
                register: None,
            }),
            invert: false,
        };

        let result = global(&range, "nomatch", &inner_cmd, false, &ctx).unwrap();

        // Zero deletes
        let delete_count = result
            .as_slice()
            .iter()
            .filter(|e| matches!(e, Effect::Delete { .. }))
            .count();
        assert_eq!(delete_count, 0);

        // Should have PatternNotFound error shown
        let has_pattern_not_found = result.as_slice().iter().any(|e| {
            matches!(e, Effect::ShowError { error, .. } if matches!(error, VimError::PatternNotFound(p) if p.as_str() == "nomatch"))
        });
        assert!(
            has_pattern_not_found,
            "outer :g with no matches should emit PatternNotFound"
        );
    }
}
