//! Integration tests for sneak/leap motions.
//!
//! Validates:
//! - `sneak_mode` option gates s/S behavior
//! - s/S act as substitute when `sneak_mode=false` (default)
//! - s/S act as two-char cross-line find when `sneak_mode=true`
//! - SetLastFind carries sneak_c2 through the effect pipeline
//! - `;` and `,` repeat sneak motions correctly
//! - Unicode two-char targets work (e.g., sneak to "ueb" in "ueber")
//! - Toggling sneak_mode on/off changes s behavior dynamically

mod common;

use common::document::TestDocument;
use common::runner::apply_effect;
use vim_core::document::Document;
use vim_core::effects::Effect;
use vim_core::execution::{InputContext, VimEngine};
use vim_core::keymap::KeyEvent;
use vim_core::primitives::Mode;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Press a key, apply all resulting effects to `doc`, and return the cursor
/// offset after the key was processed.
fn press_and_apply(engine: &mut VimEngine, doc: &mut TestDocument, key: KeyEvent) -> usize {
    let ctx = InputContext::new(&*doc, doc.cursor_offset()).validate_clamped();
    let mut response = engine.process(key, ctx);
    let effects = response.take_effects();
    for effect in effects {
        apply_effect(doc, effect);
    }
    doc.cursor_offset()
}

/// Press a key and apply effects, returning the full effect list.
fn press_apply_effects(
    engine: &mut VimEngine,
    doc: &mut TestDocument,
    key: KeyEvent,
) -> Vec<Effect> {
    let ctx = InputContext::new(&*doc, doc.cursor_offset()).validate_clamped();
    let mut response = engine.process(key, ctx);
    let effects = response.take_effects();
    for effect in &effects {
        apply_effect(doc, effect.clone());
    }
    effects
}

// ── Tests: s/S as substitute when sneak_mode=false ──────────────────────────

#[test]
fn s_is_substitute_when_sneak_disabled() {
    // Default: sneak_mode=false. 's' should enter insert mode (substitute).
    let mut doc = TestDocument::new("hello", (0, 0));
    let mut engine = VimEngine::new();

    // 's' should delete char under cursor and enter insert mode
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('s'));
    assert_eq!(
        engine.mode(),
        Mode::Insert,
        "s should enter insert mode when sneak disabled"
    );
}

#[test]
fn upper_s_is_substitute_line_when_sneak_disabled() {
    let mut doc = TestDocument::new("hello", (0, 0));
    let mut engine = VimEngine::new();

    // 'S' should clear line and enter insert mode
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('S'));
    assert_eq!(
        engine.mode(),
        Mode::Insert,
        "S should enter insert mode (substitute line) when sneak disabled"
    );
}

// ── Tests: s/S as sneak when sneak_mode=true ────────────────────────────────

#[test]
fn s_is_sneak_when_enabled() {
    let mut doc = TestDocument::new("hello ab world", (0, 0));
    let mut engine = VimEngine::new();
    engine.options_mut().set_sneak_mode(true);
    engine.invalidate_option_cache();

    // s → pending (awaiting first char)
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('s'));
    assert_eq!(
        engine.mode(),
        Mode::Normal,
        "s with sneak should stay in Normal mode"
    );

    // 'a' → pending (awaiting second char)
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('a'));
    assert_eq!(
        engine.mode(),
        Mode::Normal,
        "after first sneak char, still Normal"
    );

    // 'b' → execute sneak, cursor moves to 'a' in "ab"
    let offset = press_and_apply(&mut engine, &mut doc, KeyEvent::char('b'));
    assert_eq!(offset, 6, "sneak 'ab' should land on 'a' at offset 6");
    assert_eq!(engine.mode(), Mode::Normal);
}

#[test]
fn upper_s_is_sneak_backward_when_enabled() {
    let mut doc = TestDocument::new("ab cd ab ef", (0, 0));
    // Move cursor to end
    doc.set_cursor_offset(10);
    let mut engine = VimEngine::new();
    engine.options_mut().set_sneak_mode(true);
    engine.invalidate_option_cache();

    // S → backward sneak
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('S'));
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('a'));
    let offset = press_and_apply(&mut engine, &mut doc, KeyEvent::char('b'));
    // "ab cd ab ef" — a=0,b=1, =2,c=3,d=4, =5,a=6,b=7, =8,e=9,f=10
    // From cursor 10, backward search text is "ab cd ab e" (offsets 0..10).
    // Last "ab" pair is at offset 6.
    assert_eq!(
        offset, 6,
        "backward sneak 'ab' from offset 10 should find 'ab' at offset 6"
    );
}

