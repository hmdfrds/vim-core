//! Byte-level equivalence classes for DFA transitions.
//!
//! Maps each of the 256 possible byte values to an equivalence class ID (u8).
//! Bytes that produce identical transitions from every DFA state share the
//! same class. Multi-byte UTF-8 codepoints create separate class entries for
//! each byte in their encoding — the DFA navigates UTF-8 sequences as
//! multiple state transitions.

use crate::ir::{CharClass, CollectionItem, PosixClassName};
use crate::matchers::{CharMatcher, Matcher, ZeroWidthMatcher};
use crate::nfa::{Nfa, TransitionKind};

use super::utf8::{
    Utf8Sequences, CONT_MIN, INVALID_MIN, LEAD_2B_MIN, LEAD_3B_MIN, LEAD_4B_MIN, OVERLONG_MIN,
};

// ═══════════════════════════════════════════════════════════════════════════════
// BYTE CLASSES — PUBLIC STRUCT
// ═══════════════════════════════════════════════════════════════════════════════

/// Byte equivalence classes: maps byte -> class_id.
///
/// The transition table is indexed by class_id rather than raw byte value,
/// compressing the alphabet from 256 entries per state to typically 10-30.
#[derive(Clone)]
pub(super) struct ByteClasses {
    /// Direct 256-entry lookup: `classes[byte] = class_id`.
    classes: [u8; 256],
    /// Total number of distinct equivalence classes.
    num_classes: u16,
    /// Bitmask: bit N is set if equivalence class N contains word characters.
    word_class_mask: u128,
    /// Set to true if `saturating_add` saturated during construction,
    /// meaning the true alphabet size exceeds 256 (u8 limit).
    overflowed: bool,
    /// Maps class_id -> first byte that belongs to that class (representative).
    /// Used for O(1) char_matcher testing instead of linear scan.
    class_representative: [u8; 256],
}

impl ByteClasses {
    /// Create byte classes where all bytes map to class 0 (single class).
    pub(super) const fn empty() -> Self {
        Self {
            classes: [0; 256],
            num_classes: 1,
            word_class_mask: 0,
            overflowed: false,
            class_representative: [0; 256], // class 0 -> byte 0
        }
    }

    /// Classify a byte into its equivalence class.
    #[inline(always)]
    pub(super) fn classify(&self, byte: u8) -> u8 {
        self.classes[byte as usize]
    }

    /// Total number of equivalence classes (the DFA alphabet size).
    #[inline(always)]
    pub(super) fn num_classes(&self) -> usize {
        self.num_classes as usize
    }

    /// The stride for the transition table: smallest power of 2 >= num_classes.
    /// Using a power of 2 allows shift-based multiplication for state ordinals.
    #[allow(dead_code, reason = "used by tests and future acceleration")]
    #[inline(always)]
    pub(super) fn stride(&self) -> usize {
        (self.num_classes as usize).next_power_of_two()
    }

    /// Whether the given equivalence class contains word characters (`[a-zA-Z0-9_]`).
    #[inline]
    pub(super) fn is_word_class(&self, class: u8) -> bool {
        (class as u128) < 128 && (self.word_class_mask >> class as u128) & 1 != 0
    }

    /// Get the representative byte for a given class ID.
    /// This is the first byte (lowest value) that maps to this class.
    #[inline(always)]
    pub(super) fn representative(&self, class: u8) -> u8 {
        self.class_representative[class as usize]
    }

    /// Whether the byte class construction overflowed (> 255 distinct classes).
    #[inline]
    pub(super) const fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// Set the class for a specific byte (used during construction).
    #[inline]
    fn set(&mut self, byte: u8, class: u8) {
        self.classes[byte as usize] = class;
    }

    /// Set num_classes after construction.
    fn set_num_classes(&mut self, n: u16) {
        self.num_classes = n;
    }
}

