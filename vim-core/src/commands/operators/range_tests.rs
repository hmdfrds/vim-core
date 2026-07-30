use super::*;

#[test]
fn test_compute_linewise_range_single_line() {
    let text = "hello\nworld\n";
    let result = compute_linewise_range(text, 0, 1);
    assert_eq!(result.range.start().get(), 0);
    assert_eq!(result.range.end().get(), 6); // "hello\n"
    assert!(result.motion_type.is_line_wise());
}

#[test]
fn test_compute_linewise_range_multiple_lines() {
    let text = "line1\nline2\nline3\n";
    let result = compute_linewise_range(text, 0, 2);
    assert_eq!(result.range.start().get(), 0);
    assert_eq!(result.range.end().get(), 12); // "line1\nline2\n"
}

#[test]
fn test_maybe_promote_to_linewise() {
    // ── Non-exclusive motions are never promoted ──
    let text = "hello\nworld\n";
    let range = Range::from_raw(0, 12);
    let (_, _, promoted) = maybe_promote_to_linewise(text, range, MotionType::CharWise, false);
    assert!(!promoted, "Non-exclusive motion should never promote");

    // ── Already linewise — no change ──
    let range = Range::from_raw(0, 6);
    let (_, _, promoted) = maybe_promote_to_linewise(text, range, MotionType::LineWise, true);
    assert!(!promoted, "Already linewise should not re-promote");

    // ── End NOT at col 0 — no promotion ──
    // Range [0, 3) = "hel", end byte is 'l', not after '\n'
    let range = Range::from_raw(0, 3);
    let (_, _, promoted) = maybe_promote_to_linewise(text, range, MotionType::CharWise, true);
    assert!(!promoted, "End not at col 0 should not promote");

    // ── End at col 0, start at/before first non-blank → LINEWISE ──
    // text = "hello\nworld\n", exclusive range [0, 6) — end lands at col 0 of "world"
    // Start (0) is at first non-blank (no leading whitespace). → Promote to linewise.
    let range = Range::from_raw(0, 6);
    let (r, mt, promoted) = maybe_promote_to_linewise(text, range, MotionType::CharWise, true);
    assert!(
        promoted,
        "Start at first non-blank with end at col 0 should promote"
    );
    assert!(mt.is_line_wise());
    // Should extend to full line boundaries: "hello\n" = [0, 6)
    assert_eq!(r.start().get(), 0);
    assert_eq!(r.end().get(), 6);

    // ── End at col 0, start BEFORE first non-blank (indented) → LINEWISE ──
    let text2 = "  hello\nworld\n";
    // Range [0, 8): start=0, end=8 is col 0 of "world" (text2[7]='\n')
    let range = Range::from_raw(0, 8);
    let (_, mt, promoted) = maybe_promote_to_linewise(text2, range, MotionType::CharWise, true);
    assert!(
        promoted,
        "Start before first non-blank (at col 0 of indented line) should promote"
    );
    assert!(mt.is_line_wise());

    // ── End at col 0, start AFTER first non-blank → back up end (inclusive, not linewise) ──
    let text3 = "  hello\nworld\n";
    // Start at 4 ('l' in "  hello"), first non-blank is at 2 ('h'). 4 > 2 → no promotion.
    // Range [4, 8): end at col 0 of "world".
    let range = Range::from_raw(4, 8);
    let (r, mt, promoted) = maybe_promote_to_linewise(text3, range, MotionType::CharWise, true);
    assert!(
        !promoted,
        "Start after first non-blank should NOT promote to linewise"
    );
    assert!(!mt.is_line_wise());
    // End should be backed up to end of previous line (before the \n at byte 7).
    assert_eq!(
        r.end().get(),
        7,
        "End should be backed up to newline position"
    );

    // ── End at col 0, previous line is empty → still back up ──
    let text4 = "hello\n\nworld\n";
    // text4: h=0 e=1 l=2 l=3 o=4 \n=5 \n=6 w=7 o=8 r=9 l=10 d=11 \n=12
    // Range [3, 7): start=3 ('l' in "hello"), end=7, text4[6]='\n' → end at col 0.
    // Start (3) is after first non-blank (0). Previous line before end is line 1 (empty).
    // Vim still backs up end to newline_pos (6) even for empty previous line.
    let range = Range::from_raw(3, 7);
    let (r, _, promoted) = maybe_promote_to_linewise(text4, range, MotionType::CharWise, true);
    assert!(
        !promoted,
        "Should not promote when start is after first non-blank"
    );
    assert_eq!(
        r.end().get(),
        6,
        "End should back up to newline_pos even for empty previous line"
    );
}

