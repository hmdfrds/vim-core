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

// ── bodies ────────────────────────────────────────────────────────────

/// What `text` runs: each command of each line, as the config loader runs
/// them, and the errors of the lines the commands read after their own.
fn run(text: &str) -> (Vec<String>, Vec<(u32, String)>) {
    let mut script = Script::new(text);
    let mut commands = Vec::new();
    let mut errors = Vec::new();
    while let Some(line) = script.next_line() {
        let mut next = Some(line);
        while let Some(line) = next.take() {
            for piece in crate::grammar::split_ex_pipeline(&line.text) {
                if let Some(body) = script.read_body(&piece, &line, false) {
                    errors.extend(body.errors.iter().map(|(n, e)| (*n, e.to_string())));
                    if body.fails {
                        break;
                    }
                    next = body.tail;
                }
                commands.push(piece.into_owned());
            }
        }
    }
    (commands, errors)
}

fn runs(text: &str) -> Vec<String> {
    let (commands, errors) = run(text);
    assert!(errors.is_empty(), "{text:?}: {errors:?}");
    commands
}

fn error(line: u32, text: &str) -> (u32, String) {
    (line, text.to_owned())
}

#[test]
fn a_let_heredoc_takes_the_lines_up_to_its_marker() {
    assert_eq!(
        runs("let x =<< END\nset ic\n  END\nEND\nset tw=1\n"),
        ["let x =<< END", "set tw=1"]
    );
    assert_eq!(
        runs("const x =<< eval END\nset ic\nEND\nset tw=1\n"),
        ["const x =<< eval END", "set tw=1"]
    );
    assert_eq!(
        runs("silent! let g:x=<<END \" c\nset ic\nEND\n"),
        ["silent! let g:x=<<END \" c"]
    );
}

#[test]
fn a_trim_heredoc_ends_at_its_marker_after_the_indent_of_its_line() {
    // `heredoc_get()` takes the indent of the `:let` line, not any indent.
    assert_eq!(
        runs("  let x =<< trim END\n    END\n\tEND\nset ic\n  END\nset tw=1\n"),
        ["let x =<< trim END", "set tw=1"]
    );
    assert_eq!(
        runs("  let x =<< trim END\nset ic\nEND\nset tw=1\n"),
        ["let x =<< trim END", "set tw=1"]
    );
}

#[test]
fn a_heredoc_reads_continuation_lines_as_they_are() {
    assert_eq!(
        runs("let x =<< END\na\n\\ set ic\n\"\\ x\nEND\nset tw=1\n"),
        ["let x =<< END", "set tw=1"]
    );
}

#[test]
fn a_continuation_line_joins_the_line_of_a_heredoc() {
    // `*line-continuation*`: the first text line joins the `:let` line,
    // which then has text after its marker.
    assert_eq!(
        run("let x =<< END\n\\ a\nEND\nset ic\n"),
        (
            vec!["set ic".to_owned()],
            vec![error(1, "E488: Trailing characters:  a")]
        )
    );
}

#[test]
fn a_let_heredoc_with_a_wrong_marker_fails() {
    assert_eq!(
        run("let x =<<\nset ic\n"),
        (
            vec!["set ic".to_owned()],
            vec![error(1, "E172: Missing marker")]
        )
    );
    assert_eq!(
        run("let x =<< \" c\nset ic\n"),
        (
            vec!["set ic".to_owned()],
            vec![error(1, "E172: Missing marker")]
        )
    );
    // Deviation: Vim does not read the text; here it is skipped.
    assert_eq!(
        run("set et\nlet x =<< end\nset ic\nend\nset tw=1\n"),
        (
            vec!["set et".to_owned(), "set tw=1".to_owned()],
            vec![error(2, "E221: Marker cannot start with lower case letter")]
        )
    );
    assert_eq!(
        run("let x =<< END x | set ic\nset et\nEND\n"),
        (
            vec![],
            vec![error(1, "E488: Trailing characters:  x | set ic")]
        )
    );
    // `vim_islower()` reads the first byte of `é` as Latin-1 `Ã`.
    assert!(run("let x =<< éND\néND\n").1.is_empty());
}

