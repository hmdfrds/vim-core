//! Strategy implementations for the search cascade.
//!
//! Each module exposes a `pub(crate) fn try_search(...)` free function
//! dispatched by the `Strategy` enum in `strategy.rs`.

pub(crate) mod ac_full_match;
pub(crate) mod anchored_start;
pub(crate) mod engine_dispatch;
pub(crate) mod hybrid_dfa;
pub(crate) mod literal_bypass;
pub(crate) mod onepass_dfa;
pub(crate) mod reverse_anchored;
pub(crate) mod reverse_inner;
pub(crate) mod reverse_suffix;
pub(crate) mod small_write;
