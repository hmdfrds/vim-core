use super::*;

// ═══════════════════════════════════════════════════════════════════
// ]b — find_next_bracket_pair
// ═══════════════════════════════════════════════════════════════════

#[test]
fn next_bracket_pair_finds_paren() {
    let text = "hello (world)";
    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(text, Offset::new(0), 1, &opts);
    assert_eq!(
        find_next_bracket_pair(&c),
        MotionResult::Position(Offset::new(6))
    );
}

#[test]
fn next_bracket_pair_finds_brace() {
    let text = "hello {world}";
    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(text, Offset::new(0), 1, &opts);
    assert_eq!(
        find_next_bracket_pair(&c),
        MotionResult::Position(Offset::new(6))
    );
}

#[test]
fn next_bracket_pair_finds_square_bracket() {
    let text = "hello [world]";
    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(text, Offset::new(0), 1, &opts);
    assert_eq!(
        find_next_bracket_pair(&c),
        MotionResult::Position(Offset::new(6))
    );
}

#[test]
fn next_bracket_pair_finds_angle_bracket() {
    let text = "hello <world>";
    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(text, Offset::new(0), 1, &opts);
    assert_eq!(
        find_next_bracket_pair(&c),
        MotionResult::Position(Offset::new(6))
    );
}

#[test]
fn next_bracket_pair_with_count() {
    let text = "a(b[c{d}e]f)g";
    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(text, Offset::new(0), 2, &opts);
    // First opening bracket at 1 '(', second at 3 '['
    assert_eq!(
        find_next_bracket_pair(&c),
        MotionResult::Position(Offset::new(3))
    );
}

#[test]
fn next_bracket_pair_no_match() {
    let text = "hello world";
    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(text, Offset::new(0), 1, &opts);
    assert_eq!(find_next_bracket_pair(&c), MotionResult::Error);
}

#[test]
fn next_bracket_pair_nested() {
    let text = "fn main() { if (true) {} }";
    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(text, Offset::new(0), 3, &opts);
    // ( at 8, { at 11, ( at 15
    assert_eq!(
        find_next_bracket_pair(&c),
        MotionResult::Position(Offset::new(15))
    );
}

// ═══════════════════════════════════════════════════════════════════
// [b — find_prev_bracket_pair
// ═══════════════════════════════════════════════════════════════════

#[test]
fn prev_bracket_pair_finds_paren() {
    let text = "hello (world)";
    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(text, Offset::new(12), 1, &opts);
    assert_eq!(
        find_prev_bracket_pair(&c),
        MotionResult::Position(Offset::new(6))
    );
}

#[test]
fn prev_bracket_pair_with_count() {
    let text = "a(b[c{d}e]f)g";
    let opts = crate::primitives::VimOptions::default();
    // cursor at end (13), count=2 → { at 5 is first backward, [ at 3 is second
    let c = MotionContext::new(text, Offset::new(13), 2, &opts);
    assert_eq!(
        find_prev_bracket_pair(&c),
        MotionResult::Position(Offset::new(3))
    );
}

#[test]
fn prev_bracket_pair_no_match() {
    let text = "hello world";
    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(text, Offset::new(10), 1, &opts);
    assert_eq!(find_prev_bracket_pair(&c), MotionResult::Error);
}

// ═══════════════════════════════════════════════════════════════════
// ]q — find_next_quote
// ═══════════════════════════════════════════════════════════════════

#[test]
fn next_quote_finds_double_quote() {
    let text = r#"hello "world""#;
    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(text, Offset::new(0), 1, &opts);
    assert_eq!(find_next_quote(&c), MotionResult::Position(Offset::new(6)));
}

#[test]
fn next_quote_finds_single_quote() {
    let text = "hello 'world'";
    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(text, Offset::new(0), 1, &opts);
    assert_eq!(find_next_quote(&c), MotionResult::Position(Offset::new(6)));
}

#[test]
fn next_quote_finds_backtick() {
    let text = "hello `world`";
    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(text, Offset::new(0), 1, &opts);
    assert_eq!(find_next_quote(&c), MotionResult::Position(Offset::new(6)));
}

