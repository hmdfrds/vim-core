//! Snapshot capture and partial assertion engine.
//!
//! `VimSnapshot` captures the full observable state from a `TestSession`.
//! `ExpectedState` specifies which fields to assert (all-Optional).
//! `assert_state()` compares them, accumulates ALL mismatches, and panics
//! once with a complete diagnostic.

use std::fmt;

use vim_core::effects::{Effect, EffectKind};
use vim_core::primitives::{Mode, MotionType, SearchDirection};

use crate::session::TestSession;

/// Snapshot of a single cursor's state.
#[derive(Debug, Clone)]
pub struct CursorSnapshot {
    /// Byte offset.
    pub offset: usize,
    /// Line (0-indexed).
    pub line: usize,
    /// Column (0-indexed, byte offset within line).
    pub col: usize,
}

/// Snapshot of a register's contents.
#[derive(Debug, Clone)]
pub struct RegisterEntry {
    /// Register name character.
    pub name: char,
    /// Text content.
    pub text: String,
    /// Character-wise, line-wise, or block-wise.
    pub motion_type: MotionType,
}

/// Complete observable state snapshot.
pub struct VimSnapshot {
    /// Document text.
    pub text: String,
    /// Primary cursor byte offset.
    pub cursor_offset: usize,
    /// Primary cursor line (0-indexed).
    pub cursor_line: usize,
    /// Primary cursor column (0-indexed).
    pub cursor_col: usize,
    /// Current mode.
    pub mode: Mode,
    /// All cursor snapshots (primary first).
    pub cursors: Vec<CursorSnapshot>,
    /// Non-empty registers.
    pub registers: Vec<RegisterEntry>,
    /// Set marks as `(name, offset)`.
    pub marks: Vec<(char, usize)>,
    /// Current search pattern.
    pub search_pattern: Option<String>,
    /// Current search direction.
    pub search_direction: SearchDirection,
    /// Number of committed undo groups.
    pub change_count: usize,
    /// Whether undo is available.
    pub can_undo: bool,
    /// Whether redo is available.
    pub can_redo: bool,
    /// Effects from the last operation.
    pub effects: Vec<Effect>,
    /// Error message.
    pub errmsg: Option<String>,
}

impl VimSnapshot {
    /// Capture a snapshot from a `TestSession`.
    pub fn capture(session: &TestSession) -> Self {
        let (cursor_line, cursor_col) = session.cursor_line_col();

        let cursors: Vec<CursorSnapshot> = session
            .cursor_positions()
            .iter()
            .map(|&(line, col, offset)| CursorSnapshot { offset, line, col })
            .collect();

        let mut registers = Vec::new();
        for name in "\"0123456789abcdefghijklmnopqrstuvwxyz-+*/.:%#".chars() {
            if let Some((text, motion_type)) = session.register(name) {
                if !text.is_empty() {
                    registers.push(RegisterEntry {
                        name,
                        text,
                        motion_type,
                    });
                }
            }
        }

        let mut marks = Vec::new();
        for name in "abcdefghijklmnopqrstuvwxyz.^[]<>'".chars() {
            if let Some(offset) = session.mark(name) {
                marks.push((name, offset));
            }
        }

        Self {
            text: session.text().to_owned(),
            cursor_offset: session.cursor_offset(),
            cursor_line,
            cursor_col,
            mode: session.mode(),
            cursors,
            registers,
            marks,
            search_pattern: session.search_pattern().map(str::to_owned),
            search_direction: session.search_direction(),
            change_count: session.change_count(),
            can_undo: session.can_undo(),
            can_redo: session.can_redo(),
            effects: session.last_effects().to_vec(),
            errmsg: None,
        }
    }
}

