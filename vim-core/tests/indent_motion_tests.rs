//! Integration tests for indent navigation motions: `[i`, `]i`, `[-`, `]-`, `[+`, `]+`
//!
//! These motions are custom to vim-core (not in Neovim), so they use direct
//! VimEngine testing rather than Neovim golden comparisons.

mod common;

use common::document::TestDocument;
use common::runner::apply_effect;
use vim_core::document::Document;
use vim_core::execution::{InputContext, VimEngine};
use vim_core::keymap::KeyEvent;
use vim_core::primitives::Mode;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Run an ex command through the engine (`:command\n`).
fn run_ex(engine: &mut VimEngine, doc: &TestDocument, command: &str) {
    let _ = press_apply(engine, doc, KeyEvent::char(':'));
    for ch in command.chars() {
        let _ = press_apply(engine, doc, KeyEvent::char(ch));
    }
    let _ = press_apply(engine, doc, KeyEvent::enter());
}

/// Press a key and return the response (used for ex commands or read-only presses).
fn press_apply(
    engine: &mut VimEngine,
    doc: &TestDocument,
    key: KeyEvent,
) -> vim_core::execution::Response {
    let ctx = InputContext::new(doc, doc.cursor_offset()).validate_clamped();
    engine.process(key, ctx)
}

/// Press a key, apply all resulting effects to `doc`, and return the cursor
/// offset after the key was processed.
fn press_and_apply(engine: &mut VimEngine, doc: &mut TestDocument, key: KeyEvent) -> usize {
    let ctx = InputContext::new(&*doc, doc.cursor_offset()).validate_clamped();
    let mut response = engine.process(key, ctx);
    let effects = response.take_effects();
    for effect in effects {
        apply_effect(doc, effect);
    }
    doc.cursor_offset()
}

/// Send a sequence of characters through `press_and_apply`, returning the final cursor offset.
fn send_keys(engine: &mut VimEngine, doc: &mut TestDocument, keys: &str) -> usize {
    let mut offset = doc.cursor_offset();
    for ch in keys.chars() {
        offset = press_and_apply(engine, doc, KeyEvent::char(ch));
    }
    offset
}

// ── Basic navigation ─────────────────────────────────────────────────────────

/// `]i` — next line with same indentation.
///
/// "  a\n    b\n  c" — cursor at (0,2) on 'a' (2-space indent).
/// Line 1 has 4-space indent (different), line 2 has 2-space indent (same).
/// Expect cursor on line 2 at the first non-blank col 2.
#[test]
fn next_same_indent_basic() {
    let mut doc = TestDocument::new("  a\n    b\n  c", (0, 2));
    let mut engine = VimEngine::new();

    send_keys(&mut engine, &mut doc, "]i");

    let pos = doc.cursor_position();
    assert_eq!(pos.0, 2, "should land on line 2");
    assert_eq!(pos.1, 2, "should land at col 2 (first non-blank of '  c')");
}

/// `[i` — previous line with same indentation.
///
/// "  a\n    b\n  c" — cursor at (2,2) on 'c' (2-space indent).
/// Line 1 has 4-space indent (different), line 0 has 2-space indent (same).
/// Expect cursor on line 0 at col 2.
#[test]
fn prev_same_indent_basic() {
    let mut doc = TestDocument::new("  a\n    b\n  c", (2, 2));
    let mut engine = VimEngine::new();

    send_keys(&mut engine, &mut doc, "[i");

    let pos = doc.cursor_position();
    assert_eq!(pos.0, 0, "should land on line 0");
    assert_eq!(pos.1, 2, "should land at col 2 (first non-blank of '  a')");
}

/// `]-` — next line with lesser indentation.
///
/// "    a\n      b\n  c" — cursor at (1,6) on 'b' (6-space indent).
/// Line 2 has 2-space indent which is less than 6.
/// Expect cursor on line 2 at col 2.
#[test]
fn next_lesser_indent_basic() {
    let mut doc = TestDocument::new("    a\n      b\n  c", (1, 6));
    let mut engine = VimEngine::new();

    send_keys(&mut engine, &mut doc, "]-");

    let pos = doc.cursor_position();
    assert_eq!(pos.0, 2, "should land on line 2");
    assert_eq!(pos.1, 2, "should land at col 2 (first non-blank of '  c')");
}