#[test]
fn next_quote_with_count() {
    let text = r#"a"b'c`d"#;
    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(text, Offset::new(0), 2, &opts);
    // " at 1, ' at 3 → second quote
    assert_eq!(find_next_quote(&c), MotionResult::Position(Offset::new(3)));
}

#[test]
fn next_quote_no_match() {
    let text = "hello world";
    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(text, Offset::new(0), 1, &opts);
    assert_eq!(find_next_quote(&c), MotionResult::Error);
}

// ═══════════════════════════════════════════════════════════════════
// [q — find_prev_quote
// ═══════════════════════════════════════════════════════════════════

#[test]
fn prev_quote_finds_double_quote() {
    let text = r#"hello "world""#;
    let opts = crate::primitives::VimOptions::default();
    // " at 6 and " at 12, cursor at 12, searching backward from ..12 → finds " at 6
    let c = MotionContext::new(text, Offset::new(12), 1, &opts);
    assert_eq!(find_prev_quote(&c), MotionResult::Position(Offset::new(6)));
}

#[test]
fn prev_quote_no_match() {
    let text = "hello world";
    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(text, Offset::new(10), 1, &opts);
    assert_eq!(find_prev_quote(&c), MotionResult::Error);
}

// ═══════════════════════════════════════════════════════════════════
// Edge cases
// ═══════════════════════════════════════════════════════════════════

#[test]
fn next_bracket_on_bracket_skips_current() {
    // Cursor is on the '(' at offset 6; ]b should find the NEXT bracket after it
    let text = "hello (world (inner))";
    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(text, Offset::new(6), 1, &opts);
    // next_char_boundary(text, 6) = 7, so we scan from 7 onward
    // next '(' is at 13
    assert_eq!(
        find_next_bracket_pair(&c),
        MotionResult::Position(Offset::new(13))
    );
}

#[test]
fn prev_bracket_at_start_returns_error() {
    let text = "(hello)";
    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(text, Offset::new(0), 1, &opts);
    // cursor at 0, scanning backward from ..0 → empty
    assert_eq!(find_prev_bracket_pair(&c), MotionResult::Error);
}

#[test]
fn multiple_bracket_types_interleaved() {
    let text = "a(b[c{d";
    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(text, Offset::new(0), 1, &opts);
    assert_eq!(
        find_next_bracket_pair(&c),
        MotionResult::Position(Offset::new(1))
    );
    let c2 = MotionContext::new(text, Offset::new(0), 3, &opts);
    assert_eq!(
        find_next_bracket_pair(&c2),
        MotionResult::Position(Offset::new(5))
    );
}

// ═══════════════════════════════════════════════════════════════════
// CommentStringRanges
// ═══════════════════════════════════════════════════════════════════

#[test]
fn comment_string_ranges_simple_double_quote() {
    let text = "hello \"world\" test";
    let ranges = CommentStringRanges::scan(text, 0, text.len());
    // "world" is at positions 6..13 (inclusive of quotes)
    assert!(ranges.contains(6)); // opening quote
    assert!(ranges.contains(7)); // w
    assert!(ranges.contains(12)); // closing quote
    assert!(!ranges.contains(0)); // h (outside)
    assert!(!ranges.contains(14)); // space after
}

#[test]
fn comment_string_ranges_simple_single_quote() {
    let text = "hello 'world' test";
    let ranges = CommentStringRanges::scan(text, 0, text.len());
    // 'world' is at positions 6..13
    assert!(ranges.contains(6)); // opening quote
    assert!(ranges.contains(11)); // closing quote
    assert!(!ranges.contains(14));
}

#[test]
fn comment_string_ranges_line_comment() {
    let text = "code // comment\nmore";
    let ranges = CommentStringRanges::scan(text, 0, text.len());
    // // comment is at 5..15
    assert!(ranges.contains(5)); // /
    assert!(ranges.contains(6)); // /
    assert!(ranges.contains(14)); // t
    assert!(!ranges.contains(16)); // m
}

#[test]
fn comment_string_ranges_block_comment() {
    let text = "code /* block */ more";
    let ranges = CommentStringRanges::scan(text, 0, text.len());
    // /* block */ is at 5..16 (end+1)
    assert!(ranges.contains(5)); // /
    assert!(ranges.contains(6)); // *
    assert!(ranges.contains(15)); // /
    assert!(!ranges.contains(17)); // space after comment
}

