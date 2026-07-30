//! Compact character set for first-character analysis.
//!
//! `CharSet` uses a 128-bit bitmap (`[u64; 2]`) for ASCII codepoints 0-127,
//! a boolean for non-ASCII membership, and an optional sorted vector of
//! Unicode ranges for precise non-ASCII tracking.
//!
//! This is an internal acceleration structure used by `StartSet::of` and
//! `auto_possessify` — it is never exposed in the public API.

use crate::ir::{CaseMode, CharClass, CollectionItem, PosixClassName};
use crate::matchers::{class_matches, class_matches_ascii, posix_class_matches};

// ═══════════════════════════════════════════════════════════════════════════════
// NON-ASCII PRESENCE TRI-STATE
// ═══════════════════════════════════════════════════════════════════════════════

/// Tri-state for non-ASCII membership in a `CharSet`.
///
/// - `None` — no non-ASCII codepoints can match.
/// - `Some` — specific non-ASCII codepoints can match (tracked by `unicode_ranges`).
/// - `All` — all non-ASCII codepoints can match (wildcard).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NonAsciiPresence {
    None,
    Some,
    All,
}

// ═══════════════════════════════════════════════════════════════════════════════
// CHARSET STRUCT
// ═══════════════════════════════════════════════════════════════════════════════

/// A compact character set for first-character analysis.
///
/// Encodes which characters a regex node can match as its first consumed
/// character. Used by `auto_possessify` to determine when adjacent
/// quantifiers have disjoint first-character sets, enabling possessive
/// quantifier promotion.
///
/// # Representation
///
/// - `ascii[0]` covers codepoints 0-63 (bit *i* = codepoint *i*).
/// - `ascii[1]` covers codepoints 64-127 (bit *i* = codepoint 64+*i*).
/// - `non_ascii` indicates whether non-ASCII codepoints (>= 128) can match:
///   `None` = no, `Some` = specific ranges, `All` = all.
/// - `unicode_ranges` optionally holds sorted, non-overlapping `(lo, hi)`
///   ranges for precise non-ASCII membership. When `None` and
///   `non_ascii` is `All`, the set is conservatively treated as
///   containing *all* non-ASCII characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CharSet {
    /// 128-bit bitmap for ASCII codepoints 0-127.
    ascii: [u64; 2],
    /// Non-ASCII membership: `None` = no, `Some` = specific ranges, `All` = all.
    non_ascii: NonAsciiPresence,
    /// Sorted, non-overlapping Unicode ranges for codepoints >= 128.
    /// `None` means "unknown" — when `non_ascii` is `All`, treat as all.
    unicode_ranges: Option<Vec<(char, char)>>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// CONSTRUCTORS
// ═══════════════════════════════════════════════════════════════════════════════

impl CharSet {
    /// Empty set — matches nothing.
    pub(crate) const fn empty() -> Self {
        Self {
            ascii: [0; 2],
            non_ascii: NonAsciiPresence::None,
            unicode_ranges: None,
        }
    }

