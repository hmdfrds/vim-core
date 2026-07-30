//! Tests for `CharMatcher` — character-consuming matchers.

use crate::ir::{CharClass, CollectionItem};
use crate::matchers::{CharMatcher, MatchContext};

// ═══════════════════════════════════════════════════════════════════════════════
// CHAR MATCHER — LITERAL
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn literal_case_sensitive_by_default() {
    let m = CharMatcher::Literal('a');
    let ctx = MatchContext::simple("A");
    assert_eq!(m.matches("A", 0, &ctx), None);
}

#[test]
fn literal_case_insensitive_via_context() {
    let m = CharMatcher::Literal('a');
    let mut ctx = MatchContext::simple("A");
    ctx.case_sensitive = false;
    assert_eq!(m.matches("A", 0, &ctx), Some(1));
}

#[test]
fn literal_multibyte() {
    let m = CharMatcher::Literal('\u{00FC}'); // ü
    let ctx = MatchContext::simple("\u{00FC}ber");
    assert_eq!(m.matches("\u{00FC}ber", 0, &ctx), Some(2)); // 2 bytes in UTF-8
}

#[test]
fn literal_emoji() {
    let m = CharMatcher::Literal('\u{1F980}'); // crab emoji
    let text = "\u{1F980}rust";
    let ctx = MatchContext::simple(text);
    assert_eq!(m.matches(text, 0, &ctx), Some(4)); // emoji is 4 bytes
}

// ═══════════════════════════════════════════════════════════════════════════════
// CHAR MATCHER — ANY CHAR
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn any_char_does_not_match_newline() {
    let m = CharMatcher::AnyChar;
    let ctx = MatchContext::simple("\n");
    // AnyChar (`.`) never matches newline — use AnyCharNl (`\_.`) for that
    assert_eq!(m.matches("\n", 0, &ctx), None);
}

#[test]
fn any_char_matches_multibyte() {
    let m = CharMatcher::AnyChar;
    let ctx = MatchContext::simple("\u{00E9}"); // e-acute
    assert_eq!(m.matches("\u{00E9}", 0, &ctx), Some(2));
}

// ═══════════════════════════════════════════════════════════════════════════════
// CHAR MATCHER — ANY CHAR NL
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn any_char_nl_matches_newline() {
    let m = CharMatcher::AnyCharNl;
    let ctx = MatchContext::simple("\n");
    assert_eq!(m.matches("\n", 0, &ctx), Some(1));
}

#[test]
fn any_char_nl_matches_regular() {
    let m = CharMatcher::AnyCharNl;
    let ctx = MatchContext::simple("x");
    assert_eq!(m.matches("x", 0, &ctx), Some(1));
}

// ═══════════════════════════════════════════════════════════════════════════════
// CHAR MATCHER — COLLECTION
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn collection_single_items() {
    let m = CharMatcher::Collection {
        negated: false,
        items: vec![
            CollectionItem::Single('a'),
            CollectionItem::Single('b'),
            CollectionItem::Single('c'),
        ],
        include_newline: false,
    };
    let ctx = MatchContext::simple("b");
    assert_eq!(m.matches("b", 0, &ctx), Some(1));
    assert_eq!(m.matches("d", 0, &ctx), None);
}

#[test]
fn collection_range() {
    let m = CharMatcher::Collection {
        negated: false,
        items: vec![CollectionItem::Range('a', 'z')],
        include_newline: false,
    };
    let ctx = MatchContext::simple("m");
    assert_eq!(m.matches("m", 0, &ctx), Some(1));
    assert_eq!(m.matches("A", 0, &ctx), None);
}

#[test]
fn collection_negated() {
    let m = CharMatcher::Collection {
        negated: true,
        items: vec![CollectionItem::Single('a')],
        include_newline: false,
    };
    let ctx = MatchContext::simple("b");
    assert_eq!(m.matches("b", 0, &ctx), Some(1));
    assert_eq!(m.matches("a", 0, &ctx), None);
}

