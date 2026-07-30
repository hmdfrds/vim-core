//! Multi-cursor yank and paste-zipping post-processing.
//!
//! After `replicate_effects_precise` produces effects for all cursors and
//! `process_effects` writes single-entry register content, this module
//! enriches the register with per-cursor text entries.
//!
//! For paste-zipping, after replication produces identical Insert text for
//! all cursors, this module replaces each cursor's Insert text with the
//! corresponding register entry.
//!
//! # Architecture
//!
//! These are pure helper functions called from `mode_dispatch.rs`. They
//! access `VimState` (register read/write) and the original document text,
//! but don't modify the effect processing pipeline.

use crate::effects::Effect;
use crate::grammar::Command;
use crate::primitives::{MotionType, RegisterContent, RegisterName, Selections, VimOptions};
use crate::state::VimState;
use compact_str::CompactString;
use smallvec::SmallVec;

// ═══════════════════════════════════════════════════════════════════════════
// RANGE SOURCE — per-cursor range recomputation
// ═══════════════════════════════════════════════════════════════════════════

/// Describes how to recompute the yanked/deleted range at each cursor.
///
/// Captured from the `Command` before execution consumes it. This allows
/// `override_registers_with_multi_cursor_entries` to compute per-cursor ranges
/// instead of assuming all cursors produce the same range length.
#[derive(Clone, Copy, Debug)]
pub(super) enum RangeSource {
    /// Operator applied to a text object (e.g., `yiw`, `da(`).
    TextObject {
        textobject: crate::grammar::types::TextObject,
        count: u32,
    },
    /// Operator applied to a motion (e.g., `yw`, `d$`, `y}`).
    Motion {
        motion: crate::grammar::types::Motion,
        count: u32,
    },
    /// Operator applied to a char-find motion (e.g., `df)`, `dt,`, `dF(`).
    CharFind {
        command: crate::grammar::types::CharCommand,
        target: char,
        count: u32,
    },
    /// Linewise operator (e.g., `dd`, `yy`).
    Linewise { count: u32 },
}

