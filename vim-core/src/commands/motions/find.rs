//! Find motions: f, F, t, T, ;, ,
//!
//! Character search motions within a line.
//!
//! # Philosophy
//!
//! Plain functions, not trait methods. Each motion is a standalone
//! function called directly from dispatch.

use super::types::{MotionContext, MotionResult};
use crate::commands::helpers::{line_count, line_end, line_of, line_start, next_char_boundary};
use crate::primitives::Offset;

// ─────────────────────────────────────────────────────────────────────────────
// Find Motions
// ─────────────────────────────────────────────────────────────────────────────

/// `f{char}` - Find char forward (inclusive).
pub fn f(ctx: &MotionContext<'_>) -> MotionResult {
    let target_char = match ctx.target_char {
        Some(c) => c,
        None => return MotionResult::Error,
    };
    compute_find_forward(ctx, target_char)
}

/// `F{char}` - Find char backward (inclusive).
#[allow(non_snake_case, reason = "Vim motion name")]
pub fn F(ctx: &MotionContext<'_>) -> MotionResult {
    let target_char = match ctx.target_char {
        Some(c) => c,
        None => return MotionResult::Error,
    };
    compute_find_backward(ctx, target_char)
}

/// `t{char}` - Till char forward (exclusive).
pub fn t(ctx: &MotionContext<'_>) -> MotionResult {
    let target_char = match ctx.target_char {
        Some(c) => c,
        None => return MotionResult::Error,
    };
    compute_till_forward(ctx, target_char)
}

/// `T{char}` - Till char backward (exclusive).
#[allow(non_snake_case, reason = "Vim motion name")]
pub fn T(ctx: &MotionContext<'_>) -> MotionResult {
    let target_char = match ctx.target_char {
        Some(c) => c,
        None => return MotionResult::Error,
    };
    compute_till_backward(ctx, target_char)
}

/// `;` - Repeat last find motion.
pub fn semicolon(ctx: &MotionContext<'_>, last_find: &LastFind) -> MotionResult {
    last_find.repeat(ctx)
}

