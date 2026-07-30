//! Macro replay frame stack, key parsing, and interception.
//!
//! Centralizes all macro-related logic extracted from `engine.rs`:
//! - [`MacroFrame`] — replay frame for the engine's macro stack
//! - [`MacroEntry`] — single entry in a macro (keystroke or text block)
//! - [`MacroOutput`] — public return type from macro/typeahead draining
//! - [`parse_keys_from_string`] — register text → key event conversion
//! - [`parse_macro_entries`] — register text → mixed key/text-block entries
//! - [`intercept_macro_effects`] — extracts `PlayMacro` effects from responses
//! - [`resolve_and_parse_macro`] — register resolution and entry parsing

use std::num::NonZeroU32;

use super::Response;
use crate::commands::ex::effects as ex_effects;
use crate::errors::VimError;
use crate::keymap::KeyEvent;
use crate::primitives::RegisterName;
use crate::state::VimState;

/// Convert `MacroRecursionError` into `VimError::RecursiveMacro`.
///
/// Lives in the `execution` layer (not `errors`) to avoid an upward
/// dependency from `errors` → `state`.
impl From<crate::state::MacroRecursionError> for VimError {
    fn from(err: crate::state::MacroRecursionError) -> Self {
        Self::RecursiveMacro {
            register: err.register().char(),
            depth: err.depth() as usize,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════

/// A single entry in a macro register's parsed content.
///
/// Macro registers store a mix of keystrokes and text blocks. Keystrokes
/// go through full dispatch (including auto-pairs), while text blocks are
/// replayed via direct insertion, bypassing per-character insert dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::execution::engine) enum MacroEntry {
    /// A keystroke that goes through full dispatch (including auto-pairs).
    Key(KeyEvent),
    /// Text inserted as a block (completion, paste, IME). Replayed via
    /// direct `Effect::Insert`, bypassing per-character insert dispatch.
    TextBlock {
        text: String,
        /// Cursor byte offset relative to the start of the inserted text.
        /// E.g., 1 for `"()"` means cursor lands between the parens.
        cursor_offset: usize,
    },
}

/// Output from draining the macro replay / typeahead system.
///
/// Returned by the engine's drain API to tell the shell whether to feed
/// the next item through `process()` (a key) or apply it as a direct
/// text insertion (a text block).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacroOutput {
    /// A keystroke for the engine to process normally.
    Key(KeyEvent),
    /// A text block to insert directly at the cursor, bypassing insert dispatch.
    TextBlock {
        /// The text content to insert.
        text: String,
        /// Cursor byte offset relative to start of inserted text.
        cursor_offset: usize,
    },
}

/// A single macro replay frame on the stack.
///
/// Each `@{register}` invocation pushes one frame. Nested macros push
/// additional frames. The frame replays its `entries` vector `remaining_repeats`
/// times by resetting the cursor — no memory duplication for counts.
#[derive(Debug, Clone)]
pub(super) struct MacroFrame {
    /// Pre-parsed entry sequence from the register (keys and text blocks).
    pub entries: Vec<MacroEntry>,
    /// Current position within `entries`.
    pub cursor: usize,
    /// How many more times to replay (starts at count, decrements).
    pub remaining_repeats: NonZeroU32,
}

// ═══════════════════════════════════════════════════════════════════════
// Macro interception (extracted from engine.rs)
// ═══════════════════════════════════════════════════════════════════════

/// Process pre-collected `PlayMacro` effects: resolve registers, parse keys, push frames.
///
/// The `macro_plays` are already extracted from the response by the single-pass
/// `process_effects()` — no re-scanning needed. The engine owns the replay;
/// the shell sees no macro effects.
pub(super) fn process_macro_plays(
    state: &mut VimState,
    macro_stack: &mut Vec<MacroFrame>,
    macro_plays: smallvec::SmallVec<[(RegisterName, NonZeroU32); 2]>,
    response: &mut Response,
    options: &crate::primitives::VimOptions,
) {
    for (register, count) in macro_plays {
        match resolve_and_parse_macro(state, register, options) {
            Ok(Some(entries)) => {
                macro_stack.push(MacroFrame {
                    entries,
                    cursor: 0,
                    remaining_repeats: count,
                });
            }
            Ok(None) => {
                // resolve_and_parse_macro already called end_replay() on empty
                // For sentinel errors, emit ShowError
                if register == RegisterName::LAST_MACRO && state.macros().last_played().is_none() {
                    response.extend_effects(ex_effects::show_error(
                        crate::errors::VimError::NoPreviousRegister,
                    ));
                }
            }
            Err(vim_error) => {
                response.extend_effects(ex_effects::show_error(vim_error));
            }
        }
    }
}

/// Resolve register (handling `@@` sentinel) and parse content to entries.
///
/// Returns `None` for empty/missing registers or unresolvable `@@`.
/// Returns `Err(VimError::RecursiveMacro)` when recursion limit is hit.
/// Manages `begin_replay`/`end_replay` lifecycle.
fn resolve_and_parse_macro(
    state: &mut VimState,
    register: RegisterName,
    options: &crate::primitives::VimOptions,
) -> Result<Option<Vec<MacroEntry>>, crate::errors::VimError> {
    // Resolve '@@' sentinel → last played register
    let register = if register == RegisterName::LAST_MACRO {
        match state.macros().last_played() {
            Some(r) => r,
            None => return Ok(None),
        }
    } else {
        register
    };

    // Recursion guard — surfaces E223 instead of silently returning None
    state.macros_mut().begin_replay(register)?;

    // Read and parse register content.
    // Special case: the `.` register (LAST_INSERT) stores last-inserted text
    // in a dedicated VimState field, not in the register store.
    let entries = if register == RegisterName::LAST_INSERT {
        let text = state.last_inserted_text();
        if text.is_empty() {
            state.macros_mut().end_replay();
            return Ok(None);
        }
        parse_macro_entries(text)
    } else {
        match state.registers().get_aliased(register, options) {
            Some(content) if !content.is_empty() => parse_macro_entries(content.text()),
            _ => {
                state.macros_mut().end_replay();
                return Ok(None);
            }
        }
    };

    if entries.is_empty() {
        state.macros_mut().end_replay();
        return Ok(None);
    }

    Ok(Some(entries))
}

// ═══════════════════════════════════════════════════════════════════════
// Entry parsing (text block + key)
// ═══════════════════════════════════════════════════════════════════════

/// Parse raw register text into a sequence of [`MacroEntry`] values.
///
/// Handles three notations:
/// 1. **Raw characters** — printable chars, control codes, escape, etc.
/// 2. **Vim key notation** — `<C-w>`, `<CR>`, `<Esc>`, `<Up>`, etc.
/// 3. **Text block encoding** — `\x16\x16<len>:<offset>:<text>` for block
///    insertions (completion, paste, IME).
///
/// Register text may contain a mix of all three when populated via
/// recording (`q{reg}`), `:let @a = "..."`, or host-injected text blocks.
#[must_use]
pub(in crate::execution::engine) fn parse_macro_entries(text: &str) -> Vec<MacroEntry> {
    let mut entries = Vec::new();
    let bytes = text.as_bytes();
    let mut pos = 0;

    while pos < bytes.len() {
        // Check for text block marker: \x16\x16
        let is_marker =
            bytes.get(pos).copied() == Some(0x16) && bytes.get(pos + 1).copied() == Some(0x16);
        if is_marker {
            pos += 2; // skip past the double Ctrl-V marker

            // Read decimal digits for text_len until ':'
            let (text_len, new_pos) = parse_decimal(bytes, pos);
            pos = new_pos;

            // Expect ':' separator
            if bytes.get(pos).copied() == Some(b':') {
                pos += 1;
            } else {
                // Malformed — treat the marker bytes as individual Ctrl-V keys
                // and rewind. We already consumed the marker, so emit two Ctrl-V keys.
                entries.push(MacroEntry::Key(KeyEvent::ctrl('v')));
                entries.push(MacroEntry::Key(KeyEvent::ctrl('v')));
                continue;
            }

            // Read decimal digits for cursor_offset until ':'
            let (cursor_offset, new_pos) = parse_decimal(bytes, pos);
            pos = new_pos;

            // Expect ':' separator
            if bytes.get(pos).copied() == Some(b':') {
                pos += 1;
            } else {
                // Malformed — emit Ctrl-V keys and the digits we consumed
                entries.push(MacroEntry::Key(KeyEvent::ctrl('v')));
                entries.push(MacroEntry::Key(KeyEvent::ctrl('v')));
                continue;
            }

            // Read exactly text_len bytes
            let end = (pos + text_len).min(bytes.len());
            let block_text = bytes
                .get(pos..end)
                .map_or_else(String::new, |b| String::from_utf8_lossy(b).into_owned());
            pos = end;

            entries.push(MacroEntry::TextBlock {
                text: block_text,
                cursor_offset,
            });
            continue;
        }

        // Delegate to the key-parsing logic for non-text-block content.
        // We need to work character-by-character from this byte position.
        let Some(remaining) = text.get(pos..) else {
            break;
        };
        let Some(c) = remaining.chars().next() else {
            break;
        };

        // Check for Vim notation: <...>
        if c == '<' {
            if let Some(end) = remaining.find('>') {
                if let Some(notation) = remaining.get(..=end) {
                    if let Some(key) = KeyEvent::from_vim_notation(notation) {
                        entries.push(MacroEntry::Key(key));
                        pos += notation.len();
                        continue;
                    }
                }
            }
            // Not valid notation — treat '<' as literal char below
        }

        // Raw character mapping
        let key = match c {
            '\x1b' => KeyEvent::escape(),
            '\r' | '\n' => KeyEvent::enter(),
            '\t' => KeyEvent::tab(),
            '\x08' | '\x7f' => KeyEvent::backspace(),
            // Control characters: Ctrl-A (0x01) through Ctrl-Z (0x1A)
            // (0x16 = Ctrl-V is handled here as a lone Ctrl-V, since
            // double Ctrl-V was already caught above)
            '\x01'..='\x1a' => {
                let letter = (b'a' + (c as u8) - 1) as char;
                KeyEvent::ctrl(letter)
            }
            c => KeyEvent::char(c),
        };
        entries.push(MacroEntry::Key(key));
        pos += c.len_utf8();
    }

    entries
}

/// Parse decimal digits from `bytes` starting at `pos`.
///
/// Returns `(value, new_pos)` where `new_pos` is the position after the
/// last digit. If no digits are found, returns `(0, pos)`.
fn parse_decimal(bytes: &[u8], mut pos: usize) -> (usize, usize) {
    let mut value: usize = 0;
    while let Some(&b) = bytes.get(pos) {
        if !b.is_ascii_digit() {
            break;
        }
        value = value.saturating_mul(10) + (b - b'0') as usize;
        pos += 1;
    }
    (value, pos)
}

/// Parse raw register text into a sequence of `KeyEvent`s.
///
/// Handles two notations:
/// 1. **Raw characters** — printable chars, control codes, escape, etc.
/// 2. **Vim key notation** — `<C-w>`, `<CR>`, `<Esc>`, `<Up>`, etc.
///
/// Register text may contain a mix of both when populated via
/// recording (`q{reg}`) or `:let @a = "..."`.
///
/// **Note:** Text blocks (`\x16\x16...`) are silently filtered out.
/// Use `parse_macro_entries` to preserve them.
#[must_use]
pub fn parse_keys_from_string(text: &str) -> Vec<KeyEvent> {
    parse_macro_entries(text)
        .into_iter()
        .filter_map(|entry| match entry {
            MacroEntry::Key(key) => Some(key),
            MacroEntry::TextBlock { .. } => None,
        })
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════
// VimEngine macro replay API
// ═══════════════════════════════════════════════════════════════════════

impl super::VimEngine {
    /// Advance the macro frame stack and return the next entry.
    ///
    /// This is the internal engine for macro draining. It walks the frame
    /// stack: returns the next entry from the top frame (key or text block),
    /// handles repeat counts by resetting the cursor, and pops exhausted
    /// frames (decrementing replay depth in `MacroState`).
    ///
    /// Returns `None` when all frames are exhausted (replay complete).
    ///
    /// Used by [`drain_next_key()`](Self::drain_next_key).
    pub(in crate::execution::engine) fn pump_macro_key(&mut self) -> Option<MacroOutput> {
        loop {
            let frame = self.typeahead.macro_stack.last_mut()?;

            // Return next entry from current frame
            if let Some(entry) = frame.entries.get(frame.cursor) {
                frame.cursor += 1;
                match entry {
                    MacroEntry::Key(key) => return Some(MacroOutput::Key(*key)),
                    MacroEntry::TextBlock {
                        text,
                        cursor_offset,
                    } => {
                        return Some(MacroOutput::TextBlock {
                            text: text.clone(),
                            cursor_offset: *cursor_offset,
                        });
                    }
                }
            }

            // Current pass exhausted — check for more repeats
            let r = frame.remaining_repeats.get() - 1;
            if let Some(remaining) = NonZeroU32::new(r) {
                frame.remaining_repeats = remaining;
                frame.cursor = 0;
                // Continue loop to return first entry of next repeat
                continue;
            }

            // Frame complete — pop it and decrement depth
            self.typeahead.macro_stack.pop();
            self.state.macros_mut().end_replay();
            // Continue loop to try parent frame (if nested)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Existing parse_keys_from_string tests ───────────────────────────

    #[test]
    fn test_parse_register_basic() {
        let keys = parse_keys_from_string("iHello");
        assert_eq!(keys.len(), 6);
        assert_eq!(keys[0], KeyEvent::char('i'));
        assert_eq!(keys[1], KeyEvent::char('H'));
        assert_eq!(keys[5], KeyEvent::char('o'));
    }

    #[test]
    fn test_parse_register_escape() {
        let keys = parse_keys_from_string("iHi\x1b");
        assert_eq!(keys.len(), 4);
        assert_eq!(keys[3], KeyEvent::escape());
    }

    #[test]
    fn test_parse_register_ctrl() {
        let keys = parse_keys_from_string("\x01\x17"); // Ctrl-A, Ctrl-W
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0], KeyEvent::ctrl('a'));
        assert_eq!(keys[1], KeyEvent::ctrl('w'));
    }

    #[test]
    fn test_parse_register_enter_tab() {
        let keys = parse_keys_from_string("\n\t");
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0], KeyEvent::enter());
        assert_eq!(keys[1], KeyEvent::tab());
    }

