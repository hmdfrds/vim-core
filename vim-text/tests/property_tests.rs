//! Comprehensive property tests for vim-text.
//!
//! Tests VimText as a BLACK BOX: random operations are applied to both VimText
//! and a String oracle, then the results are compared. Every property here must
//! ALWAYS hold -- zero false negatives allowed.

use proptest::prelude::*;
use vim_text::summary::{IndentFlags, TextSummary};
use vim_text::{Change, ChangeSet, VimText};

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

/// Generate a random text string of bounded length.
/// Uses a mix of ASCII, whitespace, and newlines to exercise line-related logic.
fn text_strategy() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-zA-Z0-9 \n\t()\\[\\]{}]{0,500}").unwrap()
}

/// Generate a smaller text for composition tests (avoids blowup).
fn small_text_strategy() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-zA-Z0-9 \n]{0,50}").unwrap()
}

/// Generate text with multi-byte Unicode characters and CRLF sequences.
fn unicode_text_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop::sample::select(vec![
            'a',
            'b',
            'c',
            ' ',
            '\n',
            '\t',
            '\u{00E9}',  // é (2 bytes)
            '\u{4E16}',  // 世 (3 bytes)
            '\u{1F600}', // 😀 (4 bytes)
            '\r',
        ]),
        0..100,
    )
    .prop_map(|chars| chars.into_iter().collect::<String>())
}

/// Apply an edit to a String oracle (the ground truth).
fn apply_to_oracle(text: &mut String, start: usize, end: usize, replacement: &str) {
    let start = start.min(text.len());
    let end = end.min(text.len()).max(start);
    text.replace_range(start..end, replacement);
}

/// Build a ChangeSet from a single edit.
fn make_changeset(src_len: usize, start: usize, end: usize, replacement: &str) -> ChangeSet {
    let start = start.min(src_len);
    let end = end.min(src_len).max(start);
    ChangeSet::from_changes(
        src_len,
        vec![Change {
            start,
            end,
            text: replacement.into(),
        }],
    )
}

