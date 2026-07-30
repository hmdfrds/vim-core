//! Byte-level lazy DFA engine.
//!
//! Operates entirely on raw bytes (u8). Multi-byte UTF-8 codepoints become
//! multi-state DFA transitions at construction time. The search loop never
//! decodes UTF-8.

mod byte_classes;
mod cache;
mod search_fwd;
mod search_rev;
mod state_accel;
mod state_table;
mod utf8;

pub(crate) use cache::DfaCache;
pub(crate) use search_fwd::{dfa_search, dfa_search_anchored_stopat, StopAtResult};
pub(crate) use search_rev::dfa_search_reverse;

/// Result of a DFA search operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DfaSearchResult {
    /// Found a match at [start, end).
    Match { start: usize, end: usize },
    /// No match exists from the search position onward.
    NoMatch,
    /// DFA cannot continue (cache thrashing or unsupported feature).
    Quit,
}

/// Saved DFA state for overlapping/resumable search.
///
/// After `find_all_with_cache` reports a match, this struct holds the DFA
/// state ID and position so the next search can resume without re-scanning
/// the unanchored prefix. Used by `find_all` and syntax highlighting.
#[derive(Debug, Clone, Copy)]
#[allow(
    dead_code,
    reason = "infrastructure for future overlapping find_all API"
)]
pub(super) struct OverlappingState {
    /// The DFA state at the point where the last match ended (or was reported).
    dfa_sid: TaggedStateId,
    /// The byte position in the haystack corresponding to `dfa_sid`.
    pos: usize,
}

#[allow(
    dead_code,
    reason = "infrastructure for future overlapping find_all API"
)]
impl OverlappingState {
    /// Create an empty (no saved state) marker.
    #[inline]
    pub(super) const fn empty() -> Self {
        Self {
            dfa_sid: TaggedStateId::UNKNOWN,
            pos: 0,
        }
    }

    /// Create a saved state from a DFA state ID and position.
    #[inline]
    pub(super) const fn new(dfa_sid: TaggedStateId, pos: usize) -> Self {
        Self { dfa_sid, pos }
    }

    /// Whether this represents an empty (unsaved) state.
    #[inline]
    pub(super) const fn is_empty(&self) -> bool {
        self.dfa_sid.is_unknown()
    }

    /// The saved byte position.
    #[inline]
    pub(super) const fn pos(&self) -> usize {
        self.pos
    }

    /// The saved DFA state ID.
    #[inline]
    pub(super) const fn dfa_sid(&self) -> TaggedStateId {
        self.dfa_sid
    }
}

/// Tagged DFA state identifier.
///
/// Uses the 5 high bits of a `u32` for tag flags and the low 27 bits for
/// the premultiplied state index (ordinal * stride). This allows a single
/// `u32` comparison to detect all special states.
///
/// Layout: `[UNKNOWN|DEAD|MATCH|START|ACCEL | 27-bit premultiplied index]`
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub(super) struct TaggedStateId(u32);

impl TaggedStateId {
    const TAG_MASK: u32 = 0xF800_0000; // high 5 bits
    const INDEX_MASK: u32 = 0x07FF_FFFF; // low 27 bits
    const UNKNOWN_BIT: u32 = 1 << 31;
    const DEAD_BIT: u32 = 1 << 30;
    const MATCH_BIT: u32 = 1 << 29;
    const START_BIT: u32 = 1 << 28;
    const ACCEL_BIT: u32 = 1 << 27;

    /// Maximum premultiplied index value (2^27 - 1 = 134_217_727).
    pub(super) const MAX_INDEX: u32 = Self::INDEX_MASK;

    /// Sentinel: state not yet computed.
    pub(super) const UNKNOWN: Self = Self(Self::UNKNOWN_BIT);

    /// Sentinel: dead state (no match possible, all transitions loop to self).
    pub(super) const DEAD: Self = Self(Self::DEAD_BIT);

