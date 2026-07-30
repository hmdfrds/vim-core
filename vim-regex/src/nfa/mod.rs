//! NFA graph types and Thompson's construction for Vim regex.
//!
//! Converts parsed `VimPatternNode` trees into non-deterministic finite
//! automata (NFA) using Thompson's construction. The resulting NFA
//! supports all Vim regex features including backreferences, lookarounds,
//! and optional sequences.
//!
//! # Layering
//!
//! Imports:
//!
//! | Dependency     | Used by                        | Purpose                                |
//! |----------------|--------------------------------|----------------------------------------|
//! | `regex::ir`    | `LookaroundKind`               | AST types for NFA construction         |
//! | `regex::matchers` | `CharMatcher`, `ZeroWidthMatcher` | Matcher types for transitions      |
//!
//! Must not import `commands`, `grammar`, `effects`, `state`, `execution`,
//! `mode`, `keymap`, `dispatch`, `errors` or `document`.

pub(crate) mod builder;
mod types;

// Re-export types for sibling submodule (builder) and external consumers.
pub(crate) use types::{
    CaptureGroup, CaptureSlot, MatcherId, Nfa, PendingLookbehind, QuantifierHint, StateId,
    SubNfaId, Transition, TransitionKind,
};
pub(super) use types::{NfaFragment, NfaState};

#[cfg(test)]
#[path = "../tests/nfa/mod.rs"]
mod tests;
