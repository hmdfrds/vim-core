//! Config file processing for [`VimEngine`].
//!
//! Runs a config file's lines as the command line runs Ex commands, without
//! a buffer.

use super::conditionals::{Blocks, Kind};
use super::Response;
use crate::commands::ex::effects as ex_effects;
use crate::document::Document;
use crate::effects::{Effect, Effects, SourceContext};
use crate::errors::VimError;
use crate::execution::config::{
    ConfigAction, ConfigCommand, ConfigHost, ConfigOutcome, ConfigResponse, ConfigText,
    MapDefinition, MappingReach, OptionTarget, SourceReply, SourceRequest, TrustAll, TrustTag,
    Verdict,
};
use crate::execution::executor_ex::{self, SetKind, SetScope};
use crate::execution::script_expr::{self, Echoed, OptionLayer, Target, Value};
use crate::execution::{ExecutionContext, HostRequest, InputContext};
use crate::grammar::ex_parser::{parse_config_modifiers, parse_ex_piece_at, vim_command_parts};
use crate::grammar::script::{script_command, Control, Line, Script, ScriptCommand};
use crate::grammar::split_ex_pipeline;
use crate::grammar::types::{BlockKind, ExCommand, ModifierFlags, Reach, SetAssignment};
use crate::primitives::{Offset, OptionValue, Position, VarScope};
use crate::VimString;
use std::borrow::Cow;

/// Most files that run at once: `do_cmdline()` refuses a level past 200.
const MAX_DEPTH: u32 = 200;

/// The depth of a file sourced from the command line, which is the first
/// level.
pub(super) const TYPED_SOURCE_DEPTH: u32 = 2;

/// A config being sourced, with what Vim's `do_cmdline()` and the cookie of
/// `do_source_ext()` keep while it runs.
struct Frame<'a> {
    /// Its own name.
    name: Cow<'a, str>,
    /// The name its errors are reported under: `estack_sfile()`'s stack of
    /// the files that run, as in `a.vim[12]..b.vim`.
    file: String,
    /// The files that source it, as `file` starts with them: `a.vim[12]..`.
    stack: String,
    trust: TrustTag,
    /// How many files run, this one included.
    depth: u32,
    /// The modifiers of the `:source` that runs it, which `apply_cmdmod()`
    /// keeps in force for its commands.
    modifiers: ModifierFlags,
    script: Script<'a>,
    blocks: Blocks,
    /// Vim's `did_emsg`: an error was shown, so the commands after it do
    /// not run, up to the end of its line or of the outermost block. All
    /// files share it: a file starts with it cleared, and hands it back to
    /// the file that sourced it.
    error: bool,
    /// `:finish` ran: no line is read after the current one.
    finished: bool,
    /// The line running.
    line: Line,
    /// Its commands that have not run, the next one last.
    pieces: Vec<String>,
    /// The commands after the `|` of the line that ends a function, which
    /// run as the rest of that line.
    rest: Option<Line>,
}

/// The text of a file a `:source` gave.
struct Sourced {
    name: String,
    text: String,
    trust: TrustTag,
    /// The modifiers of the `:source`.
    modifiers: ModifierFlags,
}

impl<'a> Frame<'a> {
    fn new(
        name: Cow<'a, str>,
        file: String,
        stack: String,
        text: Cow<'a, str>,
        trust: TrustTag,
        depth: u32,
        modifiers: ModifierFlags,
    ) -> Self {
        Self {
            name,
            file,
            stack,
            trust,
            depth,
            modifiers,
            script: Script::new(text),
            blocks: Blocks::default(),
            error: false,
            finished: false,
            line: Line::default(),
            pieces: Vec::new(),
            rest: None,
        }
    }

    /// The frame of the file `sourced`, which the command running here
    /// sources. A file without a name is left out of the stack, so its
    /// errors name the files that source it, as in `a.vim[12]`.
    fn nested(&self, sourced: Sourced) -> Self {
        let stack = if self.name.is_empty() {
            self.stack.clone()
        } else {
            format!("{}[{}]..", self.file, self.script.line_number())
        };
        let file = if sourced.name.is_empty() {
            stack.strip_suffix("..").unwrap_or(&stack).to_owned()
        } else {
            format!("{stack}{}", sourced.name)
        };
        Self::new(
            Cow::Owned(sourced.name),
            file,
            stack,
            Cow::Owned(sourced.text),
            sourced.trust,
            self.depth + 1,
            sourced.modifiers,
        )
    }

    /// Whether a command does not run here (`do_one_cmd()`: `ea.skip`).
    fn skipping(&self) -> bool {
        self.error || !self.blocks.active()
    }

    /// Run `line` next, one `|` command after another.
    fn start(&mut self, line: Line) {
        let pieces = split_ex_pipeline(&line.text);
        self.pieces = pieces.into_iter().rev().map(Cow::into_owned).collect();
        self.line = line;
    }
}

/// What the runner does after a command.
enum Then {
    /// Runs the next command.
    Next,
    /// Reads no line after this one, as after `:vim9script`.
    End,
    /// Runs the file a `:source` gave before the next command.
    Source(Sourced),
}

impl super::VimEngine {
    /// Run a config file's text as Ex commands, line by line, as Vim's
    /// `:source` runs a file.
    ///
    /// A host calls it for a config it reads itself, such as at startup;
    /// `host` answers what the engine cannot know, such as `has()` of a
    /// feature named after the host. `outcome` counts the errors shown.
    /// Nothing is added after the config's own messages: Vim says nothing
    /// once a file is sourced.
    ///
    /// Each line runs as it would from the command line, `|` and command
    /// modifiers included, and what it changes is applied the same way.
    /// A command the host runs comes back as a host request.
    ///
    /// A Vim command vim-core lacks, such as `:syntax`, `:autocmd` or
    /// `:function`, gives E319, as in a Vim built without it, where the
    /// command line sends it to the host. `:filetype` and a command that
    /// is not Vim's, such as one the host defines, still go to the host.
    /// `:scriptencoding` does nothing, as the text is UTF-8 already, and
    /// `:vim9script` gives E319 and ends the file. A Vim option vim-core
    /// lacks acts as one the build lacks: setting it does nothing, asking
    /// for its value gives E519, and `&name` reads 0 or an empty String.
    ///
    /// `:if`, `:elseif`, `:else` and `:endif` choose the lines that run, as
    /// in Vim. A `:while`, `:for` or `:try` block gives E319 and the lines
    /// inside it never run. `:finish` ends the file once the rest of its
    /// line has run; inside a block, the rest of the line does not run.
    ///
    /// An error is shown and, as in Vim, the commands after it do not run,
    /// up to the end of its line or, inside a block, of the outermost
    /// block. `:silent!` hides the error, and then they do. A condition
    /// that fails makes its block never run any branch.
    ///
    /// Each error says where it was raised: its `source` holds the name of
    /// the config and the line Vim gives it at, the first of the lines a
    /// continued line joins, or the line after the last for an error at
    /// the end of the file. The messages and errors go to the message
    /// history, as those of the command line do. Before an error the
    /// history gets the lines Vim's `msg_source()` writes: `Error detected
    /// while processing {name}:` unless the error before it in this call
    /// was raised in the same config, and `line {N}:` unless that one was
    /// also raised at the same line. A config without a name gets the line
    /// labels only.
    ///
    /// `:source {file}` asks `host` for the file (see
    /// [`ConfigHost::source`]). Its text runs in place, before the commands
    /// after the `:source`, as Vim's `cmd_source()` runs it: its errors
    /// name it in Vim's stack form, `a.vim[12]..b.vim` for an error in
    /// `b.vim` sourced at line 12 of `a.vim`, the modifiers before the
    /// `:source`, such as `:silent!`, stay in force inside it, and
    /// `:finish` ends only that file. As in Vim, its errors do not skip
    /// the commands after the `:source`, unless it ends with a block open.
    /// A refused file does, and so does a `:source` inside 200 files that
    /// run, which gives `E169: Command too recursive` without asking the
    /// host. A file the host defers runs when it answers the request with
    /// the text, counted at the same depth.
    ///
    /// There is no buffer: a command that needs one (see
    /// [`ExCommand::needs_buffer`](crate::grammar::types::ExCommand::needs_buffer))
    /// gives E523, and `:setlocal` acts like `:set`. `:setglobal` sets only
    /// the global value, as in Vim.
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
    /// lines are skipped even when the command fails, as those that
    /// vim-core lacks do with E319. Where commands do not run, after an
    /// error or in a block that is not taken, only a heredoc or function
    /// body is skipped, as in Vim. Commands after a `|` on the line that
    /// ends a function run as the rest of that line.
    ///
    /// As in Vim, `:echo` evaluates its expressions, and `:let` and
    /// `:unlet` assign and remove `g:`, `b:` and plain variables and set
    /// options; `g:mapleader` and `g:maplocalleader` give the leaders.
    /// Typed, `:echo` still shows its argument as written, and `:unlet` and
    /// a `:let` of anything but the leaders still go to the host.
    pub fn source_script(
        &mut self,
        text: ConfigText<'_>,
        host: &mut dyn ConfigHost,
    ) -> ConfigResponse {
        let mut outcome = ConfigOutcome::default();
        let response = self.run_script(text, host, 1, &mut outcome);
        let errors = response
            .effects()
            .iter()
            .filter(|e| matches!(e, Effect::ShowError { .. }))
            .count();
        outcome.errors = u32::try_from(errors).unwrap_or(u32::MAX);
        ConfigResponse { response, outcome }
    }

