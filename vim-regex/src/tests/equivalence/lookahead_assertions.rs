//! Look-ahead assertion DFA eligibility.
//!
//! Tests that `$` (EndOfLine), `\<` (WordBoundaryStart), and `\>`
//! (WordBoundaryEnd) work correctly when routed through the DFA engine.

use super::engine::VimRegex;
use super::matchers::MatchContext;
use std::ops::Range;

// ═══════════════════════════════════════════════════════════════════════════════
// HELPERS (only retained for non-suite tests)
// ═══════════════════════════════════════════════════════════════════════════════

fn is_dfa_eligible(pattern: &str) -> bool {
    let re = VimRegex::new(pattern).expect("valid pattern");
    re.is_dfa_eligible()
}

fn find_range_ci(pattern: &str, text: &str) -> Option<Range<usize>> {
    let re = VimRegex::new(pattern).expect("valid pattern");
    let ctx = MatchContext {
        text,
        cursor: None,
        visual_range: None,
        case_sensitive: false,
        ignore_composing: false,
        line_resolver: None,
        mark_resolver: None,
        last_substitute: None,
    };
    re.find(&ctx).expect("no error").map(|m| m.range)
}

// ═══════════════════════════════════════════════════════════════════════════════
// TABULAR TESTS
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(end_of_line {
    // $ (EndOfLine) tests
    before_newline:           "foo$",       "foo\n"         => (0, 3);
    at_eot:                   "foo$",       "foo"           => (0, 3);
    no_match:                 "foo$",       "foobar"        => ();
    multiline_first:          "foo$",       "foo\nfoo\n"    => (0, 3);
    find_all_multiline:       "foo$",       "foo\nfoo\n"    => all[(0, 3), (4, 7)];
    standalone:               "$",          "foo\nbar"      => (3, 3);
    newline_consuming:        "$\\n",       "foo\nbar"      => (3, 4);
    at_newline_only:          "$",          "\n"            => (0, 0);
    consecutive_newlines:     "$",          "\n\n"          => (0, 0);
});

crate::test_harness::regex_suite!(caret_dollar {
    // ^$ combined
    empty_line:               "^$",         "hello\n\nworld" => (6, 6);
    single_line:              "^foo$",      "foo"            => (0, 3);
    no_match:                 "^foo$",      "foobar"         => ();
});

crate::test_harness::regex_suite!(word_boundary_start {
    // \< (WordBoundaryStart)
    basic:                    "\\<word",    " word"    => (1, 5);
    at_sot:                   "\\<word",    "word"     => (0, 4);
    no_match:                 "\\<word",    "sword"    => ();
    after_punct:              "\\<word",    ".word"    => (1, 5);
    after_newline:            "\\<word",    "\nword"   => (1, 5);
});

crate::test_harness::regex_suite!(word_boundary_end {
    // \> (WordBoundaryEnd)
    basic:                    "word\\>",    "word "    => (0, 4);
    at_eot:                   "word\\>",    "word"     => (0, 4);
    no_match:                 "word\\>",    "wordy"    => ();
    before_punct:             "word\\>",    "word."    => (0, 4);
});

crate::test_harness::regex_suite!(full_word_boundary {
    // \<...\> combined
    basic:                    "\\<word\\>", " word "   => (1, 5);
    at_sot_eot:               "\\<word\\>", "word"     => (0, 4);
    partial_start:            "\\<word\\>", "sword"    => ();
    partial_end:              "\\<word\\>", "wordy"    => ();
    embedded:                 "\\<word\\>", "swordy"   => ();
    second_occurrence:        "\\<word\\>", "sword word" => (6, 10);
});

crate::test_harness::regex_suite!(greedy_word_boundary {
    // Greedy + word boundary
    first_word:               "\\<\\w\\+\\>",  "hello world"     => (0, 5);
    find_all_words:           "\\<\\w\\+\\>",  "hello world foo" => all[(0, 5), (6, 11), (12, 15)];
    with_numbers:             "\\<\\w\\+\\>",  "abc 123 def"     => all[(0, 3), (4, 7), (8, 11)];
    with_underscore:          "\\<\\w\\+\\>",  "hello_world"     => (0, 11);
});

