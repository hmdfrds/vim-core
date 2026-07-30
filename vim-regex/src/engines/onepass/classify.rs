//! Character equivalence classes for the one-pass DFA.
//!
//! Reduces the transition table size by grouping characters that are
//! never distinguished by any NFA transition into the same class.
//!
//! The classifier operates at the character level (not byte level),
//! matching vim-regex NFA semantics. ASCII characters (0..128) get
//! individual class entries for O(1) lookup; non-ASCII characters
//! use binary search over sorted range tables.

use crate::ir::CollectionItem;
use crate::matchers::{CharMatcher, Matcher};
use crate::nfa::{Nfa, TransitionKind};

/// Maximum number of equivalence classes. Exceeding this makes the
/// pattern ineligible for one-pass DFA.
const MAX_CLASSES: u16 = 256;

/// Character classifier for the one-pass DFA.
#[derive(Debug, Clone)]
pub(crate) struct CharClassifier {
    /// ASCII byte -> class ID (fast path). Index 0..128.
    ascii_map: [u16; 128],
    /// Default class for any character not in `ascii_map` or `ranges`.
    default_class: u16,
    /// Sorted non-ASCII char ranges, each mapping to a class ID.
    /// Searched via binary search for non-ASCII characters.
    ranges: Vec<(char, char, u16)>,
    /// Total number of distinct classes.
    class_count: u16,
    /// Stride as power-of-2 exponent (for shift-based indexing into transition table).
    stride2: u32,
}

impl CharClassifier {
    /// Build a classifier from the NFA.
    ///
    /// Returns `None` if the number of equivalence classes exceeds `MAX_CLASSES`.
    ///
    /// The approach: collect all "interesting" ASCII characters from NFA
    /// transitions (literal chars, collection boundaries, character classes).
    /// Each distinct set of interesting chars gets its own class. Non-interesting
    /// chars share the default class. Non-ASCII chars that appear in literals
    /// get individual classes; those in ranges use binary search.
    pub(crate) fn build(nfa: &Nfa) -> Option<Self> {
        let mut builder = ClassBuilder::new();

        for sid in nfa.states() {
            for trans in nfa.transitions(sid) {
                match &trans.kind {
                    TransitionKind::Literal(c) => {
                        builder.mark_char(*c);
                    }
                    TransitionKind::AnyChar => {
                        // AnyChar matches everything except \n.
                        // So \n must be in its own class.
                        builder.mark_char('\n');
                    }
                    TransitionKind::AnyCharNl => {
                        // Matches everything -- no splits needed.
                    }
                    TransitionKind::Matcher(mid) => {
                        if let Matcher::Char(cm) = nfa.matcher(*mid) {
                            builder.mark_char_matcher(cm);
                        }
                    }
                    // Non-consuming transitions produce no classes.
                    TransitionKind::Epsilon
                    | TransitionKind::Save(_)
                    | TransitionKind::BackRef(_)
                    | TransitionKind::LastSubstitute => {}
                }
            }
        }

        builder.finish()
    }

    /// Classify any character.
    #[inline]
    pub(crate) fn char_class(&self, c: char) -> u16 {
        if (c as u32) < 128 {
            self.ascii_map[c as usize]
        } else {
            self.non_ascii_class(c)
        }
    }

    /// Number of equivalence classes.
    #[inline]
    pub(crate) fn class_count(&self) -> u16 {
        self.class_count
    }

    /// Stride as a power-of-2 exponent (for shift-based table indexing).
    #[inline]
    pub(crate) fn stride2(&self) -> u32 {
        self.stride2
    }

    /// Stride (number of entries per state row in the transition table).
    #[inline]
    pub(crate) fn stride(&self) -> usize {
        1 << self.stride2
    }

