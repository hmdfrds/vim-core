//! Tests for [`VimEngine::register_host_mappings`] and
//! [`VimEngine::unregister_host_mappings`].

use super::super::VimEngine;
use super::HostMapping;
use crate::keymap::{MappingMode, MappingOwner, TrieLookup};

/// Build a minimal `HostMapping` for testing.
fn host_mapping(
    modes: &[MappingMode],
    lhs: &str,
    rhs: &str,
    recursive: bool,
    silent: bool,
) -> HostMapping {
    HostMapping {
        modes: modes.iter().copied().collect(),
        lhs: compact_str::CompactString::from(lhs),
        rhs: compact_str::CompactString::from(rhs),
        recursive,
        silent,
        description: None,
    }
}

// ─── Basic registration ───────────────────────────────────────────────────────

/// Register a single mapping in Normal mode; verify it can be found in the keymap.
#[test]
fn register_single_normal_mapping_exists() {
    let mut engine = VimEngine::new();
    engine.register_host_mappings(
        "test-lsp",
        &[host_mapping(
            &[MappingMode::Normal],
            "gd",
            "<Action>(gotoDefinition)",
            false,
            false,
        )],
    );

    // Both 'g' and then 'd' — look up the two-key sequence via the trie.
    let gd_lhs = vec![
        crate::keymap::KeyEvent::char('g'),
        crate::keymap::KeyEvent::char('d'),
    ];
    let result = engine.keymap().lookup(MappingMode::Normal, &gd_lhs);
    assert!(
        matches!(result, TrieLookup::ExactOnly(_)),
        "gd should be registered in Normal mode"
    );
}

// ─── Owner tagging ────────────────────────────────────────────────────────────

/// The registered entry should carry `MappingOwner::Host("test-lsp")`.
#[test]
fn registered_entry_has_host_owner() {
    let mut engine = VimEngine::new();
    engine.register_host_mappings(
        "test-lsp",
        &[host_mapping(
            &[MappingMode::Normal],
            "gd",
            "gD",
            false,
            false,
        )],
    );

    let gd_lhs = vec![
        crate::keymap::KeyEvent::char('g'),
        crate::keymap::KeyEvent::char('d'),
    ];
    let result = engine.keymap().lookup(MappingMode::Normal, &gd_lhs);
    let TrieLookup::ExactOnly(entry) = result else {
        panic!("expected ExactOnly");
    };
    assert_eq!(
        entry.owner(),
        &MappingOwner::Host(compact_str::CompactString::from("test-lsp")),
        "owner must be Host(\"test-lsp\")"
    );
}

// ─── Unregistration ───────────────────────────────────────────────────────────

/// After unregistering, the mapping should be gone from the keymap.
#[test]
fn unregister_removes_mapping() {
    let mut engine = VimEngine::new();
    engine.register_host_mappings(
        "test-lsp",
        &[host_mapping(
            &[MappingMode::Normal],
            "gd",
            "gD",
            false,
            false,
        )],
    );
    engine.unregister_host_mappings("test-lsp");

    let gd_lhs = vec![
        crate::keymap::KeyEvent::char('g'),
        crate::keymap::KeyEvent::char('d'),
    ];
    let result = engine.keymap().lookup(MappingMode::Normal, &gd_lhs);
    assert_eq!(
        result,
        TrieLookup::NoMatch,
        "gd must be gone after unregister"
    );
}

/// Unregistering a name that was never registered must not panic or crash.
#[test]
fn unregister_nonexistent_is_noop() {
    let mut engine = VimEngine::new();
    engine.unregister_host_mappings("nonexistent-ext");
}

// ─── Multi-mode registration ──────────────────────────────────────────────────

/// A mapping registered in Normal, Visual, and Operator modes must appear in all three.
#[test]
fn register_multiple_modes_all_present() {
    let mut engine = VimEngine::new();
    engine.register_host_mappings(
        "test-lsp",
        &[host_mapping(
            &[
                MappingMode::Normal,
                MappingMode::Visual,
                MappingMode::Operator,
            ],
            "gd",
            "gD",
            false,
            false,
        )],
    );

    let gd_lhs = vec![
        crate::keymap::KeyEvent::char('g'),
        crate::keymap::KeyEvent::char('d'),
    ];
    for mm in [
        MappingMode::Normal,
        MappingMode::Visual,
        MappingMode::Operator,
    ] {
        let result = engine.keymap().lookup(mm, &gd_lhs);
        assert!(
            matches!(result, TrieLookup::ExactOnly(_)),
            "gd must be registered in mode {mm:?}"
        );
    }
}

