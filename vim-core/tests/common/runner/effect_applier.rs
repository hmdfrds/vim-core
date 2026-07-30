//! Effect applier.
//!
//! Applies VimEngine effects to a TestDocument.

use crate::common::document::TestDocument;
use vim_core::effects::Effect;

/// Shift numbered registers 1→2→...→9, placing new content in "1".
///
/// Per Vim spec: When a linewise delete happens, numbered registers shift down
/// so register 9 is lost, 8→9, 7→8, ..., 1→2, and new content goes to "1".
fn shift_numbered_registers(doc: &mut TestDocument, new_text: &str, regtype: &str) {
    // Shift from 8 to 9, 7 to 8, ..., 1 to 2
    for i in (1..9).rev() {
        let from = char::from_digit(i, 10).unwrap();
        let to = char::from_digit(i + 1, 10).unwrap();
        if let Some((text, rt)) = doc.get_register(from) {
            doc.set_register(to, text.clone(), rt.clone());
        }
    }
    // New content goes to register "1"
    doc.set_register('1', new_text.to_string(), regtype.to_string());
}

/// Apply an effect to the test document.
///
/// Maps VimEngine effects to TestDocument mutations.
pub fn apply_effect(doc: &mut TestDocument, effect: Effect) {
    match effect {
        Effect::SetCursor { offset } => {
            doc.set_cursor_offset(offset.get());
        }
        Effect::Insert { offset, text } => {
            doc.insert(offset.get(), &text);
            // Set change marks: [=start, ]=exclusive end, .=last char position
            let last_char = offset.get() + text.len().saturating_sub(1);
            let exclusive_end = offset.get() + text.len();
            doc.set_mark('[', offset.get());
            doc.set_mark(']', exclusive_end);
            doc.set_mark('.', last_char);
        }
        Effect::Delete { range } => {
            doc.delete(range.start().get()..range.end().get());
            // Set change marks at deletion point
            let pos = range.start().get();
            doc.set_mark('[', pos);
            doc.set_mark(']', pos);
            doc.set_mark('.', pos);
        }
        Effect::Replace { range, text } => {
            doc.delete(range.start().get()..range.end().get());
            doc.insert(range.start().get(), &text);
            // Set change marks for replaced region
            let last_char = range.start().get() + text.len().saturating_sub(1);
            let exclusive_end = range.start().get() + text.len();
            doc.set_mark('[', range.start().get());
            doc.set_mark(']', exclusive_end);
            doc.set_mark('.', last_char);
        }
        Effect::SetMode { .. } => {
            // Mode is tracked in VimEngine, no doc change needed
        }
        Effect::SetSelection {
            anchor,
            head,
            shape: _,
        } => {
            doc.set_selection(anchor.get(), head.get());
        }
        Effect::ClearSelection => {
            // Marks '<' and '>' are now set by the core engine before ClearSelection
            doc.clear_selection();
        }
        Effect::SetRegister {
            name,
            text,
            motion_type,
        } => {
            // Convert MotionType to Neovim regtype: 'v' = charwise, 'V' = linewise
            let regtype = match motion_type {
                vim_core::primitives::MotionType::LineWise => "V".to_string(),
                vim_core::primitives::MotionType::CharWise => "v".to_string(),
                vim_core::primitives::MotionType::BlockWise => {
                    // Neovim's blockwise regtype is Ctrl-V followed by the block
                    // width (e.g., "\x164" for width 4). Compute width as the
                    // longest line in the register text, expanding tabs to tabstop.
                    let ts = 4usize; // matches oracle tabstop setting
                    let max_width = text
                        .lines()
                        .map(|line| {
                            line.chars()
                                .map(|c| if c == '\t' { ts } else { 1 })
                                .sum::<usize>()
                        })
                        .max()
                        .unwrap_or(0);
                    if max_width > 0 {
                        format!("\x16{max_width}")
                    } else {
                        "\x16".to_string()
                    }
                }
            };

            // For linewise deletes to unnamed register, implement Vim's shifting:
            // Shift 1→2→...→9, then put new content in "1"
            if name == vim_core::primitives::RegisterName::UNNAMED
                && matches!(motion_type, vim_core::primitives::MotionType::LineWise)
            {
                // This is a linewise delete - shift numbered registers
                shift_numbered_registers(doc, &text, &regtype);
            }

            doc.set_register(name.char(), text.to_string(), regtype);
        }
        // Undo group effects
        Effect::BeginUndoGroup { cursor_strategy } => {
            doc.begin_undo_group(matches!(
                cursor_strategy,
                vim_core::primitives::UndoCursorStrategy::EntryPosition
            ));
        }
        Effect::EndUndoGroup { .. } => {
            doc.end_undo_group();
        }
        Effect::Undo { count, .. } => {
            doc.undo(count);
        }
        Effect::Redo { count, .. } => {
            doc.redo(count);
        }
        Effect::UndoLine { .. } => {
            // U command: undo all changes on the current line.
            // In Neovim's test harness, the buffer starts empty and text is set
            // via nvim_buf_set_lines (which creates an undo entry), so U undoes
            // everything back to the empty buffer. We simulate this by undoing
            // until nothing more can be undone.
            doc.undo_line();
        }
        // Jump list effects
        Effect::PushJumpList { offset } => {
            doc.push_jump_list(offset.get());
            doc.set_mark('\'', offset.get());
            doc.set_mark('`', offset.get());
        }
        Effect::JumpOlder { count } => {
            // The engine already emitted SetCursor to move the cursor.
            // JumpOlder just syncs the test document's jump list pointer
            // so it stays consistent for any future state queries.
            for _ in 0..count {
                if doc.jump_older().is_none() {
                    break;
                }
            }
        }
        Effect::JumpNewer { count } => {
            // The engine already emitted SetCursor to move the cursor.
            // JumpNewer just syncs the test document's jump list pointer.
            for _ in 0..count {
                if doc.jump_newer().is_none() {
                    break;
                }
            }
        }
        // Mark effects
        Effect::SetMark { name, offset, .. } => {
            doc.set_mark(name.char(), offset.get());
        }

        Effect::OperatorToMark {
            operator,
            mark,
            linewise,
            register,
            cursor,
        } => {
            super::operator_to_mark::apply_operator_to_mark(
                doc, None, operator, mark, linewise, register, cursor,
            );
        }
        // Error messages - capture for fidelity testing
        Effect::ShowError { error, .. } => {
            doc.set_errmsg(error.to_string());
        }

        // =====================================================================
        // Intentionally ignored effects — no document mutation needed in the
        // test harness. Every variant is listed explicitly so the compiler
        // forces an update when new Effect variants are added.
        // =====================================================================

        // --- Mode / insert entry (tracked by VimEngine, not the document) ---
        Effect::SaveLastVisual { .. } => {
            // Visual selection geometry saved by engine for dot repeat
        }
        Effect::CommandLineEdit(_) => {
            // Command-line buffer edits — not part of the document
        }
        Effect::BeginInsert { .. } => {
            // Insert-mode entry metadata — tracked by the engine
        }
        Effect::SetBlockInsert { .. } => {
            // Block visual insert context — tracked by the engine
        }

        // --- Changelist navigation (cursor already set via SetCursor) ---
        Effect::ChangelistOlder { .. } => {
            // Engine already emitted SetCursor; changelist pointer is engine-internal
        }
        Effect::ChangelistNewer { .. } => {
            // Engine already emitted SetCursor; changelist pointer is engine-internal
        }

        // --- Cross-buffer jump (single-buffer test harness) ---
        Effect::JumpToBuffer { .. } => {
            // Multi-buffer navigation — not supported in test harness
        }

        // --- Search state (no document mutation) ---
        Effect::SetSearchPattern { .. } => {
            // Search pattern stored by engine, no document change
        }
        Effect::SetLastSubstitute { .. } => {
            // Last substitute replacement string — engine-internal
        }
        Effect::SetLastSubstituteFlags { .. } => {
            // Last substitute flags — engine-internal
        }
        Effect::SetSubstitutePattern { .. } => {
            // Substitute pattern (RE_SUBST) — engine-internal
        }
        Effect::HighlightMatches { .. } => {
            // Visual search highlights — UI-only
        }
        Effect::ClearHighlights => {
            // Clear search highlights — UI-only
        }
        Effect::SetLastFind { .. } => {
            // Last f/F/t/T find state — engine-internal
        }

        // --- Compound effects (orchestrator-level, not raw document ops) ---
        Effect::NormCommand { .. } => {
            // :norm keystroke replay — handled by orchestrator, not document
        }
        Effect::OperatorFilter { .. } => {
            // Host-driven filter (!{motion}) — requires external command
        }
        Effect::OperatorReindent { .. } => {
            // Host-driven reindent (={motion}) — requires host formatter
        }

        // --- UI messages (ShowError handled above; these are informational) ---
        Effect::Bell => {
            // Audible/visual bell — no document change
        }
        Effect::ShowInfo { .. } => {
            // Informational status messages — no document change
        }
        Effect::ClearMessage => {
            // Clear message area — UI-only
        }

        // --- Scrolling / viewport (no document mutation) ---
        Effect::ScrollTo { .. } => {
            // Scroll to bring offset into view — viewport-only
        }
        Effect::CenterCursor => {
            // zz — viewport-only
        }
        Effect::CursorToTop => {
            // zt — viewport-only
        }
        Effect::CursorToBottom => {
            // zb — viewport-only
        }
        Effect::ScrollLeft { .. } => {
            // zh — viewport-only
        }
        Effect::ScrollRight { .. } => {
            // zl — viewport-only
        }

        // --- Macro recording/playback (engine-internal) ---
        Effect::StartRecording { .. } => {
            // Begin macro recording — engine-internal
        }
        Effect::StopRecording => {
            // End macro recording — engine-internal
        }
        Effect::PlayMacro { .. } => {
            // Macro replay — handled by engine's macro coordinator
        }

        // --- Clipboard (external side-effect, not document) ---
        Effect::CopyToClipboard { .. } => {
            // System clipboard — external side-effect
        }

        // --- Search info display ---
        Effect::SearchMatchInfo { .. } => {
            // "Match N of M" display — UI-only
        }

        // --- Scroll / sticky column state ---
        Effect::SetScrollHalfCount { .. } => {
            // Ctrl-D/U sticky count — engine-internal
        }
        Effect::SetStickyColumn { .. } => {
            // Vertical motion sticky column (curswant) — engine-internal
        }

        // --- Fold effects (no document mutation) ---
        Effect::FoldLine { .. } => {
            // zc — fold UI
        }
        Effect::UnfoldLine { .. } => {
            // zo — fold UI
        }
        Effect::ToggleFold { .. } => {
            // za — fold UI
        }
        Effect::ToggleFoldRecursive { .. } => {
            // zA — fold UI
        }
        Effect::FoldAll => {
            // zM — fold UI
        }
        Effect::UnfoldAll => {
            // zR — fold UI
        }
        Effect::FoldLineRecursive { .. } => {
            // zC — fold UI
        }
        Effect::UnfoldLineRecursive { .. } => {
            // zO — fold UI
        }
        Effect::DeleteFold { .. } => {
            // zd — fold UI
        }
        Effect::DeleteFoldRecursive { .. } => {
            // zD — fold UI
        }
        Effect::EliminateAllFolds => {
            // zE — fold UI
        }
        Effect::ToggleFoldEnable => {
            // zi — fold UI
        }
        Effect::SetFoldEnable { .. } => {
            // zn/zN — fold UI
        }
        Effect::SyncFoldRanges { .. } => {
            // Syntax fold range sync — fold UI
        }

        // --- Window effects (single-window test harness) ---
        Effect::WindowSplit => {}
        Effect::WindowNew => {}
        Effect::WindowVSplit => {}
        Effect::WindowClose => {}
        Effect::WindowOnly => {}
        Effect::WindowNext => {}
        Effect::WindowPrev => {}
        Effect::WindowMoveLeft => {}
        Effect::WindowMoveRight => {}
        Effect::WindowMoveUp => {}
        Effect::WindowMoveDown => {}
        Effect::WindowEqualSize => {}
        Effect::WindowIncreaseHeight { .. } => {}
        Effect::WindowDecreaseHeight { .. } => {}
        Effect::WindowIncreaseWidth { .. } => {}
        Effect::WindowDecreaseWidth { .. } => {}
        Effect::WindowRotateDown => {}
        Effect::WindowRotateUp => {
            // Window management — not supported in single-window test harness
        }

        // --- Command window ---
        Effect::OpenCommandWindow { .. } => {
            // Command-line history window — not supported in test harness
        }

        // --- Extension / host action ---
        Effect::CallOperatorFunc { .. } => {
            // g@ operator — requires host-registered operatorfunc
        }
        Effect::HostAction { .. } => {
            // <Action>(name) — requires host action registry
        }

        // --- LSP navigation (requires host LSP provider) ---
        Effect::GotoDefinition => {
            // gd — requires host LSP
        }
        Effect::ShowDocumentation => {
            // K — requires host documentation provider
        }

        // --- Plugin visual feedback ---
        Effect::HighlightRows { .. } => {
        }
        Effect::SetHighlightRange { .. } => {
        }
        Effect::ClearHighlightRange { .. } => {
        }
        Effect::SetExtState { .. } => {
        }
        Effect::ClearExtState { .. } => {
        }

        // --- Substitute preview (inccommand) ---
        Effect::SubstitutePreview { .. } => {
            // Live substitute preview — UI-only
        }
        Effect::ClearSubstitutePreview => {
            // Clear substitute preview — UI-only
        }

        // --- Substitute confirm (:s///c) ---
        Effect::SetSubstituteConfirmState { .. } => {
            // Engine-internal: confirm state machine — consumed by effect processor
        }
        Effect::ClearSubstituteConfirmState => {
            // Engine-internal: clear confirm state — consumed by effect processor
        }
        Effect::SubstituteConfirmShow { .. } => {
            // Confirm prompt UI — no document change
        }
        Effect::SubstituteConfirmEnd => {
            // Confirm session ended — no document change
        }

        // --- Syntax selection (engine-internal state) ---
        Effect::SyntaxSelectionPush { .. } => {
            // Engine-internal: syntax selection stack push
        }
        Effect::SyntaxSelectionPop => {
            // Engine-internal: syntax selection stack pop
        }
        Effect::SyntaxHistoryClear => {
            // Engine-internal: syntax selection stack clear
        }
        Effect::SetSyntaxSelections { .. } => {
            // Engine-internal: syntax selection cursor update
        }

        // --- Virtual text / decorations (UI-only) ---
        Effect::SetVirtualText { .. } => {
            // Virtual text — UI-only decoration
        }
        Effect::ClearVirtualText { .. } => {
            // Clear virtual text — UI-only
        }
        Effect::SetDiagnostics { .. } => {
            // Diagnostics — UI-only decoration
        }

        // --- Undo tree visualization ---
        Effect::UndoTreeSnapshot { .. } => {
            // :undotree visualization — UI-only
        }

        // --- Event notification ---
        Effect::Event { .. } => {
            // Typed Vim event — host notification, no document change
        }

        // --- Cursor style ---
        Effect::SetCursorStyle { .. } => {
            // Cursor shape/blink for current mode — UI-only
        }
        Effect::CursorShapeHint { .. } => {
            // Operator-pending cursor hint — UI-only
        }

        // --- No-op ---
        Effect::Noop => {
            // Placeholder — no operation
        }

        // --- Register / mark clearing ---
        Effect::ClearNamedRegister { .. } => {
            // Clear a named register — engine-internal
        }
        Effect::ClearMark { .. } => {
            // Delete a named mark — engine-internal
        }

        // Effect is #[non_exhaustive] — keep a wildcard arm so the code
        // compiles, but crash loudly so new variants are immediately noticed.
        _ => unreachable!(
            "unhandled Effect variant in test harness: {:?}  -- add an explicit arm to effect_applier.rs",
            effect
        ),
    }
}