// ── Tests: sneak_mode toggle ────────────────────────────────────────────────

#[test]
fn toggle_sneak_mode_changes_s_behavior() {
    let mut doc = TestDocument::new("hello ab world", (0, 0));
    let mut engine = VimEngine::new();

    // Phase 1: sneak_mode=false (default). 's' = substitute (enters insert mode).
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('s'));
    assert_eq!(engine.mode(), Mode::Insert, "Phase 1: s should substitute");

    // Exit insert mode
    press_and_apply(&mut engine, &mut doc, KeyEvent::escape());
    assert_eq!(engine.mode(), Mode::Normal);

    // Reset doc state for next phase
    doc = TestDocument::new("hello ab world", (0, 0));

    // Phase 2: Enable sneak_mode. 's' = sneak (stays in normal, awaits chars).
    engine.options_mut().set_sneak_mode(true);
    engine.invalidate_option_cache();

    press_and_apply(&mut engine, &mut doc, KeyEvent::char('s'));
    assert_eq!(
        engine.mode(),
        Mode::Normal,
        "Phase 2: s with sneak should stay Normal"
    );

    // Complete the sneak
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('a'));
    let offset = press_and_apply(&mut engine, &mut doc, KeyEvent::char('b'));
    assert_eq!(offset, 6, "Phase 2: sneak 'ab' should find at offset 6");

    // Phase 3: Disable sneak_mode again. 's' = substitute again.
    engine.options_mut().set_sneak_mode(false);
    engine.invalidate_option_cache();

    // Need fresh doc since cursor moved
    doc = TestDocument::new("hello world", (0, 0));
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('s'));
    assert_eq!(
        engine.mode(),
        Mode::Insert,
        "Phase 3: s should substitute again after disabling sneak"
    );
}

// ── Tests: SetLastFind carries sneak_c2 ─────────────────────────────────────

#[test]
fn set_last_find_carries_sneak_c2_in_effects() {
    let mut doc = TestDocument::new("ab cd ab ef", (0, 0));
    let mut engine = VimEngine::new();
    engine.options_mut().set_sneak_mode(true);
    engine.invalidate_option_cache();

    // Perform sneak: s + a + b
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('s'));
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('a'));
    let effects = press_apply_effects(&mut engine, &mut doc, KeyEvent::char('b'));

    // Check that SetLastFind effect has sneak_c2
    let has_sneak_last_find = effects.iter().any(|e| {
        matches!(
            e,
            Effect::SetLastFind {
                sneak_c2: Some('b'),
                ..
            }
        )
    });
    assert!(
        has_sneak_last_find,
        "SetLastFind should carry sneak_c2='b' in its effect. Effects: {effects:?}"
    );
}

#[test]
fn sneak_repeat_semicolon_and_comma() {
    // Three occurrences of "ab": at offsets 0, 6, 12
    let mut doc = TestDocument::new("ab cd ab cd ab", (0, 0));
    let mut engine = VimEngine::new();
    engine.options_mut().set_sneak_mode(true);
    engine.invalidate_option_cache();

    // sab → find first 'ab' after cursor 0 → offset 6
    // (search starts at 1, finds 'ab' starting at position 6 in original text)
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('s'));
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('a'));
    let offset = press_and_apply(&mut engine, &mut doc, KeyEvent::char('b'));
    // "ab cd ab cd ab" — offsets: a=0,b=1, =2,c=3,d=4, =5,a=6,b=7, =8,c=9,d=10, =11,a=12,b=13
    assert_eq!(offset, 6, "first sneak 'ab' from 0 should land at 6");

    // `;` → repeat sneak forward → should find 'ab' at offset 12
    let offset = press_and_apply(&mut engine, &mut doc, KeyEvent::char(';'));
    assert_eq!(offset, 12, "semicolon repeat should find next 'ab' at 12");

    // `,` → repeat in reverse (backward) → should find 'ab' at offset 6
    let offset = press_and_apply(&mut engine, &mut doc, KeyEvent::char(','));
    assert_eq!(offset, 6, "comma reverse should find previous 'ab' at 6");
}

