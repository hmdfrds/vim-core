//! Tests for [`VimEngine::host_settings()`].

use super::super::VimEngine;
use crate::primitives::{CursorShape, HostSettings, Mode};

#[test]
fn host_settings_normal_mode() {
    let engine = VimEngine::new();
    let settings = engine.host_settings();
    assert_eq!(settings.cursor_style.shape, CursorShape::Block);
    assert!(!settings.cursor_style.blink);
    assert!(!settings.input_enabled);
    assert!(settings.clip_at_eol);
    assert!(!settings.line_mode);
    assert!(!settings.relative_line_numbers);
    assert_eq!(settings.mode_appearance.name, "NORMAL");
}

#[test]
fn host_settings_respects_relativenumber() {
    let mut engine = VimEngine::new();
    engine.options_mut().set_relativenumber(true);
    engine.invalidate_option_cache();
    let settings = engine.host_settings();
    assert!(settings.relative_line_numbers);
}

#[test]
fn host_settings_after_mode_change() {
    let mut engine = VimEngine::new();
    engine.set_mode(Mode::Insert);
    let settings = engine.host_settings();
    assert_eq!(settings.cursor_style.shape, CursorShape::VerticalBar);
    assert!(settings.input_enabled);
    assert!(!settings.clip_at_eol);
}

#[test]
fn host_settings_returns_correct_type() {
    let engine = VimEngine::new();
    let settings = engine.host_settings();
    // Verify the return type matches HostSettings (compile-time check via binding).
    let _: HostSettings = settings;
}
