//! Prefilter acceleration infrastructure for the regex engine.
//!
//! Provides fast-skip heuristics that avoid running the full NFA at
//! every position. The key components:
//!
//! - **Prefilter trait** + 8 concrete variants: `SingleByte`, `DualByte`,
//!   `TriByte`, `SubstringPrefilter`, `ByteSet`, `PackedPrefilter`,
//!   `NewlineStartPrefilter`, `AcPrefilter` (Aho-Corasick)
//! - **Start bitmap** — `compute_start_bitmap` derives a 256-bit set of possible
//!   match-starting bytes from the sound `StartSet` over-approximation
//! - **Inner literal** — "regmust" extraction of mandatory interior literals
//! - **Range narrowing** — restricts the search range using `PatternProperties`

pub(crate) mod aho_corasick;
mod inner_literal;
mod prefilter;
mod prefilter_tree;
mod range_narrow;
mod required_byte;
mod start_desc;
mod suffix_literal;

pub(crate) use inner_literal::extract_inner_literal;
pub(crate) use inner_literal::extract_inner_literal_with_position;
pub(crate) use prefilter::{build_case_insensitive_prefilter, build_prefilter, Prefilter};
pub(crate) use prefilter_tree::{build_prefilter_tree, PrefilterNode};
pub(crate) use range_narrow::narrow_search_range;
pub(crate) use required_byte::extract_required_byte;
pub(crate) use start_desc::compute_start_bitmap;
pub(crate) use suffix_literal::extract_suffix_literal;
