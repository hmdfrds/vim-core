//! Select-mode execution helpers for [`VimEngine`].
//!
//! Handles select-mode replace (type-to-replace) and operator (delete/change)
//! by routing through the visual operator-selection pipeline.

use super::VimEngine;
use crate::document::Document;
use crate::effects::Effect;
use crate::execution::response::Response;
use crate::execution::{InputContext, Validated};
use crate::grammar::Command;
use crate::primitives::Mode;

impl VimEngine {
    /// Execute a select mode action by routing through the operator-selection pipeline.
    ///
    /// Select mode actions reuse the visual operator infrastructure:
    /// - SelectReplace → `OperatorSelection { operator: Change }` (delete + Insert mode)
    /// - SelectDelete → `OperatorSelection { operator: Delete }` (delete + Normal mode)
    ///
    /// The mode must be temporarily set to Visual so the operator-selection pipeline
    /// can resolve the selection range correctly (it checks `mode.is_visual()`).
    pub(in crate::execution::engine) fn execute_select_operator<D: Document>(
        &mut self,
        command: Command,
        ctx: InputContext<'_, D, Validated>,
    ) -> Response {
        // The operator-selection pipeline requires Visual mode to resolve the selection.
        // Select mode has the same selection, so temporarily switch.
        let original_mode = self.state.mode();
        if let Mode::Select(vt) = original_mode {
            self.state.set_mode(Mode::Visual(vt));
        }

        // Reset parser for operator-selection (same as process_pipeline_result does)
        self.parser.reset();

        match self.execute_effect_plan(command, false, ctx) {
            Ok(response) => response,
            Err(ref err) => self.handle_pipeline_error(err),
        }
    }

    /// Select mode type-to-replace: delete selection, enter Insert, inject typed char.
    ///
    /// Routes through the Change operator to delete the selection, then injects
    /// the typed character inside the undo group so the entire operation
    /// (deletion + first char) is one atomic undo step.
    pub(in crate::execution::engine) fn execute_select_replace<D: Document>(
        &mut self,
        ch: char,
        ctx: InputContext<'_, D, Validated>,
    ) -> Response {
        let mut response = self.execute_select_operator(
            Command::OperatorSelection {
                operator: crate::grammar::types::Operator::Change,
                register: None,
            },
            ctx,
        );
        // Inject the typed char at EVERY cursor's SetCursor position, not just
        // the last one. Collect all SetCursor offsets (descending order), then
        // inject Insert+SetCursor pairs for each.
        let char_text = compact_str::CompactString::from(&*ch.encode_utf8(&mut [0u8; 4]));
        let char_len = ch.len_utf8();

        let cursor_offsets: Vec<crate::primitives::Offset> = response
            .effects
            .iter()
            .filter_map(|e| {
                if let Effect::SetCursor { offset } = e {
                    Some(*offset)
                } else {
                    None
                }
            })
            .collect();

        if cursor_offsets.is_empty() {
            return response;
        }

        // Build per-cursor Insert+SetCursor pairs.
        let mut injected: Vec<Effect> = Vec::with_capacity(cursor_offsets.len() * 2);
        for offset in &cursor_offsets {
            injected.push(Effect::Insert {
                offset: *offset,
                text: char_text.clone(),
            });
            injected.push(Effect::SetCursor {
                offset: offset.saturating_add_raw(char_len),
            });
        }

        // Inject before EndUndoGroup for correct undo grouping.
        if let Some(end_idx) = response
            .effects
            .iter()
            .rposition(|e| matches!(e, Effect::EndUndoGroup { .. }))
        {
            for (i, effect) in injected.into_iter().enumerate() {
                response.effects.insert(end_idx + i, effect);
            }
        } else {
            response.effects.extend(injected);
        }
        super::insert::track_select_replace_char(&mut self.state, ch);
        response
    }
}
