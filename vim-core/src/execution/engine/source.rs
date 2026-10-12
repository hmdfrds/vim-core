//! Config file processing for [`VimEngine`].
//!
//! Runs a config file's lines as the command line runs Ex commands, without
//! a buffer.

use super::Response;
use crate::commands::ex::effects as ex_effects;
use crate::document::Document;
use crate::effects::Effects;
use crate::errors::VimError;
use crate::execution::executor_ex::{self, SetScope};
use crate::execution::{ExecutionContext, InputContext};
use crate::grammar::script::{Line, Script};
use crate::grammar::{parse_ex_command_with_modifiers, split_ex_pipeline};
use crate::primitives::{Offset, Position};
use compact_str::CompactString;

impl super::VimEngine {
    /// Run a config file's text as Ex commands, line by line.
    ///
    /// The engine calls it when a host answers `:source` with the file's
    /// text; a host can call it for a config it reads itself, such as at
    /// startup.
    ///
    /// Each line runs as it would from the command line, `|` and command
    /// modifiers included, and what it changes is applied the same way.
    /// A command the host runs comes back as a host request. An error
    /// stops the rest of its line and is shown; `:silent!` hides it.
    ///
    /// There is no buffer: a command that needs one (see
    /// [`ExCommand::needs_buffer`](crate::grammar::types::ExCommand::needs_buffer))
    /// gives E523, and `:setlocal` and `:setglobal` act like `:set`.
    ///
    /// The text is read as Vim reads a sourced file: a byte order mark is
    /// dropped, the CR of each CR LF goes when the first line ends in CR
    /// LF, a line whose first non-blank is `\` continues the line before
    /// it, and a line whose first non-blank is `"` is a comment.
    ///
    /// A line can therefore hold several commands, so a host that filters
    /// an untrusted config must judge every `|` command of a line, and a
    /// continuation line together with the line it continues, not only the
    /// first command.
    ///
    /// The lines a command takes as text never run: those of a `:let` or
    /// script heredoc (`:lua << EOF`), a `:function` or `:def` body,
    /// `:append`, `:insert` and `:change` text, a `{` block of `:command`
    /// or `:autocmd`, and the rest of the file after `:loadkeymap`. The
    /// host gets only the command's own line, and the lines are skipped
    /// even when the command fails. After an error earlier on the line,
    /// as in Vim, only a heredoc or function body is skipped. Commands
    /// after a `|` on the line that ends a function run as the rest of
    /// that line.
    ///
    /// The response ends with "Config file sourced".
    pub fn source_config_text(&mut self, text: &str) -> Response {
        let mut response = Response::with_effects(Effects::new());
        let mut script = Script::new(text);
        while let Some(line) = script.next_line() {
            show_warnings(&mut script, &mut response);
            let mut skipping = false;
            let mut next = Some(line);
            while let Some(line) = next {
                next = self.source_config_line(&mut script, &line, &mut skipping, &mut response);
            }
        }
        response.extend_effects(ex_effects::show_message(CompactString::from(
            "Config file sourced",
        )));
        // Not register_pending_host_requests: its effect optimizer keeps only
        // the last of several messages, and a config can print many.
        self.track_host_requests(&mut response);
        response
    }

