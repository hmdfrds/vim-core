//! TestSession — the core testing abstraction.
//!
//! Wraps `HostSession` with automatic invariant checking and effect capture.

use std::collections::HashMap;

use vim_core::effects::{validate_ordering, Effect, EffectKind, EffectTier};
use vim_core::execution::HostSession;
use vim_core::primitives::{Mode, MotionType, Offset, RegisterName, SearchDirection, VisualType};

use crate::effects::EffectLog;
use crate::golden::{GoldenState, RegisterSnapshot, WindowState};
use vim_core::test_utils::annotated_text::{
    annotate, annotate_multi, parse, parse_multi, CursorSpec, MultiCursorSpec,
};

use crate::keys::parse_keys;

/// Test session wrapping `HostSession` with invariant checks and effect capture.
///
/// After every keystroke, runs 5 invariant checks (cursor bounds, char boundary,
/// undo depth, effect ordering, internal leak). On drop, runs per-test checks
/// (cursor not on `\n`, mode/selection consistency).
pub struct TestSession {
    inner: HostSession,
    effect_log: EffectLog,
    last_effects_cache: Vec<Effect>,
    check_invariants: bool,
}

impl TestSession {
    /// Create a session from single-cursor annotated text.
    pub fn new(annotated: &str) -> Self {
        let (text, spec) = parse(annotated);
        let offset = spec.offset().get();
        let mut session = HostSession::new(&text).with_auto_handle_defaults(true);
        session.set_cursor_offset(offset);
        session.set_shadow_execution(true);
        Self {
            inner: session,
            effect_log: EffectLog::new(),
            last_effects_cache: Vec::new(),
            check_invariants: true,
        }
    }

    /// Create a session from multi-cursor annotated text.
    pub fn new_multi(annotated: &str) -> Self {
        let (text, spec) = parse_multi(annotated);
        let mut session = HostSession::new(&text).with_auto_handle_defaults(true);

        let primary = spec.primary();
        session.set_cursor_offset(primary.offset().get());

        for (num, cursor_spec) in spec.iter() {
            if num > 1 {
                session
                    .add_cursor(cursor_spec.offset().get())
                    .unwrap_or_else(|e| {
                        panic!(
                            "failed to add cursor {num} at offset {}: {e}",
                            cursor_spec.offset().get()
                        );
                    });
            }
        }

        session.set_shadow_execution(true);
        Self {
            inner: session,
            effect_log: EffectLog::new(),
            last_effects_cache: Vec::new(),
            check_invariants: true,
        }
    }

    /// Create from an already-configured `HostSession`.
    pub fn from_host_session(mut session: HostSession) -> Self {
        session.set_shadow_execution(true);
        Self {
            inner: session,
            effect_log: EffectLog::new(),
            last_effects_cache: Vec::new(),
            check_invariants: true,
        }
    }

    /// Feed a Vim notation key sequence through the engine.
    ///
    /// After each keystroke, captures effects and runs invariant checks.
    pub fn feed(&mut self, keys: &str) -> &mut Self {
        for key in parse_keys(keys) {
            let key_repr = format!("{key:?}");
            let _response = self.inner.process_key_host(key);
            let effects = self.inner.take_captured_effects();

            if self.check_invariants {
                run_per_key_invariants(&self.inner, &effects, &key_repr);
            }

            self.last_effects_cache = effects.clone();
            self.effect_log.record_step(effects);
        }
        self
    }

    /// Current document text.
    pub fn text(&self) -> &str {
        self.inner.text()
    }

    /// Primary cursor byte offset.
    pub fn cursor_offset(&self) -> usize {
        self.inner.cursor_offset()
    }

    /// Current mode.
    pub fn mode(&self) -> Mode {
        self.inner.mode()
    }

    /// Number of active cursors.
    pub fn cursor_count(&self) -> usize {
        self.inner.cursor_count()
    }

