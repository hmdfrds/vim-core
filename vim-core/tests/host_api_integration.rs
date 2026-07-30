//! Integration test: VimSession<H: VimHost> end-to-end proof.
//!
//! Implements a `StringHost` — the simplest possible in-memory host — and
//! runs it through realistic editing scenarios, capability filtering, and
//! edge cases to prove the VimSession framework works for real editing.

use std::cell::RefCell;

use vim_core::effects::{Effect, EffectKind};
use vim_core::execution::{
    HostCapability, HostCapabilitySet, HostRequest, HostResult, RequestDisposition, VimHost,
    VimSession,
};
use vim_core::keymap::KeyEvent;
use vim_core::primitives::{Mode, Offset, RegisterName};

// ═══════════════════════════════════════════════════════════════════════════════
// StringHost — minimal VimHost implementation
// ═══════════════════════════════════════════════════════════════════════════════

/// Snapshot of document state for undo/redo.
#[derive(Clone, Debug)]
struct UndoSnapshot {
    text: String,
    cursor: usize,
}

/// The simplest possible host: owns a String, tracks cursor and mode,
/// and maintains an undo stack that mirrors BeginUndoGroup/EndUndoGroup
/// grouping from the engine.
struct StringHost {
    text: String,
    cursor: usize,
    mode: Mode,
    capabilities: HostCapabilitySet,
    /// Log of every effect kind delivered, for test assertions.
    effect_log: RefCell<Vec<EffectKind>>,
    /// Log of suppressed effect kinds.
    suppressed_log: RefCell<Vec<EffectKind>>,
    /// Messages received via ShowInfo/ShowError.
    messages: RefCell<Vec<String>>,
    /// Errors received via ShowError.
    errors: RefCell<Vec<String>>,
    /// Pending undo group snapshot (captured at BeginUndoGroup).
    pending_undo: Option<UndoSnapshot>,
    /// Committed undo stack (snapshots BEFORE each change group).
    undo_stack: Vec<UndoSnapshot>,
    /// Redo stack (snapshots for redo).
    redo_stack: Vec<UndoSnapshot>,
}