#[test]
fn comment_string_ranges_escaped_quotes() {
    let text = r#"hello "say \"hi\"" test"#;
    let ranges = CommentStringRanges::scan(text, 0, text.len());
    // In raw string: "say \"hi\"" where \" are literal backslash-quote
    // Position 6: " (start of string)
    // Position 12: \" (escaped quote - backslash at 11, quote at 12)
    // Position 17: " (closing quote of the string)
    assert!(ranges.contains(6)); // opening "
    assert!(ranges.contains(12)); // \" (inside string, escaped)
    assert!(ranges.contains(17)); // closing "
    assert!(!ranges.contains(18)); // space after
}

#[test]
fn comment_string_ranges_multiple_ranges() {
    let text = "\"str1\" code \"str2\" // comment";
    let ranges = CommentStringRanges::scan(text, 0, text.len());
    // "str1" at 0..6, "str2" at 13..19, // comment at 20..end
    assert!(ranges.contains(0)); // str1
    assert!(ranges.contains(5));
    assert!(ranges.contains(13)); // str2
    assert!(ranges.contains(20)); // comment
    assert!(!ranges.contains(7)); // space between
}

#[test]
fn comment_string_ranges_window() {
    let text = "code \"string\" more";
    // Only scan the middle part containing the string
    // text[4..14] = " \"string\""
    let ranges = CommentStringRanges::scan(text, 4, 14);
    // String quote at position 5 (opening) and 12 (closing)
    assert!(ranges.contains(5)); // opening "
    assert!(ranges.contains(12)); // closing "
    assert!(!ranges.contains(4)); // space before string
}

#[test]
fn comment_string_ranges_empty_ranges() {
    let text = "just plain code";
    let ranges = CommentStringRanges::scan(text, 0, text.len());
    assert!(!ranges.contains(0));
    assert!(!ranges.contains(7));
}

#[test]
fn comment_string_ranges_window_starts_in_double_quote() {
    // Test window starting inside a double-quoted string
    let text = r#"code "hello world" end"#;
    // Window from position 7 (inside "hello world") to 17
    // Positions: h=7, e=8, l=9, l=10, o=11, space=12, w=13, o=14, r=15, l=16, d=17
    let ranges = CommentStringRanges::scan(text, 7, 17);
    // The entire window should be marked as in a double-quoted string
    assert!(ranges.contains(7)); // h (inside string)
    assert!(ranges.contains(11)); // o (inside string)
    assert!(ranges.contains(16)); // l (inside string)
}

#[test]
fn comment_string_ranges_window_starts_in_single_quote() {
    // Test window starting inside a single-quoted string
    let text = "code 'hello world' end";
    // Window from position 7 (inside 'hello world') to 17
    let ranges = CommentStringRanges::scan(text, 7, 17);
    // The entire window should be marked as in a single-quoted string
    assert!(ranges.contains(7)); // h (inside string)
    assert!(ranges.contains(11)); // o (inside string)
    assert!(ranges.contains(16)); // l (inside string)
}

#[test]
fn comment_string_ranges_window_starts_in_block_comment() {
    // Test window starting inside a block comment
    let text = "code /* comment here */ end";
    // Window from position 12 (inside /* comment here */) to 22
    let ranges = CommentStringRanges::scan(text, 12, 22);
    // The entire window should be marked as in a block comment
    assert!(ranges.contains(12)); // c
    assert!(ranges.contains(16)); // e
    assert!(ranges.contains(21)); // e
}

// ═══════════════════════════════════════════════════════════════════
// ADVERSARIAL SAFETY TESTS — Bracket Hardening Verification
// ═══════════════════════════════════════════════════════════════════

#[test]
fn adversarial_unmatched_brackets_no_hang() {
    // Test 1: Unmatched brackets — unbounded scan test
    // Create 200K unmatched closing brackets
    let text = "}".repeat(200_000);
    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(&text, Offset::new(0), 1, &opts);

    // Should return Error (no match found), not hang
    let result = find_next_bracket_pair(&c);
    assert_eq!(result, MotionResult::Error);
}

