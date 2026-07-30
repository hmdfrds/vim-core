//! Aho-Corasick multi-pattern prefilter for literal alternations.

use aho_corasick::{AhoCorasick, MatchKind};
use compact_str::CompactString;

use super::prefilter::Prefilter;
use crate::common::BACKWARD_SCAN_WINDOW;
use crate::hir::LoweredNode;

const AC_THRESHOLD: usize = 4;

pub(crate) fn extract_ac_literals(node: &LoweredNode) -> Option<Vec<CompactString>> {
    let branches = match node {
        LoweredNode::Alternation(branches) => branches,
        _ => return None,
    };
    let mut literals = Vec::with_capacity(branches.len());
    for branch in branches {
        match extract_branch_literal(branch) {
            Some(s) if !s.is_empty() => literals.push(s),
            _ => return None,
        }
    }
    if literals.is_empty() {
        return None;
    }
    Some(literals)
}

fn extract_branch_literal(node: &LoweredNode) -> Option<CompactString> {
    match node {
        LoweredNode::Literal(ch) => Some(CompactString::from(ch.to_string())),
        LoweredNode::LiteralString(s) => Some(s.clone()),
        LoweredNode::Sequence(items) => {
            let mut buf = CompactString::new("");
            for item in items {
                match item {
                    LoweredNode::Literal(ch) => buf.push(*ch),
                    LoweredNode::LiteralString(s) => buf.push_str(s),
                    _ => return None,
                }
            }
            Some(buf)
        }
        LoweredNode::Group { inner, .. } => extract_branch_literal(inner),
        _ => None,
    }
}

pub(crate) fn should_use_ac(literals: &[CompactString]) -> bool {
    if literals.len() < AC_THRESHOLD {
        return false;
    }
    if literals.iter().all(|s| s.len() == 1) {
        return false;
    }
    true
}

/// Internal dispatch between packed (Teddy) and full Aho-Corasick.
#[derive(Debug)]
enum AcKind {
    /// SIMD-accelerated packed multi-pattern searcher (Teddy).
    Packed(aho_corasick::packed::Searcher),
    /// Full Aho-Corasick automaton (NFA or DFA, depending on pattern count).
    Full(AhoCorasick),
}

#[derive(Debug)]
pub(crate) struct AcPrefilter {
    ac: AcKind,
}

impl AcPrefilter {
    pub(crate) fn new(literals: &[CompactString], case_insensitive: bool) -> Option<Self> {
        let patterns: Vec<&[u8]> = literals.iter().map(|s| s.as_bytes()).collect();

        // Try packed::Searcher first (Teddy SIMD, up to 128 patterns).
        // Only for case-sensitive mode -- packed doesn't support case folding.
        if !case_insensitive {
            if let Some(packed) = aho_corasick::packed::Searcher::new(patterns.iter().copied()) {
                return Some(Self {
                    ac: AcKind::Packed(packed),
                });
            }
        }

        // Fall back to full Aho-Corasick automaton.
        let ac = AhoCorasick::builder()
            .match_kind(MatchKind::LeftmostFirst)
            .ascii_case_insensitive(case_insensitive)
            .build(&patterns)
            .ok()?;
        Some(Self {
            ac: AcKind::Full(ac),
        })
    }

    pub(crate) fn find_first(&self, text: &str, start: usize) -> Option<(usize, usize)> {
        let haystack = text.as_bytes().get(start..)?;
        match &self.ac {
            AcKind::Packed(packed) => {
                let mat = packed.find(haystack)?;
                Some((start + mat.start(), start + mat.end()))
            }
            AcKind::Full(ac) => {
                let mat = ac.find(haystack)?;
                Some((start + mat.start(), start + mat.end()))
            }
        }
    }
}

impl Prefilter for AcPrefilter {
    fn find_next(&self, text: &str, start: usize) -> Option<usize> {
        let haystack = text.as_bytes().get(start..)?;
        match &self.ac {
            AcKind::Packed(packed) => packed.find(haystack).map(|m| start + m.start()),
            AcKind::Full(ac) => ac.find(haystack).map(|m| start + m.start()),
        }
    }

    fn find_prev(&self, text: &str, end: usize) -> Option<usize> {
        let limit = end.min(text.len());
        if limit == 0 {
            return None;
        }
        let window_start = limit.saturating_sub(BACKWARD_SCAN_WINDOW);
        let haystack = &text.as_bytes()[window_start..limit];
        let mut last_pos: Option<usize> = None;
        match &self.ac {
            AcKind::Packed(packed) => {
                for mat in packed.find_iter(haystack) {
                    last_pos = Some(window_start + mat.start());
                }
            }
            AcKind::Full(ac) => {
                for mat in ac.find_iter(haystack) {
                    last_pos = Some(window_start + mat.start());
                }
            }
        }
        last_pos
    }

