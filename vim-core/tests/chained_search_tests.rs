//! Integration tests for chained search (`/foo/;?bar`).
//!
//! Vim supports semicolon-chained searches where each segment starts from
//! the position found by the previous segment.  These tests exercise the
//! full engine pipeline to verify cursor positioning.

use vim_test::prelude::*;

// ═══════════════════════════════════════════════════════════════════════════════
// BASIC CHAINING
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn chained_forward_then_backward() {
    // /bar/;?foo — find "bar" (at offset 10), then backward for "foo" (at offset 0)
    vim("|foo hello bar world")
        .keys("/bar/;?foo<CR>")
        .expect_text("|foo hello bar world")
        .run();
}

#[test]
fn chained_forward_then_forward() {
    // /bar/;/world — find "bar" (at offset 4), then forward for "world" (at offset 8)
    vim("|foo bar world end")
        .keys("/bar/;/world<CR>")
        .expect_text("foo bar |world end")
        .run();
}

#[test]
fn chained_backward_then_forward() {
    // ?bar?;/world — from end, backward for "bar" (at offset 4), then forward for "world" (at offset 8)
    vim("foo bar world |end")
        .keys("?bar?;/world<CR>")
        .expect_text("foo bar |world end")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════════
// NO TRAILING DELIMITER ON FIRST PATTERN
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn chained_no_trailing_delimiter() {
    // /bar;?foo — no trailing / on first pattern
    vim("|foo hello bar world")
        .keys("/bar;?foo<CR>")
        .expect_text("|foo hello bar world")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════════
// THREE-SEGMENT CHAIN
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn chained_three_segments() {
    // /bbb/;/ccc/;/ddd — find "bbb", then "ccc", then "ddd"
    vim("|aaa bbb ccc ddd eee")
        .keys("/bbb/;/ccc/;/ddd<CR>")
        .expect_text("aaa bbb ccc |ddd eee")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════════
// CHAIN WITH SEARCH THAT WRAPS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn chained_second_segment_wraps() {
    // /bar/;/foo — find "bar", then forward for "foo" which wraps back to start
    vim("|foo hello bar")
        .keys("/bar/;/foo<CR>")
        .expect_text("|foo hello bar")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════════
// CHAIN FAILURE (INTERMEDIATE SEGMENT FAILS)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn chained_second_segment_fails() {
    // /foo/;?zzz — find "foo" succeeds, but backward for "zzz" fails.
    // Cursor should NOT move (stays at original position).
    vim("|foo hello world")
        .keys("/foo/;?zzz<CR>")
        .expect_text("|foo hello world")
        .run();
}

#[test]
fn chained_first_segment_fails() {
    // /zzz/;?foo — first segment fails. Cursor should not move.
    vim("|foo hello world")
        .keys("/zzz/;?foo<CR>")
        .expect_text("|foo hello world")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════════
// N/N AFTER CHAINED SEARCH
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn chained_then_n_repeats_last_pattern() {
    // /bar/;/world — find "bar", then "world".
    // After the chain, n should repeat with the last pattern ("world")
    // and the first segment's direction (forward).
    let mut s = TestSession::new("|foo bar world foo bar world");
    s.feed("/bar/;/world<CR>");

    // Cursor should now be at the first "world" (offset 8)
    let first_world = s.text().find("world").unwrap();
    assert_eq!(
        s.cursor_offset(),
        first_world,
        "chained search should land on first 'world'"
    );

    // Now press n — should find next "world" (second occurrence)
    s.feed("n");
    let second_world = s.text()[first_world + 1..]
        .find("world")
        .map(|i| i + first_world + 1);
    if let Some(pos) = second_world {
        assert_eq!(
            s.cursor_offset(),
            pos,
            "n after chained search should find next 'world'"
        );
    }
}