/// `[-` — previous line with lesser indentation (parent scope).
///
/// "  a\n    b\n    c" — cursor at (2,4) on 'c' (4-space indent).
/// Line 0 has 2-space indent which is less than 4.
/// Expect cursor on line 0 at col 2.
#[test]
fn prev_lesser_indent_basic() {
    let mut doc = TestDocument::new("  a\n    b\n    c", (2, 4));
    let mut engine = VimEngine::new();

    send_keys(&mut engine, &mut doc, "[-");

    let pos = doc.cursor_position();
    assert_eq!(pos.0, 0, "should land on line 0");
    assert_eq!(pos.1, 2, "should land at col 2 (first non-blank of '  a')");
}

/// `]+` — next line with greater indentation (child).
///
/// "  a\n    b\n  c" — cursor at (0,2) on 'a' (2-space indent).
/// Line 1 has 4-space indent which is greater.
/// Expect cursor on line 1 at col 4.
#[test]
fn next_greater_indent_basic() {
    let mut doc = TestDocument::new("  a\n    b\n  c", (0, 2));
    let mut engine = VimEngine::new();

    send_keys(&mut engine, &mut doc, "]+");

    let pos = doc.cursor_position();
    assert_eq!(pos.0, 1, "should land on line 1");
    assert_eq!(
        pos.1, 4,
        "should land at col 4 (first non-blank of '    b')"
    );
}

/// `[+` — previous line with greater indentation.
///
/// "  a\n    b\n  c" — cursor at (2,2) on 'c' (2-space indent).
/// Line 1 has 4-space indent which is greater.
/// Expect cursor on line 1 at col 4.
#[test]
fn prev_greater_indent_basic() {
    let mut doc = TestDocument::new("  a\n    b\n  c", (2, 2));
    let mut engine = VimEngine::new();

    send_keys(&mut engine, &mut doc, "[+");

    let pos = doc.cursor_position();
    assert_eq!(pos.0, 1, "should land on line 1");
    assert_eq!(
        pos.1, 4,
        "should land at col 4 (first non-blank of '    b')"
    );
}

// ── Count support ─────────────────────────────────────────────────────────────

/// `3]i` — skip to the 3rd next line with same indentation.
///
/// "  a\n    x\n  b\n    y\n  c\n    z\n  d" — cursor at (0,2) on 'a'.
/// Same-indent lines (2 spaces): lines 2 ('b'), 4 ('c'), 6 ('d').
/// Count 3 → land on line 6.
#[test]
fn next_same_indent_count_3() {
    let mut doc = TestDocument::new("  a\n    x\n  b\n    y\n  c\n    z\n  d", (0, 2));
    let mut engine = VimEngine::new();

    send_keys(&mut engine, &mut doc, "3]i");

    let pos = doc.cursor_position();
    assert_eq!(
        pos.0, 6,
        "count 3 should skip to 3rd same-indent line (line 6 'd')"
    );
    assert_eq!(pos.1, 2, "should be at col 2");
}

/// `2[-` — skip to the 2nd previous line with lesser indentation.
///
/// "a\n  b\n    c\n  d\n    e" — cursor at (4,4) on 'e' (4-space indent).
///
/// `reference_indent` is computed once from the starting line and is not reset
/// at each match, so "lesser" means "< 4" for the whole scan.
///
/// Trace, going up from line 4 ("    e", indent 4):
/// line 3: "  d" → indent 2 < 4 → found=1
/// line 2: "    c" → indent 4, not < 4 → skip
/// line 1: "  b" → indent 2 < 4 → found=2 → return line 1
/// So the cursor lands on line 1, col 2.
///
/// The task description claimed "2nd lesser-indent → line 0 col 0 ('a')", which
/// is wrong. This test asserts the implementation's actual behavior.
#[test]
fn prev_lesser_indent_count_2() {
    let mut doc = TestDocument::new("a\n  b\n    c\n  d\n    e", (4, 4));
    let mut engine = VimEngine::new();

    send_keys(&mut engine, &mut doc, "2[-");

    let pos = doc.cursor_position();
    // 1st lesser-indent from line 4 (indent=4): line 3 "  d" (indent=2) → found=1
    // 2nd lesser-indent: line 1 "  b" (indent=2) → found=2
    assert_eq!(
        pos.0, 1,
        "2nd previous lesser-indent should be line 1 '  b'"
    );
    assert_eq!(pos.1, 2, "col 2 is first non-blank of '  b'");
}