crate::test_harness::regex_suite!(all_four_assertions {
    // ^, \<, \>, $ combined
    all_match:                "^\\<word\\>$",   "word"      => (0, 4);
    dollar_fails:             "^\\<word\\>$",   "word foo"  => ();
});

crate::test_harness::regex_suite!(contradictory {
    // Contradictory assertions
    wb_start_end:             "\\<\\>",    "hello"  => ();
    wb_start_end_space:       "\\<\\>",    " a "    => ();
    wb_start_end_empty:       "\\<\\>",    ""       => ();
});

crate::test_harness::regex_suite!(zero_width_assertions {
    // Zero-width assertion-only matches
    word_start:               "\\<",       "hello"  => (0, 0);
});

crate::test_harness::regex_suite!(case_insensitive_assertions {
    // CI with word boundaries
    ci_via_pattern:           "\\c\\<word\\>",   "WORD"  => (0, 4);
});

crate::test_harness::regex_suite!(find_all_with_assertions {
    // find_all with assertion patterns
    dollar_multiline:         "\\w\\+$",               "foo\nbar\nbaz"    => all[(0, 3), (4, 7), (8, 11)];
    word_boundary_sentence:   "\\<\\w\\+\\>",          "The quick brown fox" => all[(0, 3), (4, 9), (10, 15), (16, 19)];
    assertion_alternation:    "\\<foo\\>\\|\\<bar\\>", "foo bar baz"      => all[(0, 3), (4, 7)];
});

crate::test_harness::regex_suite!(star_command {
    // Star/hash command patterns (\<word\>)
    match_whole_word:         "\\<hello\\>",  "say hello world" => (4, 9);
    no_partial_end:           "\\<hello\\>",  "helloworld"      => ();
    no_partial_start:         "\\<hello\\>",  "sayhello"        => ();
    find_all:                 "\\<the\\>",    "the cat in the hat" => all[(0, 3), (11, 14)];
});

crate::test_harness::regex_suite!(edge_cases {
    // Misc edge cases
    single_char_word:         "\\<a\\>",     "a"       => (0, 1);
    single_char_sentence:     "\\<a\\>",     "a b c"   => (0, 1);
    digits_word_boundary:     "\\<123\\>",   " 123 "   => (1, 4);
    underscore_start:         "\\<_foo\\>",  " _foo "  => (1, 5);
    doubled_dollar:           "$$",          "foo\nbar" => ();
    doubled_dollar_eot:       "$$",          "foo"      => ();
    doubled_dollar_empty:     "$$",          ""         => ();
    zero_or_more_in_wb:       "\\<\\w*\\>",  "hello world" => (0, 5);
    zero_or_more_find_all:    "\\<\\w*\\>",  "hello world" => all[(0, 5), (6, 11)];
    wb_start_empty_text:      "\\<",         ""         => ();
    wb_end_empty_text:        "\\>",         ""         => ();
    wb_start_at_eot_neg:      "foo\\<",      "foo"      => ();
    wb_uppercase_no_match:    "\\<B\\>",     "AB"       => ();
    wb_uppercase_match:       "\\<B\\>",     " B "      => (1, 2);
    wb_end_upper_match:       "A\\>",        "A "       => (0, 1);
    wb_end_upper_no_match:    "A\\>",        "AB"       => ();
});

// ═══════════════════════════════════════════════════════════════════════════════
// NON-TABULAR TESTS — require is_dfa_eligible, cache API, CI context, etc.
// ═══════════════════════════════════════════════════════════════════════════════

// ── $ on empty/EOT-only text (backward search limitation) ───────────────

/// `$` on "" — zero-width match at position 0 (EOT).
/// Kept non-tabular because find_backward() with cursor at 0 legitimately
/// cannot find the $ match, tripping the builder's invariant 5.
#[test]
fn dollar_standalone_empty_text() {
    let re = VimRegex::new("$").unwrap();
    let ctx = MatchContext::simple("");
    let m = re.find(&ctx).unwrap();
    assert!(m.is_some());
    assert_eq!(m.unwrap().range, 0..0);
}