    fn is_fast(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_pure_literal_alternation() {
        let node = LoweredNode::Alternation(vec![
            LoweredNode::LiteralString("foo".into()),
            LoweredNode::LiteralString("bar".into()),
            LoweredNode::LiteralString("baz".into()),
        ]);
        let result = extract_ac_literals(&node);
        assert_eq!(result, Some(vec!["foo".into(), "bar".into(), "baz".into()]));
    }

    #[test]
    fn reject_empty_branch() {
        let node = LoweredNode::Alternation(vec![
            LoweredNode::Sequence(vec![]),
            LoweredNode::LiteralString("foo".into()),
        ]);
        assert_eq!(extract_ac_literals(&node), None);
    }

    #[test]
    fn reject_non_literal_branch() {
        let node = LoweredNode::Alternation(vec![
            LoweredNode::LiteralString("foo".into()),
            LoweredNode::AnyChar,
        ]);
        assert_eq!(extract_ac_literals(&node), None);
    }

    #[test]
    fn threshold_rejects_small_count() {
        let lits: Vec<CompactString> = vec!["foo".into(), "bar".into(), "baz".into()];
        assert!(!should_use_ac(&lits));
    }

    #[test]
    fn threshold_accepts_sufficient_count() {
        let lits: Vec<CompactString> = vec!["foo".into(), "bar".into(), "baz".into(), "qux".into()];
        assert!(should_use_ac(&lits));
    }

    #[test]
    fn threshold_rejects_single_byte() {
        let lits: Vec<CompactString> =
            vec!["a".into(), "b".into(), "c".into(), "d".into(), "e".into()];
        assert!(!should_use_ac(&lits));
    }

    #[test]
    fn ac_prefilter_find_next() {
        let lits: Vec<CompactString> = vec!["foo".into(), "bar".into(), "baz".into(), "qux".into()];
        let pf = AcPrefilter::new(&lits, false).unwrap();
        assert_eq!(pf.find_next("hello foo world", 0), Some(6));
        assert_eq!(pf.find_next("no match here", 0), None);
    }

    #[test]
    fn ac_prefilter_find_prev() {
        let lits: Vec<CompactString> = vec!["foo".into(), "bar".into(), "baz".into(), "qux".into()];
        let pf = AcPrefilter::new(&lits, false).unwrap();
        assert_eq!(pf.find_prev("foo bar baz", 11), Some(8));
    }

    #[test]
    fn ac_case_insensitive() {
        let lits: Vec<CompactString> = vec!["foo".into(), "bar".into(), "baz".into(), "qux".into()];
        let pf = AcPrefilter::new(&lits, true).unwrap();
        assert_eq!(pf.find_next("hello FOO world", 0), Some(6));
    }

    #[test]
    fn ac_leftmost_first_priority() {
        let lits: Vec<CompactString> = vec!["abc".into(), "abcd".into()];
        let pf = AcPrefilter::new(&lits, false).unwrap();
        assert_eq!(pf.find_next("abcd", 0), Some(0));
    }

    #[test]
    fn ac_find_first_end_position() {
        // Verify find_first returns correct (start, end) for prefix-of-each-other patterns
        let lits: Vec<CompactString> = vec!["abc".into(), "abcd".into(), "xy".into(), "xyz".into()];
        let pf = AcPrefilter::new(&lits, false).unwrap();
        // LeftmostFirst: "abc" wins over "abcd" at position 0
        assert_eq!(pf.find_first("abcd xyz", 0), Some((0, 3))); // "abc" not "abcd"
        assert_eq!(pf.find_first("abcd xyz", 1), Some((5, 7))); // "xy" not "xyz"
    }

    #[test]
    fn ac_find_first_no_match() {
        let lits: Vec<CompactString> = vec!["foo".into(), "bar".into(), "baz".into(), "qux".into()];
        let pf = AcPrefilter::new(&lits, false).unwrap();
        assert_eq!(pf.find_first("no match here", 0), None);
    }

    #[test]
    fn ac_case_insensitive_non_ascii_passthrough() {
        // Non-ASCII bytes pass through literally even with CI mode
        let lits: Vec<CompactString> = vec![
            "caf\u{00E9}".into(),  // café
            "na\u{00EF}ve".into(), // naïve
            "bar".into(),
            "qux".into(),
        ];
        let pf = AcPrefilter::new(&lits, true).unwrap();
        // Exact non-ASCII match works
        assert_eq!(pf.find_next("the caf\u{00E9} is nice", 0), Some(4));
        // ASCII part case-folded
        assert_eq!(pf.find_next("the CAF\u{00E9} is nice", 0), Some(4));
        // Wrong non-ASCII byte does NOT match
        assert_eq!(pf.find_next("the cafe is nice", 0), None);
    }

    #[test]
    fn ac_prefilter_uses_packed_for_small_sets() {
        // 4 short literals -- should try packed::Searcher first
        let lits: Vec<CompactString> = vec!["foo".into(), "bar".into(), "baz".into(), "qux".into()];
        let pf = AcPrefilter::new(&lits, false).unwrap();
        // Regardless of whether packed was used internally, correctness holds
        assert_eq!(pf.find_next("hello foo world", 0), Some(6));
        assert_eq!(pf.find_next("hello bar world", 0), Some(6));
        assert!(pf.is_fast());
    }

    #[test]
    fn ac_prefilter_packed_find_prev() {
        // Verify find_prev works correctly with packed backend
        let lits: Vec<CompactString> = vec!["abc".into(), "def".into(), "ghi".into(), "jkl".into()];
        let pf = AcPrefilter::new(&lits, false).unwrap();
        assert_eq!(pf.find_prev("abc def ghi jkl", 15), Some(12));
    }
}