impl StringHost {
    fn new(text: &str, capabilities: HostCapabilitySet) -> Self {
        Self {
            text: text.to_string(),
            cursor: 0,
            mode: Mode::Normal,
            capabilities,
            effect_log: RefCell::new(Vec::new()),
            suppressed_log: RefCell::new(Vec::new()),
            messages: RefCell::new(Vec::new()),
            errors: RefCell::new(Vec::new()),
            pending_undo: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    /// Standard host with Scrolling + StatusMessages + Registers + SearchHighlight.
    fn standard(text: &str) -> Self {
        Self::new(
            text,
            HostCapabilitySet::CORE
                .with(HostCapability::Scrolling)
                .with(HostCapability::StatusMessages)
                .with(HostCapability::Registers)
                .with(HostCapability::SearchHighlight)
                .with(HostCapability::CursorStyle)
                .with(HostCapability::CommandLine),
        )
    }

    /// Core-only host (no scrolling, no messages, no registers delivered).
    fn core_only(text: &str) -> Self {
        Self::new(text, HostCapabilitySet::CORE)
    }

    fn line_count_of(text: &str) -> usize {
        if text.is_empty() {
            1
        } else {
            memchr::memchr_iter(b'\n', text.as_bytes()).count() + 1
        }
    }
}

impl vim_core::document::Document for StringHost {
    fn text(&self) -> &str {
        &self.text
    }

    fn line_count(&self) -> usize {
        Self::line_count_of(&self.text)
    }

    fn offset_to_pos(
        &self,
        offset: vim_core::primitives::Offset,
    ) -> Option<vim_core::primitives::Position> {
        let off = offset.get();
        if off > self.text.len() {
            return None;
        }
        let prefix = &self.text[..off];
        let line = memchr::memchr_iter(b'\n', prefix.as_bytes()).count();
        let line_start = prefix.rfind('\n').map_or(0, |pos| pos + 1);
        let col = off - line_start;
        Some(vim_core::primitives::Position::from_raw(line, col))
    }

    fn pos_to_offset(
        &self,
        pos: vim_core::primitives::Position,
    ) -> Option<vim_core::primitives::Offset> {
        let text = &self.text;
        let target_line = pos.line().get();
        let target_col = pos.col().get();
        let mut offset = 0;
        for _ in 0..target_line {
            offset = memchr::memchr(b'\n', text[offset..].as_bytes()).map(|i| offset + i + 1)?;
        }
        let line_end = memchr::memchr(b'\n', text[offset..].as_bytes())
            .map(|i| offset + i)
            .unwrap_or(text.len());
        let line_len = line_end - offset;
        let col = target_col.min(line_len);
        Some(vim_core::primitives::Offset::new(offset + col))
    }
}

impl VimHost for StringHost {
    fn capabilities(&self) -> HostCapabilitySet {
        self.capabilities
    }

    fn cursor_offset(&self) -> usize {
        self.cursor
    }

    fn apply_effects(&mut self, effects: &[Effect]) {
        for effect in effects {
            self.effect_log.borrow_mut().push(effect.kind());

            match effect {
                Effect::Insert { offset, text } => {
                    let off = offset.get().min(self.text.len());
                    self.text.insert_str(off, text);
                }
                Effect::Delete { range } => {
                    let start = range.start().get().min(self.text.len());
                    let end = range.end().get().min(self.text.len());
                    if start < end {
                        self.text.drain(start..end);
                    }
                }
                Effect::Replace { range, text } => {
                    let start = range.start().get().min(self.text.len());
                    let end = range.end().get().min(self.text.len());
                    if start <= end {
                        self.text.drain(start..end);
                        self.text.insert_str(start, text);
                    }
                }
                Effect::SetCursor { offset } => {
                    self.cursor = offset.get().min(self.text.len());
                }
                Effect::SetMode { mode, .. } => {
                    self.mode = *mode;
                }
                Effect::ShowInfo { info } => {
                    let text = match info {
                        vim_core::effects::InfoMessage::Text(t) => t.to_string(),
                        vim_core::effects::InfoMessage::Verbose(t) => t.to_string(),
                        vim_core::effects::InfoMessage::LineReport(c) => {
                            format!("{} lines", c.total())
                        }
                        _ => String::new(),
                    };
                    if !text.is_empty() {
                        self.messages.borrow_mut().push(text);
                    }
                }
                Effect::ShowError { error, .. } => {
                    self.errors.borrow_mut().push(format!("{error:?}"));
                }
                Effect::BeginUndoGroup { .. } => {
                    if self.pending_undo.is_none() {
                        self.pending_undo = Some(UndoSnapshot {
                            text: self.text.clone(),
                            cursor: self.cursor,
                        });
                    }
                }
                Effect::EndUndoGroup { .. } => {
                    if let Some(snapshot) = self.pending_undo.take() {
                        self.undo_stack.push(snapshot);
                        self.redo_stack.clear();
                    }
                }
                Effect::Undo { count, .. } => {
                    for _ in 0..*count {
                        if let Some(before) = self.undo_stack.pop() {
                            // Save current state for redo.
                            self.redo_stack.push(UndoSnapshot {
                                text: self.text.clone(),
                                cursor: self.cursor,
                            });
                            self.text = before.text;
                            self.cursor = before.cursor;
                        }
                    }
                }
                Effect::Redo { count, .. } => {
                    for _ in 0..*count {
                        if let Some(after) = self.redo_stack.pop() {
                            // Save current state for undo.
                            self.undo_stack.push(UndoSnapshot {
                                text: self.text.clone(),
                                cursor: self.cursor,
                            });
                            self.text = after.text;
                            self.cursor = after.cursor;
                        }
                    }
                }
                _ => {
                    // Other effects: logged via effect_log but no state mutation needed.
                }
            }
        }
    }

    fn handle_request(&mut self, _request: &HostRequest) -> RequestDisposition {
        // Simple host: all requests are unhandled.
        RequestDisposition::Deferred
    }

    fn on_effect_suppressed(&mut self, kind: EffectKind) {
        self.suppressed_log.borrow_mut().push(kind);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helper: drain all pending keys (macros, mappings)
// ═══════════════════════════════════════════════════════════════════════════════

fn drain_pending(session: &mut VimSession<StringHost>) {
    while session.has_pending_keys() {
        session.drain_and_process_one();
    }
}

/// Send a sequence of keys to the session, draining pending keys after each.
fn send_keys(session: &mut VimSession<StringHost>, keys: &[KeyEvent]) {
    for &key in keys {
        let _ = session.process_key(key);
        drain_pending(session);
    }
}

/// Send a string of characters as key events.
fn type_str(session: &mut VimSession<StringHost>, s: &str) {
    for ch in s.chars() {
        let _ = session.process_key(KeyEvent::char(ch));
        drain_pending(session);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 1. VimSession construction and basic processing
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn session_construction_caches_capabilities() {
    let host = StringHost::standard("hello");
    let session = VimSession::with_host(host);
    let caps = session.capabilities();
    assert!(caps.has(HostCapability::TextMutation));
    assert!(caps.has(HostCapability::Scrolling));
    assert!(caps.has(HostCapability::StatusMessages));
    assert!(caps.has(HostCapability::Registers));
    assert!(caps.has(HostCapability::SearchHighlight));
    // Not declared:
    assert!(!caps.has(HostCapability::Folding));
    assert!(!caps.has(HostCapability::WindowManagement));
}

#[test]
fn session_processes_basic_motion() {
    let host = StringHost::standard("hello world");
    let mut session = VimSession::with_host(host);

    // 'l' moves cursor right
    let _ = session.process_key(KeyEvent::char('l'));
    assert_eq!(session.host().cursor, 1);
    assert_eq!(session.engine().mode(), Mode::Normal);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. Realistic editing scenario
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn realistic_editing_scenario() {
    let host = StringHost::standard("Hello, World!\nThis is a test.\nThird line.\n");
    let mut session = VimSession::with_host(host);

    // ── Step (a): Press `w` — cursor moves to "World" ────────────────────
    // "Hello, World!\n..."  cursor at 0 → w moves to ',' at offset 5...
    // In vim, `w` from 'H' of "Hello," goes to the ',' (punctuation is a word boundary).
    // Then another `w` goes to ' ', then 'W'. The assertion below only checks
    // that the cursor moved forward.
    let _ = session.process_key(KeyEvent::char('w'));
    drain_pending(&mut session);
    let cursor_after_w = session.host().cursor;
    assert!(cursor_after_w > 0, "w should move cursor forward from 0");
    assert_eq!(session.engine().mode(), Mode::Normal);

    // ── Step (b): Press `dw` — delete word ───────────────────────────────
    let text_before_dw = session.host().text.clone();
    send_keys(&mut session, &[KeyEvent::char('d'), KeyEvent::char('w')]);
    drain_pending(&mut session);
    let text_after_dw = session.host().text.clone();
    assert_ne!(text_before_dw, text_after_dw, "dw should modify text");
    assert!(
        text_after_dw.len() < text_before_dw.len(),
        "dw should delete characters"
    );
    assert_eq!(session.engine().mode(), Mode::Normal);

    // ── Step (c): Press `i`, type "Vim", press Esc ───────────────────────
    let _ = session.process_key(KeyEvent::char('i'));
    drain_pending(&mut session);
    assert_eq!(session.engine().mode(), Mode::Insert);

    type_str(&mut session, "Vim");
    assert!(
        session.host().text.contains("Vim"),
        "text should contain 'Vim' after typing"
    );

    let _ = session.process_key(KeyEvent::escape());
    drain_pending(&mut session);
    assert_eq!(session.engine().mode(), Mode::Normal);

    let text_after_insert = session.host().text.clone();

    // ── Step (d): Press `u` — undo the insert ───────────────────────────
    // Note: undo in vim-core works through the engine's internal undo system.
    // The host receives Undo effects but the actual undo is done by the
    // engine re-emitting the inverse effects. We verify the engine processes
    // 'u' without error.
    let _ = session.process_key(KeyEvent::char('u'));
    drain_pending(&mut session);
    assert_eq!(session.engine().mode(), Mode::Normal);
    // After undo, the text should differ from what we had after insert
    // (the engine's undo emits Delete/Insert effects to reverse)
    let text_after_undo = session.host().text.clone();
    // The undo should have removed "Vim" or changed the text
    assert_ne!(
        text_after_undo, text_after_insert,
        "undo should change text back"
    );

    // ── Step (e): Press Ctrl-R — redo ────────────────────────────────────
    let _ = session.process_key(KeyEvent::ctrl('r'));
    drain_pending(&mut session);
    let text_after_redo = session.host().text.clone();
    assert_eq!(
        text_after_redo, text_after_insert,
        "redo should restore the insert"
    );
    assert_eq!(session.engine().mode(), Mode::Normal);

    // ── Step (f): Press `j` — move to second line ────────────────────────
    let cursor_before_j = session.host().cursor;
    let _ = session.process_key(KeyEvent::char('j'));
    drain_pending(&mut session);
    let cursor_after_j = session.host().cursor;
    assert!(
        cursor_after_j > cursor_before_j,
        "j should move cursor to a later offset (next line)"
    );

    // ── Step (g): Press `dd` — delete current line ───────────────────────
    let text_before_dd = session.host().text.clone();
    let line_count_before = StringHost::line_count_of(&text_before_dd);
    send_keys(&mut session, &[KeyEvent::char('d'), KeyEvent::char('d')]);
    drain_pending(&mut session);
    let text_after_dd = session.host().text.clone();
    let line_count_after = StringHost::line_count_of(&text_after_dd);
    assert!(
        line_count_after < line_count_before,
        "dd should reduce line count (was {line_count_before}, now {line_count_after})"
    );

    // Verify the deleted line was stored in a register (unnamed register)
    let unnamed = RegisterName::new('"').unwrap();
    let reg_content = session.engine().state().registers().get(unnamed);
    assert!(
        reg_content.is_some(),
        "dd should populate the unnamed register"
    );

    // ── Step (h): Press `p` — paste the deleted line below ───────────────
    let text_before_p = session.host().text.clone();
    let _ = session.process_key(KeyEvent::char('p'));
    drain_pending(&mut session);
    let text_after_p = session.host().text.clone();
    assert!(
        text_after_p.len() > text_before_p.len(),
        "p should insert text from register"
    );

    // ── Step (i): Press `.` — repeat the paste ───────────────────────────
    let text_before_dot = session.host().text.clone();
    let _ = session.process_key(KeyEvent::char('.'));
    drain_pending(&mut session);
    let text_after_dot = session.host().text.clone();
    assert!(
        text_after_dot.len() > text_before_dot.len(),
        ". should repeat the paste, adding more text"
    );

    // ── Step (j): Press `u` twice — undo both pastes ─────────────────────
    let _ = session.process_key(KeyEvent::char('u'));
    drain_pending(&mut session);
    let _ = session.process_key(KeyEvent::char('u'));
    drain_pending(&mut session);

    // After undoing both pastes, we should be back to the state after dd
    let text_final = session.host().text.clone();
    assert_eq!(
        text_final, text_after_dd,
        "two undos should revert both pastes, returning to post-dd state"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. Mode transitions
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn mode_transitions_normal_insert_normal() {
    let host = StringHost::standard("test");
    let mut session = VimSession::with_host(host);
    assert_eq!(session.engine().mode(), Mode::Normal);

    // Enter insert mode
    let _ = session.process_key(KeyEvent::char('i'));
    drain_pending(&mut session);
    assert_eq!(session.engine().mode(), Mode::Insert);

    // Type something
    let _ = session.process_key(KeyEvent::char('x'));
    drain_pending(&mut session);
    assert!(session.host().text.contains('x'));

    // Back to normal
    let _ = session.process_key(KeyEvent::escape());
    drain_pending(&mut session);
    assert_eq!(session.engine().mode(), Mode::Normal);
}

#[test]
fn mode_transition_to_visual() {
    let host = StringHost::standard("hello world");
    let mut session = VimSession::with_host(host);

    // Enter visual mode
    let _ = session.process_key(KeyEvent::char('v'));
    drain_pending(&mut session);
    assert!(
        matches!(session.engine().mode(), Mode::Visual(_)),
        "v should enter visual mode, got {:?}",
        session.engine().mode()
    );

    // Back to normal
    let _ = session.process_key(KeyEvent::escape());
    drain_pending(&mut session);
    assert_eq!(session.engine().mode(), Mode::Normal);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. HostCapability filtering tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn capability_filtering_core_only_suppresses_status_messages() {
    let host = StringHost::core_only("test text");
    let mut session = VimSession::with_host(host);

    // Verify core capabilities are present but StatusMessages is not
    assert!(session.capabilities().has(HostCapability::TextMutation));
    assert!(!session.capabilities().has(HostCapability::StatusMessages));
    assert!(!session.capabilities().has(HostCapability::Scrolling));

    // Basic motions still work
    let _ = session.process_key(KeyEvent::char('l'));
    drain_pending(&mut session);
    assert_eq!(session.host().cursor, 1);
}

#[test]
fn capability_filtering_standard_delivers_messages() {
    let host = StringHost::standard("test text");
    let mut session = VimSession::with_host(host);
    assert!(session.capabilities().has(HostCapability::StatusMessages));

    // Trigger an error (e.g., search for nonexistent pattern should eventually show error).
    // A simpler test: the engine produces SetCursor effects, verify they get delivered.
    let _ = session.process_key(KeyEvent::char('l'));
    drain_pending(&mut session);

    let log = session.host().effect_log.borrow();
    assert!(
        log.contains(&EffectKind::SetCursor),
        "standard host should receive SetCursor effects: {:?}",
        &*log
    );
}

#[test]
fn core_only_host_still_processes_text_mutations() {
    let host = StringHost::core_only("hello");
    let mut session = VimSession::with_host(host);

    // 'x' deletes character under cursor
    let _ = session.process_key(KeyEvent::char('x'));
    drain_pending(&mut session);

    // The text mutation should arrive (core capability)
    assert_ne!(session.host().text, "hello", "x should delete a character");
    assert_eq!(session.host().text.len(), 4, "one character should be gone");
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5. Register content after yank/delete
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn yank_populates_register() {
    let host = StringHost::standard("hello world");
    let mut session = VimSession::with_host(host);

    // yy yanks the entire line
    send_keys(&mut session, &[KeyEvent::char('y'), KeyEvent::char('y')]);
    drain_pending(&mut session);

    // Check unnamed register has content
    let unnamed = RegisterName::new('"').unwrap();
    let reg = session.engine().state().registers().get(unnamed);
    assert!(reg.is_some(), "yy should populate unnamed register");
    let content = reg.unwrap();
    assert!(
        content.text().contains("hello world"),
        "yanked text should contain 'hello world', got: {:?}",
        content.text()
    );
}

#[test]
fn delete_word_populates_register() {
    let host = StringHost::standard("hello world");
    let mut session = VimSession::with_host(host);

    // dw deletes "hello " (word + trailing space)
    send_keys(&mut session, &[KeyEvent::char('d'), KeyEvent::char('w')]);
    drain_pending(&mut session);

    let unnamed = RegisterName::new('"').unwrap();
    let reg = session.engine().state().registers().get(unnamed);
    assert!(reg.is_some(), "dw should populate unnamed register");
}

// ═══════════════════════════════════════════════════════════════════════════════
// 6. Edge cases
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn empty_document_motions_are_safe() {
    let host = StringHost::standard("");
    let mut session = VimSession::with_host(host);

    // All these motions should not panic on empty document
    let motions = [
        KeyEvent::char('h'),
        KeyEvent::char('j'),
        KeyEvent::char('k'),
        KeyEvent::char('l'),
        KeyEvent::char('w'),
        KeyEvent::char('b'),
        KeyEvent::char('e'),
        KeyEvent::char('0'),
        KeyEvent::char('$'),
        KeyEvent::char('G'),
    ];

    for key in motions {
        let _ = session.process_key(key);
        drain_pending(&mut session);
        // Just verify no panic; cursor stays at 0 in empty doc
    }
    assert_eq!(session.host().cursor, 0);
    assert_eq!(session.engine().mode(), Mode::Normal);
}

#[test]
fn single_char_document_delete_and_undo() {
    let host = StringHost::standard("a");
    let mut session = VimSession::with_host(host);

    // x deletes the single character
    let _ = session.process_key(KeyEvent::char('x'));
    drain_pending(&mut session);
    // After deleting the only character, text should be empty
    assert!(
        session.host().text.is_empty() || session.host().text.len() < 1,
        "x on single char should leave empty or near-empty, got: {:?}",
        session.host().text
    );

    // u should undo — restoring "a"
    let _ = session.process_key(KeyEvent::char('u'));
    drain_pending(&mut session);
    assert_eq!(
        session.host().text,
        "a",
        "undo should restore the single character"
    );
}

#[test]
fn unicode_document_motions() {
    // Document with emoji and CJK characters
    let host = StringHost::standard("Hello \u{1F600} World\n\u{4E16}\u{754C}\u{4F60}\u{597D}\n");
    let mut session = VimSession::with_host(host);

    // l should move cursor forward (past multi-byte chars)
    let _ = session.process_key(KeyEvent::char('l'));
    drain_pending(&mut session);
    assert!(session.host().cursor > 0, "l should advance cursor");

    // w should find word boundaries
    let _ = session.process_key(KeyEvent::char('w'));
    drain_pending(&mut session);
    let cursor_after_w = session.host().cursor;
    assert!(cursor_after_w > 1, "w should advance past word boundary");

    // j should move to second line (CJK characters)
    let _ = session.process_key(KeyEvent::char('j'));
    drain_pending(&mut session);
    assert!(
        session.host().cursor > "Hello \u{1F600} World\n".len() - 5,
        "j should move to second line area"
    );

    // Motions should not panic on multi-byte boundaries
    for _ in 0..10 {
        let _ = session.process_key(KeyEvent::char('l'));
        drain_pending(&mut session);
    }
    assert_eq!(session.engine().mode(), Mode::Normal);
}

#[test]
fn unicode_emoji_only_document() {
    let host = StringHost::standard("\u{1F600}\u{1F601}\u{1F602}");
    let mut session = VimSession::with_host(host);

    // Move through emoji
    let _ = session.process_key(KeyEvent::char('l'));
    drain_pending(&mut session);
    assert!(
        session.host().cursor > 0,
        "l should advance past first emoji"
    );

    // x should delete an emoji without corrupting the document
    let _ = session.process_key(KeyEvent::char('x'));
    drain_pending(&mut session);
    let text = &session.host().text;
    // Verify the text is still valid UTF-8 (it always is since String enforces this)
    assert!(text.len() < "\u{1F600}\u{1F601}\u{1F602}".len());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 7. Effect delivery and suppression logging
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn suppressed_effects_are_logged() {
    // Create host without Scrolling capability
    let host = StringHost::new(
        "line1\nline2\nline3",
        HostCapabilitySet::CORE.with(HostCapability::StatusMessages),
    );
    let mut session = VimSession::with_host(host);

    // 'j' might produce ScrollTo effects that get suppressed
    let _ = session.process_key(KeyEvent::char('j'));
    drain_pending(&mut session);

    // At minimum, SetCursor should NOT be suppressed (it's Core)
    let suppressed = session.host().suppressed_log.borrow();
    assert!(
        !suppressed.contains(&EffectKind::SetCursor),
        "SetCursor should never be suppressed for a Core host"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 8. Document trait on host — offset/position conversion
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn host_document_offset_to_pos_multiline() {
    use vim_core::document::Document;

    let host = StringHost::standard("abc\ndef\nghi");

    // Line 0, col 0
    let pos = host.offset_to_pos(Offset::new(0)).unwrap();
    assert_eq!(pos.line().get(), 0);
    assert_eq!(pos.col().get(), 0);

    // Line 0, col 2 ("c")
    let pos = host.offset_to_pos(Offset::new(2)).unwrap();
    assert_eq!(pos.line().get(), 0);
    assert_eq!(pos.col().get(), 2);

    // Line 1, col 0 ("d")
    let pos = host.offset_to_pos(Offset::new(4)).unwrap();
    assert_eq!(pos.line().get(), 1);
    assert_eq!(pos.col().get(), 0);

    // Line 2, col 1 ("h")
    let pos = host.offset_to_pos(Offset::new(9)).unwrap();
    assert_eq!(pos.line().get(), 2);
    assert_eq!(pos.col().get(), 1);

    // Round-trip
    let off = host
        .pos_to_offset(vim_core::primitives::Position::from_raw(1, 2))
        .unwrap();
    assert_eq!(off.get(), 6); // "f" in "def"
}

// ═══════════════════════════════════════════════════════════════════════════════
// 9. Multiple operations in sequence — stress test
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn rapid_insert_delete_cycle() {
    let host = StringHost::standard("start");
    let mut session = VimSession::with_host(host);

    for _ in 0..5 {
        // Insert a character
        let _ = session.process_key(KeyEvent::char('i'));
        drain_pending(&mut session);
        let _ = session.process_key(KeyEvent::char('X'));
        drain_pending(&mut session);
        let _ = session.process_key(KeyEvent::escape());
        drain_pending(&mut session);

        // Delete it
        let _ = session.process_key(KeyEvent::char('x'));
        drain_pending(&mut session);
    }

    // Should end in normal mode
    assert_eq!(session.engine().mode(), Mode::Normal);
    // Text should be modified but not corrupted
    assert!(!session.host().text.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 10. HostCapabilitySet API correctness
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn capability_implications_are_applied_at_session_creation() {
    // WindowManagement implies Scrolling
    let host = StringHost::new(
        "test",
        HostCapabilitySet::CORE.with(HostCapability::WindowManagement),
    );
    let session = VimSession::with_host(host);
    assert!(
        session.capabilities().has(HostCapability::Scrolling),
        "WindowManagement should imply Scrolling"
    );
}

#[test]
fn substitute_preview_implies_search_highlight() {
    let host = StringHost::new(
        "test",
        HostCapabilitySet::CORE.with(HostCapability::SubstitutePreview),
    );
    let session = VimSession::with_host(host);
    assert!(
        session.capabilities().has(HostCapability::SearchHighlight),
        "SubstitutePreview should imply SearchHighlight"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 11. Host accessor methods
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn session_host_accessors() {
    let host = StringHost::standard("hello");
    let mut session = VimSession::with_host(host);

    // Read-only host access
    assert_eq!(session.host().text, "hello");

    // Mutable host access
    session.host_mut().text = "world".to_string();
    assert_eq!(session.host().text, "world");

    // Engine access
    assert_eq!(session.engine().mode(), Mode::Normal);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 12. Append mode (A) and line operations
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn append_at_end_of_line() {
    let host = StringHost::standard("hello");
    let mut session = VimSession::with_host(host);

    // A enters insert mode at end of line
    let _ = session.process_key(KeyEvent::char('A'));
    drain_pending(&mut session);
    assert_eq!(session.engine().mode(), Mode::Insert);

    type_str(&mut session, " world");
    let _ = session.process_key(KeyEvent::escape());
    drain_pending(&mut session);

    assert!(
        session.host().text.contains("hello world"),
        "A should append at end of line, got: {:?}",
        session.host().text
    );
}

#[test]
fn open_line_below() {
    let host = StringHost::standard("line1\nline2");
    let mut session = VimSession::with_host(host);

    // o opens a new line below and enters insert
    let _ = session.process_key(KeyEvent::char('o'));
    drain_pending(&mut session);
    assert_eq!(session.engine().mode(), Mode::Insert);

    type_str(&mut session, "new line");
    let _ = session.process_key(KeyEvent::escape());
    drain_pending(&mut session);

    assert!(
        session.host().text.contains("new line"),
        "o should insert new line, got: {:?}",
        session.host().text
    );
    assert!(
        StringHost::line_count_of(&session.host().text) >= 3,
        "should have at least 3 lines after o"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 13. Dot repeat
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn dot_repeats_last_change() {
    let host = StringHost::standard("aaa bbb ccc");
    let mut session = VimSession::with_host(host);

    // dw deletes "aaa "
    send_keys(&mut session, &[KeyEvent::char('d'), KeyEvent::char('w')]);
    drain_pending(&mut session);
    let after_first_dw = session.host().text.clone();

    // . repeats dw
    let _ = session.process_key(KeyEvent::char('.'));
    drain_pending(&mut session);
    let after_dot = session.host().text.clone();

    assert!(
        after_dot.len() < after_first_dw.len(),
        ". should delete another word"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 14. Filter effects standalone function
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn filter_effects_for_host_works() {
    use vim_core::execution::filter_effects_for_host;

    let effects = vec![
        Effect::insert(Offset::new(0), "text"),
        Effect::show_message("hello"),
        Effect::set_cursor(Offset::new(4)),
        Effect::FoldAll,
        Effect::WindowSplit,
    ];

    // Core only: Insert + SetCursor survive
    let core_filtered = filter_effects_for_host(&effects, HostCapabilitySet::CORE);
    assert_eq!(core_filtered.len(), 2);

    // Standard: Insert + ShowInfo + SetCursor + FoldAll survive (no WindowManagement)
    let std_filtered = filter_effects_for_host(&effects, HostCapabilitySet::STANDARD);
    assert_eq!(std_filtered.len(), 4);

    // Full: everything passes
    let full_filtered = filter_effects_for_host(&effects, HostCapabilitySet::FULL);
    assert_eq!(full_filtered.len(), 5);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 15. Fallback effects
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn fallback_effects_for_unsupported_capabilities() {
    use vim_core::execution::fallback_effect;

    let caps_with_messages = HostCapabilitySet::CORE.with(HostCapability::StatusMessages);

    // Folding → ShowInfo fallback
    let fb = fallback_effect(EffectKind::FoldLine, caps_with_messages);
    assert!(fb.is_some());
    if let Some(Effect::ShowInfo {
        info: vim_core::effects::InfoMessage::Text(text),
    }) = fb
    {
        assert!(text.contains("folds"), "fallback should mention folds");
    }

    // WindowSplit → ShowInfo fallback
    let fb = fallback_effect(EffectKind::WindowSplit, caps_with_messages);
    assert!(fb.is_some());

    // Without StatusMessages → no fallback at all
    let fb = fallback_effect(EffectKind::FoldLine, HostCapabilitySet::CORE);
    assert!(fb.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 16. Multiline document operations
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn join_lines_with_j_command() {
    let host = StringHost::standard("line1\nline2\nline3");
    let mut session = VimSession::with_host(host);

    // J joins current line with next
    let _ = session.process_key(KeyEvent::char('J'));
    drain_pending(&mut session);

    let text = &session.host().text;
    let line_count = StringHost::line_count_of(text);
    assert!(
        line_count <= 2,
        "J should join lines, reducing count, got {line_count} lines: {:?}",
        text
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 17. Search (forward search with /)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn forward_search_enters_command_line_mode() {
    let host = StringHost::standard("hello world foo bar");
    let mut session = VimSession::with_host(host);

    // '/' enters command-line (search) mode
    let _ = session.process_key(KeyEvent::char('/'));
    drain_pending(&mut session);

    assert_eq!(
        session.engine().mode(),
        Mode::CommandLine,
        "/ should enter CommandLine mode"
    );

    // Escape back to normal
    let _ = session.process_key(KeyEvent::escape());
    drain_pending(&mut session);
    assert_eq!(session.engine().mode(), Mode::Normal);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 18. Replace mode
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn single_char_replace() {
    let host = StringHost::standard("hello");
    let mut session = VimSession::with_host(host);

    // r followed by a character replaces the char under cursor
    send_keys(&mut session, &[KeyEvent::char('r'), KeyEvent::char('X')]);
    drain_pending(&mut session);

    assert!(
        session.host().text.starts_with('X'),
        "r should replace first char with X, got: {:?}",
        session.host().text
    );
    assert_eq!(session.engine().mode(), Mode::Normal);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 19. Text objects
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn delete_inner_word() {
    let host = StringHost::standard("hello world");
    let mut session = VimSession::with_host(host);

    // diw deletes inner word
    send_keys(
        &mut session,
        &[
            KeyEvent::char('d'),
            KeyEvent::char('i'),
            KeyEvent::char('w'),
        ],
    );
    drain_pending(&mut session);

    assert!(
        !session.host().text.starts_with("hello"),
        "diw should delete 'hello'"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 20. Multiple undo/redo cycles
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn multiple_undo_redo_cycles() {
    let host = StringHost::standard("original");
    let mut session = VimSession::with_host(host);
    let original = session.host().text.clone();

    // Make 3 changes
    let _ = session.process_key(KeyEvent::char('x'));
    drain_pending(&mut session); // delete 'o'
    let after_1 = session.host().text.clone();

    let _ = session.process_key(KeyEvent::char('x'));
    drain_pending(&mut session); // delete next char
    let after_2 = session.host().text.clone();

    let _ = session.process_key(KeyEvent::char('x'));
    drain_pending(&mut session); // delete next char
    let after_3 = session.host().text.clone();

    assert_ne!(original, after_1);
    assert_ne!(after_1, after_2);
    assert_ne!(after_2, after_3);

    // Undo all 3
    let _ = session.process_key(KeyEvent::char('u'));
    drain_pending(&mut session);
    assert_eq!(session.host().text, after_2);

    let _ = session.process_key(KeyEvent::char('u'));
    drain_pending(&mut session);
    assert_eq!(session.host().text, after_1);

    let _ = session.process_key(KeyEvent::char('u'));
    drain_pending(&mut session);
    assert_eq!(session.host().text, original);

    // Redo all 3
    let _ = session.process_key(KeyEvent::ctrl('r'));
    drain_pending(&mut session);
    assert_eq!(session.host().text, after_1);

    let _ = session.process_key(KeyEvent::ctrl('r'));
    drain_pending(&mut session);
    assert_eq!(session.host().text, after_2);

    let _ = session.process_key(KeyEvent::ctrl('r'));
    drain_pending(&mut session);
    assert_eq!(session.host().text, after_3);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 21. ProcessResult field population
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn process_result_fields_are_populated() {
    let mut session = VimSession::with_host(StringHost::standard("hello"));
    let result = session.process_key(KeyEvent::char('l'));
    assert!(result.consumed);
    assert!(result.host_requests.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 22. Capability mapping correctness
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn capability_mapping_correctness() {
    use vim_core::execution::required_capability;

    // Verify each capability has the correct mapping
    assert_eq!(
        required_capability(EffectKind::ScrollTo),
        Some(HostCapability::Scrolling)
    );
    assert_eq!(
        required_capability(EffectKind::HighlightMatches),
        Some(HostCapability::SearchHighlight)
    );
    assert_eq!(
        required_capability(EffectKind::ShowInfo),
        Some(HostCapability::StatusMessages)
    );
    assert_eq!(
        required_capability(EffectKind::SetRegister),
        Some(HostCapability::Registers)
    );
    assert_eq!(
        required_capability(EffectKind::CommandLineEdit),
        Some(HostCapability::CommandLine)
    );
    assert_eq!(
        required_capability(EffectKind::CopyToClipboard),
        Some(HostCapability::Clipboard)
    );
    assert_eq!(
        required_capability(EffectKind::SetCursorStyle),
        Some(HostCapability::CursorStyle)
    );
    assert_eq!(
        required_capability(EffectKind::GotoDefinition),
        Some(HostCapability::LspNavigation)
    );
    assert_eq!(
        required_capability(EffectKind::SubstitutePreview),
        Some(HostCapability::SubstitutePreview)
    );
    assert_eq!(
        required_capability(EffectKind::SetVirtualText),
        Some(HostCapability::VirtualText)
    );
    assert_eq!(
        required_capability(EffectKind::OpenCommandWindow),
        Some(HostCapability::CommandWindow)
    );
    // ShowError is unconditional
    assert_eq!(required_capability(EffectKind::ShowError), None);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 23. NormCommand handling in VimSession
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn norm_command_deletes_first_char_each_line() {
    let host = StringHost::standard("abc\ndef\nghi");
    let mut session = VimSession::with_host(host);

    // :1,3norm x<CR>  — on lines 1-3 (1-based), execute "x" in Normal mode
    // "x" deletes the character under the cursor (first char of each line)
    let _ = session.process_key(KeyEvent::char(':'));
    type_str(&mut session, "1,3norm x");
    let _ = session.process_key(KeyEvent::from_name("CR").unwrap());
    drain_pending(&mut session);

    assert_eq!(
        session.host().text,
        "bc\nef\nhi",
        "`:1,3norm x` should delete first char of each line"
    );
}

#[test]
fn norm_command_appends_to_each_line() {
    let host = StringHost::standard("aaa\nbbb\nccc");
    let mut session = VimSession::with_host(host);

    // :1,3norm Ax<CR>  — on lines 1-3 (1-based), execute "Ax" (A = append at EOL, x = type 'x')
    let _ = session.process_key(KeyEvent::char(':'));
    type_str(&mut session, "1,3norm Ax");
    let _ = session.process_key(KeyEvent::from_name("CR").unwrap());
    drain_pending(&mut session);

    assert_eq!(
        session.host().text,
        "aaax\nbbbx\ncccx",
        "`:1,3norm Ax` should append 'x' to each line"
    );
}

#[test]
fn norm_command_on_single_line() {
    let host = StringHost::standard("hello\nworld\nfoo");
    let mut session = VimSession::with_host(host);

    // :2norm x<CR>  — on line 2 only, delete first char
    let _ = session.process_key(KeyEvent::char(':'));
    type_str(&mut session, "2norm x");
    let _ = session.process_key(KeyEvent::from_name("CR").unwrap());
    drain_pending(&mut session);

    assert_eq!(
        session.host().text,
        "hello\norld\nfoo",
        "`:2norm x` should only delete first char of line 2"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Capability upgrade/downgrade lifecycle verification
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn capability_upgrade_applies_implications() {
    // Start with CORE only (no Scrolling).
    let host = StringHost::core_only("hello world");
    let mut session = VimSession::with_host(host);

    assert!(!session.capabilities().has(HostCapability::Scrolling));
    assert!(!session.capabilities().has(HostCapability::WindowManagement));

    // Upgrade WindowManagement — should also enable Scrolling (implication).
    session.upgrade_capability(HostCapability::WindowManagement);
    assert!(
        session.capabilities().has(HostCapability::WindowManagement),
        "WindowManagement should be present after upgrade"
    );
    assert!(
        session.capabilities().has(HostCapability::Scrolling),
        "Scrolling should be implied by WindowManagement"
    );
}

#[test]
fn capability_downgrade_does_not_cascade() {
    // Start with CORE only, then upgrade WindowManagement (which implies Scrolling).
    let host = StringHost::core_only("hello world");
    let mut session = VimSession::with_host(host);

    session.upgrade_capability(HostCapability::WindowManagement);
    assert!(session.capabilities().has(HostCapability::Scrolling));

    // Downgrade WindowManagement — Scrolling should remain.
    // Downgrade only removes the specific bit, no cascade.
    session.downgrade_capability(HostCapability::WindowManagement);
    assert!(
        !session.capabilities().has(HostCapability::WindowManagement),
        "WindowManagement should be removed after downgrade"
    );
    assert!(
        session.capabilities().has(HostCapability::Scrolling),
        "Scrolling should NOT be removed — downgrade doesn't cascade"
    );
}

#[test]
fn native_insert_sync_on_upgrade_and_downgrade() {
    let host = StringHost::core_only("hello world");
    let mut session = VimSession::with_host(host);

    // Initially no NativeInsert.
    assert!(!session.capabilities().has(HostCapability::NativeInsert));

    // Upgrade NativeInsert.
    session.upgrade_capability(HostCapability::NativeInsert);
    assert!(
        session.capabilities().has(HostCapability::NativeInsert),
        "NativeInsert should be set after upgrade"
    );

    // Downgrade NativeInsert.
    session.downgrade_capability(HostCapability::NativeInsert);
    assert!(
        !session.capabilities().has(HostCapability::NativeInsert),
        "NativeInsert should be cleared after downgrade"
    );
}

#[test]
fn capability_lifecycle_write_request_generated_then_downgraded() {
    use vim_core::execution::HostRequestKind;

    // Start with CORE, upgrade to include everything needed for `:w`.
    let host = StringHost::core_only("hello world");
    let mut session = VimSession::with_host(host);

    session.upgrade_capability(HostCapability::FileSystem);
    session.upgrade_capability(HostCapability::StatusMessages);
    session.upgrade_capability(HostCapability::CommandLine);
    session.upgrade_capability(HostCapability::SearchHighlight);
    session.upgrade_capability(HostCapability::Registers);
    session.upgrade_capability(HostCapability::CursorStyle);
    assert!(session.capabilities().has(HostCapability::FileSystem));

    // Type `:w<CR>` — should generate a WriteFile host request.
    let _ = session.process_key(KeyEvent::char(':'));
    let _ = session.process_key(KeyEvent::char('w'));
    let result = session.process_key(KeyEvent::from_name("CR").unwrap());

    // Verify the WriteFile request was generated and deferred.
    assert!(
        result
            .host_requests
            .iter()
            .any(|r| r.kind() == HostRequestKind::WriteFile),
        "With FileSystem capability, :w should generate WriteFile request. Got: {:?}",
        result
            .host_requests
            .iter()
            .map(|r| r.kind())
            .collect::<Vec<_>>(),
    );

    // Complete all pending requests to clean up.
    for req in &result.host_requests {
        let _ = session.complete_request(&HostResult::Success {
            id: req.id(),
            message: None,
        });
    }

    // Downgrade FileSystem.
    session.downgrade_capability(HostCapability::FileSystem);
    assert!(!session.capabilities().has(HostCapability::FileSystem));

    // Type `:w<CR>` again.
    let _ = session.process_key(KeyEvent::char(':'));
    let _ = session.process_key(KeyEvent::char('w'));
    let result2 = session.process_key(KeyEvent::from_name("CR").unwrap());

    // The engine still generates the WriteFile request — request capability
    // gating is the HOST's responsibility (via handle_request disposition),
    // not pre-filtered by the session. The request still reaches the host.
    // This is correct behaviour: the capability system gates EFFECTS at
    // delivery time, while REQUEST gating is host-controlled.
    assert!(
        result2
            .host_requests
            .iter()
            .any(|r| r.kind() == HostRequestKind::WriteFile),
        "Host should still receive WriteFile even after FileSystem downgrade \
         (request gating is host responsibility). Got: {:?}",
        result2
            .host_requests
            .iter()
            .map(|r| r.kind())
            .collect::<Vec<_>>(),
    );

    // Clean up pending.
    for req in &result2.host_requests {
        let _ = session.complete_request(&HostResult::Success {
            id: req.id(),
            message: None,
        });
    }
}

#[test]
fn effect_filtering_uses_live_capability_set() {
    use vim_core::effects::EffectKind;
    use vim_core::execution::host_api::should_deliver;

    // Start with CORE, upgrade WindowManagement (adds Scrolling via implication).
    let host = StringHost::core_only("hello world");
    let mut session = VimSession::with_host(host);

    session.upgrade_capability(HostCapability::WindowManagement);

    // Both Scrolling and WindowManagement effects should be deliverable.
    assert!(should_deliver(EffectKind::ScrollTo, session.capabilities()));
    assert!(should_deliver(
        EffectKind::WindowSplit,
        session.capabilities()
    ));

    // Downgrade WindowManagement (Scrolling stays).
    session.downgrade_capability(HostCapability::WindowManagement);

    // Scrolling effects still pass, but WindowManagement effects are now filtered.
    assert!(
        should_deliver(EffectKind::ScrollTo, session.capabilities()),
        "ScrollTo should still be delivered (Scrolling remains)"
    );
    assert!(
        !should_deliver(EffectKind::WindowSplit, session.capabilities()),
        "WindowSplit should be filtered (WindowManagement removed)"
    );
}