// ---------------------------------------------------------------------------
// 1. Random edits match oracle
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn random_edits_match_oracle(
        initial in text_strategy(),
        edits in prop::collection::vec(
            (0usize..=500, 0usize..=500, "[a-zA-Z0-9 \n]{0,20}"),
            1..5
        ),
    ) {
        let mut vt = VimText::from_str(&initial);
        let mut oracle = initial.clone();

        for (a, b, replacement) in edits {
            let len = oracle.len();
            let start = if len == 0 { 0 } else { a % (len + 1) };
            let end = if len == 0 { 0 } else { (b % (len + 1)).max(start) };

            let cs = make_changeset(vt.byte_len(), start, end, &replacement);
            vt.apply(&cs);
            apply_to_oracle(&mut oracle, start, end, &replacement);

            // Content must match after every edit.
            let vt_str = vt.to_string();
            prop_assert_eq!(
                &vt_str, &oracle,
                "content mismatch after edit ({}, {}, {:?})", start, end, replacement
            );

            // Metrics must be consistent.
            prop_assert_eq!(
                vt.byte_len(), oracle.len(),
                "byte_len mismatch"
            );
            let expected_lines = oracle.matches('\n').count() + 1;
            prop_assert_eq!(
                vt.line_count(), expected_lines,
                "line_count mismatch: VimText={}, oracle={}",
                vt.line_count(), expected_lines
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 2. ChangeSet compose associativity
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn changeset_compose_associativity(
        initial in small_text_strategy(),
        edit_a in (0usize..=50, 0usize..=50, "[a-z]{0,10}"),
        edit_b in (0usize..=100, 0usize..=100, "[a-z]{0,10}"),
        edit_c in (0usize..=150, 0usize..=150, "[a-z]{0,10}"),
    ) {
        let src_len = initial.len();

        // Build changeset A
        let (a_s, a_e, a_t) = edit_a;
        let a_start = if src_len == 0 { 0 } else { a_s % (src_len + 1) };
        let a_end = if src_len == 0 { 0 } else { (a_e % (src_len + 1)).max(a_start) };
        let cs_a = make_changeset(src_len, a_start, a_end, &a_t);

        // Apply A to get intermediate text
        let after_a = cs_a.apply_to_string(&initial);

        // Build changeset B on after_a
        let (b_s, b_e, b_t) = edit_b;
        let len_a = after_a.len();
        let b_start = if len_a == 0 { 0 } else { b_s % (len_a + 1) };
        let b_end = if len_a == 0 { 0 } else { (b_e % (len_a + 1)).max(b_start) };
        let cs_b = make_changeset(len_a, b_start, b_end, &b_t);

        // Apply B to get after_b
        let after_b = cs_b.apply_to_string(&after_a);

        // Build changeset C on after_b
        let (c_s, c_e, c_t) = edit_c;
        let len_b = after_b.len();
        let c_start = if len_b == 0 { 0 } else { c_s % (len_b + 1) };
        let c_end = if len_b == 0 { 0 } else { (c_e % (len_b + 1)).max(c_start) };
        let cs_c = make_changeset(len_b, c_start, c_end, &c_t);

        // compose(compose(a,b),c)
        let ab = cs_a.clone().compose(cs_b.clone());
        let ab_c = ab.compose(cs_c.clone());

        // compose(a, compose(b,c))
        let bc = cs_b.compose(cs_c);
        let a_bc = cs_a.compose(bc);

        // Both must produce the same final string.
        let result_left = ab_c.apply_to_string(&initial);
        let result_right = a_bc.apply_to_string(&initial);
        prop_assert_eq!(
            result_left, result_right,
            "compose associativity violated"
        );
    }
}

// ---------------------------------------------------------------------------
// 3. ChangeSet invert roundtrip
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn changeset_invert_roundtrip(
        text in small_text_strategy(),
        start_raw in 0usize..=50,
        end_raw in 0usize..=50,
        insert_text in "[a-zA-Z0-9]{0,15}",
    ) {
        let src_len = text.len();
        let start = if src_len == 0 { 0 } else { start_raw % (src_len + 1) };
        let end = if src_len == 0 { 0 } else { (end_raw % (src_len + 1)).max(start) };

        let cs = make_changeset(src_len, start, end, &insert_text);
        let vt = VimText::from_str(&text);
        let result = cs.apply_to_string(&text);
        let inverse = cs.invert(&vt);
        let restored = inverse.apply_to_string(&result);
        prop_assert_eq!(restored, text, "invert roundtrip failed");
    }
}

// ---------------------------------------------------------------------------
// 4. line_count == newlines + 1
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn line_count_equals_newlines_plus_one(text in text_strategy()) {
        let vt = VimText::from_str(&text);
        let expected = text.matches('\n').count() + 1;
        prop_assert_eq!(
            vt.line_count(), expected,
            "line_count({}) = {}, expected {} for {:?}",
            vt.line_count(), vt.line_count(), expected, text
        );
    }
}

// ---------------------------------------------------------------------------
// 5. line_start / line_of_offset roundtrip
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn line_start_line_of_offset_roundtrip(text in text_strategy()) {
        let vt = VimText::from_str(&text);
        let line_count = vt.line_count();

        // For each line, line_start gives a valid offset, and
        // line_of_offset on that offset returns the same line.
        for line in 0..line_count {
            let start = vt.line_start(line);
            prop_assert!(start.is_some(), "line_start({}) returned None for text {:?}", line, text);
            let start = start.unwrap();
            prop_assert!(start <= vt.byte_len(), "line_start({}) = {} > byte_len {}", line, start, vt.byte_len());

            let back = vt.line_of_offset(start);
            prop_assert_eq!(
                back, line,
                "roundtrip failed: line_start({}) = {}, line_of_offset({}) = {}",
                line, start, start, back
            );
        }

        // line_start past the end should return None
        prop_assert!(vt.line_start(line_count).is_none());
    }
}