    /// Run a config file's text as [`source_script`](Self::source_script)
    /// runs a config without a name with [`TrustAll`], which allows every
    /// command. The response ends with "Config file sourced".
    #[deprecated(
        since = "0.10.0",
        note = "use `source_script`, whose host judges each command and which says what \
                the config came to; `source_config_text` goes in v0.11.0"
    )]
    pub fn source_config_text(&mut self, text: &str) -> Response {
        self.source_config_named("", text, 1)
    }

    /// Run the config `text` named `name` as
    /// [`source_script`](Self::source_script) runs it with [`TrustAll`],
    /// as the `depth`th of the files that run, as when a host answers
    /// `:source {name}` with the file's text. The response ends with
    /// "Config file sourced".
    pub(crate) fn source_config_named(&mut self, name: &str, text: &str, depth: u32) -> Response {
        let config = ConfigText::new(name, text);
        let mut response =
            self.run_script(config, &mut TrustAll, depth, &mut ConfigOutcome::default());
        let from = response.effects().len();
        response.extend_effects(ex_effects::show_message(VimString::from(
            "Config file sourced",
        )));
        self.settle(&mut response, from);
        response
    }

    /// Run the config `text` for [`source_script`](Self::source_script), as
    /// the `depth`th of the files that run.
    fn run_script(
        &mut self,
        text: ConfigText<'_>,
        host: &mut dyn ConfigHost,
        depth: u32,
        outcome: &mut ConfigOutcome,
    ) -> Response {
        if self.options_dirty {
            self.rebuild_resolved_cache();
            self.rebuild_langmap_if_needed();
        }
        // Vim forgets the place of the last error at the hit-enter prompt
        // after a config with errors.
        self.state.message_history_mut().forget_source();
        let mut response = Response::with_effects(Effects::new());
        let top = Frame::new(
            Cow::Borrowed(text.name),
            text.name.to_owned(),
            String::new(),
            Cow::Borrowed(text.text),
            text.trust,
            depth,
            ModifierFlags::empty(),
        );
        self.run_frames(top, host, outcome, &mut response);
        // Not register_pending_host_requests: its effect optimizer keeps only
        // the last of several messages, and a config can print many.
        self.track_host_requests(&mut response);
        response
    }

    /// Run the file `top` and the files it sources, each on top of the
    /// file that sources it, as nested `do_cmdline()` calls run them, but
    /// on the heap.
    fn run_frames(
        &mut self,
        top: Frame<'_>,
        host: &mut dyn ConfigHost,
        outcome: &mut ConfigOutcome,
        response: &mut Response,
    ) {
        let mut frames = vec![top];
        while let Some(frame) = frames.last_mut() {
            if let Some(piece) = frame.pieces.pop() {
                let sourced = self.source_config_command(frame, host, &piece, outcome, response);
                if let Some(sourced) = sourced {
                    let nested = frame.nested(sourced);
                    frames.push(nested);
                }
                continue;
            }
            if let Some(rest) = frame.rest.take() {
                frame.start(rest);
                continue;
            }
            // do_cmdline(): "stop skipping cmds for an error msg after all
            // endif/while/for".
            if frame.blocks.is_empty() {
                frame.error = false;
            }
            // getsourceline() reads no line once the file is finished.
            let line = if frame.finished {
                None
            } else {
                frame.script.next_line()
            };
            if let Some(line) = line {
                let from = response.effects().len();
                show_warnings(frame, response);
                self.settle(response, from);
                frame.start(line);
                continue;
            }
            if let Some(error) = frame.blocks.missing_end().filter(|_| !frame.finished) {
                let from = response.effects().len();
                show_config_errors(frame.modifiers, [error], response);
                frame.error |= shows_error(response, from);
                locate(response, from, &frame.file, frame.script.end_line());
                self.settle(response, from);
            }
            let error = frame.error;
            frames.pop();
            if let Some(outer) = frames.last_mut() {
                outer.error = error;
            }
        }
    }

    /// Run the next command of the frame's line, reading the lines it takes
    /// after the line from the frame's script. Returns the file a `:source`
    /// gave, which runs before the next command.
    fn source_config_command(
        &mut self,
        frame: &mut Frame<'_>,
        host: &mut dyn ConfigHost,
        piece: &str,
        outcome: &mut ConfigOutcome,
        response: &mut Response,
    ) -> Option<Sourced> {
        // Vim's SOURCING_LNUM: after a command read lines as text, the
        // commands after it are at the last of them.
        let at = frame.script.line_number();
        let start = response.effects().len();
        let skipping = frame.skipping();
        let body = frame.script.read_body(piece, &frame.line, skipping);
        show_warnings(frame, response);
        let from = response.effects().len();
        let mut fails = false;
        let mut sourced = None;
        if let Some(body) = body {
            frame.rest = body.tail;
            fails = body.fails;
            let (modifiers, _) = parse_config_modifiers(piece);
            for (number, error) in body.errors {
                let at_error = response.effects().len();
                show_config_errors(modifiers | frame.modifiers, [error], response);
                locate(response, at_error, &frame.file, number);
            }
        }
        #[cfg(test)]
        let before = seam::begin(self, piece, response);
        if fails {
            frame.error |= shows_error(response, from);
        } else if let Some(command) = script_command(piece) {
            self.source_config_control(frame, host, command, response);
        } else {
            if !skipping {
                match self.source_config_piece(frame, host, piece, at, outcome, response) {
                    Then::Next => {}
                    Then::End => {
                        frame.blocks.finish();
                        frame.finished = true;
                    }
                    Then::Source(file) => sourced = Some(file),
                }
            }
            frame.error |= shows_error(response, from);
        }
        #[cfg(test)]
        seam::end(self, before, response);
        locate(response, start, &frame.file, at);
        self.settle(response, start);
        sourced
    }

    /// Keep in the engine's state what the response's effects after its
    /// first `from` show, as the command line keeps its own: the messages
    /// and errors go to the message history.
    fn settle(&mut self, response: &mut Response, from: usize) {
        if let Some(effects) = response.effects.get_mut(from..) {
            crate::execution::effect_processor::sync_effects(
                &mut self.state,
                &mut self.parser,
                effects,
            );
        }
    }

    /// Run a command of a script block, `:finish`, `:break` or `:continue`
    /// as Vim's `do_one_cmd()`, `ex_eval.c` and `ex_finish()` run them. A
    /// block command runs even while skipping, so that the blocks stay
    /// matched.
    fn source_config_control(
        &self,
        frame: &mut Frame<'_>,
        host: &dyn ConfigHost,
        command: ScriptCommand<'_>,
        response: &mut Response,
    ) {
        let skipping = frame.skipping();
        let modifiers = command.modifiers | frame.modifiers;
        let argument = command.argument;
        // do_one_cmd(): what it refuses before it calls the command.
        let refused = if command.range && !skipping {
            Some(VimError::NoRangeAllowed)
        } else if command.bang {
            Some(VimError::NoBangAllowed)
        } else if !command.takes_argument() && !argument.is_empty() {
            Some(VimError::TrailingCharacters(argument.into()))
        } else {
            None
        };
        if let Some(error) = refused {
            return report(frame, modifiers, error, response);
        }
        let blocks = &mut frame.blocks;
        let error = &mut frame.error;
        let result = match command.control {
            Control::Block(BlockKind::If) => blocks.open_if(*error, || {
                self.source_config_condition(host, argument, modifiers, error, response)
            }),
            Control::Block(BlockKind::ElseIf) => blocks.add_elseif(*error, |skip| {
                if !skip {
                    return self
                        .source_config_condition(host, argument, modifiers, error, response);
                }
                // ex_else(): "When skipping we ignore most errors, but a
                // missing expression is wrong".
                if matches!(argument.as_bytes().first(), None | Some(b'|' | b'\n')) {
                    let missing = VimError::InvalidExpression(argument.into());
                    emsg(error, modifiers, missing, response);
                }
                None
            }),
            Control::Block(BlockKind::Else) => blocks.add_else(*error),
            Control::Block(BlockKind::EndIf) => blocks.close_if(),
            Control::Block(kind @ (BlockKind::While | BlockKind::For | BlockKind::Try)) => {
                let block = match kind {
                    BlockKind::While => Kind::While,
                    BlockKind::For => Kind::For,
                    _ => Kind::Try,
                };
                let opened = blocks.open_never_active(block);
                if opened.is_ok() && !skipping {
                    let mut form = VimString::from(kind.name());
                    if !argument.is_empty() {
                        form.push(' ');
                        form.push_str(argument);
                    }
                    emsg(error, modifiers, VimError::NotAvailable(form), response);
                }
                opened
            }
            Control::Block(BlockKind::EndWhile) => blocks.close_loop(Kind::While),
            Control::Block(BlockKind::EndFor) => blocks.close_loop(Kind::For),
            Control::Block(BlockKind::Catch) => blocks.continue_try(VimError::CatchWithoutTry),
            Control::Block(BlockKind::Finally) => blocks.continue_try(VimError::FinallyWithoutTry),
            Control::Block(BlockKind::EndTry) => blocks.close_try(error),
            Control::Break if !skipping => blocks.leave_loop(VimError::BreakWithoutLoop),
            Control::Continue if !skipping => blocks.leave_loop(VimError::ContinueWithoutLoop),
            Control::Finish if !skipping => {
                blocks.finish();
                frame.finished = true;
                Ok(())
            }
            Control::Break | Control::Continue | Control::Finish => Ok(()),
        };
        if let Err(error) = result {
            report(frame, modifiers, error, response);
        }
    }

    /// Whether the condition `text` of `:if` or `:elseif` is true, as
    /// `eval_to_bool()` decides it; `None` when it fails, after its errors
    /// are shown.
    fn source_config_condition(
        &self,
        host: &dyn ConfigHost,
        text: &str,
        modifiers: ModifierFlags,
        error: &mut bool,
        response: &mut Response,
    ) -> Option<bool> {
        let evaluated = script_expr::evaluate(text, &self.script_env(host));
        let mut errors = evaluated.errors;
        let result = match evaluated.value.map(|value| value.number()) {
            Some(Ok(n)) => Some(n != 0),
            Some(Err(err)) => {
                errors.push(err);
                None
            }
            None => None,
        };
        for err in errors {
            emsg(error, modifiers, err, response);
        }
        result
    }

    /// Run one command of a config line as the command line runs it,
    /// with the modifiers in force in the frame, once the engine found
    /// nothing wrong with it and `host` allows it.
    fn source_config_piece(
        &mut self,
        frame: &Frame<'_>,
        host: &mut dyn ConfigHost,
        piece: &str,
        line: u32,
        outcome: &mut ConfigOutcome,
        response: &mut Response,
    ) -> Then {
        let checked = match self.check_config_piece(frame, host, piece, response) {
            Ok(checked) => checked,
            Err(then) => return then,
        };
        let options = checked.option_targets();
        let mapping_reach = match &checked.command {
            ExCommand::Map {
                mode_prefix,
                rhs: Some(rhs),
                kind,
                flags,
                ..
            } => Some(self.mapping_reach(*mode_prefix, rhs, *kind, *flags)),
            _ => None,
        };
        let map = mapping_reach
            .as_ref()
            .and_then(|reach| map_definition(&checked.command, reach));
        let judged = checked.command.clone();
        let verdict = host.judge(&ConfigCommand {
            file: &frame.name,
            trust: frame.trust,
            depth: frame.depth,
            line,
            text: piece.trim_matches([' ', '\t']),
            modifiers: checked.modifiers,
            action: checked.action(&options, map.as_ref()),
            reach: checked.reach,
            command: &judged,
        });
        outcome.commands = outcome.commands.saturating_add(1);
        #[cfg(test)]
        seam::judged(&judged, &verdict);
        let refusal = match verdict {
            Verdict::Allow if judged == checked.command => None,
            Verdict::Allow => {
                debug_assert!(false, "{judged:?} was judged, {:?} runs", checked.command);
                Some("E48: Not allowed here: the command is not the one judged".into())
            }
            Verdict::Refuse(text) => Some(text),
            Verdict::Claim => {
                outcome.claimed = outcome.claimed.saturating_add(1);
                return Then::Next;
            }
        };
        if let Some(text) = refusal {
            outcome.refused = outcome.refused.saturating_add(1);
            show_config_errors(checked.modifiers, [VimError::Refused(text)], response);
            return Then::Next;
        }
        self.run_checked(frame, host, checked, response)
    }

    /// What `piece` will run, after the checks the engine makes itself:
    /// its parse, E319 for a Vim command vim-core lacks, E169 for a
    /// `:source` too deep, E523 for a command that needs a buffer, and the
    /// value a `:let` assigns. Otherwise its errors are shown, and what
    /// the runner does next.
    fn check_config_piece(
        &self,
        frame: &Frame<'_>,
        host: &dyn ConfigHost,
        piece: &str,
        response: &mut Response,
    ) -> Result<Checked, Then> {
        // Deviation: the modifiers only a config takes change nothing.
        let (modifiers, text) = parse_config_modifiers(piece);
        let inherited = frame.modifiers | modifiers;
        match vim_command_parts(text) {
            Some((modifiers, "echo", argument)) => {
                let command = ExCommand::Echo {
                    message: argument.into(),
                };
                return Ok(Checked::new(modifiers | inherited, command, Run::Echo));
            }
            Some((modifiers, "let", argument)) => {
                return self
                    .check_config_let(host, modifiers | inherited, argument, response)
                    .ok_or(Then::Next);
            }
            Some((modifiers, "unlet", argument)) => {
                return check_config_unlet(modifiers | inherited, argument, response)
                    .ok_or(Then::Next);
            }
            _ => {}
        }
        let (modifiers, parsed) = parse_ex_piece_at(text, self.nesting);
        let modifiers = modifiers | inherited;
        let command = match parsed {
            Ok(command) => command,
            Err(err) => {
                show_config_errors(modifiers, [err], response);
                return Err(Then::Next);
            }
        };
        if let ExCommand::Source { .. } = command {
            if frame.depth >= MAX_DEPTH {
                show_config_errors(modifiers, [VimError::CommandTooRecursive], response);
                return Err(Then::Next);
            }
            return Ok(Checked::new(modifiers, command, Run::Source));
        }
        if let Some(name) = lacking_vim_command(&command) {
            if source_config_lacking(modifiers, name, piece, response) {
                return Err(Then::End);
            }
            return Err(Then::Next);
        }
        if command.needs_buffer() {
            let error = VimError::NotAllowedHere(VimString::from(piece.trim()));
            show_config_errors(modifiers, [error], response);
            return Err(Then::Next);
        }
        Ok(Checked::new(modifiers, command, Run::Command))
    }

    /// Run a command the host allowed.
    fn run_checked(
        &mut self,
        frame: &Frame<'_>,
        host: &mut dyn ConfigHost,
        checked: Checked,
        response: &mut Response,
    ) -> Then {
        let Checked {
            modifiers,
            command,
            run,
            ..
        } = checked;
        #[cfg(test)]
        seam::executed(&command);
        match (run, &command) {
            (Run::Command, _) => {
                self.run_config_command(modifiers, &command, frame.trust, response);
            }
            (Run::Echo, ExCommand::Echo { message }) => {
                self.source_config_echo(host, modifiers, message, response);
            }
            (
                Run::Variable {
                    scope, name, value, ..
                },
                _,
            ) => {
                self.state
                    .variable_store_mut()
                    .set(scope, &name, value.into_variable());
                self.sync_leader_variable(scope, &name);
            }
            (Run::Unlet(argument), _) => self.source_config_unlet(modifiers, &argument, response),
            (Run::Source, ExCommand::Source { path }) => {
                return self.source_config_source(
                    frame,
                    host,
                    modifiers,
                    path.as_deref(),
                    response,
                );
            }
            (Run::Echo | Run::Source | Run::Nothing(_), _) => {}
        }
        Then::Next
    }

    /// `:source {file}` in a config, as `cmd_source()` runs it: the host
    /// gives the file's text, which runs before the next command, or
    /// sources it itself, refuses it, or leaves it for later as a
    /// `ReadConfigFile` request.
    fn source_config_source(
        &mut self,
        frame: &Frame<'_>,
        host: &mut dyn ConfigHost,
        modifiers: ModifierFlags,
        path: Option<&str>,
        response: &mut Response,
    ) -> Then {
        let depth = frame.depth + 1;
        let request = SourceRequest {
            path,
            from_file: &frame.name,
            from_trust: frame.trust,
            line: frame.script.line_number(),
            depth,
        };
        match host.source(&request) {
            SourceReply::Text { name, text, trust } => {
                return Then::Source(Sourced {
                    name,
                    text,
                    trust,
                    modifiers,
                });
            }
            SourceReply::Done(message) => {
                let mut effects = Effects::new();
                if let Some(message) = message {
                    effects.extend(ex_effects::show_message(message.into()));
                }
                crate::execution::modifier_filter::apply_modifiers(&mut effects, modifiers);
                response.extend_effects(effects);
            }
            SourceReply::Refuse(text) => {
                let error = VimError::Refused(text.into_boxed_str());
                show_config_errors(modifiers, [error], response);
            }
            SourceReply::Defer => {
                let request = HostRequest::ReadConfigFile {
                    meta: self.host.sequencer.next_meta(),
                    path: path.map(VimString::from),
                };
                self.host.source_depths.insert(request.id(), depth);
                response.host_requests.push(request);
            }
        }
        Then::Next
    }

    /// Run a parsed config command in a config tagged `trust`.
    fn run_config_command(
        &mut self,
        modifiers: ModifierFlags,
        parsed: &ExCommand,
        trust: TrustTag,
        response: &mut Response,
    ) {
        let input = InputContext::new(&NoBuffer, 0).validate_clamped();
        let ctx = ExecutionContext::new(input, &self.state, &self.options);
        let result = executor_ex::execute_ex_command(parsed, &ctx, &mut self.host.sequencer);
        let mut output = match result {
            Ok(output) => output,
            Err(err) => return show_config_errors(modifiers, [err], response),
        };
        crate::execution::modifier_filter::apply_modifiers(&mut output.effects, modifiers);
        // No buffer or window to hold a local value.
        for set in &mut output.set_commands {
            if set.scope == SetScope::Local {
                set.scope = SetScope::Effective;
            }
        }
        let mut applied = self.apply_ex_output(&mut output, "", trust);
        crate::execution::modifier_filter::apply_modifiers(&mut applied, modifiers);
        response.extend_effects(output.effects);
        response.extend_effects(applied);
        response.host_requests.extend(output.host_requests);
    }

    /// What an expression in a config reads.
    const fn script_env<'a>(&'a self, host: &'a dyn ConfigHost) -> script_expr::Env<'a> {
        script_expr::Env {
            variables: self.state.variable_store(),
            options: &self.resolved_options,
            global_options: &self.options,
            host,
        }
    }

    /// `:let` in a config, as Vim's `ex_let()` runs it: a `g:`, `b:` or
    /// plain variable takes the value of the expression, or adds to it
    /// with `+=`, `-=`, `*=`, `/=`, `%=`, `.=` or `..=`, and an option is
    /// set as `:set` sets it. Any other form gives E319. A typed `:let`
    /// still goes to the host.
    ///
    /// The value is computed here, before the host judges the command;
    /// `None` when that fails, after its errors are shown.
    fn check_config_let(
        &self,
        host: &dyn ConfigHost,
        modifiers: ModifierFlags,
        argument: &str,
        response: &mut Response,
    ) -> Option<Checked> {
        let argument = argument.trim_start_matches([' ', '\t']);
        let Some(assignment) = script_expr::parse_let(argument) else {
            let form = VimString::from(format!("let {argument}"));
            show_config_errors(modifiers, [VimError::NotAvailable(form)], response);
            return None;
        };
        let evaluated = script_expr::evaluate(assignment.expr, &self.script_env(host));
        let mut errors = evaluated.errors;
        let Some(value) = evaluated.value else {
            show_config_errors(modifiers, errors, response);
            return None;
        };
        let written = || ExCommand::Custom {
            command: VimString::from(format!("let {argument}")),
        };
        let checked = match assignment.target {
            Target::Variable(variable) => {
                match self.assigned_value(variable, assignment.op, value) {
                    Ok(value) => {
                        let leader = leader_variable(variable.scope, variable.name);
                        let command = match (leader, assignment.op) {
                            (Some(local), None) => ExCommand::LetLeader {
                                local,
                                value: assignment.expr.into(),
                            },
                            _ => written(),
                        };
                        let run = Run::Variable {
                            scope: variable.scope,
                            name: variable.name.to_owned(),
                            leader: leader.map(|local| (local, value.text().into_owned())),
                            value,
                        };
                        Some(Checked::new(modifiers, command, run))
                    }
                    Err(err) => {
                        errors.push(err);
                        None
                    }
                }
            }
            Target::Option(layer, name) => {
                match self.let_option(layer, name, assignment.op, &value) {
                    Ok(Some(command)) => Some(Checked::new(modifiers, command, Run::Command)),
                    Ok(None) => {
                        let run = Run::Nothing(name.to_owned());
                        Some(Checked::new(modifiers, written(), run))
                    }
                    Err(err) => {
                        errors.push(err);
                        None
                    }
                }
            }
        };
        show_config_errors(modifiers, errors, response);
        checked
    }

    /// The value `variable` takes from `value` with the operator `op`.
    fn assigned_value(
        &self,
        variable: script_expr::Variable<'_>,
        op: Option<char>,
        value: Value,
    ) -> Result<Value, VimError> {
        let Some(op) = op else {
            return Ok(value);
        };
        let current = self
            .state
            .variable_store()
            .get(variable.scope, variable.name)
            .ok_or_else(|| VimError::UndefinedVariable(variable.written.into()))?;
        // A List or a Dictionary.
        let current = Value::from_variable(current).ok_or(VimError::WrongVariableType(op))?;
        script_expr::assign_op(op, &current, &value)
    }

    /// The `:set` that `:let &name = value` amounts to, as Vim's
    /// `ex_let_option()` makes it: a Number for a boolean or number option
    /// (E805 for a Float), a String for a string option, and `.=` only for
    /// a string option. `None` for an option this build lacks, which takes
    /// the value and keeps nothing.
    fn let_option(
        &self,
        layer: OptionLayer,
        name: &str,
        op: Option<char>,
        value: &Value,
    ) -> Result<Option<ExCommand>, VimError> {
        let options = if layer == OptionLayer::Global {
            &self.options
        } else {
            &self.resolved_options
        };
        let Some((name, current)) = executor_ex::option_value(options, name) else {
            return match executor_ex::hidden_option(options, name) {
                Some(SetKind::Text) if matches!(value, Value::Bool(_)) => {
                    Err(VimError::StringRequired)
                }
                Some(SetKind::Text) => Ok(None),
                Some(_) => value.number().map(|_| None),
                None => Err(VimError::UnknownOptionToSet(name.into())),
            };
        };
        let name = VimString::from(name);
        let assignment = match current {
            OptionValue::Str(old) => {
                if let Some(op) = op.filter(|&op| op != '.') {
                    return Err(VimError::WrongVariableType(op));
                }
                if matches!(value, Value::Bool(_)) {
                    return Err(VimError::StringRequired);
                }
                let mut text = if op.is_some() {
                    old
                } else {
                    VimString::default()
                };
                text.push_str(&value.text());
                // :set takes the value without the backslash before each
                // character.
                SetAssignment::Assign(name, text.replace('\\', r"\\").into())
            }
            number => {
                if op == Some('.') {
                    return Err(VimError::WrongVariableType('.'));
                }
                let old = match number {
                    OptionValue::Bool(on) => i64::from(on),
                    OptionValue::Unsigned(n) => i64::try_from(n).unwrap_or(i64::MAX),
                    OptionValue::Signed(n) => n,
                    OptionValue::Str(_) => 0,
                };
                let n = value.number()?;
                let n = op.map_or(n, |op| script_expr::number_op(op, old, n));
                match number {
                    OptionValue::Bool(_) if n != 0 => SetAssignment::SetBool(name),
                    OptionValue::Bool(_) => SetAssignment::UnsetBool(name),
                    _ => SetAssignment::Assign(name, VimString::from(n.to_string())),
                }
            }
        };
        if let Some(error) = executor_ex::let_option_error(options, &assignment) {
            return Err(error);
        }
        let mut assignments = smallvec::SmallVec::new();
        assignments.push(assignment);
        Ok(Some(match layer {
            OptionLayer::Both => ExCommand::Set { assignments },
            OptionLayer::Local => ExCommand::SetLocal { assignments },
            OptionLayer::Global => ExCommand::SetGlobal { assignments },
        }))
    }

    /// `:unlet[!] {name} ...` in a config, as Vim's `ex_unlet()` runs it:
    /// each `g:`, `b:` or plain variable goes; one that does not exist is
    /// E108 unless with `!`. Any other name gives E319.
    fn source_config_unlet(
        &mut self,
        modifiers: ModifierFlags,
        argument: &str,
        response: &mut Response,
    ) {
        let (bang, names) = match argument.strip_prefix('!') {
            Some(names) => (true, names),
            None => (false, argument),
        };
        let mut rest = names.trim_matches([' ', '\t']);
        if rest.is_empty() {
            return show_config_errors(modifiers, [VimError::ArgumentRequired], response);
        }
        while !rest.is_empty() {
            let Some((variable, end)) = script_expr::variable_name(rest) else {
                let form = VimString::from(format!("unlet{argument}"));
                return show_config_errors(modifiers, [VimError::NotAvailable(form)], response);
            };
            let after = rest.get(end..).unwrap_or_default();
            if !after.is_empty() && !after.starts_with([' ', '\t']) {
                let error = VimError::TrailingCharacters(after.into());
                return show_config_errors(modifiers, [error], response);
            }
            let deleted = self
                .state
                .variable_store_mut()
                .delete(variable.scope, variable.name);
            if !deleted && !bang {
                let error = VimError::NoSuchVariable(variable.written.into());
                return show_config_errors(modifiers, [error], response);
            }
            self.sync_leader_variable(variable.scope, variable.name);
            rest = after.trim_start_matches([' ', '\t']);
        }
    }

    /// Take the leader again when the variable `name` of `scope` is
    /// `g:mapleader` or `g:maplocalleader`.
    fn sync_leader_variable(&mut self, scope: VarScope, name: &str) {
        if let Some(local) = leader_variable(scope, name) {
            self.sync_leader(local);
        }
    }

    /// `:echo` in a config: its expressions are evaluated, as Vim's
    /// `ex_echo()` does. A typed `:echo` shows its argument as written.
    fn source_config_echo(
        &self,
        host: &dyn ConfigHost,
        modifiers: ModifierFlags,
        argument: &str,
        response: &mut Response,
    ) {
        let mut effects = Effects::new();
        for shown in script_expr::echo(argument, &self.script_env(host)) {
            match shown {
                Echoed::Text(text) => effects.extend(ex_effects::show_message(text.into())),
                Echoed::Error(err) => effects.extend(ex_effects::show_error(err)),
            }
        }
        crate::execution::modifier_filter::apply_modifiers(&mut effects, modifiers);
        response.extend_effects(effects);
    }
}