// ── Tests: sneak cross-line ─────────────────────────────────────────────────

#[test]
fn sneak_crosses_line_boundaries() {
    let mut doc = TestDocument::new("foo\nbar", (0, 0));
    let mut engine = VimEngine::new();
    engine.options_mut().set_sneak_mode(true);
    engine.invalidate_option_cache();

    // sneak 'ba' should cross newline and find 'ba' in "bar"
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('s'));
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('b'));
    let offset = press_and_apply(&mut engine, &mut doc, KeyEvent::char('a'));
    // "foo\nbar" — f=0,o=1,o=2,\n=3,b=4,a=5,r=6
    assert_eq!(
        offset, 4,
        "sneak 'ba' should cross line boundary to offset 4"
    );
}

// ── Tests: Unicode sneak ────────────────────────────────────────────────────

#[test]
fn sneak_unicode_two_char_target() {
    // Test sneak finding a two-char sequence starting with a Unicode character.
    // "ueber" with u-umlaut: umlaut is 2 bytes in UTF-8
    let mut doc = TestDocument::new("xx \u{00fc}ber yy", (0, 0));
    let mut engine = VimEngine::new();
    engine.options_mut().set_sneak_mode(true);
    engine.invalidate_option_cache();

    // sneak for umlaut-u + 'b'
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('s'));
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('\u{00fc}'));
    let offset = press_and_apply(&mut engine, &mut doc, KeyEvent::char('b'));
    // "xx " = 3 bytes, then U+00FC = 2 bytes at offset 3
    assert_eq!(offset, 3, "sneak should find Unicode char at offset 3");
}

#[test]
fn sneak_cjk_two_char_target() {
    // Test with CJK characters (3 bytes each in UTF-8)
    let mut doc = TestDocument::new("aa\u{4f60}\u{597d}bb", (0, 0));
    let mut engine = VimEngine::new();
    engine.options_mut().set_sneak_mode(true);
    engine.invalidate_option_cache();

    // sneak for CJK pair
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('s'));
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('\u{4f60}'));
    let offset = press_and_apply(&mut engine, &mut doc, KeyEvent::char('\u{597d}'));
    // "aa" = 2 bytes, then U+4F60 at offset 2
    assert_eq!(offset, 2, "sneak should find CJK pair at offset 2");
}

// ── Tests: sneak with count ─────────────────────────────────────────────────

#[test]
fn sneak_with_count_skips_occurrences() {
    let mut doc = TestDocument::new("ab cd ab cd ab", (0, 0));
    let mut engine = VimEngine::new();
    engine.options_mut().set_sneak_mode(true);
    engine.invalidate_option_cache();

    // 2sab → skip first 'ab', find second
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('2'));
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('s'));
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('a'));
    let offset = press_and_apply(&mut engine, &mut doc, KeyEvent::char('b'));
    // First 'ab' after cursor is at 6, second at 12
    assert_eq!(offset, 12, "2sab should skip to second 'ab' at offset 12");
}

// ── Tests: operator + sneak ─────────────────────────────────────────────────

#[test]
fn delete_with_sneak_motion() {
    let mut doc = TestDocument::new("hello ab world", (0, 0));
    let mut engine = VimEngine::new();
    engine.options_mut().set_sneak_mode(true);
    engine.invalidate_option_cache();

    // dsab → delete from cursor to sneak target 'ab'
    // sneak finds 'ab' at offset 6. compute_find_range for forward creates
    // range [cursor, target+charlen) = [0, 7), deleting "hello a" (inclusive).
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('d'));
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('s'));
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('a'));
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('b'));

    let text = doc.text().to_string();
    // Range [0, 7) deletes "hello a", leaving "b world"
    assert_eq!(
        text, "b world",
        "after dsab from 0 to sneak target at 6, text should be 'b world', got: {text:?}"
    );
}