// ---------------------------------------------------------------------------
// 6. offset_to_pos / pos_to_offset roundtrip
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn offset_pos_roundtrip(text in text_strategy()) {
        let vt = VimText::from_str(&text);

        // Test every valid byte offset
        for offset in 0..=vt.byte_len() {
            // Skip offsets that aren't at the start of a line or within line bounds
            // (offset_to_pos can return any valid position)
            if let Some(pos) = vt.offset_to_pos(offset) {
                let back = vt.pos_to_offset(pos);
                prop_assert_eq!(
                    back, Some(offset),
                    "roundtrip failed at offset {}: pos={:?}, back={:?}", offset, pos, back
                );
            }
        }

        // offset past the end should return None
        prop_assert!(vt.offset_to_pos(vt.byte_len() + 1).is_none());
    }
}

// ---------------------------------------------------------------------------
// 7. Chunks concatenation
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn chunks_concatenation(text in text_strategy()) {
        let vt = VimText::from_str(&text);
        let collected: String = vt.chunks().collect();
        prop_assert_eq!(collected, text, "chunks().collect() mismatch");
    }
}

// ---------------------------------------------------------------------------
// 8. Lines concatenation
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn lines_concatenation(text in text_strategy()) {
        let vt = VimText::from_str(&text);
        let lines: Vec<String> = vt.lines().map(|c| c.into_owned()).collect();

        // Rejoin lines with \n. This should always reconstruct the original:
        //   "a\nb\n" -> ["a", "b", ""] -> join = "a\nb\n"
        //   "a\nb"   -> ["a", "b"]     -> join = "a\nb"
        //   ""       -> [""]           -> join = ""
        //   "\n"     -> ["", ""]       -> join = "\n"
        let rejoined = lines.join("\n");
        prop_assert_eq!(
            rejoined, text,
            "lines().join(\\n) mismatch"
        );
    }
}

// ---------------------------------------------------------------------------
// 9. Chars iteration
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn chars_iteration(text in text_strategy()) {
        let vt = VimText::from_str(&text);
        let collected: String = vt.chars().collect();
        prop_assert_eq!(collected, text, "chars().collect() mismatch");
    }
}

// ---------------------------------------------------------------------------
// 10. Bytes iteration
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn bytes_iteration(text in text_strategy()) {
        let vt = VimText::from_str(&text);
        let collected: Vec<u8> = vt.bytes().collect();
        prop_assert_eq!(collected, text.as_bytes().to_vec(), "bytes() mismatch");
    }
}

