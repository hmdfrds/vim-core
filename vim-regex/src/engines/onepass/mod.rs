//! One-pass DFA engine for resolving captures at DFA speed.
//!
//! A one-pass DFA can be built when the NFA has no ambiguous transitions:
//! for every state and every input character, at most one path exists.
//! This enables DFA-speed matching with simultaneous capture resolution,
//! avoiding Pike VM thread management overhead.
//!
//! ## File Structure
//!
//! - `mod.rs` -- OnePassDfa struct, public API
//! - `build.rs` -- NFA -> one-pass DFA construction with ambiguity detection
//! - `search.rs` -- Anchored search loop with capture recording
//! - `transition.rs` -- Packed u64 transition entry
//! - `classify.rs` -- Character equivalence classes

mod build;
mod classify;
mod search;
mod transition;

pub(crate) use build::is_onepass_eligible;

use crate::nfa::Nfa;
use crate::VimMatch;

/// Cached state for the one-pass DFA engine.
///
/// This is stored in `Cache::onepass` and lazily constructed on first
/// search invocation. If construction fails (pattern not one-pass),
/// `NotOnePass` prevents retry.
#[derive(Debug, Clone)]
pub(crate) enum OnePassState {
    /// Not yet attempted.
    NotBuilt,
    /// Construction was attempted and failed -- pattern is not one-pass.
    NotOnePass,
    /// One-pass DFA was successfully built.
    Built {
        data: Box<build::OnePassData>,
        scratch: search::OnePassScratch,
    },
}

impl OnePassState {
    /// Attempt to build the one-pass DFA from the given NFA.
    /// On failure, sets self to NotOnePass to prevent retry.
    pub(crate) fn ensure_built(&mut self, nfa: &Nfa) {
        if !matches!(self, Self::NotBuilt) {
            return;
        }
        match build::build(nfa) {
            build::BuildResult::Ok(data) => {
                let scratch = search::OnePassScratch::new(nfa.slot_count() + 2);
                *self = Self::Built {
                    data: Box::new(data),
                    scratch,
                };
            }
            build::BuildResult::NotOnePass => {
                *self = Self::NotOnePass;
            }
        }
    }

    /// Run an anchored one-pass search.
    ///
    /// Returns `None` if the one-pass DFA is not available (not built
    /// or not one-pass) or if no match exists at `pos`.
    pub(crate) fn search_anchored(&mut self, text: &str, pos: usize) -> Option<VimMatch> {
        match self {
            Self::Built { data, scratch } => search::search_anchored(data, scratch, text, pos),
            _ => None,
        }
    }

    /// Returns true if the one-pass DFA was successfully built.
    #[allow(
        dead_code,
        reason = "used by tests via cache.onepass_state().is_available()"
    )]
    pub(crate) fn is_available(&self) -> bool {
        matches!(self, Self::Built { .. })
    }
}
