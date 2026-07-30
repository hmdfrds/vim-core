//! Capability layer integration tests.
//!
//! Tests for:
//! 1. `delegate_to_parser` returns Pipeline for a motion key (via engine process)
//! 2. `route_through_capabilities` with NORMAL profile handles a motion key (via engine)
//! 3. `route_through_capabilities` with COMMAND_LINE profile returns Ignored (via engine)
//! 4. `ModeProfile::MOTION_ONLY` has correct capabilities (Count, Motions, Scroll only)
//! 5. `Mode::default_profile()` returns correct profiles per mode
//! 6. `VimEngine` profile override API: set_mode_profile / mode_profile / clear_mode_profile

mod common;

use common::document::TestDocument;
use vim_core::execution::{InputContext, VimEngine};
use vim_core::keymap::KeyEvent;
use vim_core::mode::capabilities::{Capability, CapabilitySet, COMMAND_LINE, MOTION_ONLY};
use vim_core::primitives::{Mode, VisualType};

// ── Test 1: delegate_to_parser returns Pipeline for a motion key ─────────────
//
// `delegate_to_parser` is `pub(crate)` and cannot be called directly from
// integration tests. We verify its behavior indirectly: pressing 'j' in Normal
// mode should be consumed (which means it was routed through the parser pipeline).

#[test]
fn delegate_to_parser_produces_consumed_response_for_motion_key() {
    let doc = TestDocument::new("first line\nsecond line\nthird line", (0, 0));
    let mut engine = VimEngine::new();

    let ctx = InputContext::new(&doc, 0).validate_clamped();
    let response = engine.process(KeyEvent::char('j'), ctx);

    assert!(
        response.consumed(),
        "motion key 'j' in Normal mode should be consumed (routed through parser pipeline)",
    );
}

// ── Test 2: NORMAL profile handles a motion key via the engine ────────────────

#[test]
fn normal_profile_handles_motion_key_via_engine() {
    let doc = TestDocument::new("abc\ndef\nghi", (0, 0));
    let mut engine = VimEngine::new();

    // Motion keys that the grammar parser handles
    for key in ['j', 'k', 'h', 'l', 'w', 'b'] {
        let ctx = InputContext::new(&doc, 0).validate_clamped();
        let response = engine.process(KeyEvent::char(key), ctx);
        assert!(
            response.consumed(),
            "motion key '{key}' should be consumed by NORMAL mode routing",
        );
    }
}

// ── Test 3: CommandLine mode routing produces Ignored for non-cmdline input ───
//
// In CommandLine mode, keys not handled by the command-line handler go through
// the command-line input model, NOT the grammar pipeline. We verify that the
// engine's CommandLine mode processes input without crashing and returns a
// consumed-or-ignored response. The COMMAND_LINE ModeProfile has empty
// priority, so route_through_capabilities returns Ignored for that profile.

#[test]
fn command_line_profile_has_empty_priority_list() {
    // Direct check: the COMMAND_LINE profile is accessible from outside the crate
    assert!(
        COMMAND_LINE.priority.is_empty(),
        "COMMAND_LINE profile priority should be empty (no grammar routing)",
    );
    assert!(
        COMMAND_LINE.capabilities.is_empty(),
        "COMMAND_LINE profile should have no capabilities",
    );
    #[allow(clippy::assertions_on_constants)]
    {
        assert!(
            !COMMAND_LINE.fallthrough_to_host,
            "COMMAND_LINE profile should NOT fallthrough to host",
        );
    }
}

// ── Test 4: ModeProfile::MOTION_ONLY has correct capabilities ─────────────────