// ---------------------------------------------------------------------------
// 11. DoubleEndedIterator correctness (chunks, chars, bytes)
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn double_ended_chunks(text in text_strategy()) {
        let vt = VimText::from_str(&text);
        let mut iter = vt.chunks();
        let mut front_parts = Vec::new();
        let mut back_parts = Vec::new();
        let mut toggle = true;

        loop {
            if toggle {
                match iter.next() {
                    Some(c) => front_parts.push(c.to_string()),
                    None => break,
                }
            } else {
                match iter.next_back() {
                    Some(c) => back_parts.push(c.to_string()),
                    None => break,
                }
            }
            toggle = !toggle;
        }

        let mut full = String::new();
        for p in &front_parts {
            full.push_str(p);
        }
        for p in back_parts.iter().rev() {
            full.push_str(p);
        }
        prop_assert_eq!(full, text, "DoubleEnded chunks mismatch");
    }

    #[test]
    fn double_ended_chars(text in text_strategy()) {
        let vt = VimText::from_str(&text);
        let expected: Vec<char> = text.chars().collect();
        let mut iter = vt.chars();
        let mut lo = 0usize;
        let mut hi = expected.len();
        let mut toggle = true;

        loop {
            if toggle {
                match iter.next() {
                    Some(c) => {
                        prop_assert_eq!(c, expected[lo], "forward char mismatch at {}", lo);
                        lo += 1;
                    }
                    None => break,
                }
            } else {
                match iter.next_back() {
                    Some(c) => {
                        hi -= 1;
                        prop_assert_eq!(c, expected[hi], "backward char mismatch at {}", hi);
                    }
                    None => break,
                }
            }
            toggle = !toggle;
        }
        prop_assert_eq!(lo, hi, "forward/backward char iterators didn't converge");
    }

    #[test]
    fn double_ended_bytes(text in text_strategy()) {
        let vt = VimText::from_str(&text);
        let expected = text.as_bytes();
        let mut iter = vt.bytes();
        let mut lo = 0usize;
        let mut hi = expected.len();
        let mut toggle = true;

        loop {
            if toggle {
                match iter.next() {
                    Some(b) => {
                        prop_assert_eq!(b, expected[lo], "forward byte mismatch at {}", lo);
                        lo += 1;
                    }
                    None => break,
                }
            } else {
                match iter.next_back() {
                    Some(b) => {
                        hi -= 1;
                        prop_assert_eq!(b, expected[hi], "backward byte mismatch at {}", hi);
                    }
                    None => break,
                }
            }
            toggle = !toggle;
        }
        prop_assert_eq!(lo, hi, "forward/backward byte iterators didn't converge");
    }
}

// ---------------------------------------------------------------------------
// 12. Snapshot independence
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn snapshot_independence(
        text in small_text_strategy(),
        start_raw in 0usize..=50,
        end_raw in 0usize..=50,
        insert_text in "[a-z]{0,10}",
    ) {
        let mut vt = VimText::from_str(&text);
        let snap = vt.snapshot();
        let snap_text = snap.to_string();

        let src_len = vt.byte_len();
        let start = if src_len == 0 { 0 } else { start_raw % (src_len + 1) };
        let end = if src_len == 0 { 0 } else { (end_raw % (src_len + 1)).max(start) };

        let cs = make_changeset(src_len, start, end, &insert_text);
        vt.apply(&cs);

        // Snapshot must be unchanged after mutating the original.
        prop_assert_eq!(
            snap.to_string(), snap_text,
            "snapshot changed after editing original"
        );
        prop_assert_eq!(
            snap.to_string(), text,
            "snapshot doesn't match original text"
        );
    }
}