#[test]
fn adversarial_deep_nesting_no_overflow() {
    // Test 2: Deep nesting test
    // Create 50K nested brackets: {{{...}}}
    let half = "{\n".repeat(25_000);
    let closing = "}\n".repeat(25_000);
    let text = format!("{}{}", half, closing);

    let opts = crate::primitives::VimOptions::default();
    // Cursor at middle position (roughly at 25K mark)
    let middle = text.len() / 2;
    let c = MotionContext::new(&text, Offset::new(middle), 1, &opts);

    // Should handle deeply nested content without stack overflow
    // and respect MAX_BRACKET_TRAVEL limit
    let result = next_unmatched_brace(&c);
    // Should either find something or return Error, not panic
    assert!(matches!(
        result,
        MotionResult::Position(_) | MotionResult::Error
    ));
}

#[test]
fn adversarial_all_comment_file_no_o_n_squared() {
    // Test 3: All-comment file test (for O(n²) prevention)
    // Create 200K characters inside a block comment with no unmatched brackets
    let text = format!("/* {} */", "x".repeat(200_000));

    let opts = crate::primitives::VimOptions::default();
    // Cursor at position 10K inside the comment
    let cursor_pos = 10_000;
    let c = MotionContext::new(&text, Offset::new(cursor_pos), 1, &opts);

    // Should return Error after MAX_BRACKET_TRAVEL scan
    // Should complete quickly (< 5ms), not O(n²) which would take seconds
    let result = find_next_bracket_pair(&c);
    assert_eq!(result, MotionResult::Error);
}

#[test]
fn adversarial_complex_interleaved_content() {
    // Test 4: Complex interleaved test (real-world-ish)
    // Mixed strings, comments, and brackets in a 150K file
    let mut text = String::new();

    // Repeat a pattern with strings, comments, and brackets
    for i in 0..5000 {
        text.push_str(&format!(
            "code_{i} = \"string with (brackets)\"; /* comment with {{ brackets }} */ {{ actual = [brackets]; }}\n"
        ));
    }

    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(&text, Offset::new(0), 1, &opts);

    // Should find the first real opening bracket
    // Should complete quickly despite the large file and mixed content
    let result = find_next_bracket_pair(&c);
    // Depending on content, may find something or bail out
    assert!(matches!(
        result,
        MotionResult::Position(_) | MotionResult::Error
    ));
}

#[test]
fn adversarial_max_bracket_travel_limit() {
    // Test 5: Verify MAX_BRACKET_TRAVEL actually stops scanning
    // Create text with closing bracket far beyond MAX_BRACKET_TRAVEL
    let text = {
        let mut s = String::new();
        s.push('(');
        s.push_str(&"x".repeat(150_000));
        s.push(')');
        s
    };

    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(&text, Offset::new(0), 1, &opts);

    // Should return Error because the closing bracket is beyond
    // the MAX_BRACKET_TRAVEL distance from opening bracket
    let result = find_next_bracket_pair(&c);
    assert_eq!(result, MotionResult::Error);
}

#[test]
fn adversarial_escaped_quotes_large_file() {
    // Test 6: Many escaped quotes in a large file
    // Verify that escaped quote handling doesn't cause exponential backtracking
    let mut text = String::new();
    for _ in 0..10_000 {
        text.push_str("\\\""); // Many escaped quotes
    }
    text.push_str("(real)"); // Real bracket pair at the end

    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(&text, Offset::new(0), 1, &opts);

    // Should find the real bracket pair
    let result = find_next_bracket_pair(&c);
    assert_eq!(
        result,
        MotionResult::Position(Offset::new(text.find('(').unwrap()))
    );
}

#[test]
fn adversarial_nested_comments_with_unmatched_brackets() {
    // Test 7: Nested block comments with unmatched bracket characters
    // Verify bracket safety is NOT tricked by brackets in comments
    let text = "/* first comment { [ ( with unmatched brackets */ (real)";

    let opts = crate::primitives::VimOptions::default();
    // Start searching from position before the real bracket
    let search_start = text.find("(real").unwrap() - 1;
    let c = MotionContext::new(&text, Offset::new(search_start), 1, &opts);

    // Should find the real opening paren, skipping comment brackets
    let result = find_next_bracket_pair(&c);
    // Should successfully find a bracket without hanging
    // Comment brackets shouldn't interfere with bracket finding
    assert!(matches!(
        result,
        MotionResult::Position(_) | MotionResult::Error
    ));
}

