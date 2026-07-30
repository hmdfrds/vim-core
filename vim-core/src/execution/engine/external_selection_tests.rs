//! Tests for [`VimEngine::notify_external_selection`].

use super::super::VimEngine;
use crate::effects::Effect;
use crate::primitives::Mode;
use crate::primitives::VisualType;

// ── Normal + has_selection → Visual ─────────────────────────────────────────

#[test]
fn normal_mode_with_selection_switches_to_visual_char() {
    let mut engine = VimEngine::new();
    assert_eq!(engine.mode(), Mode::Normal);

    let response = engine.notify_external_selection(true, false);

    assert_eq!(engine.mode(), Mode::Visual(VisualType::Char));
    assert!(
        response.effects().iter().any(|e| matches!(
            e,
            Effect::SetMode {
                mode: Mode::Visual(VisualType::Char),
                ..
            }
        )),
        "response should contain SetMode(Visual(Char))"
    );
}

#[test]
fn normal_mode_with_block_selection_switches_to_visual_block() {
    let mut engine = VimEngine::new();
    assert_eq!(engine.mode(), Mode::Normal);

    let response = engine.notify_external_selection(true, true);

    assert_eq!(engine.mode(), Mode::Visual(VisualType::Block));
    assert!(
        response.effects().iter().any(|e| matches!(
            e,
            Effect::SetMode {
                mode: Mode::Visual(VisualType::Block),
                ..
            }
        )),
        "response should contain SetMode(Visual(Block))"
    );
}

// ── Visual + no selection → Normal ──────────────────────────────────────────

#[test]
fn visual_mode_without_selection_switches_to_normal() {
    let mut engine = VimEngine::new();
    engine.set_mode(Mode::Visual(VisualType::Char));

    let response = engine.notify_external_selection(false, false);

    assert_eq!(engine.mode(), Mode::Normal);
    assert!(
        response.effects().iter().any(|e| matches!(
            e,
            Effect::SetMode {
                mode: Mode::Normal,
                ..
            }
        )),
        "response should contain SetMode(Normal)"
    );
}

#[test]
fn visual_line_mode_without_selection_switches_to_normal() {
    let mut engine = VimEngine::new();
    engine.set_mode(Mode::Visual(VisualType::Line));

    let response = engine.notify_external_selection(false, false);

    assert_eq!(engine.mode(), Mode::Normal);
}

#[test]
fn visual_block_mode_without_selection_switches_to_normal() {
    let mut engine = VimEngine::new();
    engine.set_mode(Mode::Visual(VisualType::Block));

    let response = engine.notify_external_selection(false, false);

    assert_eq!(engine.mode(), Mode::Normal);
}

// ── No-op cases ─────────────────────────────────────────────────────────────

#[test]
fn insert_mode_with_selection_no_change() {
    let mut engine = VimEngine::new();
    engine.set_mode(Mode::Insert);

    let response = engine.notify_external_selection(true, false);

    assert_eq!(engine.mode(), Mode::Insert);
    assert!(
        response.effects().is_empty(),
        "insert mode + selection should produce no effects"
    );
}

#[test]
fn normal_mode_without_selection_no_change() {
    let mut engine = VimEngine::new();
    assert_eq!(engine.mode(), Mode::Normal);

    let response = engine.notify_external_selection(false, false);

    assert_eq!(engine.mode(), Mode::Normal);
    assert!(
        response.effects().is_empty(),
        "normal mode + no selection should produce no effects"
    );
}

#[test]
fn visual_mode_with_selection_no_change() {
    let mut engine = VimEngine::new();
    engine.set_mode(Mode::Visual(VisualType::Char));

    let response = engine.notify_external_selection(true, false);

    assert_eq!(engine.mode(), Mode::Visual(VisualType::Char));
    assert!(
        response.effects().is_empty(),
        "already in visual + selection should produce no effects"
    );
}

#[test]
fn replace_mode_with_selection_no_change() {
    let mut engine = VimEngine::new();
    engine.set_mode(Mode::Replace);

    let response = engine.notify_external_selection(true, false);

    assert_eq!(engine.mode(), Mode::Replace);
    assert!(
        response.effects().is_empty(),
        "replace mode + selection should produce no effects"
    );
}
