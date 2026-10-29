//! A config file read and run as Vim sources one.

use vim_core::effects::Effect;
use vim_core::execution::{HostRequest, Response, VimEngine};

/// The text of each error the response shows.
fn errors(response: &Response) -> Vec<String> {
    response
        .effects()
        .iter()
        .filter_map(|e| match e {
            Effect::ShowError { error, .. } => Some(error.to_string()),
            _ => None,
        })
        .collect()
}

/// The command of each custom command the response sends to the host.
fn host_commands(response: &Response) -> Vec<String> {
    response
        .host_requests()
        .iter()
        .map(|request| match request {
            HostRequest::CustomExCommand { command, .. } => command.to_string(),
            other => format!("{:?}", other.kind()),
        })
        .collect()
}

#[test]
fn a_script_heredoc_never_runs_as_commands() {
    // The Lua line `x = 1` would be `:x`, which writes and quits.
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("lua << EOF\nx = 1\nEOF\nset tw=33\n");
    assert_eq!(host_commands(&response), ["lua << EOF"]);
    assert_eq!(engine.options().textwidth(), 33);
    assert!(errors(&response).is_empty(), "{:?}", errors(&response));
}

#[test]
fn a_let_heredoc_never_runs_as_commands() {
    let mut engine = VimEngine::new();
    engine.source_config_text("let x =<< END\nset tw=33\nEND\n");
    assert_eq!(engine.options().textwidth(), 0);
}

#[test]
fn a_let_heredoc_with_text_after_its_marker_fails() {
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("let x =<< END | set ic\nset tw=33\nEND\n");
    assert!(!engine.options().ignorecase());
    assert_eq!(engine.options().textwidth(), 0);
    assert_eq!(errors(&response), ["E488: Trailing characters:  | set ic"]);
    assert!(response.host_requests().is_empty());
}

#[test]
fn a_function_body_never_runs_and_the_rest_of_its_end_line_does() {
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("function! F()\n  set ic\nendfunction | set tw=33\n");
    assert!(!engine.options().ignorecase());
    assert_eq!(engine.options().textwidth(), 33);
    assert_eq!(host_commands(&response), ["function! F()"]);
}

#[test]
fn a_function_without_its_end_gives_e126_and_runs_nothing() {
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("function F()\nset ic\n");
    assert!(!engine.options().ignorecase());
    assert_eq!(errors(&response), ["E126: Missing :endfunction"]);
    assert!(response.host_requests().is_empty());

    let response = engine.source_config_text("silent! function F()\nset ic\n");
    assert!(errors(&response).is_empty(), "{:?}", errors(&response));
}

#[test]
fn a_skipped_block_still_reads_a_heredoc() {
    // do_one_cmd() calls :lua while skipping, and it reads its heredoc.
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("if 0\nlua << EOF\nset ic\nEOF\nendif\n");
    assert!(!engine.options().ignorecase());
    assert!(errors(&response).is_empty(), "{:?}", errors(&response));
    assert!(response.host_requests().is_empty());
}

#[test]
fn a_skipped_block_reads_no_append_text() {
    // do_one_cmd() does not call :append while skipping, so the :endif
    // after it closes the block.
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("if 0\nappend\nendif\nset ic\n.\n");
    assert!(engine.options().ignorecase());
    assert_eq!(errors(&response), ["E523: Not allowed here: ."]);
}

#[test]
fn append_text_never_runs_as_commands() {
    let mut engine = VimEngine::new();
    engine.source_config_text("1append\nset ic\n.\nset tw=33\n");
    assert!(!engine.options().ignorecase());
    assert_eq!(engine.options().textwidth(), 33);
}

#[test]
fn a_command_block_never_runs_as_commands() {
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("command! Foo {\n  set ic\n}\nset tw=33\n");
    assert!(!engine.options().ignorecase());
    assert_eq!(engine.options().textwidth(), 33);
    assert_eq!(host_commands(&response), ["command! Foo {"]);
}