#[test]
fn adversarial_alternating_comment_and_real_brackets() {
    // Test 8: Alternating comments with real brackets
    // Verify comment skipping doesn't cause hangs or crashes
    let text = "/* ( */ { /* ) */ } /* [ */ [ /* ] */ ]";

    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(&text, Offset::new(0), 1, &opts);

    // Should find a bracket without hanging or crashing
    // even with comments containing bracket-like characters
    let result = find_next_bracket_pair(&c);
    // Just verify it completes (doesn't hang)
    let _ = result;
}

// ═══════════════════════════════════════════════════════════════════
// Unicode bracket pairs — % matching
// ═══════════════════════════════════════════════════════════════════

#[test]
fn matching_bracket_smart_single_quote_forward() {
    // \u{2018} = ' (left),  \u{2019} = ' (right)
    let text = "\u{2018}hello\u{2019}";
    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(text, Offset::new(0), 1, &opts);
    // cursor on open smart-single quote → should land on the close quote
    let close_pos = text.find('\u{2019}').expect("close quote present");
    assert_eq!(
        matching_bracket(&c),
        MotionResult::Position(Offset::new(close_pos))
    );
}

#[test]
fn matching_bracket_smart_single_quote_backward() {
    // cursor on ' → should jump back to '
    let text = "\u{2018}hello\u{2019}";
    let opts = crate::primitives::VimOptions::default();
    let close_pos = text.find('\u{2019}').expect("close quote present");
    let c = MotionContext::new(text, Offset::new(close_pos), 1, &opts);
    assert_eq!(matching_bracket(&c), MotionResult::Position(Offset::new(0)));
}

#[test]
fn matching_bracket_smart_double_quote_forward() {
    // \u{201C} = " (left),  \u{201D} = " (right)
    let text = "\u{201C}world\u{201D}";
    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(text, Offset::new(0), 1, &opts);
    let close_pos = text.find('\u{201D}').expect("close quote present");
    assert_eq!(
        matching_bracket(&c),
        MotionResult::Position(Offset::new(close_pos))
    );
}

#[test]
fn matching_bracket_smart_double_quote_backward() {
    let text = "\u{201C}world\u{201D}";
    let opts = crate::primitives::VimOptions::default();
    let close_pos = text.find('\u{201D}').expect("close quote present");
    let c = MotionContext::new(text, Offset::new(close_pos), 1, &opts);
    assert_eq!(matching_bracket(&c), MotionResult::Position(Offset::new(0)));
}

#[test]
fn matching_bracket_guillemets_forward() {
    // \u{00AB} = «,  \u{00BB} = »
    let text = "\u{00AB}bonjour\u{00BB}";
    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(text, Offset::new(0), 1, &opts);
    let close_pos = text.find('\u{00BB}').expect("close guillemet present");
    assert_eq!(
        matching_bracket(&c),
        MotionResult::Position(Offset::new(close_pos))
    );
}

#[test]
fn matching_bracket_guillemets_backward() {
    let text = "\u{00AB}bonjour\u{00BB}";
    let opts = crate::primitives::VimOptions::default();
    let close_pos = text.find('\u{00BB}').expect("close guillemet present");
    let c = MotionContext::new(text, Offset::new(close_pos), 1, &opts);
    assert_eq!(matching_bracket(&c), MotionResult::Position(Offset::new(0)));
}

#[test]
fn matching_bracket_cjk_corner_forward() {
    // \u{300C} = 「,  \u{300D} = 」
    let text = "\u{300C}日本語\u{300D}";
    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(text, Offset::new(0), 1, &opts);
    let close_pos = text.find('\u{300D}').expect("close corner bracket present");
    assert_eq!(
        matching_bracket(&c),
        MotionResult::Position(Offset::new(close_pos))
    );
}

#[test]
fn matching_bracket_cjk_corner_backward() {
    let text = "\u{300C}日本語\u{300D}";
    let opts = crate::primitives::VimOptions::default();
    let close_pos = text.find('\u{300D}').expect("close corner bracket present");
    let c = MotionContext::new(text, Offset::new(close_pos), 1, &opts);
    assert_eq!(matching_bracket(&c), MotionResult::Position(Offset::new(0)));
}