// ── Blank line handling ───────────────────────────────────────────────────────

/// `]i` skips blank lines.
///
/// "  a\n\n\n  b" — cursor at (0,2). Lines 1-2 are blank and must be skipped.
/// Line 3 has 2-space indent = same as line 0.
/// Expect cursor on line 3 at col 2.
#[test]
fn blank_lines_skipped() {
    let mut doc = TestDocument::new("  a\n\n\n  b", (0, 2));
    let mut engine = VimEngine::new();

    send_keys(&mut engine, &mut doc, "]i");

    let pos = doc.cursor_position();
    assert_eq!(pos.0, 3, "should skip blank lines 1-2 and land on line 3");
    assert_eq!(pos.1, 2, "col 2 is first non-blank of '  b'");
}

/// `]i` on an all-blank document returns Error (no movement).
///
/// "\n\n\n" — all blank lines. No non-blank line has any indent to compare.
/// Cursor should remain at (0,0).
#[test]
fn all_blank_lines_no_movement() {
    let mut doc = TestDocument::new("\n\n\n", (0, 0));
    let mut engine = VimEngine::new();

    let initial_pos = doc.cursor_position();
    send_keys(&mut engine, &mut doc, "]i");

    let pos = doc.cursor_position();
    assert_eq!(
        pos, initial_pos,
        "cursor should not move when all lines are blank"
    );
}

// ── Tab handling ──────────────────────────────────────────────────────────────

/// `]i` with tab-indented lines (default tabstop=8).
///
/// "\ta\n\t\tb\n\tc" — cursor at (0,1). Line 0 has 1 tab = 8 cols (tabstop=8 default).
/// Line 1 has 2 tabs = 16 cols. Line 2 has 1 tab = 8 cols = same indent.
/// Expect cursor on line 2.
#[test]
fn tab_indent_same() {
    let mut doc = TestDocument::new("\ta\n\t\tb\n\tc", (0, 1));
    let mut engine = VimEngine::new();

    send_keys(&mut engine, &mut doc, "]i");

    let pos = doc.cursor_position();
    assert_eq!(
        pos.0, 2,
        "should land on line 2 (same tab indent as line 0)"
    );
}

/// `]i` with mixed tabs+spaces when tabstop=4.
///
/// "\ta\n    b" — with tabstop=4: 1 tab = 4 cols, 4 spaces = 4 cols.
/// Both lines have 4-column indent. `]i` from line 0 should find line 1.
#[test]
fn mixed_tabs_spaces_same_indent() {
    let mut doc = TestDocument::new("\ta\n    b", (0, 1));
    let mut engine = VimEngine::new();

    run_ex(&mut engine, &doc, "set tabstop=4");
    send_keys(&mut engine, &mut doc, "]i");

    let pos = doc.cursor_position();
    assert_eq!(
        pos.0, 1,
        "with tabstop=4, tab and 4 spaces are same width (4 cols); should land on line 1"
    );
}

// ── Boundary conditions ───────────────────────────────────────────────────────

/// `[i` at the first line returns Error (no movement).
///
/// "  a\n  b" — cursor at (0,2). No lines before line 0.
/// Cursor should remain at (0,2).
#[test]
fn prev_same_at_first_line_error() {
    let mut doc = TestDocument::new("  a\n  b", (0, 2));
    let mut engine = VimEngine::new();

    let initial_pos = doc.cursor_position();
    send_keys(&mut engine, &mut doc, "[i");

    let pos = doc.cursor_position();
    assert_eq!(pos, initial_pos, "no movement when already at first line");
}