/// Extract a `RangeSource` from a command before execution consumes it.
///
/// Returns `Some` for operator+textobject and linewise operator commands,
/// enabling per-cursor range recomputation in the multi-cursor register override.
pub(super) fn extract_range_source(command: &Command) -> Option<RangeSource> {
    match command {
        Command::OperatorTextObject {
            textobject, count, ..
        } => Some(RangeSource::TextObject {
            textobject: *textobject,
            count: count.get(),
        }),
        Command::OperatorMotion { motion, count, .. } => Some(RangeSource::Motion {
            motion: *motion,
            count: count.get(),
        }),
        Command::CharCommand {
            operator: Some(_),
            command,
            ref target,
            count,
            ..
        } => {
            use crate::grammar::types::CharCommand as CC;
            match command {
                CC::FindForward | CC::FindBackward | CC::TillForward | CC::TillBackward => {
                    Some(RangeSource::CharFind {
                        command: *command,
                        target: target.chars().next().unwrap_or('\0'),
                        count: count.get(),
                    })
                }
                _ => None,
            }
        }
        Command::OperatorLine { count, .. } => Some(RangeSource::Linewise { count: count.get() }),
        _ => None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// YANK OVERRIDE
// ═══════════════════════════════════════════════════════════════════════════

/// Information extracted from primary effects about a register write.
///
/// Used by multi-cursor post-processing to compute per-cursor text entries.
pub(super) struct PrimaryRegisterInfo {
    /// Register name(s) that were written.
    names: SmallVec<[RegisterName; 3]>,
    /// Motion type for the register content.
    motion_type: MotionType,
    /// The byte range in the original document that produced the primary text.
    /// For delete: from the Delete effect's range.
    /// For yank: from CHANGE_START mark to CHANGE_START + text.len().
    primary_range_start: usize,
    primary_range_end: usize,
}

/// Scan primary effects (before replication) for SetRegister information.
///
/// Returns register info needed to compute per-cursor text entries.
/// Returns None if no SetRegister effects are found.
pub(super) fn extract_primary_register_info(effects: &[Effect]) -> Option<PrimaryRegisterInfo> {
    use crate::primitives::MarkName;

    let mut names: SmallVec<[RegisterName; 3]> = SmallVec::new();
    let mut motion_type = None;
    let mut delete_range: Option<(usize, usize)> = None;
    let mut change_start: Option<usize> = None;
    let mut register_text_len: Option<usize> = None;

    for effect in effects {
        match effect {
            Effect::SetRegister {
                name,
                text,
                motion_type: mt,
            } => {
                if !names.contains(name) {
                    names.push(*name);
                }
                motion_type = Some(*mt);
                if register_text_len.is_none() {
                    register_text_len = Some(text.len());
                }
            }
            Effect::Delete { range } => {
                delete_range = Some((range.start().get(), range.end().get()));
            }
            Effect::SetMark { name, offset, .. } if *name == MarkName::CHANGE_START => {
                change_start = Some(offset.get());
            }
            _ => {}
        }
    }

    let motion_type = motion_type?;
    if names.is_empty() {
        return None;
    }

    // Determine the primary text range in the original document.
    let (primary_range_start, primary_range_end) = if let Some(range) = delete_range {
        // Delete/change: use the Delete effect's exact range.
        range
    } else if let Some(start) = change_start {
        // Pure yank: range is [CHANGE_START, CHANGE_START + text_len).
        let text_len = register_text_len.unwrap_or(0);
        (start, start + text_len)
    } else {
        // Can't determine range — safe fallback (no override).
        return None;
    };

    Some(PrimaryRegisterInfo {
        names,
        motion_type,
        primary_range_start,
        primary_range_end,
    })
}

/// Override registers with multi-entry content after process_effects.
///
/// For each cursor position, recomputes the text range using the captured
/// `RangeSource` and extracts per-cursor text from the original document.
/// This handles cursors on words/objects of different lengths correctly.
///
/// When `range_source` is `None`, falls back to the fixed-length heuristic
/// (safe degradation for commands we don't capture).
pub(super) fn override_registers_with_multi_cursor_entries(
    state: &mut VimState,
    doc_text: &str,
    info: &PrimaryRegisterInfo,
    range_source: Option<RangeSource>,
    options: &VimOptions,
) {
    let selections = state.multi_cursor().selections();
    if selections.len() <= 1 {
        return;
    }

    let primary_offset = selections.primary().head().get();
    let range_len = info
        .primary_range_end
        .saturating_sub(info.primary_range_start);
    if range_len == 0 {
        return;
    }

    // Build per-cursor text entries.
    let mut entries: SmallVec<[CompactString; 1]> = SmallVec::with_capacity(selections.len());

    for sel in selections.iter() {
        let cursor_offset = sel.head().get();

        // Try per-cursor range recomputation via RangeSource.
        let per_cursor_range = range_source
            .and_then(|src| compute_range_at_cursor(doc_text, cursor_offset, src, options));

        let text = if let Some((start, end)) = per_cursor_range {
            // Per-cursor recomputed range.
            extract_text_at_range(doc_text, start, end, info.motion_type)
        } else {
            // Fallback: fixed-length heuristic (original behavior).
            let start = if cursor_offset >= primary_offset {
                info.primary_range_start + (cursor_offset - primary_offset)
            } else {
                info.primary_range_start
                    .saturating_sub(primary_offset - cursor_offset)
            };
            let end = start + range_len;
            extract_text_at_range(doc_text, start, end, info.motion_type)
        };
        entries.push(text);
    }

    if entries.is_empty() {
        return;
    }

    let multi_content = RegisterContent::from_entries(entries, info.motion_type);

    // Override all registers that were written with the multi-entry version.
    for &name in &info.names {
        state.registers_mut().set(name, multi_content.clone());
    }
}

/// Compute the text range at a given cursor using the captured `RangeSource`.
///
/// Returns `Some((start, end))` byte range in the document, or `None` if
/// the computation fails (e.g., no valid text object at cursor).
fn compute_range_at_cursor(
    text: &str,
    cursor_offset: usize,
    source: RangeSource,
    options: &VimOptions,
) -> Option<(usize, usize)> {
    match source {
        RangeSource::TextObject { textobject, count } => {
            use crate::dispatch::{dispatch_textobject_with_count, TextObjectContext};

            let ctx = TextObjectContext::new(text, cursor_offset).with_options(options);
            let result = dispatch_textobject_with_count(textobject, &ctx, count)?;
            Some((result.range.start().get(), result.range.end().get()))
        }
        RangeSource::Motion { motion, count } => {
            use crate::commands::operators::range::compute_motion_range;
            use crate::dispatch::dispatch_motion;

            let result = compute_motion_range(
                text,
                cursor_offset,
                motion,
                count,
                None, // search — not needed for pure motions (w, $, }, etc.)
                None, // last_find — not needed for most motions
                options,
                dispatch_motion,
                None, // viewport — only H/M/L need this
            )?;
            Some((result.range.start().get(), result.range.end().get()))
        }
        RangeSource::CharFind {
            command,
            target,
            count,
        } => {
            use crate::commands::motions::find::{compute_find_backward, compute_find_forward};
            use crate::commands::motions::types::{MotionContext, MotionResult};
            use crate::grammar::types::CharCommand as CC;
            use crate::primitives::Offset;

            let ctx = MotionContext::new(text, Offset::new(cursor_offset), count, options);
            let target_pos = match command {
                CC::FindForward | CC::TillForward => match compute_find_forward(&ctx, target) {
                    MotionResult::Position(off) => {
                        let pos = off.get();
                        if matches!(command, CC::TillForward) && pos > 0 {
                            crate::commands::helpers::prev_char_boundary(text, pos)
                        } else {
                            pos
                        }
                    }
                    _ => return None,
                },
                CC::FindBackward | CC::TillBackward => match compute_find_backward(&ctx, target) {
                    MotionResult::Position(off) => {
                        let pos = off.get();
                        if matches!(command, CC::TillBackward) {
                            crate::commands::helpers::next_char_boundary(text, pos)
                        } else {
                            pos
                        }
                    }
                    _ => return None,
                },
                _ => return None,
            };

            // Operator range: [cursor, target] inclusive for forward, [target, cursor] for backward.
            let (start, end) = if target_pos >= cursor_offset {
                (
                    cursor_offset,
                    target_pos + text[target_pos..].chars().next().map_or(0, char::len_utf8),
                )
            } else {
                (target_pos, cursor_offset)
            };
            Some((start, end))
        }
        RangeSource::Linewise { count } => {
            use crate::commands::helpers::{line_end_for_offset, line_start_for_offset};

            let start = line_start_for_offset(text, cursor_offset);
            // For count > 1, extend downward by (count - 1) lines.
            let mut end = line_end_for_offset(text, cursor_offset);
            for _ in 1..count {
                if end < text.len() {
                    // Skip past the newline to the next line.
                    end = line_end_for_offset(text, end + 1);
                }
            }
            // Include the trailing newline if present (linewise always does).
            if end < text.len() && text.as_bytes()[end] == b'\n' {
                end += 1;
            }
            Some((start, end))
        }
    }
}

/// Extract text from the document at the given byte range, applying
/// linewise normalization if needed.
fn extract_text_at_range(
    doc_text: &str,
    start: usize,
    end: usize,
    motion_type: MotionType,
) -> CompactString {
    if end <= doc_text.len() && start <= end {
        let raw = &doc_text[start..end];
        if motion_type == MotionType::LineWise {
            let mut s = CompactString::from(raw);
            if !s.ends_with('\n') {
                s.push('\n');
            }
            s
        } else {
            CompactString::from(raw)
        }
    } else {
        // Range out of bounds — use empty string (safe fallback).
        if motion_type == MotionType::LineWise {
            CompactString::from("\n")
        } else {
            CompactString::new("")
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// PASTE ZIPPING
// ═══════════════════════════════════════════════════════════════════════════

/// Content needed for paste-zipping: per-cursor text entries from register.
pub(super) struct PasteZipContent {
    /// The primary cursor's paste text (used to identify matching Inserts).
    primary_text: CompactString,
    /// Per-entry text from the multi-entry register, in selection iteration order.
    entries: SmallVec<[CompactString; 1]>,
    /// Paste count (e.g., 3 for `3p`). When > 1, the Insert text is
    /// `primary_text.repeat(count)` and each entry must be repeated too.
    count: u32,
}

/// Detect if the command is a paste and the register has multi-entry content.
///
/// Returns the paste-zip content if both conditions are met:
/// 1. The command is a put/paste action
/// 2. The register being pasted from has multiple entries (from multi-cursor yank)
pub(super) fn detect_paste_zip_content(
    command: &Command,
    state: &VimState,
    resolved_options: &VimOptions,
) -> Option<PasteZipContent> {
    use crate::grammar::types::Action;

    // Check if this is a paste action.
    let (action, register, count) = match command {
        Command::Action {
            action,
            register,
            count,
            ..
        } => (*action, *register, count.get()),
        _ => return None,
    };

    let is_paste = matches!(
        action,
        Action::Put
            | Action::PutBefore
            | Action::PutAfterCursorAfter
            | Action::PutBeforeCursorAfter
            | Action::PutIndentAfter
            | Action::PutIndentBefore
    );
    if !is_paste {
        return None;
    }

    // Read register content (with clipboard aliasing).
    let content_reg = register.unwrap_or(RegisterName::UNNAMED);
    let content = state
        .registers()
        .get_aliased(content_reg, resolved_options)?;

    // Only zip if multi-entry.
    if content.entry_count() <= 1 {
        return None;
    }

    let primary_text = CompactString::from(content.text());
    let entries: SmallVec<[CompactString; 1]> = content.entries().iter().cloned().collect();

    Some(PasteZipContent {
        primary_text,
        entries,
        count,
    })
}

/// Apply paste-zipping to replicated effects.
///
/// After replication, each cursor group has the same Insert text (the primary's).
/// This function replaces the Insert text in each group with the corresponding
/// register entry, enabling per-cursor paste distribution.
///
/// The replicated effects are structured as:
/// `[BeginUndoGroup, <group_N>, <group_N-1>, ..., <group_0>, EndUndoGroup]`
/// where groups are in DESCENDING offset order.
pub(super) fn apply_paste_zip(
    effects: &crate::effects::Effects,
    selections: &Selections,
    zip: &PasteZipContent,
) -> crate::effects::Effects {
    let all = effects.as_slice();
    let num_cursors = selections.len();

    // Collect (offset, entry_index) pairs, then sort descending by offset.
    // This matches the replication group order (descending).
    let mut cursor_order: SmallVec<[(usize, usize); 8]> = selections
        .iter()
        .enumerate()
        .map(|(idx, sel)| (sel.head().get(), idx))
        .collect();
    cursor_order.sort_by_key(|&(offset, _)| std::cmp::Reverse(offset));

    // Find the primary effects group size.
    // Structure: BeginUndoGroup + N * group_size + EndUndoGroup.
    let total_effects = all.len();
    if total_effects < 2 + num_cursors {
        return effects.clone();
    }
    let inner_count = total_effects - 2;
    let group_size = inner_count / num_cursors;
    if group_size == 0 || !inner_count.is_multiple_of(num_cursors) {
        return effects.clone();
    }

    let mut result: Vec<Effect> = Vec::with_capacity(total_effects);
    // total_effects >= 2 + num_cursors >= 3, so first/last are guaranteed.
    let Some(first) = all.first() else {
        return effects.clone();
    };
    result.push(first.clone()); // BeginUndoGroup

    // When count > 1, the Insert text is primary_text repeated count times.
    // Build the match text ONCE (loop-invariant).
    let match_text = if zip.count > 1 {
        CompactString::from(zip.primary_text.repeat(zip.count as usize))
    } else {
        zip.primary_text.clone()
    };

    for (group_idx, &(_cursor_offset, entry_idx)) in cursor_order.iter().enumerate() {
        let group_start = 1 + group_idx * group_size;
        let group_end = group_start + group_size;

        // Bounds check: group_end <= total_effects - 1 (before EndUndoGroup)
        let group_slice = all.get(group_start..group_end).unwrap_or_default();
        for effect in group_slice {
            match effect {
                Effect::Insert { offset, text } if *text == match_text => {
                    // Replace with per-cursor entry text (repeated by count).
                    let clamped_idx = entry_idx.min(zip.entries.len().saturating_sub(1));
                    let entry_text = &zip.entries[clamped_idx];
                    let replacement = if zip.count > 1 {
                        CompactString::from(entry_text.repeat(zip.count as usize))
                    } else {
                        entry_text.clone()
                    };
                    result.push(Effect::Insert {
                        offset: *offset,
                        text: replacement,
                    });
                }
                other => result.push(other.clone()),
            }
        }
    }

    if let Some(last) = all.last() {
        result.push(last.clone()); // EndUndoGroup
    }
    result.into_iter().collect()
}

// ═══════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effects;
    use crate::primitives::{MarkName, Offset, Range, SelectionRange, UndoCursorStrategy};

    #[test]
    fn extract_primary_register_info_yank() {
        let effects = Effects::new()
            .set_register(RegisterName::UNNAMED, "hello", MotionType::CharWise)
            .set_register(RegisterName::LAST_YANK, "hello", MotionType::CharWise)
            .set_mark(MarkName::CHANGE_START, Offset::new(5), None);

        let info = extract_primary_register_info(effects.as_slice()).unwrap();

        assert_eq!(info.names.len(), 2);
        assert!(info.names.contains(&RegisterName::UNNAMED));
        assert!(info.names.contains(&RegisterName::LAST_YANK));
        assert_eq!(info.motion_type, MotionType::CharWise);
        assert_eq!(info.primary_range_start, 5);
        assert_eq!(info.primary_range_end, 10); // 5 + len("hello")
    }

    #[test]
    fn extract_primary_register_info_delete() {
        let effects_vec: Vec<Effect> = vec![
            Effect::Delete {
                range: Range::from_raw(3, 8),
            },
            Effect::set_register(RegisterName::UNNAMED, "world", MotionType::CharWise),
            Effect::set_register(RegisterName::SMALL_DELETE, "world", MotionType::CharWise),
        ];
        let effects: Effects = effects_vec.into_iter().collect();

        let info = extract_primary_register_info(effects.as_slice()).unwrap();

        assert_eq!(info.primary_range_start, 3);
        assert_eq!(info.primary_range_end, 8);
        assert_eq!(info.motion_type, MotionType::CharWise);
    }

    #[test]
    fn extract_primary_register_info_no_register() {
        let effects = Effects::new().set_cursor(Offset::new(5));

        let info = extract_primary_register_info(effects.as_slice());
        assert!(info.is_none());
    }

    #[test]
    fn override_registers_three_cursors_charwise() {
        let mut state = VimState::default();

        let sels = Selections::new(
            smallvec::smallvec![
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(4)),
                SelectionRange::insert_cursor(Offset::new(8)),
            ],
            0,
        );
        state.multi_cursor_mut().set_selections(sels);

        state.registers_mut().set(
            RegisterName::UNNAMED,
            RegisterContent::new("aaa", MotionType::CharWise),
        );
        state.registers_mut().set(
            RegisterName::LAST_YANK,
            RegisterContent::new("aaa", MotionType::CharWise),
        );

        let doc = "aaa bbb ccc";
        let info = PrimaryRegisterInfo {
            names: smallvec::smallvec![RegisterName::UNNAMED, RegisterName::LAST_YANK],
            motion_type: MotionType::CharWise,
            primary_range_start: 0,
            primary_range_end: 3,
        };

        let options = VimOptions::default();
        override_registers_with_multi_cursor_entries(&mut state, doc, &info, None, &options);

        let content = state.registers().get(RegisterName::UNNAMED).unwrap();
        assert_eq!(content.entry_count(), 3);
        assert_eq!(content.entry(0), "aaa");
        assert_eq!(content.entry(1), "bbb");
        assert_eq!(content.entry(2), "ccc");

        let yank = state.registers().get(RegisterName::LAST_YANK).unwrap();
        assert_eq!(yank.entry_count(), 3);
        assert_eq!(yank.entry(0), "aaa");
        assert_eq!(yank.entry(1), "bbb");
        assert_eq!(yank.entry(2), "ccc");
    }

    #[test]
    fn override_registers_linewise() {
        let mut state = VimState::default();

        let sels = Selections::new(
            smallvec::smallvec![
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(6)),
            ],
            0,
        );
        state.multi_cursor_mut().set_selections(sels);

        state.registers_mut().set(
            RegisterName::UNNAMED,
            RegisterContent::new("line1\n", MotionType::LineWise),
        );

        let doc = "line1\nline2\nline3\n";
        let info = PrimaryRegisterInfo {
            names: smallvec::smallvec![RegisterName::UNNAMED],
            motion_type: MotionType::LineWise,
            primary_range_start: 0,
            primary_range_end: 6,
        };

        let options = VimOptions::default();
        override_registers_with_multi_cursor_entries(&mut state, doc, &info, None, &options);

        let content = state.registers().get(RegisterName::UNNAMED).unwrap();
        assert_eq!(content.entry_count(), 2);
        assert_eq!(content.entry(0), "line1\n");
        assert_eq!(content.entry(1), "line2\n");
        assert_eq!(content.motion_type(), MotionType::LineWise);
    }

    #[test]
    fn paste_zip_replaces_insert_text_per_cursor() {
        let sels = Selections::new(
            smallvec::smallvec![
                SelectionRange::insert_cursor(Offset::new(5)),
                SelectionRange::insert_cursor(Offset::new(15)),
                SelectionRange::insert_cursor(Offset::new(25)),
            ],
            0,
        );

        let effects: Effects = vec![
            Effect::BeginUndoGroup {
                cursor_strategy: UndoCursorStrategy::FirstEdit,
            },
            // Cursor at 25 (entry index 2)
            Effect::Insert {
                offset: Offset::new(26),
                text: CompactString::from("AAA"),
            },
            Effect::SetCursor {
                offset: Offset::new(28),
            },
            // Cursor at 15 (entry index 1)
            Effect::Insert {
                offset: Offset::new(16),
                text: CompactString::from("AAA"),
            },
            Effect::SetCursor {
                offset: Offset::new(18),
            },
            // Cursor at 5 (entry index 0, primary)
            Effect::Insert {
                offset: Offset::new(6),
                text: CompactString::from("AAA"),
            },
            Effect::SetCursor {
                offset: Offset::new(8),
            },
            Effect::EndUndoGroup { node_id: None },
        ]
        .into_iter()
        .collect();

        let zip = PasteZipContent {
            primary_text: CompactString::from("AAA"),
            entries: smallvec::smallvec![
                CompactString::from("AAA"),
                CompactString::from("BBB"),
                CompactString::from("CCC"),
            ],
            count: 1,
        };

        let result = apply_paste_zip(&effects, &sels, &zip);
        let slice = result.as_slice();

        // Group 0 (cursor at 25, entry index 2): should have "CCC"
        assert_eq!(
            slice[1],
            Effect::Insert {
                offset: Offset::new(26),
                text: CompactString::from("CCC"),
            }
        );

        // Group 1 (cursor at 15, entry index 1): should have "BBB"
        assert_eq!(
            slice[3],
            Effect::Insert {
                offset: Offset::new(16),
                text: CompactString::from("BBB"),
            }
        );

        // Group 2 (cursor at 5, entry index 0): should have "AAA"
        assert_eq!(
            slice[5],
            Effect::Insert {
                offset: Offset::new(6),
                text: CompactString::from("AAA"),
            }
        );
    }

    #[test]
    fn detect_paste_non_paste_returns_none() {
        let state = VimState::default();
        let options = VimOptions::default();
        let cmd = Command::InsertExit;
        assert!(detect_paste_zip_content(&cmd, &state, &options).is_none());
    }

    #[test]
    fn detect_paste_recognizes_put_action() {
        use crate::grammar::types::Action;
        use std::num::NonZeroU32;

        let mut state = VimState::default();
        let options = VimOptions::default();

        let entries: SmallVec<[CompactString; 1]> =
            smallvec::smallvec![CompactString::from("foo"), CompactString::from("bar"),];
        state.registers_mut().set(
            RegisterName::UNNAMED,
            RegisterContent::from_entries(entries, MotionType::CharWise),
        );

        let cmd = Command::Action {
            count: NonZeroU32::new(1).unwrap(),
            register: None,
            action: Action::Put,
        };

        let result = detect_paste_zip_content(&cmd, &state, &options);
        assert!(result.is_some());

        let zip = result.unwrap();
        assert_eq!(zip.primary_text.as_str(), "foo");
        assert_eq!(zip.entries.len(), 2);
        assert_eq!(zip.entries[0].as_str(), "foo");
        assert_eq!(zip.entries[1].as_str(), "bar");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Per-cursor range recomputation tests
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn multi_cursor_yank_different_length_words() {
        // Cursors on "hi" (2 bytes), "world" (5 bytes), "ok" (2 bytes).
        // With RangeSource::TextObject(iw), each cursor should independently
        // compute its word boundary, producing ["hi", "world", "ok"].
        // Without per-cursor recomputation, the fixed-length bug would produce
        // ["hi", "wo", "ok"] (all 2 bytes, matching primary "hi").
        use crate::grammar::types::{TextObject, TextObjectKind, TextObjectScope};

        let mut state = VimState::default();

        // Document: "hi world ok"
        //            01 2345678 9..
        // "hi" starts at 0, "world" at 3, "ok" at 9
        let doc = "hi world ok";

        let sels = Selections::new(
            smallvec::smallvec![
                SelectionRange::insert_cursor(Offset::new(0)), // on "hi"
                SelectionRange::insert_cursor(Offset::new(3)), // on "world"
                SelectionRange::insert_cursor(Offset::new(9)), // on "ok"
            ],
            0,
        );
        state.multi_cursor_mut().set_selections(sels);

        // Primary cursor is at offset 0 ("hi"), range [0, 2).
        state.registers_mut().set(
            RegisterName::UNNAMED,
            RegisterContent::new("hi", MotionType::CharWise),
        );

        let info = PrimaryRegisterInfo {
            names: smallvec::smallvec![RegisterName::UNNAMED],
            motion_type: MotionType::CharWise,
            primary_range_start: 0,
            primary_range_end: 2,
        };

        let range_source = Some(RangeSource::TextObject {
            textobject: TextObject {
                scope: TextObjectScope::Inner,
                kind: TextObjectKind::Word,
                seek: None,
            },
            count: 1,
        });

        let options = VimOptions::default();
        override_registers_with_multi_cursor_entries(
            &mut state,
            doc,
            &info,
            range_source,
            &options,
        );

        let content = state.registers().get(RegisterName::UNNAMED).unwrap();
        assert_eq!(content.entry_count(), 3);
        assert_eq!(content.entry(0), "hi");
        assert_eq!(content.entry(1), "world");
        assert_eq!(content.entry(2), "ok");
    }

    #[test]
    fn multi_cursor_yank_different_length_words_fallback() {
        // Same scenario but with range_source = None (fallback path).
        // All cursors use fixed range_len from primary (2 bytes).
        // This verifies the fallback still works as before.
        let mut state = VimState::default();

        let doc = "hi world ok";

        let sels = Selections::new(
            smallvec::smallvec![
                SelectionRange::insert_cursor(Offset::new(0)), // on "hi"
                SelectionRange::insert_cursor(Offset::new(3)), // on "world"
                SelectionRange::insert_cursor(Offset::new(9)), // on "ok"
            ],
            0,
        );
        state.multi_cursor_mut().set_selections(sels);

        state.registers_mut().set(
            RegisterName::UNNAMED,
            RegisterContent::new("hi", MotionType::CharWise),
        );

        let info = PrimaryRegisterInfo {
            names: smallvec::smallvec![RegisterName::UNNAMED],
            motion_type: MotionType::CharWise,
            primary_range_start: 0,
            primary_range_end: 2,
        };

        let options = VimOptions::default();
        // No range_source — falls back to fixed-length.
        override_registers_with_multi_cursor_entries(&mut state, doc, &info, None, &options);

        let content = state.registers().get(RegisterName::UNNAMED).unwrap();
        assert_eq!(content.entry_count(), 3);
        assert_eq!(content.entry(0), "hi");
        // Fallback uses fixed length (2 bytes from primary), so "wo" not "world".
        assert_eq!(content.entry(1), "wo");
        assert_eq!(content.entry(2), "ok");
    }

    #[test]
    fn multi_cursor_linewise_recomputation() {
        // Test linewise range recomputation with lines of different lengths.
        // Cursors on "short\n" (6 bytes) and "much longer line\n" (17 bytes).
        let mut state = VimState::default();

        let doc = "short\nmuch longer line\nend\n";
        // "short\n" = offsets [0..6)
        // "much longer line\n" = offsets [6..23)
        // "end\n" = offsets [23..27)

        let sels = Selections::new(
            smallvec::smallvec![
                SelectionRange::insert_cursor(Offset::new(0)), // on "short"
                SelectionRange::insert_cursor(Offset::new(6)), // on "much longer line"
            ],
            0,
        );
        state.multi_cursor_mut().set_selections(sels);

        state.registers_mut().set(
            RegisterName::UNNAMED,
            RegisterContent::new("short\n", MotionType::LineWise),
        );

        let info = PrimaryRegisterInfo {
            names: smallvec::smallvec![RegisterName::UNNAMED],
            motion_type: MotionType::LineWise,
            primary_range_start: 0,
            primary_range_end: 6,
        };

        let range_source = Some(RangeSource::Linewise { count: 1 });

        let options = VimOptions::default();
        override_registers_with_multi_cursor_entries(
            &mut state,
            doc,
            &info,
            range_source,
            &options,
        );

        let content = state.registers().get(RegisterName::UNNAMED).unwrap();
        assert_eq!(content.entry_count(), 2);
        assert_eq!(content.entry(0), "short\n");
        assert_eq!(content.entry(1), "much longer line\n");
        assert_eq!(content.motion_type(), MotionType::LineWise);
    }
}
