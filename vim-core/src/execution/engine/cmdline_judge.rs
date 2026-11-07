//! Judging, when it runs, a command line that keys from a tagged mapping
//! made, through the host's [`CommandLineJudge`].

use crate::VimString;
use ahash::AHashMap;

use super::VimEngine;
use crate::execution::host::HostRequestId;
use crate::execution::HostRequest;
use crate::execution::{CommandLineJudge, RuntimeCommand, Verdict};
use crate::grammar::ex_parser::{nest, parse_ex_piece_at};
use crate::grammar::split_ex_pipeline;
use crate::grammar::types::{ExCommand, ModifierFlags};
use crate::primitives::Tags;

/// What the judge decided about one `|` command of a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PieceVerdict {
    Run,
    /// Claimed: it does not run, and nothing is shown.
    Skip,
    /// It and the commands after it do not run; the text is its error.
    Refuse(Box<str>),
}

/// The tags the engine keeps while keys from mappings run.
#[derive(Debug, Clone, Default)]
pub(super) struct RunTags {
    /// The key being processed.
    pub key: Tags,
    /// The keys of the Normal-mode command being typed, such as a count
    /// or an operator before `:`.
    pub command: Tags,
    /// A `<Cmd>` command being collected.
    pub cmd: Tags,
    /// The keys an `<expr>` mapping's request will give.
    pub expr: AHashMap<HostRequestId, Tags>,
    /// The `:normal` requests of tagged lines that have not completed.
    pub norms: Vec<(HostRequestId, Tags)>,
    /// A tagged line ran a `:normal` whose request is not made yet.
    pub norm_line: Tags,
    /// The `:windo`, `:bufdo` and `:tabdo` requests of tagged lines that
    /// have not completed.
    pub each: Vec<(HostRequestId, Tags)>,
    /// The last command line and its tags.
    pub last_line: Option<(VimString, Tags)>,
    /// A command window opened, until a line of it runs.
    pub window: Option<Tags>,
}

impl RunTags {
    /// Forget what keys in flight carried, keeping the judge and what the
    /// last line and history keep.
    pub(super) fn reset_keys(&mut self) {
        self.key = Tags::new();
        self.command = Tags::new();
        self.cmd = Tags::new();
        self.expr.clear();
        self.norms.clear();
        self.norm_line = Tags::new();
        self.each.clear();
        self.window = None;
    }

    /// The tags of the keys `feed_keys` gives while a tagged `:normal` runs.
    pub(super) fn fed(&self) -> Tags {
        let mut tags = Tags::new();
        for (_, norm) in &self.norms {
            tags.extend(norm);
        }
        tags
    }

    /// Keep `tags` for the `:windo` and its kind among `requests`.
    pub(super) fn note_each(&mut self, requests: &[HostRequest], tags: &Tags) {
        if tags.is_empty() {
            return;
        }
        for request in requests {
            if matches!(
                request,
                HostRequest::ForEachWindow { .. }
                    | HostRequest::ForEachBuffer { .. }
                    | HostRequest::ForEachTab { .. }
            ) {
                self.each.push((request.id(), tags.clone()));
            }
        }
    }

    /// The tags of a line the host runs while a tagged `:windo` or its kind
    /// waits.
    pub(super) fn each(&self) -> Tags {
        let mut tags = Tags::new();
        for (_, each) in &self.each {
            tags.extend(each);
        }
        tags
    }

    /// A request completed or was cancelled.
    pub(super) fn request_done(&mut self, id: HostRequestId) {
        self.norms.retain(|(norm, _)| *norm != id);
        self.each.retain(|(each, _)| *each != id);
    }

    /// The tags of the last command line, if it is `text`.
    pub(super) fn last_line(&self, text: &str) -> Tags {
        match &self.last_line {
            Some((line, tags)) if line.as_str() == text => tags.clone(),
            _ => Tags::new(),
        }
    }
}

impl VimEngine {
    /// Install the judge of command lines that keys from a mapping with a
    /// non-default [`TrustTag`](crate::execution::TrustTag) made, or with
    /// `None` remove it. With none, every line runs.
    pub fn set_command_line_judge(&mut self, judge: Option<Box<dyn CommandLineJudge>>) {
        self.cold.judge = judge;
    }

    /// Keep the tags of the key about to be dispatched, which the
    /// typeahead gave it, with those of the command it continues.
    pub(super) fn note_key_tags(&mut self) {
        let tags = self.typeahead.buffer.resolved_tags().clone();
        let idle = matches!(
            self.parser.state(),
            crate::grammar::InputState::Ready {
                count: None,
                register: None
            }
        );
        if idle {
            self.run_tags.command = Tags::new();
        }
        self.run_tags.command.extend(&tags);
        self.run_tags.key = tags;
    }

    /// The tags of the last command line, while the `:` register holds it.
    pub(super) fn last_line_tags(&self) -> Tags {
        self.state
            .registers()
            .get(crate::primitives::RegisterName::LAST_COMMAND)
            .map_or_else(Tags::new, |content| self.run_tags.last_line(content.text()))
    }

