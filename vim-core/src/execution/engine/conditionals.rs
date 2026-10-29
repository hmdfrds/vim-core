//! The blocks a sourced file has open, as Vim's `cstack_T` holds them and
//! the functions of `ex_eval.c` change them.
//!
//! vim-core runs no loops and catches no exceptions, so a `:while`, `:for`
//! or `:try` block never gets active: the commands inside it never run,
//! and its end still closes it.

use crate::errors::VimError;

/// Most blocks open at once (`CSTACK_LEN`).
const MAX_BLOCKS: usize = 50;

/// What a block is (`CSF_WHILE`, `CSF_FOR`, `CSF_TRY`, or none for `:if`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    If,
    While,
    For,
    Try,
}

impl Kind {
    const fn is_loop(self) -> bool {
        matches!(self, Self::While | Self::For)
    }
}

/// One open block.
#[derive(Debug, Clone, Copy)]
struct Block {
    kind: Kind,
    /// `CSF_ACTIVE`: the commands inside run.
    active: bool,
    /// `CSF_TRUE`: a branch of the `:if` was taken, or it never gets
    /// active.
    taken: bool,
    /// `CSF_ELSE`: the `:if` passed its `:else`.
    had_else: bool,
}

impl Block {
    /// A block whose commands never run.
    const fn never_active(kind: Kind) -> Self {
        Self {
            kind,
            active: false,
            taken: true,
            had_else: false,
        }
    }
}

/// The open blocks of a sourced file, innermost last.
#[derive(Debug, Default)]
pub(super) struct Blocks {
    open: Vec<Block>,
}

impl Blocks {
    pub(super) const fn is_empty(&self) -> bool {
        self.open.is_empty()
    }

    /// Whether the commands here run: no block is open, or the innermost
    /// is active.
    pub(super) fn active(&self) -> bool {
        self.open.last().is_none_or(|block| block.active)
    }

    /// Whether the block around the innermost is active.
    fn outer_active(&self) -> bool {
        self.open
            .iter()
            .rev()
            .nth(1)
            .is_none_or(|block| block.active)
    }

    fn has_loop(&self) -> bool {
        self.open.iter().any(|block| block.kind.is_loop())
    }

    /// The innermost `:try`.
    fn innermost_try(&self) -> Option<usize> {
        self.open.iter().rposition(|block| block.kind == Kind::Try)
    }

    /// `ex_if()`. `error` is Vim's `did_emsg`; `condition` evaluates the
    /// condition, `None` when it fails, and is not called when the block
    /// is skipped.
    pub(super) fn open_if(
        &mut self,
        error: bool,
        condition: impl FnOnce() -> Option<bool>,
    ) -> Result<(), VimError> {
        if self.open.len() == MAX_BLOCKS {
            return Err(VimError::IfNestingTooDeep);
        }
        let skip = error || !self.active();
        let result = if skip { None } else { condition() };
        self.open.push(Block {
            kind: Kind::If,
            active: result == Some(true),
            taken: result != Some(false),
            had_else: false,
        });
        Ok(())
    }

    /// `ex_else()` for `:else`.
    pub(super) fn add_else(&mut self, error: bool) -> Result<(), VimError> {
        match self.open.last() {
            Some(block) if block.kind == Kind::If && block.had_else => {
                return Err(VimError::MultipleElse)
            }
            Some(block) if block.kind == Kind::If => {}
            _ => return Err(VimError::ElseWithoutIf),
        }
        let skip = error || !self.outer_active();
        if let Some(block) = self.open.last_mut() {
            *block = if skip || block.taken {
                Block::never_active(Kind::If)
            } else {
                Block {
                    active: true,
                    taken: false,
                    ..*block
                }
            };
            block.had_else = true;
        }
        Ok(())
    }