impl core::fmt::Debug for ByteClasses {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ByteClasses")
            .field("num_classes", &self.num_classes)
            .finish()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CONSTRUCTION
// ═══════════════════════════════════════════════════════════════════════════════

impl ByteClasses {
    /// Build byte equivalence classes from an NFA.
    ///
    /// Analyzes all consuming transitions to determine which bytes
    /// must be distinguished. Multi-byte UTF-8 codepoints cause splits
    /// at each individual byte position in their encoding.
    pub(super) fn build(nfa: &Nfa, case_sensitive: bool) -> Self {
        let mut builder = ByteClassBuilder::new();

        // Always distinguish newline (for ^ and $ assertions).
        builder.mark_byte(b'\n');

        for sid in nfa.states() {
            for trans in nfa.transitions(sid) {
                match &trans.kind {
                    TransitionKind::Literal(ch) => {
                        builder.mark_char(*ch);
                        if !case_sensitive && ch.is_ascii() {
                            builder.mark_byte((*ch as u8).to_ascii_lowercase());
                            builder.mark_byte((*ch as u8).to_ascii_uppercase());
                        }
                    }
                    TransitionKind::AnyChar => {
                        builder.mark_byte(b'\n');
                    }
                    TransitionKind::AnyCharNl => {
                        // Matches everything — no additional splits needed.
                    }
                    TransitionKind::Matcher(id) => match nfa.matcher(*id) {
                        Matcher::Char(cm) => {
                            builder.refine_for_char_matcher(cm, case_sensitive);
                        }
                        Matcher::ZeroWidth(
                            ZeroWidthMatcher::WordBoundaryStart | ZeroWidthMatcher::WordBoundaryEnd,
                        ) => {
                            // Word boundary needs word/non-word byte distinction.
                            builder.mark_ascii_range(b'a', b'z');
                            builder.mark_ascii_range(b'A', b'Z');
                            builder.mark_ascii_range(b'0', b'9');
                            builder.mark_byte(b'_');
                        }
                        Matcher::ZeroWidth(_) | Matcher::Lookaround(_) => {}
                    },
                    TransitionKind::Epsilon
                    | TransitionKind::Save(_)
                    | TransitionKind::BackRef(_)
                    | TransitionKind::LastSubstitute => {
                        // Non-consuming transitions: no byte class refinement needed.
                    }
                }
            }
        }

        if case_sensitive {
            builder.build()
        } else {
            builder.build_case_insensitive()
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// BYTE CLASS BUILDER
// ═══════════════════════════════════════════════════════════════════════════════

/// Accumulates byte boundaries and builds the final equivalence class map.
struct ByteClassBuilder {
    /// For each byte boundary[i] = true means "new class starts at byte i".
    boundaries: [bool; 257],
}

impl ByteClassBuilder {
    fn new() -> Self {
        let mut boundaries = [false; 257];
        // Always split at byte 0 (start of first class).
        boundaries[0] = true;
        // Always split at ASCII / continuation byte boundary.
        boundaries[CONT_MIN as usize] = true;

        // UTF-8 structural boundaries for multi-byte support:
        // Invalid overlong lead bytes (0xC0-0xC1) must be separated.
        boundaries[OVERLONG_MIN as usize] = true;
        // Start of valid 2-byte leads.
        boundaries[LEAD_2B_MIN as usize] = true;
        // Start of 3-byte leads.
        boundaries[LEAD_3B_MIN as usize] = true;
        // Start of 4-byte leads.
        boundaries[LEAD_4B_MIN as usize] = true;
        // Invalid bytes (0xF5-0xFF).
        boundaries[INVALID_MIN as usize] = true;

        Self { boundaries }
    }

    /// Mark a single byte as needing its own class interval.
    fn mark_byte(&mut self, byte: u8) {
        self.boundaries[byte as usize] = true;
        if byte < 255 {
            self.boundaries[byte as usize + 1] = true;
        }
    }

    /// Mark a contiguous ASCII byte range [lo, hi] as its own interval.
    fn mark_ascii_range(&mut self, lo: u8, hi: u8) {
        self.boundaries[lo as usize] = true;
        if hi < 255 {
            self.boundaries[hi as usize + 1] = true;
        }
    }

    /// Mark all bytes in a character's UTF-8 encoding.
    /// Each byte of a multi-byte character gets its own class boundary.
    fn mark_char(&mut self, ch: char) {
        if ch.len_utf8() == 1 {
            self.mark_byte(ch as u8);
        } else {
            // For multi-byte chars, use Utf8Sequences to get proper byte ranges.
            let seq = Utf8Sequences::single(ch);
            for range in seq.as_slice() {
                self.mark_byte(range.start);
                if range.end < 255 {
                    self.boundaries[range.end as usize + 1] = true;
                }
            }
        }
    }

    /// Mark character ranges — uses UTF-8 sequence decomposition to register
    /// precise byte boundaries for each position in the multi-byte encoding.
    fn mark_char_range(&mut self, lo: char, hi: char) {
        // For ASCII ranges, mark byte boundaries directly.
        if lo.is_ascii() && hi.is_ascii() {
            self.mark_ascii_range(lo as u8, hi as u8);
            return;
        }
        // Decompose the character range into UTF-8 byte sequences and mark
        // boundaries for each byte position. This ensures the DFA can
        // distinguish between different multi-byte characters.
        self.mark_utf8_sequences(Utf8Sequences::new(lo, hi));
    }

    /// Register byte-class boundaries for a set of UTF-8 sequences.
    /// Each `Utf8Range` in each sequence adds boundaries for its start and end bytes.
    fn mark_utf8_sequences(&mut self, sequences: Utf8Sequences) {
        for seq in sequences {
            for range in seq.as_slice() {
                self.mark_byte(range.start);
                if range.end < 255 {
                    self.boundaries[range.end as usize + 1] = true;
                }
            }
        }
    }

    /// Refine for a CharMatcher.
    fn refine_for_char_matcher(&mut self, cm: &CharMatcher, case_sensitive: bool) {
        match cm {
            CharMatcher::Literal(ch) => {
                self.mark_char(*ch);
                if !case_sensitive && ch.is_ascii() {
                    self.mark_byte((*ch as u8).to_ascii_lowercase());
                    self.mark_byte((*ch as u8).to_ascii_uppercase());
                }
            }
            CharMatcher::AnyChar => {
                self.mark_byte(b'\n');
            }
            CharMatcher::AnyCharNl => {}
            CharMatcher::Collection {
                items,
                include_newline,
                ..
            } => {
                for item in items {
                    self.refine_for_collection_item(item, case_sensitive);
                }
                if *include_newline {
                    self.mark_byte(b'\n');
                }
            }
        }
    }

    fn refine_for_collection_item(&mut self, item: &CollectionItem, case_sensitive: bool) {
        match item {
            CollectionItem::Single(ch) => {
                self.mark_char(*ch);
                if !case_sensitive && ch.is_ascii() {
                    self.mark_byte((*ch as u8).to_ascii_lowercase());
                    self.mark_byte((*ch as u8).to_ascii_uppercase());
                }
            }
            CollectionItem::Range(lo, hi) => {
                self.mark_char_range(*lo, *hi);
                if !case_sensitive && lo.is_ascii() && hi.is_ascii() {
                    self.mark_ascii_range(
                        (*lo as u8).to_ascii_lowercase(),
                        (*hi as u8).to_ascii_lowercase(),
                    );
                    self.mark_ascii_range(
                        (*lo as u8).to_ascii_uppercase(),
                        (*hi as u8).to_ascii_uppercase(),
                    );
                }
            }
            CollectionItem::Class(class) => self.refine_for_char_class(*class),
            CollectionItem::PosixClass(name) => self.refine_for_posix_class(*name),
            CollectionItem::Newline => self.mark_byte(b'\n'),
        }
    }

    fn refine_for_char_class(&mut self, class: CharClass) {
        match class {
            CharClass::Digit | CharClass::NotDigit => {
                self.mark_ascii_range(b'0', b'9');
            }
            CharClass::Word | CharClass::NotWord | CharClass::Keyword | CharClass::Ident => {
                self.mark_ascii_range(b'0', b'9');
                self.mark_ascii_range(b'A', b'Z');
                self.mark_ascii_range(b'a', b'z');
                self.mark_byte(b'_');
            }
            CharClass::Whitespace | CharClass::NotWhitespace => {
                self.mark_byte(b' ');
                self.mark_byte(b'\t');
            }
            CharClass::Alpha | CharClass::NotAlpha => {
                self.mark_ascii_range(b'A', b'Z');
                self.mark_ascii_range(b'a', b'z');
            }
            CharClass::Lower | CharClass::NotLower => {
                self.mark_ascii_range(b'a', b'z');
            }
            CharClass::Upper | CharClass::NotUpper => {
                self.mark_ascii_range(b'A', b'Z');
            }
            CharClass::Hex | CharClass::NotHex => {
                self.mark_ascii_range(b'0', b'9');
                self.mark_ascii_range(b'A', b'F');
                self.mark_ascii_range(b'a', b'f');
            }
            CharClass::Head
            | CharClass::NotHead
            | CharClass::KeywordNoDigit
            | CharClass::SIdent => {
                self.mark_ascii_range(b'A', b'Z');
                self.mark_ascii_range(b'a', b'z');
                self.mark_byte(b'_');
            }
            CharClass::FileName | CharClass::FileNameNoDigit => {
                self.mark_ascii_range(b'0', b'9');
                self.mark_ascii_range(b'A', b'Z');
                self.mark_ascii_range(b'a', b'z');
                self.mark_byte(b'.');
                self.mark_byte(b'_');
                self.mark_byte(b'/');
                self.mark_byte(b'~');
                self.mark_byte(b'-');
            }
            CharClass::Print => {
                self.mark_ascii_range(0x00, 0x1f);
                self.mark_byte(0x7f);
            }
            CharClass::SPrint => {
                self.mark_ascii_range(0x00, 0x1f);
                self.mark_byte(0x7f);
                self.mark_ascii_range(b'0', b'9');
            }
            CharClass::Octal | CharClass::NOctal => {
                self.mark_ascii_range(b'0', b'7');
            }
            CharClass::Composing => {
                // Combining marks are all non-ASCII (>= U+0300).
                // No ASCII byte classes to refine.
            }
        }
    }

    fn refine_for_posix_class(&mut self, name: PosixClassName) {
        match name {
            PosixClassName::Alnum | PosixClassName::Ident | PosixClassName::Keyword => {
                self.mark_ascii_range(b'0', b'9');
                self.mark_ascii_range(b'A', b'Z');
                self.mark_ascii_range(b'a', b'z');
                self.mark_byte(b'_');
            }
            PosixClassName::Alpha => {
                self.mark_ascii_range(b'A', b'Z');
                self.mark_ascii_range(b'a', b'z');
            }
            PosixClassName::Blank => {
                self.mark_byte(b' ');
                self.mark_byte(b'\t');
            }
            PosixClassName::Cntrl => {
                self.mark_ascii_range(0x00, 0x1f);
                self.mark_byte(0x7f);
            }
            PosixClassName::Digit => {
                self.mark_ascii_range(b'0', b'9');
            }
            PosixClassName::Graph | PosixClassName::Print => {
                self.mark_ascii_range(0x00, 0x1f);
                self.mark_byte(b' ');
                self.mark_byte(0x7f);
            }
            PosixClassName::Lower => {
                self.mark_ascii_range(b'a', b'z');
            }
            PosixClassName::Punct => {
                self.mark_ascii_range(b'!', b'/');
                self.mark_ascii_range(b':', b'@');
                self.mark_ascii_range(b'[', b'`');
                self.mark_ascii_range(b'{', b'~');
            }
            PosixClassName::Space => {
                self.mark_ascii_range(0x09, 0x0d);
                self.mark_byte(b' ');
            }
            PosixClassName::Upper => {
                self.mark_ascii_range(b'A', b'Z');
            }
            PosixClassName::Xdigit => {
                self.mark_ascii_range(b'0', b'9');
                self.mark_ascii_range(b'A', b'F');
                self.mark_ascii_range(b'a', b'f');
            }
            PosixClassName::Tab => self.mark_byte(b'\t'),
            PosixClassName::Return => self.mark_byte(b'\r'),
            PosixClassName::Backspace => self.mark_byte(0x08),
            PosixClassName::Escape => self.mark_byte(0x1b),
            PosixClassName::Fname => {
                self.mark_ascii_range(b'0', b'9');
                self.mark_ascii_range(b'A', b'Z');
                self.mark_ascii_range(b'a', b'z');
                self.mark_byte(b'_');
                self.mark_byte(b'/');
                self.mark_byte(b'.');
                self.mark_byte(b'~');
                self.mark_byte(b'-');
            }
        }
    }

    /// Produce the final `ByteClasses` from accumulated boundaries.
    fn build(self) -> ByteClasses {
        let mut classes = ByteClasses::empty();
        let mut current_class: u8 = 0;
        let mut overflowed = false;

        for byte in 0u16..=255 {
            if self.boundaries[byte as usize] && byte > 0 {
                let next = current_class.saturating_add(1);
                if next == current_class {
                    // saturating_add saturated — overflow detected.
                    overflowed = true;
                }
                current_class = next;
            }
            classes.set(byte as u8, current_class);
        }

        classes.set_num_classes(current_class as u16 + 1);
        classes.overflowed = overflowed;

        // Compute class_representative: first byte for each class.
        let mut class_representative = [0u8; 256];
        let mut representative_set = [false; 256];
        for byte in 0u16..=255 {
            let class = classes.classify(byte as u8);
            if !representative_set[class as usize] {
                class_representative[class as usize] = byte as u8;
                representative_set[class as usize] = true;
            }
        }
        classes.class_representative = class_representative;

        // Compute word_class_mask: identify which classes contain word bytes.
        let mut word_class_mask: u128 = 0;
        for byte in 0u8..=127 {
            let ch = byte as char;
            if ch.is_ascii_alphanumeric() || ch == '_' {
                let class = classes.classify(byte);
                word_class_mask |= 1u128 << class as u128;
            }
        }
        classes.word_class_mask = word_class_mask;

        classes
    }

    /// Produce byte classes with case-insensitive merging.
    /// Ensures ASCII letter pairs (a/A, b/B, ...) share the same class.
    fn build_case_insensitive(self) -> ByteClasses {
        let mut classes = ByteClasses::empty();
        let mut current_class: u8 = 0;
        let mut overflowed = false;

        for byte in 0u16..=255 {
            if self.boundaries[byte as usize] && byte > 0 {
                let next_class = current_class.saturating_add(1);
                if next_class == current_class {
                    overflowed = true;
                }
                current_class = next_class;
            }
            classes.set(byte as u8, current_class);
        }

        // CI merging: ensure ASCII letter pairs map to the same class (the lower).
        for byte in 0u8..128 {
            let ch = byte as char;
            if ch.is_ascii_alphabetic() {
                let lo = ch.to_ascii_lowercase() as u8;
                let hi = ch.to_ascii_uppercase() as u8;
                let lo_class = classes.classes[lo as usize];
                let hi_class = classes.classes[hi as usize];
                let target = lo_class.min(hi_class);
                classes.classes[lo as usize] = target;
                classes.classes[hi as usize] = target;
            }
        }

        // Compact class IDs to be contiguous starting from 0.
        let mut used = [false; 256];
        for &c in &classes.classes {
            used[c as usize] = true;
        }
        let mut remap = [0u8; 256];
        let mut next: u8 = 0;
        for (old, &is_used) in used.iter().enumerate() {
            if is_used {
                remap[old] = next;
                let new_next = next.saturating_add(1);
                if new_next == next && old < 255 {
                    overflowed = true;
                }
                next = new_next;
            }
        }
        for c in &mut classes.classes {
            *c = remap[*c as usize];
        }

        classes.set_num_classes(next as u16);
        classes.overflowed = overflowed;

        // Compute class_representative: first byte for each class.
        let mut class_representative = [0u8; 256];
        let mut representative_set = [false; 256];
        for byte in 0u16..=255 {
            let class = classes.classify(byte as u8);
            if !representative_set[class as usize] {
                class_representative[class as usize] = byte as u8;
                representative_set[class as usize] = true;
            }
        }
        classes.class_representative = class_representative;

        // Compute word_class_mask.
        let mut word_class_mask: u128 = 0;
        for byte in 0u8..=127 {
            let ch = byte as char;
            if ch.is_ascii_alphanumeric() || ch == '_' {
                let class = classes.classify(byte);
                word_class_mask |= 1u128 << class as u128;
            }
        }
        classes.word_class_mask = word_class_mask;

        classes
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::lower;
    use crate::ir::VimPatternNode;
    use crate::nfa::builder::NfaBuilder;

    fn build_classes(node: &VimPatternNode, case_sensitive: bool) -> ByteClasses {
        let (lowered, _props) = lower(node);
        let nfa = NfaBuilder::build(&lowered).unwrap();
        ByteClasses::build(&nfa, case_sensitive)
    }

    #[test]
    fn byte_classes_literal_ascii() {
        let classes = build_classes(&VimPatternNode::Literal('a'), true);
        // 'a' must be in its own class, distinct from 'b'.
        assert_ne!(classes.classify(b'a'), classes.classify(b'b'));
        // Other ASCII bytes share classes.
        assert_eq!(classes.classify(b'x'), classes.classify(b'y'));
    }

    #[test]
    fn byte_classes_newline_always_isolated() {
        let classes = build_classes(&VimPatternNode::AnyCharNl, true);
        // Even for AnyCharNl, newline must be in its own class.
        assert_ne!(classes.classify(b'\n'), classes.classify(b'a'));
    }

    #[test]
    fn byte_classes_multibyte_utf8() {
        // 'e-acute' (U+00E9) is encoded as [0xC3, 0xA9].
        let classes = build_classes(&VimPatternNode::Literal('\u{00E9}'), true);
        // Both bytes of the encoding must be distinguished.
        assert_ne!(classes.classify(0xC3), classes.classify(0xC4));
        assert_ne!(classes.classify(0xA9), classes.classify(0xAA));
    }

    #[test]
    fn byte_classes_stride_is_power_of_two() {
        let classes = build_classes(&VimPatternNode::Literal('x'), true);
        let stride = classes.stride();
        assert!(stride.is_power_of_two());
        assert!(stride >= classes.num_classes());
    }

    #[test]
    fn byte_classes_num_bounded() {
        // Even complex patterns should produce reasonable class counts.
        let node = VimPatternNode::Class(CharClass::Word);
        let classes = build_classes(&node, true);
        assert!(classes.num_classes() <= 64);
    }

    #[test]
    fn byte_classes_case_insensitive_merges() {
        let classes = build_classes(&VimPatternNode::Literal('a'), false);
        assert_eq!(classes.classify(b'a'), classes.classify(b'A'));
    }

    #[test]
    fn byte_classes_word_class_mask() {
        let node = VimPatternNode::Class(CharClass::Word);
        let classes = build_classes(&node, true);
        // Word bytes should have their class in the mask.
        assert!(classes.is_word_class(classes.classify(b'a')));
        assert!(classes.is_word_class(classes.classify(b'Z')));
        assert!(classes.is_word_class(classes.classify(b'0')));
        assert!(classes.is_word_class(classes.classify(b'_')));
        // Non-word bytes should NOT.
        assert!(!classes.is_word_class(classes.classify(b' ')));
        assert!(!classes.is_word_class(classes.classify(b'\n')));
    }

    #[test]
    fn byte_classes_max_classes_no_overflow() {
        // Every byte as its own boundary → 256 classes (IDs 0-255).
        // This is the maximum possible without overflow since 256 fits in u8.
        let mut builder = ByteClassBuilder::new();
        for byte in 0u8..=255 {
            builder.mark_byte(byte);
        }
        let classes = builder.build();
        // 256 classes is the max for 256 bytes. No saturation occurs
        // because current_class reaches exactly 255 (incremented 255 times
        // from 0 → 255 is valid).
        assert!(!classes.overflowed());
        assert_eq!(classes.num_classes(), 256);
    }

    #[test]
    fn byte_classes_normal_no_overflow() {
        // A simple pattern with few boundaries should not overflow.
        let mut builder = ByteClassBuilder::new();
        builder.mark_byte(b'a');
        builder.mark_byte(b'z');
        let classes = builder.build();
        assert!(!classes.overflowed());
    }

    #[test]
    fn sprint_rejects_digits_via_dfa() {
        use crate::{MatchContext, VimRegex};
        // \P (SPrint) should NOT match digits
        let re = VimRegex::new(r"\P").unwrap();
        let ctx = MatchContext::simple("5");
        let m = re.find(&ctx).unwrap();
        assert!(
            m.is_none(),
            r"\P should NOT match '5' (SPrint excludes digits)"
        );
        // But should match letters
        let ctx2 = MatchContext::simple("a");
        assert!(re.find(&ctx2).unwrap().is_some(), r"\P should match 'a'");
    }

    #[test]
    fn fname_posix_matches_tilde() {
        use crate::{MatchContext, VimRegex};
        let re = VimRegex::new(r"[[:fname:]]").unwrap();
        let ctx = MatchContext::simple("~");
        assert!(
            re.find(&ctx).unwrap().is_some(),
            "[[:fname:]] should match ~ (consistent with \\f)"
        );
    }
}