    /// All cursor positions as `(line, col, offset)`.
    pub fn cursor_positions(&self) -> Vec<(usize, usize, usize)> {
        self.inner.cursor_positions()
    }

    /// Current text with single-cursor annotation.
    pub fn annotated(&self) -> String {
        annotate(
            self.text(),
            &CursorSpec::Cursor(Offset::new(self.cursor_offset())),
        )
    }

    /// Current text with multi-cursor annotation.
    pub fn annotated_multi(&self) -> String {
        let positions = self.cursor_positions();
        let mut pairs: Vec<(u32, CursorSpec)> = Vec::new();
        for (i, &(_line, _col, offset)) in positions.iter().enumerate() {
            pairs.push((i as u32 + 1, CursorSpec::Cursor(Offset::new(offset))));
        }
        let spec = MultiCursorSpec::from_pairs(pairs);
        annotate_multi(self.text(), &spec)
    }

    /// Primary cursor position as `(line, col)`, 0-indexed.
    pub fn cursor_line_col(&self) -> (usize, usize) {
        let text = self.text();
        let offset = self.cursor_offset().min(text.len());
        let before = &text.as_bytes()[..offset];
        let line = before.iter().filter(|&&b| b == b'\n').count();
        let col = match before.iter().rposition(|&b| b == b'\n') {
            Some(nl) => offset - nl - 1,
            None => offset,
        };
        (line, col)
    }

    /// Get register contents.
    pub fn register(&self, name: char) -> Option<(String, MotionType)> {
        self.inner.get_register(name)
    }

    /// Get mark position.
    pub fn mark(&self, name: char) -> Option<usize> {
        self.inner.get_mark(name)
    }

    /// Current search pattern.
    pub fn search_pattern(&self) -> Option<&str> {
        self.inner.search_pattern()
    }

    /// Current search direction.
    pub fn search_direction(&self) -> SearchDirection {
        self.inner.search_direction()
    }

    /// Number of committed undo groups (stable across undo/redo).
    pub fn change_count(&self) -> usize {
        self.inner.engine().state().undo_tree().change_count()
    }

    /// Whether undo is available.
    pub fn can_undo(&self) -> bool {
        self.inner.engine().state().undo_tree().can_undo()
    }

    /// Whether redo is available.
    pub fn can_redo(&self) -> bool {
        self.inner.engine().state().undo_tree().can_redo()
    }

    /// Whether an undo group is currently open (pending).
    pub fn has_pending_group(&self) -> bool {
        self.inner.engine().state().undo_tree().has_pending_group()
    }

    /// Number of times the undo store used the checkpoint fallback path.
    /// A non-zero value during normal undo operations indicates that the
    /// changeset path failed (likely due to undo group structural bug).
    pub fn checkpoint_fallback_count(&self) -> u32 {
        self.inner.undo_store_checkpoint_fallback_count()
    }

    /// The effect log — all captured effects grouped by keystroke.
    pub fn effects(&self) -> &EffectLog {
        &self.effect_log
    }

    /// Effects from the most recent keystroke.
    pub fn last_effects(&self) -> &[Effect] {
        &self.last_effects_cache
    }

    /// All effects grouped by keystroke.
    pub fn all_effects(&self) -> &[Vec<Effect>] {
        self.effect_log.steps()
    }

    /// All effects flattened into a single list.
    pub fn all_effects_flat(&self) -> Vec<&Effect> {
        self.effect_log.all_flat()
    }

    /// Select all occurrences of the word under cursor (Ctrl+D select all).
    pub fn select_all_occurrences(&mut self) -> &mut Self {
        self.inner
            .select_all_occurrences()
            .unwrap_or_else(|e| panic!("select_all_occurrences failed: {e}"));
        self
    }

    /// Add next match of word under cursor (Ctrl+D single).
    pub fn add_next_match(&mut self) -> &mut Self {
        self.inner
            .add_next_match()
            .unwrap_or_else(|e| panic!("add_next_match failed: {e}"));
        self
    }