    /// Universal set — matches every character (all ASCII + all non-ASCII).
    #[allow(
        dead_code,
        reason = "infrastructure: available for future analysis passes"
    )]
    pub(crate) fn full() -> Self {
        Self {
            ascii: [u64::MAX, u64::MAX],
            non_ascii: NonAsciiPresence::All,
            unicode_ranges: None,
        }
    }

    /// All ASCII characters (0-127), no non-ASCII.
    #[allow(
        dead_code,
        reason = "infrastructure: available for future analysis passes"
    )]
    pub(crate) const fn ascii_full() -> Self {
        Self {
            ascii: [u64::MAX, u64::MAX],
            non_ascii: NonAsciiPresence::None,
            unicode_ranges: None,
        }
    }

    /// Singleton set containing exactly one character.
    pub(crate) fn from_literal(ch: char) -> Self {
        let mut set = Self::empty();
        set.insert_char(ch);
        set
    }

    /// Singleton set containing a character AND its ASCII case-fold variant.
    /// Used during case-insensitive possessification analysis.
    pub(crate) fn from_literal_ci(ch: char) -> Self {
        let mut set = Self::from_literal(ch);
        if ch.is_ascii_alphabetic() {
            let folded = if ch.is_ascii_uppercase() {
                ch.to_ascii_lowercase()
            } else {
                ch.to_ascii_uppercase()
            };
            set.insert_char(folded);
        }
        set
    }

    /// `.` — any character except `\n`.
    #[allow(
        dead_code,
        reason = "infrastructure: available for future analysis passes"
    )]
    pub(crate) fn from_any_char() -> Self {
        let mut set = Self::full();
        set.remove_char('\n');
        set
    }

    /// `\_.` — any character including `\n` (truly universal).
    #[allow(
        dead_code,
        reason = "infrastructure: available for future analysis passes"
    )]
    pub(crate) fn from_any_char_nl() -> Self {
        Self::full()
    }

    /// Returns `true` if the set matches no characters at all.
    #[allow(
        dead_code,
        reason = "infrastructure: available for future analysis passes"
    )]
    pub(crate) fn is_empty(&self) -> bool {
        self.ascii[0] == 0
            && self.ascii[1] == 0
            && self.non_ascii == NonAsciiPresence::None
            && self.unicode_ranges.as_ref().is_none_or(|r| r.is_empty())
    }

    /// Returns `true` if the set matches every character (all ASCII + all non-ASCII).
    pub(crate) fn is_universal(&self) -> bool {
        self.ascii == [u64::MAX; 2] && self.non_ascii != NonAsciiPresence::None
    }

    /// True if any non-ASCII character is in the set (so a byte prefilter built
    /// from `to_byte_vec` — which only emits ASCII bytes — would be unsound).
    pub(crate) fn has_non_ascii(&self) -> bool {
        self.non_ascii != NonAsciiPresence::None
    }

    /// Enumerate all ASCII bytes where the corresponding bit is set.
    ///
    /// Non-ASCII characters are not included in the result.
    pub(crate) fn to_byte_vec(&self) -> Vec<u8> {
        let count = (self.ascii[0].count_ones() + self.ascii[1].count_ones()) as usize;
        let mut result = Vec::with_capacity(count);
        for word_idx in 0..2u32 {
            let mut bits = self.ascii[word_idx as usize];
            while bits != 0 {
                let bit = bits.trailing_zeros();
                result.push((word_idx * 64 + bit) as u8);
                bits &= bits - 1; // clear lowest set bit
            }
        }
        result
    }

    /// Pack the 128-bit ASCII bitmap into a 256-bit start bitmap.
    ///
    /// The output `[u32; 8]` has bit `b` set if byte value `b` is in the set.
    /// For ASCII codepoints 0-127, bit `b` maps directly to the byte value.
    /// The upper 128 bits (bytes 128-255) are all set if `non_ascii` is not `None`
    /// (any non-ASCII byte could start a multi-byte UTF-8 sequence for a matching
    /// codepoint).
    pub(crate) fn as_start_bitmap(&self) -> [u32; 8] {
        let mut bitmap = [0u32; 8];

        // Map ASCII codepoints 0-127 into the lower 4 words (bits 0-127).
        // ascii[0] covers codepoints 0-63, ascii[1] covers 64-127.
        for word_idx in 0..2u32 {
            let bits = self.ascii[word_idx as usize];
            // Each u64 spans two u32 words in the bitmap.
            let base = (word_idx * 2) as usize;
            bitmap[base] = bits as u32;
            bitmap[base + 1] = (bits >> 32) as u32;
        }

        // If non-ASCII characters can match, set bits 128-255 (words 4-7).
        // Any non-ASCII Unicode codepoint starts with a byte >= 128 in UTF-8.
        if self.non_ascii != NonAsciiPresence::None {
            bitmap[4] = u32::MAX;
            bitmap[5] = u32::MAX;
            bitmap[6] = u32::MAX;
            bitmap[7] = u32::MAX;
        }

        bitmap
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// INTERNAL HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

impl CharSet {
    /// Insert a single character into the set.
    pub(crate) fn insert_char(&mut self, ch: char) {
        let cp = ch as u32;
        if cp < 128 {
            let idx = (cp >> 6) as usize; // 0 or 1
            let bit = cp & 63;
            self.ascii[idx] |= 1u64 << bit;
        } else {
            self.non_ascii = NonAsciiPresence::Some;
            self.insert_unicode_range(ch, ch);
        }
    }

    /// Remove a single character from the set.
    pub(crate) fn remove_char(&mut self, ch: char) {
        let cp = ch as u32;
        if cp < 128 {
            let idx = (cp >> 6) as usize;
            let bit = cp & 63;
            self.ascii[idx] &= !(1u64 << bit);
        } else {
            // For non-ASCII removal we clear the range list entry.
            // If unicode_ranges is None (wildcard), we can't precisely remove
            // a single char from "all non-ASCII" without enumerating everything,
            // so this is a best-effort operation for the common case (ranges present).
            if let Some(ref mut ranges) = self.unicode_ranges {
                ranges.retain(|&(lo, hi)| !(lo == ch && hi == ch));
                if ranges.is_empty() {
                    self.non_ascii = NonAsciiPresence::None;
                }
            }
        }
    }

    /// Insert an inclusive ASCII range `[lo, hi]` where both are < 128.
    ///
    /// # Panics
    ///
    /// Panics in debug mode if `lo > hi` or either is >= 128.
    pub(crate) fn insert_ascii_range(&mut self, lo: u8, hi: u8) {
        debug_assert!(lo <= hi, "insert_ascii_range: lo ({lo}) > hi ({hi})");
        debug_assert!(hi < 128, "insert_ascii_range: hi ({hi}) >= 128");

        for cp in u32::from(lo)..=u32::from(hi) {
            let idx = (cp >> 6) as usize;
            let bit = cp & 63;
            self.ascii[idx] |= 1u64 << bit;
        }
    }

    /// Insert an inclusive Unicode range `[lo, hi]` into the non-ASCII range list.
    ///
    /// The caller must ensure both endpoints are >= 128 (or this is called from
    /// `insert_char` which already checked). Ranges are merged and kept sorted.
    pub(crate) fn insert_unicode_range(&mut self, lo: char, hi: char) {
        debug_assert!(lo <= hi, "insert_unicode_range: lo > hi");

        // If both are ASCII, delegate to insert_ascii_range.
        if (hi as u32) < 128 {
            self.insert_ascii_range(lo as u8, hi as u8);
            return;
        }

        // If the range spans into ASCII, handle the ASCII portion separately.
        if (lo as u32) < 128 {
            self.insert_ascii_range(lo as u8, 127);
            // Continue with the non-ASCII portion.
            let non_ascii_lo = '\u{80}';
            self.non_ascii = NonAsciiPresence::Some;
            let ranges = self.unicode_ranges.get_or_insert_with(Vec::new);
            Self::merge_range(ranges, non_ascii_lo, hi);
            return;
        }

        // Pure non-ASCII range.
        self.non_ascii = NonAsciiPresence::Some;
        let ranges = self.unicode_ranges.get_or_insert_with(Vec::new);
        Self::merge_range(ranges, lo, hi);
    }

    /// Merge a `(lo, hi)` range into a sorted, non-overlapping range list.
    fn merge_range(ranges: &mut Vec<(char, char)>, lo: char, hi: char) {
        // Find insertion point.
        let pos = ranges.partition_point(|&(_, rhi)| rhi < lo);

        // Determine how many existing ranges overlap or are adjacent.
        let mut merged_lo = lo;
        let mut merged_hi = hi;
        let mut end = pos;

        while end < ranges.len() && ranges[end].0 <= Self::next_char(hi) {
            merged_lo = core::cmp::min(merged_lo, ranges[end].0);
            merged_hi = core::cmp::max(merged_hi, ranges[end].1);
            end += 1;
        }

        // Also check if we should merge with the range just before `pos`.
        if pos > 0 && Self::next_char(ranges[pos - 1].1) >= lo {
            merged_lo = core::cmp::min(merged_lo, ranges[pos - 1].0);
            merged_hi = core::cmp::max(merged_hi, ranges[pos - 1].1);
            ranges.splice((pos - 1)..end, [(merged_lo, merged_hi)]);
        } else {
            ranges.splice(pos..end, [(merged_lo, merged_hi)]);
        }
    }

    /// Returns the next char after `ch`, or `ch` itself if at char::MAX.
    fn next_char(ch: char) -> char {
        let cp = ch as u32;
        // Skip the surrogate gap (0xD800..=0xDFFF).
        match cp {
            0x10FFFF => ch,       // char::MAX
            0xD7FF => '\u{E000}', // skip surrogates
            _ => char::from_u32(cp + 1).unwrap_or(char::MAX),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ALL-NON-ASCII CLASS DETECTION
// ═══════════════════════════════════════════════════════════════════════════════

/// Returns `true` if the given `CharClass` matches all non-ASCII codepoints.
///
/// This includes negated classes (`\D`, `\W`, `\S`, etc.) which match everything
/// *except* a small set of ASCII characters, and `Print`/`SPrint` which match
/// all non-ASCII codepoints via `!is_ascii_control()`.
fn matches_all_non_ascii(class: CharClass) -> bool {
    matches!(
        class,
        CharClass::NotDigit
            | CharClass::NotWord
            | CharClass::NotWhitespace
            | CharClass::NotAlpha
            | CharClass::NotLower
            | CharClass::NotUpper
            | CharClass::NotHex
            | CharClass::NotHead
            | CharClass::NOctal
            | CharClass::Print
            | CharClass::SPrint
    )
}

// ═══════════════════════════════════════════════════════════════════════════════
// FROM CLASS / FROM COLLECTION / SET OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

impl CharSet {
    /// Build a `CharSet` from a Vim `CharClass` (e.g. `\d`, `\w`, `\D`).
    ///
    /// Iterates ASCII 0-127 and tests each codepoint against [`class_matches`].
    /// For classes that match all non-ASCII (`\D`, `\W`, `Print`, `SPrint`, etc.),
    /// `non_ascii` is set to `All` because all non-ASCII codepoints match.
    ///
    /// Under `CaseMode::Insensitive` the ASCII-alpha portion is case-folded
    /// (mirroring `from_literal_ci`). A case-bearing class such as `\l` then also
    /// includes its opposite-case letters; case-neutral classes (`\d`, `\w`, `\s`,
    /// …) are unchanged because folding their alpha members yields the same set.
    /// `CaseMode::Default`/`Sensitive` are treated as case-sensitive (the set is
    /// an over-approximation; CI only widens it).
    pub(crate) fn from_class(class: CharClass, case_mode: CaseMode) -> Self {
        let mut set = Self::empty();
        for cp in 0u32..128 {
            let ch = cp as u8 as char;
            if class_matches(class, ch) {
                let idx = (cp >> 6) as usize;
                let bit = cp & 63;
                set.ascii[idx] |= 1u64 << bit;
            }
        }
        if matches!(case_mode, CaseMode::Insensitive) {
            set.fold_ascii_case();
        }
        if matches_all_non_ascii(class) {
            set.non_ascii = NonAsciiPresence::All;
            // unicode_ranges stays None — wildcard for all non-ASCII.
        }
        set
    }

    /// Build a `CharSet` from a POSIX named class (e.g. `[:alpha:]`, `[:digit:]`).
    ///
    /// Iterates ASCII 0-127 and tests each codepoint against [`posix_class_matches`].
    /// POSIX classes are always ASCII-only, so `non_ascii` remains `None`. Under
    /// `CaseMode::Insensitive` the ASCII-alpha portion is case-folded.
    pub(crate) fn from_posix_class(name: PosixClassName, case_mode: CaseMode) -> Self {
        let mut set = Self::empty();
        for cp in 0u32..128 {
            let ch = cp as u8 as char;
            if posix_class_matches(name, ch) {
                let idx = (cp >> 6) as usize;
                let bit = cp & 63;
                set.ascii[idx] |= 1u64 << bit;
            }
        }
        if matches!(case_mode, CaseMode::Insensitive) {
            set.fold_ascii_case();
        }
        set
    }

    /// Case-fold the ASCII-alpha portion in place: for every ASCII letter in the
    /// set, also add its opposite-case counterpart. Used to widen a set under
    /// `CaseMode::Insensitive` (mirrors `from_literal_ci`).
    fn fold_ascii_case(&mut self) {
        for cp in u32::from(b'a')..=u32::from(b'z') {
            let idx = (cp >> 6) as usize;
            let bit = cp & 63;
            if self.ascii[idx] & (1u64 << bit) != 0 {
                let upper = cp - 32; // 'a'..'z' -> 'A'..'Z'
                self.ascii[(upper >> 6) as usize] |= 1u64 << (upper & 63);
            }
        }
        for cp in u32::from(b'A')..=u32::from(b'Z') {
            let idx = (cp >> 6) as usize;
            let bit = cp & 63;
            if self.ascii[idx] & (1u64 << bit) != 0 {
                let lower = cp + 32; // 'A'..'Z' -> 'a'..'z'
                self.ascii[(lower >> 6) as usize] |= 1u64 << (lower & 63);
            }
        }
    }

    /// Build a `CharSet` from a `[...]` collection, handling negation and newline.
    ///
    /// Expands each `CollectionItem` into the set, then applies negation.
    /// When `include_newline` is true (from `\_[...]`), newline is always included.
    ///
    /// Under `CaseMode::Insensitive` the positive set is case-folded before
    /// negation, so `\c[a-m]` expands to `a–m ∪ A–M` and `\c[^a-m]` excludes
    /// both. `CaseMode::Default`/`Sensitive` are treated as case-sensitive — the
    /// CharSet is an over-approximation, and CI only widens it.
    pub(crate) fn from_collection(
        items: &[CollectionItem],
        negated: bool,
        include_newline: bool,
        case_mode: CaseMode,
    ) -> Self {
        // Phase 1: build the positive (un-negated) set from all items.
        let mut set = Self::empty();
        for item in items {
            let item_set = Self::from_collection_item(item, case_mode);
            set = set.union(&item_set);
        }

        // Phase 2: apply negation. Folding (in from_collection_item) happens
        // before this, so the complement of a CI set excludes both cases.
        if negated {
            set = set.complement();
            // Negated collections never match \n unless include_newline is set
            // or the items contained an explicit Newline (which would have been
            // removed by complement — we restore it below if needed).
            set.remove_char('\n');
        }

        // Phase 3: include_newline from `\_[...]` always adds \n.
        if include_newline {
            set.insert_char('\n');
        }

        set
    }

    /// Expand a single `CollectionItem` into a `CharSet`, applying ASCII case
    /// folding under `CaseMode::Insensitive`.
    fn from_collection_item(item: &CollectionItem, case_mode: CaseMode) -> Self {
        let ci = matches!(case_mode, CaseMode::Insensitive);
        match item {
            CollectionItem::Single(ch) => {
                if ci {
                    Self::from_literal_ci(*ch)
                } else {
                    Self::from_literal(*ch)
                }
            }
            CollectionItem::Range(lo, hi) => {
                let mut set = Self::empty();
                // Handle the full range, including non-ASCII if applicable.
                set.insert_unicode_range(*lo, *hi);
                if ci {
                    set.fold_ascii_case();
                }
                set
            }
            CollectionItem::Class(class) => Self::from_class(*class, case_mode),
            CollectionItem::PosixClass(name) => Self::from_posix_class(*name, case_mode),
            CollectionItem::Newline => Self::from_literal('\n'),
        }
    }

    /// Union of two `CharSet`s — the result contains any character in either set.
    ///
    /// ASCII bitmaps are OR-ed. `non_ascii` is combined (All wins over Some,
    /// Some wins over None). Unicode ranges are merged from both sides.
    pub(crate) fn union(&self, other: &CharSet) -> CharSet {
        let ascii = [
            self.ascii[0] | other.ascii[0],
            self.ascii[1] | other.ascii[1],
        ];
        let non_ascii = match (self.non_ascii, other.non_ascii) {
            (NonAsciiPresence::All, _) | (_, NonAsciiPresence::All) => NonAsciiPresence::All,
            (NonAsciiPresence::Some, _) | (_, NonAsciiPresence::Some) => NonAsciiPresence::Some,
            (NonAsciiPresence::None, NonAsciiPresence::None) => NonAsciiPresence::None,
        };

        let unicode_ranges = match (&self.unicode_ranges, &other.unicode_ranges) {
            // If either side is a wildcard (None + All), the union is also wildcard.
            (None, _) if self.non_ascii == NonAsciiPresence::All => None,
            (_, None) if other.non_ascii == NonAsciiPresence::All => None,
            // Both have explicit ranges — merge them.
            (Some(a), Some(b)) => {
                let mut merged = Self {
                    ascii: [0; 2],
                    non_ascii: NonAsciiPresence::None,
                    unicode_ranges: Some(a.clone()),
                };
                for &(lo, hi) in b {
                    Self::merge_range(
                        merged.unicode_ranges.as_mut().expect("just created"),
                        lo,
                        hi,
                    );
                }
                merged.unicode_ranges
            }
            // One side has ranges, the other has none (and no non-ASCII).
            (Some(ranges), None) | (None, Some(ranges)) => Some(ranges.clone()),
            // Neither side has ranges and neither has non-ASCII.
            (None, None) => None,
        };

        CharSet {
            ascii,
            non_ascii,
            unicode_ranges,
        }
    }

    /// Intersection of two `CharSet`s — the result contains only characters in both sets.
    ///
    /// ASCII bitmaps are AND-ed. `non_ascii` is combined conservatively (None if
    /// either is None, All if both All, Some otherwise). Unicode ranges are
    /// intersected via a two-pointer sweep over sorted ranges.
    pub(crate) fn intersection(&self, other: &CharSet) -> CharSet {
        let ascii = [
            self.ascii[0] & other.ascii[0],
            self.ascii[1] & other.ascii[1],
        ];
        let non_ascii = match (self.non_ascii, other.non_ascii) {
            (NonAsciiPresence::None, _) | (_, NonAsciiPresence::None) => NonAsciiPresence::None,
            (NonAsciiPresence::All, NonAsciiPresence::All) => NonAsciiPresence::All,
            _ => NonAsciiPresence::Some,
        };

        let unicode_ranges = if non_ascii == NonAsciiPresence::None {
            // No non-ASCII in the intersection — empty range list.
            Some(vec![])
        } else {
            match (&self.unicode_ranges, &other.unicode_ranges) {
                // Both have precise ranges — compute the intersection.
                (Some(a), Some(b)) => Some(unicode_ranges_intersect(a, b)),
                // One side is a wildcard (None + non_ascii != None) — the intersection
                // is the other side's precise ranges (or wildcard if both are).
                (Some(ranges), None) | (None, Some(ranges)) => Some(ranges.clone()),
                // Both are wildcards — intersection is also wildcard.
                (None, None) => None,
            }
        };

        CharSet {
            ascii,
            non_ascii,
            unicode_ranges,
        }
    }

    /// Returns `true` if the two sets share no characters in common.
    ///
    /// **This is the key operation for auto-possessification.** It determines
    /// whether a quantifier's repeated atom and the following atom can never
    /// match the same first character.
    ///
    /// # Conservatism
    ///
    /// This function is conservative: it may return `false` (not disjoint) when
    /// the sets are actually disjoint, but it **MUST NEVER** return `true` when
    /// they overlap. False negatives are safe (we skip the optimization). False
    /// positives would cause incorrect possessification.
    pub(crate) fn is_disjoint(&self, other: &CharSet) -> bool {
        // ─── ASCII fast path ────────────────────────────────────────────
        // Two bitwise ANDs + one OR — if any ASCII bit overlaps, bail immediately.
        if (self.ascii[0] & other.ascii[0]) | (self.ascii[1] & other.ascii[1]) != 0 {
            return false;
        }

        // ─── Non-ASCII check ────────────────────────────────────────────
        // If either side has no non-ASCII at all, they're disjoint on non-ASCII.
        if self.non_ascii == NonAsciiPresence::None || other.non_ascii == NonAsciiPresence::None {
            return true;
        }

        // Both sides have non-ASCII characters.
        // If either side is All (wildcard), they definitely overlap.
        if self.non_ascii == NonAsciiPresence::All || other.non_ascii == NonAsciiPresence::All {
            return false;
        }

        // Both sides are Some — check precise ranges.
        match (&self.unicode_ranges, &other.unicode_ranges) {
            (Some(a), Some(b)) => unicode_ranges_disjoint(a, b),
            // One side is imprecise (e.g., complement result) — conservative: not disjoint.
            _ => false,
        }
    }

    /// Complement (bitwise NOT) of a `CharSet`.
    ///
    /// Every ASCII codepoint that was out is now in, and vice versa.
    /// `non_ascii` is flipped with tri-state logic:
    /// - `None` -> `All`: no non-ASCII becomes all non-ASCII.
    /// - `Some` -> `Some`: specific non-ASCII codepoints become all-except-those
    ///   (still some non-ASCII present, just different ones).
    /// - `All` -> `None`: all non-ASCII becomes no non-ASCII.
    ///
    /// Unicode ranges: for `None` result, ranges are empty; for `All`, ranges
    /// are `None` (wildcard); for `Some`, ranges are `None` (imprecise —
    /// precise complement of ranges is expensive; conservative for disjointness).
    pub(crate) fn complement(&self) -> CharSet {
        let ascii = [!self.ascii[0], !self.ascii[1]];

        let non_ascii = match self.non_ascii {
            NonAsciiPresence::None => NonAsciiPresence::All,
            NonAsciiPresence::Some => NonAsciiPresence::Some,
            NonAsciiPresence::All => NonAsciiPresence::None,
        };

        let unicode_ranges = match non_ascii {
            NonAsciiPresence::None => Some(vec![]),
            NonAsciiPresence::All => None,
            // Precise complement of ranges is expensive; use wildcard (conservative).
            NonAsciiPresence::Some => None,
        };

        CharSet {
            ascii,
            non_ascii,
            unicode_ranges,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// UNICODE RANGE HELPERS — Two-Pointer Sweeps
// ═══════════════════════════════════════════════════════════════════════════════

/// Compute the intersection of two sorted, non-overlapping Unicode range lists.
///
/// Uses a two-pointer sweep: for each pair of ranges from `a` and `b`, the
/// intersection is `[max(a_lo, b_lo), min(a_hi, b_hi)]` when that interval is
/// non-empty. Advance whichever pointer has the smaller `hi` endpoint.
///
/// Returns a new sorted, non-overlapping range list.
fn unicode_ranges_intersect(a: &[(char, char)], b: &[(char, char)]) -> Vec<(char, char)> {
    let mut result = Vec::new();
    let mut i = 0;
    let mut j = 0;

    while i < a.len() && j < b.len() {
        let (a_lo, a_hi) = a[i];
        let (b_lo, b_hi) = b[j];

        // The intersection of [a_lo, a_hi] and [b_lo, b_hi] is
        // [max(a_lo, b_lo), min(a_hi, b_hi)] — non-empty iff lo <= hi.
        let lo = core::cmp::max(a_lo, b_lo);
        let hi = core::cmp::min(a_hi, b_hi);

        if lo <= hi {
            result.push((lo, hi));
        }

        // Advance the pointer with the smaller hi (it can't overlap with
        // any further ranges from the other list).
        if a_hi <= b_hi {
            i += 1;
        } else {
            j += 1;
        }
    }

    result
}

/// Check if two sorted, non-overlapping Unicode range lists are disjoint
/// (share no codepoints in common).
///
/// Uses a two-pointer sweep identical to `unicode_ranges_intersect` but
/// short-circuits on the first overlap found.
fn unicode_ranges_disjoint(a: &[(char, char)], b: &[(char, char)]) -> bool {
    let mut i = 0;
    let mut j = 0;

    while i < a.len() && j < b.len() {
        let (a_lo, a_hi) = a[i];
        let (b_lo, b_hi) = b[j];

        // Check for overlap: [a_lo, a_hi] and [b_lo, b_hi] overlap iff
        // max(a_lo, b_lo) <= min(a_hi, b_hi).
        let lo = core::cmp::max(a_lo, b_lo);
        let hi = core::cmp::min(a_hi, b_hi);

        if lo <= hi {
            return false;
        }

        // Advance the pointer with the smaller hi.
        if a_hi <= b_hi {
            i += 1;
        } else {
            j += 1;
        }
    }

    true
}

// ═══════════════════════════════════════════════════════════════════════════════
// STATIC DISJOINTNESS TABLE — O(1) Lookup for Common Classes
// ═══════════════════════════════════════════════════════════════════════════════

/// The 8 common Vim character classes included in the static disjointness table.
///
/// Order: `\w`, `\W`, `\d`, `\D`, `\s`, `\S`, `\h`, `\H`.
const DISJOINT_CLASSES: [CharClass; 8] = [
    CharClass::Word,
    CharClass::NotWord,
    CharClass::Digit,
    CharClass::NotDigit,
    CharClass::Whitespace,
    CharClass::NotWhitespace,
    CharClass::Head,
    CharClass::NotHead,
];

/// Map a `CharClass` to its index in `DISJOINT_TABLE`, or `None` if the class
/// is not one of the 8 common classes tracked by the table.
const fn class_table_index(class: CharClass) -> Option<usize> {
    match class {
        CharClass::Word => Some(0),
        CharClass::NotWord => Some(1),
        CharClass::Digit => Some(2),
        CharClass::NotDigit => Some(3),
        CharClass::Whitespace => Some(4),
        CharClass::NotWhitespace => Some(5),
        CharClass::Head => Some(6),
        CharClass::NotHead => Some(7),
        _ => None,
    }
}

/// Precomputed 8x8 disjointness table for the common Vim classes.
///
/// `DISJOINT_TABLE[i][j]` is `true` if `DISJOINT_CLASSES[i]` and
/// `DISJOINT_CLASSES[j]` have no ASCII codepoints in common. This is computed
/// at compile time by testing all 128 ASCII codepoints for each pair.
///
/// Non-ASCII is handled separately: two classes are only truly disjoint if
/// their ASCII sets are disjoint AND at most one of them matches non-ASCII.
/// Since all positive classes (`\w`, `\d`, `\s`, `\h`) are ASCII-only and all
/// negated classes (`\W`, `\D`, `\S`, `\H`) match all non-ASCII, we check:
/// if both classes are negated, they share non-ASCII, so they are NOT disjoint
/// even if their ASCII bitmaps are disjoint. The table accounts for this.
static DISJOINT_TABLE: [[bool; 8]; 8] = compute_disjoint_table();

/// Compute the entire 8x8 table at compile time.
const fn compute_disjoint_table() -> [[bool; 8]; 8] {
    let mut table = [[false; 8]; 8];
    let mut i = 0;
    while i < 8 {
        let mut j = 0;
        while j < 8 {
            table[i][j] = const_classes_disjoint(DISJOINT_CLASSES[i], DISJOINT_CLASSES[j]);
            j += 1;
        }
        i += 1;
    }
    table
}

/// Const-time disjointness check for two `CharClass` values.
///
/// Tests all 128 ASCII codepoints. If any codepoint matches both classes,
/// they are not disjoint. Additionally, if both classes match non-ASCII
/// (i.e. both are negated), they share all non-ASCII codepoints and are
/// not disjoint even if their ASCII bitmaps don't overlap.
const fn const_classes_disjoint(a: CharClass, b: CharClass) -> bool {
    // If both are negated, they both match all non-ASCII chars => not disjoint.
    if const_is_negated(a) && const_is_negated(b) {
        return false;
    }

    // Check ASCII overlap.
    let mut cp = 0u32;
    while cp < 128 {
        let ch = cp as u8 as char;
        if class_matches_ascii(a, ch) && class_matches_ascii(b, ch) {
            return false;
        }
        cp += 1;
    }
    true
}

/// Const-compatible version of `matches_all_non_ascii` (subset for the 8 table classes).
const fn const_is_negated(class: CharClass) -> bool {
    matches!(
        class,
        CharClass::NotWord | CharClass::NotDigit | CharClass::NotWhitespace | CharClass::NotHead
    )
}

/// O(1) disjointness check for two common Vim character classes.
///
/// Returns `Some(true)` if the classes are provably disjoint, `Some(false)` if
/// they provably overlap, or `None` if either class is not in the lookup table
/// (caller should fall back to `CharSet::from_class().is_disjoint()`).
pub(crate) fn classes_are_disjoint(a: CharClass, b: CharClass) -> Option<bool> {
    let i = class_table_index(a)?;
    let j = class_table_index(b)?;
    Some(DISJOINT_TABLE[i][j])
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Helpers ─────────────────────────────────────────────────────────

    /// Check if a specific ASCII char is in the bitmap.
    fn has_ascii(set: &CharSet, ch: char) -> bool {
        let cp = ch as u32;
        assert!(cp < 128, "has_ascii only for ASCII chars");
        let idx = (cp >> 6) as usize;
        let bit = cp & 63;
        (set.ascii[idx] >> bit) & 1 == 1
    }

    /// Count how many ASCII chars are set in the bitmap.
    fn ascii_count(set: &CharSet) -> u32 {
        set.ascii[0].count_ones() + set.ascii[1].count_ones()
    }

    // ─── from_literal() — non-ASCII ─────────────────────────────────────

    #[test]
    fn from_literal_non_ascii() {
        let set = CharSet::from_literal('\u{00E9}'); // e-acute
        assert!(set.non_ascii != NonAsciiPresence::None);
        assert_eq!(ascii_count(&set), 0);
        let ranges = set.unicode_ranges.as_ref().expect("should have ranges");
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0], ('\u{00E9}', '\u{00E9}'));
    }

    #[test]
    fn from_literal_emoji() {
        let set = CharSet::from_literal('\u{1F600}'); // grinning face
        assert!(set.non_ascii != NonAsciiPresence::None);
        assert_eq!(ascii_count(&set), 0);
        let ranges = set.unicode_ranges.as_ref().expect("should have ranges");
        assert_eq!(ranges[0], ('\u{1F600}', '\u{1F600}'));
    }

    // ─── from_any_char() ────────────────────────────────────────────────

    #[test]
    fn any_char_excludes_newline() {
        let set = CharSet::from_any_char();
        assert!(!has_ascii(&set, '\n'));
        // All other ASCII characters should be present.
        assert_eq!(ascii_count(&set), 127); // 128 - 1 (newline)
    }

    #[test]
    fn any_char_includes_common_chars() {
        let set = CharSet::from_any_char();
        assert!(has_ascii(&set, 'a'));
        assert!(has_ascii(&set, 'Z'));
        assert!(has_ascii(&set, '0'));
        assert!(has_ascii(&set, ' '));
        assert!(has_ascii(&set, '\t'));
        assert!(has_ascii(&set, '\r'));
    }

    #[test]
    fn any_char_has_non_ascii() {
        let set = CharSet::from_any_char();
        assert!(set.non_ascii != NonAsciiPresence::None);
    }

    // ─── from_any_char_nl() ─────────────────────────────────────────────

    #[test]
    fn any_char_nl_includes_newline() {
        let set = CharSet::from_any_char_nl();
        assert!(has_ascii(&set, '\n'));
    }

    #[test]
    fn any_char_nl_has_all_ascii() {
        let set = CharSet::from_any_char_nl();
        assert_eq!(ascii_count(&set), 128);
    }

    #[test]
    fn any_char_nl_has_non_ascii() {
        let set = CharSet::from_any_char_nl();
        assert!(set.non_ascii != NonAsciiPresence::None);
    }

    // ─── insert_ascii_range() ───────────────────────────────────────────

    #[test]
    fn insert_ascii_range_digits() {
        let mut set = CharSet::empty();
        set.insert_ascii_range(b'0', b'9');
        assert_eq!(ascii_count(&set), 10);
        for ch in '0'..='9' {
            assert!(has_ascii(&set, ch), "missing digit {ch}");
        }
        assert!(!has_ascii(&set, 'a'));
    }

    #[test]
    fn insert_ascii_range_lowercase() {
        let mut set = CharSet::empty();
        set.insert_ascii_range(b'a', b'z');
        assert_eq!(ascii_count(&set), 26);
        for ch in 'a'..='z' {
            assert!(has_ascii(&set, ch), "missing letter {ch}");
        }
        assert!(!has_ascii(&set, 'A'));
    }

    #[test]
    fn insert_ascii_range_single_char() {
        let mut set = CharSet::empty();
        set.insert_ascii_range(b'X', b'X');
        assert_eq!(ascii_count(&set), 1);
        assert!(has_ascii(&set, 'X'));
    }

    #[test]
    fn insert_ascii_range_full_span() {
        let mut set = CharSet::empty();
        set.insert_ascii_range(0, 127);
        assert_eq!(ascii_count(&set), 128);
    }

    #[test]
    fn insert_ascii_range_multiple_ranges() {
        let mut set = CharSet::empty();
        set.insert_ascii_range(b'a', b'z');
        set.insert_ascii_range(b'A', b'Z');
        set.insert_ascii_range(b'0', b'9');
        assert_eq!(ascii_count(&set), 62); // 26 + 26 + 10
    }

    // ─── insert_char() / remove_char() round-trip ───────────────────────

    #[test]
    fn insert_then_remove_ascii() {
        let mut set = CharSet::empty();
        set.insert_char('a');
        assert!(has_ascii(&set, 'a'));
        set.remove_char('a');
        assert!(!has_ascii(&set, 'a'));
        assert!(set.is_empty());
    }

    #[test]
    fn insert_then_remove_non_ascii() {
        let mut set = CharSet::empty();
        set.insert_char('\u{00E9}');
        assert!(set.non_ascii != NonAsciiPresence::None);
        set.remove_char('\u{00E9}');
        assert_eq!(set.non_ascii, NonAsciiPresence::None);
        assert!(set.is_empty());
    }

    // ─── Unicode range merging ──────────────────────────────────────────

    #[test]
    fn insert_unicode_range_merges_adjacent() {
        let mut set = CharSet::empty();
        set.insert_unicode_range('\u{0100}', '\u{01FF}');
        set.insert_unicode_range('\u{0200}', '\u{02FF}');
        let ranges = set.unicode_ranges.as_ref().expect("should have ranges");
        assert_eq!(ranges.len(), 1, "adjacent ranges should merge");
        assert_eq!(ranges[0], ('\u{0100}', '\u{02FF}'));
    }

    #[test]
    fn insert_unicode_range_merges_overlapping() {
        let mut set = CharSet::empty();
        set.insert_unicode_range('\u{0100}', '\u{01FF}');
        set.insert_unicode_range('\u{0180}', '\u{02FF}');
        let ranges = set.unicode_ranges.as_ref().expect("should have ranges");
        assert_eq!(ranges.len(), 1, "overlapping ranges should merge");
        assert_eq!(ranges[0], ('\u{0100}', '\u{02FF}'));
    }

    #[test]
    fn insert_unicode_range_keeps_disjoint_separate() {
        let mut set = CharSet::empty();
        set.insert_unicode_range('\u{0100}', '\u{01FF}');
        set.insert_unicode_range('\u{0300}', '\u{03FF}');
        let ranges = set.unicode_ranges.as_ref().expect("should have ranges");
        assert_eq!(ranges.len(), 2, "disjoint ranges should stay separate");
        assert_eq!(ranges[0], ('\u{0100}', '\u{01FF}'));
        assert_eq!(ranges[1], ('\u{0300}', '\u{03FF}'));
    }

    // ─── insert_unicode_range() spanning ASCII + non-ASCII ──────────────

    #[test]
    fn insert_unicode_range_spanning_ascii_boundary() {
        let mut set = CharSet::empty();
        // Range from 'z' (0x7A) to e-acute (0xE9) — spans the ASCII/non-ASCII boundary.
        set.insert_unicode_range('z', '\u{00E9}');
        // ASCII portion: z (0x7A) through DEL (0x7F) = 6 chars
        for cp in 0x7Au32..=0x7F {
            assert!(
                has_ascii(&set, char::from_u32(cp).unwrap()),
                "missing ASCII cp {cp:#x}"
            );
        }
        // Non-ASCII portion should be present.
        assert!(set.non_ascii != NonAsciiPresence::None);
        let ranges = set.unicode_ranges.as_ref().expect("should have ranges");
        assert_eq!(ranges[0], ('\u{80}', '\u{00E9}'));
    }

    // ─── matches_all_non_ascii() ──────────────────────────────────────────

    #[test]
    fn matches_all_non_ascii_detection() {
        use super::matches_all_non_ascii;
        // Positive classes do not match all non-ASCII.
        assert!(!matches_all_non_ascii(CharClass::Digit));
        assert!(!matches_all_non_ascii(CharClass::Word));
        assert!(!matches_all_non_ascii(CharClass::Whitespace));
        assert!(!matches_all_non_ascii(CharClass::Alpha));
        assert!(!matches_all_non_ascii(CharClass::Lower));
        assert!(!matches_all_non_ascii(CharClass::Upper));
        assert!(!matches_all_non_ascii(CharClass::Hex));
        assert!(!matches_all_non_ascii(CharClass::Head));
        assert!(!matches_all_non_ascii(CharClass::Octal));
        // Negated classes match all non-ASCII.
        assert!(matches_all_non_ascii(CharClass::NotDigit));
        assert!(matches_all_non_ascii(CharClass::NotWord));
        assert!(matches_all_non_ascii(CharClass::NotWhitespace));
        assert!(matches_all_non_ascii(CharClass::NotAlpha));
        assert!(matches_all_non_ascii(CharClass::NotLower));
        assert!(matches_all_non_ascii(CharClass::NotUpper));
        assert!(matches_all_non_ascii(CharClass::NotHex));
        assert!(matches_all_non_ascii(CharClass::NotHead));
        assert!(matches_all_non_ascii(CharClass::NOctal));
        // Print and SPrint match all non-ASCII.
        assert!(matches_all_non_ascii(CharClass::Print));
        assert!(matches_all_non_ascii(CharClass::SPrint));
    }

    // ─── from_class() — positive classes ────────────────────────────────

    #[test]
    fn from_class_digit() {
        let set = CharSet::from_class(CharClass::Digit, CaseMode::Sensitive);
        assert_eq!(ascii_count(&set), 10);
        for ch in '0'..='9' {
            assert!(has_ascii(&set, ch), "\\d should contain '{ch}'");
        }
        assert!(!has_ascii(&set, 'a'));
        assert!(!has_ascii(&set, 'A'));
        assert_eq!(set.non_ascii, NonAsciiPresence::None, "\\d is ASCII-only");
    }

    #[test]
    fn from_class_word() {
        let set = CharSet::from_class(CharClass::Word, CaseMode::Sensitive);
        // [0-9A-Za-z_] = 10 + 26 + 26 + 1 = 63
        assert_eq!(ascii_count(&set), 63);
        assert!(has_ascii(&set, '_'));
        assert!(has_ascii(&set, 'a'));
        assert!(has_ascii(&set, 'Z'));
        assert!(has_ascii(&set, '0'));
        assert!(!has_ascii(&set, ' '));
        assert!(!has_ascii(&set, '-'));
        assert_eq!(set.non_ascii, NonAsciiPresence::None, "\\w is ASCII-only");
    }

    #[test]
    fn from_class_whitespace() {
        let set = CharSet::from_class(CharClass::Whitespace, CaseMode::Sensitive);
        // [ \t] = 2 chars
        assert_eq!(ascii_count(&set), 2);
        assert!(has_ascii(&set, ' '));
        assert!(has_ascii(&set, '\t'));
        assert!(!has_ascii(&set, '\n'));
        assert!(!has_ascii(&set, '\r'));
        assert_eq!(set.non_ascii, NonAsciiPresence::None, "\\s is ASCII-only");
    }

    #[test]
    fn from_class_octal() {
        let set = CharSet::from_class(CharClass::Octal, CaseMode::Sensitive);
        assert_eq!(ascii_count(&set), 8); // 0-7
        for ch in '0'..='7' {
            assert!(has_ascii(&set, ch));
        }
        assert!(!has_ascii(&set, '8'));
        assert!(!has_ascii(&set, '9'));
    }

    // ─── from_class() — negated classes ─────────────────────────────────

    #[test]
    fn from_class_not_digit_has_non_ascii() {
        let set = CharSet::from_class(CharClass::NotDigit, CaseMode::Sensitive);
        // Everything except 0-9 in ASCII = 128 - 10 = 118
        assert_eq!(ascii_count(&set), 118);
        assert!(!has_ascii(&set, '0'));
        assert!(!has_ascii(&set, '9'));
        assert!(has_ascii(&set, 'a'));
        assert!(has_ascii(&set, ' '));
        assert_eq!(
            set.non_ascii,
            NonAsciiPresence::All,
            "\\D matches non-ASCII"
        );
    }

    #[test]
    fn from_class_not_word_has_non_ascii() {
        let set = CharSet::from_class(CharClass::NotWord, CaseMode::Sensitive);
        // Everything except [0-9A-Za-z_] = 128 - 63 = 65
        assert_eq!(ascii_count(&set), 65);
        assert!(!has_ascii(&set, 'a'));
        assert!(!has_ascii(&set, '_'));
        assert!(has_ascii(&set, ' '));
        assert!(has_ascii(&set, '-'));
        assert_eq!(
            set.non_ascii,
            NonAsciiPresence::All,
            "\\W matches non-ASCII"
        );
    }

    #[test]
    fn from_class_not_whitespace_has_non_ascii() {
        let set = CharSet::from_class(CharClass::NotWhitespace, CaseMode::Sensitive);
        // Everything except [ \t] = 128 - 2 = 126
        assert_eq!(ascii_count(&set), 126);
        assert!(!has_ascii(&set, ' '));
        assert!(!has_ascii(&set, '\t'));
        assert!(has_ascii(&set, 'a'));
        assert!(has_ascii(&set, '\n'));
        assert_eq!(
            set.non_ascii,
            NonAsciiPresence::All,
            "\\S matches non-ASCII"
        );
    }

    // ─── from_class() — Print / SPrint ────────────────────────────────────

    #[test]
    fn from_class_print_has_non_ascii() {
        let set = CharSet::from_class(CharClass::Print, CaseMode::Sensitive);
        assert_eq!(
            set.non_ascii,
            NonAsciiPresence::All,
            "Print matches all non-ASCII"
        );
    }

    #[test]
    fn from_class_sprint_has_non_ascii() {
        let set = CharSet::from_class(CharClass::SPrint, CaseMode::Sensitive);
        assert_eq!(
            set.non_ascii,
            NonAsciiPresence::All,
            "SPrint matches all non-ASCII"
        );
    }

    // ─── from_posix_class() ─────────────────────────────────────────────

    #[test]
    fn from_posix_class_digit() {
        let set = CharSet::from_posix_class(PosixClassName::Digit, CaseMode::Sensitive);
        assert_eq!(ascii_count(&set), 10);
        for ch in '0'..='9' {
            assert!(has_ascii(&set, ch));
        }
        assert_eq!(set.non_ascii, NonAsciiPresence::None);
    }

    #[test]
    fn from_posix_class_alpha() {
        let set = CharSet::from_posix_class(PosixClassName::Alpha, CaseMode::Sensitive);
        assert_eq!(ascii_count(&set), 52);
        assert!(has_ascii(&set, 'a'));
        assert!(has_ascii(&set, 'Z'));
        assert!(!has_ascii(&set, '0'));
        assert_eq!(set.non_ascii, NonAsciiPresence::None);
    }

    #[test]
    fn from_posix_class_blank() {
        let set = CharSet::from_posix_class(PosixClassName::Blank, CaseMode::Sensitive);
        assert_eq!(ascii_count(&set), 2);
        assert!(has_ascii(&set, ' '));
        assert!(has_ascii(&set, '\t'));
        assert!(!has_ascii(&set, '\n'));
    }

    #[test]
    fn from_posix_class_xdigit() {
        let set = CharSet::from_posix_class(PosixClassName::Xdigit, CaseMode::Sensitive);
        assert_eq!(ascii_count(&set), 22); // 0-9 + a-f + A-F
    }

    // ─── from_collection() — simple cases ───────────────────────────────

    #[test]
    fn from_collection_single_chars() {
        // [abc]
        let items = vec![
            CollectionItem::Single('a'),
            CollectionItem::Single('b'),
            CollectionItem::Single('c'),
        ];
        let set = CharSet::from_collection(&items, false, false, CaseMode::Sensitive);
        assert_eq!(ascii_count(&set), 3);
        assert!(has_ascii(&set, 'a'));
        assert!(has_ascii(&set, 'b'));
        assert!(has_ascii(&set, 'c'));
        assert!(!has_ascii(&set, 'd'));
        assert_eq!(set.non_ascii, NonAsciiPresence::None);
    }

    #[test]
    fn from_collection_range() {
        // [a-f]
        let items = vec![CollectionItem::Range('a', 'f')];
        let set = CharSet::from_collection(&items, false, false, CaseMode::Sensitive);
        assert_eq!(ascii_count(&set), 6);
        for ch in 'a'..='f' {
            assert!(has_ascii(&set, ch), "[a-f] should contain '{ch}'");
        }
        assert!(!has_ascii(&set, 'g'));
        assert_eq!(set.non_ascii, NonAsciiPresence::None);
    }

    #[test]
    fn from_collection_negated_single() {
        // [^a]
        let items = vec![CollectionItem::Single('a')];
        let set = CharSet::from_collection(&items, true, false, CaseMode::Sensitive);
        // Negated: everything except 'a' and '\n' (negated collections exclude \n).
        // ASCII: 128 - 2 = 126 (minus 'a' and '\n')
        assert!(!has_ascii(&set, 'a'));
        assert!(!has_ascii(&set, '\n'), "[^a] should exclude newline");
        assert!(has_ascii(&set, 'b'));
        assert!(has_ascii(&set, ' '));
        assert_eq!(ascii_count(&set), 126);
        assert!(
            set.non_ascii != NonAsciiPresence::None,
            "[^a] matches non-ASCII"
        );
    }

    #[test]
    fn from_collection_with_class_item() {
        // [\d]
        let items = vec![CollectionItem::Class(CharClass::Digit)];
        let set = CharSet::from_collection(&items, false, false, CaseMode::Sensitive);
        assert_eq!(ascii_count(&set), 10);
        for ch in '0'..='9' {
            assert!(has_ascii(&set, ch));
        }
        assert!(!has_ascii(&set, 'a'));
    }

    #[test]
    fn from_collection_mixed_items() {
        // [a-z\d_]
        let items = vec![
            CollectionItem::Range('a', 'z'),
            CollectionItem::Class(CharClass::Digit),
            CollectionItem::Single('_'),
        ];
        let set = CharSet::from_collection(&items, false, false, CaseMode::Sensitive);
        // 26 + 10 + 1 = 37
        assert_eq!(ascii_count(&set), 37);
        assert!(has_ascii(&set, 'a'));
        assert!(has_ascii(&set, 'z'));
        assert!(has_ascii(&set, '0'));
        assert!(has_ascii(&set, '_'));
        assert!(!has_ascii(&set, 'A'));
    }

    #[test]
    fn from_collection_include_newline() {
        // \_[abc] — always includes \n
        let items = vec![
            CollectionItem::Single('a'),
            CollectionItem::Single('b'),
            CollectionItem::Single('c'),
        ];
        let set = CharSet::from_collection(&items, false, true, CaseMode::Sensitive);
        assert_eq!(ascii_count(&set), 4); // a, b, c, \n
        assert!(has_ascii(&set, 'a'));
        assert!(has_ascii(&set, '\n'));
    }

    #[test]
    fn from_collection_negated_with_include_newline() {
        // \_[^a] — negated but include_newline forces \n in
        let items = vec![CollectionItem::Single('a')];
        let set = CharSet::from_collection(&items, true, true, CaseMode::Sensitive);
        assert!(!has_ascii(&set, 'a'));
        assert!(has_ascii(&set, '\n'), "include_newline should force \\n in");
        assert!(has_ascii(&set, 'b'));
    }

    #[test]
    fn from_collection_with_posix_class() {
        // [[:digit:]]
        let items = vec![CollectionItem::PosixClass(PosixClassName::Digit)];
        let set = CharSet::from_collection(&items, false, false, CaseMode::Sensitive);
        assert_eq!(ascii_count(&set), 10);
        for ch in '0'..='9' {
            assert!(has_ascii(&set, ch));
        }
    }

    #[test]
    fn from_collection_with_explicit_newline_item() {
        // [\n] — explicit newline item in collection
        let items = vec![CollectionItem::Newline];
        let set = CharSet::from_collection(&items, false, false, CaseMode::Sensitive);
        assert_eq!(ascii_count(&set), 1);
        assert!(has_ascii(&set, '\n'));
    }

    // ─── union() ────────────────────────────────────────────────────────

    #[test]
    fn union_disjoint_sets() {
        let digits = CharSet::from_class(CharClass::Digit, CaseMode::Sensitive);
        let alpha = CharSet::from_class(CharClass::Alpha, CaseMode::Sensitive);
        let combined = digits.union(&alpha);
        // 10 digits + 52 letters = 62
        assert_eq!(ascii_count(&combined), 62);
        assert!(has_ascii(&combined, '0'));
        assert!(has_ascii(&combined, 'a'));
        assert!(has_ascii(&combined, 'Z'));
        assert!(!has_ascii(&combined, '_'));
    }

    #[test]
    fn union_overlapping_sets() {
        let word = CharSet::from_class(CharClass::Word, CaseMode::Sensitive);
        let hex = CharSet::from_class(CharClass::Hex, CaseMode::Sensitive);
        let combined = word.union(&hex);
        // Word = [0-9A-Za-z_] (63), Hex = [0-9A-Fa-f] (22) — Hex is a subset of Word
        assert_eq!(ascii_count(&combined), 63);
    }

    #[test]
    fn union_empty_with_set() {
        let empty = CharSet::empty();
        let digits = CharSet::from_class(CharClass::Digit, CaseMode::Sensitive);
        let combined = empty.union(&digits);
        assert_eq!(ascii_count(&combined), 10);
    }

    #[test]
    fn union_preserves_non_ascii() {
        let not_digit = CharSet::from_class(CharClass::NotDigit, CaseMode::Sensitive);
        let digit = CharSet::from_class(CharClass::Digit, CaseMode::Sensitive);
        let combined = not_digit.union(&digit);
        // Everything in ASCII (128), plus non-ASCII from NotDigit
        assert_eq!(ascii_count(&combined), 128);
        assert!(combined.non_ascii != NonAsciiPresence::None);
    }

    #[test]
    fn union_merges_unicode_ranges() {
        let mut a = CharSet::empty();
        a.insert_unicode_range('\u{0100}', '\u{01FF}');
        let mut b = CharSet::empty();
        b.insert_unicode_range('\u{0200}', '\u{02FF}');
        let combined = a.union(&b);
        assert!(combined.non_ascii != NonAsciiPresence::None);
        let ranges = combined
            .unicode_ranges
            .as_ref()
            .expect("should have ranges");
        // Adjacent ranges should merge.
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0], ('\u{0100}', '\u{02FF}'));
    }

    // ─── complement() ───────────────────────────────────────────────────

    #[test]
    fn complement_of_empty_is_full_ascii() {
        let empty = CharSet::empty();
        let comp = empty.complement();
        assert_eq!(ascii_count(&comp), 128);
        assert!(
            comp.non_ascii != NonAsciiPresence::None,
            "complement of empty matches everything"
        );
    }

    #[test]
    fn complement_of_full_is_empty() {
        let full = CharSet::full();
        let comp = full.complement();
        assert_eq!(ascii_count(&comp), 0);
        assert_eq!(
            comp.non_ascii,
            NonAsciiPresence::None,
            "complement of full matches nothing"
        );
    }

    #[test]
    fn complement_of_digits() {
        let digits = CharSet::from_class(CharClass::Digit, CaseMode::Sensitive);
        let comp = digits.complement();
        // 128 - 10 = 118 ASCII chars
        assert_eq!(ascii_count(&comp), 118);
        assert!(!has_ascii(&comp, '0'));
        assert!(!has_ascii(&comp, '9'));
        assert!(has_ascii(&comp, 'a'));
        assert!(has_ascii(&comp, ' '));
        assert!(
            comp.non_ascii != NonAsciiPresence::None,
            "complement of ASCII-only set includes non-ASCII"
        );
    }

    #[test]
    fn complement_involution() {
        // complement(complement(S)) should have the same ASCII bitmap as S.
        let digits = CharSet::from_class(CharClass::Digit, CaseMode::Sensitive);
        let double_comp = digits.complement().complement();
        assert_eq!(double_comp.ascii, digits.ascii);
        // non_ascii should match the original for ASCII-only sets.
        assert_eq!(double_comp.non_ascii, digits.non_ascii);
    }

    #[test]
    fn complement_of_single_char() {
        let a = CharSet::from_literal('a');
        let comp = a.complement();
        assert!(!has_ascii(&comp, 'a'));
        assert!(has_ascii(&comp, 'b'));
        assert!(has_ascii(&comp, '\0'));
        // 128 - 1 = 127
        assert_eq!(ascii_count(&comp), 127);
        assert!(comp.non_ascii != NonAsciiPresence::None);
    }

    // ─── intersection() ─────────────────────────────────────────────

    #[test]
    fn intersection_word_and_digit_is_exactly_digits() {
        let word = CharSet::from_class(CharClass::Word, CaseMode::Sensitive);
        let digit = CharSet::from_class(CharClass::Digit, CaseMode::Sensitive);
        let inter = word.intersection(&digit);
        // \w ∩ \d = exactly the digits
        assert_eq!(ascii_count(&inter), 10);
        for ch in '0'..='9' {
            assert!(has_ascii(&inter, ch), "intersection should contain '{ch}'");
        }
        assert!(!has_ascii(&inter, 'a'));
        assert!(!has_ascii(&inter, '_'));
        assert_eq!(inter.non_ascii, NonAsciiPresence::None);
    }

    #[test]
    fn intersection_digit_and_alpha_is_empty() {
        let digit = CharSet::from_class(CharClass::Digit, CaseMode::Sensitive);
        let alpha = CharSet::from_class(CharClass::Alpha, CaseMode::Sensitive);
        let inter = digit.intersection(&alpha);
        assert!(inter.is_empty(), "\\d ∩ \\a should be empty");
        assert_eq!(ascii_count(&inter), 0);
        assert_eq!(inter.non_ascii, NonAsciiPresence::None);
    }

    #[test]
    fn intersection_with_empty() {
        let digit = CharSet::from_class(CharClass::Digit, CaseMode::Sensitive);
        let empty = CharSet::empty();
        let inter = digit.intersection(&empty);
        assert!(inter.is_empty());
    }

    #[test]
    fn intersection_with_full() {
        let digit = CharSet::from_class(CharClass::Digit, CaseMode::Sensitive);
        let full = CharSet::full();
        let inter = digit.intersection(&full);
        // \d ∩ full = \d (for ASCII part)
        assert_eq!(ascii_count(&inter), 10);
        for ch in '0'..='9' {
            assert!(has_ascii(&inter, ch));
        }
        // \d has no non-ASCII, so intersection has no non-ASCII.
        assert_eq!(inter.non_ascii, NonAsciiPresence::None);
    }

    #[test]
    fn intersection_with_self() {
        let word = CharSet::from_class(CharClass::Word, CaseMode::Sensitive);
        let inter = word.intersection(&word);
        assert_eq!(ascii_count(&inter), 63);
        assert_eq!(inter.ascii, word.ascii);
    }

    #[test]
    fn intersection_unicode_ranges_overlapping() {
        let mut a = CharSet::empty();
        a.insert_unicode_range('\u{0100}', '\u{02FF}');
        let mut b = CharSet::empty();
        b.insert_unicode_range('\u{0200}', '\u{03FF}');
        let inter = a.intersection(&b);
        assert!(inter.non_ascii != NonAsciiPresence::None);
        let ranges = inter.unicode_ranges.as_ref().expect("should have ranges");
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0], ('\u{0200}', '\u{02FF}'));
    }

    #[test]
    fn intersection_unicode_ranges_disjoint() {
        let mut a = CharSet::empty();
        a.insert_unicode_range('\u{0100}', '\u{01FF}');
        let mut b = CharSet::empty();
        b.insert_unicode_range('\u{0300}', '\u{03FF}');
        let inter = a.intersection(&b);
        // Both have non_ascii individually, but their ranges don't overlap.
        // The intersection should still report non_ascii != None (AND of flags),
        // but with an empty range list.
        assert!(inter.non_ascii != NonAsciiPresence::None);
        let ranges = inter.unicode_ranges.as_ref().expect("should have ranges");
        assert!(ranges.is_empty());
    }

    #[test]
    fn intersection_one_wildcard_non_ascii() {
        // \D has non_ascii=All, unicode_ranges=None (wildcard).
        // A precise non-ASCII set intersected with a wildcard should yield
        // the precise set.
        let not_digit = CharSet::from_class(CharClass::NotDigit, CaseMode::Sensitive);
        let mut precise = CharSet::empty();
        precise.insert_unicode_range('\u{0100}', '\u{01FF}');
        let inter = not_digit.intersection(&precise);
        assert!(inter.non_ascii != NonAsciiPresence::None);
        let ranges = inter.unicode_ranges.as_ref().expect("should have ranges");
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0], ('\u{0100}', '\u{01FF}'));
    }

    #[test]
    fn intersection_negated_classes() {
        let not_digit = CharSet::from_class(CharClass::NotDigit, CaseMode::Sensitive);
        let not_word = CharSet::from_class(CharClass::NotWord, CaseMode::Sensitive);
        let inter = not_digit.intersection(&not_word);
        // \D ∩ \W: ASCII part = everything except digits AND everything except word chars
        // = everything except (digits ∪ word chars) = everything except word chars
        // because digits ⊂ word chars. So the ASCII intersection is \W's ASCII bitmap.
        assert_eq!(ascii_count(&inter), 65); // same as \W: 128 - 63
        assert!(!has_ascii(&inter, '0'));
        assert!(!has_ascii(&inter, 'a'));
        assert!(!has_ascii(&inter, '_'));
        assert!(has_ascii(&inter, ' '));
        assert!(has_ascii(&inter, '-'));
        // Both have non-ASCII wildcard, so intersection has non-ASCII wildcard.
        assert!(inter.non_ascii != NonAsciiPresence::None);
    }

    // ─── is_disjoint() ─────────────────────────────────────────────

    #[test]
    fn is_disjoint_digit_and_alpha() {
        let digit = CharSet::from_class(CharClass::Digit, CaseMode::Sensitive);
        let alpha = CharSet::from_class(CharClass::Alpha, CaseMode::Sensitive);
        assert!(digit.is_disjoint(&alpha), "\\d and \\a should be disjoint");
    }

    #[test]
    fn is_disjoint_digit_and_word() {
        let digit = CharSet::from_class(CharClass::Digit, CaseMode::Sensitive);
        let word = CharSet::from_class(CharClass::Word, CaseMode::Sensitive);
        assert!(
            !digit.is_disjoint(&word),
            "\\d and \\w should NOT be disjoint (digits are word chars)"
        );
    }

    #[test]
    fn is_disjoint_word_and_colon() {
        let word = CharSet::from_class(CharClass::Word, CaseMode::Sensitive);
        let colon = CharSet::from_literal(':');
        assert!(word.is_disjoint(&colon), "\\w and ':' should be disjoint");
    }

    #[test]
    fn is_disjoint_whitespace_and_digit() {
        let ws = CharSet::from_class(CharClass::Whitespace, CaseMode::Sensitive);
        let digit = CharSet::from_class(CharClass::Digit, CaseMode::Sensitive);
        assert!(ws.is_disjoint(&digit), "\\s and \\d should be disjoint");
    }

    #[test]
    fn is_disjoint_empty_and_anything() {
        let empty = CharSet::empty();
        let full = CharSet::full();
        let digit = CharSet::from_class(CharClass::Digit, CaseMode::Sensitive);
        let not_digit = CharSet::from_class(CharClass::NotDigit, CaseMode::Sensitive);
        assert!(empty.is_disjoint(&full), "empty is disjoint with full");
        assert!(empty.is_disjoint(&digit), "empty is disjoint with \\d");
        assert!(empty.is_disjoint(&not_digit), "empty is disjoint with \\D");
        assert!(empty.is_disjoint(&empty), "empty is disjoint with empty");
    }

    #[test]
    fn is_disjoint_full_and_non_empty() {
        let full = CharSet::full();
        let digit = CharSet::from_class(CharClass::Digit, CaseMode::Sensitive);
        let alpha = CharSet::from_class(CharClass::Alpha, CaseMode::Sensitive);
        let single = CharSet::from_literal('x');
        assert!(
            !full.is_disjoint(&digit),
            "full should NOT be disjoint with \\d"
        );
        assert!(
            !full.is_disjoint(&alpha),
            "full should NOT be disjoint with \\a"
        );
        assert!(
            !full.is_disjoint(&single),
            "full should NOT be disjoint with a single char"
        );
    }

    #[test]
    fn is_disjoint_negated_vs_positive() {
        // \D and \d are NOT disjoint: \D matches 'a', \d doesn't, but
        // actually they share NO characters. \D = everything except 0-9,
        // \d = exactly 0-9. They ARE disjoint.
        let digit = CharSet::from_class(CharClass::Digit, CaseMode::Sensitive);
        let not_digit = CharSet::from_class(CharClass::NotDigit, CaseMode::Sensitive);
        assert!(
            digit.is_disjoint(&not_digit),
            "\\d and \\D should be disjoint"
        );
    }

    #[test]
    fn is_disjoint_symmetric() {
        let digit = CharSet::from_class(CharClass::Digit, CaseMode::Sensitive);
        let alpha = CharSet::from_class(CharClass::Alpha, CaseMode::Sensitive);
        let word = CharSet::from_class(CharClass::Word, CaseMode::Sensitive);
        // Disjointness should be symmetric.
        assert_eq!(digit.is_disjoint(&alpha), alpha.is_disjoint(&digit));
        assert_eq!(digit.is_disjoint(&word), word.is_disjoint(&digit));
        assert_eq!(alpha.is_disjoint(&word), word.is_disjoint(&alpha));
    }

    #[test]
    fn is_disjoint_unicode_precise_disjoint_ranges() {
        let mut a = CharSet::empty();
        a.insert_unicode_range('\u{0100}', '\u{01FF}');
        let mut b = CharSet::empty();
        b.insert_unicode_range('\u{0300}', '\u{03FF}');
        assert!(
            a.is_disjoint(&b),
            "disjoint Unicode ranges should be disjoint"
        );
    }

    #[test]
    fn is_disjoint_unicode_precise_overlapping_ranges() {
        let mut a = CharSet::empty();
        a.insert_unicode_range('\u{0100}', '\u{02FF}');
        let mut b = CharSet::empty();
        b.insert_unicode_range('\u{0200}', '\u{03FF}');
        assert!(
            !a.is_disjoint(&b),
            "overlapping Unicode ranges should NOT be disjoint"
        );
    }

    #[test]
    fn is_disjoint_one_wildcard_conservative() {
        // \D has non_ascii=All, unicode_ranges=None (wildcard).
        // A precise non-ASCII set should conservatively report NOT disjoint
        // because \D's wildcard could overlap.
        let not_digit = CharSet::from_class(CharClass::NotDigit, CaseMode::Sensitive);
        let mut precise = CharSet::empty();
        precise.insert_unicode_range('\u{0100}', '\u{01FF}');
        assert!(
            !not_digit.is_disjoint(&precise),
            "wildcard non-ASCII should conservatively report NOT disjoint"
        );
    }

    #[test]
    fn is_disjoint_only_one_has_non_ascii() {
        // \d (ASCII-only) vs a pure non-ASCII set — should be disjoint.
        let digit = CharSet::from_class(CharClass::Digit, CaseMode::Sensitive);
        let mut non_ascii = CharSet::empty();
        non_ascii.insert_unicode_range('\u{0100}', '\u{01FF}');
        assert!(
            digit.is_disjoint(&non_ascii),
            "ASCII-only set should be disjoint with pure non-ASCII set"
        );
    }

    #[test]
    fn is_disjoint_both_wildcards_conservative() {
        // Two negated classes both with wildcard non-ASCII — conservative false.
        let not_digit = CharSet::from_class(CharClass::NotDigit, CaseMode::Sensitive);
        let not_alpha = CharSet::from_class(CharClass::NotAlpha, CaseMode::Sensitive);
        assert!(
            !not_digit.is_disjoint(&not_alpha),
            "two wildcard non-ASCII sets should conservatively report NOT disjoint"
        );
    }

    // ─── unicode_ranges_disjoint() helper ───────────────────────────

    #[test]
    fn unicode_ranges_disjoint_helper_empty() {
        assert!(
            super::unicode_ranges_disjoint(&[], &[]),
            "empty ranges are disjoint"
        );
        assert!(
            super::unicode_ranges_disjoint(&[('\u{100}', '\u{1FF}')], &[]),
            "any range vs empty is disjoint"
        );
        assert!(
            super::unicode_ranges_disjoint(&[], &[('\u{100}', '\u{1FF}')]),
            "empty vs any range is disjoint"
        );
    }

    #[test]
    fn unicode_ranges_disjoint_helper_adjacent_not_overlapping() {
        // [0x100, 0x1FF] and [0x200, 0x2FF] are adjacent but disjoint.
        assert!(super::unicode_ranges_disjoint(
            &[('\u{100}', '\u{1FF}')],
            &[('\u{200}', '\u{2FF}')],
        ));
    }

    #[test]
    fn unicode_ranges_disjoint_helper_touching_at_boundary() {
        // [0x100, 0x200] and [0x200, 0x2FF] share 0x200 — NOT disjoint.
        assert!(!super::unicode_ranges_disjoint(
            &[('\u{100}', '\u{200}')],
            &[('\u{200}', '\u{2FF}')],
        ));
    }

    #[test]
    fn unicode_ranges_disjoint_helper_multiple_ranges() {
        let a = [('\u{100}', '\u{1FF}'), ('\u{400}', '\u{4FF}')];
        let b = [('\u{200}', '\u{2FF}'), ('\u{300}', '\u{3FF}')];
        assert!(super::unicode_ranges_disjoint(&a, &b));
    }

    #[test]
    fn unicode_ranges_disjoint_helper_interleaved_overlap() {
        let a = [('\u{100}', '\u{1FF}'), ('\u{300}', '\u{3FF}')];
        let b = [('\u{180}', '\u{280}')]; // overlaps with a[0]
        assert!(!super::unicode_ranges_disjoint(&a, &b));
    }

    // ─── unicode_ranges_intersect() helper ──────────────────────────

    #[test]
    fn unicode_ranges_intersect_helper_empty() {
        assert!(super::unicode_ranges_intersect(&[], &[]).is_empty());
        assert!(super::unicode_ranges_intersect(&[('\u{100}', '\u{1FF}')], &[]).is_empty());
    }

    #[test]
    fn unicode_ranges_intersect_helper_overlapping() {
        let result =
            super::unicode_ranges_intersect(&[('\u{100}', '\u{2FF}')], &[('\u{200}', '\u{3FF}')]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], ('\u{200}', '\u{2FF}'));
    }

    #[test]
    fn unicode_ranges_intersect_helper_contained() {
        // [0x100, 0x3FF] ∩ [0x200, 0x2FF] = [0x200, 0x2FF]
        let result =
            super::unicode_ranges_intersect(&[('\u{100}', '\u{3FF}')], &[('\u{200}', '\u{2FF}')]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], ('\u{200}', '\u{2FF}'));
    }

    #[test]
    fn unicode_ranges_intersect_helper_disjoint() {
        let result =
            super::unicode_ranges_intersect(&[('\u{100}', '\u{1FF}')], &[('\u{300}', '\u{3FF}')]);
        assert!(result.is_empty());
    }

    #[test]
    fn unicode_ranges_intersect_helper_multiple() {
        let a = [('\u{100}', '\u{2FF}'), ('\u{400}', '\u{5FF}')];
        let b = [('\u{200}', '\u{4FF}')];
        let result = super::unicode_ranges_intersect(&a, &b);
        // [0x100,0x2FF] ∩ [0x200,0x4FF] = [0x200,0x2FF]
        // [0x400,0x5FF] ∩ [0x200,0x4FF] = [0x400,0x4FF]
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], ('\u{200}', '\u{2FF}'));
        assert_eq!(result[1], ('\u{400}', '\u{4FF}'));
    }

    // ═══════════════════════════════════════════════════════════════
    // ADVERSARIAL is_disjoint EDGE CASES
    // ═══════════════════════════════════════════════════════════════
    //
    // These tests specifically target edge cases for is_disjoint's
    // conservatism contract: false negatives are OK, false positives
    // would cause incorrect possessification.

    // Case 1: Two sets sharing EXACTLY one character — must return false.
    #[test]
    fn adversarial_disjoint_share_exactly_one_char() {
        // [a-m] and [m-z] share exactly 'm'.
        let mut a = CharSet::empty();
        a.insert_ascii_range(b'a', b'm');
        let mut b = CharSet::empty();
        b.insert_ascii_range(b'm', b'z');
        assert!(
            !a.is_disjoint(&b),
            "sets sharing exactly 'm' must NOT be reported disjoint"
        );
    }

    // Case 2: Adjacent but non-overlapping ASCII ranges — should return true.
    #[test]
    fn adversarial_disjoint_adjacent_ascii_ranges() {
        // [a-m] and [n-z] — adjacent, no overlap.
        let mut a = CharSet::empty();
        a.insert_ascii_range(b'a', b'm');
        let mut b = CharSet::empty();
        b.insert_ascii_range(b'n', b'z');
        assert!(
            a.is_disjoint(&b),
            "[a-m] and [n-z] should be disjoint (adjacent, no overlap)"
        );
    }

    // Case 3: Same non-ASCII char — must return false.
    #[test]
    fn adversarial_disjoint_same_non_ascii_literal() {
        let a = CharSet::from_literal('\u{00E9}'); // e-acute
        let b = CharSet::from_literal('\u{00E9}'); // e-acute
        assert!(
            !a.is_disjoint(&b),
            "two sets both containing e-acute must NOT be disjoint"
        );
    }

    // Case 4: Different non-ASCII chars — should return true.
    #[test]
    fn adversarial_disjoint_different_non_ascii_literals() {
        let a = CharSet::from_literal('\u{00E9}'); // e-acute
        let b = CharSet::from_literal('\u{00F1}'); // n-tilde
        assert!(
            a.is_disjoint(&b),
            "e-acute and n-tilde are different chars, sets should be disjoint"
        );
    }

    // Case 5: \\d vs \\D — complements, should be disjoint.
    #[test]
    fn adversarial_disjoint_digit_vs_not_digit() {
        let d = CharSet::from_class(CharClass::Digit, CaseMode::Sensitive);
        let big_d = CharSet::from_class(CharClass::NotDigit, CaseMode::Sensitive);
        assert!(
            d.is_disjoint(&big_d),
            "\\d and \\D are complements and must be disjoint"
        );
    }

    // Case 6: Empty set vs anything — must always return true.
    #[test]
    fn adversarial_disjoint_empty_vs_everything() {
        let empty = CharSet::empty();
        assert!(empty.is_disjoint(&CharSet::full()), "empty vs full");
        assert!(
            empty.is_disjoint(&CharSet::from_literal('a')),
            "empty vs literal"
        );
        assert!(
            empty.is_disjoint(&CharSet::from_any_char()),
            "empty vs any_char"
        );
        assert!(
            empty.is_disjoint(&CharSet::from_any_char_nl()),
            "empty vs any_char_nl"
        );
        assert!(
            empty.is_disjoint(&CharSet::from_class(
                CharClass::NotDigit,
                CaseMode::Sensitive
            )),
            "empty vs \\D"
        );
        assert!(empty.is_disjoint(&CharSet::empty()), "empty vs empty");

        // Non-ASCII-only set.
        let mut non_ascii = CharSet::empty();
        non_ascii.insert_unicode_range('\u{0100}', '\u{01FF}');
        assert!(empty.is_disjoint(&non_ascii), "empty vs non-ASCII range");
    }

    // Case 7: Full set vs full set — must return false.
    #[test]
    fn adversarial_disjoint_full_vs_full() {
        let a = CharSet::full();
        let b = CharSet::full();
        assert!(!a.is_disjoint(&b), "full vs full must NOT be disjoint");
    }

    // Case 8: AnyChar (`.`) vs literal '\\n' — should return true
    // because AnyChar excludes \\n.
    #[test]
    fn adversarial_disjoint_any_char_vs_newline() {
        let dot = CharSet::from_any_char();
        let nl = CharSet::from_literal('\n');
        assert!(
            dot.is_disjoint(&nl),
            "AnyChar (`.`) excludes \\n, so `.` and `\\n` should be disjoint"
        );
    }

    // Case 9: AnyCharNl (`\\_. `) vs literal '\\n' — must return false
    // because AnyCharNl includes \\n.
    #[test]
    fn adversarial_disjoint_any_char_nl_vs_newline() {
        let dot_nl = CharSet::from_any_char_nl();
        let nl = CharSet::from_literal('\n');
        assert!(
            !dot_nl.is_disjoint(&nl),
            "AnyCharNl includes \\n, so they must NOT be disjoint"
        );
    }

    // Case 10: [^a] vs literal 'a' — should return true.
    // [^a] excludes 'a' (and \\n), so the only char in the right set ('a')
    // is NOT in the left set.
    #[test]
    fn adversarial_disjoint_negated_collection_vs_excluded_char() {
        let items = vec![CollectionItem::Single('a')];
        let not_a = CharSet::from_collection(&items, true, false, CaseMode::Sensitive);
        let a = CharSet::from_literal('a');
        assert!(
            !has_ascii(&not_a, 'a'),
            "sanity: [^a] should not contain 'a'"
        );
        assert!(not_a.is_disjoint(&a), "[^a] and 'a' should be disjoint");
    }

    // Case 11: [^a] vs literal 'b' — must return false.
    // [^a] includes 'b' (everything except 'a' and '\\n').
    #[test]
    fn adversarial_disjoint_negated_collection_vs_included_char() {
        let items = vec![CollectionItem::Single('a')];
        let not_a = CharSet::from_collection(&items, true, false, CaseMode::Sensitive);
        let b = CharSet::from_literal('b');
        assert!(has_ascii(&not_a, 'b'), "sanity: [^a] should contain 'b'");
        assert!(
            !not_a.is_disjoint(&b),
            "[^a] contains 'b', so they must NOT be disjoint"
        );
    }

    // ── Additional adversarial edge cases ─────────────────────────

    // Boundary bit: codepoint 63 is the last bit in ascii[0],
    // codepoint 64 is the first bit in ascii[1]. Test split.
    #[test]
    fn adversarial_disjoint_ascii_bitmap_boundary() {
        let a = CharSet::from_literal('?'); // codepoint 63
        let b = CharSet::from_literal('@'); // codepoint 64
        assert!(
            a.is_disjoint(&b),
            "'?' (cp 63) and '@' (cp 64) are different chars across bitmap boundary"
        );
        // Same char on each side of the boundary — not disjoint with self.
        assert!(
            !a.is_disjoint(&a),
            "a set is never disjoint with itself (non-empty)"
        );
        assert!(
            !b.is_disjoint(&b),
            "a set is never disjoint with itself (non-empty)"
        );
    }

    // Unicode ranges that touch at exactly one endpoint.
    #[test]
    fn adversarial_disjoint_unicode_ranges_share_one_endpoint() {
        let mut a = CharSet::empty();
        a.insert_unicode_range('\u{0100}', '\u{0200}');
        let mut b = CharSet::empty();
        b.insert_unicode_range('\u{0200}', '\u{0300}');
        // They share U+0200 — must NOT be disjoint.
        assert!(
            !a.is_disjoint(&b),
            "Unicode ranges sharing endpoint U+0200 must NOT be disjoint"
        );
    }

    // One off: ranges ending just before the other starts.
    #[test]
    fn adversarial_disjoint_unicode_ranges_one_off() {
        let mut a = CharSet::empty();
        a.insert_unicode_range('\u{0100}', '\u{01FF}');
        let mut b = CharSet::empty();
        b.insert_unicode_range('\u{0200}', '\u{02FF}');
        // U+01FF and U+0200 are adjacent, no overlap.
        assert!(
            a.is_disjoint(&b),
            "Unicode ranges [U+100,U+1FF] and [U+200,U+2FF] are adjacent, should be disjoint"
        );
    }

    // Singleton non-ASCII range vs itself.
    #[test]
    fn adversarial_disjoint_singleton_unicode_vs_self() {
        let a = CharSet::from_literal('\u{1F600}'); // emoji
        let b = CharSet::from_literal('\u{1F600}'); // same emoji
        assert!(
            !a.is_disjoint(&b),
            "same emoji singleton must NOT be disjoint"
        );
    }

    // Mixed: one set has ASCII + non-ASCII, other has only the non-ASCII part.
    #[test]
    fn adversarial_disjoint_mixed_vs_non_ascii_overlap() {
        let mut a = CharSet::empty();
        a.insert_ascii_range(b'a', b'z');
        a.insert_unicode_range('\u{0100}', '\u{01FF}');

        let mut b = CharSet::empty();
        b.insert_unicode_range('\u{0150}', '\u{0250}');
        // ASCII parts are disjoint (b has no ASCII), but Unicode ranges overlap.
        assert!(
            !a.is_disjoint(&b),
            "Unicode ranges overlap even though ASCII parts don't"
        );
    }

    // Mixed: one set has ASCII + non-ASCII, other has only ASCII part.
    #[test]
    fn adversarial_disjoint_mixed_vs_ascii_overlap() {
        let mut a = CharSet::empty();
        a.insert_ascii_range(b'a', b'z');
        a.insert_unicode_range('\u{0100}', '\u{01FF}');

        let b = CharSet::from_literal('m');
        // ASCII overlap on 'm'.
        assert!(
            !a.is_disjoint(&b),
            "ASCII overlap on 'm' must prevent disjoint"
        );
    }

    // \\w vs \\W — complements, should be disjoint.
    #[test]
    fn adversarial_disjoint_word_vs_not_word() {
        let w = CharSet::from_class(CharClass::Word, CaseMode::Sensitive);
        let big_w = CharSet::from_class(CharClass::NotWord, CaseMode::Sensitive);
        assert!(
            w.is_disjoint(&big_w),
            "\\w and \\W are complements and must be disjoint"
        );
    }

    // \\s vs \\S — complements, should be disjoint.
    #[test]
    fn adversarial_disjoint_whitespace_vs_not_whitespace() {
        let s = CharSet::from_class(CharClass::Whitespace, CaseMode::Sensitive);
        let big_s = CharSet::from_class(CharClass::NotWhitespace, CaseMode::Sensitive);
        assert!(
            s.is_disjoint(&big_s),
            "\\s and \\S are complements and must be disjoint"
        );
    }

    // AnyChar vs AnyChar — must NOT be disjoint (they overlap on almost everything).
    #[test]
    fn adversarial_disjoint_any_char_vs_any_char() {
        let a = CharSet::from_any_char();
        let b = CharSet::from_any_char();
        assert!(!a.is_disjoint(&b), ". vs . must NOT be disjoint");
    }

    // AnyChar vs AnyCharNl — must NOT be disjoint (AnyChar is a subset of AnyCharNl).
    #[test]
    fn adversarial_disjoint_any_char_vs_any_char_nl() {
        let a = CharSet::from_any_char();
        let b = CharSet::from_any_char_nl();
        assert!(!a.is_disjoint(&b), ". vs \\_.  must NOT be disjoint");
    }

    // ═══════════════════════════════════════════════════════════════
    // DISJOINTNESS TABLE TESTS
    // ═══════════════════════════════════════════════════════════════

    mod disjoint_table_tests {
        use super::super::*;

        // ── Complement pairs are disjoint ───────────────────────────

        #[test]
        fn complement_pair_word() {
            assert_eq!(
                classes_are_disjoint(CharClass::Word, CharClass::NotWord),
                Some(true),
                "\\w vs \\W should be disjoint (complement pair)"
            );
        }

        #[test]
        fn complement_pair_digit() {
            assert_eq!(
                classes_are_disjoint(CharClass::Digit, CharClass::NotDigit),
                Some(true),
                "\\d vs \\D should be disjoint (complement pair)"
            );
        }

        #[test]
        fn complement_pair_whitespace() {
            assert_eq!(
                classes_are_disjoint(CharClass::Whitespace, CharClass::NotWhitespace),
                Some(true),
                "\\s vs \\S should be disjoint (complement pair)"
            );
        }

        #[test]
        fn complement_pair_head() {
            assert_eq!(
                classes_are_disjoint(CharClass::Head, CharClass::NotHead),
                Some(true),
                "\\h vs \\H should be disjoint (complement pair)"
            );
        }

        // ── Cross-class disjointness (non-complement) ───────────────

        #[test]
        fn d_vs_h_disjoint() {
            assert_eq!(
                classes_are_disjoint(CharClass::Digit, CharClass::Head),
                Some(true),
                "\\d vs \\h should be disjoint (digits vs [A-Za-z_])"
            );
        }

        #[test]
        fn s_vs_d_disjoint() {
            assert_eq!(
                classes_are_disjoint(CharClass::Whitespace, CharClass::Digit),
                Some(true),
                "\\s vs \\d should be disjoint (whitespace vs digits)"
            );
        }

        #[test]
        fn s_vs_h_disjoint() {
            assert_eq!(
                classes_are_disjoint(CharClass::Whitespace, CharClass::Head),
                Some(true),
                "\\s vs \\h should be disjoint (whitespace vs head)"
            );
        }

        #[test]
        fn s_vs_w_disjoint() {
            assert_eq!(
                classes_are_disjoint(CharClass::Whitespace, CharClass::Word),
                Some(true),
                "\\s vs \\w should be disjoint (whitespace vs word)"
            );
        }

        // ── Non-disjoint pairs ──────────────────────────────────────

        #[test]
        fn d_vs_w_not_disjoint() {
            assert_eq!(
                classes_are_disjoint(CharClass::Digit, CharClass::Word),
                Some(false),
                "\\d vs \\w should NOT be disjoint (digits are word chars)"
            );
        }

        #[test]
        fn h_vs_w_not_disjoint() {
            assert_eq!(
                classes_are_disjoint(CharClass::Head, CharClass::Word),
                Some(false),
                "\\h vs \\w should NOT be disjoint (head chars are word chars)"
            );
        }

        #[test]
        fn negated_pairs_not_disjoint() {
            // Two negated classes both match all non-ASCII => never disjoint.
            assert_eq!(
                classes_are_disjoint(CharClass::NotDigit, CharClass::NotWord),
                Some(false),
                "\\D vs \\W: both negated, share non-ASCII"
            );
            assert_eq!(
                classes_are_disjoint(CharClass::NotWhitespace, CharClass::NotHead),
                Some(false),
                "\\S vs \\H: both negated, share non-ASCII"
            );
            assert_eq!(
                classes_are_disjoint(CharClass::NotDigit, CharClass::NotWhitespace),
                Some(false),
                "\\D vs \\S: both negated, share non-ASCII"
            );
        }

        // ── Unknown class returns None ──────────────────────────────

        #[test]
        fn unknown_class_returns_none() {
            // CharClass::Alpha is not in the table.
            assert_eq!(
                classes_are_disjoint(CharClass::Alpha, CharClass::Digit),
                None,
                "\\a is not in the table, should return None"
            );
            assert_eq!(
                classes_are_disjoint(CharClass::Digit, CharClass::Alpha),
                None,
                "\\a is not in the table (reversed), should return None"
            );
            assert_eq!(
                classes_are_disjoint(CharClass::Hex, CharClass::Word),
                None,
                "\\x is not in the table, should return None"
            );
            assert_eq!(
                classes_are_disjoint(CharClass::Octal, CharClass::Octal),
                None,
                "\\o is not in the table, should return None"
            );
        }

        // ── Symmetry: table[i][j] == table[j][i] ───────────────────

        #[test]
        fn table_is_symmetric() {
            for i in 0..8 {
                for j in 0..8 {
                    assert_eq!(
                        DISJOINT_TABLE[i][j], DISJOINT_TABLE[j][i],
                        "DISJOINT_TABLE[{i}][{j}] != DISJOINT_TABLE[{j}][{i}] \
                         (classes {:?} vs {:?})",
                        DISJOINT_CLASSES[i], DISJOINT_CLASSES[j],
                    );
                }
            }
        }

        // ── Diagonal: every class overlaps with itself (not disjoint)
        //    EXCEPT the empty intersection corner case, which does not
        //    apply here because all 8 classes are non-empty. ─────────

        #[test]
        fn diagonal_self_overlap() {
            for i in 0..8 {
                assert!(
                    !DISJOINT_TABLE[i][i],
                    "DISJOINT_TABLE[{i}][{i}] should be false \
                     (class {:?} overlaps with itself)",
                    DISJOINT_CLASSES[i],
                );
            }
        }

        // ── Critical validation: every table entry matches CharSet ──

        #[test]
        fn table_matches_charset_is_disjoint() {
            for i in 0..8 {
                for j in 0..8 {
                    let ci = DISJOINT_CLASSES[i];
                    let cj = DISJOINT_CLASSES[j];
                    let set_i = CharSet::from_class(ci, CaseMode::Sensitive);
                    let set_j = CharSet::from_class(cj, CaseMode::Sensitive);
                    let charset_disjoint = set_i.is_disjoint(&set_j);
                    assert_eq!(
                        DISJOINT_TABLE[i][j], charset_disjoint,
                        "DISJOINT_TABLE[{i}][{j}] ({:?} vs {:?}) = {}, \
                         but CharSet::is_disjoint = {}",
                        ci, cj, DISJOINT_TABLE[i][j], charset_disjoint,
                    );
                }
            }
        }

        // ── classes_are_disjoint agrees with table for known pairs ──

        #[test]
        fn lookup_agrees_with_table() {
            for i in 0..8 {
                for j in 0..8 {
                    let ci = DISJOINT_CLASSES[i];
                    let cj = DISJOINT_CLASSES[j];
                    let result = classes_are_disjoint(ci, cj);
                    assert_eq!(
                        result,
                        Some(DISJOINT_TABLE[i][j]),
                        "classes_are_disjoint({:?}, {:?}) should return Some({})",
                        ci,
                        cj,
                        DISJOINT_TABLE[i][j],
                    );
                }
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════
    // NON-ASCII TRI-STATE COMPLEMENT / DISJOINT TESTS
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn complement_of_non_ascii_singleton_still_has_non_ascii() {
        let set = CharSet::from_literal('\u{00E9}');
        assert_eq!(set.non_ascii, NonAsciiPresence::Some);
        let comp = set.complement();
        assert_eq!(
            comp.non_ascii,
            NonAsciiPresence::Some,
            "complement of {{e-acute}} should still have non-ASCII presence (all except e-acute)"
        );
    }

    #[test]
    fn complement_of_all_non_ascii_has_none() {
        // Full set has All non-ASCII. Complement should have None.
        let full = CharSet::full();
        assert_eq!(full.non_ascii, NonAsciiPresence::All);
        let comp = full.complement();
        assert_eq!(comp.non_ascii, NonAsciiPresence::None);
    }

    #[test]
    fn complement_of_ascii_only_has_all_non_ascii() {
        let set = CharSet::from_literal('a');
        assert_eq!(set.non_ascii, NonAsciiPresence::None);
        let comp = set.complement();
        assert_eq!(comp.non_ascii, NonAsciiPresence::All);
    }

    #[test]
    fn negated_non_ascii_collection_not_disjoint_with_non_ascii_literal() {
        use crate::ir::CollectionItem;
        let body = CharSet::from_collection(
            &[CollectionItem::Single('\u{00E9}')],
            true, // negated
            false,
            CaseMode::Sensitive,
        );
        let successor = CharSet::from_literal('\u{00F1}');
        assert!(
            !body.is_disjoint(&successor),
            "[^e-acute] and n-tilde should NOT be disjoint"
        );
    }

    #[test]
    fn disjoint_when_one_side_has_no_non_ascii() {
        // ASCII-only set vs non-ASCII-only set => disjoint
        let ascii = CharSet::from_literal('a');
        let non_ascii = CharSet::from_literal('\u{00E9}');
        assert!(ascii.is_disjoint(&non_ascii));
    }

    #[test]
    fn not_disjoint_when_both_have_all_non_ascii() {
        // Two negated classes both match all non-ASCII => not disjoint
        let set_a = CharSet::from_class(crate::ir::CharClass::NotDigit, CaseMode::Sensitive);
        let set_b = CharSet::from_class(crate::ir::CharClass::NotWord, CaseMode::Sensitive);
        // Both have All non-ASCII, so they overlap there
        assert!(!set_a.is_disjoint(&set_b));
    }
}
