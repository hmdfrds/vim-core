//! Subword motions: camelCase and snake_case boundary navigation.
//!
//! Subword motions move the cursor across word sub-boundaries that
//! `w`/`b`/`e` don't recognize. For example, in `camelCaseWord`,
//! subword-forward stops at `C` and `W`, while regular `w` skips
//! the entire identifier.
//!
//! # Boundary detection
//!
//! A subword boundary is detected at:
//! - Transition from lowercase to uppercase (`aB` — boundary before `B`)
//! - Transition after a run of uppercase before lowercase (`ABc` — boundary before `c`)
//! - Configured separator characters (boundary after separator)
//! - Transition between alpha and digit (`a1`, `1a`)
//! - Start/end of identifier (transition from/to non-word characters)
//!
//! Case boundaries and acronym detection are controlled by [`SubwordConfig`].

use super::types::{MotionContext, MotionResult};
use crate::commands::helpers::{char_at, next_char_boundary, prev_char_boundary};
use crate::primitives::{Offset, SubwordConfig};

/// Move to the start of the next subword.
pub fn subword_forward(ctx: &MotionContext<'_>) -> MotionResult {
    let text = ctx.text;
    if text.is_empty() {
        return MotionResult::Position(Offset::new(0));
    }

    let config = ctx.options.subword_config();
    let mut pos = ctx.cursor.get();
    for _ in 0..ctx.count {
        pos = find_next_subword_start(text, pos, config);
        if pos >= text.len() {
            pos = last_valid_offset(text, ctx.inclusive_end);
            break;
        }
    }

    MotionResult::Position(Offset::new(
        pos.min(last_valid_offset(text, ctx.inclusive_end)),
    ))
}

/// Move to the start of the previous subword.
pub fn subword_backward(ctx: &MotionContext<'_>) -> MotionResult {
    let text = ctx.text;
    if text.is_empty() {
        return MotionResult::Position(Offset::new(0));
    }

    let config = ctx.options.subword_config();
    let mut pos = ctx.cursor.get();
    for _ in 0..ctx.count {
        pos = find_prev_subword_start(text, pos, config);
    }

    MotionResult::Position(Offset::new(pos))
}

/// Move to the end of the previous subword.
pub fn subword_end_backward(ctx: &MotionContext<'_>) -> MotionResult {
    let text = ctx.text;
    if text.is_empty() {
        return MotionResult::Position(Offset::new(0));
    }

    let config = ctx.options.subword_config();
    let mut pos = ctx.cursor.get();
    for _ in 0..ctx.count {
        let new_pos = find_prev_subword_end(text, pos, config);
        if new_pos == pos {
            break;
        }
        pos = new_pos;
    }

    MotionResult::Position(Offset::new(pos))
}

/// Move to the end of the current/next subword.
pub fn subword_end(ctx: &MotionContext<'_>) -> MotionResult {
    let text = ctx.text;
    if text.is_empty() {
        return MotionResult::Position(Offset::new(0));
    }

    let config = ctx.options.subword_config();
    let mut pos = ctx.cursor.get();
    for _ in 0..ctx.count {
        pos = find_subword_end(text, pos, config);
        if pos >= text.len() {
            break;
        }
    }

    let max = if text.is_empty() {
        0
    } else {
        last_valid_offset(text, false)
    };
    MotionResult::Position(Offset::new(pos.min(max)))
}

// ─── Boundary classification ────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubwordClass {
    Lower,
    Upper,
    Digit,
    Separator,
    NonWord,
}

pub(crate) fn classify_subword(c: char, config: &SubwordConfig) -> SubwordClass {
    if config.is_separator(c) {
        SubwordClass::Separator
    } else if c.is_ascii_uppercase() || c.is_uppercase() {
        SubwordClass::Upper
    } else if c.is_ascii_lowercase() || c.is_lowercase() {
        SubwordClass::Lower
    } else if c.is_ascii_digit() || c.is_numeric() {
        SubwordClass::Digit
    } else {
        SubwordClass::NonWord
    }
}

/// Check if the char at `pos` starts a new subword following an uppercase char.
///
/// Detects the acronym→word boundary: in "XMLParser", `P` is a boundary
/// because it's uppercase followed by lowercase.
fn is_acronym_end_boundary(
    text: &str,
    pos: usize,
    cur_class: SubwordClass,
    config: &SubwordConfig,
) -> bool {
    if cur_class != SubwordClass::Upper {
        return false;
    }
    let next_pos = next_char_boundary(text, pos);
    next_pos < text.len()
        && char_at(text, next_pos)
            .is_some_and(|n| classify_subword(n, config) == SubwordClass::Lower)
}