    /// Access the underlying `HostSession`.
    pub fn session(&self) -> &HostSession {
        &self.inner
    }

    /// Mutable access to the underlying `HostSession`.
    pub fn session_mut(&mut self) -> &mut HostSession {
        &mut self.inner
    }

    /// Disable invariant checking (for tests that intentionally violate invariants).
    pub fn disable_invariants(&mut self) -> &mut Self {
        self.check_invariants = false;
        self
    }

    /// Disable shadow execution (for tests where it interferes).
    pub fn disable_shadow_execution(&mut self) -> &mut Self {
        self.inner.set_shadow_execution(false);
        self
    }

    // ═══════════════════════════════════════════════════════════════════════
    // FIDELITY STATE ACCESSORS
    // ═══════════════════════════════════════════════════════════════════════

    /// Virtual column for j/k movement (curswant).
    ///
    /// Returns the sticky column if set, otherwise computes from cursor position.
    pub fn curswant(&self) -> usize {
        self.inner
            .engine()
            .state()
            .sticky_column()
            .map(|vc| vc.get())
            .unwrap_or_else(|| {
                vim_core::commands::helpers::curswant_of(self.text(), self.cursor_offset(), 4)
            })
    }

    /// Jump list entries as byte offsets + current position.
    ///
    /// Returns `None` if the jump list is empty.
    pub fn jumplist(&self) -> Option<(Vec<usize>, usize)> {
        let jl = self.inner.engine().state().jump_list();
        if jl.is_empty() {
            return None;
        }
        let offsets = jl.entries().iter().map(|e| e.offset().get()).collect();
        Some((offsets, jl.position()))
    }

    /// Change list entries as byte offsets + current position.
    ///
    /// Returns `None` if the change list is empty.
    pub fn changelist(&self) -> Option<(Vec<usize>, usize)> {
        let cl = self.inner.engine().state().changelist();
        if cl.is_empty() {
            return None;
        }
        let offsets = cl.entries().iter().map(|e| e[0].get()).collect();
        Some((offsets, cl.position()))
    }

    /// Current error message (equivalent to Neovim's `v:errmsg`).
    pub fn errmsg(&self) -> Option<String> {
        self.inner.errmsg()
    }

    /// Window state — always `None` (viewport is external to TestSession).
    ///
    /// FidelitySession supplies window state externally via `capture_golden()`.
    pub fn window_state(&self) -> Option<WindowState> {
        None
    }

    // ═══════════════════════════════════════════════════════════════════════
    // GOLDEN STATE CAPTURE
    // ═══════════════════════════════════════════════════════════════════════

    /// Capture complete state as a `GoldenState` for fidelity comparison.
    ///
    /// `window` and `errmsg` are supplied externally because TestSession
    /// cannot track them across multi-key sequences:
    /// - Window/viewport is external to TestSession (host manages it).
    /// - `HostSession::errmsg()` is cleared per-key, but Neovim's `v:errmsg`
    ///   persists until overwritten. FidelitySession tracks errmsg from
    ///   `ShowError` effects across the full key sequence.
    pub fn capture_golden(
        &self,
        window: Option<WindowState>,
        errmsg: Option<String>,
    ) -> GoldenState {
        let (cursor_line, cursor_col) = self.cursor_line_col();
        let jl = self.jumplist();
        let cl = self.changelist();

        GoldenState {
            text: self.text().to_string(),
            cursor_offset: self.cursor_offset(),
            cursor_line,
            cursor_col,
            mode: mode_string(self.mode()),
            visual_type: None,
            selection_anchor: selection_anchor(&self.inner),
            registers: capture_registers(self.inner.engine()),
            marks: capture_marks(self.inner.engine()),
            search_pattern: self.search_pattern().map(|s| s.to_string()),
            search_direction: match self.search_direction() {
                SearchDirection::Forward => Some("Forward".to_string()),
                SearchDirection::Backward => Some("Backward".to_string()),
            },
            window,
            curswant: Some(self.curswant()),
            jumplist: jl.as_ref().map(|(offsets, _)| offsets.clone()),
            jumplist_idx: jl.map(|(_, idx)| idx),
            changelist: cl.as_ref().map(|(offsets, _)| offsets.clone()),
            changelist_idx: cl.map(|(_, idx)| idx),
            errmsg,
        }
    }