// ---------------------------------------------------------------------------
// 13. Bloom no false negatives (actually constructs a BloomFilter)
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn bloom_no_false_negatives(text in "[a-zA-Z0-9 ]{2,200}") {
        let filter = vim_text::BloomFilter::from_text(text.as_bytes());
        // Every bigram in the text must pass the bloom filter
        for window in text.as_bytes().windows(2) {
            let query = vim_text::BloomFilter::from_text(window);
            prop_assert!(
                filter.contains_filter(&query),
                "Bloom false negative for {:?}",
                std::str::from_utf8(window).unwrap_or("?"),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 14. RopeSlice matches substring
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn rope_slice_matches_substring(
        text in text_strategy(),
        a in 0usize..=500,
        b in 0usize..=500,
    ) {
        let vt = VimText::from_str(&text);
        let len = vt.byte_len();

        if len == 0 {
            // Only valid slice is 0..0
            let slice = vt.slice(0..0);
            prop_assert!(slice.is_some());
            prop_assert_eq!(slice.unwrap().to_string(), "");
        } else {
            let start = a % (len + 1);
            let end = (b % (len + 1)).max(start);

            let slice = vt.slice(start..end);
            prop_assert!(slice.is_some(), "slice({}..{}) returned None for len {}", start, end, len);
            let slice = slice.unwrap();

            prop_assert_eq!(
                slice.to_string(), &text[start..end],
                "slice content mismatch for {}..{}", start, end
            );
            prop_assert_eq!(
                slice.byte_len(), end - start,
                "slice byte_len mismatch"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 15. CRLF not split by chunking
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn crlf_not_split_by_chunking(
        // Generate text with CRLF sequences mixed in
        parts in prop::collection::vec(
            prop_oneof![
                "[a-zA-Z0-9]{1,100}",
                Just("\r\n".to_string()),
                Just("\n".to_string()),
            ],
            1..20
        ),
    ) {
        let text: String = parts.join("");
        let vt = VimText::from_str(&text);
        let chunks: Vec<String> = vt.chunks().map(|s| s.to_string()).collect();

        for (i, chunk) in chunks.iter().enumerate() {
            if chunk.ends_with('\r') && i + 1 < chunks.len() {
                prop_assert!(
                    !chunks[i + 1].starts_with('\n'),
                    "CRLF split: chunk {} ends with \\r and chunk {} starts with \\n",
                    i, i + 1
                );
            }
        }

        // Also verify full content is preserved
        let collected: String = chunks.join("");
        prop_assert_eq!(collected, text, "CRLF text content mismatch");
    }
}

// ---------------------------------------------------------------------------
// 16. char_count matches oracle
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn char_count_matches_oracle(text in text_strategy()) {
        let vt = VimText::from_str(&text);
        let expected = text.chars().count();
        prop_assert_eq!(
            vt.char_count(), expected,
            "char_count mismatch"
        );
    }
}

// ---------------------------------------------------------------------------
// 17. byte_len matches oracle
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn byte_len_matches_oracle(text in text_strategy()) {
        let vt = VimText::from_str(&text);
        prop_assert_eq!(
            vt.byte_len(), text.len(),
            "byte_len mismatch"
        );
    }
}

// ---------------------------------------------------------------------------
// 18. line_of_offset consistency for all offsets
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn line_of_offset_consistency(text in small_text_strategy()) {
        let vt = VimText::from_str(&text);

        // For every valid offset, line_of_offset must return a valid line number,
        // and that line's start must be <= offset, and line's end must be >= offset.
        for offset in 0..=vt.byte_len() {
            let line = vt.line_of_offset(offset);

            prop_assert!(
                line < vt.line_count(),
                "line_of_offset({}) = {} >= line_count {} for {:?}",
                offset, line, vt.line_count(), text
            );

            let line_start = vt.line_start(line).unwrap();
            prop_assert!(
                line_start <= offset,
                "line_start({}) = {} > offset {} for {:?}",
                line, line_start, offset, text
            );

            // The next line's start (or byte_len) must be > offset,
            // unless offset is at byte_len (which is the empty trailing position).
            if offset < vt.byte_len() {
                let next_start = vt.line_start(line + 1).unwrap_or(vt.byte_len());
                prop_assert!(
                    offset < next_start,
                    "offset {} >= next_line_start {} for line {} in {:?}",
                    offset, next_start, line, text
                );
            }
        }

        // Past end should clamp to last line (not panic)
        let past_end = vt.line_of_offset(vt.byte_len() + 1);
        prop_assert!(past_end <= vt.line_count().saturating_sub(1),
            "line_of_offset past end should clamp, got {}", past_end);
    }
}

// ---------------------------------------------------------------------------
// 19. Lines iterator count matches line_count
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn lines_count_matches_line_count(text in text_strategy()) {
        let vt = VimText::from_str(&text);
        let iter_count = vt.lines().count();
        prop_assert_eq!(
            iter_count, vt.line_count(),
            "lines().count() = {}, line_count() = {}",
            iter_count, vt.line_count()
        );
    }
}

// ---------------------------------------------------------------------------
// 20. RopeSlice line_count matches substring
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn rope_slice_line_count(
        text in text_strategy(),
        a in 0usize..=500,
        b in 0usize..=500,
    ) {
        let vt = VimText::from_str(&text);
        let len = vt.byte_len();
        if len == 0 {
            return Ok(());
        }

        let start = a % (len + 1);
        let end = (b % (len + 1)).max(start);

        if let Some(slice) = vt.slice(start..end) {
            let substr = &text[start..end];
            let expected_lines = substr.matches('\n').count() + 1;
            prop_assert_eq!(
                slice.line_count(), expected_lines,
                "slice({}..{}).line_count() = {}, expected {}",
                start, end, slice.line_count(), expected_lines
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 21. map_pos boundary invariants
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn map_pos_boundaries(
        text in small_text_strategy(),
        start_raw in 0usize..=50,
        end_raw in 0usize..=50,
        insert in "[a-z]{0,10}",
    ) {
        let len = text.len();
        if len == 0 { return Ok(()); }
        let start = start_raw % (len + 1);
        let end = (end_raw % (len + 1)).max(start);
        let cs = make_changeset(len, start, end, &insert);
        // Position 0 always maps to 0 with Assoc::Before
        prop_assert_eq!(cs.map_pos(0, vim_text::Assoc::Before), 0);
        // End of source maps to end of destination
        prop_assert_eq!(
            cs.map_pos(cs.src_len(), vim_text::Assoc::After),
            cs.dst_len()
        );
    }
}

// ---------------------------------------------------------------------------
// 22. map_positions matches individual map_pos calls
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn map_positions_matches_individual(
        text in small_text_strategy(),
        start_raw in 0usize..=50,
        end_raw in 0usize..=50,
        insert in "[a-z]{0,10}",
    ) {
        let len = text.len();
        if len == 0 { return Ok(()); }
        let start = start_raw % (len + 1);
        let end = (end_raw % (len + 1)).max(start);
        let cs = make_changeset(len, start, end, &insert);
        let src_len = cs.src_len();
        let step = (src_len / 5).max(1);
        let positions: Vec<usize> = (0..=src_len).step_by(step).collect();
        let individual: Vec<usize> = positions.iter()
            .map(|&p| cs.map_pos(p, vim_text::Assoc::After))
            .collect();
        let mut batch: Vec<(usize, vim_text::Assoc)> = positions.iter()
            .map(|&p| (p, vim_text::Assoc::After))
            .collect();
        cs.map_positions(&mut batch);
        let batch_results: Vec<usize> = batch.iter().map(|&(p, _)| p).collect();
        prop_assert_eq!(batch_results, individual);
    }
}

// ---------------------------------------------------------------------------
// 23. Unicode text random edits match oracle
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn random_edits_match_oracle_unicode(
        text in unicode_text_strategy(),
        start_raw in 0usize..=200,
        end_raw in 0usize..=200,
        insert_chars in prop::collection::vec(
            prop::sample::select(vec!['x', '\u{00E9}', '\u{4E16}']),
            0..10,
        ),
    ) {
        let insert: String = insert_chars.into_iter().collect();
        let len = text.len();
        if len == 0 { return Ok(()); }
        // Find valid char boundaries
        let start = {
            let raw = start_raw % (len + 1);
            let mut s = raw;
            while s < len && !text.is_char_boundary(s) { s += 1; }
            s.min(len)
        };
        let end = {
            let raw = (end_raw % (len + 1)).max(start);
            let mut e = raw;
            while e < len && !text.is_char_boundary(e) { e += 1; }
            e.min(len)
        };
        let cs = make_changeset(len, start, end, &insert);
        let mut expected = text.clone();
        apply_to_oracle(&mut expected, start, end, &insert);
        let actual = cs.apply_to_string(&text);
        prop_assert_eq!(actual, expected);
    }
}

// ---------------------------------------------------------------------------
// 24. Diff roundtrip: apply hunks in reverse reconstructs new text
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn diff_roundtrip(
        old_text in text_strategy(),
        new_text in text_strategy(),
    ) {
        let old = VimText::from_str(&old_text);
        let new_vt = VimText::from_str(&new_text);
        let hunks = old.structural_diff(&new_vt);
        let mut result = old_text.clone();
        // Apply hunks in reverse order so byte offsets remain valid
        for hunk in hunks.iter().rev() {
            let replacement = &new_text[hunk.new_range.clone()];
            result.replace_range(hunk.old_range.clone(), replacement);
        }
        prop_assert_eq!(result, new_text);
    }
}

