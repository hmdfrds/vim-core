use vim_test::prelude::*;

// ── EffectInspector ─────────────────────────────────────────

#[test]
fn effect_inspector_of_kind() {
    let mut s = TestSession::new("|hello");
    s.feed("x");
    let inspector = EffectInspector::new(s.last_effects());
    inspector.of_kind(EffectKind::Delete).expect_count(1);
    inspector.of_kind(EffectKind::Insert).expect_none();
}

#[test]
fn effect_inspector_sequence() {
    let mut s = TestSession::new("|hello");
    s.feed("x");
    let inspector = EffectInspector::new(s.last_effects());
    inspector.expect_sequence_contains(&[EffectKind::BeginUndoGroup, EffectKind::Delete]);
}

#[test]
fn effect_inspector_before() {
    let mut s = TestSession::new("|hello");
    s.feed("x");
    let inspector = EffectInspector::new(s.last_effects());
    inspector.expect_before(EffectKind::BeginUndoGroup, EffectKind::Delete);
}

#[test]
fn effect_inspector_excluding() {
    let mut s = TestSession::new("|hello");
    s.feed("x");
    let inspector = EffectInspector::new(s.last_effects());
    let filtered = inspector.excluding_internal();
    assert!(filtered.count() > 0);
}

// ── Builder new methods ─────────────────────────────────────

#[test]
fn builder_with_register() {
    vim("|hello")
        .with_register('a', "WORLD")
        .keys("\"ap")
        .expect_text("hWORL|Dello")
        .run();
}

#[test]
fn builder_expect_cursors() {
    let mut s = TestSession::new_multi("|1hello |2world");
    s.feed("l");
    assert_cursors(&s, &[(0, 1), (0, 7)]);
}

#[test]
fn builder_set_register_mid_test() {
    vim("|hello")
        .set_register('a', "X")
        .keys("\"ap")
        .expect_text("h|Xello")
        .run();
}

// ── vim_block! macro ────────────────────────────────────────

#[test]
fn vim_block_multiline() {
    vim_block!("|first line", "second line")
        .keys("dd")
        .expect_text("|second line")
        .run();
}

// ── mc_vim_spec! / mc_vim_suite! ────────────────────────────

mc_vim_spec!(mc_spec_tilde, "AaBb", [0, 2], "~" => "a|abb");

mc_vim_suite!(mc_suite_basic {
    smoke:    "hello", [0, 3], "x";
});

// ── TestSession::cursor_line_col ────────────────────────────

#[test]
fn session_cursor_line_col() {
    let mut s = TestSession::new("hello\n|world");
    assert_eq!(s.cursor_line_col(), (1, 0));
    s.feed("ll");
    assert_eq!(s.cursor_line_col(), (1, 2));
}

// ── MultiCursorSpec::to_selections ──────────────────────────

#[test]
fn multi_cursor_spec_to_selections() {
    let (_, spec) = parse_multi("|1hello |2world");
    let sels = spec.to_selections();
    assert_eq!(sels.len(), 2);
}

// ── assert_cursor_count ─────────────────────────────────────

#[test]
fn multi_cursor_assert_count() {
    let s = TestSession::new_multi("|1hello |2world |3test");
    assert_cursor_count(&s, 3);
}

// ── Builder assert_atomic / assert_round_trip ───────────────

#[test]
fn builder_assert_atomic() {
    vim("|hello world").assert_atomic("dw").run();
}

#[test]
fn builder_assert_round_trip() {
    vim("|hello world").assert_round_trip("dw").run();
}

// ── Builder expect_no_effect ────────────────────────────────

#[test]
fn builder_expect_no_effect() {
    vim("|hello")
        .keys("l")
        .expect_no_effect(EffectKind::Delete)
        .run();
}

// ── Builder with_option ─────────────────────────────────────

#[test]
fn builder_with_option() {
    vim("|hello\nworld")
        .with_option("scrolloff", OptionValue::Unsigned(2))
        .keys("j")
        .expect_cursor(1, 0)
        .run();
}

// ── to_selections sets CursorMode::Multi ────────────────────

// ── EffectLog via session.effects() ──────────────────────────

#[test]
fn effect_log_via_session() {
    let mut s = TestSession::new("|hello");
    s.feed("x");
    let log = s.effects();
    assert_eq!(log.step_count(), 1);
    assert!(log.total_count() > 0);
    let inspector = log.inspector();
    inspector.of_kind(EffectKind::Delete).expect_count(1);
}

// ── UndoSnapshot ────────────────────────────────────────────

#[test]
fn undo_snapshot_capture() {
    let s = TestSession::new("|hello");
    let snap = UndoSnapshot::capture(&s);
    assert_eq!(snap.text, "hello");
    assert_eq!(snap.cursor_offset, 0);
    assert_eq!(snap.change_count, 0);
}

// ── VimSnapshot full fields ─────────────────────────────────

#[test]
fn vim_snapshot_full_capture() {
    let mut s = TestSession::new("|hello world");
    s.feed("dw");
    let snap = VimSnapshot::capture(&s);
    assert_eq!(snap.text, "world");
    assert_eq!(snap.cursor_offset, 0);
    assert_eq!(snap.cursor_line, 0);
    assert_eq!(snap.cursor_col, 0);
    assert_eq!(snap.mode, Mode::Normal);
    assert!(snap.registers.iter().any(|r| r.name == '"'));
    assert_eq!(snap.change_count, 1);
    assert!(snap.can_undo);
    assert!(!snap.effects.is_empty());
}

// ── TestSession::annotated_multi ────────────────────────────

#[test]
fn session_annotated_multi() {
    let s = TestSession::new_multi("|1hello |2world");
    let annotated = s.annotated_multi();
    assert!(annotated.contains("|1"));
    assert!(annotated.contains("|2"));
}

#[test]
fn to_selections_multi_mode() {
    let (_, spec) = parse_multi("|1hello |2world");
    let sels = spec.to_selections();
    assert_eq!(sels.len(), 2);
    assert_eq!(sels.cursor_mode(), vim_core::primitives::CursorMode::Multi);
}