#[test]
fn a_heredoc_without_its_end_marker_gives_e990_after_the_last_line() {
    assert_eq!(
        run("set ic\nlet x =<< END\nset tw=1\n"),
        (
            vec!["set ic".to_owned(), "let x =<< END".to_owned()],
            vec![error(4, "E990: Missing end marker 'END'")]
        )
    );
    assert_eq!(
        run("lua << EOF"),
        (
            vec!["lua << EOF".to_owned()],
            vec![error(2, "E990: Missing end marker 'EOF'")]
        )
    );
}

#[test]
fn a_script_heredoc_takes_the_lines_up_to_its_marker_or_a_dot() {
    for (opener, end) in [
        ("lua << EOF", "EOF"),
        ("py3 << eof", "eof"),
        ("python3 <<", "."),
        ("pythonx << trim", "."),
        ("perl <<\" c", "."),
        ("rub << trim eval X", "X"),
        ("tcl<<X", "X"),
        ("mzscheme << X", "X"),
        ("1,2lua << X", "X"),
    ] {
        let text = format!("{opener}\nx = 1\n{end}\nset tw=1\n");
        assert_eq!(runs(&text), [opener, "set tw=1"], "{opener}");
    }
    assert_eq!(runs("lua print(1)\nset ic\n"), ["lua print(1)", "set ic"]);
}

#[test]
fn a_function_body_is_not_run() {
    assert_eq!(
        runs("function! F()\n  set ic\nendfunction\nset tw=1\n"),
        ["function! F()", "set tw=1"]
    );
    assert_eq!(
        runs("fu F(a, b) abort\nset ic\nendf\nset tw=1\n"),
        ["fu F(a, b) abort", "set tw=1"]
    );
    assert_eq!(
        runs("def F(): number\nset ic\nenddef\nset tw=1\n"),
        ["def F(): number", "set tw=1"]
    );
}

#[test]
fn a_function_without_a_paren_takes_no_body() {
    assert_eq!(runs("function Foo\nset ic\n"), ["function Foo", "set ic"]);
    assert_eq!(runs("function\nset ic\n"), ["function", "set ic"]);
    assert_eq!(runs("function /F(\nset ic\n"), ["function /F(", "set ic"]);
}

#[test]
fn a_function_ends_at_endfunction_as_get_function_body_finds_it() {
    // Exact case, at least `endf`, and no `:` after it.
    assert_eq!(
        runs("function F()\nENDFUNCTION\nendfun: 1\nendfunctions\nend\n  :endfunction!\nset ic\n"),
        ["function F()", "set ic"]
    );
}

#[test]
fn counts_the_functions_defined_inside_a_function() {
    assert_eq!(
        runs("function F()\n  function! s:G(x)\n  endfunction\n  set ic\nendfunction\nset tw=1\n"),
        ["function F()", "set tw=1"]
    );
    // `function(` is a call, and `def` counts only inside `:def`.
    assert_eq!(
        runs("function F()\n  call function('x')\n  def G()\nendfunction\nset tw=1\n"),
        ["function F()", "set tw=1"]
    );
    assert_eq!(
        runs("def F()\n  def G()\n  enddef\n  set ic\nenddef\nset tw=1\n"),
        ["def F()", "set tw=1"]
    );
}

#[test]
fn counts_the_blocks_inside_a_function() {
    assert_eq!(
        runs("function F()\n  command! X {\n  endfunction\n  }\nendfunction\nset tw=1\n"),
        ["function F()", "set tw=1"]
    );
    assert_eq!(
        runs("function F()\n  let L = (x) => {\n  endfunction\n  }\nendfunction\nset tw=1\n"),
        ["function F()", "set tw=1"]
    );
}

