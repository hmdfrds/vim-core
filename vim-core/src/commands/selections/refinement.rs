//! Selection refinement operations — pure selection transforms.
//!
//! These take `&Selections` + text + predicate/regex and return
//! `Selections` or `Option<Selections>`. They never modify document text.

use smallvec::SmallVec;

use crate::primitives::{Offset, SelectionRange, Selections};
use crate::regex::{MatchContext, VimRegex};

/// For each selection range, split at regex match boundaries.
/// The gaps between matches become new selections.
/// Always returns at least the original ranges (pass-through if no matches).
#[must_use]
pub fn split_on_matches(selections: &Selections, text: &str, regex: &VimRegex) -> Selections {
    selections.clone().transform_iter(|sel| {
        let start = sel.start().get();
        let end = sel.end().get();

        // Zero-width ranges pass through unchanged.
        if start >= end {
            return SmallVec::from_elem(sel, 1);
        }

        // Clamp to text bounds.
        let clamped_end = end.min(text.len());
        if start >= clamped_end {
            return SmallVec::from_elem(sel, 1);
        }

        let slice = &text[start..clamped_end];
        let ctx = MatchContext::simple(slice);

        let matches = regex.find_all(&ctx).unwrap_or_default();

        if matches.is_empty() {
            return SmallVec::from_elem(sel, 1);
        }

        let mut gaps: SmallVec<[SelectionRange; 1]> = SmallVec::new();
        let mut cursor = 0usize;

        for m in &matches {
            if m.range.start > cursor {
                gaps.push(SelectionRange::new(
                    Offset::new(start + cursor),
                    Offset::new(start + m.range.start),
                ));
            }
            cursor = m.range.end;
        }

        // Trailing gap after last match.
        let slice_len = clamped_end - start;
        if cursor < slice_len {
            gaps.push(SelectionRange::new(
                Offset::new(start + cursor),
                Offset::new(clamped_end),
            ));
        }

        // If all matches covered the entire range, pass through original.
        if gaps.is_empty() {
            return SmallVec::from_elem(sel, 1);
        }

        gaps
    })
}

/// Keep only selections whose text matches regex. None if all filtered.
#[must_use]
pub fn keep_matching(selections: &Selections, text: &str, regex: &VimRegex) -> Option<Selections> {
    selections.clone().filter(|sel| {
        let start = sel.start().get();
        let end = sel.end().get();

        // Zero-width ranges never match.
        if start >= end {
            return false;
        }

        let clamped_end = end.min(text.len());
        if start >= clamped_end {
            return false;
        }

        let slice = &text[start..clamped_end];
        let ctx = MatchContext::simple(slice);

        regex.is_match(&ctx).unwrap_or(false)
    })
}

/// Keep only selections whose text does NOT match. None if all filtered.
#[must_use]
pub fn remove_matching(
    selections: &Selections,
    text: &str,
    regex: &VimRegex,
) -> Option<Selections> {
    selections.clone().filter(|sel| {
        let start = sel.start().get();
        let end = sel.end().get();

        // Zero-width ranges always survive (cannot match).
        if start >= end {
            return true;
        }

        let clamped_end = end.min(text.len());
        if start >= clamped_end {
            return true;
        }

        let slice = &text[start..clamped_end];
        let ctx = MatchContext::simple(slice);

        !regex.is_match(&ctx).unwrap_or(false)
    })
}

/// Trim leading/trailing whitespace from each selection range.
/// Selections that are all-whitespace are removed entirely.
/// Returns `None` if all selections are trimmed away.
#[must_use]
pub fn trim_whitespace(selections: &Selections, text: &str) -> Option<Selections> {
    // First filter out all-whitespace ranges, then narrow the survivors.
    let filtered = selections.clone().filter(|sel| {
        let start = sel.start().get();
        let end = sel.end().get();
        if start >= end {
            return false;
        }
        let clamped_end = end.min(text.len());
        if start >= clamped_end {
            return false;
        }
        let slice = &text[start..clamped_end];
        !slice.chars().all(char::is_whitespace)
    })?;

    Some(filtered.transform(|sel| {
        let start = sel.start().get();
        let end = sel.end().get();
        let clamped_end = end.min(text.len());
        let slice = &text[start..clamped_end];

        // Find first non-whitespace byte.
        let trimmed_start = slice
            .char_indices()
            .find(|(_, c)| !c.is_whitespace())
            .map_or(0, |(i, _)| i);

        // Find last non-whitespace byte end.
        let trimmed_end = slice
            .char_indices()
            .rev()
            .find(|(_, c)| !c.is_whitespace())
            .map_or(slice.len(), |(i, c)| i + c.len_utf8());

        SelectionRange::new(
            Offset::new(start + trimmed_start),
            Offset::new(start + trimmed_end),
        )
    }))
}

/// Collapse each selection to a zero-width cursor at its head.
#[must_use]
pub fn collapse(selections: &Selections) -> Selections {
    selections
        .clone()
        .transform(|sel| SelectionRange::insert_cursor(sel.head()))
}