/// Partial state expectation. Only `Some` fields are asserted.
///
/// Use `ExpectedState::EMPTY` as the builder entry point:
/// ```ignore
/// ExpectedState::EMPTY.text("|world").mode(Mode::Normal)
/// ```
#[derive(Clone)]
pub struct ExpectedState {
    /// Expected annotated text (text + cursor position via `|` marker).
    pub text: Option<String>,
    /// Expected cursor position as `(line, col)`, 0-indexed.
    pub cursor: Option<(usize, usize)>,
    /// Expected cursor byte offset.
    pub cursor_offset: Option<usize>,
    /// Expected mode.
    pub mode: Option<Mode>,
    /// Expected register contents: `(name, text)`.
    pub registers: Vec<(char, String)>,
    /// Expected cursor count.
    pub cursor_count: Option<usize>,
    /// Expected cursor positions: `Vec<(line, col)>`.
    pub cursor_positions: Option<Vec<(usize, usize)>>,
    /// Expected undo group count.
    pub change_count: Option<usize>,
    /// Expected effect kinds from last operation.
    pub effects: Option<Vec<EffectKind>>,
}

impl ExpectedState {
    /// New empty expectation (asserts nothing).
    pub const EMPTY: ExpectedState = ExpectedState {
        text: None,
        cursor: None,
        cursor_offset: None,
        mode: None,
        registers: Vec::new(),
        cursor_count: None,
        cursor_positions: None,
        change_count: None,
        effects: None,
    };

    /// Set expected annotated text.
    pub fn text(mut self, t: impl Into<String>) -> Self {
        self.text = Some(t.into());
        self
    }

    /// Set expected cursor position as `(line, col)`.
    pub fn cursor(mut self, line: usize, col: usize) -> Self {
        self.cursor = Some((line, col));
        self
    }

    /// Set expected cursor byte offset.
    pub fn cursor_offset(mut self, offset: usize) -> Self {
        self.cursor_offset = Some(offset);
        self
    }

    /// Set expected mode.
    pub fn mode(mut self, mode: Mode) -> Self {
        self.mode = Some(mode);
        self
    }

    /// Add expected register content.
    pub fn register(mut self, name: char, content: impl Into<String>) -> Self {
        self.registers.push((name, content.into()));
        self
    }

    /// Set expected cursor count.
    pub fn cursor_count(mut self, n: usize) -> Self {
        self.cursor_count = Some(n);
        self
    }

    /// Set expected cursor positions.
    pub fn cursor_positions(mut self, positions: Vec<(usize, usize)>) -> Self {
        self.cursor_positions = Some(positions);
        self
    }

    /// Set expected undo group count.
    pub fn change_count(mut self, n: usize) -> Self {
        self.change_count = Some(n);
        self
    }

    /// Set expected effect kinds from last operation.
    pub fn effects(mut self, kinds: Vec<EffectKind>) -> Self {
        self.effects = Some(kinds);
        self
    }
}

/// A single field mismatch.
struct Mismatch {
    field: String,
    expected: String,
    actual: String,
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "  {}:\n    expected: {}\n      actual: {}",
            self.field, self.expected, self.actual
        )
    }
}

/// Detect whether an annotated string uses multi-cursor syntax (`|1`, `|2`, etc.)
/// vs single-cursor syntax (bare `|`).
fn is_multi_cursor_annotation(s: &str) -> bool {
    let bytes = s.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] == b'|' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'|' {
                continue; // escaped ||
            }
            if i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit() {
                return true; // |N = multi-cursor
            }
        }
    }
    false
}

fn check_text_single(
    session: &TestSession,
    expected_annotated: &str,
    mismatches: &mut Vec<Mismatch>,
) {
    let (expected_text, expected_spec) =
        vim_core::test_utils::annotated_text::parse(expected_annotated);
    let expected_offset = expected_spec.offset().get();
    let actual_text = session.text();
    let actual_offset = session.cursor_offset();

    if actual_text != expected_text {
        mismatches.push(Mismatch {
            field: "text".to_string(),
            expected: format!("{expected_text:?}"),
            actual: format!("{actual_text:?}"),
        });
    }
    if actual_offset != expected_offset {
        let actual_annotated = session.annotated();
        mismatches.push(Mismatch {
            field: "cursor (from annotated text)".to_string(),
            expected: format!("{expected_annotated:?} (offset {expected_offset})"),
            actual: format!("{actual_annotated:?} (offset {actual_offset})"),
        });
    }
}