/// A config command the engine found nothing wrong with, before the host
/// judges it.
struct Checked {
    /// Its modifiers and those in force in its file.
    modifiers: ModifierFlags,
    /// The command the host judges and the engine runs.
    command: ExCommand,
    /// How it runs.
    run: Run,
    /// What running it reaches.
    reach: Reach,
}

/// How a checked command runs.
enum Run {
    /// As the command line runs it.
    Command,
    /// `:echo`, whose argument is evaluated.
    Echo,
    /// `:let` of a variable, with the value it takes and, for a leader
    /// variable, whether it is the local one and its text.
    Variable {
        scope: VarScope,
        name: String,
        value: Value,
        leader: Option<(bool, String)>,
    },
    /// `:let` of the option `name`, which vim-core lacks: nothing is kept.
    Nothing(String),
    /// `:unlet`, with the text after its name.
    Unlet(String),
    /// `:source`.
    Source,
}

impl Checked {
    fn new(modifiers: ModifierFlags, command: ExCommand, run: Run) -> Self {
        let reach = match &run {
            Run::Variable { leader: None, .. } => Reach::VARIABLE,
            Run::Variable { .. } => Reach::VARIABLE | Reach::LEADER,
            Run::Nothing(name) => option_reach(name),
            Run::Unlet(argument) => unlet_reach(argument),
            Run::Command | Run::Echo | Run::Source => command.reach(),
        };
        Self {
            modifiers,
            command,
            run,
            reach,
        }
    }

