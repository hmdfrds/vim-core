//! Integration tests for required literal extraction.
//!
//! Tests end-to-end from pattern compilation through search, verifying that:
//! 1. `required_byte` is correctly extracted for various patterns.
//! 2. `suffix_literal_extracted` is correctly extracted.
//! 3. `prefilter_tree` is correctly built.
//! 4. Fast-rejection works in `search_internal`.

use crate::engine::VimRegex;
use crate::matchers::MatchContext;

// =============================================================================
// REQUIRED BYTE EXTRACTION — accesses internal fields, keep manual
// =============================================================================

#[test]
fn required_byte_simple_literal() {
    let re = VimRegex::new("hello").unwrap();
    assert_eq!(re.required_byte, Some((b'o', false)));
}

#[test]
fn required_byte_pattern_ending_with_literal() {
    let re = VimRegex::new(".*world").unwrap();
    assert_eq!(re.required_byte, Some((b'd', false)));
}

#[test]
fn required_byte_pattern_ending_with_any_char() {
    let re = VimRegex::new("hello.").unwrap();
    assert_eq!(re.required_byte, None);
}

#[test]
fn required_byte_pattern_with_trailing_dollar() {
    let re = VimRegex::new("test$").unwrap();
    assert_eq!(re.required_byte, Some((b't', false)));
}

#[test]
fn required_byte_pattern_with_quantifier() {
    let re = VimRegex::new(r"foo\d\+bar").unwrap();
    assert_eq!(re.required_byte, Some((b'r', false)));
}

#[test]
fn required_byte_alternation_same_ending() {
    let re = VimRegex::new(r"foo\|boo").unwrap();
    assert_eq!(re.required_byte, Some((b'o', false)));
}

#[test]
fn required_byte_alternation_different_ending() {
    let re = VimRegex::new(r"foo\|bar").unwrap();
    assert_eq!(re.required_byte, None);
}

// =============================================================================
// SUFFIX LITERAL EXTRACTION — accesses internal fields, keep manual
// =============================================================================

#[test]
fn suffix_literal_simple() {
    let re = VimRegex::new("hello").unwrap();
    assert_eq!(re.suffix_literal_extracted.as_deref(), Some("hello"));
}

#[test]
fn suffix_literal_after_wildcard() {
    let re = VimRegex::new(".*world").unwrap();
    assert_eq!(re.suffix_literal_extracted.as_deref(), Some("world"));
}

#[test]
fn suffix_literal_none_for_wildcard_end() {
    let re = VimRegex::new("hello.*").unwrap();
    assert_eq!(re.suffix_literal_extracted, None);
}

// =============================================================================
// PREFILTER TREE — accesses internal fields, keep manual
// =============================================================================

#[test]
fn prefilter_tree_two_literals() {
    let re = VimRegex::new("foo.*bar").unwrap();
    assert!(
        re.prefilter_tree.is_some(),
        "should build AND tree for foo.*bar"
    );
    let tree = re.prefilter_tree.as_ref().unwrap();
    assert!(tree.is_satisfied("foo something bar"));
    assert!(!tree.is_satisfied("foo something baz"));
}

#[test]
fn prefilter_tree_none_for_single_literal() {
    let re = VimRegex::new("hello").unwrap();
    assert!(re.prefilter_tree.is_none());
}

// =============================================================================
// FAST REJECTION — END-TO-END
// =============================================================================

crate::test_harness::regex_suite!(fast_rejection {
    required_byte_rejects_no_match:        "hello",      "hxllx wxrld"     => ();
    required_byte_does_not_false_reject:   "hello",      "say hello world" => (4, 9);
    prefilter_tree_rejects_missing:        "foo.*bar",   "foo something baz" => ();
    prefilter_tree_does_not_false_reject:  "foo.*bar",   "foo something bar" => (0, 17);
    with_find_all_no_match:                "xyz",        "aaaa bbbb cccc"  => ();
    with_is_match_no_match:                "xyz",        "aaaa bbbb cccc"  => ();
    skipped_for_case_insensitive:          r"\chello",   "HELLO WORLD"     => (0, 5);
});

#[test]
fn fast_rejection_respects_case_sensitive_context() {
    // Uses raw MatchContext with case_sensitive: false — keep manual.
    let re = VimRegex::new("hello").unwrap();
    let ctx = MatchContext {
        text: "HELLO",
        cursor: None,
        visual_range: None,
        case_sensitive: false,
        ignore_composing: false,
        line_resolver: None,
        mark_resolver: None,
        last_substitute: None,
    };
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(&ctx.text[m.range], "HELLO");
}