fn check_text_multi(
    session: &TestSession,
    expected_annotated: &str,
    mismatches: &mut Vec<Mismatch>,
) {
    let (expected_text, expected_spec) =
        vim_core::test_utils::annotated_text::parse_multi(expected_annotated);
    let actual_text = session.text();

    if actual_text != expected_text {
        mismatches.push(Mismatch {
            field: "text".to_string(),
            expected: format!("{expected_text:?}"),
            actual: format!("{actual_text:?}"),
        });
    }

    let expected_offsets: Vec<usize> = expected_spec
        .iter()
        .map(|(_, spec)| spec.offset().get())
        .collect();

    let actual_offsets: Vec<usize> = session
        .cursor_positions()
        .iter()
        .map(|&(_, _, offset)| offset)
        .collect();

    if actual_offsets.len() != expected_offsets.len() {
        mismatches.push(Mismatch {
            field: "cursor_count (from annotated text)".to_string(),
            expected: format!("{}", expected_offsets.len()),
            actual: format!("{}", actual_offsets.len()),
        });
    } else {
        let mut expected_sorted = expected_offsets.clone();
        let mut actual_sorted = actual_offsets.clone();
        expected_sorted.sort();
        actual_sorted.sort();

        if expected_sorted != actual_sorted {
            let actual_annotated = session.annotated_multi();
            mismatches.push(Mismatch {
                field: "cursor positions (from annotated text)".to_string(),
                expected: format!("{expected_annotated:?} (offsets {expected_sorted:?})"),
                actual: format!("{actual_annotated:?} (offsets {actual_sorted:?})"),
            });
        }
    }
}

