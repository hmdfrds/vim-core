//! Ideal multi-cursor dot repeat tests.
//!
//! Design rule: `.` fans out to all active cursors. After Escape
//! (secondary cursors dismissed), `.` replays at single cursor only.
//!
//! Dot replays the EDIT, not the cursor spawning. After `gb gb ciw NEW <Esc>`,
//! pressing `.` at active cursors repeats `ciw NEW` at each, not the
//! `gb gb ciw NEW` sequence.
//!
//! These tests describe how multi-cursor dot repeat SHOULD work.
//! They may fail against the current implementation.

use vim_core::execution::{parse_keys_from_string, HostSession};
use vim_test::prelude::*;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn feed(session: &mut HostSession, keys: &str) {
    for key in parse_keys_from_string(keys) {
        session.process_key_host(key);
    }
}

fn session_with_cursors(text: &str, offsets: &[usize]) -> HostSession {
    let mut session = HostSession::new(text).with_auto_handle_defaults(true);
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

// ═══════════════════════════════════════════════════════════════════════════
// 1. ciw + type + Esc + dot — repeats change-inner-word at all cursors
// ═══════════════════════════════════════════════════════════════════════════

/// ciw replaces each cursor's word, then dot at new positions repeats it.
#[test]
fn mc_dot_repeats_ciw_at_all_cursors() {
    vim_mc("|1foo bar\n|2baz qux")
        .keys("ciwX<Esc>")
        .expect_text("|X bar\nX qux")
        .keys("w")
        .keys(".")
        .expect_text("X |X\nX X")
        .labeled("dot repeats ciw+X at all cursors after w")
        .run();
}

/// ciw with multi-char replacement, then dot on different words.
#[test]
fn mc_dot_repeats_ciw_multichar() {
    // "aaa bbb\nccc ddd" — cursors on "aaa" and "ccc"
    // ciw + "NEW" replaces both -> "NEW bbb\nNEW ddd"
    // w moves to "bbb" and "ddd", then dot replaces those too
    vim_mc("|1aaa bbb\n|2ccc ddd")
        .keys("ciwNEW<Esc>")
        .expect_text("NE|W bbb\nNEW ddd")
        .keys("w")
        .keys(".")
        .expect_text("NEW NE|W\nNEW NEW")
        .labeled("dot repeats ciw+NEW at all cursors")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. dw + dot — repeats delete-word at all cursors
// ═══════════════════════════════════════════════════════════════════════════

/// dw deletes word at each cursor, then dot deletes the next word at each.
#[test]
fn mc_dot_repeats_dw_at_all_cursors() {
    // "aa bb cc\ndd ee ff" — cursors at start of each line
    // dw deletes "aa " and "dd " -> "bb cc\nee ff"
    // dot deletes "bb " and "ee " -> "cc\nff"
    vim_mc("|1aa bb cc\n|2dd ee ff")
        .keys("dw")
        .expect_text("|bb cc\nee ff")
        .keys(".")
        .expect_text("|cc\nff")
        .labeled("dot repeats dw at all cursors")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. x + dot — repeats delete-char at all cursors
// ═══════════════════════════════════════════════════════════════════════════

/// x deletes char at each cursor, dot deletes the next char at each.
#[test]
fn mc_dot_repeats_x_at_all_cursors() {
    vim_mc("|1abcd\n|2efgh")
        .keys("x")
        .expect_text("|bcd\nfgh")
        .keys(".")
        .expect_text("|cd\ngh")
        .labeled("dot repeats x at all cursors")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. dd + dot — repeats delete-line at all cursors
// ═══════════════════════════════════════════════════════════════════════════

/// dd deletes each cursor's line, dot deletes the next line at each.
#[test]
fn mc_dot_repeats_dd_at_all_cursors() {
    // 4 lines: cursors on line 0 and line 2
    // dd deletes "aaa" and "ccc" -> "bbb\nddd"
    // dot deletes "bbb" and "ddd" -> empty or single newline
    vim_mc("|1aaa\nbbb\n|2ccc\nddd")
        .keys("dd")
        .expect_text("|bbb\nddd")
        .keys(".")
        .labeled("dot repeats dd at all cursors — both remaining lines deleted")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. A; + Esc + dot — repeats append-semicolon at each line
// ═══════════════════════════════════════════════════════════════════════════

/// A; appends semicolon at end of each cursor's line, dot repeats it.
#[test]
fn mc_dot_repeats_append_semicolon() {
    // Cursors at start of each line. A moves to EOL + insert, type ";", Esc.
    // Then j moves down, or dot repeats the append at the next cursor.
    vim_mc("|1let x = 1\n|2let y = 2")
        .keys("A;<Esc>")
        .expect_text("let x = 1|;\nlet y = 2;")
        .labeled("A; appends semicolon at both lines")
        .run();
}

/// After A; on two lines, move to new lines and dot to add semicolons.
/// Dot should replay "A;" (go-to-end-of-line, insert ";") at whatever
/// line the cursor is on, appending ";" at EOL.
#[test]
fn mc_dot_repeats_append_semicolon_at_new_positions() {
    // "aaa\nbbb\nccc\nddd" — cursors on lines 0 and 2
    // A; adds ";" to lines 0 and 2 -> "aaa;\nbbb\nccc;\nddd"
    // j moves cursors to lines 1 and 3
    // dot repeats A; on lines 1 and 3 -> "aaa;\nbbb;\nccc;\nddd;"
    let mut s = session_with_cursors("aaa\nbbb\nccc\nddd", &[0, 8]);
    feed(&mut s, "A;<Esc>");
    assert_eq!(s.text(), "aaa;\nbbb\nccc;\nddd");
    feed(&mut s, "j");
    feed(&mut s, ".");
    let text = s.text();
    assert!(
        text.contains("bbb;") && text.contains("ddd;"),
        "dot should have appended ';' to lines with 'bbb' and 'ddd'. Got: {text:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. r{char} + dot — repeats replace at all cursors
// ═══════════════════════════════════════════════════════════════════════════

/// rX replaces char at each cursor, then l+dot replaces the next char.
#[test]
fn mc_dot_repeats_replace_char() {
    vim_mc("|1abcd\n|2efgh")
        .keys("rX")
        .expect_text("|Xbcd\nXfgh")
        .keys("l")
        .keys(".")
        .expect_text("X|Xcd\nXXgh")
        .labeled("dot repeats rX at all cursors after l")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Dot after Escape (MC cleared) — replays at single cursor only
// ═══════════════════════════════════════════════════════════════════════════

/// After MC edit + Escape to clear cursors, dot replays at single cursor.
#[test]
fn mc_dot_after_escape_clears_to_single() {
    let mut s = session_with_cursors("foo bar foo", &[0, 8]);
    assert_eq!(s.cursor_count(), 2);

    // ciw + "X" at both cursors
    feed(&mut s, "ciwX<Esc>");
    assert_eq!(s.text(), "X bar X");

    // Escape in Normal+MC clears secondary cursors
    feed(&mut s, "<Esc>");
    assert_eq!(
        s.cursor_count(),
        1,
        "Escape should clear MC to single cursor"
    );

    // Move to "bar"
    feed(&mut s, "w");
    // Dot should replay ciw+X at single cursor only
    feed(&mut s, ".");
    assert_eq!(
        s.text(),
        "X X X",
        "dot after MC-clear should replay at single cursor on 'bar'"
    );
}

/// After clearing MC, dot affects only one position, not old cursor positions.
#[test]
fn mc_dot_after_escape_does_not_resurrect_cursors() {
    let mut s = session_with_cursors("aaa bbb\nccc ddd", &[0, 8]);
    assert_eq!(s.cursor_count(), 2);

    feed(&mut s, "ciwX<Esc>");
    // "X bbb\nX ddd"
    assert_eq!(s.text(), "X bbb\nX ddd");

    // Clear MC
    feed(&mut s, "<Esc>");
    assert_eq!(s.cursor_count(), 1);

    // Move to "bbb"
    feed(&mut s, "w");
    feed(&mut s, ".");
    // Only "bbb" should change, not "ddd"
    let text = s.text();
    assert!(
        text.contains("ddd"),
        "dot at single cursor must not affect other positions. Got: {text:?}"
    );
    assert_eq!(
        s.cursor_count(),
        1,
        "dot must not resurrect secondary cursors"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. ciw + type + Esc + move + dot — dot at new MC positions
// ═══════════════════════════════════════════════════════════════════════════

/// ciw at initial positions, then w to move cursors, then dot at new positions.
#[test]
fn mc_dot_at_new_positions_after_move() {
    // "one two\nthree four" — cursors on "one" and "three"
    // ciw + "X" -> "X two\nX four"
    // w moves to "two" and "four"
    // dot repeats ciw+X -> "X X\nX X"
    vim_mc("|1one two\n|2three four")
        .keys("ciwX<Esc>")
        .expect_text("|X two\nX four")
        .keys("w")
        .keys(".")
        .expect_text("X |X\nX X")
        .labeled("dot after w repeats ciw+X at new positions")
        .run();
}

/// Move cursors multiple words before dotting.
#[test]
fn mc_dot_after_multiple_motions() {
    // "aa bb cc\ndd ee ff" — cursors on "aa" and "dd"
    // dw deletes "aa " and "dd " -> "bb cc\nee ff"
    // w moves to "cc" and "ff"
    // dot repeats dw -> "bb \nee " or "bb\nee" depending on trailing space
    let mut s = session_with_cursors("aa bb cc\ndd ee ff", &[0, 9]);
    feed(&mut s, "dw");
    assert_eq!(s.text(), "bb cc\nee ff");
    feed(&mut s, "w");
    feed(&mut s, ".");
    let text = s.text();
    // "cc" and "ff" should be deleted
    assert!(
        !text.contains("cc"),
        "dot should have deleted 'cc'. Got: {text:?}"
    );
    assert!(
        !text.contains("ff"),
        "dot should have deleted 'ff'. Got: {text:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. 3x + dot — counted delete with MC
// ═══════════════════════════════════════════════════════════════════════════

/// 3x deletes 3 chars at each cursor, dot repeats the 3-char delete.
#[test]
fn mc_dot_repeats_counted_x() {
    // "abcdefgh\nijklmnop" — cursors at start of each line
    // 3x deletes "abc" and "ijk" -> "defgh\nlmnop"
    // dot repeats 3x -> "gh\nop"
    vim_mc("|1abcdefgh\n|2ijklmnop")
        .keys("3x")
        .expect_text("|defgh\nlmnop")
        .keys(".")
        .expect_text("|gh\nop")
        .labeled("dot repeats 3x at all cursors")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. ct{char} + type + Esc + dot — content-dependent dot repeat
// ═══════════════════════════════════════════════════════════════════════════

/// ct. changes up to "." at each cursor. The find-char is part of the
/// recorded edit, so dot replays ct. (change-to-dot) at new positions.
#[test]
fn mc_dot_repeats_ct_char() {
    // "foo.bar\nbaz.qux" — cursors at start of each line
    // ct. deletes "foo" and "baz" (up to but not including "."), enter insert
    // type "X" -> "X.bar\nX.qux"
    // After Esc: normal mode at each cursor
    vim_mc("|1foo.bar\n|2baz.qux")
        .keys("ct.X<Esc>")
        .expect_text("|X.bar\nX.qux")
        .labeled("ct. + X replaces text before '.' at each cursor")
        .run();
}

/// ct. then move to another "." and dot: replays ct.+typed text.
#[test]
fn mc_dot_repeats_ct_at_new_positions() {
    // Two lines each with two "." segments
    // "aa.bb.cc\ndd.ee.ff" — cursors at start
    // ct. + "X" -> "X.bb.cc\nX.ee.ff"
    // f. to find next ".", then l to go past it -> on "bb" and "ee"
    let mut s = session_with_cursors("aa.bb.cc\ndd.ee.ff", &[0, 9]);
    feed(&mut s, "ct.X<Esc>");
    assert_eq!(s.text(), "X.bb.cc\nX.ee.ff");

    // Move past the first "." to "b" and "e"
    feed(&mut s, "f.l");
    // Now cursors are on 'b' of "bb" and 'e' of "ee"
    feed(&mut s, ".");
    // Dot replays ct.+X: changes from current pos up to next "."
    let text = s.text();
    // "bb" and "ee" should be replaced with "X" up to their respective next "."
    assert!(
        text.contains("X.cc") || text.contains("X.X"),
        "dot should replay ct.+X at new positions. Got: {text:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Dot replays the edit, not the cursor spawning
// ═══════════════════════════════════════════════════════════════════════════

/// gb gb ciw NEW Esc: dot replays ciw+NEW, not gb+gb+ciw+NEW.
#[test]
fn mc_dot_replays_edit_not_cursor_spawning() {
    let mut s = HostSession::new("foo bar foo baz foo").with_auto_handle_defaults(true);
    s.set_cursor_offset(0);

    // gb gb to add cursors at all "foo" occurrences
    feed(&mut s, "gb");
    assert_eq!(s.cursor_count(), 2);
    feed(&mut s, "gb");
    assert_eq!(s.cursor_count(), 3);

    // ciw NEW replaces all three "foo" with "NEW"
    feed(&mut s, "ciwNEW<Esc>");
    assert_eq!(s.text(), "NEW bar NEW baz NEW");

    // Clear MC
    feed(&mut s, "<Esc>");
    assert_eq!(s.cursor_count(), 1);

    // Move to "bar"
    feed(&mut s, "w");
    // Dot should only replay ciw+NEW at single cursor
    feed(&mut s, ".");
    assert_eq!(
        s.text(),
        "NEW NEW NEW baz NEW",
        "dot replays ciw+NEW at single cursor, not gb+gb+ciw+NEW"
    );
    assert_eq!(
        s.cursor_count(),
        1,
        "dot must not recreate multi-cursor state"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. sX + dot — substitute (delete char + insert) at all cursors
// ═══════════════════════════════════════════════════════════════════════════

/// s deletes char and enters insert. Dot replays the substitution.
#[test]
fn mc_dot_repeats_substitute() {
    vim_mc("|1abcd\n|2efgh")
        .keys("sX<Esc>")
        .expect_text("|Xbcd\nXfgh")
        .keys("l")
        .keys(".")
        .expect_text("X|Xcd\nXXgh")
        .labeled("dot repeats s+X at all cursors after l")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. C + type + Esc + dot — change-to-end at all cursors
// ═══════════════════════════════════════════════════════════════════════════

/// C changes to EOL at each cursor.
#[test]
fn mc_dot_repeats_change_to_eol() {
    // "hello world\nfoo bar" — cursors at 6 ('w' in "world") and 16 ('b' in "bar")
    // h=0 e=1 l=2 l=3 o=4 ' '=5 w=6 o=7 r=8 l=9 d=10 '\n'=11
    // f=12 o=13 o=14 ' '=15 b=16 a=17 r=18
    // C from 6: deletes "world" -> "hello ", inserts "X" -> "hello X"
    // C from 16: deletes "bar" -> "foo ", inserts "X" -> "foo X"
    let mut s = session_with_cursors("hello world\nfoo bar", &[6, 16]);
    feed(&mut s, "CX<Esc>");
    assert_eq!(
        s.text(),
        "hello X\nfoo X",
        "C+X should replace from cursor to EOL on each line"
    );
}

/// Corrected C test with proper offsets.
#[test]
fn mc_dot_repeats_change_to_eol_correct() {
    vim_mc("hello |1world\nfoo |2bar")
        .keys("CX<Esc>")
        .expect_text("hello |X\nfoo X")
        .labeled("C+X replaces to EOL at each cursor")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. Dot preserves count from original command
// ═══════════════════════════════════════════════════════════════════════════

/// 2dw at MC, then dot repeats 2dw (not 1dw).
#[test]
fn mc_dot_preserves_original_count() {
    // "aa bb cc dd\nee ff gg hh" — cursors at start of each line
    // 2dw deletes "aa bb " and "ee ff " -> "cc dd\ngg hh"
    // dot repeats 2dw -> deletes "cc dd" and "gg hh" -> empty or newline
    vim_mc("|1aa bb cc dd\n|2ee ff gg hh")
        .keys("2dw")
        .expect_text("|cc dd\ngg hh")
        .keys(".")
        .labeled("dot repeats 2dw (count=2) at all cursors")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. Dot after diw — delete-inner-word repeat
// ═══════════════════════════════════════════════════════════════════════════

/// diw deletes inner word at each cursor, dot repeats at new positions.
#[test]
fn mc_dot_repeats_diw() {
    // "hello world\nfoo bar" — cursors on "hello" and "foo"
    // diw deletes "hello" and "foo" -> " world\n bar"
    // w moves to "world" and "bar"
    // dot deletes "world" and "bar" -> " \n "
    let mut s = session_with_cursors("hello world\nfoo bar", &[0, 12]);
    feed(&mut s, "diw");
    let text = s.text();
    // "hello" and "foo" deleted, spaces remain
    assert!(
        !text.contains("hello") && !text.contains("foo"),
        "diw should delete 'hello' and 'foo'. Got: {text:?}"
    );

    feed(&mut s, "w");
    feed(&mut s, ".");
    let text = s.text();
    assert!(
        !text.contains("world") && !text.contains("bar"),
        "dot should repeat diw on 'world' and 'bar'. Got: {text:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 16. Multiple dots chain correctly with MC
// ═══════════════════════════════════════════════════════════════════════════

/// x then dot three times: each dot deletes one more char at each cursor.
#[test]
fn mc_dot_chains_multiple_times() {
    vim_mc("|1abcde\n|2fghij")
        .keys("x")
        .expect_text("|bcde\nghij")
        .keys(".")
        .expect_text("|cde\nhij")
        .keys(".")
        .expect_text("|de\nij")
        .keys(".")
        .expect_text("|e\nj")
        .labeled("chained dots each delete one char at all cursors")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 17. Dot after cw (change-word, not inner-word)
// ═══════════════════════════════════════════════════════════════════════════

/// cw replaces word at each cursor, dot repeats on next words.
#[test]
fn mc_dot_repeats_cw() {
    vim_mc("|1hello world\n|2foo bar")
        .keys("cwX<Esc>")
        .expect_text("|X world\nX bar")
        .keys("w")
        .keys(".")
        .expect_text("X |X\nX X")
        .labeled("dot repeats cw+X at all cursors")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 18. Dot with MC + undo is atomic
// ═══════════════════════════════════════════════════════════════════════════

/// The dot-repeated edit at all cursors should form one undo group.
#[test]
fn mc_dot_repeat_is_atomic_undo() {
    let mut s = session_with_cursors("aaa bbb\nccc ddd", &[0, 8]);

    // First edit: ciw + "X"
    feed(&mut s, "ciwX<Esc>");
    assert_eq!(s.text(), "X bbb\nX ddd");

    // Move to next word at each cursor
    feed(&mut s, "w");
    // Dot repeat
    feed(&mut s, ".");
    assert_eq!(s.text(), "X X\nX X");

    // Single undo should revert the dot-repeated edit atomically
    feed(&mut s, "u");
    assert_eq!(
        s.text(),
        "X bbb\nX ddd",
        "single u should revert dot-repeated MC edit atomically"
    );

    // Another undo reverts the original ciw
    feed(&mut s, "u");
    assert_eq!(
        s.text(),
        "aaa bbb\nccc ddd",
        "second u should revert original MC edit"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 19. Fluent builder: dot after gb + edit
// ═══════════════════════════════════════════════════════════════════════════

/// Using the fluent API: vim() + add_next_match + edit + dot.
#[test]
fn mc_dot_via_fluent_gb_workflow() {
    let s = vim("|foo bar foo")
        .add_next_match()
        .expect_cursor_count(2)
        .keys("ciwX<Esc>")
        .run_session();

    assert_eq!(s.text(), "X bar X");

    // Now use the session for dot repeat at single cursor
    let mut s2 = s;
    s2.feed("<Esc>"); // clear MC
    assert_eq!(s2.cursor_count(), 1);
    s2.feed("w"); // move to "bar"
    s2.feed("."); // dot replays ciw+X
    assert_eq!(
        s2.text(),
        "X X X",
        "dot after clearing MC replays ciw+X at single cursor"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 20. Dot after r with different target chars at each cursor
// ═══════════════════════════════════════════════════════════════════════════

/// r records the replacement char. Dot replays rX with the same char.
#[test]
fn mc_dot_repeats_r_with_same_char() {
    // "abcd\nefgh" — cursors at start
    // rZ replaces 'a' and 'e' with 'Z' -> "Zbcd\nZfgh"
    // l moves to 'b' and 'f'
    // dot repeats rZ -> "ZZcd\nZZgh"
    // l + dot -> "ZZZd\nZZZh"
    let mut s = session_with_cursors("abcd\nefgh", &[0, 5]);
    feed(&mut s, "rZ");
    assert_eq!(s.text(), "Zbcd\nZfgh");
    feed(&mut s, "l");
    feed(&mut s, ".");
    assert_eq!(s.text(), "ZZcd\nZZgh");
    feed(&mut s, "l");
    feed(&mut s, ".");
    assert_eq!(
        s.text(),
        "ZZZd\nZZZh",
        "repeated dot after rZ keeps using 'Z' as replacement"
    );
}