    /// Run one config line: each of its `|` commands in turn, reading the
    /// lines a command takes after the line from `script`. After an error
    /// (`skipping`) the commands of the line do not run, as in Vim, but
    /// those that read lines still read them. Returns the rest of the line
    /// that ends a function, which runs next.
    fn source_config_line(
        &mut self,
        script: &mut Script<'_>,
        line: &Line,
        skipping: &mut bool,
        response: &mut Response,
    ) -> Option<Line> {
        let mut rest = None;
        for command in split_ex_pipeline(&line.text) {
            let body = script.read_body(&command, line, *skipping);
            show_warnings(script, response);
            let parsed = parse_ex_command_with_modifiers(&command);
            if let Some(body) = body {
                let mut effects = Effects::new();
                for (_, error) in body.errors {
                    effects.extend(ex_effects::show_error(error));
                }
                if let Ok((modifiers, _)) = &parsed {
                    crate::execution::modifier_filter::apply_modifiers(&mut effects, *modifiers);
                }
                response.extend_effects(effects);
                *skipping |= body.fails;
                rest = body.tail;
            }
            if *skipping {
                continue;
            }
            let (modifiers, parsed) = match parsed {
                Ok(parsed) => parsed,
                Err(err) => {
                    response.extend_effects(ex_effects::show_error(err));
                    *skipping = true;
                    continue;
                }
            };
            let result = if parsed.needs_buffer() {
                Err(VimError::NotAllowedHere(CompactString::from(
                    command.trim(),
                )))
            } else {
                let input = InputContext::new(&NoBuffer, 0).validate_clamped();
                let ctx = ExecutionContext::new(input, &self.state, &self.options);
                executor_ex::execute_ex_command(&parsed, &ctx, &mut self.host.sequencer)
            };
            let mut output = match result {
                Ok(output) => output,
                Err(err) => {
                    let mut effects = ex_effects::show_error(err);
                    crate::execution::modifier_filter::apply_modifiers(&mut effects, modifiers);
                    response.extend_effects(effects);
                    *skipping = true;
                    continue;
                }
            };
            crate::execution::modifier_filter::apply_modifiers(&mut output.effects, modifiers);
            crate::execution::effect_processor::sync_effects(
                &mut self.state,
                &mut self.parser,
                output.effects.as_mut_slice(),
            );
            // No buffer or window to hold a local value.
            output.set_scope = SetScope::Effective;
            let mut applied = self.apply_ex_output(&mut output, "");
            crate::execution::modifier_filter::apply_modifiers(&mut applied, modifiers);
            response.extend_effects(output.effects);
            response.extend_effects(applied);
            response.host_requests.extend(output.host_requests);
        }
        rest
    }
}

/// Show the warnings of the lines `script` read since the last call.
fn show_warnings(script: &mut Script<'_>, response: &mut Response) {
    for (_, warning) in script.take_warnings() {
        response.extend_effects(ex_effects::show_error(warning));
    }
}

/// The buffer a config file runs against: there is none.
struct NoBuffer;

