//! Integration tests for start-bitmap and min-length pruning.
//!
//! Tests end-to-end from pattern compilation through search, verifying that:
//! 1. `start_bitmap` is correctly computed.
//! 2. Start bitmap skip accelerates search without false negatives.
//! 3. `min_match_length` is correctly threaded.
//! 4. Min-length pruning triggers early-exit without false negatives.

use crate::engine::VimRegex;
use crate::matchers::MatchContext;
use crate::test_builder::regex;

// ═══════════════════════════════════════════════════════════════════════════════
// START BITMAP — EXTRACTION — accesses internal fields, keep manual
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn start_bitmap_present_for_literal() {
    let re = VimRegex::new("hello").unwrap();
    assert!(
        re.start_bitmap.is_some(),
        "literal pattern should have start_bitmap"
    );
    let bitmap = re.start_bitmap.unwrap();
    // 'h' = 104 -> word 3 (104/32=3), bit 8 (104%32=8)
    assert_ne!(
        bitmap[104 >> 5] & (1 << (104 & 31)),
        0,
        "bitmap should include 'h'"
    );
}

#[test]
fn start_bitmap_none_for_any_char() {
    let re = VimRegex::new(".").unwrap();
    assert!(
        re.start_bitmap.is_none(),
        "AnyChar should have no start_bitmap"
    );
}

#[test]
fn start_bitmap_includes_alternation_bytes() {
    let re = VimRegex::new(r"x\|y\|z").unwrap();
    if let Some(bitmap) = re.start_bitmap {
        assert_ne!(bitmap[b'x' as usize >> 5] & (1 << (b'x' & 31)), 0);
        assert_ne!(bitmap[b'y' as usize >> 5] & (1 << (b'y' & 31)), 0);
        assert_ne!(bitmap[b'z' as usize >> 5] & (1 << (b'z' & 31)), 0);
    }
}

#[test]
fn start_bitmap_collection() {
    let re = VimRegex::new("[abc]").unwrap();
    if let Some(bitmap) = re.start_bitmap {
        assert_ne!(bitmap[b'a' as usize >> 5] & (1 << (b'a' & 31)), 0);
        assert_ne!(bitmap[b'b' as usize >> 5] & (1 << (b'b' & 31)), 0);
        assert_ne!(bitmap[b'c' as usize >> 5] & (1 << (b'c' & 31)), 0);
        // 'd' should NOT be in the bitmap
        assert_eq!(bitmap[b'd' as usize >> 5] & (1 << (b'd' & 31)), 0);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// START BITMAP — SEARCH CORRECTNESS
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(start_bitmap_search {
    does_not_false_reject:          "hello",    "say hello world"        => (4, 9);
    skips_non_matching_positions:   "zzz",      "aaaa bbbb cccc zzz dddd" => (15, 18);
});

#[test]
fn start_bitmap_with_find_all() {
    regex("[xy]")
        .text("axbycxdy")
        .expect_all_matches(&[1..2, 3..4, 5..6, 7..8])
        .run();
}

#[test]
fn start_bitmap_with_is_match() {
    // is_match is auto-checked by invariant 1.
    regex("qwerty")
        .text("aaa qwerty bbb")
        .expect_match(4..10)
        .run();
    regex("qwerty").text("aaa bbb ccc").expect_no_match().run();
}

// ═══════════════════════════════════════════════════════════════════════════════
// MIN MATCH LENGTH — EXTRACTION — accesses internal fields, keep manual
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn min_match_length_literal() {
    let re = VimRegex::new("hello").unwrap();
    assert_eq!(re.min_match_length, 5);
}

#[test]
fn min_match_length_with_quantifier() {
    let re = VimRegex::new(r"\d\{3,5}").unwrap();
    assert_eq!(re.min_match_length, 3);
}

#[test]
fn min_match_length_optional() {
    let re = VimRegex::new(r"a\?b").unwrap();
    // 'a' is optional (min 0), 'b' is required (min 1)
    assert_eq!(re.min_match_length, 1);
}

#[test]
fn min_match_length_alternation() {
    let re = VimRegex::new(r"ab\|c").unwrap();
    // Alternation min = min of branches: min("ab"=2, "c"=1) = 1
    assert_eq!(re.min_match_length, 1);
}

#[test]
fn min_match_length_star() {
    let re = VimRegex::new("a*").unwrap();
    // a* can match empty
    assert_eq!(re.min_match_length, 0);
}

// ═══════════════════════════════════════════════════════════════════════════════
// MIN MATCH LENGTH — EARLY EXIT
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(min_match_early_exit {
    short_text:            "hello",    "hell"  => ();
    does_not_false_reject: "hello",    "hello" => (0, 5);
    with_find_all:         "hello",    "hel"   => ();
    with_is_match:         "hello",    "hell"  => ();
});

#[test]
fn min_match_length_early_exit_from_offset() {
    // Uses find_at with offset — keep manual.
    let re = VimRegex::new("hello").unwrap();
    let ctx = MatchContext::simple("xxhello");
    // Search from offset 3 -> remaining = "ello" = 4 bytes < 5
    let m = re.find_at(&ctx, 3).unwrap();
    assert!(m.is_none());
}

#[test]
fn min_match_length_zero_allows_empty_match() {
    // a* matches empty string at position 0.
    regex("a*").text("bbb").expect_match(0..0).run();
}

#[test]
fn min_match_length_with_multibyte() {
    let re = VimRegex::new(r"\w\+").unwrap();
    assert!(re.min_match_length >= 1);
    regex(r"\w\+").text("x").expect_match(0..1).run();
}