/// After unregistering, the mapping must be absent in all modes.
#[test]
fn unregister_removes_all_modes() {
    let mut engine = VimEngine::new();
    engine.register_host_mappings(
        "test-lsp",
        &[host_mapping(
            &[MappingMode::Normal, MappingMode::Visual],
            "K",
            "gK",
            false,
            false,
        )],
    );
    engine.unregister_host_mappings("test-lsp");

    let k_lhs = vec![crate::keymap::KeyEvent::char('K')];
    for mm in [MappingMode::Normal, MappingMode::Visual] {
        let result = engine.keymap().lookup(mm, &k_lhs);
        assert_eq!(result, TrieLookup::NoMatch, "K must be gone in mode {mm:?}");
    }
}

// ─── Silent flag ─────────────────────────────────────────────────────────────

/// A mapping registered with `silent: true` must have the silent flag set on the entry.
#[test]
fn silent_mapping_has_silent_flag() {
    let mut engine = VimEngine::new();
    engine.register_host_mappings(
        "test-lsp",
        &[host_mapping(
            &[MappingMode::Normal],
            "gr",
            "gR",
            false,
            true,
        )],
    );

    let lhs = vec![
        crate::keymap::KeyEvent::char('g'),
        crate::keymap::KeyEvent::char('r'),
    ];
    let result = engine.keymap().lookup(MappingMode::Normal, &lhs);
    let TrieLookup::ExactOnly(entry) = result else {
        panic!("expected ExactOnly");
    };
    assert!(entry.silent(), "entry must have silent flag set");
}

/// A mapping registered with `silent: false` must NOT have the silent flag.
#[test]
fn non_silent_mapping_has_no_silent_flag() {
    let mut engine = VimEngine::new();
    engine.register_host_mappings(
        "test-lsp",
        &[host_mapping(
            &[MappingMode::Normal],
            "gr",
            "gR",
            false,
            false,
        )],
    );

    let lhs = vec![
        crate::keymap::KeyEvent::char('g'),
        crate::keymap::KeyEvent::char('r'),
    ];
    let result = engine.keymap().lookup(MappingMode::Normal, &lhs);
    let TrieLookup::ExactOnly(entry) = result else {
        panic!("expected ExactOnly");
    };
    assert!(!entry.silent(), "entry must NOT have silent flag");
}

// ─── Recursive vs NonRecursive ────────────────────────────────────────────────

/// A mapping with `recursive: true` must have `MappingKind::Recursive`.
#[test]
fn recursive_mapping_has_recursive_kind() {
    use crate::keymap::MappingKind;
    let mut engine = VimEngine::new();
    engine.register_host_mappings(
        "test-lsp",
        &[host_mapping(
            &[MappingMode::Normal],
            "gx",
            "gy",
            true,
            false,
        )],
    );

    let lhs = vec![
        crate::keymap::KeyEvent::char('g'),
        crate::keymap::KeyEvent::char('x'),
    ];
    let result = engine.keymap().lookup(MappingMode::Normal, &lhs);
    let TrieLookup::ExactOnly(entry) = result else {
        panic!("expected ExactOnly");
    };
    assert_eq!(entry.kind(), MappingKind::Recursive);
}

/// A mapping with `recursive: false` must have `MappingKind::NonRecursive`.
#[test]
fn non_recursive_mapping_has_nonrecursive_kind() {
    use crate::keymap::MappingKind;
    let mut engine = VimEngine::new();
    engine.register_host_mappings(
        "test-lsp",
        &[host_mapping(
            &[MappingMode::Normal],
            "gx",
            "gy",
            false,
            false,
        )],
    );

    let lhs = vec![
        crate::keymap::KeyEvent::char('g'),
        crate::keymap::KeyEvent::char('x'),
    ];
    let result = engine.keymap().lookup(MappingMode::Normal, &lhs);
    let TrieLookup::ExactOnly(entry) = result else {
        panic!("expected ExactOnly");
    };
    assert_eq!(entry.kind(), MappingKind::NonRecursive);
}