#[test]
fn skips_the_heredocs_and_text_inside_a_function() {
    for inside in [
        "let x =<< END\nendfunction\nEND",
        "  let x =<< trim END\nendfunction\n  END",
        "le x =<< eval END\nendfunction\nEND",
        "python << \nendfunction\n.",
        "  py3 << trim EOF\nendfunction\n  EOF",
        "py3 << trim EOF\n  EOF\nendfunction\nEOF",
        "append\nendfunction\n.",
        "2ch\nendfunction\n.",
        "in\nendfunction\n.",
    ] {
        let text = format!("function F()\n{inside}\nendfunction\nset tw=1\n");
        assert_eq!(runs(&text), ["function F()", "set tw=1"], "{inside}");
    }
}

#[test]
fn a_bar_after_endfunction_runs_the_rest_of_its_line() {
    assert_eq!(
        runs("function F()\nset ic\nendfunction | set tw=1 | set et\nset sw=2\n"),
        ["function F()", "set tw=1", "set et", "set sw=2"]
    );
    assert_eq!(
        runs("function F()\nendfunction \" c | set ic\nendfunction x | set ic\n"),
        ["function F()", "endfunction x", "set ic"]
    );
}

#[test]
fn a_function_without_its_end_fails_at_its_line() {
    assert_eq!(
        run("set ic\nfunction F()\nset tw=1\n"),
        (
            vec!["set ic".to_owned()],
            vec![error(2, "E126: Missing :endfunction")]
        )
    );
    assert_eq!(
        run("def F()\nset tw=1\n"),
        (vec![], vec![error(1, "E1057: Missing :enddef")])
    );
    assert_eq!(
        run("function F()\nlet x =<< END\nendfunction\n"),
        (
            vec![],
            vec![error(1, "E1145: Missing heredoc end marker: END")]
        )
    );
    assert_eq!(
        run("function F()\nappend\nendfunction\n"),
        (
            vec![],
            vec![error(1, "E1145: Missing heredoc end marker: .")]
        )
    );
    assert_eq!(
        run("function F()\ncommand X {\nendfunction\n"),
        (
            vec![],
            vec![error(1, "E1171: Missing } after inline function")]
        )
    );
}

#[test]
fn fifty_nested_functions_give_e1058() {
    let mut text = String::new();
    for n in 0..=50 {
        text.push_str(&format!("function F{n}()\n"));
    }
    text.push_str(&"endfunction\n".repeat(51));
    // The 51st function is not counted, so one `:endfunction` is left.
    assert_eq!(
        run(&text),
        (
            vec!["endfunction".to_owned()],
            vec![error(51, "E1058: Function nesting too deep")]
        )
    );
}

#[test]
fn append_text_ends_at_a_dot() {
    for opener in [
        "append",
        "1append",
        "a!",
        "i",
        "insert",
        "c",
        "2,3change",
        "cha",
    ] {
        let text = format!("{opener}\nset ic\n .\n.\nset tw=1\n");
        assert_eq!(runs(&text), [opener, "set tw=1"], "{opener}");
    }
    assert_eq!(runs("changes\nset ic\n"), ["changes", "set ic"]);
    // `ex_append()` reads the text before the command after the bar runs.
    assert_eq!(
        runs("append | set et\nset ic\n.\nset tw=1\n"),
        ["append", "set et", "set tw=1"]
    );
    assert_eq!(runs("append\nset ic\n"), ["append"]);
}