    /// Run per-test invariant checks explicitly.
    pub fn finalize(&self) {
        if self.check_invariants {
            run_per_test_invariants(&self.inner);
        }
    }
}

impl Drop for TestSession {
    fn drop(&mut self) {
        if self.check_invariants && !std::thread::panicking() {
            run_per_test_invariants(&self.inner);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// GOLDEN STATE HELPERS
// ═══════════════════════════════════════════════════════════════════════════

fn mode_string(mode: Mode) -> String {
    match mode {
        Mode::Normal => "Normal".to_string(),
        Mode::Insert => "Insert".to_string(),
        Mode::Visual(VisualType::Char) => "Visual".to_string(),
        Mode::Visual(VisualType::Line) => "VisualLine".to_string(),
        Mode::Visual(VisualType::Block) => "VisualBlock".to_string(),
        Mode::Replace => "Replace".to_string(),
        Mode::CommandLine => "CommandLine".to_string(),
        Mode::OperatorPending(_) => "OperatorPending".to_string(),
        Mode::Select(VisualType::Char) => "SelectChar".to_string(),
        Mode::Select(VisualType::Line) => "SelectLine".to_string(),
        Mode::Select(VisualType::Block) => "SelectBlock".to_string(),
        Mode::VirtualReplace => "VirtualReplace".to_string(),
        m => format!("{m:?}").to_lowercase(),
    }
}

fn selection_anchor(session: &HostSession) -> Option<usize> {
    session.selection_raw().map(|(anchor, _)| anchor)
}

fn capture_registers(engine: &vim_core::execution::VimEngine) -> HashMap<String, RegisterSnapshot> {
    let mut registers = HashMap::new();

    let register_names = [
        '"', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '-', '/', 'a', 'b', 'c', 'd', 'e',
        'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w',
        'x', 'y', 'z',
    ];
    for &name in &register_names {
        let reg_name = RegisterName::new(name).unwrap();
        if let Some(content) = engine.state().registers().get(reg_name) {
            if !content.text().is_empty() {
                let regtype = match content.motion_type() {
                    MotionType::LineWise => "V".to_string(),
                    MotionType::BlockWise => {
                        let ts = 4usize;
                        let width = content
                            .text()
                            .lines()
                            .map(|l| {
                                l.chars()
                                    .map(|c| if c == '\t' { ts } else { 1 })
                                    .sum::<usize>()
                            })
                            .max()
                            .unwrap_or(0);
                        format!("\x16{width}")
                    }
                    _ => "v".to_string(),
                };
                registers.insert(
                    name.to_string(),
                    RegisterSnapshot {
                        text: content.text().to_string(),
                        regtype,
                    },
                );
            }
        }
    }

    // The engine stores the search pattern in search state, not in the '/'
    // register. Populate '/' from search state if not already present.
    if !registers.contains_key("/") {
        if let Some(pattern) = engine.state().search().pattern() {
            if !pattern.is_empty() {
                registers.insert(
                    "/".to_string(),
                    RegisterSnapshot {
                        text: pattern.to_string(),
                        regtype: "v".to_string(),
                    },
                );
            }
        }
    }

    registers
}

fn capture_marks(engine: &vim_core::execution::VimEngine) -> HashMap<String, usize> {
    use vim_core::primitives::MarkName;

    let mut marks_map = HashMap::new();
    let state = engine.state();

    for c in 'a'..='z' {
        if let Some(mn) = MarkName::new(c) {
            if let Some(mark) = state.marks().get(mn) {
                marks_map.insert(c.to_string(), mark.offset().get());
            }
        }
    }

    for c in ['[', ']', '<', '>', '.', '^'] {
        if let Some(mn) = MarkName::new(c) {
            if let Some(mark) = state.marks().get(mn) {
                marks_map.insert(c.to_string(), mark.offset().get());
            }
        }
    }

    marks_map
}

// ═══════════════════════════════════════════════════════════════════════════
// INVARIANT CHECKS
// ═══════════════════════════════════════════════════════════════════════════

/// Per-keystroke invariant checks. Called by `TestSession::feed()` and
/// `FidelitySession` after each key.
pub fn run_per_key_invariants(session: &HostSession, effects: &[Effect], key_repr: &str) {
    let text = session.text();
    let cursor = session.cursor_offset();
    let mode = session.mode();

    // 1. Undo group depth — skip during macro merge
    if !session.engine().state().undo_tree().is_merging() {
        check_undo_depth(effects, key_repr);
    }

    // 2. Cursor within bounds
    let doc_len = text.len();
    let allows_past_end = matches!(
        mode,
        Mode::Insert | Mode::Replace | Mode::VirtualReplace | Mode::Visual(_)
    );
    let on_trailing_empty_line =
        cursor == doc_len && doc_len > 0 && text.as_bytes()[doc_len - 1] == b'\n';
    if doc_len == 0 {
        assert_eq!(
            cursor, 0,
            "INVARIANT: cursor={cursor} but document is empty (after key {key_repr:?})"
        );
    } else if cursor > doc_len {
        panic!(
            "INVARIANT: cursor={cursor} > doc_len={doc_len} \
             (after key {key_repr:?}, mode: {mode:?})"
        );
    } else if cursor == doc_len && !allows_past_end && !on_trailing_empty_line {
        eprintln!(
            "INVARIANT[cursor-bounds] note: cursor={cursor} == doc_len={doc_len} in {mode:?} \
             (after key {key_repr:?}) — transient, clamped on next input"
        );
    }

    // 3. Cursor on char boundary
    if cursor < doc_len {
        assert!(
            text.is_char_boundary(cursor),
            "INVARIANT: cursor={cursor} is not on a char boundary (after key {key_repr:?})"
        );
    }

    // 4. Effect ordering
    if let Err(err) = validate_ordering(effects) {
        use vim_core::effects::OrderingError;
        if !matches!(err, OrderingError::UndoGroupMismatch) {
            panic!(
                "INVARIANT: effect ordering after key {key_repr:?}\n\
                 {err:?}\n\
                 effects: {effects:?}"
            );
        }
    }

    // 5. No pure-internal effects leaked
    check_no_internal_leak(effects, key_repr);

    // 6. Undo-mode consistency: if operator enters INSERT mode,
    //    the undo group must be left open (not prematurely closed).
    check_undo_mode_consistency(session, effects, key_repr);
}

/// Per-test invariant checks. Called by `TestSession::finalize()` (and Drop)
/// and by `FidelitySession` at test end.
pub fn run_per_test_invariants(session: &HostSession) {
    let text = session.text();
    let cursor = session.cursor_offset();
    let mode = session.mode();

    // 6. Cursor not on '\n' in Normal mode (unless line is empty)
    if mode == Mode::Normal
        && !text.is_empty()
        && cursor < text.len()
        && text.as_bytes()[cursor] == b'\n'
    {
        let line_start = text[..cursor].rfind('\n').map(|pos| pos + 1).unwrap_or(0);
        let line_content = &text[line_start..cursor];
        if !line_content.is_empty() {
            panic!(
                "INVARIANT: cursor on '\\n' at offset {cursor} \
                 but line is not empty (content: {line_content:?}, mode: {mode:?})"
            );
        }
    }

    // 7. Mode/selection consistency
    let has_selection = session.selection_raw().is_some();
    match mode {
        Mode::Visual(_) => {
            assert!(has_selection, "INVARIANT: Visual mode but no selection set");
        }
        Mode::Normal => {
            assert!(
                !has_selection,
                "INVARIANT: Normal mode but selection is present \
                 (selection: {:?})",
                session.selection_raw(),
            );
        }
        _ => {}
    }

    // 8. Checkpoint fallback detector: if the UndoStore used the checkpoint
    //    fallback path at any point during this test, the changeset-based undo
    //    failed (likely due to undo group structural bugs). The fallback silently
    //    produces correct text, creating false-passing tests.
    let fallback_count = session.undo_store_checkpoint_fallback_count();
    if fallback_count > 0 {
        panic!(
            "INVARIANT[checkpoint-fallback]: UndoStore used the checkpoint \
             fallback path {fallback_count} time(s) during this test. This means \
             the normal changeset-based undo failed (document length mismatch), \
             and the checkpoint recovered the correct text silently. This masks \
             a structural undo group bug — the test appears to pass but the undo \
             system is broken.\n\
             \x20 Likely cause: undo groups are not wrapping the correct edits, \
             \x20 so the document at EndUndoGroup time doesn't match the document \
             \x20 when undo is applied."
        );
    }
}

fn check_undo_depth(effects: &[Effect], key_repr: &str) {
    let mut depth: i32 = 0;
    let mut min_depth: i32 = 0;
    for effect in effects {
        match effect {
            Effect::BeginUndoGroup { .. } => depth += 1,
            Effect::EndUndoGroup { .. } => {
                depth -= 1;
                min_depth = min_depth.min(depth);
            }
            _ => {}
        }
    }
    if min_depth < -1 {
        panic!(
            "INVARIANT: undo group depth dropped to {min_depth} \
             after key {key_repr:?} (multiple unmatched EndUndoGroup)\n\
             effects: {effects:?}"
        );
    }
}

const INTERNAL_PASSTHROUGH: &[EffectKind] = &[
    EffectKind::SaveLastVisual,
    EffectKind::SetLastFind,
    EffectKind::SetLastSubstitute,
    EffectKind::SetLastSubstituteFlags,
    EffectKind::SetSubstitutePattern,
    EffectKind::PushJumpList,
    EffectKind::JumpOlder,
    EffectKind::JumpNewer,
    EffectKind::ChangelistOlder,
    EffectKind::ChangelistNewer,
    EffectKind::SetMark,
    EffectKind::ClearMark,
    EffectKind::SetStickyColumn,
    EffectKind::SetSubstituteConfirmState,
    EffectKind::ClearSubstituteConfirmState,
    EffectKind::Noop,
];

fn check_no_internal_leak(effects: &[Effect], key_repr: &str) {
    for effect in effects {
        let kind = effect.kind();
        if kind.tier() == EffectTier::Internal && !INTERNAL_PASSTHROUGH.contains(&kind) {
            panic!(
                "INVARIANT: pure-internal effect {kind:?} leaked to response \
                 after key {key_repr:?}\n\
                 effects: {effects:?}"
            );
        }
    }
}

fn check_undo_mode_consistency(session: &HostSession, effects: &[Effect], key_repr: &str) {
    let mode = session.mode();
    let mut depth: i32 = 0;
    let mut has_begin_insert = false;
    for effect in effects {
        match effect {
            Effect::BeginUndoGroup { .. } => depth += 1,
            Effect::EndUndoGroup { .. } => depth -= 1,
            Effect::BeginInsert { .. } => has_begin_insert = true,
            _ => {}
        }
    }
    if has_begin_insert && mode.is_insert() && depth == 0 {
        panic!(
            "INVARIANT[undo-mode]: effects contain BeginInsert (operator entering \
             insert mode) but undo group is BALANCED (depth=0) after key {key_repr:?}. \
             The group must be left OPEN (depth=1) so insert-mode edits form one undo atom.\n\
             \x20 mode: {mode:?}\n\
             \x20 has_pending_group: {}\n\
             \x20 effects: {effects:?}",
            session.engine().state().undo_tree().has_pending_group(),
        );
    }
}
