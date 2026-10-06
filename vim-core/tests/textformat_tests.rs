//! Breaking lines while typing (`textwidth` with `formatoptions` `t`/`c`).
//!
//! `matches_vim_oracle` replays cases recorded from headless Vim 9.1 and
//! needs the same buffer and Insert-mode cursor. The cases cover plain text,
//! indent and tabs, wide characters, the `l`, `v`, `b`, `w`, `1` and `p`
//! flags, comment leaders, Replace mode, and the eight issue #77 scenarios
//! under three option sets. The fixture was recorded with `vim -Nu NONE -es`:
//! explicit `:setlocal` options, the keys through `:normal!`, and the cursor
//! read with `<C-R>=` right before `<Esc>`. An `InsertCharPre` autocommand
//! makes Vim take the keys one at a time, as typed. Without it Vim reads
//! plain characters ahead in one batch and never records where a blank was
//! typed, so every `b` case would show no break.
//!
//! `format_operator_matches_vim_oracle` does the same for `gq` and `gw`:
//! prose, joins, indent and `tabstop`, `#`, `##`, `//` and three-piece
//! comment blocks, leader changes, leader-only lines, comments after code,
//! the `2`, `w`, `1`, `p`, `M` and `B` flags, motions, Visual mode, where
//! `gw` leaves the cursor, and GDScript-like options. Its fixture was
//! recorded the same way, with `nojoinspaces` and the cursor read after
//! the keys.
//!
//! `format_gates_match_vim_oracle` replays cases recorded from interactive
//! Vim 9.1 through `feedkeys()`, as undo puts the cursor back differently
//! under `-es`: `u` after `gq` and `gw`, motions that fail and cancel them,
//! where their range ends, text objects, the empty last line, and the
//! repeats of an insert.
//!
//! The other tests check the typing paths (repeats, undo, literal
//! characters, abbreviations, multiple cursors) against Vim, and that typing
//! never loses, adds or reorders a character.

use proptest::prelude::*;
use serde::Deserialize;
use vim_core::execution::{parse_keys_from_string, HostSession};
use vim_core::keymap::KeyEvent;
use vim_core::primitives::VimOptions;

// ── Helpers ──────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct Fixture {
    cases: Vec<OracleCase>,
}

#[derive(Debug, Deserialize)]
struct OracleCase {
    name: String,
    lines: Vec<String>,
    cursor: [usize; 2],
    cmd: String,
    typed: String,
    opts: Opts,
    expected: Expected,
}

#[derive(Debug, Deserialize)]
struct FormatFixture {
    cases: Vec<FormatCase>,
}

#[derive(Debug, Deserialize)]
struct FormatCase {
    name: String,
    lines: Vec<String>,
    cursor: [usize; 2],
    keys: String,
    opts: Opts,
    expected: Expected,
}

#[derive(Debug, Deserialize)]
struct Opts {
    tw: usize,
    fo: String,
    ts: usize,
    sw: usize,
    et: bool,
    ai: bool,
    com: String,
}

#[derive(Debug, Deserialize)]
struct Expected {
    lines: Vec<String>,
    cursor: [usize; 2],
}

fn options(o: &Opts) -> VimOptions {
    let mut opts = VimOptions::default();
    opts.set_textwidth(o.tw);
    opts.set_formatoptions(o.fo.as_str());
    opts.set_comments(o.com.as_str());
    opts.set_tabstop(o.ts);
    opts.set_shiftwidth(o.sw);
    opts.set_expandtab(o.et);
    opts.set_autoindent(o.ai);
    opts.set_smartindent(false);
    opts.set_auto_pairs(None);
    opts
}

fn offset_of(text: &str, [row, col]: [usize; 2]) -> usize {
    text.split('\n')
        .take(row)
        .map(|l| l.len() + 1)
        .sum::<usize>()
        + col
}

fn position_of(text: &str, offset: usize) -> [usize; 2] {
    let before = &text[..offset];
    let row = before.matches('\n').count();
    let col = before.rfind('\n').map_or(offset, |nl| offset - nl - 1);
    [row, col]
}