    /// Sentinel: DFA cannot handle this pattern (backrefs, \z, etc).
    /// The search loop must decline to a fallback engine.
    ///
    /// Uses the DEAD|UNKNOWN bit combination, which is otherwise unreachable:
    /// DEAD means "no transitions out", UNKNOWN means "not yet computed".
    /// A state cannot be both simultaneously in normal operation.
    pub(super) const QUIT: Self = Self(Self::DEAD_BIT | Self::UNKNOWN_BIT);

    /// Returns `true` if this is the QUIT sentinel.
    #[inline(always)]
    pub(super) const fn is_quit(self) -> bool {
        self.0 & Self::TAG_MASK == (Self::DEAD_BIT | Self::UNKNOWN_BIT)
    }

    /// Create a new tagged state ID from a premultiplied index and tag flags.
    #[inline(always)]
    pub(super) const fn new(premul_index: u32, tags: u32) -> Self {
        debug_assert!(premul_index <= Self::MAX_INDEX);
        Self(premul_index | tags)
    }

    /// Create an untagged (normal) state ID from a premultiplied index.
    #[inline(always)]
    pub(super) const fn normal(premul_index: u32) -> Self {
        debug_assert!(premul_index <= Self::MAX_INDEX);
        Self(premul_index)
    }

    /// Returns `true` if any tag bit is set (UNKNOWN, DEAD, MATCH, START, ACCEL).
    /// This is the hot-loop exit condition.
    #[inline(always)]
    pub(super) const fn is_tagged(self) -> bool {
        self.0 & Self::TAG_MASK != 0
    }

    /// Returns the premultiplied index (the low 27 bits).
    /// Used directly as offset into the transition table: `table[sid.index() + class]`.
    #[inline(always)]
    pub(super) const fn index(self) -> usize {
        (self.0 & Self::INDEX_MASK) as usize
    }

    /// Returns `true` if this is the UNKNOWN sentinel.
    #[inline(always)]
    pub(super) const fn is_unknown(self) -> bool {
        self.0 & Self::UNKNOWN_BIT != 0
    }

    /// Returns `true` if this is a dead state.
    #[inline(always)]
    pub(super) const fn is_dead(self) -> bool {
        self.0 & Self::DEAD_BIT != 0
    }

    /// Returns `true` if this is a match state.
    #[inline(always)]
    pub(super) const fn is_match(self) -> bool {
        self.0 & Self::MATCH_BIT != 0
    }

    /// Returns `true` if this is a start state (for prefilter restart).
    #[inline(always)]
    pub(super) const fn is_start(self) -> bool {
        self.0 & Self::START_BIT != 0
    }

    /// Returns `true` if this state is accelerable (memchr skip eligible).
    #[inline(always)]
    pub(super) const fn is_accel(self) -> bool {
        self.0 & Self::ACCEL_BIT != 0
    }

    /// Return a copy with the MATCH tag set.
    #[allow(
        dead_code,
        reason = "setter counterpart to is_match(); DfaCache::alloc_state composes the MATCH bit into the tag word and calls TaggedStateId::new directly, so only this module's tests tag an existing id"
    )]
    #[inline(always)]
    pub(super) const fn with_match(self) -> Self {
        Self(self.0 | Self::MATCH_BIT)
    }

    /// Return a copy with the START tag set.
    #[inline(always)]
    pub(super) const fn with_start(self) -> Self {
        Self(self.0 | Self::START_BIT)
    }

    /// Return a copy with the ACCEL tag set.
    #[inline(always)]
    pub(super) const fn with_accel(self) -> Self {
        Self(self.0 | Self::ACCEL_BIT)
    }

    /// Return the raw u32 representation (for debug/display).
    #[allow(
        dead_code,
        reason = "escape hatch exposing the packed u32; nothing calls it because the Debug impl below renders the tags through the individual accessors instead"
    )]
    #[inline(always)]
    pub(super) const fn raw(self) -> u32 {
        self.0
    }
}

