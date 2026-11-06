//! Multi-cursor Insert against the composite of single-cursor runs.
//!
//! Each cursor sits on its own line. Running the keys with all cursors must
//! give the text that splicing together the single-cursor runs gives, each
//! run contributing only its own segment (no 'textwidth', so no formatting
//! reaches across segments). A case whose single run edits outside its
//! segment is NONLOCAL and not compared.

use std::cell::{Cell, RefCell};
use std::io::Write as _;
use std::sync::Once;

use vim_core::execution::{parse_keys_from_string, HostSession};
use vim_core::primitives::VimOptions;

thread_local! {
    static CAPTURING: Cell<bool> = const { Cell::new(false) };
    static LAST_PANIC: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Records panics raised inside `check` instead of printing them; the
/// test's own assertion failures still print.
fn install_panic_recorder() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if CAPTURING.with(Cell::get) {
                LAST_PANIC.with(|m| *m.borrow_mut() = info.to_string());
            } else {
                default(info);
            }
        }));
    });
}

struct Rng(u64);

impl Rng {
    // xorshift64
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn chance(&mut self, p: f64) -> bool {
        (self.next() % 10000) as f64 / 10000.0 < p
    }
    fn pick<'a>(&mut self, v: &[&'a str]) -> &'a str {
        v[self.below(v.len())]
    }
}

const LINES: &[&str] = &[
    "The quick brown fox",
    "    indented line one",
    "\tTabbed line two",
    "        deep indent here",
    "# hash comment",
    "short",
    "",
    "  two spaces",
    "héllo wörld 日本語",
];
const WORDS: &[&str] = &["foo", "bar", "x", "if {", "}", "é", "a b", "end."];
const SEED: u64 = 0x9e37_79b9_7f4a_7c15;
const FULL_CASES: usize = 5000;

#[derive(Debug, Clone)]
struct Case {
    lines: Vec<String>,
    rows: Vec<usize>,
    cols: Vec<usize>,
    keys: String,
    ai: bool,
    et: bool,
    ts: usize,
}

fn gen(r: &mut Rng) -> Case {
    let k = 2 + r.below(2);
    let sep = r.chance(0.5);
    let (mut lines, mut rows, mut cols) = (vec![], vec![], vec![]);
    for i in 0..k {
        if sep && i > 0 {
            lines.push(String::new());
        }
        rows.push(lines.len());
        let l = r.pick(LINES).to_owned();
        let starts: Vec<usize> = (0..l.len()).filter(|&b| l.is_char_boundary(b)).collect();
        cols.push(if starts.is_empty() {
            0
        } else {
            starts[r.below(starts.len())]
        });
        lines.push(l);
    }
    let mut keys = String::new();
    let n = 1 + r.below(3);
    for i in 0..n {
        if i > 0 && r.chance(0.25) {
            if r.chance(0.2) {
                keys.push('2');
            }
            keys.push('.');
            continue;
        }
        if r.chance(0.15) {
            keys.push_str(r.pick(&["2", "3"]));
        }
        keys.push_str(r.pick(&["i", "a", "A", "I", "o", "O", "cc", "S", "R"]));
        for _ in 0..(1 + r.below(6)) {
            let x = r.below(100);
            if x < 50 {
                keys.push_str(r.pick(WORDS));
                if r.chance(0.6) {
                    keys.push(' ');
                }
            } else if x < 70 {
                keys.push_str("<CR>");
            } else if x < 82 {
                keys.push_str("<BS>");
            } else if x < 92 {
                keys.push_str("<Tab>");
            } else {
                keys.push_str("  ");
            }
        }
        keys.push_str("<Esc>");
    }
    Case {
        lines,
        rows,
        cols,
        keys,
        ai: r.chance(0.6),
        et: r.chance(0.5),
        ts: if r.chance(0.5) { 4 } else { 8 },
    }
}

fn cases() -> Vec<Case> {
    let mut r = Rng(SEED);
    (0..FULL_CASES).map(|_| gen(&mut r)).collect()
}