/// Check if position `pos` is a subword boundary (start of a new subword).
pub(crate) fn is_subword_start(text: &str, pos: usize, config: &SubwordConfig) -> bool {
    let Some(cur) = char_at(text, pos) else {
        return false;
    };
    let cur_class = classify_subword(cur, config);

    if cur_class == SubwordClass::NonWord {
        return false;
    }
    if pos == 0 {
        return cur_class != SubwordClass::Separator;
    }

    let prev_pos = prev_char_boundary(text, pos);
    let Some(prev) = char_at(text, prev_pos) else {
        return true;
    };
    let prev_class = classify_subword(prev, config);

    // After separator or non-word → boundary
    if matches!(prev_class, SubwordClass::Separator | SubwordClass::NonWord)
        && !matches!(cur_class, SubwordClass::Separator | SubwordClass::NonWord)
    {
        return true;
    }

    // lower → Upper (camelCase boundary)
    if config.detect_case_boundaries
        && prev_class == SubwordClass::Lower
        && cur_class == SubwordClass::Upper
    {
        return true;
    }

    // Upper → Upper + Lower (acronym end: "XMLParser" → boundary before 'P')
    if config.detect_acronyms
        && prev_class == SubwordClass::Upper
        && is_acronym_end_boundary(text, pos, cur_class, config)
    {
        return true;
    }

    // Alpha ↔ Digit transitions
    let prev_is_alpha = matches!(prev_class, SubwordClass::Lower | SubwordClass::Upper);
    let cur_is_alpha = matches!(cur_class, SubwordClass::Lower | SubwordClass::Upper);
    (prev_is_alpha && cur_class == SubwordClass::Digit)
        || (prev_class == SubwordClass::Digit && cur_is_alpha)
}

// ─── Core algorithms ────────────────────────────────────────────────────

/// Find the start of the next subword after `start`.
pub(crate) fn find_next_subword_start(text: &str, start: usize, config: &SubwordConfig) -> usize {
    if start >= text.len() {
        return text.len();
    }

    let mut pos = next_char_boundary(text, start);

    // Skip through remaining chars of current subword
    while pos < text.len() {
        if is_subword_start(text, pos, config) {
            return pos;
        }
        // Also stop when transitioning from word to non-word (skip ws/punct)
        let cur = char_at(text, pos);
        if !cur.is_some_and(|c| classify_subword(c, config) != SubwordClass::NonWord) {
            // Skip non-word characters
            while pos < text.len() {
                if let Some(c) = char_at(text, pos) {
                    let class = classify_subword(c, config);
                    if class != SubwordClass::NonWord && class != SubwordClass::Separator {
                        return pos;
                    }
                    if class == SubwordClass::Separator {
                        // Skip separator, next word char is the boundary
                        pos = next_char_boundary(text, pos);
                        continue;
                    }
                }
                pos = next_char_boundary(text, pos);
            }
            return pos;
        }
        pos = next_char_boundary(text, pos);
    }

    text.len()
}

/// Find the start of the previous subword before `start`.
pub(crate) fn find_prev_subword_start(text: &str, start: usize, config: &SubwordConfig) -> usize {
    if start == 0 || text.is_empty() {
        return 0;
    }

    let mut pos = prev_char_boundary(text, start);

    // Skip non-word characters backward
    while pos > 0 {
        if let Some(c) = char_at(text, pos) {
            let class = classify_subword(c, config);
            if class != SubwordClass::NonWord && class != SubwordClass::Separator {
                break;
            }
        }
        pos = prev_char_boundary(text, pos);
    }

    // Now find the start of this subword by scanning backward
    loop {
        if is_subword_start(text, pos, config) {
            return pos;
        }
        if pos == 0 {
            return 0;
        }
        pos = prev_char_boundary(text, pos);
    }
}

