//! Interactive `:s///c` confirm key handler.
//!
//! When `SubstituteConfirmState` is active on `VimState`, this module
//! intercepts keystrokes before mode dispatch and handles the y/n/a/q/l
//! confirm protocol.

use crate::effects::Effects;
use crate::keymap::{Key, KeyEvent};
use crate::primitives::byte_delta;

use super::Response;

impl super::VimEngine {
    /// Check if a substitute confirm session is active and handle the key.
    ///
    /// Returns `Some(Response)` if the key was consumed by the confirm handler,
    /// or `None` if no confirm session is active (caller should proceed with
    /// normal mode dispatch).
    pub(in crate::execution::engine) fn try_handle_substitute_confirm(
        &mut self,
        key: KeyEvent,
    ) -> Option<Response> {
        if self.state.substitute_confirm().is_none() {
            return None;
        }

        let effects = match key.key() {
            Key::Char('y') => self.confirm_accept_current(),
            Key::Char('n') => self.confirm_skip_current(),
            Key::Char('a') => self.confirm_accept_all(),
            Key::Char('q') | Key::Escape => self.confirm_quit(),
            Key::Char('l') => self.confirm_accept_last(),
            // Unknown keys in confirm mode are ignored (consumed but no action).
            _ => Effects::new(),
        };

        let mut response = Response::with_effects(effects);

        // Run effects through the effect processor so SetSubstituteConfirmState /
        // ClearSubstituteConfirmState are consumed and applied to VimState.
        let proc_result = super::super::effect_processor::process_effects(
            &mut self.state,
            &mut self.parser,
            false,
            &mut response,
        );
        if proc_result.ended_repeat {
            self.is_repeating = false;
        }

        Some(response)
    }

    /// `y` — accept the current match, show next (or end).
    fn confirm_accept_current(&mut self) -> Effects {
        let confirm = self.state.substitute_confirm_mut().unwrap();

        let result = confirm.accept();
        let mut effects = Effects::new();

        if let Some((range, replacement)) = result {
            effects = effects.replace(range, replacement.as_str());
        }

        effects = self.confirm_advance_or_end(effects);
        effects
    }

    /// `n` — skip the current match, show next (or end).
    fn confirm_skip_current(&mut self) -> Effects {
        let confirm = self.state.substitute_confirm_mut().unwrap();
        confirm.skip();
        self.confirm_advance_or_end(Effects::new())
    }

    /// `a` — accept all remaining matches and end.
    fn confirm_accept_all(&mut self) -> Effects {
        let confirm = self.state.substitute_confirm_mut().unwrap();
        let replacements = confirm.accept_all_remaining();
        let accepted = confirm.accepted_count();
        let lines_changed = confirm.lines_changed();

        let mut effects = Effects::new();
        for (range, replacement) in replacements {
            effects = effects.replace(range, replacement.as_str());
        }

        effects = effects
            .clear_substitute_confirm_state()
            .substitute_confirm_end();

        effects = emit_confirm_summary(effects, accepted, lines_changed);
        effects
    }

    /// `q` / `Esc` — quit, keep whatever was already substituted.
    fn confirm_quit(&self) -> Effects {
        let confirm = self.state.substitute_confirm().unwrap();
        let accepted = confirm.accepted_count();
        let lines_changed = confirm.lines_changed();

        let mut effects = Effects::new()
            .clear_substitute_confirm_state()
            .substitute_confirm_end();

        effects = emit_confirm_summary(effects, accepted, lines_changed);
        effects
    }

    /// `l` — accept current match, then quit (last).
    fn confirm_accept_last(&mut self) -> Effects {
        let confirm = self.state.substitute_confirm_mut().unwrap();

        let result = confirm.accept();
        let mut effects = Effects::new();

        if let Some((range, replacement)) = result {
            effects = effects.replace(range, replacement.as_str());
        }

        let accepted = confirm.accepted_count();
        let lines_changed = confirm.lines_changed();

        effects = effects
            .clear_substitute_confirm_state()
            .substitute_confirm_end();

        effects = emit_confirm_summary(effects, accepted, lines_changed);
        effects
    }

    /// After accept/skip: if there are more matches, show the next one;
    /// otherwise end the session.
    fn confirm_advance_or_end(&self, mut effects: Effects) -> Effects {
        let confirm = self.state.substitute_confirm().unwrap();

        if confirm.is_done() {
            let accepted = confirm.accepted_count();
            let lines_changed = confirm.lines_changed();

            effects = effects
                .clear_substitute_confirm_state()
                .substitute_confirm_end();

            effects = emit_confirm_summary(effects, accepted, lines_changed);
        } else {
            let adjusted_range = confirm.current_adjusted_range().unwrap();
            let replacement = confirm.replacement().to_owned();
            let match_index = byte_delta::to_u32(confirm.current_index() + 1);
            let total = byte_delta::to_u32(confirm.total_matches());

            // Update the stored state so the effect processor sees the latest.
            let payload = confirm.to_payload();
            effects = effects
                .set_substitute_confirm_state(payload)
                .substitute_confirm_show(adjusted_range, replacement, match_index, total);
        }

        effects
    }
}

/// Emit a summary message after the confirm session ends.
fn emit_confirm_summary(effects: Effects, accepted: usize, lines_changed: usize) -> Effects {
    if accepted > 0 {
        let msg = if lines_changed > 1 {
            format!("{accepted} substitutions on {lines_changed} lines")
        } else {
            format!("{accepted} substitutions")
        };
        effects.show_message(msg)
    } else {
        effects
    }
}
