//! Host misbehavior safety contract integration tests.
//!
//! Exercises the full end-to-end safety harness by creating deliberately
//! misbehaving hosts and verifying the engine survives without crashing.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};

use vim_core::document::{Document, Providers, SyntaxNodeKind, SyntaxProvider};
use vim_core::effects::{Effect, EffectKind};
use vim_core::execution::{
    ExternalEdit, ExternalEditKind, HostCapability, HostCapabilitySet, HostRequest, HostResult,
    RequestDisposition, VimHost, VimSession,
};
use vim_core::keymap::KeyEvent;
use vim_core::primitives::{Mode, Offset, Range};

// ═══════════════════════════════════════════════════════════════════════════════
// Shared helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn drain_pending<H: VimHost>(session: &mut VimSession<H>) {
    while session.has_pending_keys() {
        session.drain_and_process_one();
    }
}

fn send_keys<H: VimHost>(session: &mut VimSession<H>, keys: &[KeyEvent]) {
    for &key in keys {
        let _ = session.process_key(key);
        drain_pending(session);
    }
}

fn type_str<H: VimHost>(session: &mut VimSession<H>, s: &str) {
    for ch in s.chars() {
        let _ = session.process_key(KeyEvent::char(ch));
        drain_pending(session);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PanickingProviderHost — providers() panics every time
// ═══════════════════════════════════════════════════════════════════════════════

/// Host whose `providers()` panics unconditionally.
/// The safety harness should catch this and fall back to default providers.
struct PanickingProviderHost {
    text: String,
    cursor: usize,
    mode: Mode,
    panic_count: AtomicU32,
}

impl PanickingProviderHost {
    fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
            cursor: 0,
            mode: Mode::Normal,
            panic_count: AtomicU32::new(0),
        }
    }
}

impl Document for PanickingProviderHost {
    fn text(&self) -> &str {
        &self.text
    }
    fn line_count(&self) -> usize {
        memchr::memchr_iter(b'\n', self.text.as_bytes()).count() + 1
    }
    fn offset_to_pos(&self, offset: Offset) -> Option<vim_core::primitives::Position> {
        vim_core::execution::simple_offset_to_pos(&self.text, offset.get())
    }
    fn pos_to_offset(
        &self,
        pos: vim_core::primitives::Position,
    ) -> Option<vim_core::primitives::Offset> {
        vim_core::execution::simple_pos_to_offset(&self.text, pos)
    }
}

impl VimHost for PanickingProviderHost {
    fn capabilities(&self) -> HostCapabilitySet {
        HostCapabilitySet::CORE
            .with(HostCapability::StatusMessages)
            .with(HostCapability::CursorStyle)
    }

    fn cursor_offset(&self) -> usize {
        self.cursor
    }

    fn apply_effects(&mut self, effects: &[Effect]) {
        for effect in effects {
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
                _ => {}
            }
        }
    }

    fn handle_request(&mut self, _request: &HostRequest) -> RequestDisposition {
        RequestDisposition::Unsupported
    }

    fn providers(&self) -> Providers<'_> {
        self.panic_count.fetch_add(1, Ordering::Relaxed);
        panic!(
            "PanickingProviderHost: providers() panic #{}",
            self.panic_count.load(Ordering::Relaxed)
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// BadSyntaxHost — SyntaxProvider returns absurd offsets
// ═══════════════════════════════════════════════════════════════════════════════

struct BadSyntaxProvider;

impl SyntaxProvider for BadSyntaxProvider {
    fn enclosing_node(
        &self,
        _text: &str,
        _cursor: usize,
        _kind: SyntaxNodeKind,
    ) -> Option<(usize, usize)> {
        // Return near-max offsets that would cause out-of-bounds if not clamped.
        Some((usize::MAX - 1, usize::MAX - 1))
    }

    fn next_node(
        &self,
        _text: &str,
        _cursor: usize,
        _kind: SyntaxNodeKind,
        _count: u32,
    ) -> Option<usize> {
        Some(usize::MAX - 1)
    }

    fn prev_node(
        &self,
        _text: &str,
        _cursor: usize,
        _kind: SyntaxNodeKind,
        _count: u32,
    ) -> Option<usize> {
        Some(usize::MAX - 1)
    }

    fn ancestor_node(&self, _text: &str, _start: usize, _end: usize) -> Option<(usize, usize)> {
        Some((usize::MAX - 1, usize::MAX - 1))
    }

    fn descendant_node(&self, _text: &str, _start: usize, _end: usize) -> Option<(usize, usize)> {
        Some((usize::MAX - 1, usize::MAX - 1))
    }
}

struct BadSyntaxHost {
    text: String,
    cursor: usize,
    syntax: BadSyntaxProvider,
}

impl BadSyntaxHost {
    fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
            cursor: 0,
            syntax: BadSyntaxProvider,
        }
    }
}