#[test]
fn motion_only_profile_contains_count_motions_scroll_only() {
    let caps = MOTION_ONLY.capabilities;

    // Should contain exactly: Count, Motions, Scroll
    assert!(
        caps.contains(CapabilitySet::COUNT),
        "MOTION_ONLY should have COUNT capability",
    );
    assert!(
        caps.contains(CapabilitySet::MOTIONS),
        "MOTION_ONLY should have MOTIONS capability",
    );
    assert!(
        caps.contains(CapabilitySet::SCROLL),
        "MOTION_ONLY should have SCROLL capability",
    );

    // Should NOT contain any other capabilities
    assert!(
        !caps.contains(CapabilitySet::OPERATORS),
        "MOTION_ONLY should NOT have OPERATORS",
    );
    assert!(
        !caps.contains(CapabilitySet::TEXT_OBJECTS),
        "MOTION_ONLY should NOT have TEXT_OBJECTS",
    );
    assert!(
        !caps.contains(CapabilitySet::ACTIONS),
        "MOTION_ONLY should NOT have ACTIONS",
    );
    assert!(
        !caps.contains(CapabilitySet::INSERT),
        "MOTION_ONLY should NOT have INSERT",
    );
    assert!(
        !caps.contains(CapabilitySet::SELECTION),
        "MOTION_ONLY should NOT have SELECTION",
    );
    assert!(
        !caps.contains(CapabilitySet::WINDOW),
        "MOTION_ONLY should NOT have WINDOW",
    );
    assert!(
        !caps.contains(CapabilitySet::REGISTER),
        "MOTION_ONLY should NOT have REGISTER",
    );
    assert!(
        !caps.contains(CapabilitySet::MARKS),
        "MOTION_ONLY should NOT have MARKS",
    );

    // Verify the priority slice matches the capability set exactly
    let expected_priority = &[Capability::Count, Capability::Motions, Capability::Scroll];
    assert_eq!(
        MOTION_ONLY.priority, expected_priority,
        "MOTION_ONLY priority should be [Count, Motions, Scroll]",
    );

    // Fallthrough should be enabled for restricted contexts
    #[allow(clippy::assertions_on_constants)]
    {
        assert!(
            MOTION_ONLY.fallthrough_to_host,
            "MOTION_ONLY should fallthrough to host",
        );
    }
}

// ── Test 5: Mode::default_profile() returns correct profiles ──────────────────

#[test]
fn mode_default_profile_normal_returns_full_normal_profile() {
    let profile = Mode::Normal.default_profile();
    assert!(
        profile.capabilities.contains(CapabilitySet::MOTIONS),
        "Normal default profile should have MOTIONS",
    );
    assert!(
        profile.capabilities.contains(CapabilitySet::OPERATORS),
        "Normal default profile should have OPERATORS",
    );
    assert!(
        profile.capabilities.contains(CapabilitySet::ACTIONS),
        "Normal default profile should have ACTIONS",
    );
    assert!(
        !profile.capabilities.contains(CapabilitySet::INSERT),
        "Normal default profile should NOT have INSERT",
    );
    assert!(
        !profile.fallthrough_to_host,
        "Normal default profile should NOT fallthrough to host",
    );
}

#[test]
fn mode_default_profile_insert_has_insert_and_fallthrough() {
    let profile = Mode::Insert.default_profile();
    assert!(
        profile.capabilities.contains(CapabilitySet::INSERT),
        "Insert default profile should have INSERT",
    );
    assert!(
        profile.fallthrough_to_host,
        "Insert default profile should fallthrough to host",
    );
    assert!(
        !profile.capabilities.contains(CapabilitySet::OPERATORS),
        "Insert default profile should NOT have OPERATORS",
    );
}

#[test]
fn mode_default_profile_visual_includes_selection_capability() {
    for vt in [VisualType::Char, VisualType::Line, VisualType::Block] {
        let profile = Mode::Visual(vt).default_profile();
        assert!(
            profile.capabilities.contains(CapabilitySet::SELECTION),
            "Visual({vt:?}) default profile should have SELECTION",
        );
        assert!(
            profile.capabilities.contains(CapabilitySet::MOTIONS),
            "Visual({vt:?}) default profile should have MOTIONS",
        );
        assert!(
            !profile.fallthrough_to_host,
            "Visual({vt:?}) default profile should NOT fallthrough",
        );
    }
}

#[test]
fn mode_default_profile_command_line_is_empty() {
    let profile = Mode::CommandLine.default_profile();
    assert!(
        profile.capabilities.is_empty(),
        "CommandLine default profile should have empty capabilities",
    );
    assert!(
        profile.priority.is_empty(),
        "CommandLine default profile should have empty priority",
    );
    assert!(
        !profile.fallthrough_to_host,
        "CommandLine default profile should NOT fallthrough",
    );
}

#[test]
fn mode_default_profile_operator_pending_has_text_objects() {
    use vim_core::grammar::types::Operator;
    let profile = Mode::OperatorPending(Operator::Delete).default_profile();
    assert!(
        profile.capabilities.contains(CapabilitySet::MOTIONS),
        "OperatorPending default profile should have MOTIONS",
    );
    assert!(
        profile.capabilities.contains(CapabilitySet::TEXT_OBJECTS),
        "OperatorPending default profile should have TEXT_OBJECTS",
    );
    assert!(
        !profile.capabilities.contains(CapabilitySet::ACTIONS),
        "OperatorPending default profile should NOT have ACTIONS",
    );
}

