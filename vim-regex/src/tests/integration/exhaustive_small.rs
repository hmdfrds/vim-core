//! Exhaustive small-pattern enumeration test.
//!
//! RE2-style: generate all regexes with <=3 atoms over alphabet `{a, b, .}`
//! and operators `{*, \+, \?, \|}`. Test every (pattern, string) pair for
//! all strings up to length 6 over `{a, b}`.
//!
//! This is a brute-force soundness check: if any pattern panics or produces
//! an internally inconsistent result, the test fails.

use crate::engine::VimRegex;
use crate::matchers::MatchContext;

/// The 3 atom symbols.
const ATOMS: &[&str] = &["a", "b", "."];

/// The 4 quantifier suffixes (including empty for "no quantifier").
const QUANTIFIERS: &[&str] = &["", "*", "\\+", "\\?"];

/// Generate all strings of length 0..=max_len over alphabet {a, b}.
fn all_strings(max_len: usize) -> Vec<String> {
    let mut result = vec![String::new()]; // empty string
    let mut current_len = vec![String::new()];

    for _ in 0..max_len {
        let mut next = Vec::new();
        for s in &current_len {
            for ch in &['a', 'b'] {
                let mut extended = s.clone();
                extended.push(*ch);
                next.push(extended);
            }
        }
        result.extend(next.iter().cloned());
        current_len = next;
    }
    result
}

/// Generate all single-atom patterns: atom + optional quantifier.
fn single_atoms() -> Vec<String> {
    let mut patterns = Vec::new();
    for atom in ATOMS {
        for quant in QUANTIFIERS {
            patterns.push(format!("{atom}{quant}"));
        }
    }
    patterns
}

/// Generate all 2-atom patterns: (atom+quant)(atom+quant) and (atom+quant)\|(atom+quant).
fn two_atom_patterns() -> Vec<String> {
    let singles = single_atoms();
    let mut patterns = Vec::new();
    for a in &singles {
        for b in &singles {
            // Concatenation.
            patterns.push(format!("{a}{b}"));
            // Alternation.
            patterns.push(format!("{a}\\|{b}"));
        }
    }
    patterns
}

/// Generate all 3-atom patterns.
fn three_atom_patterns() -> Vec<String> {
    let singles = single_atoms();
    let mut patterns = Vec::new();

    for a in &singles {
        for b in &singles {
            for c in &singles {
                // Concatenation: abc
                patterns.push(format!("{a}{b}{c}"));
                // Alternation: a|b|c
                patterns.push(format!("{a}\\|{b}\\|{c}"));
                // Mixed: ab|c and a|bc
                patterns.push(format!("{a}{b}\\|{c}"));
                patterns.push(format!("{a}\\|{b}{c}"));
            }
        }
    }
    patterns
}

/// Run a single (pattern, input) test case.
///
/// Verifies:
/// 1. No panic during compile or search.
/// 2. find() and is_match() agree.
/// 3. find() and find_all() first match agree.
/// 4. Match ranges are within bounds.
fn check_pattern_input(pattern: &str, input: &str) {
    let regex = match VimRegex::new(pattern) {
        Ok(r) => r,
        Err(_) => return, // Invalid pattern is fine.
    };
    let ctx = MatchContext::simple(input);

    // find and is_match must agree.
    let find_result = regex.find(&ctx);
    let is_match_result = regex.is_match(&ctx);

    if let (Ok(find_opt), Ok(is_match)) = (&find_result, &is_match_result) {
        assert_eq!(
            find_opt.is_some(),
            *is_match,
            "find/is_match disagree: pattern={pattern:?} input={input:?}"
        );

        // Match range must be valid.
        if let Some(m) = find_opt {
            assert!(
                m.range.start <= m.range.end,
                "inverted range: pattern={pattern:?} input={input:?} range={:?}",
                m.range
            );
            assert!(
                m.range.end <= input.len(),
                "range exceeds input: pattern={pattern:?} input={input:?} range={:?}",
                m.range
            );
        }
    }

    // find_all first match must agree with find.
    let find_all_result = regex.find_all(&ctx);
    if let (Ok(find_opt), Ok(all_matches)) = (&find_result, &find_all_result) {
        match (find_opt, all_matches.is_empty()) {
            (Some(m), false) => {
                assert_eq!(
                    m.range, all_matches[0].range,
                    "find/find_all[0] disagree: pattern={pattern:?} input={input:?}"
                );
            }
            (None, true) => {} // Both empty -- ok.
            (Some(_), true) => {
                panic!("find() matched but find_all() empty: pattern={pattern:?} input={input:?}");
            }
            (None, false) => {
                panic!(
                    "find() empty but find_all() has {} matches: pattern={pattern:?} input={input:?}",
                    all_matches.len()
                );
            }
        }

        // find_all must be monotonic.
        for window in all_matches.windows(2) {
            assert!(
                window[0].range.end <= window[1].range.start,
                "find_all overlapping: pattern={pattern:?} input={input:?} {:?} then {:?}",
                window[0].range,
                window[1].range
            );
        }
    }
}

#[test]
fn exhaustive_1_atom() {
    let patterns = single_atoms();
    let strings = all_strings(6);

    for pattern in &patterns {
        for input in &strings {
            check_pattern_input(pattern, input);
        }
    }
}

#[test]
fn exhaustive_2_atoms() {
    let patterns = two_atom_patterns();
    let strings = all_strings(6);

    for pattern in &patterns {
        for input in &strings {
            check_pattern_input(pattern, input);
        }
    }
}

#[test]
fn exhaustive_3_atoms() {
    let patterns = three_atom_patterns();
    let strings = all_strings(4); // Shorter strings for 3-atom (combinatorial explosion).

    for pattern in &patterns {
        for input in &strings {
            check_pattern_input(pattern, input);
        }
    }
}