    #[test]
    fn test_parse_register_empty() {
        let keys = parse_keys_from_string("");
        assert!(keys.is_empty());
    }

    #[test]
    fn test_parse_register_vim_notation() {
        let keys = parse_keys_from_string("i<CR><Esc>");
        assert_eq!(keys.len(), 3);
        assert_eq!(keys[0], KeyEvent::char('i'));
        assert_eq!(keys[1], KeyEvent::enter());
        assert_eq!(keys[2], KeyEvent::escape());
    }

    #[test]
    fn test_parse_register_ctrl_notation() {
        let keys = parse_keys_from_string("<C-w>j");
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0], KeyEvent::ctrl('w'));
        assert_eq!(keys[1], KeyEvent::char('j'));
    }

    #[test]
    fn test_parse_register_mixed() {
        // Mix of raw chars, raw control codes, and Vim notation
        let keys = parse_keys_from_string("dd<C-w>j\x1b");
        assert_eq!(keys.len(), 5);
        assert_eq!(keys[0], KeyEvent::char('d'));
        assert_eq!(keys[1], KeyEvent::char('d'));
        assert_eq!(keys[2], KeyEvent::ctrl('w'));
        assert_eq!(keys[3], KeyEvent::char('j'));
        assert_eq!(keys[4], KeyEvent::escape());
    }

    #[test]
    fn test_parse_register_invalid_notation_literal() {
        // '<' without valid closing — treated as literal
        let keys = parse_keys_from_string("a<bc");
        assert_eq!(keys.len(), 4);
        assert_eq!(keys[0], KeyEvent::char('a'));
        assert_eq!(keys[1], KeyEvent::char('<'));
        assert_eq!(keys[2], KeyEvent::char('b'));
        assert_eq!(keys[3], KeyEvent::char('c'));
    }

    #[test]
    fn test_macro_frame_cursor_reset() {
        // Verify that MacroFrame replays via cursor reset
        let mut frame = MacroFrame {
            entries: vec![
                MacroEntry::Key(KeyEvent::char('j')),
                MacroEntry::Key(KeyEvent::char('k')),
            ],
            cursor: 0,
            remaining_repeats: NonZeroU32::new(3).unwrap(),
        };

        let mut collected = Vec::new();
        loop {
            if frame.cursor < frame.entries.len() {
                if let MacroEntry::Key(key) = &frame.entries[frame.cursor] {
                    collected.push(*key);
                }
                frame.cursor += 1;
            } else {
                let r = frame.remaining_repeats.get() - 1;
                if let Some(new_r) = NonZeroU32::new(r) {
                    frame.remaining_repeats = new_r;
                    frame.cursor = 0;
                } else {
                    break;
                }
            }
        }

        // 3 repeats x 2 keys = 6 total
        assert_eq!(collected.len(), 6);
        assert_eq!(collected[0], KeyEvent::char('j'));
        assert_eq!(collected[1], KeyEvent::char('k'));
        assert_eq!(collected[2], KeyEvent::char('j')); // 2nd repeat
        assert_eq!(collected[4], KeyEvent::char('j')); // 3rd repeat
    }

    #[test]
    fn from_macro_recursion_error() {
        use crate::state::MacroState;
        let mut macros = MacroState::new();
        let reg = crate::primitives::RegisterName::new_unchecked('q');
        for _ in 0..crate::state::MAX_MACRO_DEPTH {
            macros
                .begin_replay(reg)
                .expect("should not error before limit");
        }
        let err = macros.begin_replay(reg).unwrap_err();
        let vim_err: VimError = err.into();
        assert!(matches!(
            vim_err,
            VimError::RecursiveMacro { register: 'q', .. }
        ));
        assert!(vim_err.to_string().starts_with("E223:"));
    }

    // ─── parse_macro_entries tests ───────────────────────────────────────

    #[test]
    fn parse_entries_no_text_blocks() {
        let entries = parse_macro_entries("iprint\x1b");
        assert!(entries.iter().all(|e| matches!(e, MacroEntry::Key(_))));
        assert_eq!(entries.len(), 7); // i, p, r, i, n, t, Esc
    }

    #[test]
    fn parse_entries_text_block_simple() {
        let entries = parse_macro_entries("\x16\x162:1:()");
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            MacroEntry::TextBlock {
                text,
                cursor_offset,
            } => {
                assert_eq!(text, "()");
                assert_eq!(*cursor_offset, 1);
            }
            _ => panic!("expected TextBlock"),
        }
    }

    #[test]
    fn parse_entries_mixed() {
        let input = format!("iprint\x16\x162:1:()\"hello\x1b");
        let entries = parse_macro_entries(&input);
        // i,p,r,i,n,t = 6 keys, TextBlock("()",1), ",h,e,l,l,o = 6 keys, Esc = 1 key
        assert_eq!(entries.len(), 14);
        assert!(matches!(
            &entries[6],
            MacroEntry::TextBlock {
                text,
                cursor_offset
            } if text == "()" && *cursor_offset == 1
        ));
        assert!(matches!(&entries[0], MacroEntry::Key(_)));
        assert!(matches!(&entries[13], MacroEntry::Key(_)));
    }

    #[test]
    fn parse_entries_text_block_with_colon() {
        let entries = parse_macro_entries("\x16\x163:3:a:b");
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            MacroEntry::TextBlock {
                text,
                cursor_offset,
            } => {
                assert_eq!(text, "a:b");
                assert_eq!(*cursor_offset, 3);
            }
            _ => panic!("expected TextBlock"),
        }
    }

    #[test]
    fn parse_entries_cursor_offset_at_end() {
        let entries = parse_macro_entries("\x16\x165:5:hello");
        match &entries[0] {
            MacroEntry::TextBlock {
                text,
                cursor_offset,
            } => {
                assert_eq!(text, "hello");
                assert_eq!(*cursor_offset, 5);
            }
            _ => panic!("expected TextBlock"),
        }
    }

    #[test]
    fn parse_keys_backward_compat() {
        // parse_keys_from_string should still work, just skipping text blocks
        let input = format!("i\x16\x162:1:()hello\x1b");
        let keys = parse_keys_from_string(&input);
        // Should have: i, h, e, l, l, o, Esc (text block filtered out)
        assert_eq!(keys.len(), 7);
    }

    #[test]
    fn parse_entries_empty_text_block() {
        let entries = parse_macro_entries("\x16\x160:0:");
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            MacroEntry::TextBlock {
                text,
                cursor_offset,
            } => {
                assert_eq!(text, "");
                assert_eq!(*cursor_offset, 0);
            }
            _ => panic!("expected TextBlock"),
        }
    }

    #[test]
    fn parse_entries_lone_ctrl_v_not_marker() {
        // A single \x16 is just Ctrl-V, not a text block marker
        let entries = parse_macro_entries("\x16a");
        assert_eq!(entries.len(), 2);
        assert!(matches!(&entries[0], MacroEntry::Key(k) if *k == KeyEvent::ctrl('v')));
        assert!(matches!(&entries[1], MacroEntry::Key(k) if *k == KeyEvent::char('a')));
    }

    #[test]
    fn parse_entries_multiple_text_blocks() {
        // Two text blocks back-to-back: "()" with cursor at 1, "[]" with cursor at 1, then 'x'.
        let input = format!("\x16\x162:1:()\x16\x162:1:[]x");
        let entries = parse_macro_entries(&input);
        assert_eq!(entries.len(), 3);
        assert!(matches!(
            &entries[0],
            MacroEntry::TextBlock { text, cursor_offset }
            if text == "()" && *cursor_offset == 1
        ));
        assert!(matches!(
            &entries[1],
            MacroEntry::TextBlock { text, cursor_offset }
            if text == "[]" && *cursor_offset == 1
        ));
        assert!(matches!(&entries[2], MacroEntry::Key(k) if *k == KeyEvent::char('x')));
    }

    #[test]
    fn parse_entries_text_block_multibyte() {
        // Text block with multi-byte UTF-8 content
        // "ab" in UTF-8 is 6 bytes (each CJK char is 3 bytes)
        let cjk = "\u{4e16}\u{754c}"; // two CJK characters, 6 bytes
        let input = format!("\x16\x166:0:{cjk}");
        let entries = parse_macro_entries(&input);
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            MacroEntry::TextBlock {
                text,
                cursor_offset,
            } => {
                assert_eq!(text, cjk);
                assert_eq!(*cursor_offset, 0);
            }
            _ => panic!("expected TextBlock"),
        }
    }

    // ─── MacroOutput type tests ──────────────────────────────────────────

    #[test]
    fn macro_output_key_equality() {
        let a = MacroOutput::Key(KeyEvent::char('x'));
        let b = MacroOutput::Key(KeyEvent::char('x'));
        assert_eq!(a, b);
    }

    #[test]
    fn macro_output_text_block_equality() {
        let a = MacroOutput::TextBlock {
            text: "()".to_string(),
            cursor_offset: 1,
        };
        let b = MacroOutput::TextBlock {
            text: "()".to_string(),
            cursor_offset: 1,
        };
        assert_eq!(a, b);
    }

    #[test]
    fn macro_output_key_vs_text_block_not_equal() {
        let key = MacroOutput::Key(KeyEvent::char('('));
        let block = MacroOutput::TextBlock {
            text: "(".to_string(),
            cursor_offset: 0,
        };
        assert_ne!(key, block);
    }

    // ─── Text block recording round-trip tests ────────────────────────────

    #[test]
    fn text_block_round_trip_recording() {
        use super::super::recording::append_text_block;

        let mut buf = String::new();
        append_text_block(&mut buf, "()", 1);
        let entries = parse_macro_entries(&buf);
        assert_eq!(entries.len(), 1);
        assert!(
            matches!(
                &entries[0],
                MacroEntry::TextBlock { text, cursor_offset }
                if text == "()" && *cursor_offset == 1
            ),
            "expected TextBlock('()', 1), got {:?}",
            &entries[0]
        );
    }

    #[test]
    fn text_block_round_trip_recording_empty() {
        use super::super::recording::append_text_block;

        let mut buf = String::new();
        append_text_block(&mut buf, "", 0);
        let entries = parse_macro_entries(&buf);
        assert_eq!(entries.len(), 1);
        assert!(matches!(
            &entries[0],
            MacroEntry::TextBlock { text, cursor_offset }
            if text.is_empty() && *cursor_offset == 0
        ));
    }

    #[test]
    fn text_block_round_trip_recording_multibyte() {
        use super::super::recording::append_text_block;

        let content = "\u{4e16}\u{754c}"; // CJK, 6 bytes
        let mut buf = String::new();
        append_text_block(&mut buf, content, 3);
        let entries = parse_macro_entries(&buf);
        assert_eq!(entries.len(), 1);
        assert!(matches!(
            &entries[0],
            MacroEntry::TextBlock { text, cursor_offset }
            if text == content && *cursor_offset == 3
        ));
    }

    #[test]
    fn text_block_round_trip_with_colons() {
        use super::super::recording::append_text_block;

        let mut buf = String::new();
        append_text_block(&mut buf, "a:b:c", 2);
        let entries = parse_macro_entries(&buf);
        assert_eq!(entries.len(), 1);
        assert!(matches!(
            &entries[0],
            MacroEntry::TextBlock { text, cursor_offset }
            if text == "a:b:c" && *cursor_offset == 2
        ));
    }

    #[test]
    fn text_block_round_trip_mixed_with_keys() {
        use super::super::recording::append_text_block;

        let mut buf = String::new();
        buf.push('i'); // key: 'i'
        append_text_block(&mut buf, "()", 1);
        buf.push('\x1b'); // key: Esc
        let entries = parse_macro_entries(&buf);
        assert_eq!(entries.len(), 3);
        assert!(matches!(&entries[0], MacroEntry::Key(k) if *k == KeyEvent::char('i')));
        assert!(matches!(
            &entries[1],
            MacroEntry::TextBlock { text, cursor_offset }
            if text == "()" && *cursor_offset == 1
        ));
        assert!(matches!(&entries[2], MacroEntry::Key(k) if *k == KeyEvent::escape()));
    }

    // ─── pump_macro_key returns MacroOutput tests ─────────────────────────

    #[test]
    fn pump_returns_text_block() {
        let mut engine = super::super::VimEngine::new();

        // Prime the macro state
        engine
            .state
            .macros_mut()
            .begin_replay(crate::primitives::RegisterName::new('a').unwrap())
            .expect("begin_replay should succeed");

        engine.typeahead.macro_stack.push(MacroFrame {
            entries: vec![MacroEntry::TextBlock {
                text: "()".to_string(),
                cursor_offset: 1,
            }],
            cursor: 0,
            remaining_repeats: NonZeroU32::MIN,
        });

        let output = engine.pump_macro_key();
        assert_eq!(
            output,
            Some(MacroOutput::TextBlock {
                text: "()".to_string(),
                cursor_offset: 1,
            })
        );

        // Stack should be exhausted
        assert_eq!(engine.pump_macro_key(), None);
    }

    #[test]
    fn pump_returns_mixed_key_and_text_block() {
        let mut engine = super::super::VimEngine::new();

        engine
            .state
            .macros_mut()
            .begin_replay(crate::primitives::RegisterName::new('b').unwrap())
            .expect("begin_replay should succeed");

        engine.typeahead.macro_stack.push(MacroFrame {
            entries: vec![
                MacroEntry::Key(KeyEvent::char('i')),
                MacroEntry::TextBlock {
                    text: "hello".to_string(),
                    cursor_offset: 5,
                },
                MacroEntry::Key(KeyEvent::escape()),
            ],
            cursor: 0,
            remaining_repeats: NonZeroU32::MIN,
        });

        assert_eq!(
            engine.pump_macro_key(),
            Some(MacroOutput::Key(KeyEvent::char('i')))
        );
        assert_eq!(
            engine.pump_macro_key(),
            Some(MacroOutput::TextBlock {
                text: "hello".to_string(),
                cursor_offset: 5,
            })
        );
        assert_eq!(
            engine.pump_macro_key(),
            Some(MacroOutput::Key(KeyEvent::escape()))
        );
        assert_eq!(engine.pump_macro_key(), None);
    }

    #[test]
    fn pump_text_block_with_repeats() {
        let mut engine = super::super::VimEngine::new();

        engine
            .state
            .macros_mut()
            .begin_replay(crate::primitives::RegisterName::new('c').unwrap())
            .expect("begin_replay should succeed");

        engine.typeahead.macro_stack.push(MacroFrame {
            entries: vec![MacroEntry::TextBlock {
                text: "x".to_string(),
                cursor_offset: 1,
            }],
            cursor: 0,
            remaining_repeats: NonZeroU32::new(3).unwrap(),
        });

        // Should return the text block 3 times
        for _ in 0..3 {
            assert_eq!(
                engine.pump_macro_key(),
                Some(MacroOutput::TextBlock {
                    text: "x".to_string(),
                    cursor_offset: 1,
                })
            );
        }
        assert_eq!(engine.pump_macro_key(), None);
    }

    // ─── @. (LAST_INSERT register as macro) tests ────────────────────────

    #[test]
    fn resolve_dot_register_reads_last_inserted_text() {
        let mut state = VimState::default();
        let options = crate::primitives::VimOptions::default();

        // Store "hello" as the last inserted text
        state.store_last_inserted_text("hello");

        // Resolve the `.` register for macro playback
        let result = resolve_and_parse_macro(&mut state, RegisterName::LAST_INSERT, &options);
        let entries = result
            .expect("should not error")
            .expect("should have entries");

        // "hello" → 5 key entries: h, e, l, l, o
        assert_eq!(entries.len(), 5);
        assert_eq!(entries[0], MacroEntry::Key(KeyEvent::char('h')));
        assert_eq!(entries[1], MacroEntry::Key(KeyEvent::char('e')));
        assert_eq!(entries[2], MacroEntry::Key(KeyEvent::char('l')));
        assert_eq!(entries[3], MacroEntry::Key(KeyEvent::char('l')));
        assert_eq!(entries[4], MacroEntry::Key(KeyEvent::char('o')));

        // Clean up replay depth
        state.macros_mut().end_replay();
    }

    #[test]
    fn resolve_dot_register_empty_returns_none() {
        let mut state = VimState::default();
        let options = crate::primitives::VimOptions::default();

        // Empty last inserted text
        state.store_last_inserted_text("");

        let result = resolve_and_parse_macro(&mut state, RegisterName::LAST_INSERT, &options);
        assert!(
            result.expect("should not error").is_none(),
            "empty dot register should return None"
        );
    }
}