#[test]
fn collection_with_class() {
    let m = CharMatcher::Collection {
        negated: false,
        items: vec![CollectionItem::Class(CharClass::Digit)],
        include_newline: false,
    };
    let ctx = MatchContext::simple("7");
    assert_eq!(m.matches("7", 0, &ctx), Some(1));
    assert_eq!(m.matches("x", 0, &ctx), None);
}

#[test]
fn collection_include_newline() {
    let m = CharMatcher::Collection {
        negated: false,
        items: vec![CollectionItem::Single('a')],
        include_newline: true,
    };
    let ctx = MatchContext::simple("\n");
    assert_eq!(m.matches("\n", 0, &ctx), Some(1));
    assert_eq!(m.matches("a", 0, &ctx), Some(1));
}

#[test]
fn collection_newline_without_include_newline() {
    let m = CharMatcher::Collection {
        negated: false,
        items: vec![CollectionItem::Single('a')],
        include_newline: false,
    };
    let ctx = MatchContext::simple("\n");
    // No explicit CollectionItem::Newline in items and include_newline is false
    // → newline is rejected (Vim default behavior for [a])
    assert_eq!(m.matches("\n", 0, &ctx), None);
}

// ═══════════════════════════════════════════════════════════════════════════════
// CHAR MATCHER — EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn literal_at_end_of_text() {
    let m = CharMatcher::Literal('a');
    let ctx = MatchContext::simple("");
    assert_eq!(m.matches("", 0, &ctx), None);
}

#[test]
fn literal_past_end() {
    let m = CharMatcher::Literal('a');
    let ctx = MatchContext::simple("a");
    assert_eq!(m.matches("a", 1, &ctx), None);
}

#[test]
fn literal_at_middle_position() {
    let m = CharMatcher::Literal('b');
    let ctx = MatchContext::simple("abc");
    assert_eq!(m.matches("abc", 1, &ctx), Some(1));
}

// ═══════════════════════════════════════════════════════════════════════════════
// COLLECTION — NEWLINE ITEM
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn collection_with_newline_item() {
    let m = CharMatcher::Collection {
        negated: false,
        items: vec![CollectionItem::Newline, CollectionItem::Single('a')],
        include_newline: false,
    };
    let ctx = MatchContext::simple("a");
    assert_eq!(m.matches("a", 0, &ctx), Some(1));
    // Explicit CollectionItem::Newline in items allows \n through
    assert_eq!(m.matches("\n", 0, &ctx), Some(1));
}

#[test]
fn collection_negated_does_not_match_excluded_newline() {
    let m = CharMatcher::Collection {
        negated: true,
        items: vec![CollectionItem::Single('a')],
        include_newline: false,
    };
    let ctx = MatchContext::simple("\n");
    // Negated collection with include_newline=false and no explicit
    // CollectionItem::Newline → newline is rejected outright
    assert_eq!(m.matches("\n", 0, &ctx), None);
}

// ═══════════════════════════════════════════════════════════════════════════════
// [\n] EXPLICIT NEWLINE IN COLLECTION
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn collection_explicit_newline_matches_newline() {
    // [\n] — explicit newline in collection should match \n
    let m = CharMatcher::Collection {
        negated: false,
        items: vec![CollectionItem::Newline],
        include_newline: false,
    };
    let ctx = MatchContext::simple("\n");
    assert_eq!(m.matches("\n", 0, &ctx), Some(1));
}

#[test]
fn collection_explicit_newline_with_other_items() {
    // [\na-z] — newline + range, should match both
    let m = CharMatcher::Collection {
        negated: false,
        items: vec![CollectionItem::Newline, CollectionItem::Range('a', 'z')],
        include_newline: false,
    };
    let ctx = MatchContext::simple("\n");
    assert_eq!(m.matches("\n", 0, &ctx), Some(1));
    assert_eq!(m.matches("m", 0, &ctx), Some(1));
    assert_eq!(m.matches("A", 0, &ctx), None);
}