/// Feed literal text as typed keys; `\r` is Enter.
fn type_text(session: &mut HostSession, text: &str) {
    for c in text.chars() {
        let key = if c == '\r' {
            KeyEvent::enter()
        } else {
            KeyEvent::char(c)
        };
        session.process_key_host(key);
    }
}

fn feed(session: &mut HostSession, keys: &str) {
    for key in parse_keys_from_string(keys) {
        session.process_key_host(key);
    }
}

/// A session over `text` with `tw`, `fo` and Vim's defaults for the rest
/// (`noautoindent`, `noexpandtab`, `tabstop=8`).
fn session(text: &str, tw: usize, fo: &str) -> HostSession {
    let mut session = HostSession::new(text);
    session.set_options(options(&Opts {
        tw,
        fo: fo.to_owned(),
        ts: 8,
        sw: 8,
        et: false,
        ai: false,
        com: "s1:/*,mb:*,ex:*/,://,b:#,:%,:XCOMM,n:>,fb:-".to_owned(),
    }));
    session
}

fn show(lines: &[String]) -> String {
    lines
        .iter()
        .map(|l| format!("    {:?}", l))
        .collect::<Vec<_>>()
        .join("\n")
}

// ── Vim oracle ───────────────────────────────────────────────────────────────

#[test]
fn matches_vim_oracle() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("fixtures/textformat_oracle.json")).unwrap();
    assert!(!fixture.cases.is_empty());
    let mut failures = Vec::new();
    for case in &fixture.cases {
        let text = case.lines.join("\n");
        let mut session = HostSession::new(&text);
        session.set_options(options(&case.opts));
        session.set_cursor_offset(offset_of(&text, case.cursor));
        type_text(&mut session, &case.cmd);
        type_text(&mut session, &case.typed);
        let cursor = position_of(session.text(), session.cursor_offset());
        session.process_key_host(KeyEvent::escape());
        let lines: Vec<String> = session.text().split('\n').map(str::to_owned).collect();
        if lines != case.expected.lines || cursor != case.expected.cursor {
            failures.push(format!(
                "{}\n  expected cursor {:?}:\n{}\n  actual cursor {:?}:\n{}",
                case.name,
                case.expected.cursor,
                show(&case.expected.lines),
                cursor,
                show(&lines),
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} oracle cases differ from Vim:\n\n{}",
        failures.len(),
        fixture.cases.len(),
        failures.join("\n\n")
    );
}

#[test]
fn format_operator_matches_vim_oracle() {
    check_format_fixture(include_str!("fixtures/format_operator_oracle.json"));
}

#[test]
fn format_gates_match_vim_oracle() {
    check_format_fixture(include_str!("fixtures/format_gate_oracle.json"));
}

/// Replay the `keys` of every case in a format fixture and compare the
/// buffer and the cursor after them with Vim's.
fn check_format_fixture(json: &str) {
    let fixture: FormatFixture = serde_json::from_str(json).unwrap();
    assert!(!fixture.cases.is_empty());
    let mut failures = Vec::new();
    for case in &fixture.cases {
        let text = case.lines.join("\n");
        let mut session = HostSession::new(&text);
        session.set_options(options(&case.opts));
        session.set_cursor_offset(offset_of(&text, case.cursor));
        feed(&mut session, &case.keys);
        let cursor = position_of(session.text(), session.cursor_offset());
        let lines: Vec<String> = session.text().split('\n').map(str::to_owned).collect();
        if lines != case.expected.lines || cursor != case.expected.cursor {
            failures.push(format!(
                "{} ({})\n  expected cursor {:?}:\n{}\n  actual cursor {:?}:\n{}",
                case.name,
                case.keys,
                case.expected.cursor,
                show(&case.expected.lines),
                cursor,
                show(&lines),
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} format operator cases differ from Vim:\n\n{}",
        failures.len(),
        fixture.cases.len(),
        failures.join("\n\n")
    );
}

// ── Typing paths (expected values from Vim 9.1) ──────────────────────────────

#[test]
fn dot_repeat_breaks_the_replayed_text() {
    let mut s = session("first\nsecond", 20, "tq");
    feed(&mut s, "Aaa bb cc dd ee ff gg<Esc>G.");
    assert_eq!(
        s.text(),
        "firstaa bb cc dd ee\nff gg\nsecondaa bb cc dd ee\nff gg"
    );
    assert_eq!(position_of(s.text(), s.cursor_offset()), [3, 4]);
}

#[test]
fn dot_repeat_of_insert_before_text() {
    let mut s = session("xx\nyy", 20, "tq");
    feed(&mut s, "iaa bb cc dd ee ff gg hh <Esc>j0.");
    assert_eq!(
        s.text(),
        "aa bb cc dd ee ff gg\nhh xx\naa bb cc dd ee ff gg\nhh yy"
    );
    assert_eq!(position_of(s.text(), s.cursor_offset()), [3, 2]);
}

#[test]
fn count_repeat_breaks_like_typing() {
    let mut s = session("", 20, "tq");
    feed(&mut s, "3iabc def <Esc>");
    assert_eq!(s.text(), "abc def abc def abc\ndef ");
    assert_eq!(position_of(s.text(), s.cursor_offset()), [1, 3]);
}

#[test]
fn count_repeat_keeps_the_indent_with_autoindent() {
    let mut s = session("    x", 20, "tq");
    s.options_mut().set_autoindent(true);
    feed(&mut s, "$3iabc def <Esc>");
    assert_eq!(s.text(), "    abc def abc def\n    abc def x");
    assert_eq!(position_of(s.text(), s.cursor_offset()), [1, 11]);
}

#[test]
fn replace_dot_repeat_breaks_where_it_appends() {
    // Vim retypes the text in Replace mode, which formats once the text
    // runs past the end of the line.
    for text in ["aa\nbb", "aaaa bbbb\ncccc dddd"] {
        let mut s = session(text, 12, "tq");
        feed(&mut s, "Rxx yy zz ww vv<Esc>j0.");
        assert_eq!(s.text(), "xx yy zz ww\nvv\nxx yy zz ww\nvv", "{text:?}");
        assert_eq!(position_of(s.text(), s.cursor_offset()), [3, 1]);
    }
}

#[test]
fn replace_dot_repeat_does_not_break_while_it_overwrites() {
    let mut s = session("aaaaaaaaaaaa\nbbbbbbbbbbbbbbbbbbbb", 12, "tq");
    feed(&mut s, "Rxx yy zz ww vv<Esc>j0.");
    assert_eq!(s.text(), "xx yy zz ww\nvv\nxx yy zz ww vvbbbbbb");
}

#[test]
fn counted_replace_breaks_the_repeats() {
    let mut s = session("aa\nbb", 12, "tq");
    feed(&mut s, "3Rxx yy <Esc>");
    assert_eq!(s.text(), "xx yy xx yy\nxx yy \nbb");
    assert_eq!(position_of(s.text(), s.cursor_offset()), [1, 5]);
}

#[test]
fn undo_removes_the_text_and_the_breaks_in_one_step() {
    let mut s = session("aaaa bbbb cccc", 20, "tq");
    feed(&mut s, "A dddd eeee ffff gggg<Esc>");
    assert_eq!(s.text(), "aaaa bbbb cccc dddd\neeee ffff gggg");
    feed(&mut s, "u");
    assert_eq!(s.text(), "aaaa bbbb cccc");
    feed(&mut s, "<C-r>");
    assert_eq!(s.text(), "aaaa bbbb cccc dddd\neeee ffff gggg");
}

#[test]
fn literal_character_breaks_like_a_typed_one() {
    let mut s = session("aaaa bbbb cccc dddd", 20, "tq");
    feed(&mut s, "A <C-v>a<C-v>b");
    assert_eq!(s.text(), "aaaa bbbb cccc dddd\nab");
    assert_eq!(position_of(s.text(), s.cursor_offset()), [1, 2]);
}

#[test]
fn copied_character_does_not_break() {
    // Vim turns 'textwidth' off while CTRL-Y and CTRL-E insert the copy.
    let mut s = session("aaaa bbbb cccc ddddefgh\naaaa bbbb cccc dddd", 20, "tq");
    feed(&mut s, "jA<C-y><C-y>");
    assert_eq!(s.text(), "aaaa bbbb cccc ddddefgh\naaaa bbbb cccc ddddef");
}

#[test]
fn abbreviation_expansion_is_formatted_as_typed() {
    let mut s = session("aaaa bbbb cccc", 20, "tq");
    feed(&mut s, ":iabbrev hw hello world<CR>A hw ");
    assert_eq!(s.text(), "aaaa bbbb cccc hello\nworld ");
    assert_eq!(position_of(s.text(), s.cursor_offset()), [1, 6]);
}

#[test]
fn abbreviation_with_non_blank_trigger() {
    let mut s = session("aaaa bbbb cccc dd", 20, "tq");
    feed(&mut s, ":iabbrev hw hello world<CR>A hw.");
    assert_eq!(s.text(), "aaaa bbbb cccc dd\nhello world.");
    assert_eq!(position_of(s.text(), s.cursor_offset()), [1, 12]);
}

#[test]
fn arrow_key_moves_the_insert_start() {
    // With 'l' the line the insert starts on is not broken when it was
    // already long. After an arrow key the insert starts again where the
    // cursor is, on a line that is short.
    let mut s = session("aaaa bbbb cccc dddd eeee\nxx", 20, "tql");
    feed(&mut s, "A ffff");
    assert_eq!(s.text(), "aaaa bbbb cccc dddd eeee ffff\nxx");
    feed(&mut s, "<Down><End> yy zz ww vv uu tt ss");
    assert_eq!(
        s.text(),
        "aaaa bbbb cccc dddd eeee ffff\nxx yy zz ww vv uu tt\nss"
    );
}

#[test]
fn backspace_to_the_line_above_keeps_the_long_line_start() {
    // Vim's ins_bs() moves the insert start to the end of the line above
    // when a backspace joins it, and keeps the length 'l' compares, so a
    // long start line still keeps the line above from breaking.
    for (enter, text) in [
        ("R", " bit long./\n# More doc."),
        ("i", " bit long./# More doc."),
    ] {
        let mut s = session(" bit long.\n# More doc.", 10, "tql");
        feed(&mut s, &format!("G0{enter}<BS>/<Esc>"));
        assert_eq!(s.text(), text, "{enter}");
        assert_eq!(position_of(s.text(), s.cursor_offset()), [0, 10], "{enter}");

        let mut s = session(" bit long.\n# More doc.", 10, "tq");
        feed(&mut s, &format!("G0{enter}<BS>/<Esc>"));
        assert!(
            s.text().starts_with(" bit\nlong./"),
            "{enter}: {:?}",
            s.text()
        );
    }
}

// ── Multiple cursors ─────────────────────────────────────────────────────────

#[test]
fn every_cursor_breaks_its_own_line() {
    let mut s = session("aaaa bbbb cccc dddd\nxxxx yyyy zzzz wwww", 20, "tq");
    feed(&mut s, "$");
    s.add_cursor(38).unwrap();
    feed(&mut s, "a eeee");
    assert_eq!(
        s.text(),
        "aaaa bbbb cccc dddd\neeee\nxxxx yyyy zzzz wwww\neeee"
    );
    feed(&mut s, "<Esc>");
    assert_eq!(
        s.text(),
        "aaaa bbbb cccc dddd\neeee\nxxxx yyyy zzzz wwww\neeee"
    );
}

#[test]
fn every_cursor_keeps_typing_after_the_break() {
    let mut s = session("aaaa bbbb cccc dddd\nxxxx yyyy zzzz wwww", 20, "tq");
    feed(&mut s, "$");
    s.add_cursor(38).unwrap();
    feed(&mut s, "a eeee ffff gggg hhhh iiii jjjj<Esc>");
    assert_eq!(
        s.text(),
        "aaaa bbbb cccc dddd\neeee ffff gggg hhhh\niiii jjjj\n\
         xxxx yyyy zzzz wwww\neeee ffff gggg hhhh\niiii jjjj"
    );
}

#[test]
fn every_cursor_formats_as_if_typing_alone() {
    // `A` on the last line leaves that cursor past the end of the buffer.
    // It still breaks its line.
    let mut s = session("words\n//g enough", 10, "tcq");
    s.set_cursor_offset(3);
    s.add_cursor(15).unwrap();
    feed(&mut s, "Ac<Esc>");
    assert_eq!(s.text(), "wordsc\n//g\n//enoughc");

    // A break that adds a comment leader moves the cursors below it on.
    let mut s = session("// g h\nk\nr", 5, "tcq");
    s.set_cursor_offset(5);
    s.add_cursor(7).unwrap();
    s.add_cursor(9).unwrap();
    feed(&mut s, "if <Esc>");
    assert_eq!(s.text(), "// g\n// f h\nf k\nf r");

    // Each cursor repeats a counted insert and breaks its own line.
    let mut s = session("t\n  three", 10, "tcq");
    s.set_cursor_offset(0);
    s.add_cursor(8).unwrap();
    feed(&mut s, "2a w<Esc>");
    assert_eq!(s.text(), "t w w\n  three w\nw");

    // And a dot-repeat.
    let mut s = session("t\ns\ns", 5, "tcq");
    s.set_cursor_offset(0);
    s.add_cursor(2).unwrap();
    s.add_cursor(4).unwrap();
    feed(&mut s, "ilor //<CR><Esc>.");
    assert_eq!(
        s.text(),
        "lor\n//\nlor\n//\nt\nlor\n//\nlor\n//\ns\nlor\n//\nlor\n//\ns"
    );
}

#[test]
fn every_cursor_enters_and_leaves_insert_as_if_alone() {
    // `A` puts every cursor at the end of its own line.
    let mut s = session("h\n//hat is long enough", 20, "tcq");
    s.set_cursor_offset(0);
    s.add_cursor(9).unwrap();
    feed(&mut s, "Ax<Esc>");
    assert_eq!(s.text(), "hx\n//hat is long\n//enoughx");

    // `<Esc>` leaves every cursor on the last character it typed, and `I`
    // puts it on the first non-blank of its own line.
    let mut s = session("line one two three four\n// that is long enough", 5, "tcq");
    s.set_cursor_offset(22);
    s.add_cursor(45).unwrap();
    feed(&mut s, "i#<Esc>Ib<Esc>");
    assert_eq!(
        s.text(),
        "line\none\ntwo\nthree\nbfou#r\n// that\n// is\n// long\nb// enoug#h"
    );
    let mut s = session("t\n    line two ur", 20, "tcq");
    let mut opts = s.options().clone();
    opts.set_autoindent(true);
    s.set_options(opts);
    s.set_cursor_offset(0);
    s.add_cursor(14).unwrap();
    feed(&mut s, "i x 日本 #<Esc> bar<Esc>");
    assert_eq!(s.text(), " x 日本 #rt\n    line two x 日本\n    #rur");

    // After a counted insert and a dot-repeat every cursor is on the last
    // character it repeated.
    let mut s = session("h\n", 10, "tcq");
    s.set_cursor_offset(0);
    s.add_cursor(2).unwrap();
    feed(&mut s, "3Itic <Esc>I<BS><Esc>");
    assert_eq!(s.text(), "tic tictic h\ntic tictic ");
    let mut s = session("g\n\nt", 5, "tcq");
    s.set_cursor_offset(0);
    s.add_cursor(2).unwrap();
    s.add_cursor(3).unwrap();
    feed(&mut s, "ic 本<Esc>.I<BS><Esc>");
    assert_eq!(s.text(), "c c本本g\nc c本本\nc c本本t");
}

#[test]
fn every_cursor_has_its_own_insert_start() {
    // Each cursor formats as the same keys typed at that cursor alone. With
    // 'l' a line that was already long when the insert started does not
    // break, also under a cursor that is not the primary one.
    let mut s = session("rrr sss ttt\nrrr sss ttt", 10, "tql");
    feed(&mut s, "$");
    s.add_cursor(22).unwrap();
    feed(&mut s, "ar<Esc>");
    assert_eq!(s.text(), "rrr sss tttr\nrrr sss tttr");

    // The start stays with the cursor from one typed character to the next.
    let mut s = session("rrr sss ttt\nrrr sss ttt", 10, "tql");
    feed(&mut s, "$");
    s.add_cursor(22).unwrap();
    feed(&mut s, "a r s<Esc>");
    assert_eq!(s.text(), "rrr sss ttt r s\nrrr sss ttt r s");

    // After a typed line break the cursor is on another line than the
    // insert start, so 'l' no longer keeps the line, at every cursor.
    let mut s = session("rrr sss ttt\nrrr sss ttt", 10, "tql");
    feed(&mut s, "$");
    s.add_cursor(22).unwrap();
    feed(&mut s, "a<CR>aaa bbb ccc<Esc>");
    assert_eq!(
        s.text(),
        "rrr sss ttt\naaa bbb\nccc\nrrr sss ttt\naaa bbb\nccc"
    );

    // 'b' records the first blank typed on the start line per cursor.
    let mut s = session("aaa\naaa", 6, "tqb");
    feed(&mut s, "$");
    s.add_cursor(6).unwrap();
    feed(&mut s, "a bbbbbb<Esc>");
    assert_eq!(s.text(), "aaa\nbbbbbb\naaa\nbbbbbb");
}

#[test]
fn two_cursors_on_one_line_leave_it_unbroken() {
    // Breaking a line under another cursor on it would move that cursor's
    // text, so formatting skips lines shared by cursors.
    let mut s = session("aaaa bbbb cccc dddd", 20, "tq");
    feed(&mut s, "$");
    s.add_cursor(3).unwrap();
    feed(&mut s, "axy");
    assert_eq!(s.text(), "aaaaxy bbbb cccc ddddxy");
}

#[test]
fn shared_line_stays_unbroken_whichever_cursor_is_primary() {
    // On the first key after `i` the host cursor is the last cursor a
    // SetCursor put it on, which need not be the primary one yet. The line
    // must count as shared either way, or the blanks after one cursor are
    // deleted under the other cursor's text.
    for (primary, secondary) in [(22, 20), (20, 22)] {
        let mut s = session("aaaa bbbb cccc dddd     z", 20, "tq");
        s.options_mut().set_autoindent(true);
        s.set_cursor_offset(primary);
        s.add_cursor(secondary).unwrap();
        feed(&mut s, "iEF<Esc>");
        assert_eq!(
            s.text(),
            "aaaa bbbb cccc dddd EF  EF  z",
            "primary {primary}"
        );
    }
    for (primary, secondary) in [(32, 27), (27, 32)] {
        let mut s = session("    aaaa bbbb cccc dddd eeee ffff", 20, "tq");
        s.options_mut().set_autoindent(true);
        s.set_cursor_offset(primary);
        s.add_cursor(secondary).unwrap();
        feed(&mut s, "axyz<Esc>");
        assert_eq!(
            s.text(),
            "    aaaa bbbb cccc dddd eeeexyz ffffxyz",
            "primary {primary}"
        );
    }
}

// ── Property: breaks only replace blanks ─────────────────────────────────────

fn non_blank(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, ' ' | '\t' | '\n'))
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Typing never loses, adds or reorders a non-blank character, and the
    /// cursor ends right after the last typed character. With fo=tq no
    /// leader is inserted, so removing the inserted newlines and indent
    /// gives the same characters as plain insertion.
    #[test]
    fn typing_keeps_every_character(
        words in prop::collection::vec("[a-z]{1,9}", 0..6),
        typed in "[a-z日 ]{1,40}",
        tw in 1usize..30,
        ai in any::<bool>(),
        at_end in any::<bool>(),
    ) {
        let line = words.join(" ");
        let mut s = session(&line, tw, "tq");
        s.options_mut().set_autoindent(ai);
        feed(&mut s, if at_end { "A" } else { "i" });
        let before = s.text().to_owned();
        let cursor = s.cursor_offset();
        for c in typed.chars() {
            s.process_key_host(KeyEvent::char(c));
        }
        let mut plain = before.clone();
        plain.insert_str(cursor, &typed);
        prop_assert_eq!(non_blank(s.text()), non_blank(&plain));
        let after = &s.text()[..s.cursor_offset()];
        let last = typed.chars().next_back().unwrap();
        prop_assert!(after.ends_with(last), "cursor not after {:?}: {:?}", last, after);
        prop_assert_eq!(non_blank(after), non_blank(&plain[..cursor + typed.len()]));
    }
}
