//! Text object seeking: navigate to next/previous instance of any text object.
//!
//! `]w` = next word boundary, `]"` = next quote pair, `[S` = previous subword, etc.
//! Works by scanning character positions until a text object computation returns
//! a DIFFERENT range than the one at the current cursor.

use super::types::MotionResult;
use crate::commands::helpers::{next_char_boundary, prev_char_boundary};
use crate::primitives::{Direction, Offset};

/// Maximum characters to scan before giving up.
const MAX_SEEK_DISTANCE: usize = 10_000;

/// Seek to the next/previous instance of a text object.
///
/// From the cursor, scans forward/backward to find the next position where
/// the text object resolver returns a range different from the current one.
/// Count support: `3]w` seeks to the 3rd next word boundary.
///
/// The `resolve` closure takes a byte offset and returns `Some((start, end))`
/// for the text object at that position, or `None` if no text object exists there.
/// This is provided by the dispatch layer to avoid architecture violations.
pub fn seek_text_object(
    text: &str,
    cursor: usize,
    count: u32,
    direction: Direction,
    resolve: impl Fn(usize) -> Option<(usize, usize)>,
) -> MotionResult {
    if text.is_empty() {
        return MotionResult::Error;
    }

    let target_count = count;

    // Get current text object at cursor (Around scope for outer boundary).
    let current_range = resolve(cursor);

    let mut found_count = 0u32;

    match direction {
        Direction::Forward => {
            // Start past current text object's end (or just past cursor).
            let mut pos =
                current_range.map_or_else(|| next_char_boundary(text, cursor), |(_, end)| end);

            let limit = (pos + MAX_SEEK_DISTANCE).min(text.len());
            while pos < limit {
                if let Some((start, end)) = resolve(pos) {
                    if Some((start, end)) != current_range {
                        found_count += 1;
                        if found_count >= target_count {
                            return MotionResult::Position(Offset::new(start));
                        }
                        // Skip past this one to find the next.
                        pos = end;
                        continue;
                    }
                }
                pos = next_char_boundary(text, pos);
            }
        }
        Direction::Backward => {
            // Start before current text object's start (or just before cursor).
            let mut pos = current_range.map_or_else(
                || cursor.saturating_sub(1),
                |(start, _)| start.saturating_sub(1),
            );

            let limit = pos.saturating_sub(MAX_SEEK_DISTANCE);
            while pos > limit {
                if let Some((start, end)) = resolve(pos) {
                    if Some((start, end)) != current_range {
                        found_count += 1;
                        if found_count >= target_count {
                            return MotionResult::Position(Offset::new(start));
                        }
                        // Skip past this one backward.
                        pos = start.saturating_sub(1);
                        continue;
                    }
                }
                if pos == 0 {
                    break;
                }
                pos = prev_char_boundary(text, pos);
            }
            // Also check position 0 if we haven't already.
            if pos == 0 && found_count < target_count {
                if let Some((start, end)) = resolve(0) {
                    if Some((start, end)) != current_range {
                        found_count += 1;
                        if found_count >= target_count {
                            return MotionResult::Position(Offset::new(start));
                        }
                    }
                }
            }
        }
    }

    MotionResult::Error
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::textobjects::TextObjectContext;
    use crate::dispatch::dispatch_textobject;
    use crate::grammar::types::{TextObject, TextObjectKind, TextObjectScope};

    /// Helper: build the resolver closure for a given text and kind.
    fn make_resolver<'a>(
        text: &'a str,
        kind: TextObjectKind,
    ) -> impl Fn(usize) -> Option<(usize, usize)> + 'a {
        move |pos: usize| {
            if pos >= text.len() {
                return None;
            }
            let ctx = TextObjectContext::new(text, pos);
            let object = TextObject {
                scope: TextObjectScope::Around,
                kind,
                seek: None,
            };
            let range = dispatch_textobject(object, &ctx)?;
            Some((range.start(), range.end()))
        }
    }

    #[test]
    fn seek_forward_to_next_paren_group() {
        let text = "(foo) bar (baz) end";
        let resolve = make_resolver(text, TextObjectKind::Paren);
        let result = seek_text_object(text, 1, 1, Direction::Forward, resolve);
        // Should land at start of "(baz)"
        assert_eq!(result, MotionResult::Position(Offset::new(10)));
    }

    #[test]
    fn seek_backward_to_prev_paren_group() {
        let text = "(foo) bar (baz) end";
        let resolve = make_resolver(text, TextObjectKind::Paren);
        let result = seek_text_object(text, 11, 1, Direction::Backward, resolve);
        // Should land at start of "(foo)"
        assert_eq!(result, MotionResult::Position(Offset::new(0)));
    }

    #[test]
    fn seek_forward_with_count() {
        let text = "(a) (b) (c) end";
        let resolve = make_resolver(text, TextObjectKind::Paren);
        let result = seek_text_object(text, 1, 2, Direction::Forward, resolve);
        // Skip (b), land at (c) start = 8
        assert_eq!(result, MotionResult::Position(Offset::new(8)));
    }

    #[test]
    fn seek_not_found_returns_error() {
        let text = "no parens here";
        let resolve = make_resolver(text, TextObjectKind::Paren);
        let result = seek_text_object(text, 0, 1, Direction::Forward, resolve);
        assert_eq!(result, MotionResult::Error);
    }

    #[test]
    fn seek_forward_bracket() {
        let text = "[foo] gap [bar]";
        let resolve = make_resolver(text, TextObjectKind::Bracket);
        let result = seek_text_object(text, 1, 1, Direction::Forward, resolve);
        // Should land at start of "[bar]"
        assert_eq!(result, MotionResult::Position(Offset::new(10)));
    }

    #[test]
    fn seek_backward_bracket() {
        let text = "[foo] gap [bar]";
        let resolve = make_resolver(text, TextObjectKind::Bracket);
        let result = seek_text_object(text, 11, 1, Direction::Backward, resolve);
        // Should land at start of "[foo]"
        assert_eq!(result, MotionResult::Position(Offset::new(0)));
    }

    #[test]
    fn seek_forward_word() {
        let text = "hello world foo";
        let resolve = make_resolver(text, TextObjectKind::Word);
        let result = seek_text_object(text, 0, 1, Direction::Forward, resolve);
        // Should seek past "hello " to the next word "world"
        assert!(matches!(result, MotionResult::Position(_)));
        if let MotionResult::Position(off) = result {
            assert_eq!(off.get(), 6);
        }
    }

    #[test]
    fn seek_empty_text_returns_error() {
        let result = seek_text_object("", 0, 1, Direction::Forward, |_| None);
        assert_eq!(result, MotionResult::Error);
    }
}
