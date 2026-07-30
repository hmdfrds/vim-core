//! Auto-pairs test suite using the vim-test framework.
//!
//! Tests single-char auto-pair behavior handled by vim-core's engine:
//! bracket auto-close, skip-over, quote suppression, backspace pair
//! deletion, undo atomicity, multi-cursor, and dot-repeat.
//!
//! Note: Esc in insert mode backs cursor up 1 position. All expected
//! cursor positions account for this.

use vim_test::prelude::*;

// ═══════════════════════════════════════════════════════════════════════════
// 1. OPENING BRACKET AUTO-CLOSE
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn auto_pair_open_paren() {
    // i( → insert "()" cursor between → Esc backs to offset 0
    vim("|")
        .with_auto_pairs()
        .keys("i(")
        .expect_mode(Mode::Insert)
        .expect_text("(|)")
        .labeled("( auto-closes with cursor between")
        .keys("<Esc>")
        .expect_text("|()")
        .run();
}

#[test]
fn auto_pair_open_bracket() {
    vim("|")
        .with_auto_pairs()
        .keys("i[")
        .expect_text("[|]")
        .keys("<Esc>")
        .expect_text("|[]")
        .run();
}

#[test]
fn auto_pair_open_brace() {
    vim("|")
        .with_auto_pairs()
        .keys("i{")
        .expect_text("{|}")
        .keys("<Esc>")
        .expect_text("|{}")
        .run();
}

#[test]
fn auto_pair_open_before_space() {
    vim("| hello")
        .with_auto_pairs()
        .keys("i(")
        .expect_text("(|) hello")
        .labeled("( before space auto-closes")
        .run();
}

#[test]
fn auto_pair_open_before_closer() {
    vim("|)")
        .with_auto_pairs()
        .keys("i(")
        .expect_text("(|))")
        .labeled("( before ) auto-closes")
        .run();
}