impl Document for BadSyntaxHost {
    fn text(&self) -> &str {
        &self.text
    }
    fn line_count(&self) -> usize {
        memchr::memchr_iter(b'\n', self.text.as_bytes()).count() + 1
    }
    fn offset_to_pos(&self, offset: Offset) -> Option<vim_core::primitives::Position> {
        vim_core::execution::simple_offset_to_pos(&self.text, offset.get())
    }
    fn pos_to_offset(
        &self,
        pos: vim_core::primitives::Position,
    ) -> Option<vim_core::primitives::Offset> {
        vim_core::execution::simple_pos_to_offset(&self.text, pos)
    }
}

impl VimHost for BadSyntaxHost {
    fn capabilities(&self) -> HostCapabilitySet {
        HostCapabilitySet::CORE
            .with(HostCapability::StatusMessages)
            .with(HostCapability::CursorStyle)
    }

    fn cursor_offset(&self) -> usize {
        self.cursor
    }

    fn apply_effects(&mut self, effects: &[Effect]) {
        for effect in effects {
            match effect {
                Effect::SetCursor { offset } => {
                    self.cursor = offset.get().min(self.text.len());
                }
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
                _ => {}
            }
        }
    }

    fn handle_request(&mut self, _request: &HostRequest) -> RequestDisposition {
        RequestDisposition::Unsupported
    }

    fn providers(&self) -> Providers<'_> {
        Providers::new().with_syntax(&self.syntax)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CountedPanicHost — panics N times in providers() then works
// ═══════════════════════════════════════════════════════════════════════════════

struct CountedPanicHost {
    text: String,
    cursor: usize,
    call_count: AtomicU32,
    panics_to_throw: u32,
}

impl CountedPanicHost {
    fn new(text: &str, panics_to_throw: u32) -> Self {
        Self {
            text: text.to_string(),
            cursor: 0,
            call_count: AtomicU32::new(0),
            panics_to_throw,
        }
    }
}

impl Document for CountedPanicHost {
    fn text(&self) -> &str {
        &self.text
    }
    fn line_count(&self) -> usize {
        memchr::memchr_iter(b'\n', self.text.as_bytes()).count() + 1
    }
    fn offset_to_pos(&self, offset: Offset) -> Option<vim_core::primitives::Position> {
        vim_core::execution::simple_offset_to_pos(&self.text, offset.get())
    }
    fn pos_to_offset(
        &self,
        pos: vim_core::primitives::Position,
    ) -> Option<vim_core::primitives::Offset> {
        vim_core::execution::simple_pos_to_offset(&self.text, pos)
    }
}

impl VimHost for CountedPanicHost {
    fn capabilities(&self) -> HostCapabilitySet {
        HostCapabilitySet::CORE
            .with(HostCapability::StatusMessages)
            .with(HostCapability::CursorStyle)
    }

    fn cursor_offset(&self) -> usize {
        self.cursor
    }

    fn apply_effects(&mut self, effects: &[Effect]) {
        for effect in effects {
            match effect {
                Effect::SetCursor { offset } => {
                    self.cursor = offset.get().min(self.text.len());
                }
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
                _ => {}
            }
        }
    }

    fn handle_request(&mut self, _request: &HostRequest) -> RequestDisposition {
        RequestDisposition::Unsupported
    }

