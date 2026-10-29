//! A config file read and run as Vim sources one.

use vim_core::effects::{Effect, InfoMessage, SourceContext};
use vim_core::execution::{
    ConfigHost, ConfigText, HostRequest, HostSession, Response, TrustTag, VimEngine,
};

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

/// The text of each message the response shows.
fn infos(response: &Response) -> Vec<String> {
    response
        .effects()
        .iter()
        .filter_map(|e| match e {
            Effect::ShowInfo {
                info: InfoMessage::Text(text),
            } => Some(text.to_string()),
            _ => None,
        })
        .collect()
}

/// Each error the response shows, with the line it was raised at in the
/// config `file`.
fn located(response: &Response, file: &str) -> Vec<(u32, String)> {
    response
        .effects()
        .iter()
        .filter_map(|e| match e {
            Effect::ShowError { error, source } => {
                let SourceContext { file: name, line } = source.clone().expect("a place");
                assert_eq!(name, file, "{error}");
                Some((line, error.to_string()))
            }
            _ => None,
        })
        .collect()
}

/// A host that has the features it names.
struct Features(&'static [&'static str]);

impl ConfigHost for Features {
    fn has_feature(&self, feature: &str) -> bool {
        self.0.contains(&feature)
    }
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
    assert!(response.host_requests().is_empty());
    assert_eq!(engine.options().textwidth(), 33);
    assert_eq!(
        errors(&response),
        ["E319: Sorry, the command is not available in this version: lua << EOF"]
    );
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
    // Under :silent! the E319 of :function counts as no error.
    let mut engine = VimEngine::new();
    let response =
        engine.source_config_text("silent! function! F()\n  set ic\nendfunction | set tw=33\n");
    assert!(!engine.options().ignorecase());
    assert_eq!(engine.options().textwidth(), 33);
    assert!(errors(&response).is_empty(), "{:?}", errors(&response));
}