impl core::fmt::Debug for TaggedStateId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut tags = String::new();
        if self.is_unknown() {
            tags.push_str("UNKNOWN|");
        }
        if self.is_dead() {
            tags.push_str("DEAD|");
        }
        if self.is_match() {
            tags.push_str("MATCH|");
        }
        if self.is_start() {
            tags.push_str("START|");
        }
        if self.is_accel() {
            tags.push_str("ACCEL|");
        }
        if tags.is_empty() {
            write!(f, "Sid({})", self.index())
        } else {
            tags.pop(); // trailing '|'
            write!(f, "Sid({}|{})", tags, self.index())
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tagged_state_id_zero_is_normal() {
        let sid = TaggedStateId::normal(0);
        assert!(!sid.is_tagged());
        assert_eq!(sid.index(), 0);
        assert!(!sid.is_unknown());
        assert!(!sid.is_dead());
        assert!(!sid.is_match());
    }

    #[test]
    fn tagged_state_id_unknown_sentinel() {
        let sid = TaggedStateId::UNKNOWN;
        assert!(sid.is_tagged());
        assert!(sid.is_unknown());
        assert!(!sid.is_dead());
        assert!(!sid.is_match());
    }

    #[test]
    fn tagged_state_id_dead_sentinel() {
        let sid = TaggedStateId::DEAD;
        assert!(sid.is_tagged());
        assert!(sid.is_dead());
        assert!(!sid.is_unknown());
        assert!(!sid.is_match());
    }

    #[test]
    fn tagged_state_id_match_preserves_index() {
        let sid = TaggedStateId::normal(42).with_match();
        assert!(sid.is_tagged());
        assert!(sid.is_match());
        assert_eq!(sid.index(), 42);
        assert!(!sid.is_dead());
    }

    #[test]
    fn tagged_state_id_accel_plus_match() {
        let sid = TaggedStateId::normal(100).with_match().with_accel();
        assert!(sid.is_tagged());
        assert!(sid.is_match());
        assert!(sid.is_accel());
        assert_eq!(sid.index(), 100);
    }

    #[test]
    fn tagged_state_id_max_index() {
        let sid = TaggedStateId::normal(TaggedStateId::MAX_INDEX);
        assert!(!sid.is_tagged());
        assert_eq!(sid.index(), TaggedStateId::MAX_INDEX as usize);
    }

    #[test]
    fn tagged_state_id_start_tag() {
        let sid = TaggedStateId::normal(8).with_start();
        assert!(sid.is_tagged());
        assert!(sid.is_start());
        assert_eq!(sid.index(), 8);
    }

    #[test]
    fn tagged_state_id_is_tagged_is_hot_path_check() {
        // Normal states must NOT trigger is_tagged (hot loop continues)
        for idx in [0u32, 1, 64, 1024, 65536, TaggedStateId::MAX_INDEX] {
            let sid = TaggedStateId::normal(idx);
            assert!(!sid.is_tagged(), "normal({idx}) must not be tagged");
        }
    }

    #[test]
    fn quit_sentinel_bit_pattern() {
        assert!(TaggedStateId::QUIT.is_quit());
        assert!(TaggedStateId::QUIT.is_dead()); // DEAD bit is set
        assert!(TaggedStateId::QUIT.is_unknown()); // UNKNOWN bit is set
        assert!(TaggedStateId::QUIT.is_tagged()); // Tag bits are set
        assert!(!TaggedStateId::DEAD.is_quit());
        assert!(!TaggedStateId::UNKNOWN.is_quit());
        assert!(!TaggedStateId::normal(0).is_quit());
    }

    #[test]
    fn overlapping_state_default_is_empty() {
        let state = OverlappingState::empty();
        assert!(state.is_empty());
    }

    #[test]
    fn overlapping_state_roundtrip() {
        let sid = TaggedStateId::normal(42);
        let state = OverlappingState::new(sid, 100);
        assert!(!state.is_empty());
        assert_eq!(state.pos(), 100);
        assert_eq!(state.dfa_sid(), sid);
    }
}