    /// The options it sets or shows.
    fn option_targets(&self) -> Vec<OptionTarget> {
        match (&self.run, &self.command) {
            (Run::Nothing(name), _) => vec![option_target(name)],
            (
                Run::Command,
                ExCommand::Set { assignments }
                | ExCommand::SetLocal { assignments }
                | ExCommand::SetGlobal { assignments },
            ) => assignments
                .iter()
                .filter_map(SetAssignment::option)
                .map(option_target)
                .collect(),
            _ => Vec::new(),
        }
    }

    /// What it does, for the host.
    fn action<'a>(
        &'a self,
        options: &'a [OptionTarget],
        map: Option<&'a MapDefinition<'a>>,
    ) -> ConfigAction<'a> {
        match &self.run {
            Run::Command => command_action(&self.command, self.reach, options, map),
            Run::Echo => ConfigAction::Message,
            Run::Variable {
                leader: Some((local, value)),
                ..
            } => ConfigAction::Leader {
                local: *local,
                value,
            },
            Run::Variable { .. } | Run::Unlet(_) => ConfigAction::Other,
            Run::Nothing(_) => ConfigAction::SetOptions { options },
            Run::Source => match &self.command {
                ExCommand::Source { path } => ConfigAction::Source {
                    path: path.as_deref(),
                },
                _ => ConfigAction::Other,
            },
        }
    }
}

/// What the command line's `command`, which reaches `reach`, does, for a
/// host judging it in a config.
fn command_action<'a>(
    command: &'a ExCommand,
    reach: Reach,
    options: &'a [OptionTarget],
    map: Option<&'a MapDefinition<'a>>,
) -> ConfigAction<'a> {
    match command {
        ExCommand::Set { .. } | ExCommand::SetLocal { .. } | ExCommand::SetGlobal { .. } => {
            ConfigAction::SetOptions { options }
        }
        ExCommand::Map { .. } => map.map_or(ConfigAction::Message, ConfigAction::Map),
        ExCommand::Unmap {
            mode_prefix, lhs, ..
        } => ConfigAction::Unmap {
            modes: *mode_prefix,
            lhs,
        },
        ExCommand::MapClear { mode, .. } => ConfigAction::MapClear { modes: *mode },
        ExCommand::Abbreviate {
            trigger: Some(trigger),
            replacement: Some(replacement),
            mode,
            ..
        } => ConfigAction::Abbreviate {
            mode: *mode,
            trigger,
            replacement,
        },
        ExCommand::Unabbreviate { trigger, mode } => ConfigAction::Unabbreviate {
            mode: *mode,
            trigger,
        },
        ExCommand::AbClear { mode, .. } => ConfigAction::AbClear { mode: *mode },
        ExCommand::LetLeader { local, value } => ConfigAction::Leader {
            local: *local,
            value,
        },
        ExCommand::SetHandler { .. } => ConfigAction::Handler,
        ExCommand::Abbreviate { .. }
        | ExCommand::Echo { .. }
        | ExCommand::NoHighlight
        | ExCommand::Registers { .. }
        | ExCommand::Messages { .. }
        | ExCommand::ActionList { .. } => ConfigAction::Message,
        ExCommand::Source { path } => ConfigAction::Source {
            path: path.as_deref(),
        },
        ExCommand::Custom { command } => custom_action(command),
        _ if reach.contains(Reach::HOST) => ConfigAction::Host,
        _ => ConfigAction::Other,
    }
}