#[test]
fn a_command_block_ends_at_a_closing_brace() {
    for opener in [
        "command! Foo {",
        "command -nargs=0 -bar Foo {",
        "command Foo { \" c",
        "autocmd BufRead * {",
        "au! BufRead,bufwrite *.c ++once {",
        "autocmd MyGroup BufRead *.c ++once nested {",
        "au User Foo {",
    ] {
        let text = format!("{opener}\n  set ic\n  | set et\n  }}\nset tw=1\n");
        assert_eq!(runs(&text), [opener, "set tw=1"], "{opener}");
    }
    for opener in [
        "command Foo {x}",
        "command Foo { }",
        "command Foo { \" }",
        "autocmd FileType c imap {",
        "au BufRead {",
    ] {
        let text = format!("{opener}\nset ic\n");
        assert_eq!(runs(&text), [opener, "set ic"], "{opener}");
    }
    assert_eq!(
        run("command Foo {\nset ic\n"),
        (
            vec!["command Foo {".to_owned()],
            vec![error(3, "E1026: Missing }")]
        )
    );
}

#[test]
fn loadkeymap_takes_the_rest_of_the_file() {
    assert_eq!(
        runs("set ic\nloadkeymap\na b\nset tw=1\n"),
        ["set ic", "loadkeymap"]
    );
    assert_eq!(runs("loadkeymap x\nset ic\n"), ["loadkeymap x", "set ic"]);
}

#[test]
fn a_command_that_does_not_run_reads_only_what_vim_still_reads() {
    // `do_one_cmd()` calls these commands while skipping, and no others.
    for (opener, end) in [
        ("let x =<< END", "END"),
        ("const x =<< END", "END"),
        ("lua << EOF", "EOF"),
        ("py3 <<", "."),
        ("function F()", "endfunction"),
        ("def F()", "enddef"),
    ] {
        let text = format!("{opener}\nset ic\n{end}\nset tw=1\n");
        let mut script = Script::new(&text);
        let line = script.next_line().unwrap();
        assert_eq!(
            script.read_body(opener, &line, true),
            Some(Body::default()),
            "{opener}"
        );
        assert_eq!(
            script.next_line().map(|line| line.text),
            Some("set tw=1".to_owned())
        );
    }
    for opener in [
        "pyx << EOF",
        "append",
        "command Foo {",
        "au BufRead * {",
        "loadkeymap",
    ] {
        let text = format!("{opener}\nset ic\n");
        let mut script = Script::new(&text);
        let line = script.next_line().unwrap();
        assert_eq!(script.read_body(opener, &line, true), None, "{opener}");
        assert_eq!(
            script.next_line().map(|line| line.text),
            Some("set ic".to_owned())
        );
    }
    // Reading gives its errors either way.
    let mut script = Script::new("let x =<<\nset ic\n");
    let line = script.next_line().unwrap();
    assert_eq!(
        script
            .read_body("let x =<<", &line, true)
            .map(|body| body.errors),
        Some(vec![(1, VimError::MissingMarker)])
    );
}

// ── commands a sourced file runs itself ───────────────────────────────

#[test]
fn finds_block_commands_past_modifiers_and_ranges() {
    // do_one_cmd() reads modifiers, the range, the name and a `!`.
    let command = script_command("  verbose silent! :elsei  x > 1 ").unwrap();
    assert_eq!(command.control, Control::Block(BlockKind::ElseIf));
    assert!(command.modifiers.contains(ModifierFlags::SILENT_BANG));
    assert!(!command.range && !command.bang);
    assert_eq!(command.argument, "x > 1");

    let command = script_command("2endif! x").unwrap();
    assert_eq!(command.control, Control::Block(BlockKind::EndIf));
    assert!(command.range && command.bang);
    assert_eq!(command.argument, "x");

    for (piece, control) in [
        ("fini", Control::Finish),
        ("brea", Control::Break),
        ("con", Control::Continue),
        ("endfo", Control::Block(BlockKind::EndFor)),
        ("cat /x/", Control::Block(BlockKind::Catch)),
    ] {
        assert_eq!(script_command(piece).map(|c| c.control), Some(control));
    }
    for piece in ["endfunction", "set ic", "\" if 1", "Foo"] {
        assert_eq!(script_command(piece), None, "{piece}");
    }
}
