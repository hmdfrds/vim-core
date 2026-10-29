//! Config file processing for [`VimEngine`].
//!
//! Runs a config file's lines as the command line runs Ex commands, without
//! a buffer.

use super::conditionals::{Blocks, Kind};
use super::Response;
use crate::commands::ex::effects as ex_effects;
use crate::document::Document;
use crate::effects::{Effect, Effects};
use crate::errors::VimError;
use crate::execution::config::{ConfigHost, ConfigOutcome, ConfigResponse, ConfigText, NoHost};
use crate::execution::executor_ex::{self, SetKind, SetScope};
use crate::execution::script_expr::{self, Echoed, OptionLayer, Target, Value};
use crate::execution::{ExecutionContext, InputContext};
use crate::grammar::ex_parser::{parse_ex_piece, vim_command_parts};
use crate::grammar::script::{script_command, Body, Control, Line, Script, ScriptCommand};
use crate::grammar::split_ex_pipeline;
use crate::grammar::types::{BlockKind, ExCommand, ModifierFlags, SetAssignment};
use crate::primitives::{Offset, OptionValue, Position, VarScope};
use compact_str::CompactString;

/// A config being sourced, with what Vim's `do_cmdline()` keeps while it
/// runs one.
struct Frame<'a> {
    script: Script<'a>,
    blocks: Blocks,
    /// Vim's `did_emsg`: an error was shown, so the commands after it do
    /// not run, up to the end of its line or of the outermost block.
    error: bool,
    /// `:finish` ran: no line is read after the current one.
    finished: bool,
}