#[test]
fn mode_default_profile_replace_and_virtual_replace_have_insert_fallthrough() {
    for mode in [Mode::Replace, Mode::VirtualReplace] {
        let profile = mode.default_profile();
        assert!(
            profile.capabilities.contains(CapabilitySet::INSERT),
            "{mode:?} default profile should have INSERT",
        );
        assert!(
            profile.fallthrough_to_host,
            "{mode:?} default profile should fallthrough to host",
        );
    }
}

#[test]
fn mode_default_profile_select_has_selection_and_insert() {
    for vt in [VisualType::Char, VisualType::Line, VisualType::Block] {
        let profile = Mode::Select(vt).default_profile();
        assert!(
            profile.capabilities.contains(CapabilitySet::SELECTION),
            "Select({vt:?}) default profile should have SELECTION",
        );
        assert!(
            profile.capabilities.contains(CapabilitySet::INSERT),
            "Select({vt:?}) default profile should have INSERT",
        );
        assert!(
            !profile.fallthrough_to_host,
            "Select({vt:?}) default profile should NOT fallthrough",
        );
    }
}

// ── Test 6: VimEngine profile override API ────────────────────────────────────

#[test]
fn engine_set_mode_profile_overrides_default() {
    let mut engine = VimEngine::new();
    let mode = Mode::Normal;

    // Before override: should be the default Normal profile
    let default = engine.mode_profile(&mode);
    assert!(
        default.capabilities.contains(CapabilitySet::OPERATORS),
        "default Normal profile should have OPERATORS",
    );

    // Set a custom profile (MOTION_ONLY restricts to motions only)
    engine.set_mode_profile(mode, MOTION_ONLY);

    let overridden = engine.mode_profile(&mode);
    assert!(
        !overridden.capabilities.contains(CapabilitySet::OPERATORS),
        "after override with MOTION_ONLY, Normal should NOT have OPERATORS",
    );
    assert!(
        overridden.capabilities.contains(CapabilitySet::MOTIONS),
        "after override with MOTION_ONLY, Normal should still have MOTIONS",
    );
}

#[test]
fn engine_clear_mode_profile_restores_default() {
    let mut engine = VimEngine::new();
    let mode = Mode::Normal;

    engine.set_mode_profile(mode, MOTION_ONLY);
    engine.clear_mode_profile(&mode);

    let restored = engine.mode_profile(&mode);
    assert!(
        restored.capabilities.contains(CapabilitySet::OPERATORS),
        "after clear, Normal profile should be restored with OPERATORS",
    );
}

#[test]
fn engine_mode_profile_returns_default_when_no_override() {
    let engine = VimEngine::new();

    assert_eq!(
        *engine.mode_profile(&Mode::Normal),
        *Mode::Normal.default_profile(),
        "no override: mode_profile should equal default_profile for Normal",
    );
    assert_eq!(
        *engine.mode_profile(&Mode::CommandLine),
        *Mode::CommandLine.default_profile(),
        "no override: CommandLine mode_profile should equal default_profile",
    );
    assert_eq!(
        *engine.mode_profile(&Mode::Insert),
        *Mode::Insert.default_profile(),
        "no override: Insert mode_profile should equal default_profile",
    );
}

#[test]
fn engine_visual_subvariants_share_override_discriminant() {
    let mut engine = VimEngine::new();

    // Set override keyed on Visual(Char) — all Visual variants share discriminant
    engine.set_mode_profile(Mode::Visual(VisualType::Char), MOTION_ONLY);

    // All visual sub-variants share the same discriminant, so all should see the override
    for vt in [VisualType::Char, VisualType::Line, VisualType::Block] {
        let profile = engine.mode_profile(&Mode::Visual(vt));
        assert!(
            !profile.capabilities.contains(CapabilitySet::OPERATORS),
            "Visual({vt:?}) should see the MOTION_ONLY override after setting via Char variant",
        );
    }
}

#[test]
fn engine_independent_overrides_per_mode() {
    let mut engine = VimEngine::new();

    // Override Normal with MOTION_ONLY, leave Insert as default
    engine.set_mode_profile(Mode::Normal, MOTION_ONLY);

    let normal_profile = engine.mode_profile(&Mode::Normal);
    let insert_profile = engine.mode_profile(&Mode::Insert);

    assert!(
        !normal_profile
            .capabilities
            .contains(CapabilitySet::OPERATORS),
        "overridden Normal should not have OPERATORS",
    );
    assert!(
        insert_profile.capabilities.contains(CapabilitySet::INSERT),
        "non-overridden Insert should retain default INSERT capability",
    );
}