#[test]
fn a_command_after_an_error_on_its_line_still_reads_a_heredoc() {
    // `do_one_cmd()` skips the rest of the line but still calls `:let`,
    // which reads its heredoc.
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("%d | let x =<< END\nset ic\nEND\nset tw=33\n");
    assert!(!engine.options().ignorecase());
    assert_eq!(engine.options().textwidth(), 33);
    assert_eq!(errors(&response).len(), 1, "{:?}", errors(&response));
    assert!(response.host_requests().is_empty());
}

#[test]
fn a_command_after_an_error_on_its_line_reads_no_append_text() {
    // Vim does not call `:append` while skipping, so its text runs.
    let mut engine = VimEngine::new();
    engine.source_config_text("%d | 1append\nset ic\n");
    assert!(engine.options().ignorecase());
}

#[test]
fn the_rest_of_an_end_line_after_an_error_still_reads_a_heredoc() {
    let mut engine = VimEngine::new();
    let response = engine.source_config_text(
        "%d | function F()\nendfunction | let x =<< END\nset ic\nEND\nset tw=33\n",
    );
    assert!(!engine.options().ignorecase());
    assert_eq!(engine.options().textwidth(), 33);
    assert_eq!(errors(&response), ["E523: Not allowed here: %d"]);
}

#[test]
fn an_error_ends_its_line() {
    // do_one_cmd() skips every command after an error on its line, :set
    // errors and those of plain :silent included.
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("set tw=abc | set ic\nsilent set nosuch | set ts=3\n");
    assert!(!engine.options().ignorecase());
    assert_ne!(engine.options().tabstop(), 3);
    assert_eq!(errors(&response).len(), 2, "{:?}", errors(&response));
}

#[test]
fn silent_bang_hides_an_error_and_skips_nothing() {
    // emsg() under :silent! neither shows the error nor counts it.
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("sil! %d | set tw=44\nsilent! source! x | set ic\n");
    assert_eq!(engine.options().textwidth(), 44);
    assert!(engine.options().ignorecase());
    assert!(errors(&response).is_empty(), "{:?}", errors(&response));
}

fn strings(texts: &[&str]) -> Vec<String> {
    texts.iter().map(|&text| text.to_owned()).collect()
}

/// The textwidth a config leaves, and the errors it shows.
fn textwidth_after(config: &str) -> (usize, Vec<String>) {
    let mut engine = VimEngine::new();
    let response = engine.source_config_text(config);
    (engine.options().textwidth(), errors(&response))
}

#[test]
fn if_runs_the_lines_of_its_true_branch() {
    // ex_if(), ex_else(), ex_endif()
    for (config, textwidth) in [
        ("if 0\nset tw=33\nendif\n", 0),
        ("if 0 | set tw=33 | endif\n", 0),
        ("if 1 | set tw=33 | endif\n", 33),
        ("if 1\nset tw=33\nelse\nset tw=44\nendif\n", 33),
        ("if 0\nset tw=33\nelse\nset tw=44\nendif\n", 44),
        (
            "if has('nvim')\nset tw=10\nelseif v:version >= 800\nset tw=20\nelse\nset tw=30\nendif\n",
            20,
        ),
        ("if 0\nif 1\nset tw=33\nendif\nendif\n", 0),
        ("if 1\nif 0\nelse\nset tw=33\nendif\nendif\n", 33),
        ("if 0\nset tw=1\nelsei 1\nset tw=2\nel\nset tw=3\nen\n", 2),
        ("if 1\nelseif nosuch\nset tw=33\nendif\n", 0),
        ("let x = 3\nif x > 2 && &tw == 0\nset tw=33\nendif\n", 33),
        ("if 0 \" c\nelse \" c\nset tw=33\nendif \" c\n", 33),
        ("silent! if 0\nelse\nset tw=33\nendif\n", 33),
    ] {
        assert_eq!(textwidth_after(config), (textwidth, vec![]), "{config:?}");
    }
}