    /// `ex_else()` for `:elseif`. `condition(skip)` evaluates the condition,
    /// `None` when it fails; when `skip`, it only checks that there is one.
    pub(super) fn add_elseif(
        &mut self,
        error: bool,
        condition: impl FnOnce(bool) -> Option<bool>,
    ) -> Result<(), VimError> {
        let mut skip = error || !self.outer_active();
        let refused = match self.open.last() {
            Some(block) if block.kind == Kind::If && block.had_else => {
                Some(VimError::ElseifAfterElse)
            }
            Some(block) if block.kind == Kind::If => None,
            _ => Some(VimError::ElseifWithoutIf),
        };
        skip |= refused.is_some() || self.open.last().is_some_and(|block| block.taken);
        let result = condition(skip);
        if let Some(error) = refused {
            return Err(error);
        }
        if let Some(block) = self.open.last_mut() {
            *block = match result {
                Some(taken) if !skip => Block {
                    kind: Kind::If,
                    active: taken,
                    taken,
                    had_else: false,
                },
                _ => Block::never_active(Kind::If),
            };
        }
        Ok(())
    }

    /// `ex_endif()`.
    pub(super) fn close_if(&mut self) -> Result<(), VimError> {
        if self.open.last().is_some_and(|block| block.kind == Kind::If) {
            self.open.pop();
            Ok(())
        } else {
            Err(VimError::EndifWithoutIf)
        }
    }

    /// `ex_while()` for `:while` and `:for`, and `ex_try()`: a block that
    /// never gets active.
    pub(super) fn open_never_active(&mut self, kind: Kind) -> Result<(), VimError> {
        if self.open.len() == MAX_BLOCKS {
            return Err(match kind {
                Kind::Try => VimError::TryNestingTooDeep,
                _ => VimError::LoopNestingTooDeep,
            });
        }
        self.open.push(Block::never_active(kind));
        Ok(())
    }

    /// `ex_endwhile()` for `:endwhile` and `:endfor`, then what
    /// `do_cmdline()` does after it for a loop that is not active: it
    /// closes the loop. Blocks inside the loop that are still open are
    /// closed with it, after an error for the innermost.
    pub(super) fn close_loop(&mut self, kind: Kind) -> Result<(), VimError> {
        let without = if kind == Kind::While {
            VimError::EndwhileWithoutWhile
        } else {
            VimError::EndforWithoutFor
        };
        let Some(&top) = self.open.last().filter(|_| self.has_loop()) else {
            return Err(without);
        };
        let mut error = match (kind, top.kind) {
            (Kind::For, Kind::While) => Some(VimError::EndforWithWhile),
            (Kind::While, Kind::For) => Some(VimError::EndwhileWithFor),
            _ => None,
        };
        if !top.kind.is_loop() {
            if top.kind == Kind::If {
                error = Some(VimError::MissingEndif);
            }
            // Down to the second block, as `ex_endwhile()` looks; it gives
            // up at a `:try` not in its `:finally`, which every `:try`
            // here is.
            let found = self
                .open
                .iter()
                .enumerate()
                .skip(1)
                .rev()
                .find(|(_, block)| block.kind == Kind::Try || block.kind == kind);
            let at = match found {
                Some((_, block)) if block.kind == Kind::Try => return Err(without),
                Some((at, _)) => at,
                None => 0,
            };
            self.open.truncate(at + 1);
        }
        self.open.pop();
        error.map_or(Ok(()), Err)
    }

    /// `ex_break()` and `ex_continue()`: when no loop is open, `error`.
    /// A loop never gets active, so a `:break` that runs is outside one.
    pub(super) fn leave_loop(&self, error: VimError) -> Result<(), VimError> {
        if self.has_loop() {
            Ok(())
        } else {
            Err(error)
        }
    }

    /// `ex_catch()` and `ex_finally()`: the blocks inside the `:try` close,
    /// after an error for the innermost. `without` is the error when no
    /// `:try` is open.
    pub(super) fn continue_try(&mut self, without: VimError) -> Result<(), VimError> {
        let Some(at) = self.innermost_try() else {
            return Err(without);
        };
        let error = self.missing_end_inside(at);
        self.open.truncate(at + 1);
        error.map_or(Ok(()), Err)
    }

    /// `ex_endtry()`: closes the innermost `:try` and the blocks inside it,
    /// after an error for the innermost of those. The error is given even
    /// after another error, so `error` (Vim's `did_emsg`) is cleared then.
    pub(super) fn close_try(&mut self, error: &mut bool) -> Result<(), VimError> {
        let Some(at) = self.innermost_try() else {
            return Err(VimError::EndtryWithoutTry);
        };
        let missing = self.missing_end_inside(at);
        if missing.is_some() {
            *error = false;
        }
        self.open.truncate(at);
        missing.map_or(Ok(()), Err)
    }