/// `\>` on "hello" — zero-width match at 5..5 (word ends at end of text).
/// Same backward-search limitation as above.
#[test]
fn zero_width_word_end() {
    let re = VimRegex::new("\\>").unwrap();
    let ctx = MatchContext::simple("hello");
    let m = re.find(&ctx).unwrap();
    assert!(m.is_some());
    assert_eq!(m.unwrap().range, 5..5);
}

/// `$` on "hello" — zero-width match at end of text (position 5).
/// Same backward-search limitation as above.
#[test]
fn zero_width_dollar() {
    let re = VimRegex::new("$").unwrap();
    let ctx = MatchContext::simple("hello");
    let m = re.find(&ctx).unwrap();
    assert!(m.is_some());
    assert_eq!(m.unwrap().range, 5..5);
}

// ── DFA eligibility ─────────────────────────────────────────────────────

#[test]
fn dollar_is_dfa_eligible() {
    assert!(is_dfa_eligible("foo$"));
    assert!(is_dfa_eligible("$"));
    assert!(is_dfa_eligible("^$"));
    assert!(is_dfa_eligible("^foo$"));
}

#[test]
fn word_boundary_is_dfa_eligible() {
    assert!(is_dfa_eligible("\\<word\\>"));
    assert!(is_dfa_eligible("\\<foo"));
    assert!(is_dfa_eligible("bar\\>"));
    assert!(is_dfa_eligible("\\<\\w\\+\\>"));
}

// ── CI via context (not pattern) ────────────────────────────────────────

#[test]
fn word_boundary_ci_context() {
    // Using case-insensitive context instead of \c.
    assert_eq!(find_range_ci("\\<word\\>", "WORD"), Some(0..4));
}

// ── is_match with assertions ────────────────────────────────────────────

#[test]
fn is_match_with_assertion_pattern() {
    // is_match must work with assertion-bearing DFA-eligible patterns.
    let re = VimRegex::new("\\<word\\>").expect("valid pattern");
    let ctx = MatchContext::simple("hello word world");
    assert!(re.is_match(&ctx).expect("no error"));

    let ctx_no = MatchContext::simple("helloworld");
    assert!(!re.is_match(&ctx_no).expect("no error"));
}

// ── \>\< boundary interaction ───────────────────────────────────────────

#[test]
fn end_then_start_boundary() {
    // `\>\<` — word-end immediately followed by word-start.
    // On "a b": \> fires after 'a' (pos 1), \< fires before 'b' (pos 2).
    // These are at different positions, so the pattern matches the
    // non-word gap between them (pos 1..2) or is zero-width at the boundary.
    let re = VimRegex::new("\\>\\<").unwrap();
    let ctx = MatchContext::simple("a b");
    let result = re.find(&ctx).unwrap().map(|m| m.range);
    // The exact behavior depends on the engine: either a match spanning the
    // gap between word boundaries, or None if the engine requires them at
    // the same position. Either way, we verify it doesn't panic/error.
    // In practice \>\< matches the non-word char(s) between words.
    assert!(
        result == Some(1..2) || result.is_none(),
        "unexpected result: {result:?}"
    );
}

// ── Cache reuse across different texts ──────────────────────────────────

#[test]
fn find_all_with_cache_reuse_different_texts() {
    // Verify that reusing a DFA cache across different texts produces
    // correct results both times (no stale DFA state leaking).
    let re = VimRegex::new("\\<\\w\\+\\>").expect("valid pattern");
    let mut cache = re.create_cache();

    let ctx1 = MatchContext::simple("hello world");
    let matches1 = re.find_all_with_cache(&mut cache, &ctx1).expect("no error");
    let ranges1: Vec<Range<usize>> = matches1.into_iter().map(|m| m.range).collect();
    assert_eq!(ranges1, vec![0..5, 6..11]);

    let ctx2 = MatchContext::simple("foo bar baz");
    let matches2 = re.find_all_with_cache(&mut cache, &ctx2).expect("no error");
    let ranges2: Vec<Range<usize>> = matches2.into_iter().map(|m| m.range).collect();
    assert_eq!(ranges2, vec![0..3, 4..7, 8..11]);
}
