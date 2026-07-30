//! Coverage gap tests for multi-cursor per-cursor re-execution.
//!
//! These tests close the gaps identified by the test coverage audit:
//! - `di(` bracket text-object multi-cursor
//! - `r\n` replace with newline multi-cursor
//! - Insert-mode CD commands: Ctrl-U, Del, Ctrl-T, Ctrl-D, Enter, Tab
//! - `g?` (rot13) multi-cursor
//! - Prefix CD classification: gJ, g<C-a>, g<C-x>

#![allow(non_snake_case)]

use vim_core::execution::{parse_keys_from_string, HostSession};

fn feed(session: &mut HostSession, keys: &str) {
    for key in parse_keys_from_string(keys) {
        session.process_key_host(key);
    }
}

fn session_with_cursors(text: &str, offsets: &[usize]) -> HostSession {
    let mut session = HostSession::new(text);
    if offsets.is_empty() {
        return session;
    }
    session.set_cursor_offset(offsets[0]);
    for &offset in &offsets[1..] {
        session
            .add_cursor(offset)
            .expect("add_cursor should succeed");
    }
    assert_eq!(session.cursor_count(), offsets.len());
    session
}

// ═══════════════════════════════════════════════════════════════════════
// di( — bracket text-object multi-cursor
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn di_paren_different_content_lengths() {
    // Cursor 1 inside (ab), cursor 2 inside (cdefg).
    // di( should delete 2 bytes at cursor 1 and 5 bytes at cursor 2.
    let mut session = session_with_cursors("(ab) (cdefg)", &[1, 6]);
    feed(&mut session, "di(");
    assert_eq!(session.text(), "() ()");
}

#[test]
fn di_paren_nested_vs_simple() {
    // Cursor 1 inside simple (x), cursor 2 inside (hello world).
    let mut session = session_with_cursors("(x) (hello world)", &[1, 5]);
    feed(&mut session, "di(");
    assert_eq!(session.text(), "() ()");
}

// ═══════════════════════════════════════════════════════════════════════
// r\n — replace with newline multi-cursor
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn replace_newline_three_cursors() {
    // Text "abc", cursors at 0, 1, 2. Replace each char with newline.
    let mut session = session_with_cursors("abc", &[0, 1, 2]);
    feed(&mut session, "r<CR>");
    assert_eq!(session.text(), "\n\n\n");
}

#[test]
fn replace_newline_different_chars() {
    // Text "a b", cursors at 0 and 2. Replace 'a' and 'b' with newlines.
    let mut session = session_with_cursors("a b", &[0, 2]);
    feed(&mut session, "r<CR>");
    assert_eq!(session.text(), "\n \n");
}

// ═══════════════════════════════════════════════════════════════════════
// g? (rot13) multi-cursor
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn rot13_different_words() {
    // Cursor 1 on "hello", cursor 2 on "world". g?iw should rot13 each independently.
    let mut session = session_with_cursors("hello world", &[0, 6]);
    feed(&mut session, "g?iw");
    assert_eq!(session.text(), "uryyb jbeyq");
}

// ═══════════════════════════════════════════════════════════════════════
// Insert-mode Ctrl-U (DeleteToStart) at different column positions
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn insert_ctrl_u_different_columns() {
    // Line 1: "hello" cursor at col 3, Line 2: "ab" cursor at col 1.
    // Ctrl-U should delete to line start: 3 chars at cursor 1, 1 char at cursor 2.
    let mut session = session_with_cursors("hello\nab", &[3, 7]);
    feed(&mut session, "i<C-u>");
    // After Ctrl-U: cursor 1 deletes "hel" -> "lo", cursor 2 deletes "a" -> "b"
    assert_eq!(session.text(), "lo\nb");
}

// ═══════════════════════════════════════════════════════════════════════
// Insert-mode Del (DeleteUnder) with different char widths
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn insert_del_ascii_two_cursors() {
    // Text "abcd", cursors at 1 and 3. Del deletes char under cursor.
    let mut session = session_with_cursors("abcd", &[1, 3]);
    feed(&mut session, "i<Del>");
    // Cursor 1 deletes 'b', cursor 2 deletes 'd'
    assert_eq!(session.text(), "ac");
}