fn offset(lines: &[String], row: usize, col: usize) -> usize {
    lines[..row].iter().map(|l| l.len() + 1).sum::<usize>() + col
}

fn run(case: &Case, which: &[usize]) -> String {
    let mut s = HostSession::new(&case.lines.join("\n"));
    let mut o = VimOptions::default();
    o.set_autoindent(case.ai);
    o.set_expandtab(case.et);
    o.set_tabstop(case.ts);
    o.set_shiftwidth(4);
    o.set_smartindent(false);
    o.set_auto_pairs(None);
    s.set_options(o);
    s.set_cursor_offset(offset(
        &case.lines,
        case.rows[which[0]],
        case.cols[which[0]],
    ));
    for &j in &which[1..] {
        s.add_cursor(offset(&case.lines, case.rows[j], case.cols[j]))
            .unwrap();
    }
    for k in parse_keys_from_string(&case.keys) {
        s.process_key_host(k);
    }
    s.text().to_owned()
}

/// Splices each single-cursor run's own segment; `None` when a run edits
/// outside its segment.
fn composite(case: &Case, singles: &[String]) -> Option<String> {
    let d = case.lines.join("\n");
    let starts: Vec<usize> = std::iter::once(0)
        .chain(
            case.rows[1..]
                .iter()
                .map(|&row| offset(&case.lines, row, 0)),
        )
        .collect();
    let mut ends = starts[1..].to_vec();
    ends.push(d.len());
    let mut exp = String::new();
    for (j, t) in singles.iter().enumerate() {
        let (pre, suf) = (&d[..starts[j]], &d[ends[j]..]);
        if t.len() < pre.len() + suf.len() || !t.starts_with(pre) || !t.ends_with(suf) {
            return None;
        }
        exp.push_str(&t[pre.len()..t.len() - suf.len()]);
    }
    Some(exp)
}

enum Outcome {
    Ok,
    Text { expected: String, multi: String },
    NonLocal,
    Panic(String),
}

fn check(case: &Case) -> Outcome {
    let k = case.rows.len();
    let all: Vec<usize> = (0..k).collect();
    CAPTURING.with(|c| c.set(true));
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let multi = run(case, &all);
        let singles: Vec<String> = (0..k).map(|j| run(case, &[j])).collect();
        (multi, singles)
    }));
    CAPTURING.with(|c| c.set(false));
    let Ok((multi, singles)) = res else {
        return Outcome::Panic(LAST_PANIC.with(|m| m.borrow().clone()));
    };
    match composite(case, &singles) {
        None => Outcome::NonLocal,
        Some(expected) if expected == multi => Outcome::Ok,
        Some(expected) => Outcome::Text { expected, multi },
    }
}

/// The whole generated set. Prints the tally; set `MC_COMPOSITE_OUT` to a
/// file path to get one line per non-OK case.
#[test]
#[ignore = "5000 cases, run on demand"]
fn multicursor_insert_full_composite() {
    install_panic_recorder();
    let mut out = std::env::var_os("MC_COMPOSITE_OUT").map(|p| std::fs::File::create(p).unwrap());
    let (mut ok, mut text, mut nonlocal, mut panics) = (0, 0, 0, 0);
    for (i, case) in cases().iter().enumerate() {
        let line = match check(case) {
            Outcome::Ok => {
                ok += 1;
                continue;
            }
            Outcome::NonLocal => {
                nonlocal += 1;
                format!("{i} NONLOCAL {case:?}")
            }
            Outcome::Text { expected, multi } => {
                text += 1;
                format!("{i} TEXT {case:?}\n  expected {expected:?}\n  multi    {multi:?}")
            }
            Outcome::Panic(msg) => {
                panics += 1;
                format!("{i} PANIC {case:?}\n  {msg}")
            }
        };
        if let Some(f) = out.as_mut() {
            writeln!(f, "{line}").unwrap();
        }
    }
    println!("OK {ok} TEXT {text} NONLOCAL {nonlocal} PANIC {panics}");
    assert_eq!(
        panics, 0,
        "OK {ok} TEXT {text} NONLOCAL {nonlocal} PANIC {panics}"
    );
}