/// Find the end of the current/next subword from `start`.
pub(crate) fn find_subword_end(text: &str, start: usize, config: &SubwordConfig) -> usize {
    if start >= text.len() {
        return last_valid_offset(text, false);
    }

    // Step forward one character first
    let mut pos = next_char_boundary(text, start);

    // Skip non-word characters
    while pos < text.len() {
        if let Some(c) = char_at(text, pos) {
            let class = classify_subword(c, config);
            if class != SubwordClass::NonWord && class != SubwordClass::Separator {
                break;
            }
        }
        pos = next_char_boundary(text, pos);
    }

    if pos >= text.len() {
        return last_valid_offset(text, false);
    }

    // Advance until the next boundary
    loop {
        let next = next_char_boundary(text, pos);
        if next >= text.len() {
            return pos;
        }
        if is_subword_start(text, next, config) {
            return pos;
        }
        // Stop if next char is non-word
        if let Some(c) = char_at(text, next) {
            if classify_subword(c, config) == SubwordClass::NonWord
                || classify_subword(c, config) == SubwordClass::Separator
            {
                return pos;
            }
        }
        pos = next;
    }
}

/// Find the end of the previous subword before `start`.
pub(crate) fn find_prev_subword_end(text: &str, start: usize, config: &SubwordConfig) -> usize {
    if start == 0 || text.is_empty() {
        return 0;
    }

    // Step back one character
    let mut pos = prev_char_boundary(text, start);

    // Skip whitespace and separators backward
    while pos > 0 {
        if let Some(c) = char_at(text, pos) {
            let class = classify_subword(c, config);
            if class != SubwordClass::NonWord && class != SubwordClass::Separator {
                break;
            }
        }
        pos = prev_char_boundary(text, pos);
    }

    // Check if we landed on whitespace/separator at pos 0
    if let Some(c) = char_at(text, pos) {
        let class = classify_subword(c, config);
        if class == SubwordClass::NonWord || class == SubwordClass::Separator {
            return 0;
        }
    }

    // We're now inside a subword. If this position is a subword start,
    // we need to go back one more to the previous subword's end.
    if is_subword_start(text, pos, config) {
        if pos == 0 {
            return 0;
        }
        pos = prev_char_boundary(text, pos);

        // Skip whitespace/separators again
        while pos > 0 {
            if let Some(c) = char_at(text, pos) {
                let class = classify_subword(c, config);
                if class != SubwordClass::NonWord && class != SubwordClass::Separator {
                    break;
                }
            }
            pos = prev_char_boundary(text, pos);
        }
    } else {
        // We're in the middle of a subword. Find its start,
        // then the end of the previous subword is start - 1.
        let subword_start = find_prev_subword_start(text, next_char_boundary(text, pos), config);
        if subword_start == 0 {
            return 0;
        }
        pos = prev_char_boundary(text, subword_start);

        // Skip whitespace/separators
        while pos > 0 {
            if let Some(c) = char_at(text, pos) {
                let class = classify_subword(c, config);
                if class != SubwordClass::NonWord && class != SubwordClass::Separator {
                    break;
                }
            }
            pos = prev_char_boundary(text, pos);
        }
    }

    pos
}

