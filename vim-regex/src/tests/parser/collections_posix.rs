//! Parser tests for `\z1`–`\z9` error recognition, and for `[=c=]` /
//! `[.c.]` POSIX equivalence-class / collating-element parsing.
//!
//! `\z1`–`\z9` are Vim's syntax-region backreferences and only carry
//! meaning inside `:syntax` definitions. This engine does not implement
//! them, so they must surface as `InvalidEscape` with the digit consumed,
//! rather than being silently swallowed or leaking a stray literal digit
//! into the AST.
//!
//! `[=c=]` and `[.c.]` are the POSIX equivalence-class and collating-
//! element forms. Vim collapses a single-character body to the plain
//! literal `c`; a longer body is not well-formed, so `[` falls through to
//! ordinary collection parsing and the body becomes individual characters.

use crate::ir::{CollectionItem, VimPatternNode, VimRegexErrorKind};
use crate::parser::parse_pattern;
use crate::{MatchContext, VimRegex};

// ═══════════════════════════════════════════════════════════════════════
// \z1–\z9: must produce InvalidEscape (not be silently swallowed)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn z1_produces_invalid_escape_error() {
    let err = VimRegex::new(r"\z1").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::InvalidEscape { ch, .. } => {
            assert_eq!(*ch, 'z');
        }
        other => panic!("expected InvalidEscape, got {other:?}"),
    }
}

#[test]
fn z9_produces_invalid_escape_error() {
    let err = VimRegex::new(r"\z9").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::InvalidEscape { ch, .. } => {
            assert_eq!(*ch, 'z');
        }
        other => panic!("expected InvalidEscape, got {other:?}"),
    }
}

#[test]
fn z2_through_z8_each_produce_invalid_escape() {
    for digit in '2'..='8' {
        let pattern = format!(r"\z{digit}");
        let err = VimRegex::new(&pattern).unwrap_err();
        match &err.kind {
            VimRegexErrorKind::InvalidEscape { ch, .. } => {
                assert_eq!(*ch, 'z', "digit {digit}: expected ch='z'");
            }
            other => panic!("digit {digit}: expected InvalidEscape, got {other:?}"),
        }
    }
}