    /// The tags of `input` run from the command window: those of the
    /// keys that opened it and of the history entry it is, or of every
    /// entry the window showed when it is none of them.
    pub(super) fn command_window_line_tags(&mut self, input: &str) -> Tags {
        let Some(mut tags) = self.run_tags.window.take() else {
            return Tags::new();
        };
        let line = self.state.command_line();
        let history = line.ex_history();
        match history.iter().rposition(|entry| entry.as_str() == input) {
            Some(index) => tags.extend(&line.ex_history_tags(index)),
            None => {
                for index in 0..history.len() {
                    tags.extend(&line.ex_history_tags(index));
                }
            }
        }
        tags
    }

    /// What the judge decides about each `|` command of `line`, run
    /// `depth` levels deep with `tags`; `None` when it runs as it is.
    pub(super) fn judge_line(
        &mut self,
        line: &str,
        depth: u8,
        tags: &Tags,
    ) -> Option<Vec<PieceVerdict>> {
        if tags.is_empty() {
            return None;
        }
        let mut judge = self.cold.judge.take()?;
        let mut verdicts = Vec::new();
        for piece in split_ex_pipeline(line) {
            let verdict = judge_piece(judge.as_mut(), &piece, None, depth, tags);
            let refused = matches!(verdict, PieceVerdict::Refuse(_));
            verdicts.push(verdict);
            if refused {
                break;
            }
        }
        self.cold.judge = Some(judge);
        Some(verdicts)
    }
}

/// Judge one command run `depth` levels deep, then the commands it runs.
/// `parsed` is the command when its owner parsed it already.
fn judge_piece(
    judge: &mut dyn CommandLineJudge,
    text: &str,
    parsed: Option<&ExCommand>,
    depth: u8,
    tags: &Tags,
) -> PieceVerdict {
    let text = text.trim_matches([' ', '\t']);
    let (modifiers, command) = match parsed {
        Some(command) => (ModifierFlags::default(), command.clone()),
        None => match parse_ex_piece_at(text, depth) {
            (modifiers, Ok(command)) => (modifiers, command),
            // The engine gives the error itself.
            (_, Err(_)) => return PieceVerdict::Run,
        },
    };
    let verdict = judge.judge(&RuntimeCommand {
        text,
        tags: tags.as_slice(),
        modifiers,
        command: &command,
        reach: command.reach_at(depth),
    });
    #[cfg(test)]
    seen::judged(text, &verdict);
    let own = match verdict {
        Verdict::Allow => PieceVerdict::Run,
        Verdict::Claim => PieceVerdict::Skip,
        Verdict::Refuse(text) => return PieceVerdict::Refuse(text),
    };
    match judge_nested(judge, text, &command, depth, tags) {
        PieceVerdict::Run => own,
        nested => nested,
    }
}

/// Judge the commands `command`, run `depth` levels deep, runs: those of a
/// `:g` list and the command of `:windo` and its kind.
fn judge_nested(
    judge: &mut dyn CommandLineJudge,
    text: &str,
    command: &ExCommand,
    depth: u8,
    tags: &Tags,
) -> PieceVerdict {
    let mut verdict = PieceVerdict::Run;
    match command {
        ExCommand::Global { commands, .. } => {
            // Deeper than the limit, `:g` gives an error before it runs.
            let Ok(depth) = nest(depth) else {
                return verdict;
            };
            for piece in split_ex_pipeline(commands) {
                match judge_piece(judge, &piece, None, depth, tags) {
                    PieceVerdict::Run => {}
                    PieceVerdict::Skip => verdict = PieceVerdict::Skip,
                    refused @ PieceVerdict::Refuse(_) => return refused,
                }
            }
        }
        ExCommand::WinDo { source, .. }
        | ExCommand::BufDo { source, .. }
        | ExCommand::TabDo { source, .. } => {
            verdict = judge_piece(judge, source, None, depth.saturating_add(1), tags);
        }
        ExCommand::StructuralExtract { command, .. }
        | ExCommand::StructuralComplement { command, .. } => {
            verdict = judge_piece(judge, text, Some(command), depth.saturating_add(1), tags);
        }
        _ => {}
    }
    verdict
}

#[cfg(test)]
#[path = "cmdline_judge_tests.rs"]
mod tests;

/// What the judge saw and what ran on this thread, in order, for tests
/// that hold every command of a tagged line to a judged one.
#[cfg(test)]
pub(crate) mod seen {
    use std::cell::RefCell;

    use crate::execution::Verdict;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum Event {
        Judged(String, Verdict),
        /// A command ran, from a tagged line or not.
        Ran(String, bool),
    }

    thread_local! {
        static EVENTS: RefCell<Vec<Event>> = const { RefCell::new(Vec::new()) };
    }

    pub(crate) fn judged(text: &str, verdict: &Verdict) {
        EVENTS.with(|events| {
            events
                .borrow_mut()
                .push(Event::Judged(text.to_owned(), verdict.clone()));
        });
    }

    pub(crate) fn ran(piece: &str, tagged: bool) {
        let piece = piece.trim_matches([' ', '\t']).to_owned();
        EVENTS.with(|events| events.borrow_mut().push(Event::Ran(piece, tagged)));
    }

    /// What happened since the last call.
    pub(crate) fn take() -> Vec<Event> {
        EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut()))
    }
}
