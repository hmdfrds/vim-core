//! Substitute command (`:s/pattern/replacement/flags`).
//!
//! Regex-based search and replace within a line range.

use super::range::resolve_range;
use super::types::{ExContext, ExResult};
use crate::commands::helpers;
use crate::effects::Effects;
use crate::grammar::types::ExRange;
use crate::primitives::byte_delta;
use crate::primitives::{CaseSensitivity, Offset, Range, SubFlags, SubstitutePreviewMatch};
use crate::primitives::{ConfirmMatchPayload, SubstituteConfirmPayload};
use crate::regex::replacement::apply_replacement;
use crate::regex::{Cache, MatchContext, VimRegex};
use compact_str::CompactString;

/// Apply a signed offset to a `usize` base, clamping to `[0, max]`.
#[inline]
fn saturating_offset(base: usize, shift: isize, max: usize) -> usize {
    if shift >= 0 {
        base.saturating_add(shift.unsigned_abs()).min(max)
    } else {
        base.saturating_sub(shift.unsigned_abs())
    }
}

/// Execute substitute command.
///
/// # Arguments
/// * `range` - Lines to operate on
/// * `pattern` - Regex pattern to match (empty = reuse via `RE_LAST`)
/// * `replacement` - Replacement text (supports `\1`, `&`, etc.)
/// * `flags` - Substitute flags (g, c, i, I, n, r, &)
/// * `ctx` - Execution context
///
/// # Errors
///
/// Returns `VimError::PatternNotFound` for invalid regex or `VimError::InvalidRange` for invalid range.
pub fn substitute(
    range: &ExRange,
    pattern: &str,
    replacement: &str,
    flags: SubFlags,
    ctx: &ExContext,
) -> ExResult {
    let resolved = resolve_range(range, ctx)?;

    // `r` flag: use the last `/`-search pattern instead of the `:s` pattern.
    // When `r` is set the caller-supplied `pattern` is discarded and we use
    // the search register (`RE_SEARCH`) directly.
    let used_r_flag = flags.use_last_search();
    let pattern = if used_r_flag {
        ctx.last_search_pattern
            .ok_or_else(|| crate::errors::VimError::PatternNotFound(CompactString::new("")))?
    } else {
        pattern
    };

    // Resolve the effective pattern: when empty, use the two-pattern RE_LAST system.
    // When there's no previous pattern, Vim returns E476 ("Invalid command"),
    // not E486 ("Pattern not found").
    let effective_pattern = if pattern.is_empty() {
        ctx.resolved_substitute_pattern
            .ok_or(crate::errors::VimError::InvalidCommand)?
    } else {
        pattern
    };

    // Compile pattern with our Vim regex engine (handles \c, \C, \v, \V, magic natively)
    let re = VimRegex::new(effective_pattern)
        .map_err(|e| crate::errors::VimError::PatternNotFound(format!("{e}").into()))?;

    // When an explicit pattern is given, update both pattern stores.
    // SetSearchPattern for hlsearch, SetSubstitutePattern for RE_SUBST.
    let mut effects = if pattern.is_empty() {
        // Empty pattern: don't update any pattern stores — just reuse the resolved one.
        // We still emit SetSearchPattern for hlsearch to highlight the resolved pattern.
        Effects::new().set_search_pattern(effective_pattern, crate::primitives::Direction::Forward)
    } else if used_r_flag {
        // `r` flag: pattern came from RE_SEARCH — only update search pattern for hlsearch.
        // Do NOT emit SetSubstitutePattern; we borrowed from RE_SEARCH and must not
        // overwrite RE_SUBST with the search pattern.
        Effects::new().set_search_pattern(pattern, crate::primitives::Direction::Forward)
    } else {
        // Explicit user-supplied pattern: update search pattern (hlsearch) AND substitute pattern (RE_SUBST).
        Effects::new()
            .set_search_pattern(pattern, crate::primitives::Direction::Forward)
            .set_substitute_pattern(pattern)
    };

    // XOR the parsed `g` flag with `gdefault` — when gdefault is true, the
    // meaning of `g` is inverted: bare `:s` replaces globally, while `:s/…/…/g`
    // replaces only the first occurrence on each line.
    let effective_global = flags.global() ^ ctx.gdefault;

    // Determine case sensitivity from flags.
    // Default and CaseSensitive both produce case-sensitive matching;
    // only an explicit `i` flag makes matching case-insensitive.
    let case_sensitive = match flags.case() {
        CaseSensitivity::Default | CaseSensitivity::CaseSensitive => true,
        CaseSensitivity::IgnoreCase => false,
    };

    // Allocate one Cache and reuse it for the entire line loop, so the
    // per-line match does not reallocate engine scratch space.
    let mut cache = re.create_cache();

    // Confirm mode: collect matches, enter interactive confirm session
    if flags.confirm() {
        return substitute_confirm_start(
            &SubstituteJob {
                re: &re,
                resolved: &resolved,
                effective_pattern,
                replacement,
                effective_global,
                case_sensitive,
                flags,
                ctx,
            },
            &mut cache,
            effects,
        );
    }

    // Multi-line pattern: when pattern contains \n or \_, join lines and
    // run the regex on the joined text instead of per-line.
    if crate::commands::motions::search::pattern_might_span_lines(effective_pattern) {
        return substitute_multiline(
            &SubstituteJob {
                re: &re,
                resolved: &resolved,
                effective_pattern,
                replacement,
                effective_global,
                case_sensitive,
                flags,
                ctx,
            },
            &mut cache,
            effects,
        );
    }

    // Bloom pre-filter: sorted candidate lines whose leaf might contain the pattern
    let bloom_candidates: Option<Vec<usize>> = ctx.tree.and_then(|tree| {
        if !case_sensitive {
            return None;
        }
        re.bloom_literal()
            .map(|literal| tree.find_matching_lines(literal, resolved.start()..resolved.end() + 1))
    });

    let mut total_replacements = 0usize;
    let mut total_lines_changed = 0usize;
    let mut last_replace_info: Option<(usize, String)> = None;
    let mut first_replace_start: Option<usize> = None;
    let mut offset_shift_before_end_line: isize = 0;

    let mut offset_shift: isize = 0;
    let mut running_doc_len = ctx.text.len();

    // Batch: collect per-line results, then emit a single Replace covering
    // from the first matching line to the last matching line.
    let mut first_match_line: Option<usize> = None;
    let mut last_match_line: Option<usize> = None;
    // Indexed by (line_idx - resolved.start()). Each entry is either the
    // replaced text (Some) or None (use original line_text).
    let line_count = resolved.end() - resolved.start() + 1;
    let mut line_results: Vec<Option<String>> = Vec::with_capacity(line_count);

    // Precompute line starts for the resolved range in a single forward scan.
    // This avoids O(n²) from calling ctx.line_range() per line (each scans
    // from the start of the document).
    let precomputed_lines: Vec<(Offset, &str)> = {
        let range_start_offset = helpers::line_start(ctx.text, resolved.start()).unwrap_or(0);
        let mut lines = Vec::with_capacity(line_count);
        let mut pos = range_start_offset;
        for _ in resolved.start()..=resolved.end() {
            let line_end = ctx.text[pos..]
                .find('\n')
                .map_or(ctx.text.len(), |p| pos + p);
            lines.push((Offset::new(pos), &ctx.text[pos..line_end]));
            pos = line_end + 1;
        }
        lines
    };

    let mut bloom_iter_idx = 0usize;
    for (rel_idx, line_idx) in (resolved.start()..=resolved.end()).enumerate() {
        if line_idx == resolved.end() {
            offset_shift_before_end_line = offset_shift;
        }

        let bloom_skip = if let Some(ref candidates) = bloom_candidates {
            while bloom_iter_idx < candidates.len() && candidates[bloom_iter_idx] < line_idx {
                bloom_iter_idx += 1;
            }
            let is_candidate =
                bloom_iter_idx < candidates.len() && candidates[bloom_iter_idx] == line_idx;
            !is_candidate
        } else {
            false
        };

        let (line_start, line_text) = precomputed_lines[rel_idx];

        // Bloom-skipped lines: no replacement.
        if bloom_skip {
            line_results.push(None);
            continue;
        }

        let line_ctx = build_line_ctx(line_text, case_sensitive, ctx.last_substitute);

        if flags.count_only() {
            total_replacements += re
                .find_all_with_cache(&mut cache, &line_ctx)
                .map_err(|e| crate::errors::VimError::PatternNotFound(format!("{e}").into()))?
                .len();
            line_results.push(None);
            continue;
        }

        let (new_line, line_replacements) = replace_in_line(
            &re,
            &mut cache,
            line_text,
            &line_ctx,
            replacement,
            effective_global,
            ctx.last_substitute,
        )?;

        if line_replacements > 0 {
            let actual_start = saturating_offset(line_start.get(), offset_shift, running_doc_len);

            last_replace_info = Some((actual_start, new_line.clone()));
            if first_replace_start.is_none() {
                first_replace_start = Some(actual_start);
            }
            if first_match_line.is_none() {
                first_match_line = Some(line_idx);
            }
            last_match_line = Some(line_idx);

            line_results.push(Some(new_line.clone()));

            let len_diff = byte_delta::delta(new_line.len(), line_text.len());
            offset_shift += len_diff;
            running_doc_len = saturating_offset(running_doc_len, len_diff, usize::MAX);

            total_replacements += line_replacements;
            total_lines_changed += 1;
        } else {
            line_results.push(None);
        }
    }

    // Emit a single Replace covering from the first matching line to the
    // last matching line. Non-matching lines between them are included
    // verbatim so the Replace range is contiguous.
    //
    // `first_match_line`/`last_match_line` are set together, and only on the
    // branch that also increments `total_replacements`; that branch is skipped
    // entirely under `count_only`. So both being `Some` is exactly the old
    // `total_replacements > 0 && !flags.count_only()` condition, without the
    // unwrap.
    if let (Some(first_ml), Some(last_ml)) = (first_match_line, last_match_line) {
        let batch_start = ctx
            .line_range(first_ml)
            .ok_or(crate::errors::VimError::InvalidRange)?
            .0
            .get();
        let last_ml_text = ctx
            .line_text(last_ml)
            .ok_or(crate::errors::VimError::InvalidRange)?;
        let batch_end = ctx
            .line_range(last_ml)
            .ok_or(crate::errors::VimError::InvalidRange)?
            .0
            .get()
            + last_ml_text.len();

        let mut result_text =
            String::with_capacity(batch_end - batch_start + offset_shift.unsigned_abs());
        for line_idx in first_ml..=last_ml {
            if line_idx > first_ml {
                result_text.push('\n');
            }
            let slot = line_idx - resolved.start();
            if let Some(ref replaced) = line_results[slot] {
                result_text.push_str(replaced);
            } else {
                let lt = ctx
                    .line_text(line_idx)
                    .ok_or(crate::errors::VimError::InvalidRange)?;
                result_text.push_str(lt);
            }
        }

        effects = effects.replace(
            Range::new(Offset::new(batch_start), Offset::new(batch_end)),
            &result_text,
        );
    }

    // Emit change marks for the substituted range.
    // Neovim sets [/] to line-start of first/last affected lines.
    if total_replacements > 0 {
        // Mark `[`: line-start of first line in the resolved range.
        let mark_bracket_start = ctx.line_range(resolved.start()).map_or(0, |(ls, _)| {
            if resolved.start() == 0 {
                0
            } else {
                saturating_offset(ls.get(), 0, running_doc_len)
            }
        });
        // Mark `.`: Neovim's do_sub() tracks sub_firstlnum — the first
        // line where a replacement actually occurred — and calls
        // changed_lines(sub_firstlnum, 0, ...) which sets mark '.'.
        let mark_dot = first_replace_start.unwrap_or(mark_bracket_start);
        effects = effects
            .set_mark(
                crate::primitives::MarkName::CHANGE_START,
                Offset::new(mark_bracket_start),
                None,
            )
            .set_mark(
                crate::primitives::MarkName::LAST_CHANGE,
                Offset::new(mark_dot),
                None,
            );
        // ] mark: start of the last line in the range, adjusted for offset
        // shifts from prior lines only (NOT including the end line's own shifts).
        // When the last replacement creates new lines (\r), use the start of
        // the last resulting line instead.
        // For count-only, use mark_bracket_start as fallback.
        let mark_end_pos = if let Some((line_start, ref new_line)) = last_replace_info {
            if let Some(last_nl) = new_line.rfind('\n') {
                // Multi-line result: ] = start of last resulting line
                line_start + last_nl + 1
            } else {
                // Single-line result: use the last line in the range,
                // adjusted for prior-line shifts only.
                ctx.line_range(resolved.end())
                    .map_or(mark_bracket_start, |(ls, _)| {
                        saturating_offset(ls.get(), offset_shift_before_end_line, running_doc_len)
                    })
            }
        } else {
            mark_bracket_start
        };
        effects = effects.set_mark(
            crate::primitives::MarkName::CHANGE_END,
            Offset::new(mark_end_pos),
            None,
        );
    }

    if let Some((line_start, ref new_line)) = last_replace_info {
        // Vim places cursor at first non-blank of the last resulting line.
        // When \r in replacement creates new lines, cursor goes to last line.
        let last_line_start = new_line.rfind('\n').map_or(0, |p| p + 1);
        let last_line = &new_line[last_line_start..];
        let fnb = last_line.find(|c: char| c != ' ' && c != '\t').unwrap_or(0);
        let cursor_pos = line_start + last_line_start + fnb;
        effects = effects.set_cursor(Offset::new(cursor_pos));
        // Explicitly set curswant: ex commands go through sync_effects (not
        // process_effects) which lacks auto-emit for SetStickyColumn.
        // Use fnb as the virtual column — correct for spaces; for tabs,
        // byte_to_vcol would be needed but ExContext lacks tabstop.
        effects.push(crate::effects::Effect::SetStickyColumn {
            column: Some(crate::primitives::VirtualColumn::new(fnb)),
        });
    }

    // Store the replacement string and flags for `:&` / `:&&` repeat.
    if total_replacements > 0 {
        effects = effects.set_last_substitute(replacement);
    }
    // Always store the flags so `:&&` can repeat them even if there were no matches.
    effects = effects.set_last_substitute_flags(flags);

    effects = emit_result_message(
        effects,
        flags,
        total_replacements,
        total_lines_changed,
        effective_pattern,
    );
    Ok(effects)
}