// ---------------------------------------------------------------------------
// 25. from_str metrics match manual count (validates the SIMD/scalar split)
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn from_str_metrics_match_manual_count(text in "\\PC{0,200}") {
        let summary = TextSummary::from_str(&text);
        let expected_bytes = text.len() as u32;
        let expected_chars = text.chars().count() as u32;
        let expected_newlines = text.bytes().filter(|&b| b == b'\n').count() as u32;
        let expected_utf16: u32 = text.chars().map(|c| if (c as u32) > 0xFFFF { 2u32 } else { 1u32 }).sum();

        prop_assert_eq!(summary.metrics.bytes, expected_bytes, "bytes mismatch");
        prop_assert_eq!(summary.metrics.chars, expected_chars, "chars mismatch");
        prop_assert_eq!(summary.metrics.newlines, expected_newlines, "newlines mismatch");
        prop_assert_eq!(summary.metrics.utf16_len, expected_utf16, "utf16_len mismatch");
    }
}

// ---------------------------------------------------------------------------
// 26. ALL_ASCII flag matches manual check
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn all_ascii_flag_matches_manual(text in "\\PC{0,200}") {
        let summary = TextSummary::from_str(&text);
        // Empty string is the identity element: ALL_ASCII is unset by design
        // (compose relies on this for neutral identity behavior).
        let expected = !text.is_empty() && text.bytes().all(|b| b < 128);
        prop_assert_eq!(
            summary.indent.flags.contains(IndentFlags::ALL_ASCII),
            expected,
            "ALL_ASCII flag mismatch for {:?}", text
        );
    }
}