/// With 'autoindent', each cursor's `A<CR><CR>z` keeps its own typed lines.
/// Vim: edit.c ins_eol() and open_line() in change.c; the indent of an empty
/// autoindented line is removed when another line is opened.
#[test]
fn autoindent_open_twice_three_cursors_matches_composite() {
    let case = Case {
        lines: vec!["x".into(), "    a".into(), "    b".into()],
        rows: vec![0, 1, 2],
        cols: vec![0, 4, 4],
        keys: "A<CR><CR>z<Esc>".into(),
        ai: true,
        et: false,
        ts: 8,
    };
    for which in [&[0usize, 1, 2][..], &[0, 1], &[1, 2]] {
        let sub = Case {
            lines: which.iter().map(|&j| case.lines[j].clone()).collect(),
            rows: (0..which.len()).collect(),
            cols: which.iter().map(|&j| case.cols[j]).collect(),
            ..case.clone()
        };
        let singles: Vec<String> = (0..which.len()).map(|j| run(&sub, &[j])).collect();
        let expected = composite(&sub, &singles).expect("single runs stay local");
        let all: Vec<usize> = (0..which.len()).collect();
        assert_eq!(run(&sub, &all), expected, "cursors on {:?}", sub.lines);
    }
}

/// A line break before an indent moves a cursor further than the start of
/// the next cursor's line; the cursors keep their order.
#[test]
fn autoindented_line_break_keeps_cursor_order() {
    assert_composite(&["    a", ""], &[0, 0], "Ax<CR>y<Esc>", true);
    assert_composite(
        &["  two spaces", ""],
        &[5, 0],
        "A<CR><CR>  foo x<Esc>",
        true,
    );
}

/// Checks `keys` with a cursor at `cols[i]` on line `i` against the
/// composite of the single-cursor runs.
fn assert_composite(lines: &[&str], cols: &[usize], keys: &str, ai: bool) {
    let case = Case {
        lines: lines.iter().map(|&l| l.to_owned()).collect(),
        rows: (0..lines.len()).collect(),
        cols: cols.to_vec(),
        keys: keys.to_owned(),
        ai,
        et: false,
        ts: 8,
    };
    let all: Vec<usize> = (0..lines.len()).collect();
    let singles: Vec<String> = all.iter().map(|&j| run(&case, &[j])).collect();
    let expected = composite(&case, &singles).expect("single runs stay local");
    assert_eq!(run(&case, &all), expected, "{keys:?} on {lines:?}");
}

fn session(text: &str, offsets: &[usize]) -> HostSession {
    let mut s = HostSession::new(text);
    s.set_cursor_offset(offsets[0]);
    for &o in &offsets[1..] {
        s.add_cursor(o).unwrap();
    }
    s
}

fn feed(s: &mut HostSession, keys: &str) {
    for k in parse_keys_from_string(keys) {
        s.process_key_host(k);
    }
}

/// A counted Replace repeats its text over the characters that follow,
/// one character for each typed one.
/// Vim: insert.txt, Replace mode; change.c ins_char_bytes() replaces one
/// character and edit.c ins_eol() replaces none.
#[test]
fn counted_replace_overwrites_characters() {
    let mut s = session("éééé", &[0]);
    feed(&mut s, "3Rxy<Esc>");
    assert_eq!(s.text(), "xyxyxy");

    let mut s = session("abcdef", &[0]);
    feed(&mut s, "2Rx<CR>y<Esc>");
    assert_eq!(s.text(), "x\nyx\nyef");
}