/// Multi-line substitute: join lines and run regex on the joined text.
///
/// Used when the pattern contains `\n` or `\_` (multi-line character class).
/// Joins all lines in the resolved range with `\n`, runs the regex on the
/// whole blob, performs replacement, and emits a single Replace effect.
/// One `:substitute` invocation, minus the mutable scratch the strategies
/// need. `substitute_multiline` and `substitute_confirm_start` took the same
/// nine values in the same order; they now take this record plus the `Cache`
/// and the `Effects` accumulator they mutate.
struct SubstituteJob<'a, 'text> {
    /// Compiled search pattern.
    re: &'a VimRegex,
    /// Line range the command applies to.
    resolved: &'a super::types::ResolvedRange,
    /// Pattern source text, after `~`/empty-pattern resolution.
    effective_pattern: &'a str,
    /// Replacement text, before per-match expansion.
    replacement: &'a str,
    /// Whether every match on a line is replaced (`g`, possibly inverted by
    /// `'gdefault'`).
    effective_global: bool,
    /// Whether matching is case sensitive.
    case_sensitive: bool,
    /// The parsed `:s` flags.
    flags: SubFlags,
    /// Ex-command execution context (document text, options, marks).
    ctx: &'a ExContext<'text>,
}

fn substitute_multiline(
    job: &SubstituteJob<'_, '_>,
    cache: &mut Cache,
    mut effects: Effects,
) -> ExResult {
    let &SubstituteJob {
        re,
        resolved,
        effective_pattern,
        replacement,
        effective_global,
        case_sensitive,
        flags,
        ctx,
    } = job;
    // Build the joined text for the range
    let range_start_offset = helpers::line_start(ctx.text, resolved.start()).unwrap_or(0);
    let range_end = ctx
        .line_range(resolved.end())
        .ok_or(crate::errors::VimError::InvalidRange)?;
    let range_end_offset = range_end.0.get()
        + ctx
            .line_text(resolved.end())
            .ok_or(crate::errors::VimError::InvalidRange)?
            .len();
    let joined = &ctx.text[range_start_offset..range_end_offset];

    let joined_ctx = build_line_ctx(joined, case_sensitive, ctx.last_substitute);

    let matches = if effective_global {
        re.find_all_with_cache(cache, &joined_ctx)
            .map_err(|e| crate::errors::VimError::PatternNotFound(format!("{e}").into()))?
    } else {
        re.find_with_cache(cache, &joined_ctx)
            .map_err(|e| crate::errors::VimError::PatternNotFound(format!("{e}").into()))?
            .into_iter()
            .collect::<Vec<_>>()
    };

    if flags.count_only() {
        let total = matches.len();
        effects = emit_result_message(effects, flags, total, 0, effective_pattern);
        effects = effects.set_last_substitute_flags(flags);
        return Ok(effects);
    }

    if matches.is_empty() {
        effects = emit_result_message(effects, flags, 0, 0, effective_pattern);
        effects = effects.set_last_substitute_flags(flags);
        return Ok(effects);
    }

    // Build replacement text
    let mut result = String::with_capacity(joined.len());
    let mut last_end = 0;
    let mut total_replacements = 0;
    let mut prev_was_nonempty = false;

    for m in &matches {
        let is_empty = m.range.start == m.range.end;
        if is_empty && m.range.start == last_end && prev_was_nonempty {
            continue;
        }
        if is_empty && m.range.start == joined.len() {
            continue;
        }

        if let Some(before) = joined.get(last_end..m.range.start) {
            result.push_str(before);
        }

        let rep_text = apply_replacement(joined, m, replacement, ctx.last_substitute)
            .unwrap_or_else(|_| replacement.to_owned());
        result.push_str(&rep_text);

        total_replacements += 1;
        last_end = m.range.end;
        prev_was_nonempty = !is_empty;
    }
    if let Some(after) = joined.get(last_end..) {
        result.push_str(after);
    }

    // Count affected lines
    let total_lines_changed = joined
        .bytes()
        .filter(|&b| b == b'\n')
        .count()
        .saturating_add(1);

    effects = effects.replace(
        Range::new(
            Offset::new(range_start_offset),
            Offset::new(range_end_offset),
        ),
        &result,
    );

    // Cursor at first non-blank of last resulting line
    let last_line_start = result.rfind('\n').map_or(0, |p| p + 1);
    let last_line = &result[last_line_start..];
    let fnb = last_line.find(|c: char| c != ' ' && c != '\t').unwrap_or(0);
    let cursor_pos = range_start_offset + last_line_start + fnb;
    effects = effects.set_cursor(Offset::new(cursor_pos));
    effects.push(crate::effects::Effect::SetStickyColumn {
        column: Some(crate::primitives::VirtualColumn::new(fnb)),
    });

    // Marks
    effects = effects.set_mark(
        crate::primitives::MarkName::CHANGE_START,
        Offset::new(range_start_offset),
        None,
    );
    let mark_end = range_start_offset + result.len();
    effects = effects.set_mark(
        crate::primitives::MarkName::CHANGE_END,
        Offset::new(mark_end),
        None,
    );
    effects = effects.set_mark(
        crate::primitives::MarkName::LAST_CHANGE,
        Offset::new(range_start_offset),
        None,
    );

    if total_replacements > 0 {
        effects = effects.set_last_substitute(replacement);
    }
    effects = effects.set_last_substitute_flags(flags);

    effects = emit_result_message(
        effects,
        flags,
        total_replacements,
        total_lines_changed,
        effective_pattern,
    );
    Ok(effects)
}

