//! Macro recording helpers.
//!
//! Free functions for macro recording, extracted from `VimEngine` to keep
//! the engine focused on orchestration. These only need the recording buffer,
//! not the full engine.

use std::fmt::Write;

use crate::effects::Effect;
use crate::keymap::{Key, KeyEvent, Modifiers};
use crate::primitives::{MotionType, RegisterName};

/// Safety cap: maximum recorded keystrokes before auto-stop.
/// Prevents unbounded memory growth from pathological macros.
const MAX_RECORDING_LENGTH: usize = 100_000;

/// Append a key to the recording buffer.
///
/// Uses raw bytes where possible (matches nvim's register format),
/// falling back to [`KeyEvent::to_vim_notation`] for keys that have
/// no single-byte representation (arrows, F-keys, modifiers, etc.).
///
/// Round-trips through [`parse_keys_from_string`](super::macro_replay::parse_keys_from_string)
/// which handles both raw bytes and Vim `<...>` notation.
///
/// Returns `false` if the buffer has reached `MAX_RECORDING_LENGTH`,
/// signaling the caller to stop recording.
pub(super) fn record_key(buf: &mut String, key: KeyEvent) -> bool {
    if buf.len() >= MAX_RECORDING_LENGTH {
        return false;
    }
    if key.modifiers().contains(Modifiers::CTRL) {
        if let Key::Char(c) = key.key() {
            // Raw control byte: Ctrl-A = 0x01 .. Ctrl-Z = 0x1A.
            // Guard: only ASCII alphabetic chars map to control bytes.
            if c.is_ascii_alphabetic() {
                let ctrl_byte = c.to_ascii_lowercase() as u8 - b'a' + 1;
                buf.push(ctrl_byte as char);
                return true;
            }
        }
        // Ctrl + non-char key — fall through to Vim notation
        buf.push_str(&key.to_vim_notation());
    } else if key.modifiers().is_empty() {
        match key.key() {
            Key::Char(c) => buf.push(c),
            Key::Escape => buf.push('\x1b'),
            Key::Enter => buf.push('\r'),
            Key::Tab => buf.push('\t'),
            Key::Backspace => buf.push('\x08'),
            // Everything else (arrows, F-keys, Space, Delete, etc.)
            // drift: non-printable keys serialise to Vim notation (<Up>, <F1>, etc.) for macro playback fidelity
            _ => buf.push_str(&key.to_vim_notation()),
        }
    } else {
        // Alt, Shift, or other modifiers — use Vim notation
        buf.push_str(&key.to_vim_notation());
    }
    true
}

/// Flush the recording buffer to the target register.
///
/// Returns a `SetRegister` effect if there was an active recording,
/// so the caller can append it to the response.
pub(super) fn flush_recording(
    recording_buffer: &mut Option<(RegisterName, String)>,
) -> Option<Effect> {
    let (register, mut keys_text) = recording_buffer.take()?;
    // Strip trailing Ctrl-O (\x0f) — when recording stops via `q` from
    // insert mode's one-shot normal (Ctrl-O q), the Ctrl-O that entered
    // one-shot mode gets recorded but shouldn't be replayed.
    while keys_text.ends_with('\x0f') {
        keys_text.pop();
    }
    Some(Effect::set_register(
        register,
        keys_text,
        MotionType::CharWise,
    ))
}

/// Append a text block encoding to the recording buffer.
///
/// Encodes the text block as `\x16\x16<len>:<offset>:<text>`, matching the
/// wire format parsed by [`parse_macro_entries`](super::macro_replay::parse_macro_entries).
/// This allows host-side text insertions (completions, paste, IME) to be
/// faithfully replayed as atomic blocks rather than individual keystrokes.
pub(super) fn append_text_block(buf: &mut String, text: &str, cursor_offset: usize) {
    buf.push('\x16');
    buf.push('\x16');
    // `fmt::Write` for `String` is infallible — discard the always-Ok result.
    let _ = write!(buf, "{}:{}:{}", text.len(), cursor_offset, text);
}

// ═══════════════════════════════════════════════════════════════════════
// VimEngine recording API
// ═══════════════════════════════════════════════════════════════════════

impl super::VimEngine {
    /// Append a key to the recording buffer.
    ///
    /// If the safety cap is reached, the buffer is flushed to the target
    /// register (preserving recorded content), recording state is cleaned up,
    /// and effects are emitted to the response so the host can update UI.
    pub(super) fn record_key(&mut self, key: KeyEvent, response: &mut super::Response) {
        if let Some((_reg, ref mut buf)) = self.recording.buffer {
            if !record_key(buf, key) {
                // Safety cap reached — flush to register (preserving content),
                // stop recording cleanly, and notify.
                if let Some(eff) = flush_recording(&mut self.recording.buffer) {
                    self.apply_effect(&eff);
                    response.effects.push(eff);
                }
                self.state.macros_mut().stop_recording();
                self.parser.set_recording(None);
                response.effects.push(Effect::StopRecording);
                response.effects.push(Effect::ShowInfo {
                    info: crate::effects::InfoMessage::Text(compact_str::CompactString::new(
                        "Recording stopped (limit)",
                    )),
                });
            }
        }
    }

    /// Flush the recording buffer to the target register.
    pub(super) fn flush_recording(&mut self, response: &mut super::Response) {
        if let Some(eff) = flush_recording(&mut self.recording.buffer) {
            self.apply_effect(&eff);
            response.effects.push(eff);
        }
    }

    /// Append literal text to the macro recording buffer.
    ///
    /// Used by the shell to record host-side text mutations (e.g. completion
    /// confirmation) that bypass the engine's normal key processing pipeline.
    /// Only printable characters should be passed — control sequences are not
    /// escaped. No-op if not currently recording.
    pub fn append_to_recording(&mut self, text: &str) {
        if let Some((_reg, ref mut buf)) = self.recording.buffer {
            if buf.len() + text.len() > MAX_RECORDING_LENGTH {
                return;
            }
            buf.push_str(text);
        }
    }

    /// Append a text block to the macro recording buffer.
    ///
    /// Records host-side text mutations (completions, paste, IME) using the
    /// text block encoding (`\x16\x16<len>:<offset>:<text>`) so they can be
    /// faithfully replayed as atomic insertions rather than individual keystrokes.
    /// No-op if not currently recording.
    ///
    /// # Arguments
    /// * `text` - The text content to record.
    /// * `cursor_offset` - Cursor byte offset relative to start of inserted text.
    pub fn append_text_block_to_recording(&mut self, text: &str, cursor_offset: usize) {
        if let Some((_reg, ref mut buf)) = self.recording.buffer {
            if buf.len() + text.len() + 20 > MAX_RECORDING_LENGTH {
                return;
            }
            append_text_block(buf, text, cursor_offset);
        }
    }

    /// Append N backspace characters to the macro recording buffer.
    ///
    /// Used before a TextBlock when a completion replaces text that differs
    /// from the inserted prefix (case-insensitive or fuzzy match). On replay,
    /// the backspaces erase the stale prefix typed by preceding keystrokes.
    pub fn append_backspaces_to_recording(&mut self, count: usize) {
        if let Some((_reg, ref mut buf)) = self.recording.buffer {
            if buf.len() + count > MAX_RECORDING_LENGTH {
                return;
            }
            for _ in 0..count {
                buf.push('\x08');
            }
        }
    }
}