#[test]
fn auto_pair_no_close_before_alpha() {
    vim("|hello")
        .with_auto_pairs()
        .keys("i(")
        .expect_text("(|hello")
        .labeled("( before alpha does NOT auto-close")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. CLOSING BRACKET SKIP-OVER
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn auto_pair_skip_over_close_paren() {
    // i( → "()" cursor@1, ) → skip-over cursor@2, Esc → cursor@1
    vim("|")
        .with_auto_pairs()
        .keys("i()")
        .expect_text("()|")
        .labeled(") skips over auto-closed )")
        .keys("<Esc>")
        .expect_text("(|)")
        .run();
}

#[test]
fn auto_pair_skip_over_close_bracket() {
    vim("|")
        .with_auto_pairs()
        .keys("i[]")
        .expect_text("[]|")
        .labeled("] skips over auto-closed ]")
        .keys("<Esc>")
        .expect_text("[|]")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. SAME-CHAR PAIRS (QUOTES)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn auto_pair_double_quote() {
    vim("|")
        .with_auto_pairs()
        .keys("i\"")
        .expect_text("\"|\"")
        .labeled("\" auto-closes to \"\"")
        .run();
}

#[test]
fn auto_pair_single_quote() {
    vim("|")
        .with_auto_pairs()
        .keys("i'")
        .expect_text("'|'")
        .labeled("' auto-closes to ''")
        .run();
}

#[test]
fn auto_pair_backtick() {
    vim("|")
        .with_auto_pairs()
        .keys("i`")
        .expect_text("`|`")
        .labeled("` auto-closes to ``")
        .run();
}

#[test]
fn auto_pair_quote_skip_over() {
    // i" → '""' cursor@1, " → skip-over cursor@2
    vim("|")
        .with_auto_pairs()
        .keys("i\"\"")
        .expect_text("\"\"|")
        .labeled("second \" skips over the auto-closed one")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. QUOTE SUPPRESSION AFTER ALPHANUMERIC
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn auto_pair_quote_after_alpha_no_close() {
    vim("|")
        .with_auto_pairs()
        .keys("ia\"")
        .expect_text("a\"|")
        .labeled("\" after alpha does NOT auto-close")
        .run();
}

#[test]
fn auto_pair_quote_after_digit_no_close() {
    vim("|")
        .with_auto_pairs()
        .keys("i1\"")
        .expect_text("1\"|")
        .labeled("\" after digit does NOT auto-close")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. BACKSPACE PAIR DELETION
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn auto_pair_backspace_deletes_both() {
    vim("|")
        .with_auto_pairs()
        .keys("i(<BS><Esc>")
        .expect_text("|")
        .labeled("backspace between () deletes both")
        .run();
}

#[test]
fn auto_pair_backspace_quote_deletes_both() {
    vim("|")
        .with_auto_pairs()
        .keys("i\"<BS><Esc>")
        .expect_text("|")
        .labeled("backspace between \"\" deletes both")
        .run();
}

#[test]
fn auto_pair_backspace_non_pair_normal() {
    vim("|")
        .with_auto_pairs()
        .keys("iab<BS><Esc>")
        .expect_text("|a")
        .labeled("backspace on non-pair just deletes one char")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. UNDO ATOMICITY
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn auto_pair_undo_atomic() {
    vim("|")
        .with_auto_pairs()
        .keys("i(<Esc>")
        .keys("u")
        .expect_text("|")
        .labeled("u undoes entire () as one atom")
        .run();
}

#[test]
fn auto_pair_type_and_undo_atomic() {
    vim("|")
        .with_auto_pairs()
        .keys("i(hello<Esc>")
        .keys("u")
        .expect_text("|")
        .labeled("u undoes (hello) as one atom")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. MULTI-CURSOR AUTO-PAIRS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn auto_pair_multi_cursor_open() {
    vim_mc("|1 |2")
        .with_auto_pairs()
        .keys("i(")
        .expect_mode(Mode::Insert)
        .expect_cursor_count(2)
        .labeled("auto-pair at each cursor in insert mode")
        .run();
}

#[test]
fn auto_pair_multi_cursor_undo() {
    vim_mc("|1 |2")
        .with_auto_pairs()
        .keys("i(<Esc>")
        .keys("u")
        .expect_text("| ")
        .labeled("u reverts all auto-pairs atomically")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. WITHOUT AUTO-PAIRS (control group)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_auto_pair_by_default() {
    vim("|")
        .keys("i(")
        .expect_text("(|")
        .labeled("without with_auto_pairs(), ( inserts just (")
        .run();
}

#[test]
fn no_auto_pair_quote_by_default() {
    vim("|")
        .keys("i\"")
        .expect_text("\"|")
        .labeled("without with_auto_pairs(), \" inserts just \"")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. DOT-REPEAT WITH AUTO-PAIRS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn dot_repeat_reproduces_auto_pair_paren() {
    vim("|")
        .with_auto_pairs()
        .keys("i(<Esc>")
        .expect_text("|()")
        .labeled("initial insert: ( auto-closes to ()")
        .keys("A(<Esc>")
        .expect_text("()|()")
        .labeled("second insert at end also auto-closes")
        .keys(".")
        .expect_text("()()|()") // BUG: currently produces "()()|(" — missing )
        .labeled("dot-repeat must reproduce the closing )")
        .run();
}

#[test]
fn dot_repeat_reproduces_auto_pair_bracket() {
    vim("|")
        .with_auto_pairs()
        .keys("i[<Esc>")
        .expect_text("|[]")
        .labeled("initial insert: [ auto-closes to []")
        .keys("A[<Esc>")
        .expect_text("[]|[]")
        .labeled("second insert at end also auto-closes")
        .keys(".")
        .expect_text("[][]|[]") // BUG: currently produces "[][]|[" — missing ]
        .labeled("dot-repeat must reproduce the closing ]")
        .run();
}

#[test]
fn dot_repeat_reproduces_auto_pair_with_content() {
    vim("|")
        .with_auto_pairs()
        .keys("i(hello<Esc>")
        .expect_text("(hell|o)")
        .labeled("initial insert: (hello) with auto-pair")
        .keys("A(world<Esc>")
        .expect_text("(hello)(worl|d)")
        .labeled("second insert with content and auto-pair")
        .keys(".")
        .expect_text("(hello)(world)(worl|d)") // BUG: currently "(world" — missing )
        .labeled("dot-repeat reproduces content + closing paren")
        .run();
}