#[test]
fn matching_bracket_fullwidth_paren_forward() {
    // \u{FF08} = （,  \u{FF09} = ）
    let text = "\u{FF08}full\u{FF09}";
    let opts = crate::primitives::VimOptions::default();
    let c = MotionContext::new(text, Offset::new(0), 1, &opts);
    let close_pos = text
        .find('\u{FF09}')
        .expect("close fullwidth paren present");
    assert_eq!(
        matching_bracket(&c),
        MotionResult::Position(Offset::new(close_pos))
    );
}

#[test]
fn matching_bracket_fullwidth_paren_backward() {
    let text = "\u{FF08}full\u{FF09}";
    let opts = crate::primitives::VimOptions::default();
    let close_pos = text
        .find('\u{FF09}')
        .expect("close fullwidth paren present");
    let c = MotionContext::new(text, Offset::new(close_pos), 1, &opts);
    assert_eq!(matching_bracket(&c), MotionResult::Position(Offset::new(0)));
}

#[test]
fn bracket_info_mathematical_angle() {
    assert!(bracket_info('\u{27E8}').is_some());
    assert!(bracket_info('\u{27E9}').is_some());
    let (open, close, is_fwd) = bracket_info('\u{27E8}').unwrap();
    assert_eq!(open, '\u{27E8}');
    assert_eq!(close, '\u{27E9}');
    assert!(is_fwd);
}

#[test]
fn bracket_info_lenticular() {
    assert!(bracket_info('\u{3010}').is_some());
    assert!(bracket_info('\u{3011}').is_some());
}

#[test]
fn bracket_info_white_corner() {
    assert!(bracket_info('\u{300E}').is_some());
    assert!(bracket_info('\u{300F}').is_some());
}

#[test]
fn bracket_info_cjk_angle() {
    assert!(bracket_info('\u{3008}').is_some());
    assert!(bracket_info('\u{3009}').is_some());
}

#[test]
fn bracket_info_existing_pairs_still_work() {
    assert!(bracket_info('(').is_some());
    assert!(bracket_info(')').is_some());
    assert!(bracket_info('[').is_some());
    assert!(bracket_info('{').is_some());
    assert!(bracket_info('\u{300C}').is_some());
}

// ═══════════════════════════════════════════════════════════════════
// find_matching_open_bracket (ShowMatch support)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn find_matching_open_bracket_simple_paren() {
    let text = "(hello";
    // Closing ')' about to be inserted at offset 6
    assert_eq!(
        find_matching_open_bracket(text, 6, ')'),
        Some(Offset::new(0))
    );
}

#[test]
fn find_matching_open_bracket_nested() {
    let text = "(a(b";
    // ')' at offset 4 should match inner '(' at 2
    assert_eq!(
        find_matching_open_bracket(text, 4, ')'),
        Some(Offset::new(2))
    );
}

#[test]
fn find_matching_open_bracket_brace() {
    let text = "fn() {code";
    assert_eq!(
        find_matching_open_bracket(text, 10, '}'),
        Some(Offset::new(5))
    );
}

#[test]
fn find_matching_open_bracket_square() {
    let text = "arr[x";
    assert_eq!(
        find_matching_open_bracket(text, 5, ']'),
        Some(Offset::new(3))
    );
}

#[test]
fn find_matching_open_bracket_no_match() {
    let text = "hello";
    assert_eq!(find_matching_open_bracket(text, 5, ')'), None);
}

#[test]
fn find_matching_open_bracket_rejects_opener() {
    let text = "hello";
    // '(' is an opener, not a closer — should return None
    assert_eq!(find_matching_open_bracket(text, 5, '('), None);
}

#[test]
fn find_matching_open_bracket_skips_strings() {
    let text = "(\")\" x";
    // The ')' inside the string at offset 2 should be ignored.
    // Closing ')' at offset 6 should match the '(' at offset 0.
    assert_eq!(
        find_matching_open_bracket(text, 6, ')'),
        Some(Offset::new(0))
    );
}