// ─── User mappings survive host unregistration ────────────────────────────────

/// User-defined mappings (owner = User) must survive when a host extension is unregistered.
#[test]
fn user_mappings_survive_host_unregistration() {
    use crate::keymap::MappingKind;

    let mut engine = VimEngine::new();

    // Register a user mapping.
    engine.map(
        MappingMode::Normal,
        &[crate::keymap::KeyEvent::char('j')],
        vec![crate::keymap::KeyEvent::char('k')],
        MappingKind::NonRecursive,
        crate::keymap::MappingFlags::default(),
    );

    // Register host mappings.
    engine.register_host_mappings(
        "test-lsp",
        &[host_mapping(
            &[MappingMode::Normal],
            "gd",
            "gD",
            false,
            false,
        )],
    );

    // Unregister host mappings.
    engine.unregister_host_mappings("test-lsp");

    // User mapping must still be present.
    let j_lhs = vec![crate::keymap::KeyEvent::char('j')];
    let result = engine.keymap().lookup(MappingMode::Normal, &j_lhs);
    assert!(
        matches!(result, TrieLookup::ExactOnly(_)),
        "user mapping 'j' must survive host unregistration"
    );

    // Host mapping must be gone.
    let gd_lhs = vec![
        crate::keymap::KeyEvent::char('g'),
        crate::keymap::KeyEvent::char('d'),
    ];
    let result = engine.keymap().lookup(MappingMode::Normal, &gd_lhs);
    assert_eq!(
        result,
        TrieLookup::NoMatch,
        "host mapping 'gd' must be gone"
    );
}

// ─── Action key parsing ───────────────────────────────────────────────────────

/// A mapping with `<Action>(gotoDefinition)` as RHS must produce an Action key event.
#[test]
fn action_rhs_is_parsed_correctly() {
    use crate::keymap::Key;
    let mut engine = VimEngine::new();
    engine.register_host_mappings(
        "test-lsp",
        &[host_mapping(
            &[MappingMode::Normal],
            "gd",
            "<Action>(gotoDefinition)",
            false,
            false,
        )],
    );

    let gd_lhs = vec![
        crate::keymap::KeyEvent::char('g'),
        crate::keymap::KeyEvent::char('d'),
    ];
    let result = engine.keymap().lookup(MappingMode::Normal, &gd_lhs);
    let TrieLookup::ExactOnly(entry) = result else {
        panic!("expected ExactOnly");
    };

    let seq = entry.sequence();
    assert_eq!(
        seq.len(),
        1,
        "RHS must be exactly one key event (the Action)"
    );
    assert!(
        matches!(seq[0].key(), Key::Action(_)),
        "RHS key must be Key::Action, got {:?}",
        seq[0].key()
    );

    // The action name must be registered in the keymap.
    let action_id = match seq[0].key() {
        Key::Action(id) => id,
        _ => panic!("not an action"),
    };
    let name = engine.keymap().action_name(action_id);
    assert_eq!(name, Some("gotoDefinition"));
}

// ─── Multiple extensions coexist ─────────────────────────────────────────────

/// Two extensions can register non-overlapping mappings; unregistering one does
/// not affect the other.
#[test]
fn two_extensions_coexist_and_independent_unregister() {
    let mut engine = VimEngine::new();

    engine.register_host_mappings(
        "lsp",
        &[host_mapping(
            &[MappingMode::Normal],
            "gd",
            "gD",
            false,
            false,
        )],
    );
    engine.register_host_mappings(
        "formatter",
        &[host_mapping(
            &[MappingMode::Normal],
            "gf",
            "gF",
            false,
            false,
        )],
    );

    engine.unregister_host_mappings("lsp");

    let gd = vec![
        crate::keymap::KeyEvent::char('g'),
        crate::keymap::KeyEvent::char('d'),
    ];
    let gf = vec![
        crate::keymap::KeyEvent::char('g'),
        crate::keymap::KeyEvent::char('f'),
    ];

    assert_eq!(
        engine.keymap().lookup(MappingMode::Normal, &gd),
        TrieLookup::NoMatch,
        "lsp mapping 'gd' must be gone"
    );
    assert!(
        matches!(
            engine.keymap().lookup(MappingMode::Normal, &gf),
            TrieLookup::ExactOnly(_)
        ),
        "formatter mapping 'gf' must survive"
    );
}
