use super::*;

/// Every line of `text`, as (number, text).
fn lines(text: &str) -> Vec<(u32, String)> {
    let mut script = Script::new(text);
    std::iter::from_fn(|| script.next_line())
        .map(|line| (line.number, line.text))
        .collect()
}

fn line(number: u32, text: &str) -> (u32, String) {
    (number, text.to_owned())
}

// ── lines of the file ─────────────────────────────────────────────────

#[test]
fn drops_a_byte_order_mark() {
    // As `do_source_ext()` does.
    assert_eq!(lines("\u{feff}set ic\n"), [line(1, "set ic")]);
    assert_eq!(
        lines("set ic\n\u{feff}set tw=1"),
        [line(1, "set ic"), line(2, "\u{feff}set tw=1")]
    );
}

#[test]
fn a_first_line_ending_in_cr_lf_drops_every_cr_before_lf() {
    assert_eq!(
        lines("set ic\r\nmap x y\r\nset tw=1\r"),
        [line(1, "set ic"), line(2, "map x y"), line(3, "set tw=1\r")]
    );
}

#[test]
fn a_first_line_ending_in_lf_keeps_every_cr() {
    // `get_one_sourceline()` strips a CR only once the first line ended in
    // CR LF, so the `^M` of a mapping stays.
    assert_eq!(
        lines("set ic\nmap x :help\r\nset tw=1\r\n"),
        [
            line(1, "set ic"),
            line(2, "map x :help\r"),
            line(3, "set tw=1\r")
        ]
    );
}

#[test]
fn a_lone_lf_after_cr_lf_gives_w15_once_and_keeps_later_crs() {
    let mut script = Script::new("a\r\nb\r\nc\nd\r\ne\n");
    let mut seen = Vec::new();
    while let Some(line) = script.next_line() {
        seen.push((line.number, line.text, script.take_warnings()));
    }
    assert_eq!(
        seen,
        [
            (1, "a".to_owned(), vec![]),
            (2, "b".to_owned(), vec![]),
            (3, "c".to_owned(), vec![(3, VimError::WrongLineSeparator)]),
            (4, "d\r".to_owned(), vec![]),
            (5, "e".to_owned(), vec![]),
        ]
    );
}

#[test]
fn a_ctrl_z_after_the_last_lf_of_a_dos_file_is_dropped() {
    assert_eq!(lines("set ic\r\n\u{1a}"), [line(1, "set ic")]);
    assert_eq!(
        lines("set ic\n\u{1a}"),
        [line(1, "set ic"), line(2, "\u{1a}")]
    );
    assert_eq!(
        lines("set ic\r\n\u{1a}\r\nx\r\n"),
        [line(1, "set ic"), line(2, "\u{1a}"), line(3, "x")]
    );
}

#[test]
fn an_odd_number_of_ctrl_v_before_lf_joins_the_next_line() {
    assert_eq!(
        lines("map x a\u{16}\nb\nset ic\n"),
        [line(1, "map x a\u{16}\nb"), line(3, "set ic")]
    );
    assert_eq!(
        lines("map x a\u{16}\u{16}\nb\n"),
        [line(1, "map x a\u{16}\u{16}"), line(2, "b")]
    );
    // In a DOS file the CR goes first.
    assert_eq!(
        lines("map x a\u{16}\r\nb\r\n"),
        [line(1, "map x a\u{16}\nb")]
    );
}

#[test]
fn a_nul_ends_the_text_of_its_line() {
    // `fgets()` reads the whole line, but Vim's line ends at the NUL.
    assert_eq!(
        lines("set ic\0 | set tw=1\nset et\n"),
        [line(1, "set ic"), line(2, "set et")]
    );
}

// ── continuation lines ────────────────────────────────────────────────

#[test]
fn joins_a_line_whose_first_non_blank_is_a_backslash() {
    assert_eq!(
        lines("nnoremap x\n  \\ :set ic<CR>\n\t\\\n"),
        [line(1, "nnoremap x :set ic<CR>")]
    );
}

#[test]
fn a_joined_line_has_the_number_of_its_first_line() {
    assert_eq!(
        lines("set\n\\ ic\n\\ et\nset tw=1\n"),
        [line(1, "set ic et"), line(4, "set tw=1")]
    );
}

#[test]
fn drops_a_continuation_comment() {
    // Inside a run and right after the line, as `getsourceline()` does.
    assert_eq!(
        lines("set\n\\ ic\n  \"\\ note\n\\ et\n\"\\ x\nset tw=1\n"),
        [line(1, "set ic et"), line(6, "set tw=1")]
    );
    assert_eq!(
        lines("set ic\n\"\\ note\nset et\n"),
        [line(1, "set ic"), line(3, "set et")]
    );
    // A tab after the backslash makes no comment.
    assert_eq!(
        lines("set ic\n\"\\\tnote\n"),
        [line(1, "set ic"), line(2, "\"\\\tnote")]
    );
}

#[test]
fn a_comment_takes_the_continuation_lines_after_it() {
    assert_eq!(lines("\" note\n\\ set ic\n"), [line(1, "\" note set ic")]);
}

#[test]
fn skips_only_spaces_and_tabs_before_a_backslash() {
    assert_eq!(
        lines("set ic\n\u{a0}\\ et\n"),
        [line(1, "set ic"), line(2, "\u{a0}\\ et")]
    );
}

#[test]
fn a_first_line_with_a_backslash_is_not_joined() {
    assert_eq!(lines("\\ set ic\n"), [line(1, "\\ set ic")]);
}

// ── heredocs ──────────────────────────────────────────────────────────

#[test]
fn finds_a_heredoc_after_the_variables_as_ex_let_does() {
    for (arg, start) in [
        ("x =<< END", 5),
        ("x=<<END", 4),
        ("[a, b] =<< END", 10),
        ("@a =<< END", 6),
        ("&l:tw =<< END", 9),
        ("$HOME =<< END", 9),
        ("g:d.key =<< END", 11),
        ("x[0] =<< END", 8),
    ] {
        assert_eq!(heredoc_start(arg), Some(start), "{arg}");
    }
    for arg in ["x = 1", "x .=<< END", "x = <<END", "x", ""] {
        assert_eq!(heredoc_start(arg), None, "{arg}");
    }
}