#[test]
fn collection_negated_explicit_newline() {
    // [^\n] — negated: \n is in set, so negated means NO match for \n
    let m = CharMatcher::Collection {
        negated: true,
        items: vec![CollectionItem::Newline],
        include_newline: false,
    };
    let ctx = MatchContext::simple("\n");
    assert_eq!(m.matches("\n", 0, &ctx), None);
    // Other chars should match (they are NOT in the set)
    assert_eq!(m.matches("a", 0, &ctx), Some(1));
}

#[test]
fn collection_no_newline_items_rejects_newline() {
    // [a-z] — no explicit newline, no include_newline: \n rejected
    let m = CharMatcher::Collection {
        negated: false,
        items: vec![CollectionItem::Range('a', 'z')],
        include_newline: false,
    };
    let ctx = MatchContext::simple("\n");
    assert_eq!(m.matches("\n", 0, &ctx), None);
}

// ═══════════════════════════════════════════════════════════════════════════════
// CASE-INSENSITIVE MATCHING INSIDE COLLECTIONS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn collection_case_insensitive_single() {
    // [a] with case_sensitive=false should match 'A'
    let m = CharMatcher::Collection {
        negated: false,
        items: vec![CollectionItem::Single('a')],
        include_newline: false,
    };
    let mut ctx = MatchContext::simple("A");
    ctx.case_sensitive = false;
    assert_eq!(m.matches("A", 0, &ctx), Some(1));
    assert_eq!(m.matches("a", 0, &ctx), Some(1));
}

#[test]
fn collection_case_sensitive_single_no_fold() {
    // [a] with case_sensitive=true should NOT match 'A'
    let m = CharMatcher::Collection {
        negated: false,
        items: vec![CollectionItem::Single('a')],
        include_newline: false,
    };
    let ctx = MatchContext::simple("A");
    assert_eq!(m.matches("A", 0, &ctx), None);
    assert_eq!(m.matches("a", 0, &ctx), Some(1));
}

#[test]
fn collection_case_insensitive_range_upper_matches_lower() {
    // [A-Z] with case_sensitive=false should match lowercase letters
    let m = CharMatcher::Collection {
        negated: false,
        items: vec![CollectionItem::Range('A', 'Z')],
        include_newline: false,
    };
    let mut ctx = MatchContext::simple("a");
    ctx.case_sensitive = false;
    assert_eq!(m.matches("a", 0, &ctx), Some(1));
    assert_eq!(m.matches("z", 0, &ctx), Some(1));
    assert_eq!(m.matches("A", 0, &ctx), Some(1));
    assert_eq!(m.matches("Z", 0, &ctx), Some(1));
}

#[test]
fn collection_case_insensitive_range_lower_matches_upper() {
    // [a-z] with case_sensitive=false should match uppercase letters
    let m = CharMatcher::Collection {
        negated: false,
        items: vec![CollectionItem::Range('a', 'z')],
        include_newline: false,
    };
    let mut ctx = MatchContext::simple("A");
    ctx.case_sensitive = false;
    assert_eq!(m.matches("A", 0, &ctx), Some(1));
    assert_eq!(m.matches("Z", 0, &ctx), Some(1));
}

#[test]
fn collection_case_sensitive_range_no_fold() {
    // [A-Z] with case_sensitive=true should NOT match lowercase
    let m = CharMatcher::Collection {
        negated: false,
        items: vec![CollectionItem::Range('A', 'Z')],
        include_newline: false,
    };
    let ctx = MatchContext::simple("a");
    assert_eq!(m.matches("a", 0, &ctx), None);
    assert_eq!(m.matches("A", 0, &ctx), Some(1));
}

#[test]
fn collection_case_insensitive_negated() {
    // [^a] with case_sensitive=false: 'A' and 'a' are in set, negated => no match
    let m = CharMatcher::Collection {
        negated: true,
        items: vec![CollectionItem::Single('a')],
        include_newline: false,
    };
    let mut ctx = MatchContext::simple("A");
    ctx.case_sensitive = false;
    assert_eq!(m.matches("A", 0, &ctx), None);
    assert_eq!(m.matches("a", 0, &ctx), None);
    // 'b' should match (not in set)
    assert_eq!(m.matches("b", 0, &ctx), Some(1));
}
