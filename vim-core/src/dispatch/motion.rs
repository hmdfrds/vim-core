//! Motion dispatcher.
//!
//! Maps `Grammar::Motion` enum to `commands::motions` implementations.
//! This is the ONLY place to update when adding motions.
//!
//! # Design
//!
//! Grammar layer has flat `Motion` enum for parsing.
//! Commands layer has plain functions organized by module.
//! This dispatcher bridges them via exhaustive match.
//!
//! # Philosophy
//!
//! Plain functions, not trait methods: this dispatcher calls the motion
//! implementations directly (e.g., `char::h(ctx)`), so there is no vtable
//! in the key-handling path.

pub use crate::commands::motions::types::ViewportInfo;
use crate::commands::motions::{
    bracket, char, document, find, indent, line, mark, misc, paragraph, search, search_object,
    section, seek_textobject, sentence, subword, word,
};
pub use crate::commands::motions::{MotionContext, MotionResult, SearchMotion};
use crate::grammar::types::Motion;
use crate::primitives::SelectionShape;

/// Dispatch a `Grammar::Motion` to the appropriate motion implementation.
///
/// This is an exhaustive match - adding a new Motion variant will cause
/// a compile error until handled here.
///
/// # Arguments
/// * `motion` - The motion from Grammar
/// * `ctx` - Motion context with text, cursor, count
///
/// # Returns
/// * `MotionResult` - Position, `LinePosition`, `NeedsViewport`, or Failed
#[inline]
pub fn dispatch_motion(motion: Motion, ctx: &MotionContext<'_>) -> MotionResult {
    match motion {
        // ═══════════════════════════════════════════════════════════════
        // Character motions (h, l, 0, $, ^, g_)
        // ═══════════════════════════════════════════════════════════════
        Motion::Left => char::h(ctx),
        Motion::Right => char::l(ctx),
        Motion::Space => char::space(ctx),
        Motion::LineStart => char::zero(ctx),
        Motion::LineEnd => char::dollar(ctx),
        Motion::FirstNonBlank => char::caret(ctx),
        Motion::LastNonBlank => char::g_underscore(ctx),

        // ═══════════════════════════════════════════════════════════════
        // Line motions (j, k, gj, gk, +, -, _)
        // ═══════════════════════════════════════════════════════════════
        Motion::Up => line::k(ctx),
        Motion::Down => line::j(ctx),
        Motion::DisplayDown => line::gj(ctx),
        Motion::DisplayUp => line::gk(ctx),
        Motion::DownFirstNonBlank => line::plus(ctx),
        Motion::UpFirstNonBlank => line::minus(ctx),
        Motion::FirstNonBlankLine => line::underscore(ctx),

        // ═══════════════════════════════════════════════════════════════
        // Word motions (w, b, e, ge, W, B, E, gE)
        // ═══════════════════════════════════════════════════════════════
        Motion::WordForward => word::w(ctx),
        Motion::WordBackward => word::b(ctx),
        Motion::WordEnd => word::e(ctx),
        Motion::WordEndBackward => word::ge(ctx),
        Motion::WORDForward => word::W(ctx),
        Motion::WORDBackward => word::B(ctx),
        Motion::WORDEnd => word::E(ctx),
        Motion::WORDEndBackward => word::gE(ctx),

        // ═══════════════════════════════════════════════════════════════
        // Document motions (gg, G, %)
        // ═══════════════════════════════════════════════════════════════
        Motion::GotoLine => document::G(ctx),
        Motion::GotoFirstLine => document::gg(ctx),
        Motion::MatchingPair => {
            // In Vim, bare `%` does bracket matching, but `{count}%` goes to
            // percentage of file. We differentiate by count > 1.
            if ctx.explicit_count {
                document::percent(ctx)
            } else {
                bracket::matching_bracket(ctx)
            }
        }

        // ═══════════════════════════════════════════════════════════════
        // Screen motions (H, M, L) - Use document functions
        // ═══════════════════════════════════════════════════════════════
        Motion::ScreenHigh => document::H(ctx),
        Motion::ScreenMiddle => document::M(ctx),
        Motion::ScreenLow => document::L(ctx),

        // ═══════════════════════════════════════════════════════════════
        // Paragraph/Sentence motions ({, }, (, ))
        // ═══════════════════════════════════════════════════════════════
        Motion::ParagraphForward => paragraph::close_brace(ctx),
        Motion::ParagraphBackward => paragraph::open_brace(ctx),
        Motion::SentenceForward => sentence::close_paren(ctx),
        Motion::SentenceBackward => sentence::open_paren(ctx),

        // ═══════════════════════════════════════════════════════════════
        // Section motions ([[, ]], ][ , [])
        // ═══════════════════════════════════════════════════════════════
        Motion::SectionForwardStart => section::section_forward_start(ctx),
        Motion::SectionBackwardStart => section::section_backward_start(ctx),
        Motion::SectionForwardEnd => section::section_forward_end(ctx),
        Motion::SectionBackwardEnd => section::section_backward_end(ctx),

        // ═══════════════════════════════════════════════════════════════
        // Column motion (|)
        // ═══════════════════════════════════════════════════════════════
        Motion::GoToColumn => line::go_to_column(ctx),

        // ═══════════════════════════════════════════════════════════════
        // Screen-line motions (g0, g$, g^)
        // ═══════════════════════════════════════════════════════════════
        Motion::ScreenLineStart => line::g0(ctx),
        Motion::ScreenLineEnd => line::g_dollar(ctx),
        Motion::ScreenFirstNonBlank => line::g_caret(ctx),

        // ═══════════════════════════════════════════════════════════════
        // Misc motions (gm, gM, go)
        // ═══════════════════════════════════════════════════════════════
        Motion::MiddleOfScreenLine => misc::middle_of_screen_line(ctx),
        Motion::MiddleOfTextLine => misc::middle_of_text_line(ctx),
        Motion::GotoByte => misc::goto_byte(ctx),

        // ═══════════════════════════════════════════════════════════════
        // Scroll motions (Ctrl-D, Ctrl-U, Ctrl-F, Ctrl-B, Ctrl-E, Ctrl-Y)
        // ═══════════════════════════════════════════════════════════════
        Motion::ScrollHalfDown => document::ctrl_d(ctx),
        Motion::ScrollHalfUp => document::ctrl_u(ctx),
        Motion::ScrollFullDown => document::ctrl_f(ctx),
        Motion::ScrollFullUp => document::ctrl_b(ctx),
        Motion::ScrollLineDown => document::ctrl_e(ctx),
        Motion::ScrollLineUp => document::ctrl_y(ctx),

        // ═══════════════════════════════════════════════════════════════
        // Search motions (n, N, *, #) - Use SearchMotion
        // ═══════════════════════════════════════════════════════════════
        Motion::SearchNext => SearchMotion::NextMatch.compute(ctx),
        Motion::SearchPrev => SearchMotion::PrevMatch.compute(ctx),
        Motion::WordSearchForward => SearchMotion::WordUnderCursor.compute(ctx),
        Motion::WordSearchBackward => SearchMotion::WordUnderCursorBack.compute(ctx),

        // ═══════════════════════════════════════════════════════════════
        // Find repeat (; ,) - Use last_find from context
        // ═══════════════════════════════════════════════════════════════
        Motion::RepeatFind => match &ctx.last_find {
            Some(last_find) => find::semicolon(ctx, last_find),
            None => MotionResult::Error,
        },
        Motion::RepeatFindReverse => match &ctx.last_find {
            Some(last_find) => find::comma(ctx, last_find),
            None => MotionResult::Error,
        },

        // ═══════════════════════════════════════════════════════════════
        // Changelist (g;, g,) — intercepted by executor → effects.
        // Fallback to Failed if reached from operator-pending context.
        // ═══════════════════════════════════════════════════════════════
        Motion::ChangelistOlder | Motion::ChangelistNewer => MotionResult::NoMotion,

        // ═══════════════════════════════════════════════════════════════
        // Search object (gn, gN)
        // ═══════════════════════════════════════════════════════════════
        Motion::SearchObjectForward => search_object::gn(ctx),
        Motion::SearchObjectBackward => search_object::gN(ctx),

        // ═══════════════════════════════════════════════════════════════
        // Unmatched bracket motions ([{, ]}, [(, ]))
        // ═══════════════════════════════════════════════════════════════
        Motion::PrevUnmatchedBrace => bracket::prev_unmatched_brace(ctx),
        Motion::NextUnmatchedBrace => bracket::next_unmatched_brace(ctx),
        Motion::PrevUnmatchedParen => bracket::prev_unmatched_paren(ctx),
        Motion::NextUnmatchedParen => bracket::next_unmatched_paren(ctx),

        // ═══════════════════════════════════════════════════════════════
        // Method boundary motions ([m, ]m, [M, ]M)
        // ═══════════════════════════════════════════════════════════════
        Motion::PrevMethodStart => bracket::prev_method_start(ctx),
        Motion::NextMethodStart => bracket::next_method_start(ctx),
        Motion::PrevMethodEnd => bracket::prev_method_end(ctx),
        Motion::NextMethodEnd => bracket::next_method_end(ctx),

        // ═══════════════════════════════════════════════════════════════
        // Comment navigation ([/, ]/)
        // ═══════════════════════════════════════════════════════════════
        Motion::PrevCommentStart => bracket::prev_comment_start(ctx),
        Motion::NextCommentEnd => bracket::next_comment_end(ctx),

        // ═══════════════════════════════════════════════════════════════
        // Bracket/quote pair navigation (]b, [b, ]q, [q)
        // ═══════════════════════════════════════════════════════════════
        Motion::NextBracketPair => bracket::find_next_bracket_pair(ctx),
        Motion::PrevBracketPair => bracket::find_prev_bracket_pair(ctx),
        Motion::NextQuotePair => bracket::find_next_quote(ctx),
        Motion::PrevQuotePair => bracket::find_prev_quote(ctx),

        // ═══════════════════════════════════════════════════════════════
        // Partial word search (g*, g#)
        // ═══════════════════════════════════════════════════════════════
        Motion::PartialWordSearchForward => SearchMotion::PartialWord.compute(ctx),
        Motion::PartialWordSearchBackward => SearchMotion::PartialWordBack.compute(ctx),

        // ═══════════════════════════════════════════════════════════════
        // Subword motions (camelCase/snake_case boundaries)
        // ═══════════════════════════════════════════════════════════════
        Motion::SubwordForward => subword::subword_forward(ctx),
        Motion::SubwordBackward => subword::subword_backward(ctx),
        Motion::SubwordEnd => subword::subword_end(ctx),
        Motion::SubwordEndBackward => subword::subword_end_backward(ctx),

        // ═══════════════════════════════════════════════════════════════
        // Mark navigation (]', [')
        // ═══════════════════════════════════════════════════════════════
        Motion::NextMark => mark::next_mark(ctx),
        Motion::PreviousMark => mark::previous_mark(ctx),

        // Indent navigation ([i, ]i, [-, ]-, [+, ]+)
        Motion::PrevSameIndent => indent::prev_same_indent(ctx),
        Motion::NextSameIndent => indent::next_same_indent(ctx),
        Motion::PrevLesserIndent => indent::prev_lesser_indent(ctx),
        Motion::NextLesserIndent => indent::next_lesser_indent(ctx),
        Motion::PrevGreaterIndent => indent::prev_greater_indent(ctx),
        Motion::NextGreaterIndent => indent::next_greater_indent(ctx),

        // ═══════════════════════════════════════════════════════════════
        // Text object seeking (]x, [x)
        // ═══════════════════════════════════════════════════════════════
        Motion::SeekTextObject { kind, direction } => {
            use crate::commands::textobjects::TextObjectContext;
            use crate::grammar::types::{TextObject, TextObjectScope};
            let text = ctx.text;
            let options = ctx.options;
            let providers = ctx.providers;
            let resolve = |pos: usize| -> Option<(usize, usize)> {
                if pos >= text.len() {
                    return None;
                }
                let to_ctx = TextObjectContext::new(text, pos)
                    .with_options(options)
                    .with_providers(providers);
                let object = TextObject {
                    scope: TextObjectScope::Around,
                    kind,
                    seek: None,
                };
                let range = crate::dispatch::textobject::dispatch_textobject(object, &to_ctx)?;
                Some((range.start(), range.end()))
            };
            seek_textobject::seek_text_object(text, ctx.cursor.get(), ctx.count, direction, resolve)
        }

        // ═══════════════════════════════════════════════════════════════
        // Custom (host-registered) motions
        // ═══════════════════════════════════════════════════════════════
        Motion::Custom(id) => {
            if let Some(provider) = ctx.providers.custom_motions {
                if let Some(result) =
                    provider.compute_motion_with_info(id, ctx.text, ctx.cursor.get(), ctx.count)
                {
                    let clamped = result.offset.min(ctx.text.len());
                    if result.inclusivity.is_exclusive() {
                        // Default: exclusive charwise — same as before
                        MotionResult::Position(crate::primitives::Offset::new(clamped))
                    } else {
                        // Custom motion with explicit type info — bypass static inclusivity lookup
                        MotionResult::PositionWithType {
                            offset: crate::primitives::Offset::new(clamped),
                            inclusivity: result.inclusivity,
                        }
                    }
                } else {
                    MotionResult::NoMotion
                }
            } else {
                MotionResult::NoMotion
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Full motion dispatch with effects
// ─────────────────────────────────────────────────────────────────────────────

pub use crate::commands::motions::MotionEffectsContext;

/// Dispatch a motion and produce the full `CommandResult` with all side effects.
///
/// This is the enterprise-grade motion dispatch that owns the complete lifecycle:
/// 1. **Star search pre-effects** — for `*`/`#`, set search pattern + register
///    before motion dispatch (Neovim records pattern even on failed `*`)
/// 2. **Motion dispatch** — calls `dispatch_motion` for the raw position
/// 3. **Post-motion effects** — visual: `extend_selection`, normal: `move_cursor`
///    with cross-line jump detection for jump list
/// 4. **Star jump list** — `*`/`#` always push to jump list on success
pub fn dispatch_motion_with_effects(
    mut ctx: MotionEffectsContext<'_>,
) -> crate::commands::CommandResult {
    use crate::commands::actions::effects as action_effects;
    use crate::commands::visual::selection as vs;
    use crate::commands::CommandResult;
    use crate::effects::Effects;
    use crate::primitives::Offset;

    // Derive inclusive_end from mode — visual/insert/replace allow cursor at EOL
    let is_visual = ctx.mode.is_visual();
    ctx.motion_ctx.inclusive_end = is_visual || ctx.mode.is_insert() || ctx.mode.is_replace();

    let mut effects = Effects::new();

    // 1. Star search pre-effects (always emitted, even on motion failure)
    let is_star = matches!(
        ctx.motion,
        Motion::WordSearchForward
            | Motion::WordSearchBackward
            | Motion::PartialWordSearchForward
            | Motion::PartialWordSearchBackward
    );

    // visual_star_pattern: when visualstar is enabled and we're in Visual mode with
    // a selection, we extract the selected text and build a `\V`-prefixed pattern here.
    // This pattern is used below to override the motion dispatch so that `*`/`#` search
    // for the selected text rather than word-under-cursor.
    let mut visual_star_pattern: Option<String> = None;

    if is_star {
        // Derive search_direction from motion variant (domain knowledge lives here, not executor)
        let search_direction = match ctx.motion {
            Motion::WordSearchForward | Motion::PartialWordSearchForward => {
                crate::primitives::Direction::Forward
            }
            Motion::WordSearchBackward | Motion::PartialWordSearchBackward => {
                crate::primitives::Direction::Backward
            }
            _ => unreachable!("is_star guarantees word/partial search variant"),
        };

        let is_partial = matches!(
            ctx.motion,
            Motion::PartialWordSearchForward | Motion::PartialWordSearchBackward
        );
        let star_effects = if is_visual && ctx.motion_ctx.options.visualstar() {
            if let Some(ref sel) = ctx.selection {
                // Build the visual star pattern for the motion override below.
                let start = sel.start().get();
                let end = sel.end().get();
                let end_inclusive =
                    crate::commands::helpers::next_char_boundary(ctx.motion_ctx.text, end);
                let selected = ctx.motion_ctx.text.get(start..end_inclusive).unwrap_or("");
                if !selected.is_empty() {
                    visual_star_pattern = Some(format!("\\V{selected}"));
                }

                action_effects::star_search_visual(
                    ctx.motion_ctx.text,
                    start,
                    end,
                    search_direction,
                )
            } else {
                Effects::new()
            }
        } else if is_visual {
            // Visual mode but visualstar is off — fall through to normal visual
            // star behavior (sets pattern from selection, but motion uses word-under-cursor)
            if let Some(ref sel) = ctx.selection {
                action_effects::star_search_visual(
                    ctx.motion_ctx.text,
                    sel.start().get(),
                    sel.end().get(),
                    search_direction,
                )
            } else {
                Effects::new()
            }
        } else if is_partial {
            action_effects::partial_star_search_normal(
                ctx.motion_ctx.text,
                ctx.cursor_offset.get(),
                search_direction,
            )
        } else {
            action_effects::star_search_normal(
                ctx.motion_ctx.text,
                ctx.cursor_offset.get(),
                search_direction,
            )
        };
        effects.extend(star_effects);
    }

    // 2. Motion dispatch
    //
    // When visualstar is active, override the motion: instead of dispatching
    // `WordSearchForward`/`WordSearchBackward` (which extracts word-under-cursor),
    // use `SearchMotion::NextMatch`/`PrevMatch` with the pattern from the selection.
    // This makes `*`/`#` in Visual mode search for the selected text.
    //
    // The visual star pattern is an owned String that must outlive the temporary
    // borrow into `search_pattern: Option<&str>`. We scope the computation inside
    // a block so the borrow is confined and the borrow checker is satisfied.
    let result = match visual_star_pattern {
        Some(ref vs_pattern) => {
            let direction = match ctx.motion {
                Motion::WordSearchForward | Motion::PartialWordSearchForward => {
                    crate::primitives::Direction::Forward
                }
                _ => crate::primitives::Direction::Backward,
            };
            // Build a temporary MotionContext with the visual star pattern.
            // We clone the immutable fields and override search_pattern + search_direction.
            //
            // Cursor position for the search: for forward (*), use the end of the
            // selection so the search skips the current match and finds the NEXT
            // occurrence. For backward (#), use the start of the selection so it
            // finds the PREVIOUS occurrence. This mirrors Vim's behavior where */#
            // skip the word under cursor.
            let search_cursor = if let Some(ref sel) = ctx.selection {
                if direction.is_forward() {
                    // Forward: search from one past selection end to skip current match
                    let end = sel.end().get();
                    Offset::new(crate::commands::helpers::next_char_boundary(
                        ctx.motion_ctx.text,
                        end,
                    ))
                } else {
                    // Backward: search from selection start to skip current match
                    sel.start()
                }
            } else {
                ctx.motion_ctx.cursor
            };
            let mut vs_ctx = MotionContext::new(
                ctx.motion_ctx.text,
                search_cursor,
                ctx.motion_ctx.count,
                ctx.motion_ctx.options,
            );
            vs_ctx.search_pattern = Some(vs_pattern.as_str());
            vs_ctx.search_direction = direction;
            vs_ctx.inclusive_end = ctx.motion_ctx.inclusive_end;
            vs_ctx.providers = ctx.motion_ctx.providers;
            // Always use NextMatch — it follows search_direction directly.
            // PrevMatch would reverse it, producing the wrong direction.
            SearchMotion::NextMatch.compute(&vs_ctx)
        }
        None => dispatch_motion(ctx.motion, &ctx.motion_ctx),
    };

    match result {
        MotionResult::Position(new_offset) => {
            // 3. Post-motion effects: visual vs normal
            // Skip extend_selection for star/hash in Visual mode — star_search_visual
            // already emitted ClearSelection + SetMode(Normal).
            if let Some(ref sel) = ctx.selection.filter(|_| !(is_star && is_visual)) {
                let shape = ctx
                    .mode
                    .visual_type()
                    .map_or(SelectionShape::Char, SelectionShape::from);
                let sel_effects = vs::extend_selection(sel, new_offset, shape);
                effects.extend(sel_effects);
            } else {
                let is_cross_line = ctx.motion.is_jump_motion() && {
                    let text = ctx.motion_ctx.text;
                    crate::commands::helpers::line_of(text, ctx.cursor_offset.get())
                        != crate::commands::helpers::line_of(text, new_offset.get())
                };
                // Only emit SetCursor when the position actually changed.
                // In Neovim, a failed motion (e.g. `l` at end of line) does NOT
                // set w_set_curswant, so curswant retains its previous value.
                // Emitting SetCursor when position is unchanged would trigger
                // the auto-emit SetStickyColumn in effect_processor, incorrectly
                // resetting curswant.
                if new_offset != ctx.cursor_offset || ctx.motion.is_jump_motion() {
                    let move_effects =
                        vs::move_cursor(ctx.cursor_offset, new_offset, is_cross_line);
                    effects.extend(move_effects);
                }
            }

            // 4. Star jump list — * and # push is handled by move_cursor above
            // (is_jump_motion returns true for star motions), so no extra push needed.
            // Previously this pushed `new_offset` (the destination), creating a
            // double-push. In Neovim, * pushes only the pre-jump position.

            // 5. Search match count info (n/N/*/# → "Match N of M")
            if is_search_motion(ctx.motion) {
                if let Some(pattern) = ctx.motion_ctx.search_pattern {
                    // Try cache-hit path for n/N: if pattern and document
                    // haven't changed, advance `current` arithmetically
                    // instead of rescanning all matches.
                    let cached_result = if is_n_or_big_n(ctx.motion) {
                        try_cached_search_count(
                            &ctx.search_count_cache,
                            pattern,
                            ctx.motion_ctx.count,
                            resolve_search_direction(ctx.motion, ctx.motion_ctx.search_direction),
                        )
                    } else {
                        None
                    };

                    let info = cached_result.or_else(|| {
                        // Full scan fallback (with 50ms deadline on native).
                        #[cfg(not(target_arch = "wasm32"))]
                        let deadline =
                            Some(std::time::Instant::now() + std::time::Duration::from_millis(50));

                        search::count_search_matches(
                            ctx.motion_ctx.text,
                            pattern,
                            new_offset.get(),
                            #[cfg(not(target_arch = "wasm32"))]
                            deadline,
                        )
                    });

                    if let Some(info) = info {
                        effects.push(crate::effects::Effect::SearchMatchInfo {
                            current: info.current,
                            total: info.total,
                            complete: info.complete,
                        });
                    }
                }

                // Detect search wrapping: if the result is "behind" the cursor
                // relative to the search direction, the search wrapped around.
                //
                // Known false negatives (acceptable — heuristic without search internals):
                // - Same-position wrap: `*` on the only occurrence wraps the entire
                //   document and lands back at cursor (new_offset == cursor_offset).
                // - Multi-count wrap: `3n` where wrapping occurs mid-count but the
                //   final position is still ahead of cursor in the search direction.
                let search_dir =
                    resolve_search_direction(ctx.motion, ctx.motion_ctx.search_direction);
                let wrapped = match search_dir {
                    crate::primitives::Direction::Forward => new_offset < ctx.cursor_offset,
                    crate::primitives::Direction::Backward => new_offset > ctx.cursor_offset,
                };
                if wrapped {
                    effects.push(crate::effects::Effect::Event {
                        kind: crate::primitives::VimEvent::SearchWrapped {
                            direction: crate::primitives::SearchDirection::from(search_dir),
                        },
                    });
                }
            }

            // 6. Ctrl-D/Ctrl-U sticky count — persist explicit count for future scrolls
            if is_scroll_half_motion(ctx.motion) && ctx.motion_ctx.explicit_count {
                effects.push(crate::effects::Effect::SetScrollHalfCount {
                    count: ctx.motion_ctx.count,
                });
            }

            // 6b. Scroll viewport effects for scroll-class motions.
            // These motions compute a new topline internally but only return
            // cursor position. Emit ScrollTo so the host can track viewport.
            //
            // Ctrl-E/Y/F/B always scroll the viewport (even when doc fits).
            // Ctrl-D/U only scroll when the document is taller than viewport
            // (matching Neovim behavior where Ctrl-D on a small doc doesn't scroll).
            if let Some(viewport) = ctx.motion_ctx.viewport.filter(|vp| {
                let total = crate::commands::helpers::line_count(ctx.motion_ctx.text);
                match ctx.motion {
                    Motion::ScrollLineDown
                    | Motion::ScrollLineUp
                    | Motion::ScrollFullDown
                    | Motion::ScrollFullUp => true,
                    Motion::ScrollHalfDown | Motion::ScrollHalfUp => total > vp.height,
                    _ => false,
                }
            }) {
                let total = crate::commands::helpers::line_count(ctx.motion_ctx.text);
                let count = ctx.motion_ctx.count_usize();
                let half_amount = if ctx.motion_ctx.explicit_count {
                    count
                } else if let Some(sticky) = ctx.motion_ctx.scroll_half_count {
                    sticky as usize
                } else {
                    viewport.height / 2
                };
                let new_topline = match ctx.motion {
                    Motion::ScrollLineDown => {
                        (viewport.first_line + count).min(total.saturating_sub(1))
                    }
                    Motion::ScrollLineUp => viewport.first_line.saturating_sub(count),
                    Motion::ScrollFullDown => {
                        let page = viewport.height.saturating_sub(2).max(1);
                        (viewport.first_line + page).min(total.saturating_sub(1))
                    }
                    Motion::ScrollFullUp => {
                        let page = viewport.height.saturating_sub(2).max(1);
                        viewport.first_line.saturating_sub(page)
                    }
                    Motion::ScrollHalfDown => {
                        (viewport.first_line + half_amount).min(total.saturating_sub(1))
                    }
                    Motion::ScrollHalfUp => viewport.first_line.saturating_sub(half_amount),
                    _ => viewport.first_line,
                };
                if new_topline != viewport.first_line {
                    let topline_offset = nth_line_start(ctx.motion_ctx.text, new_topline);
                    effects.push(crate::effects::Effect::ScrollTo {
                        offset: Offset::new(topline_offset),
                    });
                }
            }

            // 7. Sticky column (curswant) — vertical motions preserve, horizontal update
            //    Only update on horizontal motions when the cursor actually moved.
            //    In Vim, a failed `l` at end-of-line does NOT reset curswant.
            //    Exception: `$`/`g$` ALWAYS set END_OF_LINE even when cursor is
            //    already at EOL (Neovim always sets curswant=MAXCOL for `$`).
            if !ctx.motion.is_vertical()
                && (new_offset != ctx.cursor_offset
                    || is_eol_motion(ctx.motion)
                    || ctx.motion == Motion::GoToColumn)
            {
                let column = if is_eol_motion(ctx.motion) {
                    Some(crate::primitives::VirtualColumn::END_OF_LINE)
                } else if ctx.motion == Motion::GoToColumn {
                    // `|` sets curswant to the requested column (count-1),
                    // even when clamped. This matches Neovim where `99|` on
                    // a 5-char line sets curswant=98 so j/k try column 98.
                    Some(crate::primitives::VirtualColumn::new(
                        (ctx.motion_ctx.count as usize).saturating_sub(1),
                    ))
                } else {
                    // Use virtual column (curswant) for sticky column, matching
                    // Neovim's coladvance behavior. This correctly handles tabs.
                    let tabstop = ctx.motion_ctx.options.tabstop();
                    Some(crate::primitives::VirtualColumn::new(
                        crate::commands::helpers::curswant_of(
                            ctx.motion_ctx.text,
                            new_offset.get(),
                            tabstop,
                        ),
                    ))
                };
                effects.push(crate::effects::Effect::SetStickyColumn { column });
            } else if ctx.motion.is_vertical() {
                // Vertical motions preserve curswant. If we already have a
                // sticky column, re-emit it so the auto-emit in
                // effect_processor doesn't overwrite it with the clamped
                // cursor position. If there's no prior sticky column, latch
                // the pre-motion column for subsequent j/k motions.
                let column = if let Some(existing) = ctx.motion_ctx.sticky_column {
                    Some(existing)
                } else {
                    let tabstop = ctx.motion_ctx.options.tabstop();
                    Some(crate::primitives::VirtualColumn::new(
                        crate::commands::helpers::curswant_of(
                            ctx.motion_ctx.text,
                            ctx.cursor_offset.get(),
                            tabstop,
                        ),
                    ))
                };
                effects.push(crate::effects::Effect::SetStickyColumn { column });
            }

            CommandResult::effects_only(effects)
        }
        MotionResult::Range { start, end } => {
            // gn/gN: text-object-like range result.
            // In visual mode: extend selection to cover the match.
            // In normal mode: enter visual mode and select the match.
            // Convert exclusive end to the gap before the last matched character.
            // Use prev_char_boundary (not saturating_sub(1)) for multi-byte safety.
            let text = ctx.motion_ctx.text;
            let last_char_gap = if end.get() > 0 {
                crate::commands::helpers::prev_char_boundary(text, end.get())
            } else {
                0
            };
            let last_byte_offset = Offset::new(last_char_gap.min(text.len()));
            let is_backward = matches!(ctx.motion, Motion::SearchObjectBackward);

            if let Some(ref sel) = ctx.selection {
                // Visual mode: extend selection to cover the match
                let head = if is_backward { start } else { last_byte_offset };
                let shape = ctx
                    .mode
                    .visual_type()
                    .map_or(SelectionShape::Char, SelectionShape::from);
                let sel_effects = vs::extend_selection(sel, head, shape);
                effects.extend(sel_effects);
            } else {
                // Normal mode: enter visual char mode and select the match.
                // gn: anchor=start, cursor=end (forward selection)
                // gN: anchor=end, cursor=start (backward selection)
                let (anchor, head) = if is_backward {
                    (last_byte_offset, start)
                } else {
                    (start, last_byte_offset)
                };
                effects.extend(
                    Effects::new()
                        .set_mode(crate::primitives::Mode::Visual(
                            crate::primitives::VisualType::Char,
                        ))
                        .set_visual_selection(anchor, head, SelectionShape::Char),
                );
            }

            // Neovim's current_search() does NOT update curswant.
            // Preserve the existing sticky column to prevent the auto-emit
            // in effect_processor from overwriting it with the cursor position.
            let column = if let Some(existing) = ctx.motion_ctx.sticky_column {
                Some(existing)
            } else {
                let tabstop = ctx.motion_ctx.options.tabstop();
                Some(crate::primitives::VirtualColumn::new(
                    crate::commands::helpers::curswant_of(text, ctx.cursor_offset.get(), tabstop),
                ))
            };
            effects.push(crate::effects::Effect::SetStickyColumn { column });

            CommandResult::effects_only(effects)
        }
        MotionResult::PositionWithType { offset, .. } => {
            // Custom motion with type info: for pure motion (non-operator) dispatch,
            // just move the cursor — inclusivity only matters for operator ranges.
            if let Some(ref sel) = ctx.selection {
                let shape = ctx
                    .mode
                    .visual_type()
                    .map_or(SelectionShape::Char, SelectionShape::from);
                effects.extend(vs::extend_selection(sel, offset, shape));
            } else {
                let is_cross_line = {
                    let text = ctx.motion_ctx.text;
                    crate::commands::helpers::line_of(text, ctx.cursor_offset.get())
                        != crate::commands::helpers::line_of(text, offset.get())
                };
                effects.extend(vs::move_cursor(ctx.cursor_offset, offset, is_cross_line));
            }
            CommandResult::effects_only(effects)
        }
        MotionResult::NeedsViewport | MotionResult::NoMotion => {
            // On failure, still return pre-effects (star search pattern)
            CommandResult::effects_only(effects)
        }
        MotionResult::Error => {
            // Emit specific error messages for search motions.
            // In Vim, failed motions produce error messages that are visible
            // to the user and abort macro replay.
            match ctx.motion {
                Motion::WordSearchForward | Motion::WordSearchBackward => {
                    effects.push(crate::effects::Effect::ShowError {
                        error: crate::errors::VimError::NoStringUnderCursor,
                        source: None,
                    });
                }
                Motion::SearchNext | Motion::SearchPrev => {
                    // n/N with no previous pattern
                    if ctx.motion_ctx.search_pattern.is_none()
                        || ctx.motion_ctx.search_pattern == Some("")
                    {
                        effects.push(crate::effects::Effect::ShowError {
                            error: crate::errors::VimError::NoPreviousPattern,
                            source: None,
                        });
                    } else {
                        let pat = ctx.motion_ctx.search_pattern.unwrap_or("");
                        effects.push(crate::effects::Effect::ShowError {
                            error: crate::errors::VimError::PatternNotFound(pat.into()),
                            source: None,
                        });
                    }
                }
                // Search object motions (gn/gN) report the search error
                Motion::SearchObjectForward | Motion::SearchObjectBackward => {
                    if ctx.motion_ctx.search_pattern.is_none()
                        || ctx.motion_ctx.search_pattern == Some("")
                    {
                        effects.push(crate::effects::Effect::ShowError {
                            error: crate::errors::VimError::NoPreviousPattern,
                            source: None,
                        });
                    } else {
                        let pat = ctx.motion_ctx.search_pattern.unwrap_or("");
                        effects.push(crate::effects::Effect::ShowError {
                            error: crate::errors::VimError::PatternNotFound(pat.into()),
                            source: None,
                        });
                    }
                }
                _ => {
                    // Generic motion failure: emit ShowError so macro replay aborts.
                    // In Vim, any failed motion beeps and aborts macro playback.
                    effects.push(crate::effects::Effect::ShowError {
                        error: crate::errors::VimError::MotionFailed,
                        source: None,
                    });
                }
            }
            CommandResult::effects_only(effects)
        }
    }
}

/// Check whether a motion is a half-page scroll (Ctrl-D/Ctrl-U).
#[must_use]
const fn is_scroll_half_motion(motion: Motion) -> bool {
    matches!(motion, Motion::ScrollHalfDown | Motion::ScrollHalfUp)
}

/// Check whether a motion sets sticky column to [`VirtualColumn::END_OF_LINE`].
///
/// In Vim, `$` and `g$` set `curswant` to `MAXCOL` so that vertical motions
/// after `$` stay at the end of each line.
///
/// `g$` (`ScreenLineEnd`) is NOT included: Neovim sets curswant to the
/// actual column for `g$`, not MAXCOL. Only `$` triggers the "end of line
/// forever" sticky behavior.
#[must_use]
const fn is_eol_motion(motion: Motion) -> bool {
    matches!(motion, Motion::LineEnd)
}

/// Check whether a motion is a search motion (n/N/*/#/g*/g#).
#[must_use]
const fn is_search_motion(motion: Motion) -> bool {
    matches!(
        motion,
        Motion::SearchNext
            | Motion::SearchPrev
            | Motion::WordSearchForward
            | Motion::WordSearchBackward
            | Motion::PartialWordSearchForward
            | Motion::PartialWordSearchBackward
    )
}

/// Check whether a motion is specifically `n` or `N` (eligible for cache hit).
///
/// `*`/`#`/`g*`/`g#` change the search pattern, so the cache hash won't
/// match and they always need a full scan.
#[must_use]
const fn is_n_or_big_n(motion: Motion) -> bool {
    matches!(motion, Motion::SearchNext | Motion::SearchPrev)
}

/// Attempt to use the cached search count instead of a full document scan.
///
/// Returns `Some(SearchCountResult)` if the cache is valid for the given
/// pattern and the count can be advanced arithmetically.
#[must_use]
fn try_cached_search_count(
    cache: &Option<crate::state::SearchCountCache>,
    pattern: &str,
    count: u32,
    effective_direction: crate::primitives::Direction,
) -> Option<search::SearchCountResult> {
    let cache = cache.as_ref()?;

    // Verify pattern hash matches.
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();
    pattern.hash(&mut hasher);
    if hasher.finish() != cache.pattern_hash {
        return None;
    }

    let forward = effective_direction == crate::primitives::Direction::Forward;
    let advanced = cache.try_advance(count, forward)?;
    Some(search::SearchCountResult {
        current: advanced.current,
        total: advanced.total,
        complete: advanced.complete,
    })
}

/// Resolve the effective search direction for a search motion.
///
/// `n` follows the current search direction, `N` reverses it.
/// `*`/`g*` are always forward, `#`/`g#` are always backward.
#[must_use]
const fn resolve_search_direction(
    motion: Motion,
    search_direction: crate::primitives::Direction,
) -> crate::primitives::Direction {
    match motion {
        Motion::WordSearchForward | Motion::PartialWordSearchForward => {
            crate::primitives::Direction::Forward
        }
        Motion::WordSearchBackward | Motion::PartialWordSearchBackward => {
            crate::primitives::Direction::Backward
        }
        Motion::SearchNext => search_direction,
        Motion::SearchPrev => search_direction.reverse(),
        _ => search_direction, // shouldn't be called for non-search motions
    }
}

fn nth_line_start(text: &str, line: usize) -> usize {
    if line == 0 {
        return 0;
    }
    let mut current = 0usize;
    for (i, &b) in text.as_bytes().iter().enumerate() {
        if b == b'\n' {
            current += 1;
            if current == line {
                return i + 1;
            }
        }
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Offset;

    // Compile-time test: ensure all Motion variants are handled
    // If a variant is added to Motion enum, this will fail to compile
    // until dispatch_motion is updated.
    #[test]
    fn exhaustive_match_compiles() {
        let opts = crate::primitives::VimOptions::default();
        // This test just needs to compile - it proves the match is exhaustive
        let ctx = MotionContext::new("test", Offset::new(0), 1, &opts);
        let _ = dispatch_motion(Motion::Left, &ctx);
    }

    #[test]
    fn resolve_search_direction_star_always_forward() {
        use crate::primitives::Direction;
        assert_eq!(
            resolve_search_direction(Motion::WordSearchForward, Direction::Backward),
            Direction::Forward
        );
        assert_eq!(
            resolve_search_direction(Motion::PartialWordSearchForward, Direction::Backward),
            Direction::Forward
        );
    }

    #[test]
    fn resolve_search_direction_hash_always_backward() {
        use crate::primitives::Direction;
        assert_eq!(
            resolve_search_direction(Motion::WordSearchBackward, Direction::Forward),
            Direction::Backward
        );
        assert_eq!(
            resolve_search_direction(Motion::PartialWordSearchBackward, Direction::Forward),
            Direction::Backward
        );
    }

    #[test]
    fn resolve_search_direction_n_follows_current() {
        use crate::primitives::Direction;
        assert_eq!(
            resolve_search_direction(Motion::SearchNext, Direction::Forward),
            Direction::Forward
        );
        assert_eq!(
            resolve_search_direction(Motion::SearchNext, Direction::Backward),
            Direction::Backward
        );
    }

    #[test]
    fn resolve_search_direction_big_n_reverses() {
        use crate::primitives::Direction;
        assert_eq!(
            resolve_search_direction(Motion::SearchPrev, Direction::Forward),
            Direction::Backward
        );
        assert_eq!(
            resolve_search_direction(Motion::SearchPrev, Direction::Backward),
            Direction::Forward
        );
    }

    #[test]
    fn search_wrapped_detection_forward() {
        // Forward search from offset 10, result at offset 5 → wrapped
        let opts = crate::primitives::VimOptions::default();
        let mut ctx = MotionEffectsContext {
            motion: Motion::SearchNext,
            cursor_offset: Offset::new(10),
            mode: crate::primitives::Mode::Normal,
            selection: None,
            motion_ctx: MotionContext::new("hello world test", Offset::new(10), 1, &opts),
            search_count_cache: None,
        };
        ctx.motion_ctx.search_pattern = Some("hello");
        ctx.motion_ctx.search_direction = crate::primitives::Direction::Forward;
        let result = dispatch_motion_with_effects(ctx);
        // If search found a match at offset 0 (< 10), SearchWrapped should be emitted
        let has_wrapped = result.effects.iter().any(|e| {
            matches!(
                e,
                crate::effects::Effect::Event {
                    kind: crate::primitives::VimEvent::SearchWrapped {
                        direction: crate::primitives::SearchDirection::Forward,
                    }
                }
            )
        });
        // "hello" is at offset 0, searching forward from 10 wraps back
        assert!(
            has_wrapped,
            "Forward search wrapping to earlier position should emit SearchWrapped"
        );
    }

    #[test]
    fn search_not_wrapped_forward() {
        // Forward search from offset 0, result at offset 6 → no wrap
        let opts = crate::primitives::VimOptions::default();
        let mut ctx = MotionEffectsContext {
            motion: Motion::SearchNext,
            cursor_offset: Offset::new(0),
            mode: crate::primitives::Mode::Normal,
            selection: None,
            motion_ctx: MotionContext::new("hello world hello", Offset::new(0), 1, &opts),
            search_count_cache: None,
        };
        ctx.motion_ctx.search_pattern = Some("hello");
        ctx.motion_ctx.search_direction = crate::primitives::Direction::Forward;
        let result = dispatch_motion_with_effects(ctx);
        let has_wrapped = result.effects.iter().any(|e| {
            matches!(
                e,
                crate::effects::Effect::Event {
                    kind: crate::primitives::VimEvent::SearchWrapped { .. }
                }
            )
        });
        // "hello" at offset 12, forward from 0 → no wrap
        assert!(
            !has_wrapped,
            "Forward search finding later match should not emit SearchWrapped"
        );
    }

    #[test]
    fn search_wrapped_backward() {
        // Backward search from offset 0, result should wrap to end
        let opts = crate::primitives::VimOptions::default();
        let mut ctx = MotionEffectsContext {
            motion: Motion::SearchNext,
            cursor_offset: Offset::new(0),
            mode: crate::primitives::Mode::Normal,
            selection: None,
            motion_ctx: MotionContext::new("hello world hello", Offset::new(0), 1, &opts),
            search_count_cache: None,
        };
        ctx.motion_ctx.search_pattern = Some("hello");
        ctx.motion_ctx.search_direction = crate::primitives::Direction::Backward;
        let result = dispatch_motion_with_effects(ctx);
        let has_wrapped = result.effects.iter().any(|e| {
            matches!(
                e,
                crate::effects::Effect::Event {
                    kind: crate::primitives::VimEvent::SearchWrapped {
                        direction: crate::primitives::SearchDirection::Backward,
                    }
                }
            )
        });
        // Backward from 0, "hello" at offset 12 (wrapped to end) → wrapped
        assert!(
            has_wrapped,
            "Backward search wrapping to later position should emit SearchWrapped"
        );
    }

    // ═══════════════════════════════════════════════════════════════════
    // Visual star (visualstar option) tests
    // ═══════════════════════════════════════════════════════════════════

    /// Helper: run a star/hash motion in visual mode with the given options and selection.
    ///
    /// Returns the `CommandResult` from `dispatch_motion_with_effects`.
    fn run_visual_star(
        text: &str,
        cursor: usize,
        sel_anchor: usize,
        sel_head: usize,
        motion: Motion,
        opts: &crate::primitives::VimOptions,
    ) -> crate::commands::CommandResult {
        let sel =
            crate::primitives::SelectionRange::new(Offset::new(sel_anchor), Offset::new(sel_head));
        let ctx = MotionEffectsContext {
            motion,
            cursor_offset: Offset::new(cursor),
            mode: crate::primitives::Mode::Visual(crate::primitives::VisualType::Char),
            selection: Some(sel),
            motion_ctx: MotionContext::new(text, Offset::new(cursor), 1, opts),
            search_count_cache: None,
        };
        dispatch_motion_with_effects(ctx)
    }

    /// Extract the search pattern string from a `SetSearchPattern` effect.
    fn extract_search_pattern(result: &crate::commands::CommandResult) -> Option<String> {
        result.effects.iter().find_map(|e| {
            if let crate::effects::Effect::SetSearchPattern { pattern, .. } = e {
                Some(pattern.to_string())
            } else {
                None
            }
        })
    }

    /// Extract the cursor destination from a `SetCursor` effect.
    fn extract_cursor_move(result: &crate::commands::CommandResult) -> Option<usize> {
        result.effects.iter().find_map(|e| match e {
            crate::effects::Effect::SetCursor { offset, .. } => Some(offset.get()),
            _ => None,
        })
    }

    #[test]
    fn star_normal_mode_still_searches_word_under_cursor() {
        // Normal mode * should search for word-under-cursor regardless of visualstar
        let text = "hello world hello again";
        let mut opts = crate::primitives::VimOptions::default();
        opts.set_visualstar(true);
        let ctx = MotionEffectsContext {
            motion: Motion::WordSearchForward,
            cursor_offset: Offset::new(0), // on "hello"
            mode: crate::primitives::Mode::Normal,
            selection: None,
            motion_ctx: MotionContext::new(text, Offset::new(0), 1, &opts),
            search_count_cache: None,
        };
        let result = dispatch_motion_with_effects(ctx);
        // Pattern should be \<hello\> (word boundary)
        let pattern = extract_search_pattern(&result);
        assert!(
            pattern.is_some(),
            "Normal mode * should set a search pattern"
        );
        let pat = pattern.unwrap();
        assert!(
            pat.contains("hello"),
            "Pattern should contain 'hello', got: {pat}"
        );
        assert!(
            pat.contains("\\<") && pat.contains("\\>"),
            "Normal * uses word boundaries, got: {pat}"
        );
    }

    #[test]
    fn visual_star_with_visualstar_enabled_searches_selected_text() {
        // "hello world hello again"
        //  01234567890123456789012
        // Visual selection: "world" (offsets 6..10)
        // With visualstar=true, * should search for `\Vworld`
        let text = "hello world hello again";
        let mut opts = crate::primitives::VimOptions::default();
        opts.set_visualstar(true);
        let result = run_visual_star(text, 10, 6, 10, Motion::WordSearchForward, &opts);
        let pattern = extract_search_pattern(&result);
        assert!(
            pattern.is_some(),
            "Visual * with visualstar should set search pattern"
        );
        let pat = pattern.unwrap();
        assert_eq!(
            pat, "\\Vworld",
            "Visual * with visualstar should use \\V prefix with selected text"
        );
    }

    #[test]
    fn visual_star_with_visualstar_disabled_still_uses_word_under_cursor_pattern() {
        // When visualstar=false (default), visual * should still set a pattern
        // from the selection (star_search_visual does this) but the MOTION
        // dispatches to word-under-cursor. The pattern is still \V{selected}.
        let text = "hello world hello again";
        let mut opts = crate::primitives::VimOptions::default();
        opts.set_visualstar(false);
        let result = run_visual_star(text, 10, 6, 10, Motion::WordSearchForward, &opts);
        let pattern = extract_search_pattern(&result);
        assert!(
            pattern.is_some(),
            "Visual * without visualstar still emits pattern from selection"
        );
        let pat = pattern.unwrap();
        // star_search_visual sets \V{selected} pattern regardless of visualstar
        assert_eq!(
            pat, "\\Vworld",
            "Pattern from star_search_visual should be \\Vworld"
        );
    }

    #[test]
    fn visual_star_with_visualstar_finds_next_occurrence() {
        // "foo bar foo baz foo"
        //  0123456789012345678
        // Select "foo" at [0..2], cursor at 2, * forward should find "foo" at 8
        let text = "foo bar foo baz foo";
        let mut opts = crate::primitives::VimOptions::default();
        opts.set_visualstar(true);
        let result = run_visual_star(text, 2, 0, 2, Motion::WordSearchForward, &opts);
        let cursor_pos = extract_cursor_move(&result);
        assert_eq!(
            cursor_pos,
            Some(8),
            "Visual * should jump to next occurrence of selected text 'foo' at offset 8"
        );
    }

    #[test]
    fn visual_hash_with_visualstar_searches_backward() {
        // "foo bar foo baz foo"
        //  0123456789012345678
        // Select "foo" at [8..10], cursor at 10, # backward should find "foo" at 0
        let text = "foo bar foo baz foo";
        let mut opts = crate::primitives::VimOptions::default();
        opts.set_visualstar(true);
        let result = run_visual_star(text, 10, 8, 10, Motion::WordSearchBackward, &opts);
        let cursor_pos = extract_cursor_move(&result);
        assert_eq!(
            cursor_pos,
            Some(0),
            "Visual # should jump backward to 'foo' at offset 0"
        );
    }

    #[test]
    fn visual_star_cross_word_boundary_selection() {
        // Select text that spans word boundaries: "foo bar" (offset 4..10)
        // This is NOT a single word — with visualstar, * should search for
        // the literal "foo bar" string, not just a word-under-cursor.
        //
        // "bar foo bar foo bar xyz foo bar"
        //  0         1         2         3
        //  0123456789012345678901234567890
        //
        // "foo bar" appears at offsets 4, 12, 24
        let text = "bar foo bar foo bar xyz foo bar";
        let mut opts = crate::primitives::VimOptions::default();
        opts.set_visualstar(true);
        // Select "foo bar" starting at offset 4, ending at offset 10
        // text[4..11] = "foo bar" (next_char_boundary(10) = 11)
        let result = run_visual_star(text, 10, 4, 10, Motion::WordSearchForward, &opts);
        let pattern = extract_search_pattern(&result);
        assert!(
            pattern.is_some(),
            "Cross-word selection should still set pattern"
        );
        let pat = pattern.unwrap();
        assert_eq!(
            pat, "\\Vfoo bar",
            "Cross-word selection should produce literal pattern \\Vfoo bar"
        );
        // The motion should find the next "foo bar" occurrence after cursor position 10
        let cursor_pos = extract_cursor_move(&result);
        // "foo bar" at offsets 4, 12, 24. Searching forward from 10 → offset 12.
        assert_eq!(
            cursor_pos,
            Some(12),
            "Cross-word visual * should find next 'foo bar' at offset 12"
        );
    }

    #[test]
    fn visual_star_exits_visual_mode() {
        // visual * should produce a SetMode(Normal) effect (via star_search_visual)
        let text = "hello world hello";
        let mut opts = crate::primitives::VimOptions::default();
        opts.set_visualstar(true);
        let result = run_visual_star(text, 4, 0, 4, Motion::WordSearchForward, &opts);
        let sets_normal = result.effects.iter().any(|e| {
            matches!(
                e,
                crate::effects::Effect::SetMode {
                    mode: crate::primitives::Mode::Normal,
                    ..
                }
            )
        });
        assert!(sets_normal, "Visual * should exit visual mode (set Normal)");
    }

    #[test]
    fn n_after_visual_star_uses_correct_pattern() {
        // After visual * sets pattern `\Vworld`, pressing `n` should search
        // for `\Vworld` (not word-under-cursor). This test verifies that the
        // pattern emitted by visual star is compatible with SearchMotion::NextMatch.
        let text = "hello world hello world again";
        let mut opts = crate::primitives::VimOptions::default();
        opts.set_visualstar(true);

        // First: do visual * to set the pattern
        let star_result = run_visual_star(text, 10, 6, 10, Motion::WordSearchForward, &opts);
        let pattern = extract_search_pattern(&star_result).expect("visual * should set pattern");
        assert_eq!(pattern, "\\Vworld");

        // Now simulate `n` (SearchNext) with that pattern set in the context
        let mut n_ctx = MotionEffectsContext {
            motion: Motion::SearchNext,
            cursor_offset: Offset::new(6), // After visual *, cursor is at first match
            mode: crate::primitives::Mode::Normal,
            selection: None,
            motion_ctx: MotionContext::new(text, Offset::new(6), 1, &opts),
            search_count_cache: None,
        };
        n_ctx.motion_ctx.search_pattern = Some(&pattern);
        n_ctx.motion_ctx.search_direction = crate::primitives::Direction::Forward;
        let n_result = dispatch_motion_with_effects(n_ctx);
        let n_cursor = extract_cursor_move(&n_result);
        // "world" at offset 6 and 18. From 6 forward, next is 18.
        assert_eq!(
            n_cursor,
            Some(18),
            "n after visual * should find next 'world' at offset 18"
        );
    }

    #[test]
    fn visualstar_default_is_false() {
        let opts = crate::primitives::VimOptions::default();
        assert!(!opts.visualstar(), "visualstar should default to false");
    }
}