#[test]
fn a_condition_that_fails_runs_no_branch() {
    // ex_if(): an error sets CSF_TRUE, so the block never gets active,
    // even under :silent!, which only hides the error.
    let config = "if Foo()\nset ic\nelse\nset tw=5\nendif\nset sw=3\n";
    let mut engine = VimEngine::new();
    let response = engine.source_config_text(config);
    assert!(!engine.options().ignorecase());
    assert_eq!(engine.options().textwidth(), 0);
    assert_eq!(engine.options().shiftwidth(), 3);
    assert_eq!(errors(&response), ["E117: Unknown function: Foo"]);

    let mut engine = VimEngine::new();
    let response = engine.source_config_text(&format!("silent! {config}"));
    assert!(!engine.options().ignorecase());
    assert_eq!(engine.options().textwidth(), 0);
    assert_eq!(engine.options().shiftwidth(), 3);
    assert!(errors(&response).is_empty(), "{:?}", errors(&response));

    let (textwidth, errors) =
        textwidth_after("if 0\nelseif 1.5\nset tw=1\nelse\nset tw=2\nendif\n");
    assert_eq!(textwidth, 0);
    assert_eq!(errors, ["E805: Using a Float as a Number"]);
}

#[test]
fn an_error_inside_a_block_skips_to_its_outermost_end() {
    // do_cmdline() clears did_emsg only when no block is open.
    let mut engine = VimEngine::new();
    let response = engine.source_config_text(
        "if 1\nif 1\nset tw=abc\nendif\nset ic\nendif\nset sw=3\nif 1 | set ts=abc | endif | set ts=3\n",
    );
    assert!(!engine.options().ignorecase());
    assert_eq!(engine.options().shiftwidth(), 3);
    assert_ne!(engine.options().tabstop(), 3);
    assert_eq!(errors(&response).len(), 2, "{:?}", errors(&response));
}

#[test]
fn silent_bang_inside_a_block_skips_nothing() {
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("if 1\nsilent! set tw=abc\nset ic\nendif\n");
    assert!(engine.options().ignorecase());
    assert!(errors(&response).is_empty(), "{:?}", errors(&response));
}

#[test]
fn a_skipped_block_runs_and_refuses_nothing() {
    let mut engine = VimEngine::new();
    let response = engine.source_config_text(
        "if 0\nwrite\nnosuch\n%d\necho nosuch\nlet x = nosuch\nfinish\nbreak\nendif\nset ic\n",
    );
    assert!(engine.options().ignorecase());
    assert!(errors(&response).is_empty(), "{:?}", errors(&response));
    assert!(response.host_requests().is_empty());
}

#[test]
fn an_unknown_command_in_a_skipped_block_takes_the_rest_of_its_line() {
    // do_one_cmd() sets no next command for a name it does not know, so
    // the :endif after it is lost.
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("if 0 | Foo | endif\nset ic\n");
    assert!(!engine.options().ignorecase());
    assert_eq!(errors(&response), ["E171: Missing :endif"]);
}

#[test]
fn an_open_block_at_the_end_of_the_file_is_an_error() {
    // do_cmdline(): one error, for the innermost block.
    for (config, expected) in [
        ("if 1\n", vec!["E171: Missing :endif"]),
        (
            "if 1\nwhile 1\n",
            vec![
                "E319: Sorry, the command is not available in this version: while 1",
                "E170: Missing :endwhile",
            ],
        ),
        (
            "for x in [1]\n",
            vec![
                "E319: Sorry, the command is not available in this version: for x in [1]",
                "E170: Missing :endfor",
            ],
        ),
        (
            "try\n",
            vec![
                "E319: Sorry, the command is not available in this version: try",
                "E600: Missing :endtry",
            ],
        ),
    ] {
        assert_eq!(
            textwidth_after(config),
            (0, strings(&expected)),
            "{config:?}"
        );
    }
}

#[test]
fn a_loop_or_try_block_never_runs() {
    // vim-core runs no loops and catches no exceptions: E319, and the
    // lines up to the end of the block are skipped.
    let mut engine = VimEngine::new();
    let response = engine.source_config_text(
        "wh 1\nset ic\nbrea\ncon\nendw\nset tw=7\n\
         try\nset ic\ncat /x/\nset tw=1\nfina\nset tw=2\nendt\nset sw=2\n\
         for x in [1]\nset ic\nendfo\nset ts=3\n",
    );
    assert!(!engine.options().ignorecase());
    assert_eq!(engine.options().textwidth(), 7);
    assert_eq!(engine.options().shiftwidth(), 2);
    assert_eq!(engine.options().tabstop(), 3);
    assert_eq!(
        errors(&response),
        [
            "E319: Sorry, the command is not available in this version: while 1",
            "E319: Sorry, the command is not available in this version: try",
            "E319: Sorry, the command is not available in this version: for x in [1]",
        ]
    );
}