/// Assert that a `TestSession` matches an `ExpectedState`.
///
/// Only fields set in `expected` are checked. All mismatches are
/// accumulated and reported in a single panic.
#[track_caller]
pub fn assert_state(session: &TestSession, expected: ExpectedState, label: &str) {
    let mut mismatches: Vec<Mismatch> = Vec::new();

    // Text + cursor(s) — auto-detect single vs multi-cursor syntax
    if let Some(ref expected_annotated) = expected.text {
        if is_multi_cursor_annotation(expected_annotated) {
            check_text_multi(session, expected_annotated, &mut mismatches);
        } else {
            check_text_single(session, expected_annotated, &mut mismatches);
        }
    }

    // Cursor (line, col) — derived from cursor_offset, not cursor_positions
    if let Some((exp_line, exp_col)) = expected.cursor {
        let text = session.text();
        let offset = session.cursor_offset();
        let (act_line, act_col) = offset_to_line_col(text, offset);
        if act_line != exp_line || act_col != exp_col {
            mismatches.push(Mismatch {
                field: "cursor".to_string(),
                expected: format!("({exp_line}, {exp_col})"),
                actual: format!("({act_line}, {act_col})"),
            });
        }
    }

    // Cursor offset
    if let Some(exp_offset) = expected.cursor_offset {
        let act_offset = session.cursor_offset();
        if act_offset != exp_offset {
            mismatches.push(Mismatch {
                field: "cursor_offset".to_string(),
                expected: exp_offset.to_string(),
                actual: act_offset.to_string(),
            });
        }
    }

    // Mode
    if let Some(exp_mode) = expected.mode {
        let act_mode = session.mode();
        if act_mode != exp_mode {
            mismatches.push(Mismatch {
                field: "mode".to_string(),
                expected: format!("{exp_mode:?}"),
                actual: format!("{act_mode:?}"),
            });
        }
    }

    // Registers
    for (name, exp_content) in &expected.registers {
        let actual = session
            .register(*name)
            .map(|(text, _)| text)
            .unwrap_or_default();
        if actual != *exp_content {
            mismatches.push(Mismatch {
                field: format!("register '{name}'"),
                expected: format!("{exp_content:?}"),
                actual: format!("{actual:?}"),
            });
        }
    }

    // Cursor count
    if let Some(exp_count) = expected.cursor_count {
        let act_count = session.cursor_count();
        if act_count != exp_count {
            mismatches.push(Mismatch {
                field: "cursor_count".to_string(),
                expected: exp_count.to_string(),
                actual: act_count.to_string(),
            });
        }
    }

    // Cursor positions
    if let Some(ref exp_positions) = expected.cursor_positions {
        let act_positions: Vec<(usize, usize)> = session
            .cursor_positions()
            .iter()
            .map(|&(line, col, _)| (line, col))
            .collect();
        if act_positions != *exp_positions {
            mismatches.push(Mismatch {
                field: "cursor_positions".to_string(),
                expected: format!("{exp_positions:?}"),
                actual: format!("{act_positions:?}"),
            });
        }
    }

    // Change count
    if let Some(exp_count) = expected.change_count {
        let act_count = session.change_count();
        if act_count != exp_count {
            mismatches.push(Mismatch {
                field: "change_count".to_string(),
                expected: exp_count.to_string(),
                actual: act_count.to_string(),
            });
        }
    }

    // Effects
    if let Some(ref exp_kinds) = expected.effects {
        let act_kinds: Vec<EffectKind> = session.last_effects().iter().map(|e| e.kind()).collect();
        if act_kinds != *exp_kinds {
            mismatches.push(Mismatch {
                field: "effects".to_string(),
                expected: format!("{exp_kinds:?}"),
                actual: format!("{act_kinds:?}"),
            });
        }
    }

    // Report
    if !mismatches.is_empty() {
        let r = crate::error::TerminalRenderer::new();
        let count = mismatches.len();
        let banner = r.banner("ASSERTION FAILED");
        let details: String = mismatches
            .iter()
            .enumerate()
            .map(|(i, m)| {
                format!(
                    "\n[{}/{}]   {}:\n    {}: {}\n    {}:   {}",
                    i + 1,
                    count,
                    m.field,
                    r.green("expected"),
                    m.expected,
                    r.red("actual"),
                    m.actual,
                )
            })
            .collect();

        panic!(
            "\n\n {banner} {label}\n\
             {details}\n\n\
             \x20 actual state:\n\
             \x20   annotated: {:?}\n\
             \x20   mode: {:?}\n\
             \x20   cursor_count: {}\n",
            session.annotated(),
            session.mode(),
            session.cursor_count(),
        );
    }
}

fn offset_to_line_col(text: &str, offset: usize) -> (usize, usize) {
    let clamped = offset.min(text.len());
    let before = &text.as_bytes()[..clamped];
    let line = before.iter().filter(|&&b| b == b'\n').count();
    let col = match before.iter().rposition(|&b| b == b'\n') {
        Some(nl) => clamped - nl - 1,
        None => clamped,
    };
    (line, col)
}

/// Assert text and cursor position using annotated text notation.
///
/// Convenience for the common case where you only need to check the
/// document content and cursor position:
/// ```ignore
/// assert_text(&session, "|world");
/// ```
#[track_caller]
pub fn assert_text(session: &TestSession, expected_annotated: &str) {
    assert_state(
        session,
        ExpectedState::EMPTY.text(expected_annotated),
        "text assertion",
    );
}

/// Assert the current mode.
#[track_caller]
pub fn assert_mode(session: &TestSession, expected: Mode) {
    assert_state(
        session,
        ExpectedState::EMPTY.mode(expected),
        "mode assertion",
    );
}