fn substitute_confirm_start(
    job: &SubstituteJob<'_, '_>,
    cache: &mut Cache,
    mut effects: Effects,
) -> ExResult {
    let &SubstituteJob {
        re,
        resolved,
        effective_pattern,
        replacement,
        effective_global,
        case_sensitive,
        flags,
        ctx,
    } = job;
    let mut all_matches = Vec::new();
    for line_idx in resolved.start()..=resolved.end() {
        let (line_start, _) = ctx
            .line_range(line_idx)
            .ok_or(crate::errors::VimError::InvalidRange)?;
        let line_text = ctx
            .line_text(line_idx)
            .ok_or(crate::errors::VimError::InvalidRange)?;
        let line_ctx = build_line_ctx(line_text, case_sensitive, ctx.last_substitute);
        let matches = if effective_global {
            re.find_all_with_cache(cache, &line_ctx)
                .map_err(|e| crate::errors::VimError::PatternNotFound(format!("{e}").into()))?
        } else {
            re.find_with_cache(cache, &line_ctx)
                .map_err(|e| crate::errors::VimError::PatternNotFound(format!("{e}").into()))?
                .into_iter()
                .collect()
        };
        for m in &matches {
            if m.range.start == m.range.end && m.range.start == line_text.len() {
                continue;
            }
            all_matches.push(ConfirmMatchPayload {
                range: Range::new(
                    Offset::new(line_start.get() + m.range.start),
                    Offset::new(line_start.get() + m.range.end),
                ),
                line_idx,
                line_start,
                line_text: CompactString::from(line_text),
            });
        }
    }
    if all_matches.is_empty() {
        if !flags.suppress_error() {
            effects = effects.show_error(crate::errors::VimError::PatternNotFound(
                effective_pattern.into(),
            ));
        }
        return Ok(effects);
    }
    let total = all_matches.len();
    let first_range = all_matches[0].range;
    let payload = SubstituteConfirmPayload {
        matches: all_matches,
        replacement: CompactString::from(replacement),
        pattern: CompactString::from(effective_pattern),
        flags,
        gdefault: ctx.gdefault,
    };
    effects = effects.set_substitute_confirm_state(payload);
    effects =
        effects.substitute_confirm_show(first_range, replacement, 1u32, byte_delta::to_u32(total));
    Ok(effects)
}

