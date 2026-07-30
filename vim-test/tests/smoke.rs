use vim_test::prelude::*;

#[test]
fn builder_simple_motion() {
    vim("|hello world").keys("w").expect_cursor(0, 6).run();
}

#[test]
fn builder_delete_word() {
    vim("|hello world").keys("dw").expect_text("|world").run();
}

#[test]
fn builder_delete_and_undo() {
    vim("|hello world")
        .keys("dw")
        .expect_text("|world")
        .labeled("after delete")
        .undo(1)
        .expect_text("|hello world")
        .labeled("after undo")
        .run();
}

#[test]
fn builder_register_check() {
    vim("|hello world")
        .keys("dw")
        .expect_register('"', "hello ")
        .run();
}

#[test]
fn builder_mode_check() {
    vim("|hello")
        .keys("i")
        .expect_mode(Mode::Insert)
        .keys("<Esc>")
        .expect_mode(Mode::Normal)
        .run();
}

#[test]
fn builder_change_count() {
    vim("|hello world")
        .expect_change_count(0)
        .keys("x")
        .expect_change_count(1)
        .keys("x")
        .expect_change_count(2)
        .run();
}

#[test]
fn builder_multi_step_with_labels() {
    vim("|foo bar baz")
        .keys("dw")
        .expect_text("|bar baz")
        .labeled("first delete")
        .keys("dw")
        .expect_text("|baz")
        .labeled("second delete")
        .undo(2)
        .expect_text("|foo bar baz")
        .labeled("full undo")
        .run();
}

#[test]
fn session_direct_usage() {
    let mut s = TestSession::new("|hello world");
    s.feed("dw");
    assert_text(&s, "|world");
    assert_eq!(s.change_count(), 1);
}

#[test]
fn session_multi_cursor() {
    let s = TestSession::new_multi("|1hello |2world");
    assert_eq!(s.cursor_count(), 2);
    assert_eq!(s.text(), "hello world");
}

#[test]
fn undo_atomic_delete() {
    let mut s = TestSession::new("|hello world");
    assert_atomic(&mut s, "dw");
}

#[test]
fn undo_atomic_x() {
    let mut s = TestSession::new("|hello");
    assert_atomic(&mut s, "x");
}

#[test]
fn undo_round_trip_delete() {
    let mut s = TestSession::new("|hello world");
    assert_round_trip(&mut s, "dw");
}

#[test]
fn undo_round_trip_multiple_ops() {
    let mut s = TestSession::new("|hello world test");
    assert_round_trip(&mut s, "dw");
    assert_round_trip(&mut s, "dw");
}

// ── Shadow–Undo sync regression tests ──────────────────────────────────────

#[test]
fn consecutive_undo_ciw_then_insert() {
    let mut s = TestSession::new("|aaa bbb\nccc ddd");
    s.feed("ciwXXX<Esc>");
    assert_text(&s, "XX|X bbb\nccc ddd");
    s.feed("eaYYY<Esc>");
    s.feed("u");
    assert_eq!(s.text(), "XXX bbb\nccc ddd", "undo second edit");
    s.feed("u");
    assert_eq!(
        s.text(),
        "aaa bbb\nccc ddd",
        "undo first edit — must not skip"
    );
}

#[test]
fn consecutive_undo_two_ciw_different_lines() {
    let mut s = TestSession::new("|aaa bbb\nccc ddd");
    s.feed("ciwXXX<Esc>");
    s.feed("jciwYYY<Esc>");
    s.feed("u");
    assert_eq!(s.text(), "XXX bbb\nccc ddd", "undo second ciw");
    s.feed("u");
    assert_eq!(
        s.text(),
        "aaa bbb\nccc ddd",
        "undo first ciw — must not skip"
    );
}

#[test]
fn undo_round_trip_ciw() {
    let mut s = TestSession::new("|aaa bbb ccc");
    assert_round_trip(&mut s, "ciwXXX<Esc>");
}

#[test]
fn undo_round_trip_consecutive_edits() {
    let mut s = TestSession::new("|aaa bbb ccc");
    assert_round_trip(&mut s, "ciwXXX<Esc>");
    assert_round_trip(&mut s, "wciwYYY<Esc>");
}

#[test]
fn undo_atomic_ciw() {
    let mut s = TestSession::new("|hello world");
    assert_atomic(&mut s, "ciwNEW<Esc>");
}