#[test]
fn test_compute_motion_range_word() {
    use crate::dispatch::dispatch_motion;
    let opts = crate::primitives::VimOptions::default();
    let text = "hello world";
    let result = compute_motion_range(
        text,
        0,
        Motion::WordEnd,
        1,
        None,
        None,
        &opts,
        dispatch_motion,
        None,
    );
    assert!(result.is_some());
    let result = result.unwrap();
    // Should cover "hello"
    assert_eq!(result.range.start().get(), 0);
    assert!(result.range.end().get() >= 5);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Exclusive-to-linewise promotion: detailed unit tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_exclusive_linewise_start_at_first_non_blank() {
    // Start is exactly at first non-blank on an unindented line.
    // Exclusive end at col 0 → should promote to linewise.
    let text = "hello\nworld\n";
    let range = Range::from_raw(0, 6); // end at col 0 of "world"
    let (r, mt, promoted) = maybe_promote_to_linewise(text, range, MotionType::CharWise, true);
    assert!(promoted);
    assert!(mt.is_line_wise());
    assert_eq!(r.start().get(), 0);
    assert_eq!(r.end().get(), 6); // "hello\n" — one line
}

#[test]
fn test_exclusive_linewise_start_before_first_non_blank() {
    // Start is at col 0 of a line with leading whitespace (before first non-blank).
    // Exclusive end at col 0 → should promote to linewise.
    let text = "  hello\nworld\n";
    let range = Range::from_raw(0, 8); // end at col 0 of "world"
    let (r, mt, promoted) = maybe_promote_to_linewise(text, range, MotionType::CharWise, true);
    assert!(promoted);
    assert!(mt.is_line_wise());
    assert_eq!(r.start().get(), 0);
    assert_eq!(r.end().get(), 8); // "  hello\n"
}

#[test]
fn test_exclusive_linewise_start_in_whitespace() {
    // Start at col 1 (still before first non-blank at col 2).
    let text = "  hello\nworld\n";
    let range = Range::from_raw(1, 8);
    let (_, mt, promoted) = maybe_promote_to_linewise(text, range, MotionType::CharWise, true);
    assert!(
        promoted,
        "Start in leading whitespace (before first non-blank) should promote"
    );
    assert!(mt.is_line_wise());
}

#[test]
fn test_exclusive_linewise_start_at_first_non_blank_exact() {
    // Start exactly at the first non-blank character.
    let text = "  hello\nworld\n";
    let range = Range::from_raw(2, 8); // start at 'h', first non-blank at 2
    let (_, mt, promoted) = maybe_promote_to_linewise(text, range, MotionType::CharWise, true);
    assert!(
        promoted,
        "Start at exactly the first non-blank should promote"
    );
    assert!(mt.is_line_wise());
}

#[test]
fn test_exclusive_linewise_start_after_first_non_blank_backs_up() {
    // Start is after the first non-blank → should NOT promote, but should back up end.
    let text = "  hello\nworld\n";
    let range = Range::from_raw(3, 8); // start at 'e' (col 3), first non-blank at col 2
    let (r, mt, promoted) = maybe_promote_to_linewise(text, range, MotionType::CharWise, true);
    assert!(!promoted, "Start after first non-blank should NOT promote");
    assert!(!mt.is_line_wise());
    // End should be backed up from 8 to 7 (end of "  hello", the \n position).
    assert_eq!(r.end().get(), 7);
}

#[test]
fn test_exclusive_linewise_multiline_promotion() {
    // Range spans 3 lines, start at col 0, end at col 0 of line 3.
    let text = "line1\nline2\nline3\nline4\n";
    let range = Range::from_raw(0, 18); // end at col 0 of "line4"
    let (r, mt, promoted) = maybe_promote_to_linewise(text, range, MotionType::CharWise, true);
    assert!(promoted);
    assert!(mt.is_line_wise());
    // Should cover lines 0-2: "line1\nline2\nline3\n" = [0, 18)
    assert_eq!(r.start().get(), 0);
    assert_eq!(r.end().get(), 18);
}

#[test]
fn test_exclusive_linewise_multiline_no_promotion() {
    // Range spans 3 lines, but start is after first non-blank.
    let text = "line1\nline2\nline3\nline4\n";
    let range = Range::from_raw(2, 18); // start at 'n' in "line1"
    let (r, mt, promoted) = maybe_promote_to_linewise(text, range, MotionType::CharWise, true);
    assert!(!promoted);
    assert!(!mt.is_line_wise());
    // End backed up to 17 (last char of "line3" before its \n).
    assert_eq!(r.end().get(), 17);
}

#[test]
fn test_exclusive_linewise_empty_prev_line_backs_up() {
    // End at col 0, previous line is empty → still backs up.
    // Vim's coladvance(MAXCOL) on an empty line keeps col 0 but the range
    // should still exclude the line that `end` was pointing at.
    let text = "hello\n\nworld\n";
    // "hello\n\nworld\n": h=0 e=1 l=2 l=3 o=4 \n=5 \n=6 w=7 ...
    // Range [3, 7): start='l' (col 3 > first_non_blank 0), end at col 0 of "world"
    // Previous line is empty (byte 6 is '\n').
    let range = Range::from_raw(3, 7);
    let (r, mt, promoted) = maybe_promote_to_linewise(text, range, MotionType::CharWise, true);
    assert!(!promoted);
    assert!(!mt.is_line_wise());
    // End should be backed up to newline_pos (6) — the start of the empty line.
    assert_eq!(r.end().get(), 6);
}

#[test]
fn test_exclusive_linewise_start_at_col0_empty_prev_line_promotes() {
    // Start at col 0 (= at first non-blank 'h'), end at col 0, previous line is empty.
    // Should promote because start <= first_non_blank, regardless of empty prev line.
    let text = "hello\n\nworld\n";
    let range = Range::from_raw(0, 7); // end at col 0 of "world"
    let (r, mt, promoted) = maybe_promote_to_linewise(text, range, MotionType::CharWise, true);
    assert!(
        promoted,
        "Start at first non-blank should promote even with empty intermediate line"
    );
    assert!(mt.is_line_wise());
    // Linewise extension covers lines 0 and 1 (the empty line): "hello\n\n" = [0, 7)
    assert_eq!(r.start().get(), 0);
    assert_eq!(r.end().get(), 7);
}

#[test]
fn test_exclusive_linewise_end_not_at_col0_noop() {
    // End is NOT at col 0 (not after a '\n') → rule doesn't apply, no change.
    let text = "hello\nworld\n";
    let range = Range::from_raw(0, 3); // end in middle of "hello"
    let (r, mt, promoted) = maybe_promote_to_linewise(text, range, MotionType::CharWise, true);
    assert!(!promoted);
    assert!(!mt.is_line_wise());
    assert_eq!(r, range); // unchanged
}

#[test]
fn test_exclusive_linewise_empty_range_noop() {
    // Empty range (start == end) → nothing to do.
    let text = "hello\nworld\n";
    let range = Range::from_raw(6, 6);
    let (r, _, promoted) = maybe_promote_to_linewise(text, range, MotionType::CharWise, true);
    assert!(!promoted);
    assert_eq!(r, range);
}

#[test]
fn test_exclusive_linewise_eof_promotion() {
    // Range ends at text.len() with text not ending in '\n'.
    // text.len() is NOT preceded by '\n' → rule doesn't trigger.
    let text = "hello\nworld";
    let range = Range::from_raw(0, 11); // end = text.len()
    let (_, _, promoted) = maybe_promote_to_linewise(text, range, MotionType::CharWise, true);
    assert!(
        !promoted,
        "End at EOF without trailing newline should not trigger col-0 rule"
    );
}

#[test]
fn test_exclusive_linewise_eof_with_trailing_newline() {
    // Text ends with '\n', end at text.len() → text[end-1] == '\n' → end at col 0.
    let text = "hello\nworld\n";
    let range = Range::from_raw(0, 12); // end = text.len(), text[11]='\n'
    let (r, mt, promoted) = maybe_promote_to_linewise(text, range, MotionType::CharWise, true);
    assert!(
        promoted,
        "End at EOF with trailing newline should trigger (start at first non-blank)"
    );
    assert!(mt.is_line_wise());
    // extend_to_full_lines on backed_up_end=11 should cover [0, 12).
    assert_eq!(r.start().get(), 0);
    assert_eq!(r.end().get(), 12);
}

#[test]
fn test_exclusive_linewise_tab_indented_start() {
    // Tab-indented line: first non-blank at col 1 (byte offset).
    let text = "\thello\nworld\n";
    // Range [0, 7): start at col 0, first non-blank at byte 1 ('\t' is whitespace).
    let range = Range::from_raw(0, 7);
    let (_, mt, promoted) = maybe_promote_to_linewise(text, range, MotionType::CharWise, true);
    assert!(
        promoted,
        "Start at col 0 before tab-indented content should promote"
    );
    assert!(mt.is_line_wise());
}