    /// `get_end_emsg()` for the innermost block, when it is inside the
    /// block at `at`.
    fn missing_end_inside(&self, at: usize) -> Option<VimError> {
        (self.open.len() > at + 1)
            .then(|| self.missing_end())
            .flatten()
    }

    /// The error for the innermost open block at the end of a file, as
    /// `do_cmdline()` gives it.
    pub(super) fn missing_end(&self) -> Option<VimError> {
        Some(match self.open.last()?.kind {
            Kind::If => VimError::MissingEndif,
            Kind::While => VimError::MissingEndwhile,
            Kind::For => VimError::MissingEndfor,
            Kind::Try => VimError::MissingEndtry,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_if(blocks: &mut Blocks, value: Option<bool>) {
        blocks.open_if(false, || value).unwrap();
    }

    #[test]
    fn an_if_runs_its_first_true_branch() {
        let mut blocks = Blocks::default();
        open_if(&mut blocks, Some(false));
        assert!(!blocks.active());
        blocks.add_elseif(false, |_| Some(true)).unwrap();
        assert!(blocks.active());
        blocks.add_elseif(false, |_| Some(true)).unwrap();
        assert!(!blocks.active());
        blocks.add_else(false).unwrap();
        assert!(!blocks.active());
        blocks.close_if().unwrap();
        assert!(blocks.is_empty());
    }

    #[test]
    fn an_if_whose_condition_fails_never_gets_active() {
        // ex_if(): "set TRUE, so this conditional will never get active".
        let mut blocks = Blocks::default();
        open_if(&mut blocks, None);
        blocks.add_elseif(false, |_| Some(true)).unwrap();
        assert!(!blocks.active());
        blocks.add_else(false).unwrap();
        assert!(!blocks.active());
    }

    #[test]
    fn an_elseif_whose_condition_fails_ends_the_branches() {
        let mut blocks = Blocks::default();
        open_if(&mut blocks, Some(false));
        blocks.add_elseif(false, |_| None).unwrap();
        blocks.add_else(false).unwrap();
        assert!(!blocks.active());
    }

    #[test]
    fn a_skipped_if_evaluates_nothing_and_never_gets_active() {
        let mut blocks = Blocks::default();
        open_if(&mut blocks, Some(false));
        blocks
            .open_if(false, || panic!("evaluated while skipping"))
            .unwrap();
        blocks
            .add_elseif(false, |skip| {
                assert!(skip);
                Some(true)
            })
            .unwrap();
        blocks.add_else(false).unwrap();
        assert!(!blocks.active());
        // After an error (did_emsg), an :else of an :if not taken skips too.
        let mut blocks = Blocks::default();
        open_if(&mut blocks, Some(false));
        blocks.add_else(true).unwrap();
        assert!(!blocks.active());
    }

    #[test]
    fn else_and_elseif_errors() {
        // ex_else()
        let mut blocks = Blocks::default();
        assert_eq!(blocks.add_else(false), Err(VimError::ElseWithoutIf));
        assert_eq!(
            blocks.add_elseif(false, |_| Some(true)),
            Err(VimError::ElseifWithoutIf)
        );
        open_if(&mut blocks, Some(false));
        blocks.add_else(false).unwrap();
        assert!(blocks.active());
        assert_eq!(blocks.add_else(false), Err(VimError::MultipleElse));
        assert_eq!(
            blocks.add_elseif(false, |_| Some(true)),
            Err(VimError::ElseifAfterElse)
        );
        // An error changes nothing: the :else branch still runs.
        assert!(blocks.active());
        blocks.open_never_active(Kind::While).unwrap();
        assert_eq!(blocks.add_else(false), Err(VimError::ElseWithoutIf));
        assert_eq!(blocks.close_if(), Err(VimError::EndifWithoutIf));
    }

    #[test]
    fn blocks_nest_fifty_deep() {
        // CSTACK_LEN
        let mut blocks = Blocks::default();
        for _ in 0..MAX_BLOCKS {
            open_if(&mut blocks, Some(true));
        }
        assert_eq!(
            blocks.open_if(false, || Some(true)),
            Err(VimError::IfNestingTooDeep)
        );
        assert_eq!(
            blocks.open_never_active(Kind::For),
            Err(VimError::LoopNestingTooDeep)
        );
        assert_eq!(
            blocks.open_never_active(Kind::Try),
            Err(VimError::TryNestingTooDeep)
        );
        assert_eq!(blocks.open.len(), MAX_BLOCKS);
    }

    #[test]
    fn a_loop_never_gets_active_and_its_end_closes_it() {
        let mut blocks = Blocks::default();
        blocks.open_never_active(Kind::While).unwrap();
        assert!(!blocks.active());
        assert_eq!(blocks.leave_loop(VimError::BreakWithoutLoop), Ok(()));
        blocks.close_loop(Kind::While).unwrap();
        assert!(blocks.is_empty());
        assert_eq!(
            blocks.leave_loop(VimError::BreakWithoutLoop),
            Err(VimError::BreakWithoutLoop)
        );
        assert_eq!(
            blocks.close_loop(Kind::For),
            Err(VimError::EndforWithoutFor)
        );
    }

    #[test]
    fn a_loop_end_of_the_other_kind_still_closes_the_loop() {
        // ex_endwhile(): E732 and E733, then do_cmdline() leaves the loop.
        let mut blocks = Blocks::default();
        blocks.open_never_active(Kind::While).unwrap();
        assert_eq!(blocks.close_loop(Kind::For), Err(VimError::EndforWithWhile));
        assert!(blocks.is_empty());
        blocks.open_never_active(Kind::For).unwrap();
        assert_eq!(
            blocks.close_loop(Kind::While),
            Err(VimError::EndwhileWithFor)
        );
        assert!(blocks.is_empty());
    }

    #[test]
    fn a_loop_end_closes_the_blocks_inside_the_loop() {
        let mut blocks = Blocks::default();
        open_if(&mut blocks, Some(true));
        blocks.open_never_active(Kind::For).unwrap();
        blocks.open_if(false, || Some(true)).unwrap();
        assert_eq!(blocks.close_loop(Kind::For), Err(VimError::MissingEndif));
        assert!(blocks.active());
        blocks.close_if().unwrap();
        assert!(blocks.is_empty());
    }

    #[test]
    fn a_loop_end_does_not_reach_past_a_try() {
        // ex_endwhile(): "Give up at a try conditional not in its finally
        // clause."
        let mut blocks = Blocks::default();
        blocks.open_never_active(Kind::While).unwrap();
        blocks.open_never_active(Kind::Try).unwrap();
        assert_eq!(
            blocks.close_loop(Kind::While),
            Err(VimError::EndwhileWithoutWhile)
        );
        assert_eq!(blocks.open.len(), 2);
    }

    #[test]
    fn catch_finally_and_endtry_close_the_blocks_inside_the_try() {
        let mut blocks = Blocks::default();
        assert_eq!(
            blocks.continue_try(VimError::CatchWithoutTry),
            Err(VimError::CatchWithoutTry)
        );
        let mut error = false;
        assert_eq!(
            blocks.close_try(&mut error),
            Err(VimError::EndtryWithoutTry)
        );
        blocks.open_never_active(Kind::Try).unwrap();
        blocks.continue_try(VimError::CatchWithoutTry).unwrap();
        blocks.open_never_active(Kind::While).unwrap();
        assert_eq!(
            blocks.continue_try(VimError::FinallyWithoutTry),
            Err(VimError::MissingEndwhile)
        );
        assert_eq!(blocks.open.len(), 1);
        open_if(&mut blocks, Some(true));
        let mut error = true;
        assert_eq!(blocks.close_try(&mut error), Err(VimError::MissingEndif));
        assert!(!error, "ex_endtry() reports the error after another");
        assert!(blocks.is_empty());
    }

    #[test]
    fn the_end_of_a_file_names_the_innermost_block() {
        let mut blocks = Blocks::default();
        assert_eq!(blocks.missing_end(), None);
        blocks.open_never_active(Kind::Try).unwrap();
        assert_eq!(blocks.missing_end(), Some(VimError::MissingEndtry));
        open_if(&mut blocks, Some(true));
        assert_eq!(blocks.missing_end(), Some(VimError::MissingEndif));
        blocks.open_never_active(Kind::For).unwrap();
        assert_eq!(blocks.missing_end(), Some(VimError::MissingEndfor));
    }
}