#[test]
fn block_commands_out_of_place_are_errors() {
    // ex_else(), ex_endif(), ex_endwhile(), ex_break(), ex_continue(),
    // ex_catch(), ex_finally(), ex_endtry()
    for (config, error) in [
        ("if 1\nelseif\nendif\n", "E15: Invalid expression: \"\""),
        (
            "if 1\nelseif 1\nelseif\nendif\n",
            "E15: Invalid expression: \"\"",
        ),
        ("else\n", "E581: :else without :if"),
        ("elseif 1\n", "E582: :elseif without :if"),
        ("if 1\nelse\nelse\nendif\n", "E583: Multiple :else"),
        ("if 1\nelse\nelseif 1\nendif\n", "E584: :elseif after :else"),
        ("endif\n", "E580: :endif without :if"),
        ("endwhile\n", "E588: :endwhile without :while"),
        ("endfor\n", "E588: :endfor without :for"),
        ("break\n", "E587: :break without :while or :for"),
        ("continue\n", "E586: :continue without :while or :for"),
        ("catch\n", "E603: :catch without :try"),
        ("finally\n", "E606: :finally without :try"),
        ("endtry\n", "E602: :endtry without :try"),
        ("if 1\nendif x\nendif\n", "E488: Trailing characters: x"),
        ("if 1\nendif!\nendif\n", "E477: No ! allowed"),
        ("2endif\n", "E481: No range allowed"),
    ] {
        assert_eq!(
            textwidth_after(config),
            (0, vec![error.to_owned()]),
            "{config:?}"
        );
    }
}

#[test]
fn an_elseif_without_its_expression_is_an_error_even_when_skipped() {
    // ex_else(): "a missing expression is wrong, perhaps it should have
    // been else".
    let (_, errors) = textwidth_after("if 0\nif 1\nelseif\nendif\nendif\n");
    assert_eq!(errors, ["E15: Invalid expression: \"\""]);
}

#[test]
fn a_block_error_after_another_error_is_not_shown() {
    // do_one_cmd() shows a command's errormsg only when no error was
    // shown since did_emsg was cleared; emsg() shows E15 at once.
    for (config, expected) in [
        (
            "if 1\necho nosuch\nelse\nelse\nendif\n",
            vec!["E121: Undefined variable: nosuch"],
        ),
        ("elseif\n", vec!["E15: Invalid expression: \"\""]),
        (
            "echo nosuch | endif\n",
            vec!["E121: Undefined variable: nosuch"],
        ),
    ] {
        assert_eq!(textwidth_after(config).1, strings(&expected), "{config:?}");
    }
}

#[test]
fn endtry_reports_an_open_block_inside_it_after_another_error() {
    // ex_endtry() clears did_emsg to report it, and closes both blocks.
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("try\nif 1\nendtry\nset ic\n");
    assert!(engine.options().ignorecase());
    assert_eq!(
        errors(&response),
        [
            "E319: Sorry, the command is not available in this version: try",
            "E171: Missing :endif"
        ]
    );
}

#[test]
fn blocks_nest_fifty_deep() {
    // CSTACK_LEN: ex_if() opens no block for the 51st, so the last :endif
    // has none to close, once the flag of E579 is cleared.
    let config = "if 1\n".repeat(50) + &"endif\n".repeat(50);
    assert_eq!(textwidth_after(&config), (0, vec![]));
    let config = "if 1\n".repeat(51) + &"endif\n".repeat(51) + "set tw=33\n";
    let (textwidth, errors) = textwidth_after(&config);
    assert_eq!(textwidth, 33);
    assert_eq!(
        errors,
        ["E579: :if nesting too deep", "E580: :endif without :if"]
    );
}