#[test]
fn append_and_insert_entries_match_composite() {
    let lines = ["日本語 x", "  ab", "", "héllo wörld 日本語"];
    for keys in ["ax<Esc>", "Ax<Esc>", "Ix<Esc>", "gIx<Esc>", "a<CR>y<Esc>"] {
        assert_composite(&lines, &[0, 3, 0, 1], keys, false);
    }
}

/// The cursors do the same whichever one the host cursor is on.
#[test]
fn insert_does_not_depend_on_the_primary_cursor() {
    for (keys, expected) in [
        ("Ay<Esc>x", "ab\ncd\nef"),
        ("iy<Esc>x", "ab\ncd\nef"),
        ("R<Esc>x", "b\nd\nf"),
        ("Ry<Esc>x", "b\nd\nf"),
        ("oy<Esc>x", "ab\n\ncd\n\nef\n"),
        ("A<Tab>y<Esc>.x", "ab  y  \ncd  y  \nef  y  "),
    ] {
        for offsets in [[0, 3, 6], [3, 0, 6], [6, 3, 0]] {
            let mut s = session("ab\ncd\nef", &offsets);
            feed(&mut s, keys);
            assert_eq!(s.text(), expected, "{keys:?} from {offsets:?}");
            assert_eq!(s.cursor_count(), 3, "{keys:?} from {offsets:?}");
        }
    }
}

#[test]
fn esc_steps_back_at_each_cursor() {
    assert_composite(&["é", "ab", "日本"], &[0, 0, 3], "a<Esc>x", false);
    assert_composite(&["日本", "ab"], &[3, 1], "i<Esc>x", false);
}

#[test]
fn open_and_substitute_entries_match_composite() {
    let lines = ["    a", "b", "\té"];
    for keys in ["ox<Esc>", "Ox<Esc>", "sé<Esc>", "Sx<Esc>", "2sx<Esc>"] {
        assert_composite(&lines, &[4, 0, 1], keys, true);
    }
}

/// A counted change that reaches the next cursor's line takes that cursor.
#[test]
fn counted_change_over_another_cursor_keeps_one() {
    let mut s = session("ab\ncd\nef\ngh", &[0, 3, 6]);
    feed(&mut s, "2SX<Esc>");
    assert_eq!(s.text(), "X\nX");
    assert_eq!(s.cursor_count(), 2);
}

#[test]
fn replace_mode_matches_composite() {
    let lines = ["éé", "ab", "日本"];
    for keys in ["Rxy<Esc>", "Rxyz<Esc>", "Rx<CR>y<Esc>"] {
        assert_composite(&lines, &[0, 0, 3], keys, false);
    }
}

#[test]
fn counted_insert_and_empty_open_match_composite() {
    let lines = ["日本", "    ab"];
    for keys in [
        "3ix<Esc>",
        "2Ay<Esc>",
        "oé<BS><Esc>",
        "3Ox<Esc>",
        "2Ré<Esc>",
    ] {
        assert_composite(&lines, &[3, 5], keys, true);
    }
    // Each cursor ends on its own last repeated character.
    assert_composite(&["a", "short"], &[0, 4], "3ax<CR>é<Esc>x", true);
}

#[test]
fn dot_repeat_matches_composite() {
    let lines = ["ab", "日本語", "  x"];
    for keys in [
        "Ax<Esc>0.",
        "ix<Esc>.",
        "ax<Esc>3.",
        "ox<Esc>.",
        "Rxé<Esc>2.",
    ] {
        assert_composite(&lines, &[0, 3, 2], keys, true);
    }
}

/// A repeat that would overwrite past the next cursor on the line leaves
/// that text to the next cursor.
#[test]
fn dot_repeat_of_replace_on_one_line_keeps_text_whole() {
    let mut s = session("abcdefgh", &[0, 3]);
    feed(&mut s, "Rxé<Esc>2.");
    let text = s.text().to_owned();
    assert!(s
        .cursor_positions()
        .iter()
        .all(|&(.., offset)| text.is_char_boundary(offset)));
    assert_eq!(s.cursor_count(), 2);
}