/// Last valid cursor position.
const fn last_valid_offset(text: &str, inclusive_end: bool) -> usize {
    if text.is_empty() {
        return 0;
    }
    if inclusive_end {
        return text.len();
    }
    let mut pos = text.len() - 1;
    while pos > 0 && !text.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> SubwordConfig {
        SubwordConfig::default()
    }

    #[test]
    fn camel_case_boundaries() {
        let text = "camelCaseWord";
        let cfg = default_config();
        // Boundaries at: 0(c), 5(C), 9(W)
        assert!(is_subword_start(text, 0, &cfg));
        assert!(!is_subword_start(text, 1, &cfg));
        assert!(is_subword_start(text, 5, &cfg)); // C
        assert!(is_subword_start(text, 9, &cfg)); // W
    }

    #[test]
    fn snake_case_boundaries() {
        let text = "snake_case_word";
        let cfg = default_config();
        // Boundaries at: 0(s), 6(c), 11(w) — after each _
        assert!(is_subword_start(text, 0, &cfg));
        assert!(is_subword_start(text, 6, &cfg)); // c after _
        assert!(is_subword_start(text, 11, &cfg)); // w after _
    }

    #[test]
    fn acronym_boundaries() {
        let text = "XMLParser";
        let cfg = default_config();
        // Boundaries at: 0(X), 3(P) — "XML" is one unit, "Parser" starts at P
        assert!(is_subword_start(text, 0, &cfg));
        assert!(!is_subword_start(text, 1, &cfg)); // M in XML
        assert!(!is_subword_start(text, 2, &cfg)); // L in XML
        assert!(is_subword_start(text, 3, &cfg)); // P in Parser
    }

    #[test]
    fn digit_transitions() {
        let text = "var123name";
        let cfg = default_config();
        // Boundaries at: 0(v), 3(1), 6(n)
        assert!(is_subword_start(text, 0, &cfg));
        assert!(is_subword_start(text, 3, &cfg)); // 1
        assert!(is_subword_start(text, 6, &cfg)); // n
    }

    #[test]
    fn forward_camel_case() {
        let text = "camelCaseWord";
        let cfg = default_config();
        assert_eq!(find_next_subword_start(text, 0, &cfg), 5); // -> C
        assert_eq!(find_next_subword_start(text, 5, &cfg), 9); // -> W
    }

    #[test]
    fn forward_snake_case() {
        let text = "snake_case_word";
        let cfg = default_config();
        assert_eq!(find_next_subword_start(text, 0, &cfg), 6); // -> c (after _)
        assert_eq!(find_next_subword_start(text, 6, &cfg), 11); // -> w (after _)
    }

    #[test]
    fn backward_camel_case() {
        let text = "camelCaseWord";
        let cfg = default_config();
        assert_eq!(find_prev_subword_start(text, 13, &cfg), 9); // <- W
        assert_eq!(find_prev_subword_start(text, 9, &cfg), 5); // <- C
        assert_eq!(find_prev_subword_start(text, 5, &cfg), 0); // <- c
    }

    #[test]
    fn end_camel_case() {
        let text = "camelCaseWord";
        let cfg = default_config();
        assert_eq!(find_subword_end(text, 0, &cfg), 4); // l (end of "camel")
        assert_eq!(find_subword_end(text, 5, &cfg), 8); // e (end of "Case")
    }

    #[test]
    fn forward_with_spaces() {
        let text = "foo barBaz";
        let cfg = default_config();
        assert_eq!(find_next_subword_start(text, 0, &cfg), 4); // -> b
        assert_eq!(find_next_subword_start(text, 4, &cfg), 7); // -> B
    }

    #[test]
    fn motion_forward_with_count() {
        let text = "camelCaseWord";
        let opts = crate::primitives::VimOptions::default();
        let ctx = MotionContext::new(text, Offset::new(0), 2, &opts);
        let result = subword_forward(&ctx);
        // count=2: skip to 2nd subword boundary = 9 (W)
        assert_eq!(result, MotionResult::Position(Offset::new(9)));
    }

    #[test]
    fn motion_backward_with_count() {
        let text = "camelCaseWord";
        let opts = crate::primitives::VimOptions::default();
        let ctx = MotionContext::new(text, Offset::new(12), 2, &opts);
        let result = subword_backward(&ctx);
        // count=2 from end: back to C(5)
        assert_eq!(result, MotionResult::Position(Offset::new(5)));
    }

    #[test]
    fn motion_end_basic() {
        let text = "camelCaseWord";
        let opts = crate::primitives::VimOptions::default();
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts);
        let result = subword_end(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(4)));
    }

    #[test]
    fn empty_text() {
        let text = "";
        let opts = crate::primitives::VimOptions::default();
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts);
        assert_eq!(
            subword_forward(&ctx),
            MotionResult::Position(Offset::new(0))
        );
        assert_eq!(
            subword_backward(&ctx),
            MotionResult::Position(Offset::new(0))
        );
        assert_eq!(subword_end(&ctx), MotionResult::Position(Offset::new(0)));
    }

    // ─── SubwordConfig-specific tests ───────────────────────────────────

    #[test]
    fn case_boundaries_disabled() {
        let text = "camelCaseWord";
        let cfg = SubwordConfig {
            detect_case_boundaries: false,
            ..SubwordConfig::default()
        };
        // With case boundaries off, "camelCaseWord" is one subword
        assert!(is_subword_start(text, 0, &cfg));
        assert!(!is_subword_start(text, 5, &cfg)); // C no longer a boundary
        assert!(!is_subword_start(text, 9, &cfg)); // W no longer a boundary
    }

    #[test]
    fn acronym_detection_disabled() {
        let text = "XMLParser";
        let cfg = SubwordConfig {
            detect_acronyms: false,
            ..SubwordConfig::default()
        };
        // With acronym detection off, "XMLParser" has no boundary at P
        assert!(is_subword_start(text, 0, &cfg));
        // P is still uppercase after uppercase — but without acronym detection,
        // the Upper→Upper+Lower rule doesn't fire. However, this is still not
        // a lower→upper boundary (both are Upper class).
        assert!(!is_subword_start(text, 3, &cfg));
    }

    #[test]
    fn custom_separator() {
        let text = "foo/bar.baz";
        let cfg = SubwordConfig {
            separators: compact_str::CompactString::new("/"),
            detect_case_boundaries: true,
            detect_acronyms: true,
        };
        assert!(is_subword_start(text, 0, &cfg)); // f
        assert!(is_subword_start(text, 4, &cfg)); // b (after / separator)
        assert!(is_subword_start(text, 8, &cfg)); // b (after . which is NonWord)
    }

    #[test]
    fn dot_separator_in_default() {
        let text = "foo.bar";
        let cfg = default_config();
        // "." is a default separator
        assert!(is_subword_start(text, 0, &cfg)); // f
        assert!(is_subword_start(text, 4, &cfg)); // b after .
    }

    #[test]
    fn hyphen_separator_in_default() {
        let text = "kebab-case-word";
        let cfg = default_config();
        assert!(is_subword_start(text, 0, &cfg)); // k
        assert!(is_subword_start(text, 6, &cfg)); // c after -
        assert!(is_subword_start(text, 11, &cfg)); // w after -
    }

    #[test]
    fn forward_with_custom_config() {
        let text = "camelCaseWord";
        let mut opts = crate::primitives::VimOptions::default();
        opts.set_subword_config(SubwordConfig {
            separators: compact_str::CompactString::new("._-"),
            detect_case_boundaries: false,
            detect_acronyms: true,
        });
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts);
        let result = subword_forward(&ctx);
        // With case boundaries disabled, "camelCaseWord" is one subword,
        // so forward from 0 goes to end of text
        assert_eq!(
            result,
            MotionResult::Position(Offset::new(last_valid_offset(text, false)))
        );
    }

    // ─── SubwordEndBackward ────────────────────────────────────────────

    #[test]
    fn end_backward_camel_case() {
        let text = "camelCaseWord";
        let cfg = default_config();
        // From end (13) → end of "Case" = 8
        assert_eq!(find_prev_subword_end(text, 13, &cfg), 8);
        // From 8 → end of "camel" = 4
        assert_eq!(find_prev_subword_end(text, 8, &cfg), 4);
        // From 4 → start of doc = 0
        assert_eq!(find_prev_subword_end(text, 4, &cfg), 0);
        // From 0 → stays at 0
        assert_eq!(find_prev_subword_end(text, 0, &cfg), 0);
    }

    #[test]
    fn end_backward_snake_case() {
        let text = "snake_case_word";
        let cfg = default_config();
        assert_eq!(find_prev_subword_end(text, 14, &cfg), 9);
        assert_eq!(find_prev_subword_end(text, 9, &cfg), 4);
    }

    #[test]
    fn end_backward_with_whitespace() {
        let text = "foo barBaz";
        let cfg = default_config();
        assert_eq!(find_prev_subword_end(text, 9, &cfg), 6);
        assert_eq!(find_prev_subword_end(text, 6, &cfg), 2);
    }

    #[test]
    fn end_backward_acronym() {
        let text = "XMLParser";
        let cfg = default_config();
        assert_eq!(find_prev_subword_end(text, 8, &cfg), 2);
        assert_eq!(find_prev_subword_end(text, 2, &cfg), 0);
    }

    #[test]
    fn end_backward_all_caps() {
        let text = "ALLCAPS";
        let cfg = default_config();
        assert_eq!(find_prev_subword_end(text, 6, &cfg), 0);
    }

    #[test]
    fn end_backward_empty() {
        let text = "";
        let cfg = default_config();
        assert_eq!(find_prev_subword_end(text, 0, &cfg), 0);
    }

    #[test]
    fn motion_end_backward_with_count() {
        let text = "camelCaseWord";
        let opts = crate::primitives::VimOptions::default();
        let ctx = MotionContext::new(text, Offset::new(12), 2, &opts);
        let result = subword_end_backward(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(4)));
    }

    #[test]
    fn motion_end_backward_empty() {
        let text = "";
        let opts = crate::primitives::VimOptions::default();
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts);
        assert_eq!(
            subword_end_backward(&ctx),
            MotionResult::Position(Offset::new(0))
        );
    }
}