#[test]
fn the_e319_of_a_function_skips_the_rest_of_its_end_line() {
    // The text after `endfunction |` is the rest of the :function line.
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("function! F()\n  set ic\nendfunction | set tw=33\n");
    assert!(!engine.options().ignorecase());
    assert_eq!(engine.options().textwidth(), 0);
    assert_eq!(
        errors(&response),
        ["E319: Sorry, the command is not available in this version: function! F()"]
    );
    assert!(response.host_requests().is_empty());
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
    assert_eq!(
        errors(&response),
        ["E319: Sorry, the command is not available in this version: command! Foo {"]
    );
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

#[test]
fn a_set_error_ends_the_set_and_its_line() {
    // do_set() returns at its first bad argument, and the error skips the
    // rest of the line.
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("set ts=7 nosuch sw=2 | set ic\nset tw=33\n");
    assert_eq!(engine.options().tabstop(), 7);
    assert_eq!(engine.options().shiftwidth(), 4);
    assert!(!engine.options().ignorecase());
    assert_eq!(engine.options().textwidth(), 33);
    assert_eq!(errors(&response), ["E518: Unknown option: nosuch"]);
}

#[test]
fn silent_bang_hides_a_set_error_but_still_ends_the_set() {
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("silent! set nosuch ic | set tw=33\n");
    assert!(!engine.options().ignorecase());
    assert_eq!(engine.options().textwidth(), 33);
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

#[test]
fn finish_ends_the_file_after_its_line() {
    // ex_finish(): getsourceline() reads no line once the file is
    // finished, and do_cmdline() runs the rest of the line first.
    for (config, textwidth) in [
        ("finish\nset tw=33\n", 0),
        ("fini\nset tw=33\n", 0),
        ("set tw=1 | finish | set tw=9\nset tw=33\n", 9),
        ("if 0\nfinish\nendif\nset tw=33\n", 33),
        ("set tw=abc | finish\nset tw=33\n", 33),
    ] {
        let (found, errors) = textwidth_after(config);
        assert_eq!(found, textwidth, "{config:?}");
        assert!(errors.len() <= 1, "{config:?}: {errors:?}");
    }
}

#[test]
fn finish_inside_a_block_skips_the_rest_of_its_line() {
    // do_finish(): cleanup_conditionals() leaves no block active, and no
    // E171 follows (source_finished()).
    for config in [
        "if 1 | finish | set tw=9 | endif\nset tw=33\n",
        "if 1\nfinish\nendif\nset tw=33\n",
        "if 1\nif 1 | finish | endif | else | set tw=9\n",
        "if 0\nelse\nfinish | set tw=9\n",
    ] {
        assert_eq!(textwidth_after(config), (0, vec![]), "{config:?}");
    }
}

#[test]
fn finish_guards_a_config_sourced_twice() {
    let config = "if exists('g:loaded') | finish | endif\nlet g:loaded = 1\nlet &tw += 1\n";
    let mut engine = VimEngine::new();
    engine.source_config_text(config);
    let response = engine.source_config_text(config);
    assert_eq!(engine.options().textwidth(), 1);
    assert!(errors(&response).is_empty(), "{:?}", errors(&response));
}

#[test]
fn finish_with_an_argument_is_an_error_and_the_file_goes_on() {
    let (textwidth, errors) = textwidth_after("finish x\nset tw=33\n");
    assert_eq!(textwidth, 33);
    assert_eq!(errors, ["E488: Trailing characters: x"]);
}

// ── what vim-core lacks ───────────────────────────────────────────────

#[test]
fn a_vim_option_vim_core_lacks_is_set_silently() {
    // do_set_option(): a hidden option takes the value and keeps nothing.
    let mut engine = VimEngine::new();
    let response = engine.source_config_text(
        "set nocompatible\nset encoding=utf-8\nset mouse=a\nset t_Co=256\n\
         set guifont=Monospace\\ 11\nlet &mouse = 'a'\nlet &compatible += 1\nset ic\n",
    );
    assert!(errors(&response).is_empty(), "{:?}", errors(&response));
    assert!(engine.options().ignorecase());
}

#[test]
fn asking_for_a_vim_option_vim_core_lacks_is_e519() {
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("set mouse? | set ic\nset nosuch\n");
    assert!(!engine.options().ignorecase());
    assert_eq!(
        errors(&response),
        [
            "E519: Option not supported: mouse?",
            "E518: Unknown option: nosuch"
        ]
    );
}

#[test]
fn a_vim_option_vim_core_lacks_exists_and_reads_as_empty() {
    // eval_option(): `&name` of a hidden option is 0 or "", and only
    // `+name` tells that it does not work.
    let mut engine = VimEngine::new();
    engine.source_config_text(
        "if exists('&mouse') && !exists('+mouse') && &mouse == '' && &compatible == 0\n\
         set ic\nendif\n",
    );
    assert!(engine.options().ignorecase());
}

#[test]
fn let_of_a_vim_option_vim_core_lacks_checks_the_value_type() {
    // ex_let_option(): a Float is no Number, v:true no String.
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("let &compatible = 1.5\nlet &mouse = v:true\n");
    assert_eq!(
        errors(&response),
        ["E805: Using a Float as a Number", "E928: String required"]
    );
}

#[test]
fn a_vim_command_vim_core_lacks_is_e319_and_ends_its_line() {
    // ex_ni(): `syntax` is Vim's, so the error names it and ends the line.
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("syntax on | set ic\ncolorscheme desert\n");
    assert!(!engine.options().ignorecase());
    assert_eq!(
        errors(&response),
        [
            "E319: Sorry, the command is not available in this version: syntax on",
            "E319: Sorry, the command is not available in this version: colorscheme desert"
        ]
    );
    assert!(response.host_requests().is_empty());
}

#[test]
fn a_vim_command_vim_core_lacks_skips_the_rest_of_its_block() {
    // do_cmdline(): the error skips to the end of the outermost block.
    let mut engine = VimEngine::new();
    let response =
        engine.source_config_text("if 1\nautocmd BufRead * set ts=2\nset ic\nendif\nset tw=33\n");
    assert!(!engine.options().ignorecase());
    assert_eq!(engine.options().textwidth(), 33);
    assert_eq!(errors(&response).len(), 1, "{:?}", errors(&response));
}

#[test]
fn a_skipped_vim_command_vim_core_lacks_gives_nothing() {
    let mut engine = VimEngine::new();
    let response =
        engine.source_config_text("if 0\nsyntax on\nendif\nsilent! syntax on | set ic\n");
    assert!(errors(&response).is_empty(), "{:?}", errors(&response));
    assert!(engine.options().ignorecase());
}

#[test]
fn filetype_and_commands_that_are_not_vim_s_go_to_the_host() {
    // find_ex_command(): a name that starts with a capital is a user
    // command, but for :Next, :Print and :X.
    let mut engine = VimEngine::new();
    let response =
        engine.source_config_text("filetype plugin indent on\npanelmap x y\nMake all\nNext\n");
    assert_eq!(
        errors(&response),
        ["E319: Sorry, the command is not available in this version: Next"]
    );
    assert_eq!(
        host_commands(&response),
        ["filetype plugin indent on", "panelmap x y", "Make all"]
    );
}

#[test]
fn scriptencoding_does_nothing() {
    // ex_scriptencoding(): the text is UTF-8 already.
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("scriptencoding utf-8 | set ic\n");
    assert!(errors(&response).is_empty(), "{:?}", errors(&response));
    assert!(response.host_requests().is_empty());
    assert!(engine.options().ignorecase());
}

#[test]
fn vim9script_is_e319_and_ends_the_file() {
    // Deviation: Vim reads the rest as Vim9 script.
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("if 1\nvim9script\nendif\nset ic\n");
    assert!(!engine.options().ignorecase());
    assert_eq!(
        errors(&response),
        ["E319: Sorry, the command is not available in this version: vim9script"]
    );
    let response = engine.source_config_text("silent! vim9script\nset ic\n");
    assert!(errors(&response).is_empty(), "{:?}", errors(&response));
    assert!(!engine.options().ignorecase());
}

// ── the entry point ───────────────────────────────────────────────────

#[test]
fn source_script_counts_its_errors_and_adds_no_message() {
    // do_source_ext() says nothing after a file, unless 'verbose' is 2 or more.
    let mut engine = VimEngine::new();
    let text = ConfigText::new("a.vim", "set ic\nset nosuch\nsilent! set nosuch\nset tw?\n")
        .with_trust(TrustTag::new(2));
    assert_eq!(text.trust.get(), 2);
    let sourced = engine.source_script(text, &mut Features(&[]));
    assert!(engine.options().ignorecase());
    assert_eq!(sourced.outcome.errors, 1);
    assert_eq!(errors(&sourced.response), ["E518: Unknown option: nosuch"]);
    assert_eq!(infos(&sourced.response), ["textwidth=0"]);
}

#[test]
fn has_asks_the_host_for_a_feature_vim_core_does_not_know() {
    // f_has(): with {check}, whether the feature is known at all.
    let config = "if has('godot') | set ic | endif\nlet x = has('godot', 1) . has('eval')\n\
                  if has('nosuch') || has('nosuch', 1) | set tw=33 | endif\n";
    let mut engine = VimEngine::new();
    let sourced = engine.source_script(ConfigText::new("", config), &mut Features(&["godot"]));
    assert!(
        errors(&sourced.response).is_empty(),
        "{:?}",
        errors(&sourced.response)
    );
    assert!(engine.options().ignorecase());
    assert_eq!(engine.options().textwidth(), 0);
    let shown = engine.source_script(ConfigText::new("", "echo x\n"), &mut Features(&[]));
    assert_eq!(infos(&shown.response), ["11"]);

    let mut engine = VimEngine::new();
    let _ = engine.source_config_text("if has('godot') | set ic | endif\n");
    assert!(!engine.options().ignorecase());
}

#[test]
fn a_session_sources_a_script_through_its_host() {
    let mut session = HostSession::new("a");
    let _ = session.source_script(
        ConfigText::new("init.vim", "if has('godot')\nset ic\nendif\n"),
        &mut Features(&["godot"]),
    );
    assert!(session.options().ignorecase());
}

// ── where errors are raised ───────────────────────────────────────────

#[test]
fn an_error_says_where_it_was_raised() {
    let mut engine = VimEngine::new();
    let text = ConfigText::new("a.vim", "set ic\nset nosuch\n");
    let sourced = engine.source_script(text, &mut Features(&[]));
    assert_eq!(
        sourced.response.effects(),
        [Effect::ShowError {
            error: vim_core::errors::VimError::UnknownOptionArgument("nosuch".into()),
            source: Some(SourceContext {
                file: "a.vim".into(),
                line: 2
            }),
        }]
    );
    assert_eq!(sourced.outcome.errors, 1);
}

#[test]
fn an_error_of_a_continued_line_is_at_its_first_line() {
    // getsourceline(): SOURCING_LNUM is the line the joined lines start at.
    let mut engine = VimEngine::new();
    let text = ConfigText::new("a.vim", "set ic\nset tw=1\nset\n  \\ nosuch\nset nosuch\n");
    let response = engine.source_script(text, &mut Features(&[])).response;
    assert_eq!(
        located(&response, "a.vim"),
        [
            (3, "E518: Unknown option: nosuch".to_owned()),
            (5, "E518: Unknown option: nosuch".to_owned())
        ]
    );
}

#[test]
fn an_error_at_the_end_of_the_file_is_at_the_line_after_the_last() {
    let mut engine = VimEngine::new();
    let response = engine
        .source_script(
            ConfigText::new("a.vim", "if 1\nset ic\n"),
            &mut Features(&[]),
        )
        .response;
    assert_eq!(
        located(&response, "a.vim"),
        [(3, "E171: Missing :endif".to_owned())]
    );
    let response = engine
        .source_script(
            ConfigText::new("a.vim", "let x =<< END\na\n"),
            &mut Features(&[]),
        )
        .response;
    // The command's own error stays at its line.
    assert_eq!(
        located(&response, "a.vim"),
        [
            (3, "E990: Missing end marker 'END'".to_owned()),
            (
                1,
                "E319: Sorry, the command is not available in this version: let x =<< END"
                    .to_owned()
            )
        ]
    );
}

#[test]
fn an_error_after_lines_read_as_text_is_at_the_last_of_them() {
    // get_function_body() leaves SOURCING_LNUM at :endfunction and
    // ex_append() at the `.`; E126 is at the :function line.
    let config = "silent! function! F()\n  set ic\nendfunction | set nosuch\n\
                  silent! append | set nosuch\nx\n.\nfunction! G()\n";
    let mut engine = VimEngine::new();
    let response = engine
        .source_script(ConfigText::new("a.vim", config), &mut Features(&[]))
        .response;
    assert_eq!(
        located(&response, "a.vim"),
        [
            (3, "E518: Unknown option: nosuch".to_owned()),
            (6, "E518: Unknown option: nosuch".to_owned()),
            (7, "E126: Missing :endfunction".to_owned())
        ]
    );
}

#[test]
fn a_warning_is_at_its_own_line() {
    let mut engine = VimEngine::new();
    let text = ConfigText::new("a.vim", "set ic\r\nset tw=33\nset tw=44\r\n");
    let response = engine.source_script(text, &mut Features(&[])).response;
    assert_eq!(
        located(&response, "a.vim"),
        [
            (
                2,
                "W15: Warning: Wrong line separator, ^M may be missing".to_owned()
            ),
            (3, "E521: Number required after =: tw=44^M".to_owned())
        ]
    );
}

#[test]
fn a_config_without_a_name_places_its_errors_at_their_lines() {
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("set nosuch");
    assert_eq!(
        located(&response, ""),
        [(1, "E518: Unknown option: nosuch".to_owned())]
    );
    assert_eq!(infos(&response), ["Config file sourced"]);
}