/// After `\z1` the parser must have consumed the digit so that no phantom
/// literal for the digit leaks into the AST on error recovery.  The cleanest
/// way to check this is to verify that the digit is part of the error span
/// rather than appearing after it — i.e. the span covers the digit position.
#[test]
fn z1_error_span_covers_the_digit() {
    // Pattern "\z1": chars are \=0, z=1, 1=2 (byte offsets match for ASCII).
    let err = VimRegex::new(r"\z1").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::InvalidEscape { span, .. } => {
            // span_from(backslash_pos) is called AFTER advancing past the digit,
            // so the span end must be > 2 (it covers at least the digit at byte 2).
            assert!(span.end > 2, "span should extend past the digit: {span:?}");
        }
        other => panic!("expected InvalidEscape, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// [=c=]: equivalence class — treated as literal `c`
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn equiv_class_in_collection_parses_to_single_char() {
    let result = parse_pattern("[=a=]").expect("should parse");
    assert_eq!(
        result.node,
        VimPatternNode::Collection {
            negated: false,
            items: vec![CollectionItem::Single('a')],
            include_newline: false,
        }
    );
}

#[test]
fn equiv_class_matches_the_literal_char() {
    let re = VimRegex::new("[=a=]").expect("should compile");
    let mut cache = re.create_cache();
    let ctx = MatchContext::simple("a");
    assert!(
        re.is_match_with_cache(&mut cache, &ctx).unwrap(),
        "[=a=] should match 'a'"
    );
}

#[test]
fn equiv_class_does_not_match_other_chars() {
    let re = VimRegex::new("[=a=]").expect("should compile");
    let mut cache = re.create_cache();
    let ctx = MatchContext::simple("b");
    assert!(
        !re.is_match_with_cache(&mut cache, &ctx).unwrap(),
        "[=a=] should not match 'b'"
    );
}

#[test]
fn equiv_class_mixed_with_other_chars_in_collection() {
    // [=a=bc] — 'a' via equiv class, then literals 'b' and 'c'
    let result = parse_pattern("[=a=bc]").expect("should parse");
    assert_eq!(
        result.node,
        VimPatternNode::Collection {
            negated: false,
            items: vec![
                CollectionItem::Single('a'),
                CollectionItem::Single('b'),
                CollectionItem::Single('c'),
            ],
            include_newline: false,
        }
    );
}

#[test]
fn equiv_class_mixed_collection_matches_all_three_chars() {
    let re = VimRegex::new("[=a=bc]").expect("should compile");
    let mut cache = re.create_cache();
    for ch in ['a', 'b', 'c'] {
        let s = ch.to_string();
        let ctx = MatchContext::simple(&s);
        assert!(
            re.is_match_with_cache(&mut cache, &ctx).unwrap(),
            "[=a=bc] should match '{ch}'"
        );
    }
    let ctx = MatchContext::simple("d");
    assert!(
        !re.is_match_with_cache(&mut cache, &ctx).unwrap(),
        "[=a=bc] should not match 'd'"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// [.c.]: collating element — treated as literal `c`
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn collating_element_in_collection_parses_to_single_char() {
    let result = parse_pattern("[.a.]").expect("should parse");
    assert_eq!(
        result.node,
        VimPatternNode::Collection {
            negated: false,
            items: vec![CollectionItem::Single('a')],
            include_newline: false,
        }
    );
}

#[test]
fn collating_element_matches_the_literal_char() {
    let re = VimRegex::new("[.a.]").expect("should compile");
    let mut cache = re.create_cache();
    let ctx = MatchContext::simple("a");
    assert!(
        re.is_match_with_cache(&mut cache, &ctx).unwrap(),
        "[.a.] should match 'a'"
    );
}

#[test]
fn collating_element_does_not_match_other_chars() {
    let re = VimRegex::new("[.a.]").expect("should compile");
    let mut cache = re.create_cache();
    let ctx = MatchContext::simple("b");
    assert!(
        !re.is_match_with_cache(&mut cache, &ctx).unwrap(),
        "[.a.] should not match 'b'"
    );
}

#[test]
fn collating_element_mixed_with_range() {
    // [.a.x-z] — 'a' via collating element, then range x-z
    let result = parse_pattern("[.a.x-z]").expect("should parse");
    assert_eq!(
        result.node,
        VimPatternNode::Collection {
            negated: false,
            items: vec![CollectionItem::Single('a'), CollectionItem::Range('x', 'z'),],
            include_newline: false,
        }
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Fall-through: malformed [= or [. not matching the pattern
// ═══════════════════════════════════════════════════════════════════════

/// `[=abc=]` has more than one char between `=…=` — not a valid equivalence
/// class syntax, so `[` falls through to a literal `[`.  This means the
/// collection parser sees `[`, then `=`, `a`, `b`, `c`, `=`, `]` as
/// individual characters, closed by `]`.
#[test]
fn equiv_class_multi_char_content_falls_through() {
    // [=abc=] — not well-formed for our single-char rule.
    // The `[` is treated as a literal, then `=abc=` are literals too,
    // and the outer `]` ends the collection.
    let result = parse_pattern("[=abc=]");
    // It should not error (it parses as a collection of individual chars),
    // but the exact AST depends on fall-through behaviour. The key property
    // is that parsing does not panic or mis-consume.
    assert!(
        result.is_ok(),
        "multi-char equiv class should not error: {result:?}"
    );
}

/// `[.ab.]` has more than one char between `.…` — not a valid collating
/// element, so `[` falls through to a literal.
#[test]
fn collating_element_multi_char_content_falls_through() {
    let result = parse_pattern("[.ab.]");
    assert!(
        result.is_ok(),
        "multi-char collating element should not error: {result:?}"
    );
}