/// Build a `MatchContext` for a single line.
fn build_line_ctx<'a>(
    line_text: &'a str,
    case_sensitive: bool,
    last_substitute: Option<&'a str>,
) -> MatchContext<'a> {
    // Mirrors evolve's build_line_ctx: no cursor for a per-line context, and
    // `last_substitute` set only when present -- the builder takes `&str`, so
    // leaving it uncalled is how "no previous substitute" is expressed and `~`
    // stays unset rather than binding to an empty string.
    let mut builder = MatchContext::builder(line_text).case_sensitive(case_sensitive);
    if let Some(sub) = last_substitute {
        builder = builder.last_substitute(sub);
    }
    builder.build()
}

/// Replace matches in a single line, returning the new line and replacement count.
///
/// # Errors
///
/// Returns `VimError::PatternNotFound` if the regex engine reports an error
/// (e.g., `PatternTooComplex`).
fn replace_in_line(
    re: &VimRegex,
    cache: &mut Cache,
    line_text: &str,
    ctx: &MatchContext<'_>,
    replacement: &str,
    global: bool,
    previous_replacement: Option<&str>,
) -> Result<(String, usize), crate::errors::VimError> {
    let matches = if global {
        re.find_all_with_cache(cache, ctx)
            .map_err(|e| crate::errors::VimError::PatternNotFound(format!("{e}").into()))?
    } else {
        // Non-global: only first match
        re.find_with_cache(cache, ctx)
            .map_err(|e| crate::errors::VimError::PatternNotFound(format!("{e}").into()))?
            .into_iter()
            .collect()
    };

    if matches.is_empty() {
        return Ok((String::new(), 0));
    }

    let mut new_line = String::with_capacity(line_text.len());
    let mut last_end = 0;
    let mut count = 0;
    let mut prev_was_nonempty = false;

    for m in &matches {
        let is_empty = m.range.start == m.range.end;

        // Vim skips empty matches that occur at the exact end of the previous
        // non-empty match. It also skips trailing empty matches at end of line.
        if is_empty && m.range.start == last_end && prev_was_nonempty {
            // Skip this empty match — Vim does not replace here
            continue;
        }
        // Vim does not insert a replacement for an empty match at end of string.
        if is_empty && m.range.start == line_text.len() {
            continue;
        }

        // Copy text before match
        if let Some(before) = line_text.get(last_end..m.range.start) {
            new_line.push_str(before);
        }

        // Apply replacement (ignore errors from \= — fall back to literal)
        let rep_text = apply_replacement(line_text, m, replacement, previous_replacement)
            .unwrap_or_else(|_| replacement.to_owned());
        new_line.push_str(&rep_text);

        count += 1;
        last_end = m.range.end;
        prev_was_nonempty = !is_empty;
    }

    // Copy remaining text
    if let Some(after) = line_text.get(last_end..) {
        new_line.push_str(after);
    }

    Ok((new_line, count))
}

/// Emit the appropriate result message effect.
fn emit_result_message(
    mut effects: Effects,
    flags: SubFlags,
    total_replacements: usize,
    total_lines_changed: usize,
    pattern: &str,
) -> Effects {
    if flags.count_only() {
        effects = effects.show_message(format!("{total_replacements} matches"));
    } else if total_replacements > 0 {
        let msg = if total_lines_changed > 1 {
            format!("{total_replacements} substitutions on {total_lines_changed} lines")
        } else {
            format!("{total_replacements} substitutions")
        };
        effects = effects.show_message(msg);
    } else if !flags.suppress_error() {
        effects = effects.show_error(crate::errors::VimError::PatternNotFound(pattern.into()));
    }
    effects
}

// ═══════════════════════════════════════════════════════════════════════════════
// PREVIEW (inccommand dry-run)
// ═══════════════════════════════════════════════════════════════════════════════