impl Frame<'_> {
    /// Whether a command does not run here (`do_one_cmd()`: `ea.skip`).
    fn skipping(&self) -> bool {
        self.error || !self.blocks.active()
    }
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
        let response = self.run_script(text, host);
        let errors = response
            .effects()
            .iter()
            .filter(|e| matches!(e, Effect::ShowError { .. }))
            .count();
        ConfigResponse {
            response,
            outcome: ConfigOutcome {
                errors: u32::try_from(errors).unwrap_or(u32::MAX),
            },
        }
    }

    /// Run a config file's text as [`source_script`](Self::source_script)
    /// runs a config without a name, with a host that answers nothing.
    ///
    /// The engine calls it when a host answers `:source` with the file's
    /// text. The response ends with "Config file sourced".
    pub fn source_config_text(&mut self, text: &str) -> Response {
        let mut response = self
            .source_script(ConfigText::new("", text), &mut NoHost)
            .response;
        response.extend_effects(ex_effects::show_message(CompactString::from(
            "Config file sourced",
        )));
        response
    }

    /// Run the config `text` for [`source_script`](Self::source_script).
    fn run_script(&mut self, text: ConfigText<'_>, host: &dyn ConfigHost) -> Response {
        if self.options_dirty {
            self.rebuild_resolved_cache();
            self.rebuild_langmap_if_needed();
        }
        let mut response = Response::with_effects(Effects::new());
        let mut frame = Frame {
            script: Script::new(text.text),
            blocks: Blocks::default(),
            error: false,
            finished: false,
        };
        loop {
            // do_cmdline(): "stop skipping cmds for an error msg after all
            // endif/while/for".
            if frame.blocks.is_empty() {
                frame.error = false;
            }
            // getsourceline() reads no line once the file is finished.
            if frame.finished {
                break;
            }
            let Some(line) = frame.script.next_line() else {
                break;
            };
            show_warnings(&mut frame.script, &mut response);
            let mut next = Some(line);
            while let Some(line) = next {
                next = self.source_config_line(&mut frame, host, &line, &mut response);
            }
        }
        if let Some(error) = frame.blocks.missing_end().filter(|_| !frame.finished) {
            response.extend_effects(ex_effects::show_error(error));
        }
        // Not register_pending_host_requests: its effect optimizer keeps only
        // the last of several messages, and a config can print many.
        self.track_host_requests(&mut response);
        response
    }

    /// Run one config line: each of its `|` commands in turn, reading the
    /// lines a command takes after the line from the frame's script.
    /// Returns the rest of the line that ends a function, which runs next.
    fn source_config_line(
        &mut self,
        frame: &mut Frame<'_>,
        host: &dyn ConfigHost,
        line: &Line,
        response: &mut Response,
    ) -> Option<Line> {
        let mut rest = None;
        for piece in split_ex_pipeline(&line.text) {
            let skipping = frame.skipping();
            let body = frame.script.read_body(&piece, line, skipping);
            show_warnings(&mut frame.script, response);
            let from = response.effects().len();
            if let Some(Body {
                errors,
                fails,
                tail,
            }) = body
            {
                rest = tail;
                let (modifiers, _) = parse_ex_piece(&piece);
                let errors = errors.into_iter().map(|(_, error)| error);
                show_config_errors(modifiers, errors, response);
                if fails {
                    frame.error |= shows_error(response, from);
                    continue;
                }
            }
            if let Some(command) = script_command(&piece) {
                self.source_config_control(frame, host, command, response);
                continue;
            }
            if !skipping && self.source_config_piece(host, &piece, response) {
                frame.blocks.finish();
                frame.finished = true;
            }
            frame.error |= shows_error(response, from);
        }
        rest
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
        let modifiers = command.modifiers;
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
                    let mut form = CompactString::from(kind.name());
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

    /// Run one command of a config line as the command line runs it.
    /// Returns whether no line is read after this one, as after
    /// `:vim9script`.
    fn source_config_piece(
        &mut self,
        host: &dyn ConfigHost,
        piece: &str,
        response: &mut Response,
    ) -> bool {
        match vim_command_parts(piece) {
            Some((modifiers, "echo", argument)) => {
                self.source_config_echo(host, modifiers, argument, response);
            }
            Some((modifiers, "let", argument)) => {
                self.source_config_let(host, modifiers, argument, response);
            }
            Some((modifiers, "unlet", argument)) => {
                self.source_config_unlet(modifiers, argument, response);
            }
            _ => match parse_ex_piece(piece) {
                (modifiers, Ok(parsed)) => {
                    if let Some(name) = lacking_vim_command(&parsed) {
                        return source_config_lacking(modifiers, name, piece, response);
                    }
                    self.run_config_command(modifiers, &parsed, piece, response);
                }
                (modifiers, Err(err)) => show_config_errors(modifiers, [err], response),
            },
        }
        false
    }

    /// Run a parsed config command, written as `command`.
    fn run_config_command(
        &mut self,
        modifiers: ModifierFlags,
        parsed: &ExCommand,
        command: &str,
        response: &mut Response,
    ) {
        let result = if parsed.needs_buffer() {
            Err(VimError::NotAllowedHere(CompactString::from(
                command.trim(),
            )))
        } else {
            let input = InputContext::new(&NoBuffer, 0).validate_clamped();
            let ctx = ExecutionContext::new(input, &self.state, &self.options);
            executor_ex::execute_ex_command(parsed, &ctx, &mut self.host.sequencer)
        };
        let mut output = match result {
            Ok(output) => output,
            Err(err) => return show_config_errors(modifiers, [err], response),
        };
        crate::execution::modifier_filter::apply_modifiers(&mut output.effects, modifiers);
        crate::execution::effect_processor::sync_effects(
            &mut self.state,
            &mut self.parser,
            output.effects.as_mut_slice(),
        );
        // No buffer or window to hold a local value.
        for set in &mut output.set_commands {
            set.scope = SetScope::Effective;
        }
        let mut applied = self.apply_ex_output(&mut output, "");
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
    fn source_config_let(
        &mut self,
        host: &dyn ConfigHost,
        modifiers: ModifierFlags,
        argument: &str,
        response: &mut Response,
    ) {
        let argument = argument.trim_start_matches([' ', '\t']);
        let Some(assignment) = script_expr::parse_let(argument) else {
            let form = CompactString::from(format!("let {argument}"));
            return show_config_errors(modifiers, [VimError::NotAvailable(form)], response);
        };
        let evaluated = script_expr::evaluate(assignment.expr, &self.script_env(host));
        let mut errors = evaluated.errors;
        let Some(value) = evaluated.value else {
            return show_config_errors(modifiers, errors, response);
        };
        let assigned = match assignment.target {
            Target::Variable(variable) => self.let_variable(variable, assignment.op, value),
            Target::Option(layer, name) => {
                match self.let_option(layer, name, assignment.op, &value) {
                    Ok(Some(command)) => {
                        show_config_errors(modifiers, errors, response);
                        let text = format!("let {argument}");
                        return self.run_config_command(modifiers, &command, &text, response);
                    }
                    Ok(None) => Ok(()),
                    Err(err) => Err(err),
                }
            }
        };
        errors.extend(assigned.err());
        show_config_errors(modifiers, errors, response);
    }

    /// Assign to a variable, and take the leader from `g:mapleader` or
    /// `g:maplocalleader`.
    fn let_variable(
        &mut self,
        variable: script_expr::Variable<'_>,
        op: Option<char>,
        value: Value,
    ) -> Result<(), VimError> {
        let value = match op {
            None => value,
            Some(op) => {
                let current = self
                    .state
                    .variable_store()
                    .get(variable.scope, variable.name)
                    .ok_or_else(|| VimError::UndefinedVariable(variable.written.into()))?;
                // A List or a Dictionary.
                let current =
                    Value::from_variable(current).ok_or(VimError::WrongVariableType(op))?;
                script_expr::assign_op(op, &current, &value)?
            }
        };
        self.state
            .variable_store_mut()
            .set(variable.scope, variable.name, value.into_variable());
        self.sync_leader_variable(variable);
        Ok(())
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
        let name = CompactString::from(name);
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
                    CompactString::default()
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
                    _ => SetAssignment::Assign(name, CompactString::from(n.to_string())),
                }
            }
        };
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
                let form = CompactString::from(format!("unlet{argument}"));
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
            self.sync_leader_variable(variable);
            rest = after.trim_start_matches([' ', '\t']);
        }
    }

    /// Take the leader again when `variable` is `g:mapleader` or
    /// `g:maplocalleader`.
    fn sync_leader_variable(&mut self, variable: script_expr::Variable<'_>) {
        if variable.scope == VarScope::Global {
            match variable.name {
                "mapleader" => self.sync_leader(false),
                "maplocalleader" => self.sync_leader(true),
                _ => {}
            }
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
    let error = VimError::NotAvailable(CompactString::from(piece.trim()));
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
        let response = engine.source_config_text("set tw=80 | set tw?\necho \"hi\"\n");
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
            engine.source_config_text("set tw=33\necho \"tw\" &tw has('eval') 1.0 . 'x'\n");
        assert_eq!(infos(&response), ["tw 33 1 1.0x", "Config file sourced"]);
        assert!(errors(&response).is_empty(), "{:?}", errors(&response));
    }

    #[test]
    fn source_config_echo_error_ends_its_line() {
        // Under :silent! emsg() counts no error, so the line goes on.
        let mut engine = VimEngine::new();
        let response = engine
            .source_config_text("echo \"a\" nosuch | set ic\nsilent! echo nosuch | set tw=33\n");
        assert!(!engine.options().ignorecase());
        assert_eq!(engine.options().textwidth(), 33);
        assert_eq!(infos(&response), ["a", "Config file sourced"]);
        assert_eq!(errors(&response), ["E121: Undefined variable: nosuch"]);
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
        // `get_one_sourceline()` gives W15 as an error; the line still runs,
        // and the file is read as Unix from there, so the CR of the next
        // line stays in its value.
        let mut engine = VimEngine::new();
        let response = engine.source_config_text("set ic\r\nset tw=33\nset tw=44\r\n");
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
        let response = engine.source_config_text(config);
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
        engine.source_config_text("let mapleader = ','\nlet mapleader = ''\n");
        assert_eq!(engine.leader_keys(), [KeyEvent::char('\\')]);
        // `<Space>` in a value is seven keys; only "\<Space>" is Space.
        engine.source_config_text("let mapleader = '<Space>'\n");
        assert_eq!(engine.leader_keys().len(), 7);
        engine.source_config_text("let mapleader = \"\\<C-W>\\t\"\nlet maplocalleader = ','\n");
        assert_eq!(engine.leader_keys(), [KeyEvent::ctrl('w'), KeyEvent::tab()]);
        assert_eq!(engine.local_leader_keys(), [KeyEvent::char(',')]);
        let response =
            engine.source_config_text("echo g:maplocalleader exists('g:mapleader') g:mapleader\n");
        assert_eq!(infos(&response)[0], ", 1 \x17\t");
    }

    #[test]
    fn source_config_keeps_the_leader_when_its_expression_fails() {
        let mut engine = VimEngine::new();
        let response =
            engine.source_config_text("let mapleader = nosuch | set ic\nlet mapleader = \\\n");
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
        let response = engine.source_config_text(
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
        let response = engine.source_config_text(config);
        assert_eq!(infos(&response)[0], "1 ab1 51");
        assert!(errors(&response).is_empty(), "{:?}", errors(&response));
    }

    #[test]
    fn source_config_let_errors_end_their_line() {
        let mut engine = VimEngine::new();
        let config = "let nosuch += 1 | set ic\nlet x = 1 | let x .= 1.5 | set ic\n\
                      let x = nosuch | set ic\nlet s:x = 1\nlet [a, b] = [1, 2]\nlet x\nlet $X = 1\n";
        let response = engine.source_config_text(config);
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
        let response = engine.source_config_text(config);
        assert!(errors(&response).is_empty(), "{:?}", errors(&response));
        assert_eq!(engine.options().textwidth(), 35);
        assert!(engine.options().ignorecase());
        assert_eq!(engine.options().whichwrap(), "b,s,h");
        assert_eq!(engine.options().shiftwidth(), 3);
        assert_eq!(engine.options().tabstop(), 6);
        engine.source_config_text("let &ic = 0\n");
        assert!(!engine.options().ignorecase());
    }

    #[test]
    fn source_config_let_option_keeps_backslashes_and_blanks() {
        // ex_let_option() sets the value itself, without :set's escapes.
        let mut engine = VimEngine::new();
        let response = engine.source_config_text("let &isk = '@,\\,4 8' | let &isk .= '\\'\n");
        assert!(errors(&response).is_empty(), "{:?}", errors(&response));
        assert_eq!(engine.options().iskeyword(), "@,\\,4 8\\");
    }

    #[test]
    fn source_config_let_option_errors() {
        let mut engine = VimEngine::new();
        let config =
            "let &tw .= 1\nlet &ww += 1\nlet &nosuch = 1\nlet &tw = 1.5\nlet &ww = v:true\n";
        let response = engine.source_config_text(config);
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
    fn source_config_unlet_removes_variables() {
        // ex_unlet(), do_unlet()
        let mut engine = VimEngine::new();
        let config = "let g:x = 1 | let y = 2 | unlet g:x y\necho exists('x') exists('y')\n\
                      unlet nosuch | set ic\nunlet! nosuch\nunlet\nunlet $X\nlet x = 1 | unlet x+1\n";
        let response = engine.source_config_text(config);
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
        engine.source_config_text("let g:mapleader = ',' | let mapleader .= ','\n");
        assert_eq!(
            engine.leader_keys(),
            [KeyEvent::char(','), KeyEvent::char(',')]
        );
        engine.source_config_text("unlet mapleader\nlet maplocalleader = 1\n");
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