/// `,` - Repeat last find motion in reverse direction.
pub fn comma(ctx: &MotionContext<'_>, last_find: &LastFind) -> MotionResult {
    last_find.repeat_reverse(ctx)
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal Implementations
// ─────────────────────────────────────────────────────────────────────────────

/// Case-aware character comparison for f/t/F/T.
///
/// Follows Vim's two-flag model (same as regex search):
/// - `ignorecase=false`: exact match only, `smartcase` irrelevant
/// - `ignorecase=true, smartcase=false`: always case-insensitive
/// - `ignorecase=true, smartcase=true`: case-insensitive for lowercase
///   target, case-sensitive for uppercase target
///
/// Non-cased characters (digits, symbols, CJK) always use exact match.
///
/// Note: `target.to_lowercase()` is called per-candidate. For single-line
/// character find (typically <200 chars), this is negligible. The hot path
/// (`!ignorecase`) returns immediately with zero overhead.
fn char_matches(target: char, candidate: char, ignorecase: bool, smartcase: bool) -> bool {
    if !ignorecase {
        return candidate == target;
    }
    if smartcase && target.is_uppercase() {
        return candidate == target;
    }
    candidate.to_lowercase().eq(target.to_lowercase())
}

pub(crate) fn compute_find_forward(ctx: &MotionContext<'_>, target: char) -> MotionResult {
    let cursor = ctx.cursor.get();
    let line = line_of(ctx.text, cursor);

    let search_end = if ctx.options.multiline_find() && ctx.options.multiline_find_range() > 0 {
        let total_lines = line_count(ctx.text);
        let max_line =
            (line + ctx.options.multiline_find_range()).min(total_lines.saturating_sub(1));
        line_end(ctx.text, max_line).unwrap_or(ctx.text.len())
    } else {
        line_end(ctx.text, line).unwrap_or(ctx.text.len())
    };

    // Start searching after current position (multi-byte safe)
    let search_start = next_char_boundary(ctx.text, cursor);
    if search_start >= search_end {
        return MotionResult::Error;
    }

    let search_text = &ctx.text[search_start..search_end];
    let ignorecase = ctx.options.ignorecase();
    let smartcase = ctx.options.smartcase();

    let mut found_count = 0;
    for (i, c) in search_text.char_indices() {
        if char_matches(target, c, ignorecase, smartcase) {
            found_count += 1;
            if found_count >= ctx.count {
                return MotionResult::Position(Offset::new(search_start + i));
            }
        }
    }

    MotionResult::Error
}

pub(crate) fn compute_find_backward(ctx: &MotionContext<'_>, target: char) -> MotionResult {
    let cursor = ctx.cursor.get();
    let line = line_of(ctx.text, cursor);

    let search_start = if ctx.options.multiline_find() && ctx.options.multiline_find_range() > 0 {
        let min_line = line.saturating_sub(ctx.options.multiline_find_range());
        line_start(ctx.text, min_line).unwrap_or(0)
    } else {
        line_start(ctx.text, line).unwrap_or(0)
    };

    if cursor == search_start {
        return MotionResult::Error;
    }

    let search_text = &ctx.text[search_start..cursor];

    // Zero-allocation reverse search: iterate forward, collect last `count` matches.
    // We want the count-th match from the end — track matches in a small ring buffer.
    let count = ctx.count_usize();
    let mut matches: smallvec::SmallVec<[usize; 4]> = smallvec::SmallVec::new();
    let ignorecase = ctx.options.ignorecase();
    let smartcase = ctx.options.smartcase();

    for (i, c) in search_text.char_indices() {
        if char_matches(target, c, ignorecase, smartcase) {
            matches.push(i);
        }
    }

    // count-th from end: if we have 5 matches and count=2, we want index 3 (5-2)
    if matches.len() >= count {
        let idx = matches.len() - count;
        MotionResult::Position(Offset::new(
            search_start + matches.get(idx).copied().unwrap_or(0),
        ))
    } else {
        MotionResult::Error
    }
}

fn compute_till_forward(ctx: &MotionContext<'_>, target: char) -> MotionResult {
    let cursor = ctx.cursor.get();
    match compute_find_forward(ctx, target) {
        MotionResult::Position(pos) if pos.get() > cursor => {
            // Go to one before the found char
            let before = &ctx.text[cursor..pos.get()];
            if let Some(last_grapheme_start) = before.char_indices().last() {
                MotionResult::Position(Offset::new(cursor + last_grapheme_start.0))
            } else {
                MotionResult::Position(Offset::new(pos.get().saturating_sub(1)))
            }
        }
        result => result,
    }
}

fn compute_till_backward(ctx: &MotionContext<'_>, target: char) -> MotionResult {
    match compute_find_backward(ctx, target) {
        MotionResult::Position(pos) => {
            // Go to one after the found char
            let at_pos = &ctx.text[pos.get()..];
            if let Some((_, c)) = at_pos.char_indices().next() {
                MotionResult::Position(Offset::new(pos.get() + c.len_utf8()))
            } else {
                MotionResult::Position(Offset::new(pos.get() + 1))
            }
        }
        result => result,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Sneak Motions
// ─────────────────────────────────────────────────────────────────────────────

/// Sneak forward: find two-character sequence forward, crossing line boundaries.
///
/// Cursor lands ON the first character of the match (exclusive motion, like `t`).
pub fn sneak_forward(ctx: &MotionContext<'_>, c1: char, c2: char) -> MotionResult {
    compute_sneak_forward(ctx, c1, c2)
}

/// Sneak backward: find two-character sequence backward, crossing line boundaries.
pub fn sneak_backward(ctx: &MotionContext<'_>, c1: char, c2: char) -> MotionResult {
    compute_sneak_backward(ctx, c1, c2)
}

fn compute_sneak_forward(ctx: &MotionContext<'_>, c1: char, c2: char) -> MotionResult {
    let cursor = ctx.cursor.get();

    // Search from after current position to end of document
    let search_start = next_char_boundary(ctx.text, cursor);
    if search_start >= ctx.text.len() {
        return MotionResult::Error;
    }

    let search_text = &ctx.text[search_start..];
    let ignorecase = ctx.options.ignorecase();
    let smartcase = ctx.options.smartcase();

    let mut found_count = 0u32;
    let mut chars = search_text.char_indices().peekable();
    while let Some((i, ch)) = chars.next() {
        if char_matches(c1, ch, ignorecase, smartcase) {
            // Check if the next char matches c2
            if let Some(&(_, ch2)) = chars.peek() {
                if char_matches(c2, ch2, ignorecase, smartcase) {
                    found_count += 1;
                    if found_count >= ctx.count {
                        return MotionResult::Position(Offset::new(search_start + i));
                    }
                }
            }
        }
    }

    MotionResult::Error
}

fn compute_sneak_backward(ctx: &MotionContext<'_>, c1: char, c2: char) -> MotionResult {
    let cursor = ctx.cursor.get();
    if cursor == 0 {
        return MotionResult::Error;
    }

    let search_text = &ctx.text[..cursor];
    let ignorecase = ctx.options.ignorecase();
    let smartcase = ctx.options.smartcase();

    // Collect all two-char matches, then pick count-th from the end
    let mut matches: smallvec::SmallVec<[usize; 4]> = smallvec::SmallVec::new();
    let mut chars = search_text.char_indices().peekable();
    while let Some((i, ch)) = chars.next() {
        if char_matches(c1, ch, ignorecase, smartcase) {
            if let Some(&(_, ch2)) = chars.peek() {
                if char_matches(c2, ch2, ignorecase, smartcase) {
                    matches.push(i);
                }
            }
        }
    }

    let count = ctx.count_usize();
    if matches.len() >= count {
        let idx = matches.len() - count;
        MotionResult::Position(Offset::new(matches.get(idx).copied().unwrap_or(0)))
    } else {
        MotionResult::Error
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Find Motion State Types (re-exported from state layer)
// ─────────────────────────────────────────────────────────────────────────────

// Data types (FindDirection, LastFind) live in state::find_types.
// Only motion-dependent methods are defined here.
pub use crate::primitives::{FindDirection, LastFind};

// ─────────────────────────────────────────────────────────────────────────────
// CharCommand → FindDirection mapping
// ─────────────────────────────────────────────────────────────────────────────

/// Map a `CharCommand` (grammar type) to the corresponding `FindDirection`
/// (state type) for `;` and `,` repeat tracking.
///
/// Returns `None` for `Replace`, which is not a find motion.
#[inline]
#[must_use]
pub const fn char_command_to_direction(
    cmd: crate::grammar::types::CharCommand,
) -> Option<FindDirection> {
    use crate::grammar::types::CharCommand;
    match cmd {
        CharCommand::FindForward => Some(FindDirection::FindForward),
        CharCommand::FindBackward => Some(FindDirection::FindBackward),
        CharCommand::TillForward => Some(FindDirection::TillForward),
        CharCommand::TillBackward => Some(FindDirection::TillBackward),
        CharCommand::Replace => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Find-with-tracking (produces CommandResult)
// ─────────────────────────────────────────────────────────────────────────────

/// Execute a find motion and produce a `CommandResult` with state tracking.
///
/// This is the enterprise-grade commands-layer function for find motions.
/// It encapsulates the full find lifecycle:
/// 1. Map `CharCommand → FindDirection` for `;`/`,` repeat tracking
/// 2. Always emit `SetLastFind` (Neovim records even failed finds)
/// 3. Dispatch the find motion
/// 4. Set cursor on success
///
/// Returns `CommandResult` — never raw `Effects`.
pub fn find_with_tracking(
    direction: FindDirection,
    target_char: char,
    motion_result: Option<&MotionResult>,
) -> crate::commands::CommandResult {
    use crate::commands::CommandResult;
    use crate::effects::Effects;

    let base = Effects::new().set_last_find(direction, target_char);

    match motion_result {
        Some(MotionResult::Position(offset)) => {
            CommandResult::effects_only(base.set_cursor(*offset))
        }
        _ => CommandResult::effects_only(base),
    }
}

/// Like [`find_with_tracking`] but stores resolved case flags in the effect.
pub fn find_with_tracking_with_case(
    direction: FindDirection,
    target_char: char,
    motion_result: Option<&MotionResult>,
    ignorecase: bool,
    smartcase: bool,
) -> crate::commands::CommandResult {
    use crate::commands::CommandResult;
    use crate::effects::Effects;

    let base =
        Effects::new().set_last_find_with_case(direction, target_char, ignorecase, smartcase);

    match motion_result {
        Some(MotionResult::Position(offset)) => {
            CommandResult::effects_only(base.set_cursor(*offset))
        }
        _ => CommandResult::effects_only(base),
    }
}

/// Execute a find motion in the given direction.
fn execute_find(direction: FindDirection, ctx: &MotionContext<'_>, target: char) -> MotionResult {
    match direction {
        FindDirection::FindForward => compute_find_forward(ctx, target),
        FindDirection::FindBackward => compute_find_backward(ctx, target),
        FindDirection::TillForward => compute_till_forward(ctx, target),
        FindDirection::TillBackward => compute_till_backward(ctx, target),
        // Sneak directions need c2 from LastFind; handled in repeat/repeat_reverse
        FindDirection::SneakForward | FindDirection::SneakBackward => MotionResult::Error,
    }
}

/// Execute a find or sneak motion in the given direction.
///
/// This version handles sneak directions by using the c2 from `LastFind`.
fn execute_find_with_sneak(
    direction: FindDirection,
    ctx: &MotionContext<'_>,
    target: char,
    c2: Option<char>,
) -> MotionResult {
    match direction {
        FindDirection::SneakForward => {
            if let Some(c2) = c2 {
                compute_sneak_forward(ctx, target, c2)
            } else {
                MotionResult::Error
            }
        }
        FindDirection::SneakBackward => {
            if let Some(c2) = c2 {
                compute_sneak_backward(ctx, target, c2)
            } else {
                MotionResult::Error
            }
        }
        other => execute_find(other, ctx, target),
    }
}

/// Motion-dependent extension methods for `LastFind`.
///
/// These methods depend on `MotionContext` and compute functions,
/// so they live in the commands layer, not the state layer.
impl LastFind {
    /// Repeat the last find (;).
    ///
    /// For till motions (t/T), we need to adjust the cursor to step "into" the
    /// found character before searching, since the cursor is positioned just
    /// before/after the target character.
    ///
    /// Uses the case flags stored at original find time, NOT the current options.
    pub fn repeat(&self, ctx: &MotionContext<'_>) -> MotionResult {
        match (self.direction(), self.target_char()) {
            (Some(direction), Some(c)) => {
                // Override case flags with the ones stored at original find time.
                let opts = options_with_stored_case(self, ctx.options);
                let case_ctx = MotionContext::new(ctx.text, ctx.cursor, ctx.count, &opts);
                if direction.is_sneak() {
                    execute_find_with_sneak(direction, &case_ctx, c, self.sneak_c2())
                } else {
                    let adjusted_ctx = adjust_cursor_for_till_repeat(&case_ctx, direction);
                    execute_find(direction, &adjusted_ctx, c)
                }
            }
            _ => MotionResult::NoMotion,
        }
    }

    /// Repeat the last find in reverse direction (,).
    ///
    /// Uses the case flags stored at original find time, NOT the current options.
    pub fn repeat_reverse(&self, ctx: &MotionContext<'_>) -> MotionResult {
        match (self.direction(), self.target_char()) {
            (Some(direction), Some(c)) => {
                let reversed = direction.reverse();
                // Override case flags with the ones stored at original find time.
                let opts = options_with_stored_case(self, ctx.options);
                let case_ctx = MotionContext::new(ctx.text, ctx.cursor, ctx.count, &opts);
                if reversed.is_sneak() {
                    execute_find_with_sneak(reversed, &case_ctx, c, self.sneak_c2())
                } else {
                    // For reverse, we need to adjust based on the REVERSED direction's till-ness
                    let adjusted_ctx = adjust_cursor_for_till_repeat(&case_ctx, reversed);
                    execute_find(reversed, &adjusted_ctx, c)
                }
            }
            _ => MotionResult::NoMotion,
        }
    }
}

/// Create a copy of options with case flags overridden from stored `LastFind` state.
fn options_with_stored_case(
    last_find: &LastFind,
    base: &crate::primitives::VimOptions,
) -> crate::primitives::VimOptions {
    let mut opts = base.clone();
    opts.set_ignorecase(last_find.resolved_ignorecase());
    opts.set_smartcase(last_find.resolved_smartcase());
    opts
}

/// Adjust cursor for till repeat.
///
/// When repeating a till motion, we need to "step back into" the target
/// character so the search finds the next occurrence, not the one we're
/// currently next to.
fn adjust_cursor_for_till_repeat<'text>(
    ctx: &'text MotionContext<'text>,
    direction: FindDirection,
) -> MotionContext<'text> {
    let cursor = ctx.cursor.get();
    match direction {
        FindDirection::TillForward => {
            // For t (till forward), we're one BEFORE the target char
            // Step 1 forward to be ON the target, then search will find the next
            let new_cursor = ctx
                .text
                .get(cursor..)
                .and_then(|s| {
                    let mut chars = s.char_indices();
                    chars.next(); // skip current char
                    chars.next().map(|(i, _)| cursor + i)
                })
                .unwrap_or(cursor);
            MotionContext::new(ctx.text, Offset::new(new_cursor), ctx.count, ctx.options)
        }
        FindDirection::TillBackward => {
            // For T (till backward), we're one AFTER the target char
            // Step 1 backward to be ON the target, then search will find the next
            if cursor > 0 {
                // Find the previous character boundary
                let before = &ctx.text[..cursor];
                if let Some((i, _)) = before.char_indices().next_back() {
                    return MotionContext::new(ctx.text, Offset::new(i), ctx.count, ctx.options);
                }
            }
            ctx.clone()
        }
        _ => ctx.clone(), // f/F don't need adjustment
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::VimOptions;

    fn opts() -> VimOptions {
        VimOptions::default()
    }

    fn make_ctx_with_char<'a>(
        text: &'a str,
        cursor: usize,
        count: u32,
        target: char,
        options: &'a VimOptions,
    ) -> MotionContext<'a> {
        MotionContext::new(text, Offset::new(cursor), count, options).with_target_char(target)
    }

    fn make_ctx<'a>(
        text: &'a str,
        cursor: usize,
        count: u32,
        options: &'a VimOptions,
    ) -> MotionContext<'a> {
        MotionContext::new(text, Offset::new(cursor), count, options)
    }

    // f tests

    #[test]
    fn f_basic_find_forward() {
        let o = opts();
        let c = make_ctx_with_char("hello world", 0, 1, 'w', &o);
        assert_eq!(f(&c), MotionResult::Position(Offset::new(6)));
    }

    #[test]
    fn f_find_next_occurrence() {
        let o = opts();
        let c = make_ctx_with_char("abcabc", 0, 1, 'b', &o);
        assert_eq!(f(&c), MotionResult::Position(Offset::new(1)));
    }

    #[test]
    fn f_with_count() {
        let o = opts();
        let c = make_ctx_with_char("abcabc", 0, 2, 'b', &o);
        assert_eq!(f(&c), MotionResult::Position(Offset::new(4)));
    }

    #[test]
    fn f_not_found_returns_error() {
        let o = opts();
        let c = make_ctx_with_char("hello", 0, 1, 'z', &o);
        assert_eq!(f(&c), MotionResult::Error);
    }

    #[test]
    fn f_no_target_char_returns_error() {
        let o = opts();
        let c = make_ctx("hello", 0, 1, &o);
        assert_eq!(f(&c), MotionResult::Error);
    }

    #[test]
    fn f_does_not_cross_newline() {
        let o = opts();
        let c = make_ctx_with_char("hello\nworld", 0, 1, 'a', &o);
        assert_eq!(f(&c), MotionResult::Error);
    }

    #[test]
    fn f_at_line_end_returns_error() {
        let o = opts();
        let c = make_ctx_with_char("hello\nworld", 4, 1, 'x', &o);
        assert_eq!(f(&c), MotionResult::Error);
    }

    #[test]
    fn f_finds_same_char_as_current() {
        let o = opts();
        let c = make_ctx_with_char("aaa", 0, 1, 'a', &o);
        assert_eq!(f(&c), MotionResult::Position(Offset::new(1)));
    }

    #[test]
    fn f_count_exceeds_occurrences() {
        let o = opts();
        let c = make_ctx_with_char("abcb", 0, 3, 'b', &o);
        assert_eq!(f(&c), MotionResult::Error);
    }

    // F tests

    #[test]
    fn f_upper_basic_find_backward() {
        let o = opts();
        let c = make_ctx_with_char("hello world", 10, 1, 'h', &o);
        assert_eq!(F(&c), MotionResult::Position(Offset::new(0)));
    }

    #[test]
    fn f_upper_with_count() {
        let o = opts();
        let c = make_ctx_with_char("abcabc", 5, 2, 'a', &o);
        assert_eq!(F(&c), MotionResult::Position(Offset::new(0)));
    }

    #[test]
    fn f_upper_not_found_returns_error() {
        let o = opts();
        let c = make_ctx_with_char("hello", 4, 1, 'z', &o);
        assert_eq!(F(&c), MotionResult::Error);
    }

    #[test]
    fn f_upper_at_line_start_returns_error() {
        let o = opts();
        let c = make_ctx_with_char("hello", 0, 1, 'x', &o);
        assert_eq!(F(&c), MotionResult::Error);
    }

    #[test]
    fn f_upper_does_not_cross_newline() {
        let o = opts();
        let c = make_ctx_with_char("hello\nworld", 7, 1, 'h', &o);
        assert_eq!(F(&c), MotionResult::Error);
    }

    #[test]
    fn f_upper_finds_on_same_line() {
        let o = opts();
        let c = make_ctx_with_char("hello\nworld", 10, 1, 'w', &o);
        assert_eq!(F(&c), MotionResult::Position(Offset::new(6)));
    }

    // t tests

    #[test]
    fn t_basic_till_forward() {
        let o = opts();
        let c = make_ctx_with_char("hello world", 0, 1, 'w', &o);
        assert_eq!(t(&c), MotionResult::Position(Offset::new(5)));
    }

    #[test]
    fn t_adjacent_char() {
        let o = opts();
        let c = make_ctx_with_char("ab", 0, 1, 'b', &o);
        assert_eq!(t(&c), MotionResult::Position(Offset::new(0)));
    }

    #[test]
    fn t_not_found_returns_error() {
        let o = opts();
        let c = make_ctx_with_char("hello", 0, 1, 'z', &o);
        assert_eq!(t(&c), MotionResult::Error);
    }

    #[test]
    fn t_with_count() {
        let o = opts();
        let c = make_ctx_with_char("abcabc", 0, 2, 'b', &o);
        assert_eq!(t(&c), MotionResult::Position(Offset::new(3)));
    }

    // T tests

    #[test]
    fn t_upper_basic_till_backward() {
        let o = opts();
        let c = make_ctx_with_char("hello world", 10, 1, 'h', &o);
        assert_eq!(T(&c), MotionResult::Position(Offset::new(1)));
    }

    #[test]
    fn t_upper_adjacent_char() {
        let o = opts();
        let c = make_ctx_with_char("ab", 1, 1, 'a', &o);
        assert_eq!(T(&c), MotionResult::Position(Offset::new(1)));
    }

    #[test]
    fn t_upper_not_found_returns_error() {
        let o = opts();
        let c = make_ctx_with_char("hello", 4, 1, 'z', &o);
        assert_eq!(T(&c), MotionResult::Error);
    }

    #[test]
    fn t_upper_with_count() {
        let o = opts();
        let c = make_ctx_with_char("abcabc", 5, 2, 'a', &o);
        assert_eq!(T(&c), MotionResult::Position(Offset::new(1)));
    }

    // Semicolon and comma tests

    #[test]
    fn semicolon_repeats_find_forward() {
        let o = opts();
        let mut lf = LastFind::new();
        lf.record(FindDirection::FindForward, 'l');
        let c = make_ctx("hello", 1, 1, &o).with_last_find(lf.clone());
        assert_eq!(semicolon(&c, &lf), MotionResult::Position(Offset::new(2)));
    }

    #[test]
    fn semicolon_no_last_find_returns_no_motion() {
        let o = opts();
        let lf = LastFind::new();
        let c = make_ctx("hello", 0, 1, &o);
        assert_eq!(semicolon(&c, &lf), MotionResult::NoMotion);
    }

    #[test]
    fn comma_reverses_find_direction() {
        let o = opts();
        let mut lf = LastFind::new();
        lf.record(FindDirection::FindForward, 'l');
        let c = make_ctx("hello", 3, 1, &o).with_last_find(lf.clone());
        assert_eq!(comma(&c, &lf), MotionResult::Position(Offset::new(2)));
    }

    #[test]
    fn comma_no_last_find_returns_no_motion() {
        let o = opts();
        let lf = LastFind::new();
        let c = make_ctx("hello", 4, 1, &o);
        assert_eq!(comma(&c, &lf), MotionResult::NoMotion);
    }

    #[test]
    fn semicolon_repeats_find_backward() {
        let o = opts();
        let mut lf = LastFind::new();
        lf.record(FindDirection::FindBackward, 'l');
        let c = make_ctx("hello", 4, 1, &o).with_last_find(lf.clone());
        assert_eq!(semicolon(&c, &lf), MotionResult::Position(Offset::new(3)));
    }

    // Multi-byte character find tests

    #[test]
    fn f_multibyte_target() {
        let o = opts();
        let c = make_ctx_with_char("a\u{00e9}bc\u{00e9}", 0, 1, '\u{00e9}', &o);
        assert_eq!(f(&c), MotionResult::Position(Offset::new(1)));
    }

    #[test]
    fn f_multibyte_target_second_occurrence() {
        let o = opts();
        let c = make_ctx_with_char("a\u{00e9}bc\u{00e9}", 0, 2, '\u{00e9}', &o);
        assert_eq!(f(&c), MotionResult::Position(Offset::new(5)));
    }

    #[test]
    fn f_upper_multibyte_target() {
        let o = opts();
        let c = make_ctx_with_char("a\u{00e9}bc\u{00e9}", 5, 1, '\u{00e9}', &o);
        assert_eq!(F(&c), MotionResult::Position(Offset::new(1)));
    }

    #[test]
    fn t_multibyte_one_before() {
        let o = opts();
        let c = make_ctx_with_char("a\u{597d}bcd", 0, 1, '\u{597d}', &o);
        assert_eq!(t(&c), MotionResult::Position(Offset::new(0)));
    }

    #[test]
    fn f_cjk_char() {
        let o = opts();
        let c = make_ctx_with_char("hello\u{4f60}\u{597d}world", 0, 1, '\u{4f60}', &o);
        assert_eq!(f(&c), MotionResult::Position(Offset::new(5)));
    }

    #[test]
    fn f_emoji_target() {
        let o = opts();
        let c = make_ctx_with_char("ab\u{1f600}cd", 0, 1, '\u{1f600}', &o);
        assert_eq!(f(&c), MotionResult::Position(Offset::new(2)));
    }

    #[test]
    fn t_upper_multibyte_one_after() {
        let o = opts();
        let c = make_ctx_with_char("a\u{00e9}bcd", 5, 1, '\u{00e9}', &o);
        assert_eq!(T(&c), MotionResult::Position(Offset::new(3)));
    }

    // char_command_to_direction tests

    #[test]
    fn char_command_mapping_find_forward() {
        use crate::grammar::types::CharCommand;
        assert_eq!(
            char_command_to_direction(CharCommand::FindForward),
            Some(FindDirection::FindForward)
        );
    }

    #[test]
    fn char_command_mapping_replace_returns_none() {
        use crate::grammar::types::CharCommand;
        assert_eq!(char_command_to_direction(CharCommand::Replace), None);
    }

    // ── Sneak motion tests ──────────────────────────────────────────────

    #[test]
    fn sneak_forward_basic() {
        let o = opts();
        let ctx = make_ctx("hello ab world", 0, 1, &o);
        assert_eq!(
            sneak_forward(&ctx, 'a', 'b'),
            MotionResult::Position(Offset::new(6))
        );
    }

    #[test]
    fn sneak_forward_cross_line() {
        let o = opts();
        let ctx = make_ctx("foo\nbar", 0, 1, &o);
        assert_eq!(
            sneak_forward(&ctx, 'b', 'a'),
            MotionResult::Position(Offset::new(4))
        );
    }

    #[test]
    fn sneak_forward_not_found() {
        let o = opts();
        let ctx = make_ctx("hello world", 0, 1, &o);
        assert_eq!(sneak_forward(&ctx, 'z', 'z'), MotionResult::Error);
    }

    #[test]
    fn sneak_forward_with_count() {
        let o = opts();
        // "xab cd ab ef" — from position 0, first "ab" at 1, second "ab" at 7
        let ctx = make_ctx("xab cd ab ef", 0, 2, &o);
        assert_eq!(
            sneak_forward(&ctx, 'a', 'b'),
            MotionResult::Position(Offset::new(7))
        );
    }

    #[test]
    fn sneak_backward_basic() {
        let o = opts();
        let ctx = make_ctx("hello ab world", 13, 1, &o);
        assert_eq!(
            sneak_backward(&ctx, 'a', 'b'),
            MotionResult::Position(Offset::new(6))
        );
    }

    #[test]
    fn sneak_backward_cross_line() {
        let o = opts();
        // "foo\nbar" — f=0,o=1,o=2,\n=3,b=4,a=5,r=6
        // search for "fo" from position 6 backward
        let ctx = make_ctx("foo\nbar", 6, 1, &o);
        assert_eq!(
            sneak_backward(&ctx, 'f', 'o'),
            MotionResult::Position(Offset::new(0))
        );
        // search for "oo" from position 6 backward
        let ctx = make_ctx("foo\nbar", 6, 1, &o);
        assert_eq!(
            sneak_backward(&ctx, 'o', 'o'),
            MotionResult::Position(Offset::new(1))
        );
    }

    #[test]
    fn sneak_backward_not_found() {
        let o = opts();
        let ctx = make_ctx("hello world", 10, 1, &o);
        assert_eq!(sneak_backward(&ctx, 'z', 'z'), MotionResult::Error);
    }

    #[test]
    fn sneak_backward_with_count() {
        let o = opts();
        let ctx = make_ctx("ab cd ab ef", 10, 2, &o);
        assert_eq!(
            sneak_backward(&ctx, 'a', 'b'),
            MotionResult::Position(Offset::new(0))
        );
    }

    #[test]
    fn sneak_forward_cursor_at_end() {
        let o = opts();
        let ctx = make_ctx("ab", 2, 1, &o);
        assert_eq!(sneak_forward(&ctx, 'a', 'b'), MotionResult::Error);
    }

    #[test]
    fn sneak_backward_cursor_at_start() {
        let o = opts();
        let ctx = make_ctx("ab", 0, 1, &o);
        assert_eq!(sneak_backward(&ctx, 'a', 'b'), MotionResult::Error);
    }

    #[test]
    fn sneak_repeat_via_semicolon() {
        let o = opts();
        let mut lf = LastFind::new();
        lf.record_sneak(FindDirection::SneakForward, 'a', 'b');
        let ctx = make_ctx("ab cd ab ef", 0, 1, &o).with_last_find(lf.clone());
        let result = semicolon(&ctx, &lf);
        assert_eq!(result, MotionResult::Position(Offset::new(6)));
    }

    #[test]
    fn sneak_repeat_via_comma() {
        let o = opts();
        let mut lf = LastFind::new();
        lf.record_sneak(FindDirection::SneakForward, 'a', 'b');
        let ctx = make_ctx("ab cd ab ef", 10, 1, &o).with_last_find(lf.clone());
        let result = comma(&ctx, &lf);
        assert_eq!(result, MotionResult::Position(Offset::new(6)));
    }

    // ── Multiline find tests ────────────────────────────────────────────

    fn multiline_opts() -> VimOptions {
        let mut o = VimOptions::default();
        o.set_multiline_find(true);
        o.set_multiline_find_range(5);
        o
    }

    #[test]
    fn f_multiline_disabled_does_not_cross_line() {
        let o = opts(); // default: multiline_find = false
        let c = make_ctx_with_char("abc\ndef", 0, 1, 'e', &o);
        assert_eq!(f(&c), MotionResult::Error);
    }

    #[test]
    fn f_multiline_enabled_finds_char_on_next_line() {
        let o = multiline_opts();
        // "abc\ndef" — a=0,b=1,c=2,\n=3,d=4,e=5,f=6
        let c = make_ctx_with_char("abc\ndef", 0, 1, 'e', &o);
        assert_eq!(f(&c), MotionResult::Position(Offset::new(5)));
    }

    #[test]
    fn f_multiline_enabled_still_finds_on_same_line() {
        let o = multiline_opts();
        let c = make_ctx_with_char("abcdef", 0, 1, 'e', &o);
        assert_eq!(f(&c), MotionResult::Position(Offset::new(4)));
    }

    #[test]
    fn f_multiline_range_limits_search() {
        let mut o = multiline_opts();
        o.set_multiline_find_range(1);
        // "a\nb\nc\n" — cursor on 'a', looking for 'c' which is 2 lines down
        let c = make_ctx_with_char("a\nb\nc\n", 0, 1, 'c', &o);
        assert_eq!(f(&c), MotionResult::Error);
    }

    #[test]
    fn f_multiline_range_includes_boundary_line() {
        let mut o = multiline_opts();
        o.set_multiline_find_range(2);
        // "a\nb\nc\n" — cursor on 'a', 'c' is on line 2 (0-indexed), within range 2
        let c = make_ctx_with_char("a\nb\nc\n", 0, 1, 'c', &o);
        assert_eq!(f(&c), MotionResult::Position(Offset::new(4)));
    }

    #[test]
    fn f_upper_multiline_disabled_does_not_cross_line() {
        let o = opts();
        let c = make_ctx_with_char("abc\ndef", 6, 1, 'a', &o);
        assert_eq!(F(&c), MotionResult::Error);
    }

    #[test]
    fn f_upper_multiline_enabled_finds_char_on_prev_line() {
        let o = multiline_opts();
        let c = make_ctx_with_char("abc\ndef", 6, 1, 'a', &o);
        assert_eq!(F(&c), MotionResult::Position(Offset::new(0)));
    }

    #[test]
    fn f_upper_multiline_range_limits_search() {
        let mut o = multiline_opts();
        o.set_multiline_find_range(1);
        // "a\nb\nc\n" — cursor on 'c' (line 2), looking for 'a' (line 0), range=1 only reaches line 1
        let c = make_ctx_with_char("a\nb\nc\n", 4, 1, 'a', &o);
        assert_eq!(F(&c), MotionResult::Error);
    }

    #[test]
    fn f_upper_multiline_range_includes_boundary_line() {
        let mut o = multiline_opts();
        o.set_multiline_find_range(2);
        // "a\nb\nc\n" — cursor on 'c' (line 2), 'a' is on line 0, within range 2
        let c = make_ctx_with_char("a\nb\nc\n", 4, 1, 'a', &o);
        assert_eq!(F(&c), MotionResult::Position(Offset::new(0)));
    }

    #[test]
    fn t_multiline_enabled_finds_one_before_target_on_next_line() {
        let o = multiline_opts();
        // "abc\ndef" — a=0,b=1,c=2,\n=3,d=4,e=5,f=6
        // cursor on 'a', find till 'e' (at 5) should land on 'd' (at 4)
        let c = make_ctx_with_char("abc\ndef", 0, 1, 'e', &o);
        assert_eq!(t(&c), MotionResult::Position(Offset::new(4)));
    }

    #[test]
    fn t_upper_multiline_enabled_finds_one_after_target_on_prev_line() {
        let o = multiline_opts();
        // "abc\ndef" — a=0,b=1,c=2,\n=3,d=4,e=5,f=6
        // cursor on 'f' (offset 6), till backward 'c' (at 2) should land on '\n' (at 3)
        let c = make_ctx_with_char("abc\ndef", 6, 1, 'c', &o);
        assert_eq!(T(&c), MotionResult::Position(Offset::new(3)));
    }

    #[test]
    fn f_multiline_count_across_lines() {
        let o = multiline_opts();
        // "a.b\nc.d" — cursor on 'a', find 2nd '.' — first is at 1, second at 5
        let c = make_ctx_with_char("a.b\nc.d", 0, 2, '.', &o);
        assert_eq!(f(&c), MotionResult::Position(Offset::new(5)));
    }

    #[test]
    fn f_multiline_range_zero_equivalent_to_disabled() {
        let mut o = VimOptions::default();
        o.set_multiline_find(true);
        o.set_multiline_find_range(0);
        let c = make_ctx_with_char("abc\ndef", 0, 1, 'e', &o);
        assert_eq!(f(&c), MotionResult::Error);
    }

    // ── Additional sneak validation tests ──────────────────────────────

    #[test]
    fn sneak_forward_count_exceeds_occurrences() {
        let o = opts();
        // Only one "ab" in the text, count=2 should fail
        let ctx = make_ctx("xab cd ef", 0, 2, &o);
        assert_eq!(sneak_forward(&ctx, 'a', 'b'), MotionResult::Error);
    }

    #[test]
    fn sneak_backward_count_exceeds_occurrences() {
        let o = opts();
        // Only one "ab" before cursor, count=2 should fail
        let ctx = make_ctx("ab cd ef", 7, 2, &o);
        assert_eq!(sneak_backward(&ctx, 'a', 'b'), MotionResult::Error);
    }

    #[test]
    fn sneak_forward_cross_multiple_lines() {
        let o = opts();
        // Target "xy" is on the third line
        let ctx = make_ctx("aaa\nbbb\nxy end", 0, 1, &o);
        assert_eq!(
            sneak_forward(&ctx, 'x', 'y'),
            MotionResult::Position(Offset::new(8))
        );
    }

    #[test]
    fn sneak_backward_cross_multiple_lines() {
        let o = opts();
        // "ab" is on line 1, cursor is on line 3
        let ctx = make_ctx("ab\nccc\nddd\n", 10, 1, &o);
        assert_eq!(
            sneak_backward(&ctx, 'a', 'b'),
            MotionResult::Position(Offset::new(0))
        );
    }

    #[test]
    fn sneak_forward_at_eof_empty_text() {
        let o = opts();
        let ctx = make_ctx("", 0, 1, &o);
        assert_eq!(sneak_forward(&ctx, 'a', 'b'), MotionResult::Error);
    }

    #[test]
    fn sneak_backward_at_eof_empty_text() {
        let o = opts();
        let ctx = make_ctx("", 0, 1, &o);
        assert_eq!(sneak_backward(&ctx, 'a', 'b'), MotionResult::Error);
    }

    #[test]
    fn sneak_forward_pair_spans_newline() {
        let o = opts();
        // The two-char sequence "a\n" crosses the newline boundary
        // "hella\nworld" — 'a' at 4, '\n' at 5
        let ctx = make_ctx("hella\nworld", 0, 1, &o);
        assert_eq!(
            sneak_forward(&ctx, 'a', '\n'),
            MotionResult::Position(Offset::new(4))
        );
    }

    #[test]
    fn sneak_forward_multibyte_target() {
        let o = opts();
        // Two-char sequence with a multi-byte character
        // "\u{00e9}x" — e-acute at 0 (2 bytes), x at 2
        let ctx = make_ctx("a\u{00e9}xbc", 0, 1, &o);
        assert_eq!(
            sneak_forward(&ctx, '\u{00e9}', 'x'),
            MotionResult::Position(Offset::new(1))
        );
    }

    #[test]
    fn sneak_backward_multibyte_target() {
        let o = opts();
        // Search backward for "\u{00e9}x" from end
        let ctx = make_ctx("a\u{00e9}xbc", 5, 1, &o);
        assert_eq!(
            sneak_backward(&ctx, '\u{00e9}', 'x'),
            MotionResult::Position(Offset::new(1))
        );
    }

    #[test]
    fn sneak_repeat_overrides_last_f() {
        let o = opts();
        // Record a regular f find first
        let mut lf = LastFind::new();
        lf.record(FindDirection::FindForward, 'z');
        // Then record a sneak — this should override the f find
        lf.record_sneak(FindDirection::SneakForward, 'a', 'b');

        assert_eq!(lf.direction(), Some(FindDirection::SneakForward));
        assert_eq!(lf.target_char(), Some('a'));
        assert_eq!(lf.sneak_c2(), Some('b'));

        // Semicolon should repeat the sneak, not the f
        let ctx = make_ctx("ab cd ab ef", 0, 1, &o).with_last_find(lf.clone());
        let result = semicolon(&ctx, &lf);
        assert_eq!(result, MotionResult::Position(Offset::new(6)));
    }

    #[test]
    fn sneak_record_clears_c2_on_normal_find() {
        let o = opts();
        // Record a sneak first
        let mut lf = LastFind::new();
        lf.record_sneak(FindDirection::SneakForward, 'a', 'b');
        assert_eq!(lf.sneak_c2(), Some('b'));

        // Now record a normal f — sneak_c2 should be cleared
        lf.record(FindDirection::FindForward, 'z');
        assert_eq!(lf.direction(), Some(FindDirection::FindForward));
        assert_eq!(lf.target_char(), Some('z'));
        assert_eq!(lf.sneak_c2(), None);

        // Semicolon should repeat the f, not the sneak
        let ctx = make_ctx("xyzxyz", 0, 1, &o).with_last_find(lf.clone());
        let result = semicolon(&ctx, &lf);
        assert_eq!(result, MotionResult::Position(Offset::new(2)));
    }

    #[test]
    fn sneak_reverse_via_comma_backward_to_forward() {
        let o = opts();
        // Record sneak backward, then comma should search forward
        let mut lf = LastFind::new();
        lf.record_sneak(FindDirection::SneakBackward, 'a', 'b');
        let ctx = make_ctx("ab cd ab ef", 0, 1, &o).with_last_find(lf.clone());
        let result = comma(&ctx, &lf);
        // Comma reverses SneakBackward → SneakForward, searching forward from 0
        assert_eq!(result, MotionResult::Position(Offset::new(6)));
    }

    #[test]
    fn sneak_forward_ignorecase() {
        let mut o = opts();
        o.set_ignorecase(true);
        let ctx = make_ctx("hello AB world", 0, 1, &o);
        // With ignorecase, searching for 'a','b' should match 'A','B'
        assert_eq!(
            sneak_forward(&ctx, 'a', 'b'),
            MotionResult::Position(Offset::new(6))
        );
    }

    #[test]
    fn sneak_forward_smartcase_lowercase_query() {
        let mut o = opts();
        o.set_ignorecase(true);
        o.set_smartcase(true);
        let ctx = make_ctx("hello AB world", 0, 1, &o);
        // Lowercase query with smartcase+ignorecase → case-insensitive
        assert_eq!(
            sneak_forward(&ctx, 'a', 'b'),
            MotionResult::Position(Offset::new(6))
        );
    }

    #[test]
    fn sneak_forward_smartcase_uppercase_query() {
        let mut o = opts();
        o.set_ignorecase(true);
        o.set_smartcase(true);
        // Uppercase query with smartcase → case-sensitive for that char
        let ctx = make_ctx("hello ab AB world", 0, 1, &o);
        assert_eq!(
            sneak_forward(&ctx, 'A', 'B'),
            MotionResult::Position(Offset::new(9))
        );
    }

    #[test]
    fn find_direction_reverse_sneak() {
        assert_eq!(
            FindDirection::SneakForward.reverse(),
            FindDirection::SneakBackward
        );
        assert_eq!(
            FindDirection::SneakBackward.reverse(),
            FindDirection::SneakForward
        );
    }

    #[test]
    fn find_direction_is_sneak() {
        assert!(FindDirection::SneakForward.is_sneak());
        assert!(FindDirection::SneakBackward.is_sneak());
        assert!(!FindDirection::FindForward.is_sneak());
        assert!(!FindDirection::FindBackward.is_sneak());
        assert!(!FindDirection::TillForward.is_sneak());
        assert!(!FindDirection::TillBackward.is_sneak());
    }

    #[test]
    fn last_find_new_is_empty() {
        let lf = LastFind::new();
        assert_eq!(lf.direction(), None);
        assert_eq!(lf.target_char(), None);
        assert_eq!(lf.sneak_c2(), None);
    }

    #[test]
    fn sneak_repeat_with_no_last_find_returns_no_motion() {
        let o = opts();
        let lf = LastFind::new();
        let ctx = make_ctx("ab cd ab ef", 0, 1, &o);
        assert_eq!(semicolon(&ctx, &lf), MotionResult::NoMotion);
        assert_eq!(comma(&ctx, &lf), MotionResult::NoMotion);
    }

    // ── Smartcase flag propagation for ; and , ──────────────────────

    /// Original `fA` with ignorecase+smartcase stores case-sensitive flags.
    /// When repeating with `;` but options changed to noignorecase, the
    /// stored flags should still be used (case-sensitive search).
    #[test]
    fn semicolon_uses_stored_case_flags_not_current() {
        // Original find: ignorecase=true, smartcase=true, target='A' (uppercase → case-sensitive)
        // Text: "xAxBxAx" — 'A' at 1, 'B' at 3, 'A' at 5
        let mut lf = LastFind::new();
        lf.record_with_case(FindDirection::FindForward, 'A', true, true);

        // Repeat with DIFFERENT current options (ignorecase=false, smartcase=false).
        // The stored flags (ignorecase=true, smartcase=true) should be used,
        // meaning uppercase 'A' is case-sensitive → finds 'A' at 5, skipping 'a'.
        let mut o = opts();
        o.set_ignorecase(false);
        o.set_smartcase(false);
        let ctx = make_ctx("xAxaxAx", 2, 1, &o);
        let result = semicolon(&ctx, &lf);
        // With stored flags (ic=true, sc=true, target='A'), uppercase target
        // forces case-sensitive match. Skips lowercase 'a' at 3, finds 'A' at 5.
        assert_eq!(result, MotionResult::Position(Offset::new(5)));
    }

    /// Original `fa` with ignorecase=true (case-insensitive) finds 'A' too.
    /// Repeating `;` with ignorecase now false should still use stored flags.
    #[test]
    fn semicolon_case_insensitive_stored_finds_uppercase() {
        // Original find: ignorecase=true, smartcase=false → case-insensitive
        let mut lf = LastFind::new();
        lf.record_with_case(FindDirection::FindForward, 'a', true, false);

        // Current options: ignorecase=false (exact match)
        // But stored flags say ignorecase=true, so it should still match 'A'.
        let mut o = opts();
        o.set_ignorecase(false);
        let ctx = make_ctx("xAx", 0, 1, &o);
        let result = semicolon(&ctx, &lf);
        // Stored ic=true → case-insensitive, so 'a' matches 'A' at offset 1.
        assert_eq!(result, MotionResult::Position(Offset::new(1)));
    }

    /// Comma (reverse repeat) also uses stored case flags.
    #[test]
    fn comma_uses_stored_case_flags() {
        // Original find forward with ignorecase=true, smartcase=true, target='a'
        // (lowercase target + smartcase → case-insensitive)
        let mut lf = LastFind::new();
        lf.record_with_case(FindDirection::FindForward, 'a', true, true);

        // Current options: ignorecase=false (would be case-sensitive)
        let mut o = opts();
        o.set_ignorecase(false);
        // Comma reverses to FindBackward. Search backward for 'a' from offset 3.
        // With stored ic=true, sc=true, target='a' (lowercase) → case-insensitive
        let ctx = make_ctx("xAxa", 3, 1, &o);
        let result = comma(&ctx, &lf);
        // Case-insensitive backward from 3: finds 'A' at 1 (or 'a' at... wait 'x' at 2)
        // Text "xAxa": x=0, A=1, x=2, a=3. Backward from 3: finds 'A' at 1.
        assert_eq!(result, MotionResult::Position(Offset::new(1)));
    }

    /// record (without _with_case) defaults to ignorecase=false, smartcase=false.
    /// This ensures backward compatibility.
    #[test]
    fn record_without_case_defaults_to_exact_match() {
        let mut lf = LastFind::new();
        lf.record(FindDirection::FindForward, 'a');
        assert!(!lf.resolved_ignorecase());
        assert!(!lf.resolved_smartcase());
    }

    /// record_with_case stores and retrieves the flags correctly.
    #[test]
    fn record_with_case_stores_flags() {
        let mut lf = LastFind::new();
        lf.record_with_case(FindDirection::FindForward, 'x', true, true);
        assert!(lf.resolved_ignorecase());
        assert!(lf.resolved_smartcase());
    }

    /// record_sneak_with_case stores case flags for sneak too.
    #[test]
    fn record_sneak_with_case_stores_flags() {
        let mut lf = LastFind::new();
        lf.record_sneak_with_case(FindDirection::SneakForward, 'a', 'b', true, false);
        assert!(lf.resolved_ignorecase());
        assert!(!lf.resolved_smartcase());
        assert_eq!(lf.sneak_c2(), Some('b'));
    }

    // ── Sneak repeat uses stored case flags (not current options) ──

    /// Sneak forward with ignorecase+smartcase stored, then repeat (;) with
    /// smartcase turned off. The stored flags should still govern matching.
    ///
    /// Text: "xABxabxAB"
    ///   - Original sneak for 'a','b' with ic=true, sc=true → lowercase target
    ///     is case-insensitive, so "AB" at offset 1 matches.
    ///   - After landing at offset 1, repeat (;) should use stored flags
    ///     (ic=true, sc=true) even though current options have sc=false.
    ///   - With stored flags, lowercase 'a' is still case-insensitive, so
    ///     next match is "ab" at offset 4.
    #[test]
    fn sneak_repeat_semicolon_uses_stored_case_flags() {
        // Record sneak with ignorecase=true, smartcase=true, target='a','b' (lowercase)
        let mut lf = LastFind::new();
        lf.record_sneak_with_case(FindDirection::SneakForward, 'a', 'b', true, true);

        // Current options: ignorecase=false, smartcase=false (opposite of stored)
        let mut o = opts();
        o.set_ignorecase(false);
        o.set_smartcase(false);

        // "xABxabxAB" — AB at 1, ab at 4, AB at 7
        // Cursor at offset 2 (after first AB). Repeat should use stored ic=true,sc=true.
        // Lowercase target 'a' with sc=true → case-insensitive. Next match: "ab" at 4.
        let ctx = make_ctx("xABxabxAB", 2, 1, &o);
        let result = semicolon(&ctx, &lf);
        assert_eq!(result, MotionResult::Position(Offset::new(4)));
    }

    /// Sneak forward with exact match stored (ic=false), then repeat (;) with
    /// ignorecase now turned on. Stored exact-match flags should still be used.
    ///
    /// Text: "xABxabxAB"
    ///   - Stored: ic=false → exact match for 'a','b' (lowercase only).
    ///   - Current options: ic=true (would be case-insensitive).
    ///   - Repeat should use stored ic=false, so "AB" is skipped.
    #[test]
    fn sneak_repeat_semicolon_stored_exact_ignores_current_ignorecase() {
        let mut lf = LastFind::new();
        lf.record_sneak_with_case(FindDirection::SneakForward, 'a', 'b', false, false);

        // Current options have ignorecase=true, but stored has ic=false
        let mut o = opts();
        o.set_ignorecase(true);
        o.set_smartcase(false);

        // "xABxabxAB" — from offset 2, stored ic=false means exact 'a','b'.
        // "AB" at 7 doesn't match (uppercase), "ab" at 4 matches.
        let ctx = make_ctx("xABxabxAB", 2, 1, &o);
        let result = semicolon(&ctx, &lf);
        assert_eq!(result, MotionResult::Position(Offset::new(4)));
    }

    /// Sneak comma (reverse repeat) also uses stored case flags, not current.
    ///
    /// Text: "ABxabx"
    ///   - Stored sneak forward with ic=true, sc=true, target='a','b'.
    ///   - Comma reverses to SneakBackward. Cursor at 5.
    ///   - Stored flags: ic=true, sc=true, lowercase → case-insensitive.
    ///   - Should find "ab" at offset 3 first, then "AB" at 0.
    #[test]
    fn sneak_repeat_comma_uses_stored_case_flags() {
        let mut lf = LastFind::new();
        lf.record_sneak_with_case(FindDirection::SneakForward, 'a', 'b', true, true);

        // Current options: opposite of stored
        let mut o = opts();
        o.set_ignorecase(false);
        o.set_smartcase(false);

        // "ABxabx" — AB at 0, ab at 3. Cursor at 5.
        // Comma reverses SneakForward → SneakBackward. Stored ic=true,sc=true.
        // Backward from 5: search_text "ABxab" → matches: "AB" at 0, "ab" at 3.
        // count-th from end (count=1) → last match = "ab" at 3.
        let ctx = make_ctx("ABxabx", 5, 1, &o);
        let result = comma(&ctx, &lf);
        assert_eq!(result, MotionResult::Position(Offset::new(3)));

        // From offset 3, backward again should find "AB" at 0 (case-insensitive).
        let ctx2 = make_ctx("ABxabx", 3, 1, &o);
        let result2 = comma(&ctx2, &lf);
        assert_eq!(result2, MotionResult::Position(Offset::new(0)));
    }
}