    fn non_ascii_class(&self, c: char) -> u16 {
        // Binary search over sorted (start, end, class) ranges.
        match self.ranges.binary_search_by(|&(lo, hi, _)| {
            if c < lo {
                std::cmp::Ordering::Greater
            } else if c > hi {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        }) {
            Ok(idx) => self.ranges[idx].2,
            Err(_) => self.default_class,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CLASS BUILDER
// ═══════════════════════════════════════════════════════════════════════════════

/// Collects "interesting" characters from NFA transitions and assigns
/// equivalence classes.
struct ClassBuilder {
    /// Which ASCII chars are "interesting" (distinguish transitions).
    ascii_interesting: [bool; 128],
    /// Non-ASCII characters that need their own class.
    non_ascii_chars: Vec<char>,
}

impl ClassBuilder {
    fn new() -> Self {
        Self {
            ascii_interesting: [false; 128],
            non_ascii_chars: Vec::new(),
        }
    }

    fn mark_char(&mut self, c: char) {
        if (c as u32) < 128 {
            self.ascii_interesting[c as usize] = true;
        } else if !self.non_ascii_chars.contains(&c) {
            self.non_ascii_chars.push(c);
        }
    }

    fn mark_char_matcher(&mut self, cm: &CharMatcher) {
        match cm {
            CharMatcher::Literal(c) => self.mark_char(*c),
            CharMatcher::AnyChar => self.mark_char('\n'),
            CharMatcher::AnyCharNl => {}
            CharMatcher::Collection {
                negated: _,
                items,
                include_newline: _,
            } => {
                for item in items {
                    self.mark_collection_item(item);
                }
                // Newline is always interesting for collections.
                self.mark_char('\n');
            }
        }
    }

    fn mark_collection_item(&mut self, item: &CollectionItem) {
        match item {
            CollectionItem::Single(c) => self.mark_char(*c),
            CollectionItem::Range(lo, hi) => {
                // Mark boundary characters as interesting so the classifier
                // can distinguish in-range from out-of-range.
                self.mark_char(*lo);
                self.mark_char(*hi);
                // For ASCII ranges, mark all chars in the range.
                // This is fine because ASCII ranges are at most 128 chars.
                if (*lo as u32) < 128 && (*hi as u32) < 128 {
                    let lo_byte = *lo as u8;
                    let hi_byte = *hi as u8;
                    for b in lo_byte..=hi_byte {
                        self.ascii_interesting[b as usize] = true;
                    }
                }
            }
            CollectionItem::Class(class) => {
                // Character classes define which ASCII chars are interesting.
                // Mark all ASCII chars that match the class.
                use crate::matchers::class_matches;
                for b in 0u8..128 {
                    let c = b as char;
                    if class_matches(*class, c) {
                        self.ascii_interesting[b as usize] = true;
                    }
                }
            }
            CollectionItem::PosixClass(name) => {
                use crate::matchers::posix_class_matches;
                for b in 0u8..128 {
                    let c = b as char;
                    if posix_class_matches(*name, c) {
                        self.ascii_interesting[b as usize] = true;
                    }
                }
            }
            CollectionItem::Newline => {
                self.mark_char('\n');
            }
        }
    }

    fn finish(self) -> Option<CharClassifier> {
        // Class 0 is always the default (uninteresting) class.
        let mut next_class: u16 = 1;
        let mut ascii_map = [0u16; 128];

        // Assign classes to interesting ASCII characters.
        // Group consecutive interesting chars into the same class if they
        // are all interesting (they share behavior). Each interesting char
        // gets its own class to be safe -- the classifier is conservative.
        for (i, &interesting) in self.ascii_interesting.iter().enumerate() {
            if interesting {
                ascii_map[i] = next_class;
                next_class += 1;
                if next_class > MAX_CLASSES {
                    return None;
                }
            }
            // else: stays at class 0 (default).
        }

        // Assign classes to non-ASCII characters.
        let mut non_ascii_sorted = self.non_ascii_chars;
        non_ascii_sorted.sort_unstable();
        non_ascii_sorted.dedup();

        let mut ranges = Vec::with_capacity(non_ascii_sorted.len());
        for &c in &non_ascii_sorted {
            let class = next_class;
            next_class += 1;
            if next_class > MAX_CLASSES {
                return None;
            }
            ranges.push((c, c, class));
        }

        let class_count = next_class;
        let stride2 = compute_stride2(class_count);

        Some(CharClassifier {
            ascii_map,
            default_class: 0,
            ranges,
            class_count,
            stride2,
        })
    }
}

/// Compute the smallest k such that 2^k >= class_count.
fn compute_stride2(class_count: u16) -> u32 {
    if class_count <= 1 {
        return 0;
    }
    u16::BITS - (class_count - 1).leading_zeros()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_stride2_powers() {
        assert_eq!(compute_stride2(1), 0);
        assert_eq!(compute_stride2(2), 1);
        assert_eq!(compute_stride2(3), 2);
        assert_eq!(compute_stride2(4), 2);
        assert_eq!(compute_stride2(5), 3);
        assert_eq!(compute_stride2(128), 7);
        assert_eq!(compute_stride2(129), 8);
        assert_eq!(compute_stride2(256), 8);
    }

    #[test]
    fn classifier_default_class_for_unknown() {
        // Build a classifier from a simple NFA (just a literal 'a').
        let re = crate::VimRegex::new("a").unwrap();
        let classifier = CharClassifier::build(&re.nfa).unwrap();

        // 'a' should have a non-default class.
        assert_ne!(classifier.char_class('a'), 0);
        // 'z' should have the default class (not interesting).
        assert_eq!(classifier.char_class('z'), 0);
        // Non-ASCII should have the default class.
        assert_eq!(classifier.char_class('\u{1F600}'), 0);
    }

    #[test]
    fn classifier_stride_is_power_of_2() {
        let re = crate::VimRegex::new(r"\d\+").unwrap();
        let classifier = CharClassifier::build(&re.nfa).unwrap();
        let stride = classifier.stride();
        assert!(
            stride.is_power_of_two(),
            "stride={stride} is not power of 2"
        );
        assert!(stride >= classifier.class_count() as usize);
    }

    #[test]
    fn classifier_distinguishes_digit_from_non_digit() {
        let re = crate::VimRegex::new(r"\d").unwrap();
        let classifier = CharClassifier::build(&re.nfa).unwrap();

        // All digits should have non-default classes (they are interesting).
        let digit_class_0 = classifier.char_class('0');
        assert_ne!(digit_class_0, 0, "digits should be interesting");

        // All digits should have non-default classes.
        for c in '0'..='9' {
            assert_ne!(
                classifier.char_class(c),
                0,
                "digit '{c}' should have a non-default class"
            );
        }

        // A letter should have the default class (not interesting for \d).
        let letter_class = classifier.char_class('z');
        assert_eq!(
            letter_class, 0,
            "letters should be in the default class for \\d"
        );
    }
}