/// The mapping `command` defines, if it defines one, whose keys reach
/// `reach`.
fn map_definition<'a>(
    command: &'a ExCommand,
    reach: &'a MappingReach,
) -> Option<MapDefinition<'a>> {
    match command {
        ExCommand::Map {
            mode_prefix,
            lhs,
            rhs: Some(rhs),
            kind,
            flags,
            ..
        } => Some(MapDefinition {
            modes: *mode_prefix,
            lhs,
            rhs,
            kind: *kind,
            flags: *flags,
            reach,
        }),
        _ => None,
    }
}

/// The parts of a command the engine leaves to the host: its name, in
/// full for one of Vim's, the text after it, and whether it is Vim's.
fn custom_action(command: &str) -> ConfigAction<'_> {
    let command = command.trim_start_matches([' ', '\t', ':']);
    if let Some((_, name, args)) = vim_command_parts(command) {
        return ConfigAction::Custom {
            name,
            args: args.trim_matches([' ', '\t']),
            vim_command: true,
        };
    }
    let end = command
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .unwrap_or(command.len());
    let (name, args) = command.split_at(end);
    ConfigAction::Custom {
        name,
        args: args.trim_matches([' ', '\t']),
        vim_command: false,
    }
}

/// What a host sees of the option `written` names, with any `no` or `inv`
/// before it.
fn option_target(written: &str) -> OptionTarget {
    let named = crate::grammar::types::option_target(written);
    OptionTarget {
        name: named.full.unwrap_or(written).into(),
        known: named.known,
        secure: named.secure,
    }
}

/// What setting the option `name` reaches.
fn option_reach(name: &str) -> Reach {
    if option_target(name).secure {
        Reach::OPTIONS | Reach::SECURE_OPTION
    } else {
        Reach::OPTIONS
    }
}

/// Whether the variable `name` of `scope` holds a leader, and whether the
/// local one.
fn leader_variable(scope: VarScope, name: &str) -> Option<bool> {
    match (scope, name) {
        (VarScope::Global, "mapleader") => Some(false),
        (VarScope::Global, "maplocalleader") => Some(true),
        _ => None,
    }
}

/// What `:unlet` with the text `argument` after its name reaches.
fn unlet_reach(argument: &str) -> Reach {
    let names = argument.strip_prefix('!').unwrap_or(argument);
    let leader = names.split([' ', '\t']).any(|name| {
        script_expr::variable_name(name)
            .is_some_and(|(variable, _)| leader_variable(variable.scope, variable.name).is_some())
    });
    if leader {
        Reach::VARIABLE | Reach::LEADER
    } else {
        Reach::VARIABLE
    }
}

/// `:unlet` before the host judges it: E471 without a name.
fn check_config_unlet(
    modifiers: ModifierFlags,
    argument: &str,
    response: &mut Response,
) -> Option<Checked> {
    let names = argument.strip_prefix('!').unwrap_or(argument);
    if names.trim_matches([' ', '\t']).is_empty() {
        show_config_errors(modifiers, [VimError::ArgumentRequired], response);
        return None;
    }
    let command = ExCommand::Custom {
        command: VimString::from(format!("unlet{argument}")),
    };
    Some(Checked::new(
        modifiers,
        command,
        Run::Unlet(argument.to_owned()),
    ))
}

/// The Vim command that `parsed` runs when it is one the engine leaves to
/// the host and vim-core lacks; not `:filetype`, which hosts run, nor a
/// command that is not Vim's, such as one the host defines.
fn lacking_vim_command(parsed: &ExCommand) -> Option<&'static str> {
    let ExCommand::Custom { command } = parsed else {
        return None;
    };
    // find_ex_command(): a name that starts with a capital is a user
    // command, unless the case of Vim's `:Next`, `:Print` or `:X` matches.
    let written_len = command.bytes().take_while(u8::is_ascii_alphabetic).count();
    let written = command.get(..written_len)?;
    if written.starts_with(|c: char| c.is_ascii_uppercase())
        && !["Next", "Print", "X"]
            .iter()
            .any(|full| full.starts_with(written))
    {
        return None;
    }
    let (_, name, _) = vim_command_parts(command)?;
    (name != "filetype").then_some(name)
}

/// Run the Vim command `name`, which vim-core lacks, written as `piece`:
/// E319, as `ex_ni()` gives it in a Vim built without the command.
/// Returns whether no line is read after this one.
fn source_config_lacking(
    modifiers: ModifierFlags,
    name: &str,
    piece: &str,
    response: &mut Response,
) -> bool {
    // ex_scriptencoding(): the text is UTF-8 already.
    if name == "scriptencoding" {
        return false;
    }
    let error = VimError::NotAvailable(VimString::from(piece.trim()));
    show_config_errors(modifiers, [error], response);
    // Deviation: Vim reads the rest of the file as Vim9 script, which the
    // engine does not read.
    name == "vim9script"
}

/// Show `errors` as the command's modifiers let them.
fn show_config_errors(
    modifiers: ModifierFlags,
    errors: impl IntoIterator<Item = VimError>,
    response: &mut Response,
) {
    let mut effects = Effects::new();
    for error in errors {
        effects.extend(ex_effects::show_error(error));
    }
    crate::execution::modifier_filter::apply_modifiers(&mut effects, modifiers);
    response.extend_effects(effects);
}

/// Show `error` as Vim's `emsg()` does: not at all under `:silent!`, and
/// otherwise counted in `flag`, Vim's `did_emsg`.
fn emsg(flag: &mut bool, modifiers: ModifierFlags, error: VimError, response: &mut Response) {
    if !modifiers.contains(ModifierFlags::SILENT_BANG) {
        response.extend_effects(ex_effects::show_error(error));
        *flag = true;
    }
}

/// Show the error a command refused with, as `do_one_cmd()` shows
/// `errormsg`: only when no error was shown before it.
fn report(
    frame: &mut Frame<'_>,
    modifiers: ModifierFlags,
    error: VimError,
    response: &mut Response,
) {
    if !frame.error {
        emsg(&mut frame.error, modifiers, error, response);
    }
}

/// Whether the response shows an error after its first `from` effects.
fn shows_error(response: &Response, from: usize) -> bool {
    response
        .effects()
        .get(from..)
        .is_some_and(|new| new.iter().any(|e| matches!(e, Effect::ShowError { .. })))
}

/// Show the warnings of the lines the frame's script read since the last
/// call, each at its line.
fn show_warnings(frame: &mut Frame<'_>, response: &mut Response) {
    for (line, warning) in frame.script.take_warnings() {
        let from = response.effects().len();
        show_config_errors(frame.modifiers, [warning], response);
        locate(response, from, &frame.file, line);
    }
}