/// Compute substitute preview matches without modifying the document.
///
/// This is the "dry-run" version of [`substitute()`] for inccommand preview.
/// It finds all regex matches in the specified line range and computes what
/// each replacement would produce, but emits no effects and modifies no state.
///
/// Returns an empty `Vec` if the pattern is invalid or no matches are found.
/// The `max_matches` parameter caps the total results to bound computation time.
///
/// # Arguments
///
/// * `text` — the full document text
/// * `start_line` — 0-indexed start line of the range
/// * `end_line` — 0-indexed end line of the range (inclusive)
/// * `pattern` — the Vim regex pattern to search for
/// * `replacement` — the Vim replacement string (supports `&`, `\1`, etc.)
/// * `global` — whether to match all occurrences per line (`g` flag, already XOR'd with `gdefault`)
/// * `max_matches` — cap on total results returned
/// * `include_original_lines` — when `true`, each match carries the original
///   line text for split-preview diff rendering (`inccommand=split`)
#[must_use]
pub fn compute_preview_matches(
    text: &str,
    start_line: usize,
    end_line: usize,
    pattern: &str,
    replacement: &str,
    global: bool,
    max_matches: usize,
    include_original_lines: bool,
) -> Vec<SubstitutePreviewMatch> {
    // Empty pattern → empty results (user is still typing)
    if pattern.is_empty() {
        return Vec::new();
    }

    // Compile the regex; if it fails, return empty (user is mid-keystroke)
    let re = match VimRegex::new(pattern) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    let mut cache = re.create_cache();
    let mut results = Vec::new();

    for line_idx in start_line..=end_line {
        let line_start_offset = match helpers::line_start(text, line_idx) {
            Some(o) => o,
            None => break, // line out of range
        };

        let line_text = match helpers::line_content(text, line_idx) {
            Some(t) => t,
            None => break,
        };

        let line_ctx = MatchContext::simple(line_text);

        let matches = if global {
            re.find_all_with_cache(&mut cache, &line_ctx)
                .unwrap_or_default()
        } else {
            re.find_with_cache(&mut cache, &line_ctx)
                .ok()
                .flatten()
                .into_iter()
                .collect()
        };

        for m in &matches {
            if results.len() >= max_matches {
                return results;
            }

            let rep_text = apply_replacement(line_text, m, replacement, None)
                .unwrap_or_else(|_| replacement.to_owned());

            let preview_match = if include_original_lines {
                SubstitutePreviewMatch::with_original_line(
                    Offset::new(line_start_offset + m.range.start),
                    Offset::new(line_start_offset + m.range.end),
                    CompactString::from(rep_text),
                    CompactString::from(line_text),
                )
            } else {
                SubstitutePreviewMatch::new(
                    Offset::new(line_start_offset + m.range.start),
                    Offset::new(line_start_offset + m.range.end),
                    CompactString::from(rep_text),
                )
            };
            results.push(preview_match);
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;

    fn ctx() -> ExContext<'static> {
        ExContext::new("hello world\nfoo bar baz\nhello again", 0)
    }

    #[test]
    fn test_substitute_single() {
        let range = ExRange::current_line();
        let result = substitute(&range, "world", "rust", SubFlags::default(), &ctx()).unwrap();

        assert!(result.iter().any(|e| matches!(e, Effect::Replace { .. })));
    }

    #[test]
    fn test_substitute_global() {
        let range = ExRange::single_line(2);
        let flags = SubFlags::parse("g");
        let result = substitute(&range, "a", "X", flags, &ctx()).unwrap();

        // Line "foo bar baz" should have "a" replaced globally
        assert!(result.iter().any(|e| matches!(e, Effect::Replace { .. })));
    }

    #[test]
    fn test_substitute_count_only() {
        let range = ExRange::entire_file();
        let flags = SubFlags::parse("n");
        let result = substitute(&range, "hello", "", flags, &ctx()).unwrap();

        // Should only show message, no replacement
        assert!(result.iter().any(|e| matches!(e, Effect::ShowInfo { .. })));
        assert!(!result.iter().any(|e| matches!(e, Effect::Replace { .. })));
    }

    #[test]
    fn test_substitute_case_insensitive() {
        let ctx = ExContext::new("Hello HELLO hello", 0);
        let range = ExRange::current_line();
        let flags = SubFlags::parse("gi");
        let result = substitute(&range, "hello", "X", flags, &ctx).unwrap();

        // Should match all three
        assert!(result
            .iter()
            .any(|e| matches!(e, Effect::ShowInfo { info: crate::effects::InfoMessage::Text(text), .. } if text.contains("3"))));
    }

    #[test]
    fn test_pattern_not_found() {
        let range = ExRange::current_line();
        let result = substitute(&range, "xyz", "a", SubFlags::default(), &ctx()).unwrap();

        assert!(result.iter().any(|e| matches!(e, Effect::ShowError { .. })));
    }

    #[test]
    fn test_set_last_substitute_emitted() {
        let range = ExRange::current_line();
        let result = substitute(&range, "hello", "REPLACED", SubFlags::default(), &ctx()).unwrap();

        assert!(result.iter().any(|e| matches!(
            e,
            Effect::SetLastSubstitute { replacement } if replacement.as_str() == "REPLACED"
        )));
    }

    #[test]
    fn test_last_substitute_not_emitted_on_no_match() {
        let range = ExRange::current_line();
        let result = substitute(&range, "xyz", "REPLACED", SubFlags::default(), &ctx()).unwrap();

        assert!(!result
            .iter()
            .any(|e| matches!(e, Effect::SetLastSubstitute { .. })));
    }

    #[test]
    fn test_tilde_uses_last_substitute_in_replacement() {
        let ctx = ExContext::new("hello world", 0).with_last_substitute(Some("PREV"));
        let range = ExRange::current_line();
        let result = substitute(&range, "hello", "~!", SubFlags::default(), &ctx).unwrap();

        // The replacement "~!" with previous="PREV" should produce "PREV!"
        let has_replace = result.iter().any(|e| {
            if let Effect::Replace { text, .. } = e {
                text.contains("PREV!")
            } else {
                false
            }
        });
        assert!(has_replace);
    }

    #[test]
    fn test_tilde_in_pattern_uses_last_substitute() {
        // `~` in pattern matches the last substitute string
        let ctx = ExContext::new("hello PREV world", 0).with_last_substitute(Some("PREV"));
        let range = ExRange::current_line();
        let result = substitute(&range, "~", "FOUND", SubFlags::default(), &ctx).unwrap();

        assert!(result.iter().any(|e| matches!(e, Effect::Replace { .. })));
    }

    // ── compute_preview_matches tests ────────────────────────────────────

    #[test]
    fn preview_simple_match() {
        let text = "foo bar foo";
        let result = compute_preview_matches(text, 0, 0, "foo", "baz", false, 100, false);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].match_start(), Offset::new(0));
        assert_eq!(result[0].match_end(), Offset::new(3));
        assert_eq!(result[0].replacement(), "baz");
    }

    #[test]
    fn preview_global_match() {
        let text = "foo bar foo";
        let result = compute_preview_matches(text, 0, 0, "foo", "baz", true, 100, false);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].match_start(), Offset::new(0));
        assert_eq!(result[0].match_end(), Offset::new(3));
        assert_eq!(result[0].replacement(), "baz");
        assert_eq!(result[1].match_start(), Offset::new(8));
        assert_eq!(result[1].match_end(), Offset::new(11));
        assert_eq!(result[1].replacement(), "baz");
    }

    #[test]
    fn preview_no_match() {
        let text = "foo bar foo";
        let result = compute_preview_matches(text, 0, 0, "xyz", "baz", false, 100, false);
        assert!(result.is_empty());
    }

    #[test]
    fn preview_empty_pattern() {
        let text = "foo bar foo";
        let result = compute_preview_matches(text, 0, 0, "", "baz", false, 100, false);
        assert!(result.is_empty());
    }

    #[test]
    fn preview_invalid_regex() {
        let text = "foo bar foo";
        // Unclosed bracket is invalid regex
        let result = compute_preview_matches(text, 0, 0, "[", "baz", false, 100, false);
        assert!(result.is_empty());
    }

    #[test]
    fn preview_multi_line() {
        let text = "alpha\nbeta\nalpha\ndelta\nalpha";
        // Lines: 0=alpha, 1=beta, 2=alpha, 3=delta, 4=alpha
        // "alpha\n" = 6 bytes, "beta\n" = 5 bytes → line 2 starts at offset 11
        // Search lines 0..=2 for "alpha"
        let result = compute_preview_matches(text, 0, 2, "alpha", "X", false, 100, false);
        assert_eq!(result.len(), 2);
        // Line 0: "alpha" starts at offset 0
        assert_eq!(result[0].match_start(), Offset::new(0));
        assert_eq!(result[0].match_end(), Offset::new(5));
        // Line 2: "alpha" starts at offset 11 (after "alpha\nbeta\n")
        assert_eq!(result[1].match_start(), Offset::new(11));
        assert_eq!(result[1].match_end(), Offset::new(16));
    }

    #[test]
    fn preview_replacement_with_ampersand() {
        let text = "foo bar";
        let result = compute_preview_matches(text, 0, 0, "foo", "(&)", false, 100, false);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].replacement(), "(foo)");
    }

    #[test]
    fn preview_replacement_literal() {
        let text = "foo bar";
        let result = compute_preview_matches(text, 0, 0, "foo", "bar", false, 100, false);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].replacement(), "bar");
    }

    #[test]
    fn preview_line_range_restricts_search() {
        // 5 lines: only search lines 1-2
        let text = "aaa\nbbb\nccc\nddd\neee";
        let result = compute_preview_matches(text, 1, 2, "b\\|c", "X", false, 100, false);
        assert_eq!(result.len(), 2);
        // Line 1 starts at offset 4 ("aaa\n"), "bbb" → first "b" at 4
        assert_eq!(result[0].match_start(), Offset::new(4));
        assert_eq!(result[0].match_end(), Offset::new(5));
        // Line 2 starts at offset 8 ("aaa\nbbb\n"), "ccc" → first "c" at 8
        assert_eq!(result[1].match_start(), Offset::new(8));
        assert_eq!(result[1].match_end(), Offset::new(9));
    }

    #[test]
    fn preview_max_matches_cap() {
        // Create text with many matches
        let text = "aaa\naaa\naaa\naaa\naaa";
        // Global on each line: 3 matches per line × 5 lines = 15 total
        let result = compute_preview_matches(text, 0, 4, "a", "X", true, 5, false);
        assert_eq!(result.len(), 5);
    }

    #[test]
    fn preview_offsets_are_document_global() {
        // Line 0: "hello" (len 5, + newline = 6)
        // Line 1: "world" starts at offset 6
        let text = "hello\nworld";
        let result = compute_preview_matches(text, 1, 1, "world", "X", false, 100, false);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].match_start(), Offset::new(6));
        assert_eq!(result[0].match_end(), Offset::new(11));
    }

    #[test]
    fn preview_single_line_no_range() {
        let text = "hello world";
        let result = compute_preview_matches(text, 0, 0, "world", "X", false, 100, false);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].match_start(), Offset::new(6));
        assert_eq!(result[0].match_end(), Offset::new(11));
        assert_eq!(result[0].replacement(), "X");
    }

    #[test]
    fn preview_case_transform_in_replacement() {
        let text = "hello";
        let result = compute_preview_matches(text, 0, 0, "hello", "\\U&\\e", false, 100, false);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].replacement(), "HELLO");
    }

    #[test]
    fn preview_global_multiple_per_line() {
        let text = "abab";
        let result = compute_preview_matches(text, 0, 0, "ab", "X", true, 100, false);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].match_start(), Offset::new(0));
        assert_eq!(result[0].match_end(), Offset::new(2));
        assert_eq!(result[1].match_start(), Offset::new(2));
        assert_eq!(result[1].match_end(), Offset::new(4));
    }

    #[test]
    fn preview_start_line_beyond_text() {
        let text = "hello";
        let result = compute_preview_matches(text, 5, 10, "hello", "X", false, 100, false);
        assert!(result.is_empty());
    }

    #[test]
    fn preview_max_matches_zero() {
        let text = "foo";
        let result = compute_preview_matches(text, 0, 0, "foo", "bar", false, 0, false);
        assert!(result.is_empty());
    }

    #[test]
    fn preview_capture_group_replacement() {
        // \(\w\+\) captures word, replacement uses \1
        let text = "hello world";
        let result =
            compute_preview_matches(text, 0, 0, "\\(\\w\\+\\)", "[\\1]", false, 100, false);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].replacement(), "[hello]");
    }

    #[test]
    fn preview_empty_replacement() {
        let text = "foo bar";
        let result = compute_preview_matches(text, 0, 0, "foo", "", false, 100, false);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].replacement(), "");
    }

    // ── gdefault tests ──────────────────────────────────────────────────

    /// Helper: create a context with gdefault enabled.
    fn ctx_gdefault() -> ExContext<'static> {
        ExContext::new("hello world\nfoo bar baz\nhello again", 0).with_gdefault(true)
    }

    #[test]
    fn gdefault_bare_substitute_replaces_all() {
        // With gdefault=true, bare `:s` (no `g` flag) should replace all on line
        let ctx = ExContext::new("aXaXa", 0).with_gdefault(true);
        let range = ExRange::current_line();
        let result = substitute(&range, "X", "Y", SubFlags::default(), &ctx).unwrap();

        // Should report 2 substitutions (two X's on the line)
        let msg = result.iter().find_map(|e| {
            if let Effect::ShowInfo {
                info: crate::effects::InfoMessage::Text(text),
                ..
            } = e
            {
                Some(text.as_str().to_owned())
            } else {
                None
            }
        });
        assert!(
            msg.as_deref().is_some_and(|m| m.contains('2')),
            "expected 2 substitutions, got: {msg:?}"
        );
    }

    #[test]
    fn gdefault_g_flag_replaces_first_only() {
        // With gdefault=true, `g` flag inverts: replaces first occurrence only
        let ctx = ExContext::new("aXaXa", 0).with_gdefault(true);
        let range = ExRange::current_line();
        let flags = SubFlags::parse("g"); // g flag now means "first only"
        let result = substitute(&range, "X", "Y", flags, &ctx).unwrap();

        // Should report 1 substitution
        let msg = result.iter().find_map(|e| {
            if let Effect::ShowInfo {
                info: crate::effects::InfoMessage::Text(text),
                ..
            } = e
            {
                Some(text.as_str().to_owned())
            } else {
                None
            }
        });
        assert!(
            msg.as_deref().is_some_and(|m| m.contains('1')),
            "expected 1 substitution (g inverts with gdefault), got: {msg:?}"
        );
    }

    #[test]
    fn gdefault_false_bare_substitute_replaces_first_only() {
        // Control test: gdefault=false, bare `:s` replaces first only
        let ctx = ExContext::new("aXaXa", 0);
        let range = ExRange::current_line();
        let result = substitute(&range, "X", "Y", SubFlags::default(), &ctx).unwrap();

        let msg = result.iter().find_map(|e| {
            if let Effect::ShowInfo {
                info: crate::effects::InfoMessage::Text(text),
                ..
            } = e
            {
                Some(text.as_str().to_owned())
            } else {
                None
            }
        });
        assert!(
            msg.as_deref().is_some_and(|m| m.contains('1')),
            "expected 1 substitution without gdefault, got: {msg:?}"
        );
    }

    #[test]
    fn gdefault_with_multiline_range() {
        // gdefault=true across multiple lines
        let ctx = ExContext::new("aXa\naXa", 0).with_gdefault(true);
        let range = ExRange::entire_file();
        let result = substitute(&range, "X", "Y", SubFlags::default(), &ctx).unwrap();

        // Should replace both X's (one per line, global per line)
        let msg = result.iter().find_map(|e| {
            if let Effect::ShowInfo {
                info: crate::effects::InfoMessage::Text(text),
                ..
            } = e
            {
                Some(text.as_str().to_owned())
            } else {
                None
            }
        });
        assert!(
            msg.as_deref().is_some_and(|m| m.contains('2')),
            "expected 2 substitutions across 2 lines, got: {msg:?}"
        );
    }

    #[test]
    fn gdefault_query_shows_current_value() {
        let ctx = ctx_gdefault();
        assert!(ctx.gdefault);
    }

    #[test]
    fn preview_respects_gdefault_via_xor() {
        // compute_preview_matches receives `global` already XOR'd, so we test
        // that the logic produces the right number of matches.
        let text = "aXaXa";
        // gdefault=true, no g flag → effective_global = false ^ true = true
        let all = compute_preview_matches(text, 0, 0, "X", "Y", true, 100, false);
        assert_eq!(all.len(), 2);

        // gdefault=true, g flag → effective_global = true ^ true = false
        let first = compute_preview_matches(text, 0, 0, "X", "Y", false, 100, false);
        assert_eq!(first.len(), 1);
    }

    // ── Two-pattern system (RE_LAST) tests ───────────────────────────

    #[test]
    fn empty_pattern_reuses_resolved_substitute_pattern() {
        // When `:s//rep/` is used, the resolved substitute pattern is used
        let ctx = ExContext::new("hello world", 0).with_resolved_substitute_pattern(Some("hello"));
        let range = ExRange::current_line();
        let result = substitute(&range, "", "REPLACED", SubFlags::default(), &ctx).unwrap();

        // Should find "hello" via resolved pattern and replace it
        assert!(result.iter().any(|e| matches!(e, Effect::Replace { .. })));
        // Should report success
        assert!(result.iter().any(|e| matches!(e, Effect::ShowInfo { .. })));
    }

    #[test]
    fn empty_pattern_errors_when_no_resolved_pattern() {
        // When `:s//rep/` is used with no previous pattern, should error
        let ctx = ExContext::new("hello world", 0);
        // resolved_substitute_pattern is None (default)
        let result = substitute(
            &ExRange::current_line(),
            "",
            "rep",
            SubFlags::default(),
            &ctx,
        );
        assert!(result.is_err());
    }

    #[test]
    fn explicit_pattern_emits_set_substitute_pattern() {
        // When `:s/explicit/rep/` is used, should emit SetSubstitutePattern
        let ctx = ExContext::new("hello world", 0);
        let range = ExRange::current_line();
        let result = substitute(&range, "hello", "REPLACED", SubFlags::default(), &ctx).unwrap();

        assert!(result.iter().any(|e| matches!(
            e,
            Effect::SetSubstitutePattern { pattern } if pattern.as_str() == "hello"
        )));
    }

    #[test]
    fn explicit_pattern_emits_both_search_and_substitute_patterns() {
        // Explicit pattern updates both stores
        let ctx = ExContext::new("hello world", 0);
        let range = ExRange::current_line();
        let result = substitute(&range, "hello", "REPLACED", SubFlags::default(), &ctx).unwrap();

        // SetSearchPattern for hlsearch
        assert!(result.iter().any(|e| matches!(
            e,
            Effect::SetSearchPattern { pattern, .. } if pattern.as_str() == "hello"
        )));
        // SetSubstitutePattern for RE_SUBST
        assert!(result.iter().any(|e| matches!(
            e,
            Effect::SetSubstitutePattern { pattern } if pattern.as_str() == "hello"
        )));
    }

    #[test]
    fn empty_pattern_does_not_emit_set_substitute_pattern() {
        // When `:s//rep/` is used, should NOT emit SetSubstitutePattern
        // (only reusing, not updating the substitute pattern store)
        let ctx = ExContext::new("hello world", 0).with_resolved_substitute_pattern(Some("hello"));
        let range = ExRange::current_line();
        let result = substitute(&range, "", "REPLACED", SubFlags::default(), &ctx).unwrap();

        assert!(!result
            .iter()
            .any(|e| matches!(e, Effect::SetSubstitutePattern { .. })));
    }

    #[test]
    fn empty_pattern_still_emits_search_pattern_for_hlsearch() {
        // Even when reusing a resolved pattern, we emit SetSearchPattern
        // so hlsearch highlights the pattern
        let ctx = ExContext::new("hello world", 0).with_resolved_substitute_pattern(Some("hello"));
        let range = ExRange::current_line();
        let result = substitute(&range, "", "REPLACED", SubFlags::default(), &ctx).unwrap();

        assert!(result.iter().any(|e| matches!(
            e,
            Effect::SetSearchPattern { pattern, .. } if pattern.as_str() == "hello"
        )));
    }

    // ── :s///r flag (use last search pattern) tests ───────────────────

    #[test]
    fn r_flag_uses_last_search_pattern() {
        // `:s/ignored/REPLACED/r` should use the last search pattern, not "ignored"
        let ctx = ExContext::new("hello world", 0).with_last_search_pattern(Some("hello"));
        let range = ExRange::current_line();
        let flags = SubFlags::parse("r");
        let result = substitute(&range, "ignored", "REPLACED", flags, &ctx).unwrap();

        // Should have replaced "hello" (from last search), not "ignored"
        assert!(result.iter().any(|e| matches!(e, Effect::Replace { .. })));
    }

    #[test]
    fn r_flag_ignores_given_pattern() {
        // With `r` flag, "nonexistent" should be ignored; "hello" (last search) is used
        let ctx = ExContext::new("hello world", 0).with_last_search_pattern(Some("hello"));
        let range = ExRange::current_line();
        let flags = SubFlags::parse("r");
        let result = substitute(&range, "nonexistent", "REPLACED", flags, &ctx).unwrap();

        // "hello" is found via last search pattern — should succeed
        assert!(result.iter().any(|e| matches!(e, Effect::Replace { .. })));
    }

    #[test]
    fn r_flag_errors_when_no_last_search_pattern() {
        // `r` with no previous search should return PatternNotFound error
        let ctx = ExContext::new("hello world", 0);
        // last_search_pattern is None
        let range = ExRange::current_line();
        let flags = SubFlags::parse("r");
        let result = substitute(&range, "", "REPLACED", flags, &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn r_flag_parse() {
        let flags = SubFlags::parse("r");
        assert!(flags.use_last_search());
        assert!(!flags.global());
    }

    #[test]
    fn r_flag_combined_with_g() {
        // `:s/ignored/X/gr` should replace all occurrences of the search pattern
        let ctx = ExContext::new("aa bb aa", 0).with_last_search_pattern(Some("aa"));
        let range = ExRange::current_line();
        let flags = SubFlags::parse("gr");
        let result = substitute(&range, "ignored", "X", flags, &ctx).unwrap();

        // Should report 2 substitutions (two "aa"s)
        let msg = result.iter().find_map(|e| {
            if let Effect::ShowInfo {
                info: crate::effects::InfoMessage::Text(text),
                ..
            } = e
            {
                Some(text.as_str().to_owned())
            } else {
                None
            }
        });
        assert!(
            msg.as_deref().is_some_and(|m| m.contains('2')),
            "expected 2 substitutions, got: {msg:?}"
        );
    }

    // ── :s///& flag (reuse previous flags) tests ─────────────────────

    #[test]
    fn ampersand_flag_parse() {
        let flags = SubFlags::parse("&");
        assert!(flags.reuse_flags());
        assert!(!flags.global());
    }

    #[test]
    fn reuse_flags_flag_detected() {
        let flags = SubFlags::parse("g&");
        assert!(flags.reuse_flags());
        assert!(flags.global()); // explicit g
    }

    // :s///c confirm mode tests

    #[test]
    fn confirm_flag_enters_confirm_mode() {
        let ctx = ExContext::new("foo bar foo", 0);
        let range = ExRange::current_line();
        let flags = SubFlags::parse("gc");
        let result = substitute(&range, "foo", "bar", flags, &ctx).unwrap();
        assert!(result
            .iter()
            .any(|e| matches!(e, Effect::SetSubstituteConfirmState { .. })));
        assert!(result.iter().any(|e| matches!(
            e,
            Effect::SubstituteConfirmShow {
                match_index: 1,
                total_matches: 2,
                ..
            }
        )));
        assert!(!result.iter().any(|e| matches!(e, Effect::Replace { .. })));
    }

    #[test]
    fn confirm_flag_no_matches_shows_error() {
        let ctx = ExContext::new("hello world", 0);
        let range = ExRange::current_line();
        let flags = SubFlags::parse("c");
        let result = substitute(&range, "xyz", "bar", flags, &ctx).unwrap();
        assert!(result.iter().any(|e| matches!(e, Effect::ShowError { .. })));
        assert!(!result
            .iter()
            .any(|e| matches!(e, Effect::SetSubstituteConfirmState { .. })));
    }

    #[test]
    fn confirm_flag_global_all_matches() {
        let ctx = ExContext::new("foo foo foo", 0);
        let range = ExRange::current_line();
        let flags = SubFlags::parse("gc");
        let result = substitute(&range, "foo", "bar", flags, &ctx).unwrap();
        assert!(result.iter().any(|e| matches!(
            e,
            Effect::SubstituteConfirmShow {
                match_index: 1,
                total_matches: 3,
                ..
            }
        )));
    }

    #[test]
    fn confirm_flag_non_global_first_match_only() {
        let ctx = ExContext::new("foo foo foo", 0);
        let range = ExRange::current_line();
        let flags = SubFlags::parse("c");
        let result = substitute(&range, "foo", "bar", flags, &ctx).unwrap();
        assert!(result.iter().any(|e| matches!(
            e,
            Effect::SubstituteConfirmShow {
                match_index: 1,
                total_matches: 1,
                ..
            }
        )));
    }

    #[test]
    fn confirm_state_has_correct_fields() {
        let ctx = ExContext::new("foo bar foo", 0);
        let range = ExRange::current_line();
        let flags = SubFlags::parse("gc");
        let result = substitute(&range, "foo", "bar", flags, &ctx).unwrap();
        let payload = result.iter().find_map(|e| match e {
            Effect::SetSubstituteConfirmState { payload } => Some(payload),
            _ => None,
        });
        assert!(payload.is_some());
        let payload = payload.unwrap();
        assert_eq!(payload.matches.len(), 2);
        assert_eq!(payload.replacement.as_str(), "bar");
        assert_eq!(payload.pattern.as_str(), "foo");
    }

    #[test]
    fn confirm_emits_search_pattern() {
        let ctx = ExContext::new("hello world", 0);
        let range = ExRange::current_line();
        let flags = SubFlags::parse("c");
        let result = substitute(&range, "hello", "bar", flags, &ctx).unwrap();
        assert!(result.iter().any(
            |e| matches!(e, Effect::SetSearchPattern { pattern, .. } if pattern.as_str() == "hello")
        ));
    }

    // ── :~ (SubTilde) dispatch tests ─────────────────────────────────
    //
    // SubTilde is dispatched in dispatch/ex.rs which calls substitute()
    // with the search pattern and last substitute replacement. These tests
    // verify the underlying substitute call works correctly with those inputs.

    #[test]
    fn subtilde_uses_search_pattern_and_last_replacement() {
        // Simulates :~ dispatch: pattern comes from last search, replacement
        // from last substitute.
        let ctx = ExContext::new("hello world", 0);
        let range = ExRange::current_line();
        let result = substitute(&range, "hello", "greetings", SubFlags::default(), &ctx).unwrap();

        // Should replace "hello" with "greetings"
        let has_replace = result.iter().any(|e| {
            if let Effect::Replace { text, .. } = e {
                text.contains("greetings")
            } else {
                false
            }
        });
        assert!(has_replace, "expected 'hello' replaced with 'greetings'");
    }

    // ── :s///e flag (suppress no-match error) tests ────────────────────

    #[test]
    fn e_flag_suppresses_no_match_error() {
        let ctx = ExContext::new("hello world", 0);
        let range = ExRange::current_line();
        let flags = SubFlags::parse("e");
        let result = substitute(&range, "nonexistent", "new", flags, &ctx).unwrap();

        // No ShowError should be emitted
        assert!(!result.iter().any(|e| matches!(e, Effect::ShowError { .. })));
        // No Replace either — nothing matched
        assert!(!result.iter().any(|e| matches!(e, Effect::Replace { .. })));
    }

    #[test]
    fn no_e_flag_emits_no_match_error() {
        let ctx = ExContext::new("hello world", 0);
        let range = ExRange::current_line();
        let result = substitute(&range, "nonexistent", "new", SubFlags::default(), &ctx).unwrap();

        assert!(result.iter().any(|e| matches!(e, Effect::ShowError { .. })));
    }

    #[test]
    fn subtilde_with_global_flag() {
        // Simulates :~g — should replace all occurrences
        let ctx = ExContext::new("hello hello hello", 0);
        let range = ExRange::current_line();
        let flags = SubFlags::parse("g");
        let result = substitute(&range, "hello", "X", flags, &ctx).unwrap();

        let msg = result.iter().find_map(|e| {
            if let Effect::ShowInfo {
                info: crate::effects::InfoMessage::Text(text),
                ..
            } = e
            {
                Some(text.as_str().to_owned())
            } else {
                None
            }
        });
        assert!(
            msg.as_deref().is_some_and(|m| m.contains('3')),
            "expected 3 substitutions, got: {msg:?}"
        );
    }

    // ═══════════════════════════════════════════════════════════════════
    // Inccommand split preview (original_lines)
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn preview_without_original_lines() {
        let text = "hello world\nfoo bar";
        let matches = compute_preview_matches(text, 0, 0, "hello", "HI", false, 100, false);
        assert_eq!(matches.len(), 1);
        assert!(
            matches[0].original_line().is_none(),
            "nosplit mode should not include original lines"
        );
    }

    #[test]
    fn preview_with_original_lines() {
        let text = "hello world\nfoo bar";
        let matches = compute_preview_matches(text, 0, 0, "hello", "HI", false, 100, true);
        assert_eq!(matches.len(), 1);
        assert_eq!(
            matches[0].original_line(),
            Some("hello world"),
            "split mode should include original line text"
        );
        assert_eq!(matches[0].replacement(), "HI");
    }

    #[test]
    fn preview_split_multiple_lines() {
        let text = "foo bar\nbaz foo\nqux";
        let matches = compute_preview_matches(text, 0, 2, "foo", "X", true, 100, true);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].original_line(), Some("foo bar"));
        assert_eq!(matches[1].original_line(), Some("baz foo"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // Multi-line regex in :s
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn multiline_substitute_basic() {
        let text = "foo\nbar\nbaz";
        let ctx = ExContext::new(text, 0);
        let range = ExRange::entire_file();
        let result =
            substitute(&range, "foo\\nbar", "REPLACED", SubFlags::default(), &ctx).unwrap();
        let has_replace = result.iter().any(|e| {
            if let Effect::Replace { text, .. } = e {
                text.contains("REPLACED")
            } else {
                false
            }
        });
        assert!(has_replace, "multi-line pattern should match across lines");
    }

    #[test]
    fn multiline_substitute_preserves_unmatched_lines() {
        let text = "foo\nbar\nbaz";
        let ctx = ExContext::new(text, 0);
        let range = ExRange::entire_file();
        let result = substitute(&range, "foo\\nbar", "X", SubFlags::default(), &ctx).unwrap();
        for e in result.iter() {
            if let Effect::Replace { text, .. } = e {
                assert!(text.contains("baz"), "unmatched lines should remain");
            }
        }
    }

    #[test]
    fn singleline_pattern_uses_fast_path() {
        // A pattern without \n should NOT go through multi-line path
        let text = "foo bar foo";
        let ctx = ExContext::new(text, 0);
        let range = ExRange::current_line();
        let result = substitute(&range, "foo", "X", SubFlags::default(), &ctx).unwrap();
        let has_replace = result.iter().any(|e| matches!(e, Effect::Replace { .. }));
        assert!(has_replace, "single-line pattern should work normally");
    }
}