// ---------------------------------------------------------------------------
// 27. SliceLines matches string split (O(log n) line seeking)
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn slice_lines_matches_string_split(text in "[a-z\n ]{1,500}") {
        let vt = VimText::from_str(&text);
        let byte_len = vt.byte_len();
        if byte_len == 0 { return Ok(()); }

        let slice = vt.slice(0..byte_len).unwrap();
        let slice_lines: Vec<String> = slice.lines().map(|cow| cow.into_owned()).collect();

        // Compare against string split
        let expected: Vec<&str> = text.split('\n').collect();

        prop_assert_eq!(slice_lines.len(), expected.len(),
            "line count mismatch for text len={}", text.len());
        for (i, (got, exp)) in slice_lines.iter().zip(expected.iter()).enumerate() {
            prop_assert_eq!(got.as_str(), *exp,
                "line {} mismatch", i);
        }
    }
}

// ---------------------------------------------------------------------------
// 28. SliceLines partial matches string split (start > 0 correctness)
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn slice_lines_partial_matches_string_split(
        text in "[a-z\n ]{1,500}",
        a in 0usize..=500,
        b in 0usize..=500,
    ) {
        let vt = VimText::from_str(&text);
        let len = vt.byte_len();
        if len == 0 { return Ok(()); }
        let start = a % (len + 1);
        let end = (b % (len + 1)).max(start);
        if start == end { return Ok(()); }

        let slice = vt.slice(start..end).unwrap();
        let slice_lines: Vec<String> = slice.lines().map(|cow| cow.into_owned()).collect();
        let expected: Vec<&str> = text[start..end].split('\n').collect();

        prop_assert_eq!(slice_lines.len(), expected.len(),
            "line count mismatch for text[{}..{}] (len={})", start, end, len);
        for (i, (got, exp)) in slice_lines.iter().zip(expected.iter()).enumerate() {
            prop_assert_eq!(got.as_str(), *exp,
                "line {} mismatch for text[{}..{}]", i, start, end);
        }
    }
}