/// Place each error the response shows after its first `from` effects
/// that has no place yet at `line` of the config `file`.
fn locate(response: &mut Response, from: usize, file: &str, line: u32) {
    for effect in response.effects.iter_mut().skip(from) {
        if let Effect::ShowError { source, .. } = effect {
            source.get_or_insert_with(|| SourceContext::new(file, line));
        }
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

/// A record of each command a config runs, for the tests of the seam
/// between the engine and [`ConfigHost::judge`].
#[cfg(test)]
pub(super) mod seam {
    use super::super::{Response, VimEngine};
    use crate::execution::config::Verdict;
    use crate::grammar::types::ExCommand;
    use std::cell::RefCell;

    /// One command of a config line.
    #[derive(Debug)]
    pub(crate) struct Entry {
        /// The command as written.
        pub(crate) text: String,
        /// What the host judged and decided.
        pub(crate) judged: Vec<(ExCommand, Verdict)>,
        /// What the engine ran.
        pub(crate) executed: Vec<ExCommand>,
        /// The engine's state changed.
        pub(crate) changed: bool,
        /// The host requests it made.
        pub(crate) requests: usize,
    }

    thread_local! {
        static LOG: RefCell<Option<Vec<Entry>>> = const { RefCell::new(None) };
    }

    /// Record the commands configs run on this thread from now on.
    pub(crate) fn start() {
        LOG.with(|log| *log.borrow_mut() = Some(Vec::new()));
    }

    /// Whether [`start`] was called and [`take`] not since.
    pub(crate) fn recording() -> bool {
        LOG.with(|log| log.borrow().is_some())
    }

    /// The commands recorded since [`start`], which stops recording.
    pub(crate) fn take() -> Vec<Entry> {
        LOG.with(|log| log.borrow_mut().take()).unwrap_or_default()
    }

    fn with_last(f: impl FnOnce(&mut Entry)) {
        LOG.with(|log| {
            if let Some(entry) = log.borrow_mut().as_mut().and_then(|log| log.last_mut()) {
                f(entry);
            }
        });
    }

    /// The state before the command `text` runs.
    pub(crate) fn begin(engine: &VimEngine, text: &str, response: &Response) -> (String, usize) {
        LOG.with(|log| {
            if let Some(log) = log.borrow_mut().as_mut() {
                log.push(Entry {
                    text: text.to_owned(),
                    judged: Vec::new(),
                    executed: Vec::new(),
                    changed: false,
                    requests: 0,
                });
            }
        });
        (engine.config_fingerprint(), response.host_requests.len())
    }

    pub(crate) fn end(engine: &VimEngine, before: (String, usize), response: &Response) {
        let changed = engine.config_fingerprint() != before.0;
        let requests = response.host_requests.len() - before.1;
        with_last(|entry| {
            entry.changed = changed;
            entry.requests = requests;
        });
    }

    pub(crate) fn judged(command: &ExCommand, verdict: &Verdict) {
        with_last(|entry| entry.judged.push((command.clone(), verdict.clone())));
    }

    pub(crate) fn executed(command: &ExCommand) {
        with_last(|entry| entry.executed.push(command.clone()));
    }
}

#[cfg(test)]
impl super::VimEngine {
    /// Run `text` as a config without a name that allows every command.
    pub(crate) fn source_config_str(&mut self, text: &str) -> Response {
        self.source_config_named("", text, 1)
    }

    /// What a config command can change in the engine, as text.
    fn config_fingerprint(&self) -> String {
        if !seam::recording() {
            return String::new();
        }
        format!(
            "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{}|{}",
            self.options,
            self.buffer_overrides,
            self.window_overrides,
            self.keymap,
            self.abbrev_table,
            self.handler_map,
            self.state.variable_store(),
            self.langmap_table,
            self.host.source_depths,
            self.state.message_history().len(),
            self.state.jump_list().len(),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::effects::Effect;
    use crate::execution::host::HostRequestKind;
    use crate::execution::{parse_keys_from_string, HostSession, Response, VimEngine};
    use crate::primitives::{AbbrevMode, OptionId, OptionValue};

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
                Effect::ShowInfo { info } => Some(info.to_string()),
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
    fn source_config_abclear_clears_every_abbreviation() {
        // map_clear_mode() takes the Insert and Command-line bits off each.
        let mut session = sourced("x", "iabbrev a b\ncabbrev c d\nabclear\n");
        assert!(session.engine().abbrev_table.is_empty());
        type_keys(&mut session, "A a <Esc>");
        assert_eq!(session.text(), "x a ");
    }

    #[test]
    fn source_config_iabclear_keeps_the_command_line_part() {
        let mut session = sourced("x", "abbreviate e f\niabclear\n");
        let table = &session.engine().abbrev_table;
        assert!(!table.defines("e", AbbrevMode::Insert));
        assert_eq!(
            table
                .find("e", AbbrevMode::CommandLine)
                .unwrap()
                .replacement,
            "f"
        );
        type_keys(&mut session, "A e <Esc>");
        assert_eq!(session.text(), "x e ");
    }

    #[test]
    fn source_config_abclear_buffer_clears_nothing() {
        // vim-core has no buffer abbreviations for map_clear() to clear.
        let mut session = sourced("x", "iabbrev teh the\nabclear <buffer>\n");
        type_keys(&mut session, "A teh <Esc>");
        assert_eq!(session.text(), "x the ");
    }

    // ── trust tags ────────────────────────────────────────────────

    /// A host whose files are `(name, text, tag)`.
    struct TaggedFiles(&'static [(&'static str, &'static str, u32)]);

    impl crate::execution::ConfigHost for TaggedFiles {
        fn source(
            &mut self,
            request: &crate::execution::SourceRequest<'_>,
        ) -> crate::execution::SourceReply {
            let found = self.0.iter().find(|(name, ..)| Some(*name) == request.path);
            match found {
                Some(&(name, text, tag)) => crate::execution::SourceReply::Text {
                    name: name.to_owned(),
                    text: text.to_owned(),
                    trust: crate::execution::TrustTag::new(tag),
                },
                None => crate::execution::SourceReply::Refuse(format!("E484: {request:?}")),
            }
        }
    }

    /// The tag of the Normal mapping of `lhs`.
    fn mapping_tag(engine: &VimEngine, lhs: char) -> Option<u32> {
        use crate::keymap::KeyEvent;
        use crate::primitives::Mode;
        let entry = engine
            .keymap()
            .get_user_mapping(KeyEvent::char(lhs), Mode::Normal)?;
        Some(entry.trust().get())
    }

    #[test]
    fn mappings_and_abbreviations_keep_the_tag_of_their_file() {
        use crate::execution::{ConfigText, TrustTag};
        let mut engine = VimEngine::new();
        let mut host = TaggedFiles(&[("b.vim", "nnoremap b x\ncabbrev bb x\n", 3)]);
        let text = "nnoremap a x\niabbrev aa x\nsource b.vim\n";
        let config = ConfigText::new("a.vim", text).with_trust(TrustTag::new(2));
        let response = engine.source_script(config, &mut host).response;
        assert_eq!(errors(&response), Vec::<String>::new());
        assert_eq!(mapping_tag(&engine, 'a'), Some(2));
        assert_eq!(mapping_tag(&engine, 'b'), Some(3));
        let tag = |trigger: &str| engine.abbreviations().get(trigger).map(|e| e.trust.get());
        assert_eq!(tag("aa"), Some(2));
        assert_eq!(tag("bb"), Some(3));
    }

    #[test]
    fn typed_and_host_mappings_get_the_default_tag() {
        use crate::execution::{ConfigText, HostMapping, TrustTag};
        use crate::keymap::MappingMode;
        let mut session = HostSession::new("x");
        let config = ConfigText::new("a.vim", "nnoremap a x\n").with_trust(TrustTag::new(2));
        let _ = session.source_script(config, &mut TaggedFiles(&[]));
        type_keys(&mut session, ":nnoremap a y<CR>:iabbrev aa y<CR>");
        session.engine_mut().register_host_mappings(
            "ext",
            &[HostMapping {
                modes: smallvec::smallvec![MappingMode::Normal],
                lhs: "h".into(),
                rhs: "x".into(),
                recursive: false,
                silent: false,
                description: None,
            }],
        );
        assert_eq!(mapping_tag(session.engine(), 'a'), Some(0));
        assert_eq!(mapping_tag(session.engine(), 'h'), Some(0));
        let entry = session.engine().abbrev_table.get("aa");
        assert_eq!(entry.map(|e| e.trust), Some(TrustTag::default()));
    }

    // ── reset_config_state ────────────────────────────────────────

    #[test]
    fn reset_config_state_clears_what_a_config_defines() {
        use crate::execution::HostMapping;
        use crate::keymap::{KeyEvent, MappingMode, MappingOwner};
        let mut engine = VimEngine::new();
        engine.register_host_mappings(
            "ext",
            &[HostMapping {
                modes: smallvec::smallvec![MappingMode::Normal],
                lhs: "gd".into(),
                rhs: "<Action>(goto)".into(),
                recursive: false,
                silent: false,
                description: None,
            }],
        );
        let config = "nnoremap a b\nxnoremap c d\nsnoremap e f\nonoremap g h\n\
                      inoremap i j\ncnoremap k l\niabbrev m n\ncabbrev o p\n\
                      abbreviate q r\nsethandler <C-a> n:ide\nlet mapleader = \",\"\n\
                      let maplocalleader = \";\"\nset tw=33\n";
        let response = engine.source_config_str(config);
        assert_eq!(errors(&response), Vec::<String>::new());
        assert!(!engine.handler_map().is_empty());

        engine.reset_config_state();

        for mm in MappingMode::ALL {
            let owners: Vec<_> = engine
                .keymap()
                .list_mappings(mm)
                .into_iter()
                .map(|(_, entry, _)| entry.owner().clone())
                .collect();
            let host = MappingOwner::Host("ext".into());
            let expected = if mm == MappingMode::Normal {
                vec![host]
            } else {
                vec![]
            };
            assert_eq!(owners, expected, "{mm:?}");
        }
        assert!(engine.abbrev_table.is_empty());
        assert!(engine.handler_map().is_empty());
        assert_eq!(engine.leader_keys(), [KeyEvent::char('\\')]);
        assert_eq!(engine.local_leader_keys(), [KeyEvent::char('\\')]);
        let variables = engine.state().variable_store();
        assert!(!variables.exists(crate::primitives::VarScope::Global, "mapleader"));
        assert!(!variables.exists(crate::primitives::VarScope::Global, "maplocalleader"));
        assert_eq!(engine.options().textwidth(), 33);
    }

    #[test]
    fn reset_config_state_drops_pending_keys_and_marks_key_interest() {
        // A key waiting as the start of a mapping would be matched against
        // mappings that are gone, and the host's key interest held them.
        let mut session = sourced("abc", "nnoremap ab x\n");
        type_keys(&mut session, "a");
        assert!(session.engine().has_pending_mapping());
        session.engine_mut().key_interest_dirty = false;
        session.reset_config_state();
        assert!(!session.engine().has_pending_keys());
        assert!(session.engine().key_interest_dirty);
        type_keys(&mut session, "b");
        assert_eq!(session.text(), "abc");
    }

    #[test]
    fn reset_config_state_lets_a_config_run_again_from_nothing() {
        let mut session = sourced("x", "let mapleader = \",\"\niabbrev teh the\n");
        session.reset_config_state();
        let _ = session.source_config("nnoremap <Leader>d dd\n");
        type_keys(&mut session, "A teh <Esc>,d");
        assert_eq!(session.text(), "x teh ");
        type_keys(&mut session, "\\d");
        assert_eq!(session.text(), "");
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
        let response = engine.source_config_str(config);
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
        engine.source_config_str("nnoremap x y\nnmapclear\n");
        assert!(!engine.could_start_mapping(crate::keymap::KeyEvent::char('x')));
    }

    /// The global and the effective 'textwidth' after `config`, in an
    /// engine whose buffer has a local 'textwidth' of 20.
    fn textwidths_after(config: &str) -> (usize, OptionValue) {
        let mut engine = VimEngine::new();
        engine
            .buffer_overrides
            .set(OptionId::TextWidth, OptionValue::Unsigned(20));
        engine.rebuild_resolved_cache();
        let response = engine.source_config_str(config);
        assert!(errors(&response).is_empty(), "{:?}", errors(&response));
        (
            engine.options().textwidth(),
            engine.effective_option(OptionId::TextWidth),
        )
    }

    #[test]
    fn source_config_treats_setlocal_as_set() {
        // There is no buffer to hold a local value.
        let ten = OptionValue::Unsigned(10);
        assert_eq!(textwidths_after("setlocal tw=10\n"), (10, ten.clone()));
        assert_eq!(textwidths_after("set tw=10\n"), (10, ten));
    }

    #[test]
    fn source_config_setglobal_sets_only_the_global_value() {
        // ex_set() passes OPT_GLOBAL, and :let &g: sets the global value.
        let twenty = OptionValue::Unsigned(20);
        assert_eq!(textwidths_after("setglobal tw=10\n"), (10, twenty.clone()));
        assert_eq!(
            textwidths_after("setglobal tw=4 | setglobal tw+=5\n"),
            (9, twenty.clone())
        );
        assert_eq!(textwidths_after("let &g:tw = 10\n"), (10, twenty));
        let mut engine = VimEngine::new();
        engine.source_config_str("setglobal ic\n");
        assert!(engine.options().ignorecase());
    }

    // ── no buffer ─────────────────────────────────────────────────

    #[test]
    fn source_config_refuses_commands_that_need_a_buffer() {
        let mut engine = VimEngine::new();
        let response = engine.source_config_str("set ic | %d | set tw=33\nnormal dd\n");

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
        let response = engine.source_config_str("silent! normal dd\nsilent! set nosuchoption\n");
        assert!(errors(&response).is_empty());
    }

    #[test]
    fn source_config_reports_an_error_and_goes_on() {
        let mut engine = VimEngine::new();
        let response = engine.source_config_str("set nosuchoption\nset tw=33\n");
        assert_eq!(engine.options().textwidth(), 33);
        assert_eq!(errors(&response).len(), 1);
    }

    #[test]
    fn source_config_effects_are_messages_only() {
        // Hosts apply config effects without a buffer.
        let mut engine = VimEngine::new();
        let config = "set tw=80 | set tw?\niabbrev teh the\niabbrev\nnoh\necho hi\n\
                      registers\nmessages\nnormal x\nclearjumps\nnnoremap x y\nsethandler <C-a> n:vim\n";
        let response = engine.source_config_str(config);
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
        let response = engine.source_config_str("set tw=80 | set tw?\necho \"hi\"\n");
        assert_eq!(
            infos(&response),
            ["textwidth=80", "hi", "Config file sourced"]
        );
    }

    #[test]
    fn source_config_echo_evaluates_its_expressions() {
        // ex_echo(): the values joined by a space.
        let mut engine = VimEngine::new();
        let response =
            engine.source_config_str("set tw=33\necho \"tw\" &tw has('eval') 1.0 . 'x'\n");
        assert_eq!(infos(&response), ["tw 33 1 1.0x", "Config file sourced"]);
        assert!(errors(&response).is_empty(), "{:?}", errors(&response));
    }

    #[test]
    fn source_config_echo_error_ends_its_line() {
        // Under :silent! emsg() counts no error, so the line goes on.
        let mut engine = VimEngine::new();
        let response = engine
            .source_config_str("echo \"a\" nosuch | set ic\nsilent! echo nosuch | set tw=33\n");
        assert!(!engine.options().ignorecase());
        assert_eq!(engine.options().textwidth(), 33);
        assert_eq!(infos(&response), ["a", "Config file sourced"]);
        assert_eq!(errors(&response), ["E121: Undefined variable: nosuch"]);
    }

    // ── host commands ─────────────────────────────────────────────

    #[test]
    fn source_config_sends_host_commands_to_the_host() {
        let mut engine = VimEngine::new();
        let response = engine.source_config_str("write\nfiletype plugin on\npanelmap x y\n");
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

    #[test]
    fn source_config_forgets_the_depth_of_a_source_once_its_request_goes() {
        use crate::execution::HostResult;

        let mut engine = VimEngine::new();
        let response = engine.source_config_str("source a.vim\nsource b.vim\nsource c.vim\n");
        let ids: Vec<_> = response.host_requests().iter().map(|r| r.id()).collect();
        assert_eq!(ids.len(), 3);
        assert!(ids
            .iter()
            .all(|id| engine.host.source_depths.get(id) == Some(&2)));
        let _ = engine.complete_host_request(&HostResult::Success {
            id: ids[0],
            message: None,
        });
        let _ = engine.remove_pending_host_request(ids[1]);
        let left: Vec<_> = engine.host.source_depths.keys().copied().collect();
        assert_eq!(left, [ids[2]]);
        engine.reset();
        assert!(engine.host.source_depths.is_empty());
    }

    // ── how Vim reads a sourced file ──────────────────────────────

    #[test]
    fn source_config_skips_only_spaces_and_tabs_before_a_comment() {
        // `skipwhite()` skips no other white space, so after a no-break
        // space `"` starts no comment and the line is a command.
        let mut engine = VimEngine::new();
        let response = engine.source_config_str("\u{a0}\" x\n \t\" y\n");
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
        let response = engine.source_config_str("set tw=33\r\nset ic\r\n");
        assert_eq!(engine.options().textwidth(), 33);
        assert!(engine.options().ignorecase());
        assert!(errors(&response).is_empty(), "{:?}", errors(&response));
    }

    #[test]
    fn source_config_warns_of_a_lone_lf_in_a_dos_file() {
        // `get_one_sourceline()` gives W15 as an error; the line still runs,
        // and the file is read as Unix from there, so the CR of the next
        // line stays in its value.
        let mut engine = VimEngine::new();
        let response = engine.source_config_str("set ic\r\nset tw=33\nset tw=44\r\n");
        assert!(engine.options().ignorecase());
        assert_eq!(engine.options().textwidth(), 33);
        assert_eq!(
            errors(&response),
            [
                "W15: Warning: Wrong line separator, ^M may be missing",
                "E521: Number required after =: tw=44^M"
            ]
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
        let response = engine.source_config_str(config);
        assert!(!engine.options().ignorecase());
        assert_eq!(engine.options().textwidth(), 33);
        assert_eq!(
            errors(&response),
            ["E319: Sorry, the command is not available in this version: function! Foo()"]
        );
        assert!(response.host_requests().is_empty());
    }

    #[test]
    fn source_config_applies_set_options() {
        let mut engine = VimEngine::new();
        let config = "set tabstop=8\nset expandtab\nset noautoindent\n";
        let response = engine.source_config_str(config);

        assert_eq!(engine.options().tabstop(), 8);
        assert!(engine.options().expandtab());
        assert!(!engine.options().autoindent());
        assert!(!response.effects.is_empty());
    }

    #[test]
    fn source_config_skips_comments_and_empty_lines() {
        let mut engine = VimEngine::new();
        let config = "\" This is a comment\n\n  \" Another comment\nset tabstop=2\n";
        engine.source_config_str(config);

        assert_eq!(engine.options().tabstop(), 2);
    }

    #[test]
    fn source_config_skips_unparseable_lines() {
        let mut engine = VimEngine::new();
        let config = "not_a_valid_command\nset tabstop=4\ngibberish!!!!\n";
        engine.source_config_str(config);

        assert_eq!(engine.options().tabstop(), 4);
    }

    #[test]
    fn source_config_applies_mappings() {
        let mut engine = VimEngine::new();
        let config = "nnoremap jk <Esc>\n";
        let response = engine.source_config_str(config);

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
        engine.source_config_str(config);

        let new_leader = engine.leader();
        assert_ne!(default_leader, new_leader);
        assert_eq!(new_leader, crate::keymap::KeyEvent::char(' '));
    }

    #[test]
    fn source_config_leader_before_mapping() {
        let mut engine = VimEngine::new();
        let config = "let mapleader = \",\"\nnnoremap <Leader>w :save<CR>\n";
        engine.source_config_str(config);

        // The mapping should be registered under comma (the leader)
        let comma = crate::keymap::KeyEvent::char(',');
        assert!(engine.could_start_mapping(comma));
    }

    #[test]
    fn source_config_takes_the_leader_from_an_expression() {
        // ex_let(); replace_termcodes() reads g:mapleader.
        let config = "let mapleader = \"\\<Space>\"\nnnoremap <Leader>w :set ic<CR>\n";
        let mut session = sourced("a", config);
        type_keys(&mut session, "<Space>w");
        assert!(session.options().ignorecase());
    }

    #[test]
    fn source_config_takes_a_leader_of_several_keys() {
        let mut session = sourced(
            "a",
            "let mapleader = ',,'\nnnoremap <Leader>w :set ic<CR>\n",
        );
        type_keys(&mut session, ",,w");
        assert!(session.options().ignorecase());
    }

    #[test]
    fn source_config_keeps_the_leader_in_g_mapleader() {
        use crate::keymap::KeyEvent;

        let mut engine = VimEngine::new();
        engine.source_config_str("let mapleader = ','\nlet mapleader = ''\n");
        assert_eq!(engine.leader_keys(), [KeyEvent::char('\\')]);
        // `<Space>` in a value is seven keys; only "\<Space>" is Space.
        engine.source_config_str("let mapleader = '<Space>'\n");
        assert_eq!(engine.leader_keys().len(), 7);
        engine.source_config_str("let mapleader = \"\\<C-W>\\t\"\nlet maplocalleader = ','\n");
        assert_eq!(engine.leader_keys(), [KeyEvent::ctrl('w'), KeyEvent::tab()]);
        assert_eq!(engine.local_leader_keys(), [KeyEvent::char(',')]);
        let response =
            engine.source_config_str("echo g:maplocalleader exists('g:mapleader') g:mapleader\n");
        assert_eq!(infos(&response)[0], ", 1 \x17\t");
    }

    #[test]
    fn source_config_keeps_the_leader_when_its_expression_fails() {
        let mut engine = VimEngine::new();
        let response =
            engine.source_config_str("let mapleader = nosuch | set ic\nlet mapleader = \\\n");
        assert_eq!(engine.leader(), crate::keymap::KeyEvent::char('\\'));
        assert!(!engine.options().ignorecase());
        assert_eq!(
            errors(&response),
            [
                "E121: Undefined variable: nosuch",
                "E15: Invalid expression: \"\\\""
            ]
        );
    }

    #[test]
    fn a_typed_let_mapleader_takes_an_expression() {
        // It gave E521 for anything but one character.
        let mut session = HostSession::new("a");
        type_keys(&mut session, ":let mapleader = \"\\<lt>Space>\"<CR>");
        assert_eq!(
            session.engine().leader_keys(),
            [crate::keymap::KeyEvent::char(' ')]
        );
        type_keys(&mut session, ":let maplocalleader = ',' . ','<CR>");
        assert_eq!(session.engine().local_leader_keys().len(), 2);
    }

    // ── :let and :unlet ───────────────────────────────────────────

    #[test]
    fn source_config_let_assigns_variables() {
        // ex_let(): a plain name is global at the script level.
        let mut engine = VimEngine::new();
        let response = engine.source_config_str(
            "let g:x = 1 | let y = 'a' | let b:z = 2.5\necho g:x y b:z x exists('g:y')\n",
        );
        assert_eq!(infos(&response)[0], "1 a 2.5 1 1");
        assert!(errors(&response).is_empty(), "{:?}", errors(&response));
    }

    #[test]
    fn source_config_let_operators_change_the_value() {
        let mut engine = VimEngine::new();
        let config = "let x = 1 | let x += 2 | let x *= 3 | let x -= 1 | let x /= 2 | let x %= 3\n\
                      let s = 'a' | let s .= 'b' | let s ..= 1\nlet n = 5 | let n .= 1\necho x s n\n";
        let response = engine.source_config_str(config);
        assert_eq!(infos(&response)[0], "1 ab1 51");
        assert!(errors(&response).is_empty(), "{:?}", errors(&response));
    }

    #[test]
    fn source_config_let_errors_end_their_line() {
        let mut engine = VimEngine::new();
        let config = "let nosuch += 1 | set ic\nlet x = 1 | let x .= 1.5 | set ic\n\
                      let x = nosuch | set ic\nlet s:x = 1\nlet [a, b] = [1, 2]\nlet x\nlet $X = 1\n";
        let response = engine.source_config_str(config);
        assert!(!engine.options().ignorecase());
        assert_eq!(
            errors(&response),
            [
                "E121: Undefined variable: nosuch",
                "E734: Wrong variable type for .=",
                "E121: Undefined variable: nosuch",
                "E319: Sorry, the command is not available in this version: let s:x = 1",
                "E319: Sorry, the command is not available in this version: let [a, b] = [1, 2]",
                "E319: Sorry, the command is not available in this version: let x",
                "E319: Sorry, the command is not available in this version: let $X = 1",
            ]
        );
        assert!(response.host_requests().is_empty());
    }

    #[test]
    fn source_config_let_sets_options() {
        // ex_let_option()
        let mut engine = VimEngine::new();
        let config =
            "let &tw = 33 | let &tw += 2\nlet &ic = 1\nlet &ww = 'b,s' | let &ww .= ',h'\n\
                      let &l:sw = 3 | let &g:ts = 6\n";
        let response = engine.source_config_str(config);
        assert!(errors(&response).is_empty(), "{:?}", errors(&response));
        assert_eq!(engine.options().textwidth(), 35);
        assert!(engine.options().ignorecase());
        assert_eq!(engine.options().whichwrap(), "b,s,h");
        assert_eq!(engine.options().shiftwidth(), 3);
        assert_eq!(engine.options().tabstop(), 6);
        engine.source_config_str("let &ic = 0\n");
        assert!(!engine.options().ignorecase());
    }

    #[test]
    fn source_config_let_option_keeps_backslashes_and_blanks() {
        // ex_let_option() sets the value itself, without :set's escapes.
        let mut engine = VimEngine::new();
        let response = engine.source_config_str("let &isk = '@,\\,4 8' | let &isk .= '\\'\n");
        assert!(errors(&response).is_empty(), "{:?}", errors(&response));
        assert_eq!(engine.options().iskeyword(), "@,\\,4 8\\");
    }

    #[test]
    fn source_config_let_option_errors() {
        let mut engine = VimEngine::new();
        let config =
            "let &tw .= 1\nlet &ww += 1\nlet &nosuch = 1\nlet &tw = 1.5\nlet &ww = v:true\n";
        let response = engine.source_config_str(config);
        assert_eq!(
            errors(&response),
            [
                "E734: Wrong variable type for .=",
                "E734: Wrong variable type for +=",
                "E355: Unknown option: nosuch",
                "E805: Using a Float as a Number",
                "E928: String required",
            ]
        );
    }

    #[test]
    fn source_config_let_local_option_acts_like_setlocal() {
        // Deviation: with no buffer or window, `&l:` sets the global value
        // too, as :setlocal does in a config.
        for config in [
            "setlocal tw=20 et\nset tw? et?\n",
            "let &l:tw = 20 | let &l:et = 1\nset tw? et?\n",
        ] {
            let mut engine = VimEngine::new();
            let response = engine.source_config_str(config);
            assert!(errors(&response).is_empty(), "{:?}", errors(&response));
            assert_eq!(engine.options().textwidth(), 20, "{config:?}");
            assert!(engine.options().expandtab(), "{config:?}");
            assert_eq!(infos(&response)[0], "textwidth=20\nexpandtab", "{config:?}");
        }
    }

    #[test]
    fn source_config_let_option_error_names_no_argument() {
        // ex_let_option(): set_option_value_give_err() shows the message
        // alone, where :set adds the argument.
        let mut engine = VimEngine::new();
        let config = "let &tw = -1\nlet &ts = 0\nlet &fo = 'tZ'\nlet &l:sw = -2\nlet &tw = 5\n";
        let response = engine.source_config_str(config);
        assert_eq!(
            errors(&response),
            [
                "E487: Argument must be positive",
                "E487: Argument must be positive",
                "E539: Illegal character <Z>",
                "E487: Argument must be positive",
            ]
        );
        assert_eq!(engine.options().textwidth(), 5);
        assert_eq!(engine.options().tabstop(), 4);
    }

    #[test]
    fn source_config_unlet_removes_variables() {
        // ex_unlet(), do_unlet()
        let mut engine = VimEngine::new();
        let config = "let g:x = 1 | let y = 2 | unlet g:x y\necho exists('x') exists('y')\n\
                      unlet nosuch | set ic\nunlet! nosuch\nunlet\nunlet $X\nlet x = 1 | unlet x+1\n";
        let response = engine.source_config_str(config);
        assert_eq!(infos(&response)[0], "0 0");
        assert!(!engine.options().ignorecase());
        assert_eq!(
            errors(&response),
            [
                "E108: No such variable: \"nosuch\"",
                "E471: Argument required",
                "E319: Sorry, the command is not available in this version: unlet $X",
                "E488: Trailing characters: +1",
            ]
        );
    }

    #[test]
    fn source_config_let_and_unlet_change_the_leader() {
        use crate::keymap::KeyEvent;

        let mut engine = VimEngine::new();
        engine.source_config_str("let g:mapleader = ',' | let mapleader .= ','\n");
        assert_eq!(
            engine.leader_keys(),
            [KeyEvent::char(','), KeyEvent::char(',')]
        );
        engine.source_config_str("unlet mapleader\nlet maplocalleader = 1\n");
        assert_eq!(engine.leader_keys(), [KeyEvent::char('\\')]);
        assert_eq!(engine.local_leader_keys(), [KeyEvent::char('1')]);
    }

    #[test]
    fn a_typed_vim_command_vim_core_lacks_still_goes_to_the_host() {
        let mut session = HostSession::new("a");
        type_keys(&mut session, ":syntax on");
        let response = session.process_key_host(crate::keymap::KeyEvent::enter());
        let kinds: Vec<_> = response.host_requests.iter().map(|r| r.kind()).collect();
        assert_eq!(kinds, [HostRequestKind::CustomExCommand]);
    }

    /// Vim applies them (ex_docmd.c `parse_command_modifiers()`); vim-core
    /// has no meaning for them and runs the command.
    #[test]
    fn source_config_runs_a_command_after_a_modifier_only_a_config_takes() {
        let mut engine = VimEngine::new();
        let config = "verbose set tw=33\n3verbose set ic | noa set ts=3\nkeepmarks set sw=2\n\
                      lefta set et\nrightbelow set ai\nunsilent echo 'x'\nsilent! verbose set nosuch\n";
        let response = engine.source_config_str(config);
        assert_eq!(errors(&response), Vec::<String>::new());
        assert_eq!(infos(&response), ["x", "Config file sourced"]);
        let options = engine.options();
        assert_eq!(
            (options.textwidth(), options.tabstop(), options.shiftwidth()),
            (33, 3, 2)
        );
        assert!(options.ignorecase() && options.expandtab() && options.autoindent());
    }

    #[test]
    fn source_config_still_refuses_a_lacking_command_after_such_a_modifier() {
        let mut engine = VimEngine::new();
        let response = engine.source_config_str("verbose syntax on | set ic\nsandbox set tw=3\n");
        let syntax = "E319: Sorry, the command is not available in this version: verbose syntax on";
        let sandbox = "E319: Sorry, the command is not available in this version: sandbox set tw=3";
        assert_eq!(errors(&response), [syntax, sandbox]);
        assert!(!engine.options().ignorecase());
    }

    #[test]
    fn a_typed_command_after_a_modifier_only_a_config_takes_still_goes_to_the_host() {
        let mut session = HostSession::new("a");
        type_keys(&mut session, ":verbose set tw=33");
        let response = session.process_key_host(crate::keymap::KeyEvent::enter());
        let kinds: Vec<_> = response.host_requests.iter().map(|r| r.kind()).collect();
        assert_eq!(kinds, [HostRequestKind::CustomExCommand]);
        assert_eq!(session.options().textwidth(), 0);
    }

    #[test]
    fn a_typed_let_still_goes_to_the_host() {
        let mut session = HostSession::new("a");
        type_keys(&mut session, ":let g:x = 1<CR>");
        let variables = session.engine().state().variable_store();
        assert!(!variables.exists(crate::primitives::VarScope::Global, "x"));
    }

    #[test]
    fn source_config_shows_sourced_message() {
        let mut engine = VimEngine::new();
        let response = engine.source_config_str("");

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
        engine.source_config_str(config);

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
        engine.source_config_str(config);

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
        engine.source_config_str(config);

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
        engine.source_config_str(config);

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
        engine.source_config_str(config);

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
        engine.source_config_str(config);

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
        engine.source_config_str(config);

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
        let response = engine.source_config_str("sethandler <C-a> n:banana\n");

        assert!(engine.handler_map().is_empty());
        assert_eq!(errors(&response).len(), 1);
    }

    #[test]
    fn source_config_sethandler_no_key_is_noop() {
        let mut engine = VimEngine::new();
        // No key specified — global default, currently a no-op
        let config = "sethandler n:vim\n";
        engine.source_config_str(config);

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
        engine.source_config_str(config);

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
        // On by default, as in Vim's option table.
        assert!(engine.options().langremap());

        let config = "set nolangremap\n";
        engine.source_config_str(config);
        assert!(
            !engine.options().langremap(),
            "langremap should be false after :set nolangremap"
        );

        let config2 = "set langremap\n";
        engine.source_config_str(config2);
        assert!(
            engine.options().langremap(),
            "langremap should be true after :set langremap"
        );
    }

    #[test]
    fn source_config_langmap_sets_key_interest_dirty() {
        let mut engine = VimEngine::new();
        // Clear the dirty flag (it starts true)
        engine.key_interest_dirty = false;

        let config = "set langmap=ab\n";
        engine.source_config_str(config);

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
        engine.source_config_str(config);
        assert!(!engine.langmap_table.is_empty());

        // Now clear it
        let config2 = "set langmap=\n";
        engine.source_config_str(config2);
        assert!(
            engine.langmap_table.is_empty(),
            "langmap table should be empty after :set langmap="
        );
    }
}