/// Flip anchor and head of each selection.
#[must_use]
pub fn flip(selections: &Selections) -> Selections {
    selections.clone().transform(SelectionRange::flipped)
}

/// Ensure all selections are forward (anchor <= head).
#[must_use]
pub fn ensure_forward(selections: &Selections) -> Selections {
    selections
        .clone()
        .transform(|sel| if sel.is_forward() { sel } else { sel.flipped() })
}

/// For each selection range, find all regex matches within it.
/// Each match becomes a new selection range.
///
/// Returns `None` if no matches found across any selection.
#[must_use]
pub fn select_on_matches(
    selections: &Selections,
    text: &str,
    regex: &VimRegex,
) -> Option<Selections> {
    let mut new_ranges: SmallVec<[SelectionRange; 1]> = SmallVec::new();
    let mut new_primary = 0usize;
    let mut found_primary = false;

    for (i, sel) in selections.ranges().iter().enumerate() {
        let start = sel.start().get();
        let end = sel.end().get();

        // Skip zero-width ranges — nothing to search.
        if start >= end {
            continue;
        }

        // Clamp to text bounds.
        let clamped_end = end.min(text.len());
        if start >= clamped_end {
            continue;
        }

        let slice = &text[start..clamped_end];
        let ctx = MatchContext::simple(slice);

        let matches = regex.find_all(&ctx).unwrap_or_default();

        // First match from the primary's range becomes the new primary.
        if i == selections.primary_index() && !found_primary && !matches.is_empty() {
            new_primary = new_ranges.len();
            found_primary = true;
        }

        for m in matches {
            let abs_start = Offset::new(start + m.range.start);
            let abs_end = Offset::new(start + m.range.end);
            new_ranges.push(SelectionRange::new(abs_start, abs_end));
        }
    }

    if new_ranges.is_empty() {
        return None;
    }

    Some(Selections::new(new_ranges, new_primary).normalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regex(pattern: &str) -> VimRegex {
        VimRegex::new(pattern).unwrap()
    }

    #[test]
    fn select_on_matches_basic() {
        let text = "hello world foo bar";
        // Single selection covering entire text.
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(text.len())));
        let re = regex(r"\w\+");
        let result = select_on_matches(&sel, text, &re).unwrap();
        assert_eq!(result.len(), 4);
        // "hello"
        assert_eq!(result.ranges()[0].start(), Offset::new(0));
        assert_eq!(result.ranges()[0].end(), Offset::new(5));
        // "world"
        assert_eq!(result.ranges()[1].start(), Offset::new(6));
        assert_eq!(result.ranges()[1].end(), Offset::new(11));
        // "foo"
        assert_eq!(result.ranges()[2].start(), Offset::new(12));
        assert_eq!(result.ranges()[2].end(), Offset::new(15));
        // "bar"
        assert_eq!(result.ranges()[3].start(), Offset::new(16));
        assert_eq!(result.ranges()[3].end(), Offset::new(19));
    }

    #[test]
    fn select_on_matches_no_matches() {
        let text = "hello world";
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(text.len())));
        // Search for digits — none exist.
        let re = regex(r"\d\+");
        assert!(select_on_matches(&sel, text, &re).is_none());
    }

    #[test]
    fn select_on_matches_within_range() {
        let text = "hello world foo bar";
        // Only select "world foo" (bytes 6..15).
        let sel = Selections::single(SelectionRange::new(Offset::new(6), Offset::new(15)));
        let re = regex(r"\w\+");
        let result = select_on_matches(&sel, text, &re).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result.ranges()[0].start(), Offset::new(6));
        assert_eq!(result.ranges()[0].end(), Offset::new(11));
        assert_eq!(result.ranges()[1].start(), Offset::new(12));
        assert_eq!(result.ranges()[1].end(), Offset::new(15));
    }

    #[test]
    fn select_on_matches_multiple_ranges() {
        let text = "aaa bbb ccc ddd";
        // Two ranges: "aaa bbb" (0..7) and "ccc ddd" (8..15).
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(0), Offset::new(7)),
            SelectionRange::new(Offset::new(8), Offset::new(15)),
        ]);
        let sel = Selections::new(ranges, 0);
        let re = regex(r"\w\+");
        let result = select_on_matches(&sel, text, &re).unwrap();
        assert_eq!(result.len(), 4);
        // From first range.
        assert_eq!(result.ranges()[0].start(), Offset::new(0));
        assert_eq!(result.ranges()[0].end(), Offset::new(3));
        assert_eq!(result.ranges()[1].start(), Offset::new(4));
        assert_eq!(result.ranges()[1].end(), Offset::new(7));
        // From second range.
        assert_eq!(result.ranges()[2].start(), Offset::new(8));
        assert_eq!(result.ranges()[2].end(), Offset::new(11));
        assert_eq!(result.ranges()[3].start(), Offset::new(12));
        assert_eq!(result.ranges()[3].end(), Offset::new(15));
    }

    #[test]
    fn select_on_matches_zero_width_skipped() {
        let text = "hello";
        // Zero-width range.
        let sel = Selections::single(SelectionRange::new(Offset::new(3), Offset::new(3)));
        let re = regex(r"\w\+");
        assert!(select_on_matches(&sel, text, &re).is_none());
    }

    // ── Edge cases ────────────────────────────────────────────────────

    #[test]
    fn adjacent_ranges_merged_by_normalize() {
        // Two adjacent selection ranges: 0..10 and 10..19.
        // Matches that touch at the boundary get merged by normalize().
        let text = "aaa bbb ccc ddd eee";
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(0), Offset::new(10)),
            SelectionRange::new(Offset::new(10), Offset::new(19)),
        ]);
        let sel = Selections::new(ranges, 0);
        let re = regex(r"\w\+");
        let result = select_on_matches(&sel, text, &re).unwrap();
        // First range "aaa bbb cc" yields: "aaa" (0..3), "bbb" (4..7), "cc" (8..10).
        // Second range "c ddd eee" yields: "c" (10..11), "ddd" (12..15), "eee" (16..19).
        // normalize() merges "cc" (8..10) and "c" (10..11) into (8..11) since 10 <= 10.
        assert_eq!(result.len(), 5);
        assert_eq!(
            result.ranges()[0],
            SelectionRange::new(Offset::new(0), Offset::new(3))
        );
        assert_eq!(
            result.ranges()[1],
            SelectionRange::new(Offset::new(4), Offset::new(7))
        );
        assert_eq!(
            result.ranges()[2],
            SelectionRange::new(Offset::new(8), Offset::new(11))
        ); // merged
        assert_eq!(
            result.ranges()[3],
            SelectionRange::new(Offset::new(12), Offset::new(15))
        );
        assert_eq!(
            result.ranges()[4],
            SelectionRange::new(Offset::new(16), Offset::new(19))
        );
    }

    #[test]
    fn non_adjacent_ranges_not_merged() {
        // Two ranges with a gap: 0..7 and 12..19 (gap at 7..12).
        // No merging happens since ranges don't touch.
        let text = "aaa bbb ... ccc ddd";
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(0), Offset::new(7)),
            SelectionRange::new(Offset::new(12), Offset::new(19)),
        ]);
        let sel = Selections::new(ranges, 0);
        let re = regex(r"\w\+");
        let result = select_on_matches(&sel, text, &re).unwrap();
        assert_eq!(result.len(), 4);
        assert_eq!(
            result.ranges()[0],
            SelectionRange::new(Offset::new(0), Offset::new(3))
        );
        assert_eq!(
            result.ranges()[1],
            SelectionRange::new(Offset::new(4), Offset::new(7))
        );
        assert_eq!(
            result.ranges()[2],
            SelectionRange::new(Offset::new(12), Offset::new(15))
        );
        assert_eq!(
            result.ranges()[3],
            SelectionRange::new(Offset::new(16), Offset::new(19))
        );
    }

    #[test]
    fn anchored_start_pattern() {
        // `^` anchors to the start of the *slice* passed to the regex engine,
        // which is the beginning of each selection range.
        let text = "hello world";
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(text.len())));
        let re = regex(r"^hello");
        let result = select_on_matches(&sel, text, &re).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result.ranges()[0].start(), Offset::new(0));
        assert_eq!(result.ranges()[0].end(), Offset::new(5));
    }

    #[test]
    fn anchored_end_pattern() {
        let text = "hello world";
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(text.len())));
        let re = regex(r"world$");
        let result = select_on_matches(&sel, text, &re).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result.ranges()[0].start(), Offset::new(6));
        assert_eq!(result.ranges()[0].end(), Offset::new(11));
    }

    #[test]
    fn anchored_start_no_match_mid_range() {
        // `^` should not match in the middle of the selection slice.
        let text = "aaa bbb";
        // Selection starts at offset 4 ("bbb"). The slice is "bbb".
        // `^aaa` should NOT match because the slice starts with "bbb".
        let sel = Selections::single(SelectionRange::new(Offset::new(4), Offset::new(7)));
        let re = regex(r"^aaa");
        assert!(select_on_matches(&sel, text, &re).is_none());
    }

    #[test]
    fn unicode_text_multibyte_offsets() {
        // Each CJK char is 3 bytes. "日本語" = 9 bytes.
        // Adjacent single-char matches get merged by normalize, so instead
        // we use a word pattern on text with spaces between CJK chars.
        let text = "日 本 語";
        assert_eq!(text.len(), 11); // 3+1+3+1+3
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(text.len())));
        let re = regex(r"[日本語]");
        let result = select_on_matches(&sel, text, &re).unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(
            result.ranges()[0],
            SelectionRange::new(Offset::new(0), Offset::new(3))
        );
        assert_eq!(
            result.ranges()[1],
            SelectionRange::new(Offset::new(4), Offset::new(7))
        );
        assert_eq!(
            result.ranges()[2],
            SelectionRange::new(Offset::new(8), Offset::new(11))
        );
    }

    #[test]
    fn unicode_word_matches() {
        // Use text where multibyte characters are separated by non-word characters.
        let text = "hello мир world";
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(text.len())));
        let re = regex(r"\w\+");
        let result = select_on_matches(&sel, text, &re).unwrap();
        // "hello", "мир", "world" — 3 words.
        assert!(
            result.len() >= 2,
            "expected at least 2 word matches, got {}",
            result.len()
        );
        assert_eq!(result.ranges()[0].start(), Offset::new(0));
    }

    #[test]
    fn many_matches_over_100() {
        // 200 words separated by spaces.
        let text: String = (0..200)
            .map(|i| format!("w{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(text.len())));
        let re = regex(r"\w\+");
        let result = select_on_matches(&sel, &text, &re).unwrap();
        assert_eq!(result.len(), 200);
        // Spot-check first and last.
        assert_eq!(result.ranges()[0].start(), Offset::new(0));
        let last = result.ranges()[199];
        assert!(last.end().get() == text.len());
    }

    #[test]
    fn primary_tracking_when_primary_has_no_matches() {
        // Primary range (index 1) has no matches; other range does.
        // Primary should fall back to 0 (first match from the first range).
        let text = "abc 123 xyz";
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(0), Offset::new(3)), // "abc"
            SelectionRange::new(Offset::new(4), Offset::new(7)), // "123"
        ]);
        // Primary is range 1 ("123").
        let sel = Selections::new(ranges, 1);
        // Only match letters — "123" has no letters.
        let re = regex(r"[a-z]\+");
        let result = select_on_matches(&sel, text, &re).unwrap();
        assert_eq!(result.len(), 1);
        // Primary should be 0 (the only match, from range 0).
        assert_eq!(result.primary_index(), 0);
    }

    #[test]
    fn primary_tracking_when_primary_has_matches() {
        // Primary range (index 1) has matches — its first match becomes new primary.
        let text = "aaa bbb ccc ddd";
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(0), Offset::new(7)), // "aaa bbb"
            SelectionRange::new(Offset::new(8), Offset::new(15)), // "ccc ddd"
        ]);
        let sel = Selections::new(ranges, 1);
        let re = regex(r"\w\+");
        let result = select_on_matches(&sel, text, &re).unwrap();
        assert_eq!(result.len(), 4);
        // Range 0 produces matches at indices 0,1. Range 1 (primary) produces 2,3.
        // First match from primary range = index 2.
        assert_eq!(result.primary_index(), 2);
    }

    #[test]
    fn end_beyond_text_length_clamped() {
        let text = "hi";
        // Selection end far beyond text length — should clamp.
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(1000)));
        let re = regex(r"\w\+");
        let result = select_on_matches(&sel, text, &re).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            result.ranges()[0],
            SelectionRange::new(Offset::new(0), Offset::new(2))
        );
    }

    #[test]
    fn start_beyond_text_length_returns_none() {
        let text = "hi";
        // Selection starts past the text — should produce no matches.
        let sel = Selections::single(SelectionRange::new(Offset::new(100), Offset::new(200)));
        let re = regex(r"\w\+");
        assert!(select_on_matches(&sel, text, &re).is_none());
    }

    #[test]
    fn adjacent_matches_merged_by_normalize() {
        // Pattern "a" on "aaaaa" produces 5 adjacent matches:
        // (0..1),(1..2),(2..3),(3..4),(4..5).
        // normalize() merges them all into a single (0..5).
        let text = "aaaaa";
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(text.len())));
        let re = regex("a");
        let result = select_on_matches(&sel, text, &re).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            result.ranges()[0],
            SelectionRange::new(Offset::new(0), Offset::new(5))
        );
    }

    #[test]
    fn non_adjacent_single_char_matches_stay_separate() {
        // Pattern "a" on "a b a" produces matches at (0..1) and (4..5).
        // These are not adjacent, so normalize() keeps them separate.
        let text = "a b a";
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(text.len())));
        let re = regex("a");
        let result = select_on_matches(&sel, text, &re).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(
            result.ranges()[0],
            SelectionRange::new(Offset::new(0), Offset::new(1))
        );
        assert_eq!(
            result.ranges()[1],
            SelectionRange::new(Offset::new(4), Offset::new(5))
        );
    }

    #[test]
    fn all_ranges_no_matches_returns_none() {
        let text = "aaa bbb ccc";
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(0), Offset::new(3)),
            SelectionRange::new(Offset::new(4), Offset::new(7)),
            SelectionRange::new(Offset::new(8), Offset::new(11)),
        ]);
        let sel = Selections::new(ranges, 0);
        // Search for digits — none in any range.
        let re = regex(r"\d\+");
        assert!(select_on_matches(&sel, text, &re).is_none());
    }

    #[test]
    fn multiline_with_anchors() {
        let text = "line1\nline2\nline3\n";
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(text.len())));
        // Match whole lines using `^...$` — vim regex, `\_.*` won't work here,
        // but `^` and `$` should match at line boundaries.
        let re = regex(r"^line\d$");
        let result = select_on_matches(&sel, text, &re).unwrap();
        assert_eq!(result.len(), 3);
    }

    // ── split_on_matches tests ───────────────────────────────────────

    #[test]
    fn split_basic() {
        // "hello,world,foo" split on `,` → 3 gap ranges.
        let text = "hello,world,foo";
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(text.len())));
        let re = regex(",");
        let result = split_on_matches(&sel, text, &re);
        assert_eq!(result.len(), 3);
        assert_eq!(
            result.ranges()[0],
            SelectionRange::new(Offset::new(0), Offset::new(5))
        );
        assert_eq!(
            result.ranges()[1],
            SelectionRange::new(Offset::new(6), Offset::new(11))
        );
        assert_eq!(
            result.ranges()[2],
            SelectionRange::new(Offset::new(12), Offset::new(15))
        );
    }

    #[test]
    fn split_no_matches() {
        // No commas → original range passes through.
        let text = "hello world foo";
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(text.len())));
        let re = regex(",");
        let result = split_on_matches(&sel, text, &re);
        assert_eq!(result.len(), 1);
        assert_eq!(
            result.ranges()[0],
            SelectionRange::new(Offset::new(0), Offset::new(text.len()))
        );
    }

    #[test]
    fn split_multiple_ranges() {
        // Two ranges each with splits.
        let text = "a,b c,d";
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(0), Offset::new(3)), // "a,b"
            SelectionRange::new(Offset::new(4), Offset::new(7)), // "c,d"
        ]);
        let sel = Selections::new(ranges, 0);
        let re = regex(",");
        let result = split_on_matches(&sel, text, &re);
        assert_eq!(result.len(), 4);
        // From "a,b": "a" and "b"
        assert_eq!(
            result.ranges()[0],
            SelectionRange::new(Offset::new(0), Offset::new(1))
        );
        assert_eq!(
            result.ranges()[1],
            SelectionRange::new(Offset::new(2), Offset::new(3))
        );
        // From "c,d": "c" and "d"
        assert_eq!(
            result.ranges()[2],
            SelectionRange::new(Offset::new(4), Offset::new(5))
        );
        assert_eq!(
            result.ranges()[3],
            SelectionRange::new(Offset::new(6), Offset::new(7))
        );
    }

    #[test]
    fn split_zero_width_passthrough() {
        let text = "hello,world";
        let sel = Selections::single(SelectionRange::new(Offset::new(3), Offset::new(3)));
        let re = regex(",");
        let result = split_on_matches(&sel, text, &re);
        assert_eq!(result.len(), 1);
        assert_eq!(
            result.ranges()[0],
            SelectionRange::new(Offset::new(3), Offset::new(3))
        );
    }

    #[test]
    fn split_leading_match() {
        // Match at very start of range — no leading gap, only trailing.
        let text = ",hello,world";
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(text.len())));
        let re = regex(",");
        let result = split_on_matches(&sel, text, &re);
        // Commas at 0 and 6. Gaps: [1..6] "hello", [7..12] "world".
        assert_eq!(result.len(), 2);
        assert_eq!(
            result.ranges()[0],
            SelectionRange::new(Offset::new(1), Offset::new(6))
        );
        assert_eq!(
            result.ranges()[1],
            SelectionRange::new(Offset::new(7), Offset::new(12))
        );
    }

    // ── keep_matching / remove_matching tests ────────────────────────

    #[test]
    fn keep_matching_basic() {
        // 4 word selections, keep those matching "h" or "f" at start.
        let text = "hello world foo bar";
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(0), Offset::new(5)), // "hello"
            SelectionRange::new(Offset::new(6), Offset::new(11)), // "world"
            SelectionRange::new(Offset::new(12), Offset::new(15)), // "foo"
            SelectionRange::new(Offset::new(16), Offset::new(19)), // "bar"
        ]);
        let sel = Selections::new(ranges, 0);
        let re = regex(r"^[hf]");
        let result = keep_matching(&sel, text, &re).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(
            result.ranges()[0],
            SelectionRange::new(Offset::new(0), Offset::new(5))
        );
        assert_eq!(
            result.ranges()[1],
            SelectionRange::new(Offset::new(12), Offset::new(15))
        );
    }

    #[test]
    fn keep_matching_none() {
        let text = "hello world";
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(5)));
        let re = regex(r"\d\+");
        assert!(keep_matching(&sel, text, &re).is_none());
    }

    #[test]
    fn remove_matching_basic() {
        // Remove words starting with "w".
        let text = "hello world foo bar";
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(0), Offset::new(5)), // "hello"
            SelectionRange::new(Offset::new(6), Offset::new(11)), // "world"
            SelectionRange::new(Offset::new(12), Offset::new(15)), // "foo"
            SelectionRange::new(Offset::new(16), Offset::new(19)), // "bar"
        ]);
        let sel = Selections::new(ranges, 0);
        let re = regex(r"^w");
        let result = remove_matching(&sel, text, &re).unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(
            result.ranges()[0],
            SelectionRange::new(Offset::new(0), Offset::new(5))
        );
        assert_eq!(
            result.ranges()[1],
            SelectionRange::new(Offset::new(12), Offset::new(15))
        );
        assert_eq!(
            result.ranges()[2],
            SelectionRange::new(Offset::new(16), Offset::new(19))
        );
    }

    #[test]
    fn remove_matching_all() {
        // All match → None.
        let text = "hello";
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(5)));
        let re = regex(r"\w\+");
        assert!(remove_matching(&sel, text, &re).is_none());
    }

    #[test]
    fn keep_matching_zero_width() {
        // Zero-width never matches for keep.
        let text = "hello";
        let sel = Selections::single(SelectionRange::new(Offset::new(2), Offset::new(2)));
        let re = regex(".");
        assert!(keep_matching(&sel, text, &re).is_none());
    }

    // ── trim / collapse / flip / ensure_forward tests ────────────────

    #[test]
    fn trim_basic() {
        let text = "  hello  ";
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(text.len())));
        let result = trim_whitespace(&sel, text).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            result.ranges()[0],
            SelectionRange::new(Offset::new(2), Offset::new(7))
        );
    }

    #[test]
    fn trim_all_whitespace() {
        let text = "   ";
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(text.len())));
        assert!(trim_whitespace(&sel, text).is_none());
    }

    #[test]
    fn collapse_basic() {
        // Forward selection [2, 7) → cursor at head (7).
        let sel = Selections::single(SelectionRange::new(Offset::new(2), Offset::new(7)));
        let result = collapse(&sel);
        assert_eq!(result.len(), 1);
        assert!(result.ranges()[0].is_collapsed());
        assert_eq!(result.ranges()[0].head(), Offset::new(7));
        assert_eq!(result.ranges()[0].anchor(), Offset::new(7));
    }

    #[test]
    fn flip_basic() {
        // Forward [2, 7) → backward [7, 2).
        let sel = Selections::single(SelectionRange::new(Offset::new(2), Offset::new(7)));
        let result = flip(&sel);
        assert_eq!(result.len(), 1);
        assert_eq!(result.ranges()[0].anchor(), Offset::new(7));
        assert_eq!(result.ranges()[0].head(), Offset::new(2));
        assert!(!result.ranges()[0].is_forward());
    }

    #[test]
    fn ensure_forward_basic() {
        // Backward [7, 2) → forward [2, 7).
        let sel = Selections::single(SelectionRange::new(Offset::new(7), Offset::new(2)));
        assert!(!sel.ranges()[0].is_forward());
        let result = ensure_forward(&sel);
        assert_eq!(result.len(), 1);
        assert!(result.ranges()[0].is_forward());
        assert_eq!(result.ranges()[0].anchor(), Offset::new(2));
        assert_eq!(result.ranges()[0].head(), Offset::new(7));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Edge-case tests (split, keep/remove, trim, collapse, flip,
    //  ensure_forward)
    // ══════════════════════════════════════════════════════════════════

    // ── split_on_matches edge cases ─────────────────────────────────

    #[test]
    fn split_trailing_match() {
        // Match at the very end of range — no trailing gap.
        let text = "hello,";
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(text.len())));
        let re = regex(",");
        let result = split_on_matches(&sel, text, &re);
        // Comma at 5. Gap: [0..5] "hello". No trailing gap.
        assert_eq!(result.len(), 1);
        assert_eq!(
            result.ranges()[0],
            SelectionRange::new(Offset::new(0), Offset::new(5))
        );
    }

    #[test]
    fn split_full_range_match() {
        // The entire range IS the match — no gaps at all → pass through.
        let text = ",,,,";
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(text.len())));
        let re = regex(",");
        let result = split_on_matches(&sel, text, &re);
        // Every byte is a comma. Gaps are empty → falls through to original.
        assert_eq!(result.len(), 1);
        assert_eq!(
            result.ranges()[0],
            SelectionRange::new(Offset::new(0), Offset::new(4))
        );
    }

    #[test]
    fn split_adjacent_matches() {
        // Two adjacent commas — no gap between them.
        let text = "a,,b";
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(text.len())));
        let re = regex(",");
        let result = split_on_matches(&sel, text, &re);
        // Commas at 1 and 2. Gaps: [0..1] "a", [3..4] "b". No gap between commas.
        assert_eq!(result.len(), 2);
        assert_eq!(
            result.ranges()[0],
            SelectionRange::new(Offset::new(0), Offset::new(1))
        );
        assert_eq!(
            result.ranges()[1],
            SelectionRange::new(Offset::new(3), Offset::new(4))
        );
    }

    #[test]
    fn split_multi_char_separator() {
        // Separator is multiple characters wide.
        let text = "a::b::c";
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(text.len())));
        let re = regex("::");
        let result = split_on_matches(&sel, text, &re);
        assert_eq!(result.len(), 3);
        assert_eq!(
            result.ranges()[0],
            SelectionRange::new(Offset::new(0), Offset::new(1))
        );
        assert_eq!(
            result.ranges()[1],
            SelectionRange::new(Offset::new(3), Offset::new(4))
        );
        assert_eq!(
            result.ranges()[2],
            SelectionRange::new(Offset::new(6), Offset::new(7))
        );
    }

    // ── keep_matching edge cases ────────────────────────────────────

    #[test]
    fn keep_matching_all_match() {
        // All selections match → all kept, same count.
        let text = "aaa bbb ccc";
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(0), Offset::new(3)),
            SelectionRange::new(Offset::new(4), Offset::new(7)),
            SelectionRange::new(Offset::new(8), Offset::new(11)),
        ]);
        let sel = Selections::new(ranges, 1);
        let re = regex(r"\w");
        let result = keep_matching(&sel, text, &re).unwrap();
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn keep_matching_none_match() {
        // No selections match → None.
        let text = "aaa bbb ccc";
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(0), Offset::new(3)),
            SelectionRange::new(Offset::new(4), Offset::new(7)),
        ]);
        let sel = Selections::new(ranges, 0);
        let re = regex(r"\d");
        assert!(keep_matching(&sel, text, &re).is_none());
    }

    #[test]
    fn keep_matching_primary_removed_fallback() {
        // Primary is removed — falls back to nearest surviving range.
        let text = "abc 123 xyz";
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(0), Offset::new(3)), // "abc"
            SelectionRange::new(Offset::new(4), Offset::new(7)), // "123"
            SelectionRange::new(Offset::new(8), Offset::new(11)), // "xyz"
        ]);
        // Primary is "123" (index 1).
        let sel = Selections::new(ranges, 1);
        // Keep only alphabetic — "123" is removed.
        let re = regex(r"^[a-z]\+$");
        let result = keep_matching(&sel, text, &re).unwrap();
        assert_eq!(result.len(), 2);
        // Primary should have fallen back (not be out of bounds).
        assert!(result.primary_index() < result.len());
    }

    // ── remove_matching edge cases ──────────────────────────────────

    #[test]
    fn remove_matching_none_match() {
        // No selections match → all survive.
        let text = "aaa bbb";
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(0), Offset::new(3)),
            SelectionRange::new(Offset::new(4), Offset::new(7)),
        ]);
        let sel = Selections::new(ranges, 0);
        let re = regex(r"\d");
        let result = remove_matching(&sel, text, &re).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn remove_matching_all_match_returns_none() {
        // Every selection matches → all removed → None.
        let text = "abc def";
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(0), Offset::new(3)),
            SelectionRange::new(Offset::new(4), Offset::new(7)),
        ]);
        let sel = Selections::new(ranges, 0);
        let re = regex(r"\w");
        assert!(remove_matching(&sel, text, &re).is_none());
    }

    #[test]
    fn remove_matching_zero_width_survives() {
        // Zero-width ranges cannot match, so they survive removal.
        let text = "hello";
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(0), Offset::new(5)), // "hello" — matches
            SelectionRange::new(Offset::new(2), Offset::new(2)), // zero-width — survives
        ]);
        let sel = Selections::new(ranges, 0);
        let re = regex(r"\w");
        let result = remove_matching(&sel, text, &re).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result.ranges()[0].is_collapsed());
    }

    // ── trim_whitespace edge cases ──────────────────────────────────

    #[test]
    fn trim_leading_only() {
        // Leading whitespace but no trailing.
        let text = "  hello";
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(text.len())));
        let result = trim_whitespace(&sel, text).unwrap();
        assert_eq!(
            result.ranges()[0],
            SelectionRange::new(Offset::new(2), Offset::new(7))
        );
    }

    #[test]
    fn trim_trailing_only() {
        // Trailing whitespace but no leading.
        let text = "hello  ";
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(text.len())));
        let result = trim_whitespace(&sel, text).unwrap();
        assert_eq!(
            result.ranges()[0],
            SelectionRange::new(Offset::new(0), Offset::new(5))
        );
    }

    #[test]
    fn trim_tabs_and_mixed_whitespace() {
        // Mix of tabs, spaces, and a newline.
        let text = "\t \nhello\t \n";
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(text.len())));
        let result = trim_whitespace(&sel, text).unwrap();
        assert_eq!(
            result.ranges()[0],
            SelectionRange::new(Offset::new(3), Offset::new(8))
        );
    }

    #[test]
    fn trim_no_whitespace_unchanged() {
        // No whitespace at all — range unchanged.
        let text = "hello";
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(text.len())));
        let result = trim_whitespace(&sel, text).unwrap();
        assert_eq!(
            result.ranges()[0],
            SelectionRange::new(Offset::new(0), Offset::new(5))
        );
    }

    #[test]
    fn trim_mixed_ranges_some_all_whitespace() {
        // Multiple ranges: one all-whitespace, one with content.
        let text = "   hello   ";
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(0), Offset::new(3)), // "   " — all whitespace
            SelectionRange::new(Offset::new(3), Offset::new(11)), // "hello   " — has content
        ]);
        let sel = Selections::new(ranges, 0);
        let result = trim_whitespace(&sel, text).unwrap();
        // Only the second range survives, trimmed to "hello".
        assert_eq!(result.len(), 1);
        assert_eq!(
            result.ranges()[0],
            SelectionRange::new(Offset::new(3), Offset::new(8))
        );
    }

    #[test]
    fn trim_zero_width_removed() {
        // Zero-width range is treated as all-whitespace → filtered out.
        let text = "hello";
        let sel = Selections::single(SelectionRange::new(Offset::new(2), Offset::new(2)));
        assert!(trim_whitespace(&sel, text).is_none());
    }

    // ── collapse edge cases ─────────────────────────────────────────

    #[test]
    fn collapse_backward_range() {
        // Backward selection [7, 2) → collapses to cursor at head (2).
        let sel = Selections::single(SelectionRange::new(Offset::new(7), Offset::new(2)));
        assert!(!sel.ranges()[0].is_forward());
        let result = collapse(&sel);
        assert_eq!(result.len(), 1);
        assert!(result.ranges()[0].is_collapsed());
        assert_eq!(result.ranges()[0].head(), Offset::new(2));
    }

    #[test]
    fn collapse_multi_range() {
        // Multiple ranges all collapsed.
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(0), Offset::new(5)),
            SelectionRange::new(Offset::new(10), Offset::new(15)),
            SelectionRange::new(Offset::new(20), Offset::new(25)),
        ]);
        let sel = Selections::new(ranges, 1);
        let result = collapse(&sel);
        assert_eq!(result.len(), 3);
        for r in result.ranges() {
            assert!(r.is_collapsed());
        }
        assert_eq!(result.ranges()[0].head(), Offset::new(5));
        assert_eq!(result.ranges()[1].head(), Offset::new(15));
        assert_eq!(result.ranges()[2].head(), Offset::new(25));
    }

    #[test]
    fn collapse_already_collapsed() {
        // Already collapsed → no change.
        let sel = Selections::single(SelectionRange::insert_cursor(Offset::new(3)));
        let result = collapse(&sel);
        assert!(result.ranges()[0].is_collapsed());
        assert_eq!(result.ranges()[0].head(), Offset::new(3));
    }

    // ── flip edge cases ─────────────────────────────────────────────

    #[test]
    fn flip_already_backward() {
        // Backward [7, 2) → flipped to forward [2, 7).
        let sel = Selections::single(SelectionRange::new(Offset::new(7), Offset::new(2)));
        let result = flip(&sel);
        assert!(result.ranges()[0].is_forward());
        assert_eq!(result.ranges()[0].anchor(), Offset::new(2));
        assert_eq!(result.ranges()[0].head(), Offset::new(7));
    }

    #[test]
    fn flip_zero_width() {
        // Zero-width [3, 3) → flipped is still [3, 3).
        let sel = Selections::single(SelectionRange::insert_cursor(Offset::new(3)));
        let result = flip(&sel);
        assert!(result.ranges()[0].is_collapsed());
        assert_eq!(result.ranges()[0].head(), Offset::new(3));
    }

    #[test]
    fn flip_double_flip_roundtrip() {
        // Flipping twice returns to original.
        let sel = Selections::single(SelectionRange::new(Offset::new(2), Offset::new(7)));
        let flipped_once = flip(&sel);
        let flipped_twice = flip(&flipped_once);
        assert_eq!(flipped_twice.ranges()[0].anchor(), Offset::new(2));
        assert_eq!(flipped_twice.ranges()[0].head(), Offset::new(7));
    }

    // ── ensure_forward edge cases ───────────────────────────────────

    #[test]
    fn ensure_forward_already_forward() {
        // Forward selection stays unchanged.
        let sel = Selections::single(SelectionRange::new(Offset::new(2), Offset::new(7)));
        let result = ensure_forward(&sel);
        assert_eq!(result.ranges()[0].anchor(), Offset::new(2));
        assert_eq!(result.ranges()[0].head(), Offset::new(7));
    }

    #[test]
    fn ensure_forward_mixed_directions() {
        // Mix of forward and backward ranges — all become forward.
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(0), Offset::new(5)), // forward
            SelectionRange::new(Offset::new(15), Offset::new(10)), // backward
            SelectionRange::new(Offset::new(20), Offset::new(25)), // forward
        ]);
        let sel = Selections::new(ranges, 0);
        let result = ensure_forward(&sel);
        for r in result.ranges() {
            assert!(r.is_forward(), "range {:?} should be forward", r);
        }
        // Backward range [15, 10) became [10, 15).
        assert_eq!(result.ranges()[1].anchor(), Offset::new(10));
        assert_eq!(result.ranges()[1].head(), Offset::new(15));
    }

    #[test]
    fn ensure_forward_zero_width() {
        // Zero-width is already forward (anchor == head) — unchanged.
        let sel = Selections::single(SelectionRange::insert_cursor(Offset::new(5)));
        let result = ensure_forward(&sel);
        assert!(result.ranges()[0].is_forward());
        assert!(result.ranges()[0].is_collapsed());
    }
}