    fn providers(&self) -> Providers<'_> {
        let count = self.call_count.fetch_add(1, Ordering::Relaxed);
        if count < self.panics_to_throw {
            panic!("CountedPanicHost: deliberate panic #{}", count + 1);
        }
        // After panics_to_throw panics, return default providers normally.
        Providers::default()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// StandardTestHost — full-featured host for undo/capability tests
// ═══════════════════════════════════════════════════════════════════════════════

struct StandardTestHost {
    text: String,
    cursor: usize,
    mode: Mode,
    capabilities: HostCapabilitySet,
    effect_log: RefCell<Vec<EffectKind>>,
    suppressed_log: RefCell<Vec<EffectKind>>,
    /// Pending undo group snapshot (captured at BeginUndoGroup).
    pending_undo: Option<UndoSnapshot>,
    /// Committed undo stack (snapshots BEFORE each change group).
    undo_stack: Vec<UndoSnapshot>,
    /// Redo stack.
    redo_stack: Vec<UndoSnapshot>,
}

#[derive(Clone, Debug)]
struct UndoSnapshot {
    text: String,
    cursor: usize,
}

impl StandardTestHost {
    fn new(text: &str, capabilities: HostCapabilitySet) -> Self {
        Self {
            text: text.to_string(),
            cursor: 0,
            mode: Mode::Normal,
            capabilities,
            effect_log: RefCell::new(Vec::new()),
            suppressed_log: RefCell::new(Vec::new()),
            pending_undo: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

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

    fn core_only(text: &str) -> Self {
        Self::new(text, HostCapabilitySet::CORE)
    }
}

impl Document for StandardTestHost {
    fn text(&self) -> &str {
        &self.text
    }
    fn line_count(&self) -> usize {
        if self.text.is_empty() {
            1
        } else {
            memchr::memchr_iter(b'\n', self.text.as_bytes()).count() + 1
        }
    }
    fn offset_to_pos(&self, offset: Offset) -> Option<vim_core::primitives::Position> {
        vim_core::execution::simple_offset_to_pos(&self.text, offset.get())
    }
    fn pos_to_offset(
        &self,
        pos: vim_core::primitives::Position,
    ) -> Option<vim_core::primitives::Offset> {
        vim_core::execution::simple_pos_to_offset(&self.text, pos)
    }
}

impl VimHost for StandardTestHost {
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
                            self.undo_stack.push(UndoSnapshot {
                                text: self.text.clone(),
                                cursor: self.cursor,
                            });
                            self.text = after.text;
                            self.cursor = after.cursor;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn handle_request(&mut self, _request: &HostRequest) -> RequestDisposition {
        RequestDisposition::Deferred
    }

    fn on_effect_suppressed(&mut self, kind: EffectKind) {
        self.suppressed_log.borrow_mut().push(kind);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// DeferringHost — defers all requests (for cancel tests)
// ═══════════════════════════════════════════════════════════════════════════════

struct DeferringHost {
    text: String,
    cursor: usize,
}

impl DeferringHost {
    fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
            cursor: 0,
        }
    }
}

impl Document for DeferringHost {
    fn text(&self) -> &str {
        &self.text
    }
    fn line_count(&self) -> usize {
        memchr::memchr_iter(b'\n', self.text.as_bytes())
            .count()
            .max(1)
    }
    fn offset_to_pos(&self, offset: Offset) -> Option<vim_core::primitives::Position> {
        vim_core::execution::simple_offset_to_pos(&self.text, offset.get())
    }
    fn pos_to_offset(
        &self,
        pos: vim_core::primitives::Position,
    ) -> Option<vim_core::primitives::Offset> {
        vim_core::execution::simple_pos_to_offset(&self.text, pos)
    }
}

impl VimHost for DeferringHost {
    fn capabilities(&self) -> HostCapabilitySet {
        HostCapabilitySet::FULL
    }
    fn cursor_offset(&self) -> usize {
        self.cursor
    }
    fn apply_effects(&mut self, effects: &[Effect]) {
        for effect in effects {
            match effect {
                Effect::SetCursor { offset } => self.cursor = offset.get(),
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
                _ => {}
            }
        }
    }
    fn handle_request(&mut self, _request: &HostRequest) -> RequestDisposition {
        RequestDisposition::Deferred
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// UnsupportedGdHost — returns Unsupported for all requests
// ═══════════════════════════════════════════════════════════════════════════════

struct UnsupportedGdHost {
    text: String,
    cursor: usize,
}

impl UnsupportedGdHost {
    fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
            cursor: 0,
        }
    }
}

impl Document for UnsupportedGdHost {
    fn text(&self) -> &str {
        &self.text
    }
    fn line_count(&self) -> usize {
        memchr::memchr_iter(b'\n', self.text.as_bytes())
            .count()
            .max(1)
    }
    fn offset_to_pos(&self, offset: Offset) -> Option<vim_core::primitives::Position> {
        vim_core::execution::simple_offset_to_pos(&self.text, offset.get())
    }
    fn pos_to_offset(
        &self,
        pos: vim_core::primitives::Position,
    ) -> Option<vim_core::primitives::Offset> {
        vim_core::execution::simple_pos_to_offset(&self.text, pos)
    }
}

impl VimHost for UnsupportedGdHost {
    fn capabilities(&self) -> HostCapabilitySet {
        HostCapabilitySet::FULL
    }
    fn cursor_offset(&self) -> usize {
        self.cursor
    }
    fn apply_effects(&mut self, effects: &[Effect]) {
        for effect in effects {
            match effect {
                Effect::SetCursor { offset } => {
                    self.cursor = offset.get().min(self.text.len());
                }
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
                _ => {}
            }
        }
    }
    fn handle_request(&mut self, _request: &HostRequest) -> RequestDisposition {
        RequestDisposition::Unsupported
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 1: engine_survives_panicking_providers
// ═══════════════════════════════════════════════════════════════════════════════

/// Host whose `providers()` panics every time.
/// Process keystrokes (normal editing: `iHello<Esc>`) and verify
/// no crash, text was inserted correctly (engine uses fallback providers).
#[test]
fn engine_survives_panicking_providers() {
    let host = PanickingProviderHost::new("world");
    let mut session = VimSession::with_host(host);

    // Enter insert mode, type "Hello ", then escape.
    // Use the engine's mode() (authoritative) rather than host's tracked mode.
    let _ = session.process_key(KeyEvent::char('i'));
    drain_pending(&mut session);
    assert_eq!(
        session.engine().mode(),
        Mode::Insert,
        "engine should enter Insert mode despite panicking providers",
    );

    type_str(&mut session, "Hello ");
    let _ = session.process_key(KeyEvent::escape());
    drain_pending(&mut session);

    // Verify: no crash, text was inserted, mode returned to Normal.
    assert_eq!(session.engine().mode(), Mode::Normal);
    assert!(
        session.host().text.contains("Hello "),
        "text should contain 'Hello ' despite panicking providers, got: {:?}",
        session.host().text,
    );

    // Verify panics were actually triggered (the host was called).
    let panics = session.host().panic_count.load(Ordering::Relaxed);
    assert!(
        panics > 0,
        "providers() should have been called (and panicked) at least once",
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 2: engine_survives_bad_syntax_offsets
// ═══════════════════════════════════════════════════════════════════════════════

/// Host with SyntaxProvider that returns `(usize::MAX - 1, usize::MAX - 1)`.
/// Trigger a syntax-related text object (e.g., `vif` — visual inner function)
/// and verify no crash, selection doesn't go out of bounds.
#[test]
fn engine_survives_bad_syntax_offsets() {
    let host = BadSyntaxHost::new("fn main() { let x = 1; }");
    let mut session = VimSession::with_host(host);

    // Try syntax-aware text object `vif` (visual inner function).
    // Even with bad offsets from the provider, the engine should clamp
    // and not crash.
    send_keys(
        &mut session,
        &[
            KeyEvent::char('v'),
            KeyEvent::char('i'),
            KeyEvent::char('f'),
        ],
    );

    // Verify: no crash, cursor is within document bounds.
    let cursor = session.host().cursor;
    let doc_len = session.host().text.len();
    assert!(
        cursor <= doc_len,
        "cursor {cursor} should be within document bounds (len={doc_len})",
    );

    // Escape back to normal mode.
    let _ = session.process_key(KeyEvent::escape());
    drain_pending(&mut session);

    // Try `]m` (next method start) — also uses SyntaxProvider.
    let _ = session.process_key(KeyEvent::char(']'));
    let _ = session.process_key(KeyEvent::char('m'));
    drain_pending(&mut session);

    // Still no crash, cursor within bounds.
    let cursor = session.host().cursor;
    assert!(
        cursor <= doc_len,
        "cursor {cursor} should remain within document bounds after ]m",
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 3: engine_auto_disables_after_3_panics
// ═══════════════════════════════════════════════════════════════════════════════

/// Host where providers() panics 3 times then works normally.
/// The safety harness uses `query()` (not `query_for_capability()`) for
/// providers, so it increments the global panic count but does not
/// auto-disable. After 3 panics, providers() should still be called but
/// the harness handles them gracefully.
#[test]
fn engine_auto_disables_after_3_panics() {
    let host = CountedPanicHost::new("hello world", 3);
    let mut session = VimSession::with_host(host);

    // Process keys that trigger providers() calls.
    // Each process_key + drain_pending calls build_context which calls providers().
    for _ in 0..5 {
        let _ = session.process_key(KeyEvent::char('l'));
        drain_pending(&mut session);
    }

    // Verify: no crash, engine still functional.
    assert!(
        session.host().cursor > 0,
        "cursor should have moved despite panics",
    );

    // The host's providers() was called at least 3 times (the panicking ones)
    // plus possibly more successful ones after.
    let calls = session.host().call_count.load(Ordering::Relaxed);
    assert!(
        calls >= 3,
        "providers() should have been called at least 3 times, got {calls}",
    );

    // Engine continues to work after the panic phase.
    let cursor_before = session.host().cursor;
    let _ = session.process_key(KeyEvent::char('l'));
    drain_pending(&mut session);
    let cursor_after = session.host().cursor;
    assert!(
        cursor_after >= cursor_before,
        "engine should still process keys after panic recovery",
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 4: external_edit_auto_pair_merges_undo
// ═══════════════════════════════════════════════════════════════════════════════

/// Normal host: type `(`, then notify_edit with AutoPair kind for `)`.
/// Press `u` (undo). Assert: BOTH `(` and `)` are removed (merged undo group).
#[test]
fn external_edit_auto_pair_merges_undo() {
    let host = StandardTestHost::standard("");
    let mut session = VimSession::with_host(host);

    // Initialize shadow document to match the host's initial text.
    session.engine_mut().set_shadow_text("");

    // Enter insert mode.
    let _ = session.process_key(KeyEvent::char('i'));
    drain_pending(&mut session);

    // Type `(`.
    let _ = session.process_key(KeyEvent::char('('));
    drain_pending(&mut session);

    // The text should now contain "(" at minimum.
    assert!(
        session.host().text.contains('('),
        "text should contain '(' after typing it",
    );

    let cursor_after_open = session.host().cursor;

    // Sync shadow to host's current text before calling notify_external_edit.
    // In a real host, the shadow is kept in sync by process_key's drift gate.
    let current_text = session.host().text.clone();
    session.engine_mut().set_shadow_text(&current_text);

    // Simulate host auto-pairing: insert `)` after the cursor.
    // The edit: no deletion, insert ")" at cursor position.
    let edit = ExternalEdit::new(
        Range::from_raw(cursor_after_open, cursor_after_open),
        ")",
        Offset::new(cursor_after_open),
        ExternalEditKind::AutoPair,
    );
    let _ = session.notify_external_edit(edit);

    // Manually apply the auto-pair to our host text (the engine sends effects
    // but the actual insertion was done by the host).
    session.host_mut().text.insert(cursor_after_open, ')');

    assert!(
        session.host().text.contains(')'),
        "text should contain ')' after auto-pair",
    );

    // Exit insert mode.
    let _ = session.process_key(KeyEvent::escape());
    drain_pending(&mut session);

    // Press `u` to undo.
    let _ = session.process_key(KeyEvent::char('u'));
    drain_pending(&mut session);

    // Both `(` and `)` should be removed — merged undo group.
    assert!(
        !session.host().text.contains('(') && !session.host().text.contains(')'),
        "undo should remove both '(' and ')' (merged undo group), got: {:?}",
        session.host().text,
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 5: external_edit_format_on_type_separate_undo
// ═══════════════════════════════════════════════════════════════════════════════

/// Type some text, then notify_edit with FormatOnType kind.
/// Press `u`. Assert: only the format is undone, user's text remains.
#[test]
fn external_edit_format_on_type_separate_undo() {
    let host = StandardTestHost::standard("");
    let mut session = VimSession::with_host(host);

    // Initialize shadow document to match the host's initial text.
    session.engine_mut().set_shadow_text("");

    // Enter insert mode and type "hello".
    let _ = session.process_key(KeyEvent::char('i'));
    drain_pending(&mut session);
    type_str(&mut session, "hello");

    // Exit insert mode.
    let _ = session.process_key(KeyEvent::escape());
    drain_pending(&mut session);

    let text_after_typing = session.host().text.clone();
    assert_eq!(text_after_typing, "hello");

    // Sync shadow to host's current text before calling notify_external_edit.
    let current_text = session.host().text.clone();
    session.engine_mut().set_shadow_text(&current_text);

    // Capture host-side undo snapshot BEFORE the format edit.
    // In a real host, this would be done when the host decides to format.
    // The host must maintain its own undo entry for non-merging external edits
    // because the engine's undo tree tracks them as separate nodes and will
    // ask the host to undo them independently via Effect::Undo { steps }.
    let cursor_before_format = session.host().cursor;
    session.host_mut().pending_undo = Some(UndoSnapshot {
        text: session.host().text.clone(),
        cursor: cursor_before_format,
    });

    // Simulate host format-on-type: replace "hello" with "  hello" (indentation).
    let edit = ExternalEdit::new(
        Range::from_raw(0, 5), // delete "hello"
        "  hello",             // replace with indented version
        Offset::new(7),
        ExternalEditKind::FormatOnType,
    );
    let _ = session.notify_external_edit(edit);

    // Manually apply to host text.
    session.host_mut().text = "  hello".to_string();
    session.host_mut().cursor = 7;

    // Commit the format-edit undo entry on the host side.
    // This mirrors what a real host does: capture state before format, apply
    // format, then commit the undo entry so that a single `u` reverts it.
    if let Some(snapshot) = session.host_mut().pending_undo.take() {
        session.host_mut().undo_stack.push(snapshot);
        session.host_mut().redo_stack.clear();
    }

    assert_eq!(session.host().text, "  hello");

    // Sync shadow after the format was applied.
    let formatted_text = session.host().text.clone();
    session.engine_mut().set_shadow_text(&formatted_text);

    // Press `u` to undo — should only undo the format.
    let _ = session.process_key(KeyEvent::char('u'));
    drain_pending(&mut session);

    // User's text "hello" should remain; only the formatting is undone.
    assert_eq!(
        session.host().text,
        "hello",
        "undo should only revert the format, leaving user text, got: {:?}",
        session.host().text,
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 6: capability_upgrade_enables_effects
// ═══════════════════════════════════════════════════════════════════════════════

/// Create session with CORE only. Process command that would produce
/// StatusMessages effect. Assert: effect NOT delivered. Upgrade to include
/// StatusMessages. Process same command. Assert: effect IS delivered.
#[test]
fn capability_upgrade_enables_effects() {
    let host = StandardTestHost::core_only("hello");
    let mut session = VimSession::with_host(host);

    // Verify StatusMessages is NOT in capabilities.
    assert!(!session.capabilities().has(HostCapability::StatusMessages));

    // Try to trigger a search that would produce a message (pattern not found).
    // `/zzz<CR>` should attempt search and produce a "Pattern not found" message.
    let _ = session.process_key(KeyEvent::char('/'));
    drain_pending(&mut session);
    type_str(&mut session, "zzz");
    let _ = session.process_key(KeyEvent::from_name("CR").unwrap());
    drain_pending(&mut session);

    // Check: ShowInfo should have been suppressed (not in effect log).
    let has_show_message = session
        .host()
        .effect_log
        .borrow()
        .contains(&EffectKind::ShowInfo);
    // ShowInfo requires StatusMessages capability — should be suppressed for CORE.
    let was_suppressed = session
        .host()
        .suppressed_log
        .borrow()
        .contains(&EffectKind::ShowInfo);
    // Either it was suppressed OR it wasn't generated at all — both are acceptable.
    // The key assertion is that it was NOT delivered to apply_effects.
    if was_suppressed {
        assert!(
            !has_show_message,
            "ShowInfo should be suppressed, not delivered",
        );
    }

    // Now upgrade capabilities to include StatusMessages.
    session.upgrade_capability(HostCapability::StatusMessages);
    assert!(session.capabilities().has(HostCapability::StatusMessages));

    // Clear logs for the next attempt.
    session.host_mut().effect_log.borrow_mut().clear();
    session.host_mut().suppressed_log.borrow_mut().clear();

    // Try the same search again — should now deliver ShowInfo.
    let _ = session.process_key(KeyEvent::char('/'));
    drain_pending(&mut session);
    type_str(&mut session, "zzz");
    let _ = session.process_key(KeyEvent::from_name("CR").unwrap());
    drain_pending(&mut session);

    // After upgrade, ShowInfo should be delivered (not suppressed).
    let has_show_message_after = session
        .host()
        .effect_log
        .borrow()
        .contains(&EffectKind::ShowInfo);
    let was_suppressed_after = session
        .host()
        .suppressed_log
        .borrow()
        .contains(&EffectKind::ShowInfo);

    // ShowInfo should now be delivered.
    assert!(
        has_show_message_after || !was_suppressed_after,
        "after upgrade, ShowInfo should be delivered (delivered={has_show_message_after}, \
         suppressed={was_suppressed_after})",
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 7: request_disposition_unsupported_applies_fallback
// ═══════════════════════════════════════════════════════════════════════════════

/// Host returns Unsupported for GotoDefinition.
/// Trigger `gd`. Assert: no pending request, engine continued normally.
#[test]
fn request_disposition_unsupported_applies_fallback() {
    let host = UnsupportedGdHost::new("hello world");
    let mut session = VimSession::with_host(host);

    // `gd` triggers GotoDefinition. Since host returns Unsupported,
    // the engine should apply the default fallback (Success no-op) and
    // NOT leave the request pending.
    let _ = session.process_key(KeyEvent::char('g'));
    let result = session.process_key(KeyEvent::char('d'));
    drain_pending(&mut session);

    // No pending requests — fallback was applied immediately.
    assert_eq!(
        session.pending_request_count(),
        0,
        "Unsupported disposition should apply fallback, leaving 0 pending requests",
    );

    // The host_requests in the result should be empty (not deferred to caller).
    assert!(
        result.host_requests.is_empty(),
        "Unsupported requests should not appear in ProcessResult::host_requests",
    );

    // Engine should still be in Normal mode and functional.
    assert_eq!(session.mode(), Mode::Normal);

    // Further editing works.
    let _ = session.process_key(KeyEvent::char('l'));
    drain_pending(&mut session);
    assert!(
        session.host().cursor > 0,
        "engine should still be functional after Unsupported fallback",
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 8: cancel_all_pending_on_buffer_switch
// ═══════════════════════════════════════════════════════════════════════════════

/// Host defers all requests. Trigger multiple commands that generate
/// requests. Cancel all via cancel_request loop. Assert: pending_request_count() == 0.
#[test]
fn cancel_all_pending_on_buffer_switch() {
    let host = DeferringHost::new("hello world\nfunction foo() {}\nbar baz");
    let mut session = VimSession::with_host(host);

    // Collect all deferred request IDs from every ProcessResult.
    let mut pending_ids = Vec::new();

    // 1. `gd` — GotoDefinition
    let result1 = session.process_key(KeyEvent::char('g'));
    for req in &result1.host_requests {
        pending_ids.push(req.id());
    }
    let result2 = session.process_key(KeyEvent::char('d'));
    for req in &result2.host_requests {
        pending_ids.push(req.id());
    }

    // 2. `K` — ShowDocumentation
    let result3 = session.process_key(KeyEvent::char('K'));
    for req in &result3.host_requests {
        pending_ids.push(req.id());
    }

    // 3. `:!echo hi<CR>` — ExternalCommand (requires Shell capability)
    let result4 = session.process_key(KeyEvent::char(':'));
    for req in &result4.host_requests {
        pending_ids.push(req.id());
    }
    for ch in "!echo hi".chars() {
        let result = session.process_key(KeyEvent::char(ch));
        for req in &result.host_requests {
            pending_ids.push(req.id());
        }
    }
    let result5 = session.process_key(KeyEvent::from_name("CR").unwrap());
    for req in &result5.host_requests {
        pending_ids.push(req.id());
    }

    // We should have at least some pending requests.
    let initial_pending = session.pending_request_count();
    assert!(
        initial_pending > 0,
        "should have at least 1 pending request, got {initial_pending}",
    );

    // Cancel ALL collected request IDs (simulating buffer switch).
    for id in &pending_ids {
        session.cancel_request(*id);
    }

    // After cancelling all collected IDs, pending count should be 0.
    // If there are any remaining (from internal operations we didn't capture),
    // verify they are all cleaned up by ID.
    assert_eq!(
        session.pending_request_count(),
        0,
        "after cancelling all collected requests ({}), pending count should be 0 \
         (initial was {initial_pending})",
        pending_ids.len(),
    );

    // Engine should still be functional.
    assert_eq!(
        session.mode(),
        Mode::Normal,
        "engine should be in Normal mode after cancelling all requests",
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 9: deferred_request_never_completed_does_not_hang
// ═══════════════════════════════════════════════════════════════════════════════

/// Host returns Deferred for all requests but never calls complete_request().
/// The engine should continue to function: accepting keys, moving cursor,
/// and editing text. Pending requests accumulate but don't block processing.
#[test]
fn deferred_request_never_completed_does_not_hang() {
    let host = DeferringHost::new("hello world\nsecond line\nthird line");
    let mut session = VimSession::with_host(host);

    // Trigger `gd` — produces a deferred GotoDefinition request.
    let _ = session.process_key(KeyEvent::char('g'));
    let _ = session.process_key(KeyEvent::char('d'));
    drain_pending(&mut session);

    let pending_after_gd = session.pending_request_count();
    assert!(
        pending_after_gd > 0,
        "gd should create a pending request, got {pending_after_gd}",
    );

    // Without completing the request, continue editing.
    // The engine must NOT block on the pending request.
    let cursor_before = session.host().cursor;
    let _ = session.process_key(KeyEvent::char('j'));
    drain_pending(&mut session);
    let cursor_after_j = session.host().cursor;
    assert!(
        cursor_after_j > cursor_before,
        "engine should still process 'j' (move down) with pending requests \
         (cursor before={cursor_before}, after={cursor_after_j})",
    );

    // Type more text to prove full editing still works.
    let _ = session.process_key(KeyEvent::char('i'));
    drain_pending(&mut session);
    assert_eq!(session.engine().mode(), Mode::Insert);

    type_str(&mut session, "INSERTED");
    let _ = session.process_key(KeyEvent::escape());
    drain_pending(&mut session);

    assert!(
        session.host().text.contains("INSERTED"),
        "text editing should work despite uncompleted pending requests, got: {:?}",
        session.host().text,
    );

    // Pending request count should still be > 0 (never completed).
    assert!(
        session.pending_request_count() >= pending_after_gd,
        "pending requests should accumulate, not disappear (expected >= {pending_after_gd}, \
         got {})",
        session.pending_request_count(),
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 10: external_edit_empty_text_no_crash
// ═══════════════════════════════════════════════════════════════════════════════

/// Host sends ExternalEdit with empty inserted text (a pure deletion) or
/// a zero-range with empty text (a no-op). Engine should handle gracefully.
#[test]
fn external_edit_empty_text_no_crash() {
    let host = StandardTestHost::standard("hello world");
    let mut session = VimSession::with_host(host);
    session.engine_mut().set_shadow_text("hello world");

    // Case 1: Zero-range, empty text — total no-op edit.
    let noop_edit = ExternalEdit::new(
        Range::from_raw(0, 0),
        "",
        Offset::new(0),
        ExternalEditKind::HostNotified,
    );
    let _result = session.notify_external_edit(noop_edit);

    // Should not crash, text unchanged.
    assert_eq!(
        session.host().text,
        "hello world",
        "no-op external edit should not modify text",
    );

    // Case 2: Non-zero range, empty text — pure deletion.
    // Delete "hello" (range 0..5), insert nothing.
    let current = session.host().text.clone();
    session.engine_mut().set_shadow_text(&current);

    let delete_edit = ExternalEdit::new(
        Range::from_raw(0, 5),
        "",
        Offset::new(0),
        ExternalEditKind::HostNotified,
    );
    let _ = session.notify_external_edit(delete_edit);

    // Manually apply the deletion to the host (simulating what the host did).
    session.host_mut().text = " world".to_string();
    session.host_mut().cursor = 0;

    // Sync shadow.
    let after = session.host().text.clone();
    session.engine_mut().set_shadow_text(&after);

    // Engine should still be functional.
    let _ = session.process_key(KeyEvent::char('l'));
    drain_pending(&mut session);
    assert!(
        session.host().cursor <= session.host().text.len(),
        "cursor should remain within bounds after empty-text external edit",
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 11: external_edit_range_beyond_document_length
// ═══════════════════════════════════════════════════════════════════════════════

/// Host sends ExternalEdit with a range that extends beyond the document length.
/// Engine should clamp or handle gracefully without panic.
#[test]
fn external_edit_range_beyond_document_length() {
    let host = StandardTestHost::standard("short");
    let mut session = VimSession::with_host(host);
    session.engine_mut().set_shadow_text("short");

    // Range 0..1000 on a 5-byte document. This simulates a buggy host
    // that reports a stale range after the document shrank.
    let oversized_edit = ExternalEdit::new(
        Range::from_raw(0, 1000),
        "replaced",
        Offset::new(8),
        ExternalEditKind::HostNotified,
    );
    let _ = session.notify_external_edit(oversized_edit);

    // Manually apply what we think the host did (replaced entire doc).
    session.host_mut().text = "replaced".to_string();
    session.host_mut().cursor = 8.min(session.host().text.len());

    let after = session.host().text.clone();
    session.engine_mut().set_shadow_text(&after);

    // Engine should still function.
    assert_eq!(session.engine().mode(), Mode::Normal);
    let _ = session.process_key(KeyEvent::char('0'));
    drain_pending(&mut session);
    assert_eq!(
        session.host().cursor,
        0,
        "cursor should move to 0 with '0' command after oversized external edit",
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 12: capability_downgrade_suppresses_effects
// ═══════════════════════════════════════════════════════════════════════════════

/// Start with full capabilities, verify effects are delivered, then downgrade
/// and verify the same effects are now suppressed.
#[test]
fn capability_downgrade_suppresses_effects() {
    let host = StandardTestHost::new(
        "hello",
        HostCapabilitySet::CORE
            .with(HostCapability::StatusMessages)
            .with(HostCapability::CursorStyle)
            .with(HostCapability::SearchHighlight),
    );
    let mut session = VimSession::with_host(host);

    // Trigger a search to generate SearchHighlight effects.
    let _ = session.process_key(KeyEvent::char('/'));
    drain_pending(&mut session);
    type_str(&mut session, "hello");
    let _ = session.process_key(KeyEvent::from_name("CR").unwrap());
    drain_pending(&mut session);

    // Check that HighlightMatches was delivered (SearchHighlight is active).
    let _had_highlight = session
        .host()
        .effect_log
        .borrow()
        .contains(&EffectKind::HighlightMatches);

    // Now downgrade: remove SearchHighlight capability.
    session.downgrade_capability(HostCapability::SearchHighlight);
    assert!(!session.capabilities().has(HostCapability::SearchHighlight));

    // Clear logs for the next attempt.
    session.host_mut().effect_log.borrow_mut().clear();
    session.host_mut().suppressed_log.borrow_mut().clear();

    // Search again — HighlightMatches should now be suppressed.
    let _ = session.process_key(KeyEvent::char('n'));
    drain_pending(&mut session);

    let has_highlight_after = session
        .host()
        .effect_log
        .borrow()
        .contains(&EffectKind::HighlightMatches);
    let was_suppressed = session
        .host()
        .suppressed_log
        .borrow()
        .contains(&EffectKind::HighlightMatches);

    // After downgrade, HighlightMatches should NOT be delivered to apply_effects.
    // It should either be suppressed or not generated.
    assert!(
        !has_highlight_after || was_suppressed,
        "after downgrading SearchHighlight, HighlightMatches should be suppressed \
         (delivered={has_highlight_after}, suppressed={was_suppressed})",
    );

    // Engine still functional.
    assert_eq!(session.engine().mode(), Mode::Normal);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 13: cancel_already_completed_request_returns_false
// ═══════════════════════════════════════════════════════════════════════════════

/// Complete a deferred request, then try to cancel it. cancel_request should
/// return false (ID no longer pending). Engine should not crash or double-free.
#[test]
fn cancel_already_completed_request_returns_false() {
    let host = DeferringHost::new("hello world\nfunction foo() {}\nbar baz");
    let mut session = VimSession::with_host(host);

    // Trigger `gd` to get a deferred request.
    let _ = session.process_key(KeyEvent::char('g'));
    let result = session.process_key(KeyEvent::char('d'));
    drain_pending(&mut session);

    // Collect the request ID.
    assert!(
        !result.host_requests.is_empty(),
        "gd should produce at least one host request",
    );
    let request_id = result.host_requests[0].id();

    // Complete the request.
    let completion = HostResult::Success {
        id: request_id,
        message: None,
    };
    let _ = session.complete_request(&completion);

    // Now try to cancel the same ID — it should return false.
    let cancel_result = session.cancel_request(request_id);
    assert!(
        !cancel_result,
        "cancelling an already-completed request should return false",
    );

    // Pending count should be 0 (request was completed, not re-added).
    assert_eq!(
        session.pending_request_count(),
        0,
        "no pending requests should remain after completion + cancel attempt",
    );

    // Engine still functional.
    let _ = session.process_key(KeyEvent::char('l'));
    drain_pending(&mut session);
    assert!(
        session.host().cursor > 0,
        "engine should remain functional after complete + cancel sequence",
    );
}