impl Document for NoBuffer {
    fn text(&self) -> &'static str {
        ""
    }

    fn line_count(&self) -> usize {
        1
    }

    fn offset_to_pos(&self, offset: Offset) -> Option<Position> {
        (offset == Offset::ZERO).then(|| Position::from_raw(0, 0))
    }

    fn pos_to_offset(&self, pos: Position) -> Option<Offset> {
        (pos.line().get() == 0).then_some(Offset::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use crate::effects::Effect;
    use crate::execution::host::HostRequestKind;
    use crate::execution::{parse_keys_from_string, HostSession, Response, VimEngine};

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

    /// A session on `text` that has sourced `config`.
    fn sourced(text: &str, config: &str) -> HostSession {
        let mut session = HostSession::new(text);
        let _ = session.source_config(config);
        session
    }

    fn type_keys(session: &mut HostSession, keys: &str) {
        for key in parse_keys_from_string(keys) {
            let _ = session.process_key_host(key);
        }
    }

    // ── lines run as on the command line ──────────────────────────

    #[test]
    fn source_config_defines_abbreviations() {
        // :iabbrev was skipped without a word.
        let mut session = sourced("x", "iabbrev teh the\n");
        type_keys(&mut session, "A teh <Esc>");
        assert_eq!(session.text(), "x the ");
    }

    #[test]
    fn source_config_runs_lines_written_with_a_colon() {
        let session = sourced("", ":set tw=33\n  : :set ic\n");
        assert_eq!(session.options().textwidth(), 33);
        assert!(session.options().ignorecase());
    }

    #[test]
    fn source_config_runs_each_command_of_a_bar_line() {
        let session = sourced("", "set tw=33 | set ic\n");
        assert_eq!(session.options().textwidth(), 33);
        assert!(session.options().ignorecase());
    }

    #[test]
    fn source_config_does_not_run_a_bar_inside_a_comment() {
        let session = sourced("", "set tw=33  \" wrap | set ic\n");
        assert_eq!(session.options().textwidth(), 33);
        assert!(!session.options().ignorecase());
    }

    #[test]
    fn source_config_ends_each_command_where_vim_s_table_says() {
        let mut engine = VimEngine::new();
        let config = "filetype plugin indent on | set ic\necho \"a|b\" | set tw=33\n\
                      nnoremap x y | set et\n";
        let response = engine.source_config_text(config);
        assert!(engine.options().ignorecase());
        assert_eq!(engine.options().textwidth(), 33);
        assert!(engine.options().expandtab());
        assert!(errors(&response).is_empty(), "{:?}", errors(&response));
        let kinds: Vec<_> = response.host_requests().iter().map(|r| r.kind()).collect();
        assert_eq!(kinds, [HostRequestKind::CustomExCommand]);
    }

    #[test]
    fn source_config_keeps_an_escaped_bar_in_a_mapping() {
        let mut session = sourced("ac", "nnoremap x :s/c/d/ \\| set ic<CR>\n");
        assert!(!session.options().ignorecase());
        type_keys(&mut session, "x");
        assert_eq!(session.text(), "ad");
        assert!(session.options().ignorecase());
    }

    #[test]
    fn source_config_clears_mappings_with_mapclear() {
        let mut engine = VimEngine::new();
        engine.source_config_text("nnoremap x y\nnmapclear\n");
        assert!(!engine.could_start_mapping(crate::keymap::KeyEvent::char('x')));
    }

    #[test]
    fn source_config_treats_setlocal_as_set() {
        // There is no buffer to hold a local value.
        let mut engine = VimEngine::new();
        engine.source_config_text("setlocal tw=33\nsetglobal ic\n");
        assert_eq!(engine.options().textwidth(), 33);
        assert!(engine.options().ignorecase());
    }

    // ── no buffer ─────────────────────────────────────────────────

    #[test]
    fn source_config_refuses_commands_that_need_a_buffer() {
        let mut engine = VimEngine::new();
        let response = engine.source_config_text("set ic | %d | set tw=33\nnormal dd\n");

        assert!(engine.options().ignorecase());
        assert_eq!(
            engine.options().textwidth(),
            0,
            "the line stops at the error"
        );
        assert_eq!(
            errors(&response),
            [
                "E523: Not allowed here: %d",
                "E523: Not allowed here: normal dd"
            ]
        );
    }

    #[test]
    fn source_config_silent_bang_hides_errors() {
        let mut engine = VimEngine::new();
        let response = engine.source_config_text("silent! normal dd\nsilent! set nosuchoption\n");
        assert!(errors(&response).is_empty());
    }

    #[test]
    fn source_config_reports_an_error_and_goes_on() {
        let mut engine = VimEngine::new();
        let response = engine.source_config_text("set nosuchoption\nset tw=33\n");
        assert_eq!(engine.options().textwidth(), 33);
        assert_eq!(errors(&response).len(), 1);
    }

    #[test]
    fn source_config_effects_are_messages_only() {
        // Hosts apply config effects without a buffer.
        let mut engine = VimEngine::new();
        let config = "set tw=80 | set tw?\niabbrev teh the\niabbrev\nnoh\necho hi\n\
                      registers\nmessages\nnormal x\nclearjumps\nnnoremap x y\nsethandler <C-a> n:vim\n";
        let response = engine.source_config_text(config);
        for effect in response.effects() {
            assert!(
                matches!(
                    effect,
                    Effect::ShowInfo { .. } | Effect::ShowError { .. } | Effect::ClearHighlights
                ),
                "unexpected effect {effect:?}"
            );
        }
    }

    #[test]
    fn source_config_returns_every_message() {
        let mut engine = VimEngine::new();
        let response = engine.source_config_text("set tw=80 | set tw?\necho hi\n");
        let infos: Vec<String> = response
            .effects()
            .iter()
            .filter_map(|e| match e {
                Effect::ShowInfo { info } => Some(info.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(infos, ["textwidth=80", "hi", "Config file sourced"]);
    }

    #[test]
    fn source_config_refuses_script_blocks_for_now() {
        // `if` ends the line with E319, so nothing after it runs.
        let mut engine = VimEngine::new();
        let response = engine.source_config_text(
            "if 0 | set ic | endif
finish
",
        );
        assert!(!engine.options().ignorecase());
        assert_eq!(
            errors(&response),
            [
                "E319: Sorry, the command is not available in this version: if 0",
                "E168: :finish used outside of a sourced file"
            ]
        );
        assert!(response.host_requests().is_empty());
    }

    // ── host commands ─────────────────────────────────────────────

    #[test]
    fn source_config_sends_host_commands_to_the_host() {
        let mut engine = VimEngine::new();
        let response = engine.source_config_text("write\nfiletype plugin on\npanelmap x y\n");
        let kinds: Vec<_> = response.host_requests().iter().map(|r| r.kind()).collect();
        assert_eq!(
            kinds,
            [
                HostRequestKind::WriteFile,
                HostRequestKind::CustomExCommand,
                HostRequestKind::CustomExCommand
            ]
        );
        for request in response.host_requests() {
            assert!(engine.has_pending_host_request(request.id()));
        }
        assert!(errors(&response).is_empty());
    }

    #[test]
    fn source_config_host_requests_reach_a_session_host() {
        let mut session = HostSession::new("");
        let response = session.source_config("write\nfoo\n");
        let kinds: Vec<_> = response.host_requests.iter().map(|r| r.kind()).collect();
        assert_eq!(
            kinds,
            [HostRequestKind::WriteFile, HostRequestKind::CustomExCommand]
        );
        assert_eq!(session.pending_request_count(), 2);
    }

    // ── how Vim reads a sourced file ──────────────────────────────

    #[test]
    fn source_config_skips_only_spaces_and_tabs_before_a_comment() {
        // `skipwhite()` skips no other white space, so after a no-break
        // space `"` starts no comment and the line is a command.
        let mut engine = VimEngine::new();
        let response = engine.source_config_text("\u{a0}\" x\n \t\" y\n");
        let kinds: Vec<_> = response.host_requests().iter().map(|r| r.kind()).collect();
        assert_eq!(kinds, [HostRequestKind::CustomExCommand]);
    }

    #[test]
    fn source_config_drops_a_byte_order_mark() {
        let session = sourced("", "\u{feff}set ic\n");
        assert!(session.options().ignorecase());
    }

    #[test]
    fn source_config_reads_a_dos_file() {
        let mut engine = VimEngine::new();
        let response = engine.source_config_text("set tw=33\r\nset ic\r\n");
        assert_eq!(engine.options().textwidth(), 33);
        assert!(engine.options().ignorecase());
        assert!(errors(&response).is_empty(), "{:?}", errors(&response));
    }

    #[test]
    fn source_config_warns_of_a_lone_lf_in_a_dos_file() {
        // `get_one_sourceline()` gives W15 as an error; the line still runs.
        let mut engine = VimEngine::new();
        let response = engine.source_config_text("set ic\r\nset tw=33\nset et\r\n");
        assert_eq!(engine.options().textwidth(), 33);
        assert!(engine.options().expandtab());
        assert_eq!(
            errors(&response),
            ["W15: Warning: Wrong line separator, ^M may be missing"]
        );
    }

    #[test]
    fn source_config_joins_continuation_lines() {
        let config = "nnoremap x\n      \\ :set ic<CR>\n\"\\ a comment inside\n      \\\n";
        let mut session = sourced("a", config);
        type_keys(&mut session, "x");
        assert!(session.options().ignorecase());
    }

    #[test]
    fn source_config_does_not_run_a_function_body() {
        let config = "function! Foo()\n  set ic\n  function! Bar()\n  endfunction\n  set tw=1\nendfunction\nset tw=33\n";
        let mut engine = VimEngine::new();
        let response = engine.source_config_text(config);
        assert!(!engine.options().ignorecase());
        assert_eq!(engine.options().textwidth(), 33);
        // The host decides what the :function line means.
        assert_eq!(response.host_requests().len(), 1);
    }

    #[test]
    fn source_config_applies_set_options() {
        let mut engine = VimEngine::new();
        let config = "set tabstop=8\nset expandtab\nset noautoindent\n";
        let response = engine.source_config_text(config);

        assert_eq!(engine.options().tabstop(), 8);
        assert!(engine.options().expandtab());
        assert!(!engine.options().autoindent());
        assert!(!response.effects.is_empty());
    }

    #[test]
    fn source_config_skips_comments_and_empty_lines() {
        let mut engine = VimEngine::new();
        let config = "\" This is a comment\n\n  \" Another comment\nset tabstop=2\n";
        engine.source_config_text(config);

        assert_eq!(engine.options().tabstop(), 2);
    }

    #[test]
    fn source_config_skips_unparseable_lines() {
        let mut engine = VimEngine::new();
        let config = "not_a_valid_command\nset tabstop=4\ngibberish!!!!\n";
        engine.source_config_text(config);

        assert_eq!(engine.options().tabstop(), 4);
    }

    #[test]
    fn source_config_applies_mappings() {
        let mut engine = VimEngine::new();
        let config = "nnoremap jk <Esc>\n";
        let response = engine.source_config_text(config);

        // Mapping was applied (we can verify it indirectly by checking
        // that could_start_mapping returns true for 'j')
        let j_key = crate::keymap::KeyEvent::char('j');
        assert!(engine.could_start_mapping(j_key));
        assert!(!response.effects.is_empty());
    }

    #[test]
    fn source_config_applies_let_mapleader() {
        let mut engine = VimEngine::new();
        // Default leader is backslash
        let default_leader = engine.leader();

        let config = "let mapleader = \" \"\n";
        engine.source_config_text(config);

        let new_leader = engine.leader();
        assert_ne!(default_leader, new_leader);
        assert_eq!(new_leader, crate::keymap::KeyEvent::char(' '));
    }

    #[test]
    fn source_config_leader_before_mapping() {
        let mut engine = VimEngine::new();
        let config = "let mapleader = \",\"\nnnoremap <Leader>w :save<CR>\n";
        engine.source_config_text(config);

        // The mapping should be registered under comma (the leader)
        let comma = crate::keymap::KeyEvent::char(',');
        assert!(engine.could_start_mapping(comma));
    }

    #[test]
    fn source_config_shows_sourced_message() {
        let mut engine = VimEngine::new();
        let response = engine.source_config_text("");

        // Even with empty config, should show "Config file sourced"
        let has_message = response.effects.iter().any(|e| {
            matches!(e, crate::effects::Effect::ShowInfo { info: crate::effects::InfoMessage::Text(text) } if text.contains("sourced"))
        });
        assert!(has_message);
    }

    // ── :sethandler via source ──────────────────────────────────────

    #[test]
    fn source_config_applies_sethandler_single_key_mode() {
        use crate::keymap::{Handler, KeyEvent, MappingMode};

        let mut engine = VimEngine::new();
        let config = "sethandler <C-a> i:ide\n";
        engine.source_config_text(config);

        assert!(
            engine
                .handler_map()
                .is_host_handled(KeyEvent::ctrl('a'), MappingMode::Insert,),
            "Ctrl-A should be host-handled in Insert after sethandler <C-a> i:ide"
        );
        assert_eq!(
            engine
                .handler_map()
                .get(KeyEvent::ctrl('a'), MappingMode::Normal),
            Handler::Vim,
            "Ctrl-A should still be Vim-handled in Normal (not configured)"
        );
    }

    #[test]
    fn source_config_applies_sethandler_multiple_assignments() {
        use crate::keymap::{KeyEvent, MappingMode};

        let mut engine = VimEngine::new();
        let config = "sethandler <C-a> n:vim i:ide\n";
        engine.source_config_text(config);

        assert!(
            !engine
                .handler_map()
                .is_host_handled(KeyEvent::ctrl('a'), MappingMode::Normal,),
            "Ctrl-A Normal should be Vim"
        );
        assert!(
            engine
                .handler_map()
                .is_host_handled(KeyEvent::ctrl('a'), MappingMode::Insert,),
            "Ctrl-A Insert should be Host"
        );
    }

    #[test]
    fn source_config_applies_sethandler_dash_modes() {
        use crate::keymap::{KeyEvent, MappingMode};

        let mut engine = VimEngine::new();
        let config = "sethandler <C-c> n-v:ide i:vim\n";
        engine.source_config_text(config);

        assert!(
            engine
                .handler_map()
                .is_host_handled(KeyEvent::ctrl('c'), MappingMode::Normal,),
            "Ctrl-C Normal should be Host (ide)"
        );
        assert!(
            engine
                .handler_map()
                .is_host_handled(KeyEvent::ctrl('c'), MappingMode::Visual,),
            "Ctrl-C Visual should be Host (ide)"
        );
        assert!(
            !engine
                .handler_map()
                .is_host_handled(KeyEvent::ctrl('c'), MappingMode::Insert,),
            "Ctrl-C Insert should be Vim"
        );
    }

    #[test]
    fn source_config_applies_sethandler_all_modes() {
        use crate::keymap::{KeyEvent, MappingMode};

        let mut engine = VimEngine::new();
        let config = "sethandler <C-v> a:host\n";
        engine.source_config_text(config);

        for mm in MappingMode::ALL {
            assert!(
                engine
                    .handler_map()
                    .is_host_handled(KeyEvent::ctrl('v'), mm),
                "Ctrl-V should be host-handled in {mm:?}"
            );
        }
    }

    #[test]
    fn source_config_sethandler_then_key_returns_ignored() {
        use crate::execution::InputContext;
        use crate::keymap::KeyEvent;
        use crate::test_utils::SimpleDocument;

        let mut engine = VimEngine::new();
        let config = "sethandler <C-a> n:ide\n";
        engine.source_config_text(config);

        // Process Ctrl-A in Normal mode — should be ignored (host-handled)
        let doc = SimpleDocument::new("hello");
        let ctx = InputContext::new(&doc, 0).validate().unwrap();
        let response = engine.process(KeyEvent::ctrl('a'), ctx);

        assert!(
            !response.consumed(),
            "Ctrl-A in Normal should return Ignored after sethandler n:ide"
        );
    }

    #[test]
    fn source_config_sethandler_vim_still_processes() {
        use crate::execution::InputContext;
        use crate::keymap::KeyEvent;
        use crate::test_utils::SimpleDocument;

        let mut engine = VimEngine::new();
        // Set Ctrl-A to Vim in Normal, Host in Insert
        let config = "sethandler <C-a> n:vim i:ide\n";
        engine.source_config_text(config);

        // Process Ctrl-A in Normal mode — should be consumed (Vim handles it)
        let doc = SimpleDocument::new("hello");
        let ctx = InputContext::new(&doc, 0).validate().unwrap();
        let response = engine.process(KeyEvent::ctrl('a'), ctx);

        assert!(
            response.consumed(),
            "Ctrl-A in Normal should be consumed after sethandler n:vim"
        );
    }

    #[test]
    fn source_config_sethandler_abbreviated() {
        use crate::keymap::{KeyEvent, MappingMode};

        let mut engine = VimEngine::new();
        let config = "seth <C-a> i:ide\n";
        engine.source_config_text(config);

        assert!(
            engine
                .handler_map()
                .is_host_handled(KeyEvent::ctrl('a'), MappingMode::Insert,),
            "seth abbreviation should work for sethandler"
        );
    }

    #[test]
    fn source_config_sethandler_invalid_handler_is_reported() {
        let mut engine = VimEngine::new();
        let response = engine.source_config_text("sethandler <C-a> n:banana\n");

        assert!(engine.handler_map().is_empty());
        assert_eq!(errors(&response).len(), 1);
    }

    #[test]
    fn source_config_sethandler_no_key_is_noop() {
        let mut engine = VimEngine::new();
        // No key specified — global default, currently a no-op
        let config = "sethandler n:vim\n";
        engine.source_config_text(config);

        // Should not error, handler map stays empty
        assert!(engine.handler_map().is_empty());
    }

    // ── :set langmap / langremap via source ────────────────────────────

    #[test]
    fn source_config_langmap_remaps_key_in_normal_mode() {
        use crate::effects::Effect;
        use crate::execution::InputContext;
        use crate::keymap::KeyEvent;
        use crate::test_utils::SimpleDocument;

        let mut engine = VimEngine::new();
        // Semicolon format: й maps to j, ц maps to k
        let config = "set langmap=йц;jk\n";
        engine.source_config_text(config);

        // Verify the langmap table was populated
        assert!(
            !engine.langmap_table.is_empty(),
            "langmap table should be non-empty after :set langmap=йц;jk"
        );

        // Process й in Normal mode — should be remapped to j (cursor down)
        let doc: &'static SimpleDocument =
            Box::leak(Box::new(SimpleDocument::new("hello\nworld\nfoo\n")));
        let ctx = InputContext::new(doc, 0).validate_clamped();
        let response = engine.process(KeyEvent::char('й'), ctx);

        assert!(response.consumed(), "й should be consumed (remapped to j)");
        assert!(
            response
                .effects()
                .iter()
                .any(|e| matches!(e, Effect::SetCursor { .. })),
            "langmap й→j should produce a SetCursor effect (cursor down), got: {:?}",
            response.effects(),
        );
    }

    #[test]
    fn source_config_langmap_direct_table_set() {
        use crate::effects::Effect;
        use crate::execution::InputContext;
        use crate::keymap::KeyEvent;
        use crate::test_utils::SimpleDocument;

        let mut engine = VimEngine::new();
        // Directly set the langmap table for testing (bypassing :set)
        engine.langmap_table = crate::keymap::LangmapTable::parse("йц;jk").unwrap();

        let doc: &'static SimpleDocument =
            Box::leak(Box::new(SimpleDocument::new("hello\nworld\nfoo\n")));
        let ctx = InputContext::new(doc, 0).validate_clamped();
        let response = engine.process(KeyEvent::char('й'), ctx);

        assert!(
            response.consumed(),
            "й should be consumed (remapped to j via direct table)"
        );
        assert!(
            response
                .effects()
                .iter()
                .any(|e| matches!(e, Effect::SetCursor { .. })),
            "langmap й→j should produce SetCursor, got: {:?}",
            response.effects(),
        );
    }

    #[test]
    fn source_config_set_langremap() {
        let mut engine = VimEngine::new();
        // Default is false (Neovim convention)
        assert!(!engine.options().langremap());

        let config = "set langremap\n";
        engine.source_config_text(config);
        assert!(
            engine.options().langremap(),
            "langremap should be true after :set langremap"
        );

        let config2 = "set nolangremap\n";
        engine.source_config_text(config2);
        assert!(
            !engine.options().langremap(),
            "langremap should be false after :set nolangremap"
        );
    }

    #[test]
    fn source_config_langmap_sets_key_interest_dirty() {
        let mut engine = VimEngine::new();
        // Clear the dirty flag (it starts true)
        engine.key_interest_dirty = false;

        let config = "set langmap=ab\n";
        engine.source_config_text(config);

        assert!(
            engine.key_interest_dirty,
            "key_interest_dirty should be set after langmap change"
        );
    }

    #[test]
    fn source_config_langmap_empty_clears_table() {
        let mut engine = VimEngine::new();
        // First set a non-empty langmap
        let config = "set langmap=йц;jk\n";
        engine.source_config_text(config);
        assert!(!engine.langmap_table.is_empty());

        // Now clear it
        let config2 = "set langmap=\n";
        engine.source_config_text(config2);
        assert!(
            engine.langmap_table.is_empty(),
            "langmap table should be empty after :set langmap="
        );
    }
}