/// `]i` at the last line returns Error (no movement).
///
/// "  a\n  b" — cursor at (1,2). No lines after line 1.
/// Cursor should remain at (1,2).
#[test]
fn next_same_at_last_line_error() {
    let mut doc = TestDocument::new("  a\n  b", (1, 2));
    let mut engine = VimEngine::new();

    let initial_pos = doc.cursor_position();
    send_keys(&mut engine, &mut doc, "]i");

    let pos = doc.cursor_position();
    assert_eq!(pos, initial_pos, "no movement when already at last line");
}

/// `[-` at zero indentation returns Error (no movement).
///
/// "a\nb" — cursor at (0,0). Indent = 0. Nothing can be less than 0.
/// Cursor should remain at (0,0).
#[test]
fn zero_indent_lesser_error() {
    let mut doc = TestDocument::new("a\nb", (0, 0));
    let mut engine = VimEngine::new();

    let initial_pos = doc.cursor_position();
    send_keys(&mut engine, &mut doc, "[-");

    let pos = doc.cursor_position();
    assert_eq!(
        pos, initial_pos,
        "no movement: nothing has less indent than 0"
    );
}

// ── Operator composition ──────────────────────────────────────────────────────

/// `d]i` — delete from cursor line through next same-indent line (linewise).
///
/// "  a\n    b\n  c\n  d" — cursor at (0,2) on 'a' (2-space indent).
/// `]i` motion finds line 2 "  c". Linewise delete covers lines 0-2.
/// After deletion, doc should contain only "  d".
#[test]
fn delete_next_same_indent() {
    let mut doc = TestDocument::new("  a\n    b\n  c\n  d", (0, 2));
    let mut engine = VimEngine::new();

    send_keys(&mut engine, &mut doc, "d]i");

    let text = doc.text().to_string();
    // Lines 0-2 ("  a", "    b", "  c") should be deleted.
    // Only "  d" should remain.
    assert!(
        text.contains("  d"),
        "after d]i, '  d' should still be in the document; got: {:?}",
        text
    );
    assert!(
        !text.contains("  a"),
        "after d]i, '  a' should be deleted; got: {:?}",
        text
    );
    assert!(
        !text.contains("  c"),
        "after d]i, '  c' should be deleted; got: {:?}",
        text
    );
}

/// `y]+` — yank from cursor line through next greater-indent line.
///
/// "  a\n    b\n  c" — cursor at (0,2) on 'a' (2-space indent).
/// `]+` motion finds line 1 "    b". Linewise yank covers lines 0-1.
/// The unnamed register should contain those lines.
#[test]
fn yank_next_greater_indent() {
    let mut doc = TestDocument::new("  a\n    b\n  c", (0, 2));
    let mut engine = VimEngine::new();

    send_keys(&mut engine, &mut doc, "y]+");

    // Unnamed register '"' should contain the yanked text
    let register = doc.get_register('"');
    assert!(
        register.is_some(),
        "unnamed register should be set after yank"
    );
    let (yanked_text, _regtype) = register.unwrap();
    assert!(
        yanked_text.contains("  a"),
        "yanked text should contain '  a'; got: {:?}",
        yanked_text
    );
    assert!(
        yanked_text.contains("    b"),
        "yanked text should contain '    b'; got: {:?}",
        yanked_text
    );
}

/// `c[-` — change from previous lesser-indent through cursor line.
///
/// "a\n  b\n  c" — cursor at (2,2) on 'c' (2-space indent).
/// `[-` motion finds line 0 "a" (0-space indent, which is less than 2).
/// After `c`, the engine should enter Insert mode.
#[test]
fn change_prev_lesser_indent() {
    let mut doc = TestDocument::new("a\n  b\n  c", (2, 2));
    let mut engine = VimEngine::new();

    send_keys(&mut engine, &mut doc, "c[-");

    assert_eq!(
        engine.mode(),
        Mode::Insert,
        "after c[-, engine should be in Insert mode"
    );
}