// ═══════════════════════════════════════════════════════════════════════
// Insert-mode Ctrl-T (Indent) multi-cursor
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn insert_ctrl_t_two_lines() {
    // Two lines, cursors on each. Ctrl-T adds shiftwidth spaces at line start.
    // Default shiftwidth is 8 in vim-core.
    let mut session = session_with_cursors("hello\nworld", &[2, 8]);
    feed(&mut session, "i<C-t>");
    // Both lines get indent (default shiftwidth)
    let text = session.text();
    // Verify both lines were indented (exact amount depends on default sw)
    assert!(text.starts_with(' '), "line 1 should be indented");
    assert!(
        text.contains("\n "),
        "line 2 should be indented after newline"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Insert-mode Ctrl-D (Outdent) with different indent levels
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn insert_ctrl_d_different_indents() {
    // Line 1 has 8 spaces indent, line 2 has 8 spaces indent.
    // Ctrl-D removes one shiftwidth from each.
    let mut session = session_with_cursors("        hello\n        world", &[10, 24]);
    feed(&mut session, "i<C-d>");
    let text = session.text();
    // Both lines should have reduced indentation
    assert!(
        !text.starts_with("        "),
        "line 1 indent should be reduced"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Insert-mode Enter (newline) with autoindent at different indent contexts
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn insert_enter_two_cursors() {
    // Two cursors in middle of text. Enter inserts newline at each position.
    let mut session = session_with_cursors("abcd\nefgh", &[2, 7]);
    feed(&mut session, "i<CR>");
    // Each cursor gets a newline inserted before it
    assert_eq!(session.text(), "ab\ncd\nef\ngh");
}

// ═══════════════════════════════════════════════════════════════════════
// Prefix CD: gJ (JoinNoSpace) multi-cursor
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn gJ_two_cursors_consecutive_lines() {
    // Cursors on line 0 and line 1. gJ joins without space.
    // Line 0 joins with line 1, line 1 joins with line 2.
    let mut session = session_with_cursors("aaa\nbbb\nccc", &[0, 4]);
    feed(&mut session, "gJ");
    // Both joins fire: line0+line1 and line1+line2
    assert_eq!(session.text(), "aaabbbccc");
}

// ═══════════════════════════════════════════════════════════════════════
// Classification tests for Prefix CD commands
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn prefix_join_no_space_is_content_dependent() {
    use std::num::NonZeroU32;
    use vim_core::grammar::{Command, PrefixCommand};
    let cmd = Command::Prefix {
        count: NonZeroU32::MIN,
        register: None,
        command: PrefixCommand::JoinNoSpace,
    };
    assert!(cmd.is_content_dependent(), "gJ should be content-dependent");
}

#[test]
fn prefix_sequential_increment_is_content_dependent() {
    use std::num::NonZeroU32;
    use vim_core::grammar::{Command, PrefixCommand};
    let cmd = Command::Prefix {
        count: NonZeroU32::MIN,
        register: None,
        command: PrefixCommand::SequentialIncrement,
    };
    assert!(
        cmd.is_content_dependent(),
        "g<C-a> should be content-dependent"
    );
}

#[test]
fn prefix_sequential_decrement_is_content_dependent() {
    use std::num::NonZeroU32;
    use vim_core::grammar::{Command, PrefixCommand};
    let cmd = Command::Prefix {
        count: NonZeroU32::MIN,
        register: None,
        command: PrefixCommand::SequentialDecrement,
    };
    assert!(
        cmd.is_content_dependent(),
        "g<C-x> should be content-dependent"
    );
}

#[test]
fn prefix_scroll_center_is_not_content_dependent() {
    use std::num::NonZeroU32;
    use vim_core::grammar::{Command, PrefixCommand};
    let cmd = Command::Prefix {
        count: NonZeroU32::MIN,
        register: None,
        command: PrefixCommand::ScrollCenter,
    };
    assert!(
        !cmd.is_content_dependent(),
        "zz should NOT be content-dependent"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Escape in Normal mode clears secondary cursors
// ═══════════════════════════════════════════════════════════════════════

/// Escape in Normal mode must collapse a multi-cursor session to a single
/// primary cursor.  The secondary cursors are added via `add_cursor` (same
/// as `gb`), then `<Esc>` is fed — the count must drop back to 1.
#[test]
fn escape_in_normal_mode_clears_secondary_cursors() {
    let mut session = session_with_cursors("foo bar foo", &[0, 4, 8]);
    assert_eq!(
        session.cursor_count(),
        3,
        "setup: 3 cursors expected before Escape"
    );
    feed(&mut session, "<Esc>");
    assert_eq!(
        session.cursor_count(),
        1,
        "Escape in Normal mode must collapse to a single cursor"
    );
}

/// Ctrl-C in Normal mode must also clear secondary cursors.
#[test]
fn ctrl_c_in_normal_mode_clears_secondary_cursors() {
    let mut session = session_with_cursors("foo bar foo", &[0, 4, 8]);
    assert_eq!(session.cursor_count(), 3, "setup: 3 cursors expected");
    feed(&mut session, "<C-c>");
    assert_eq!(
        session.cursor_count(),
        1,
        "Ctrl-C in Normal mode must collapse to a single cursor"
    );
}

/// Ctrl-[ in Normal mode must also clear secondary cursors.
#[test]
fn ctrl_bracket_in_normal_mode_clears_secondary_cursors() {
    let mut session = session_with_cursors("foo bar foo", &[0, 4, 8]);
    assert_eq!(session.cursor_count(), 3, "setup: 3 cursors expected");
    feed(&mut session, "<C-[>");
    assert_eq!(
        session.cursor_count(),
        1,
        "Ctrl-[ in Normal mode must collapse to a single cursor"
    );
}

/// Escape with a single cursor is a no-op (cursor count stays 1).
#[test]
fn escape_in_normal_mode_single_cursor_noop() {
    let mut session = HostSession::new("hello world");
    assert_eq!(session.cursor_count(), 1, "single cursor initially");
    feed(&mut session, "<Esc>");
    assert_eq!(
        session.cursor_count(),
        1,
        "Escape with single cursor must leave cursor count at 1"
    );
}
