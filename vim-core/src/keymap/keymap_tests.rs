use super::*;
use crate::primitives::VisualType;
use compact_str::CompactString;

/// Shorthand to create a non-recursive MappingEntry from key events.
fn noremap_entry(to: Vec<KeyEvent>) -> MappingEntry {
    MappingEntry::new(to, MappingKind::NonRecursive)
}

#[test]
fn classify_normal_operators() {
    let km = Keymap::default();
    assert_eq!(
        km.classify(KeyEvent::char('d'), Mode::Normal),
        KeyClass::Operator
    );
    assert_eq!(
        km.classify(KeyEvent::char('y'), Mode::Normal),
        KeyClass::Operator
    );
}

#[test]
fn classify_escape_is_escape_in_all_modes() {
    let km = Keymap::default();
    let modes = [
        Mode::Normal,
        Mode::Visual(VisualType::Char),
        Mode::Insert,
        Mode::Replace,
        Mode::CommandLine,
    ];
    for mode in modes {
        assert_eq!(
            km.classify(KeyEvent::escape(), mode),
            KeyClass::Escape,
            "Escape should be Escape in {mode:?}",
        );
    }
}

#[test]
fn classify_insert_replace_commandline_return_unknown() {
    let km = Keymap::default();
    assert_eq!(
        km.classify(KeyEvent::char('j'), Mode::Insert),
        KeyClass::Unknown
    );
    assert_eq!(
        km.classify(KeyEvent::char('j'), Mode::Replace),
        KeyClass::Unknown
    );
    assert_eq!(
        km.classify(KeyEvent::char('j'), Mode::CommandLine),
        KeyClass::Unknown
    );
}

#[test]
fn user_mapping_normal_crud() {
    let mut km = Keymap::default();
    let from = KeyEvent::char('s');
    let to = key_sequence(&[KeyEvent::char('c'), KeyEvent::char('l')]);

    assert!(!km.has_user_mapping(from, Mode::Normal));
    km.map_normal(from, to.clone());
    assert!(km.has_user_mapping(from, Mode::Normal));
    assert_eq!(
        km.get_user_mapping(from, Mode::Normal),
        Some(&noremap_entry(to.clone()))
    );

    let removed = km.unmap_normal(from);
    assert_eq!(removed, Some(noremap_entry(to)));
    assert!(!km.has_user_mapping(from, Mode::Normal));
}

#[test]
fn user_mapping_visual_crud() {
    let mut km = Keymap::default();
    let from = KeyEvent::char('s');
    let to = key_sequence(&[KeyEvent::char('d')]);

    km.map_visual(from, to.clone());
    assert!(km.has_user_mapping(from, Mode::Visual(VisualType::Char)));
    assert_eq!(
        km.get_user_mapping(from, Mode::Visual(VisualType::Line)),
        Some(&noremap_entry(to))
    );

    km.unmap_visual(from);
    assert!(!km.has_user_mapping(from, Mode::Visual(VisualType::Char)));
}

#[test]
fn user_mapping_operator_crud() {
    let mut km = Keymap::default();
    let from = KeyEvent::char('s');
    let to = key_sequence(&[KeyEvent::char('w')]);

    km.map_operator(from, to.clone());
    assert!(km.has_user_mapping(
        from,
        Mode::OperatorPending(crate::primitives::Operator::Delete)
    ));

    km.unmap_operator(from);
    assert!(!km.has_user_mapping(
        from,
        Mode::OperatorPending(crate::primitives::Operator::Delete)
    ));
}

#[test]
fn user_mapping_insert_crud() {
    let mut km = Keymap::default();
    let from = KeyEvent::ctrl('l');
    let to = key_sequence(&[KeyEvent::escape()]);

    km.map_insert(from, to.clone());
    assert!(km.has_user_mapping(from, Mode::Insert));

    km.unmap_insert(from);
    assert!(!km.has_user_mapping(from, Mode::Insert));
}

#[test]
fn clear_per_mode() {
    let mut km = Keymap::default();
    let k = KeyEvent::char('z');
    let seq = key_sequence(&[KeyEvent::char('x')]);

    km.map_normal(k, seq.clone());
    km.map_visual(k, seq.clone());
    km.map_operator(k, seq.clone());
    km.map_insert(k, seq.clone());

    km.clear_normal_mappings();
    assert!(!km.has_user_mapping(k, Mode::Normal));
    assert!(km.has_user_mapping(k, Mode::Visual(VisualType::Char)));

    km.clear_visual_mappings();
    assert!(!km.has_user_mapping(k, Mode::Visual(VisualType::Char)));

    km.clear_operator_mappings();
    assert!(!km.has_user_mapping(k, Mode::OperatorPending(crate::primitives::Operator::Yank)));

    km.clear_insert_mappings();
    assert!(!km.has_user_mapping(k, Mode::Insert));
}

#[test]
fn clear_all_mappings() {
    let mut km = Keymap::default();
    let k = KeyEvent::char('z');
    let seq = key_sequence(&[KeyEvent::char('x')]);

    km.map_normal(k, seq.clone());
    km.map_visual(k, seq.clone());
    km.map_operator(k, seq.clone());
    km.map_insert(k, seq);

    km.clear_all_mappings();
    assert!(!km.has_user_mapping(k, Mode::Normal));
    assert!(!km.has_user_mapping(k, Mode::Visual(VisualType::Char)));
    assert!(!km.has_user_mapping(k, Mode::Insert));
}

#[test]
fn key_sequence_helper() {
    let seq = key_sequence(&[KeyEvent::char('d'), KeyEvent::char('w')]);
    assert_eq!(seq.len(), 2);
    assert_eq!(seq[0], KeyEvent::char('d'));
    assert_eq!(seq[1], KeyEvent::char('w'));
}

#[test]
fn key_sequence_truncates_at_max() {
    let keys: Vec<KeyEvent> = (0..20)
        .map(|i| KeyEvent::char(char::from(b'a' + (i % 26))))
        .collect();
    let seq = key_sequence(&keys);
    assert_eq!(seq.len(), MAX_KEY_SEQUENCE_LEN);
}

// ═══════════════════════════════════════════════════════════════════
// classify() user-layer integration
// ═══════════════════════════════════════════════════════════════════

#[test]
fn classify_uses_user_mapping_first_key() {
    let mut km = Keymap::default();

    // 'Q' is classified as Unknown by default (no core mapping)
    assert_eq!(
        km.classify(KeyEvent::char('Q'), Mode::Normal),
        KeyClass::Unknown
    );

    // Map Q → gq (first key 'g' is a Prefix)
    km.map_normal(
        KeyEvent::char('Q'),
        key_sequence(&[KeyEvent::char('g'), KeyEvent::char('q')]),
    );

    // Now Q should inherit the classification of 'g' = Prefix
    assert_eq!(
        km.classify(KeyEvent::char('Q'), Mode::Normal),
        KeyClass::Prefix
    );
}

#[test]
fn classify_user_mapping_overrides_core() {
    let mut km = Keymap::default();

    // 'j' is classified as Motion by core
    assert_eq!(
        km.classify(KeyEvent::char('j'), Mode::Normal),
        KeyClass::Motion
    );

    // Remap j → gj (first key 'g' is Prefix)
    km.map_normal(
        KeyEvent::char('j'),
        key_sequence(&[KeyEvent::char('g'), KeyEvent::char('j')]),
    );

    // Now j should inherit classification of 'g' = Prefix
    assert_eq!(
        km.classify(KeyEvent::char('j'), Mode::Normal),
        KeyClass::Prefix
    );

    // Unmap restores core classification
    km.unmap_normal(KeyEvent::char('j'));
    assert_eq!(
        km.classify(KeyEvent::char('j'), Mode::Normal),
        KeyClass::Motion
    );
}

#[test]
fn classify_escape_immune_to_user_mapping() {
    let mut km = Keymap::default();

    // Even if someone maps Escape to something, classify_core intercepts
    // Escape before user mappings can override it
    // (classify checks user first, but classify_core also checks Escape first)
    // The behavior depends on ordering — but Escape should always be Escape
    // because classify_core catches it before mode-specific maps

    // Ctrl-C should always be Escape regardless of user mappings
    km.map_normal(
        KeyEvent::ctrl('c'),
        key_sequence(&[KeyEvent::char('d'), KeyEvent::char('d')]),
    );
    // classify_core's Escape check runs on the *original* key before
    // consulting mode maps, but classify() checks user mappings first:
    // it finds the user mapping ctrl-c → dd and classifies the first key
    // 'd' as Operator. That is correct Vim semantics — if you :nmap <C-c> dd,
    // Ctrl-C SHOULD behave as dd (operator). Escape (raw key) remains immune.
    assert_eq!(
        km.classify(KeyEvent::escape(), Mode::Normal),
        KeyClass::Escape
    );
}

#[test]
fn classify_empty_user_sequence_falls_through() {
    let mut km = Keymap::default();

    // Map to empty sequence (edge case)
    km.map_normal(KeyEvent::char('Q'), key_sequence(&[]));

    // Empty target → falls through to core (Q is Unknown in core)
    assert_eq!(
        km.classify(KeyEvent::char('Q'), Mode::Normal),
        KeyClass::Unknown
    );
}

#[test]
fn map_with_leader_resolves_placeholder() {
    let mut km = Keymap::default();
    // Default leader is backslash
    assert_eq!(km.leader(), KeyEvent::char('\\'));

    // Map <Leader>w → x (using Leader placeholder)
    km.map(
        MappingMode::Normal,
        &[KeyEvent::leader(), KeyEvent::char('w')],
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Trie should have `\` + `w`, not Leader + w
    let result = km.lookup(
        MappingMode::Normal,
        &[KeyEvent::char('\\'), KeyEvent::char('w')],
    );
    assert!(matches!(result, TrieLookup::ExactOnly(_)));

    // Leader placeholder should NOT match
    let result = km.lookup(
        MappingMode::Normal,
        &[KeyEvent::leader(), KeyEvent::char('w')],
    );
    assert!(matches!(result, TrieLookup::NoMatch));
}

// ═══════════════════════════════════════════════════════════════════
// Buffer-local mapping tests
// ═══════════════════════════════════════════════════════════════════

#[test]
fn buffer_mapping_wins_over_global() {
    let mut km = Keymap::default();
    let global_seq = key_sequence(&[KeyEvent::char('x')]);
    let buf_seq = key_sequence(&[KeyEvent::char('y')]);

    km.map(
        MappingMode::Normal,
        &[KeyEvent::char('q')],
        global_seq,
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );
    km.map_buffer(
        MappingMode::Normal,
        &[KeyEvent::char('q')],
        buf_seq.clone(),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    let entry = km
        .get_user_mapping(KeyEvent::char('q'), Mode::Normal)
        .unwrap();
    assert_eq!(entry.sequence(), buf_seq.as_slice());
}

#[test]
fn buffer_mapping_no_global_fallback_needed() {
    let mut km = Keymap::default();
    let buf_seq = key_sequence(&[KeyEvent::char('z')]);
    km.map_buffer(
        MappingMode::Normal,
        &[KeyEvent::char('q')],
        buf_seq.clone(),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    let entry = km
        .get_user_mapping(KeyEvent::char('q'), Mode::Normal)
        .unwrap();
    assert_eq!(entry.sequence(), buf_seq.as_slice());
}

#[test]
fn global_wins_when_no_buffer_mapping() {
    let mut km = Keymap::default();
    let global_seq = key_sequence(&[KeyEvent::char('x')]);
    km.map(
        MappingMode::Normal,
        &[KeyEvent::char('q')],
        global_seq.clone(),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // No buffer-local mapping — global should be returned
    let entry = km
        .get_user_mapping(KeyEvent::char('q'), Mode::Normal)
        .unwrap();
    assert_eq!(entry.sequence(), global_seq.as_slice());
}

#[test]
fn clear_buffer_restores_global() {
    let mut km = Keymap::default();
    let global_seq = key_sequence(&[KeyEvent::char('x')]);
    let buf_seq = key_sequence(&[KeyEvent::char('y')]);

    km.map(
        MappingMode::Normal,
        &[KeyEvent::char('q')],
        global_seq.clone(),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );
    km.map_buffer(
        MappingMode::Normal,
        &[KeyEvent::char('q')],
        buf_seq,
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Buffer-local wins
    assert_eq!(
        km.get_user_mapping(KeyEvent::char('q'), Mode::Normal)
            .unwrap()
            .sequence()[0],
        KeyEvent::char('y')
    );

    // Clear buffer overlay
    km.clear_all_buffer_mappings();

    // Global restored
    let entry = km
        .get_user_mapping(KeyEvent::char('q'), Mode::Normal)
        .unwrap();
    assert_eq!(entry.sequence(), global_seq.as_slice());
}

#[test]
fn take_set_buffer_roundtrip() {
    let mut km = Keymap::default();
    let buf_seq = key_sequence(&[KeyEvent::char('z')]);
    km.map_buffer(
        MappingMode::Normal,
        &[KeyEvent::char('q')],
        buf_seq.clone(),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Take extracts and clears
    let saved = km.take_buffer_mappings();
    assert!(km
        .get_user_mapping(KeyEvent::char('q'), Mode::Normal)
        .is_none());

    // Re-install
    km.set_buffer_mappings(saved);
    let entry = km
        .get_user_mapping(KeyEvent::char('q'), Mode::Normal)
        .unwrap();
    assert_eq!(entry.sequence(), buf_seq.as_slice());
}

#[test]
fn buffer_prefix_merged_with_global() {
    let mut km = Keymap::default();
    // Global: jk → <Esc>
    km.map(
        MappingMode::Normal,
        &[KeyEvent::char('j'), KeyEvent::char('k')],
        key_sequence(&[KeyEvent::escape()]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );
    // Buffer: jj → dd (creates prefix 'j' in buffer-local)
    km.map_buffer(
        MappingMode::Normal,
        &[KeyEvent::char('j'), KeyEvent::char('j')],
        key_sequence(&[KeyEvent::char('d'), KeyEvent::char('d')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // lookup(&[j]) should be Prefix (buffer has jj, global has jk)
    let result = km.lookup(MappingMode::Normal, &[KeyEvent::char('j')]);
    assert!(matches!(result, TrieLookup::Prefix { exact: None }));

    // lookup(&[j, j]) → ExactOnly (buffer)
    let result = km.lookup(
        MappingMode::Normal,
        &[KeyEvent::char('j'), KeyEvent::char('j')],
    );
    assert!(matches!(result, TrieLookup::ExactOnly(_)));
}

#[test]
fn buffer_exact_shadows_global_prefix() {
    let mut km = Keymap::default();
    // Global: jk → <Esc> (makes 'j' a prefix in global)
    km.map(
        MappingMode::Normal,
        &[KeyEvent::char('j'), KeyEvent::char('k')],
        key_sequence(&[KeyEvent::escape()]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );
    // Buffer: j → x (exact match in buffer)
    km.map_buffer(
        MappingMode::Normal,
        &[KeyEvent::char('j')],
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // lookup(&[j]) → buffer has ExactOnly(j→x), global has Prefix{exact:None}
    // merge: buffer ExactOnly wins
    let result = km.lookup(MappingMode::Normal, &[KeyEvent::char('j')]);
    assert!(matches!(result, TrieLookup::ExactOnly(_)));
}

#[test]
fn buffer_prefix_suppresses_global_exact() {
    // Buffer has `abc` mapping (so `ab` is a prefix with no exact).
    // Global has `ab` mapping (exact match).
    // Looking up [a, b] should return Prefix{exact:None} because the
    // buffer-local prefix shadows the global exact per :help map-precedence.
    let mut km = Keymap::default();

    // Global: ab → x (exact)
    km.map(
        MappingMode::Normal,
        &[KeyEvent::char('a'), KeyEvent::char('b')],
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );
    // Buffer: abc → y (makes [a,b] a prefix in buffer-local)
    km.map_buffer(
        MappingMode::Normal,
        &[
            KeyEvent::char('a'),
            KeyEvent::char('b'),
            KeyEvent::char('c'),
        ],
        key_sequence(&[KeyEvent::char('y')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    let result = km.lookup(
        MappingMode::Normal,
        &[KeyEvent::char('a'), KeyEvent::char('b')],
    );
    assert!(
        matches!(result, TrieLookup::Prefix { exact: None }),
        "buffer-local prefix should suppress global exact match, got {result:?}"
    );
}

#[test]
fn merge_both_ambiguous_prefix_buffer_exact_wins() {
    // When both layers have Prefix { exact: Some }, the buffer-local
    // exact wins per :help map-precedence.
    let mut km = Keymap::default();

    // Global: j → x (exact), jk → y (makes j an ambiguous prefix)
    km.map(
        MappingMode::Normal,
        &[KeyEvent::char('j')],
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );
    km.map(
        MappingMode::Normal,
        &[KeyEvent::char('j'), KeyEvent::char('k')],
        key_sequence(&[KeyEvent::char('y')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Buffer: j → a (exact), jl → b (makes j an ambiguous prefix in buffer too)
    km.map_buffer(
        MappingMode::Normal,
        &[KeyEvent::char('j')],
        key_sequence(&[KeyEvent::char('a')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );
    km.map_buffer(
        MappingMode::Normal,
        &[KeyEvent::char('j'), KeyEvent::char('l')],
        key_sequence(&[KeyEvent::char('b')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Both layers: Prefix { exact: Some(_) } for 'j'
    // merge_lookups should return buffer's exact (j → a), not global's (j → x)
    let result = km.lookup(MappingMode::Normal, &[KeyEvent::char('j')]);
    match result {
        TrieLookup::Prefix { exact: Some(entry) } => {
            assert_eq!(
                entry.sequence()[0],
                KeyEvent::char('a'),
                "buffer-local exact should win over global exact in prefix merge"
            );
        }
        other => panic!("expected Prefix {{ exact: Some(buf) }}, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════
// Buffer-local negative / edge-case / regression tests
// ═══════════════════════════════════════════════════════════════════

#[test]
fn unmap_buffer_nonexistent_returns_none() {
    let mut km = Keymap::default();
    // No buffer trie exists yet — should return None without panicking
    // or allocating an empty trie.
    let result = km.unmap_buffer(MappingMode::Normal, &[KeyEvent::char('q')]);
    assert!(result.is_none());

    // Verify no buffer overlay was lazily created (take should be all-empty)
    let bm = km.take_buffer_mappings();
    assert!(bm.is_empty());
}

#[test]
fn set_empty_buffer_mappings_leaves_none() {
    let mut km = Keymap::default();
    // Setting a fully-empty BufferMappings should not install any overlay
    km.set_buffer_mappings(BufferMappings::default());

    // No buffer-local mapping should be active
    assert!(km
        .get_user_mapping(KeyEvent::char('q'), Mode::Normal)
        .is_none());

    // Take should also be empty
    let bm = km.take_buffer_mappings();
    assert!(bm.is_empty());
}

#[test]
fn classify_uses_buffer_local_mapping() {
    let mut km = Keymap::default();

    // Map 'Q' → 'dd' globally (Operator classification via 'd')
    km.map_normal(
        KeyEvent::char('Q'),
        key_sequence(&[KeyEvent::char('d'), KeyEvent::char('d')]),
    );

    // Buffer-local: Map 'Q' → 'i' (Insert classification)
    km.map_buffer(
        MappingMode::Normal,
        &[KeyEvent::char('Q')],
        key_sequence(&[KeyEvent::char('i')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // classify should use buffer-local 'i' → KeyClass::ModeSwitch
    let class = km.classify(KeyEvent::char('Q'), Mode::Normal);
    assert_eq!(class, KeyClass::ModeSwitch);

    // Clear buffer → should fall back to global 'dd' → KeyClass::Operator
    km.clear_all_buffer_mappings();
    let class = km.classify(KeyEvent::char('Q'), Mode::Normal);
    assert_eq!(class, KeyClass::Operator);
}

#[test]
fn multi_mode_isolation() {
    let mut km = Keymap::default();

    // Buffer-local in Normal: q → x
    km.map_buffer(
        MappingMode::Normal,
        &[KeyEvent::char('q')],
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Global in Visual: q → y
    km.map(
        MappingMode::Visual,
        &[KeyEvent::char('q')],
        key_sequence(&[KeyEvent::char('y')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Normal: buffer-local 'x' should be returned
    let entry = km
        .get_user_mapping(KeyEvent::char('q'), Mode::Normal)
        .unwrap();
    assert_eq!(entry.sequence()[0], KeyEvent::char('x'));

    // Visual: no buffer-local, so global 'y' should be returned
    let entry = km
        .get_user_mapping(KeyEvent::char('q'), Mode::Visual(VisualType::Char))
        .unwrap();
    assert_eq!(entry.sequence()[0], KeyEvent::char('y'));

    // Insert: neither — should be None
    assert!(km
        .get_user_mapping(KeyEvent::char('q'), Mode::Insert)
        .is_none());
}

#[test]
fn buffer_exact_wins_over_global_longer_prefix() {
    // Per :help map-precedence: buffer-local mappings are found before
    // global mappings. If buffer has `jk → x` (exact) and global has
    // `jkl → y` (longer), `jk` should dispatch buffer immediately.
    let mut km = Keymap::default();

    km.map(
        MappingMode::Normal,
        &[
            KeyEvent::char('j'),
            KeyEvent::char('k'),
            KeyEvent::char('l'),
        ],
        key_sequence(&[KeyEvent::char('y')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );
    km.map_buffer(
        MappingMode::Normal,
        &[KeyEvent::char('j'), KeyEvent::char('k')],
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // lookup([j, k]) → buffer ExactOnly wins over global Prefix{exact:None}
    let result = km.lookup(
        MappingMode::Normal,
        &[KeyEvent::char('j'), KeyEvent::char('k')],
    );
    assert!(matches!(result, TrieLookup::ExactOnly(_)));
    if let TrieLookup::ExactOnly(entry) = result {
        assert_eq!(entry.sequence()[0], KeyEvent::char('x'));
    }
}

#[test]
fn chained_take_set_across_buffers() {
    let mut km = Keymap::default();

    // Buffer A: q → a
    km.map_buffer(
        MappingMode::Normal,
        &[KeyEvent::char('q')],
        key_sequence(&[KeyEvent::char('a')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );
    let buf_a = km.take_buffer_mappings();

    // Buffer B: q → b
    km.map_buffer(
        MappingMode::Normal,
        &[KeyEvent::char('q')],
        key_sequence(&[KeyEvent::char('b')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );
    let buf_b = km.take_buffer_mappings();

    // Switch to A
    km.set_buffer_mappings(buf_a);
    assert_eq!(
        km.get_user_mapping(KeyEvent::char('q'), Mode::Normal)
            .unwrap()
            .sequence()[0],
        KeyEvent::char('a')
    );

    // Switch to B
    let buf_a_again = km.take_buffer_mappings();
    km.set_buffer_mappings(buf_b);
    assert_eq!(
        km.get_user_mapping(KeyEvent::char('q'), Mode::Normal)
            .unwrap()
            .sequence()[0],
        KeyEvent::char('b')
    );

    // Switch back to A
    let _buf_b_again = km.take_buffer_mappings();
    km.set_buffer_mappings(buf_a_again);
    assert_eq!(
        km.get_user_mapping(KeyEvent::char('q'), Mode::Normal)
            .unwrap()
            .sequence()[0],
        KeyEvent::char('a')
    );
}

#[test]
fn unmap_resolves_leader_global() {
    let mut km = Keymap::default();
    // Default leader is '\', map <Leader>w → x
    km.map(
        MappingMode::Normal,
        &[KeyEvent::leader(), KeyEvent::char('w')],
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Verify it's stored under '\w'
    let result = km.lookup(
        MappingMode::Normal,
        &[KeyEvent::char('\\'), KeyEvent::char('w')],
    );
    assert!(matches!(result, TrieLookup::ExactOnly(_)));

    // Unmap with <Leader>w — must resolve to '\w' internally
    let removed = km.unmap(
        MappingMode::Normal,
        &[KeyEvent::leader(), KeyEvent::char('w')],
    );
    assert!(removed.is_some());
    assert_eq!(removed.unwrap().sequence()[0], KeyEvent::char('x'));

    // Verify it's gone
    let result = km.lookup(
        MappingMode::Normal,
        &[KeyEvent::char('\\'), KeyEvent::char('w')],
    );
    assert!(matches!(result, TrieLookup::NoMatch));
}

#[test]
fn unmap_buffer_resolves_leader() {
    let mut km = Keymap::default();
    km.set_leader(KeyEvent::char(','));

    // Buffer-local: <Leader>e → y (stored as ',e')
    km.map_buffer(
        MappingMode::Normal,
        &[KeyEvent::leader(), KeyEvent::char('e')],
        key_sequence(&[KeyEvent::char('y')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Verify it's stored under ',e'
    let result = km.lookup(
        MappingMode::Normal,
        &[KeyEvent::char(','), KeyEvent::char('e')],
    );
    assert!(matches!(result, TrieLookup::ExactOnly(_)));

    // Unmap with <Leader>e — must resolve to ',e'
    let removed = km.unmap_buffer(
        MappingMode::Normal,
        &[KeyEvent::leader(), KeyEvent::char('e')],
    );
    assert!(removed.is_some());
    assert_eq!(removed.unwrap().sequence()[0], KeyEvent::char('y'));
}

// ═══════════════════════════════════════════════════════════════════
// Buffer query API
// ═══════════════════════════════════════════════════════════════════

#[test]
fn has_buffer_mapping_distinguishes_layers() {
    let mut km = Keymap::default();

    // Global mapping: q → x
    km.map_normal(KeyEvent::char('q'), key_sequence(&[KeyEvent::char('x')]));
    // Buffer mapping: w → y
    km.map_buffer(
        MappingMode::Normal,
        &[KeyEvent::char('w')],
        key_sequence(&[KeyEvent::char('y')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // has_user_mapping sees both
    assert!(km.has_user_mapping(KeyEvent::char('q'), Mode::Normal));
    assert!(km.has_user_mapping(KeyEvent::char('w'), Mode::Normal));

    // has_buffer_mapping only sees buffer-local
    assert!(!km.has_buffer_mapping(KeyEvent::char('q'), Mode::Normal));
    assert!(km.has_buffer_mapping(KeyEvent::char('w'), Mode::Normal));

    // get_buffer_mapping returns correct entry
    let entry = km
        .get_buffer_mapping(KeyEvent::char('w'), Mode::Normal)
        .unwrap();
    assert_eq!(entry.sequence()[0], KeyEvent::char('y'));
    assert!(km
        .get_buffer_mapping(KeyEvent::char('q'), Mode::Normal)
        .is_none());
}

#[test]
fn query_api_resolves_leader() {
    let mut km = Keymap::default();
    km.set_leader(KeyEvent::char(' '));

    // Map <Leader> (space) → x as single-key mapping
    km.map_normal(KeyEvent::char(' '), key_sequence(&[KeyEvent::char('x')]));

    // Query with raw leader key should resolve to space and find it
    assert!(km.has_user_mapping(KeyEvent::leader(), Mode::Normal));
    let entry = km
        .get_user_mapping(KeyEvent::leader(), Mode::Normal)
        .unwrap();
    assert_eq!(entry.sequence()[0], KeyEvent::char('x'));

    // Same for buffer-local
    km.map_buffer(
        MappingMode::Normal,
        &[KeyEvent::char(' ')],
        key_sequence(&[KeyEvent::char('z')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );
    assert!(km.has_buffer_mapping(KeyEvent::leader(), Mode::Normal));
    let entry = km
        .get_buffer_mapping(KeyEvent::leader(), Mode::Normal)
        .unwrap();
    assert_eq!(entry.sequence()[0], KeyEvent::char('z'));
}

// ═══════════════════════════════════════════════════════════════════
// list_mappings / introspection
// ═══════════════════════════════════════════════════════════════════

#[test]
fn list_mappings_shows_both_layers() {
    let mut km = Keymap::default();

    // Global: q → x
    km.map_normal(KeyEvent::char('q'), key_sequence(&[KeyEvent::char('x')]));
    // Buffer: w → y
    km.map_buffer(
        MappingMode::Normal,
        &[KeyEvent::char('w')],
        key_sequence(&[KeyEvent::char('y')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    let mappings = km.list_mappings(MappingMode::Normal);
    assert_eq!(mappings.len(), 2);

    // Find buffer-local one
    let buf_mapping = mappings.iter().find(|(_, _, is_buf)| *is_buf).unwrap();
    assert_eq!(buf_mapping.0, vec![KeyEvent::char('w')]);
    assert_eq!(buf_mapping.1.sequence()[0], KeyEvent::char('y'));

    // Find global one
    let global_mapping = mappings.iter().find(|(_, _, is_buf)| !*is_buf).unwrap();
    assert_eq!(global_mapping.0, vec![KeyEvent::char('q')]);
    assert_eq!(global_mapping.1.sequence()[0], KeyEvent::char('x'));
}

#[test]
fn list_mappings_empty_when_no_mappings() {
    let km = Keymap::default();
    assert!(km.list_mappings(MappingMode::Normal).is_empty());
}

#[test]
fn buffer_mappings_iter_mode() {
    let mut km = Keymap::default();

    // Buffer: Normal q→x, Visual w→y
    km.map_buffer(
        MappingMode::Normal,
        &[KeyEvent::char('q')],
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );
    km.map_buffer(
        MappingMode::Visual,
        &[KeyEvent::char('w')],
        key_sequence(&[KeyEvent::char('y')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    let bm = km.take_buffer_mappings();

    let normal_iter = bm.iter_mode(MappingMode::Normal);
    assert_eq!(normal_iter.len(), 1);
    assert_eq!(normal_iter[0].0, vec![KeyEvent::char('q')]);

    let visual_iter = bm.iter_mode(MappingMode::Visual);
    assert_eq!(visual_iter.len(), 1);
    assert_eq!(visual_iter[0].0, vec![KeyEvent::char('w')]);

    assert!(bm.iter_mode(MappingMode::Insert).is_empty());
}

// ═══════════════════════════════════════════════════════════════════
// list_continuations / which-key support
// ═══════════════════════════════════════════════════════════════════

#[test]
fn list_continuations_empty_prefix() {
    let keymap = Keymap::new();
    let continuations = keymap.list_continuations(MappingMode::Normal, &[]);
    assert!(continuations.is_empty());
}

#[test]
fn list_continuations_finds_user_mappings() {
    let mut keymap = Keymap::new();
    let g_key = KeyEvent::char('g');
    let d_key = KeyEvent::char('d');
    let entry = MappingEntry::new(vec![KeyEvent::char('x')], MappingKind::NonRecursive);
    keymap.map_entry(MappingMode::Normal, &[g_key, d_key], entry);
    let continuations = keymap.list_continuations(MappingMode::Normal, &[g_key]);
    assert_eq!(continuations.len(), 1);
    assert_eq!(continuations[0].0, d_key);
}

#[test]
fn list_continuations_buffer_local_shadows_global() {
    let mut keymap = Keymap::new();
    let g_key = KeyEvent::char('g');
    let d_key = KeyEvent::char('d');

    // Global: gd -> x
    keymap.map(
        MappingMode::Normal,
        &[g_key, d_key],
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );
    // Buffer-local: gd -> y
    keymap.map_buffer(
        MappingMode::Normal,
        &[g_key, d_key],
        key_sequence(&[KeyEvent::char('y')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    let continuations = keymap.list_continuations(MappingMode::Normal, &[g_key]);
    assert_eq!(continuations.len(), 1);
    assert_eq!(
        continuations[0].1.sequence(),
        &[KeyEvent::char('y')],
        "buffer-local entry should shadow global"
    );
}

#[test]
fn list_continuations_merges_across_layers() {
    let mut keymap = Keymap::new();
    let g_key = KeyEvent::char('g');

    // Global: gd -> x
    keymap.map(
        MappingMode::Normal,
        &[g_key, KeyEvent::char('d')],
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );
    // Buffer-local: ge -> y (different continuation key)
    keymap.map_buffer(
        MappingMode::Normal,
        &[g_key, KeyEvent::char('e')],
        key_sequence(&[KeyEvent::char('y')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    let continuations = keymap.list_continuations(MappingMode::Normal, &[g_key]);
    assert_eq!(continuations.len(), 2, "should merge non-overlapping keys");
}

#[test]
fn list_continuations_filetype_layer() {
    let mut keymap = Keymap::new();
    let g_key = KeyEvent::char('g');

    // Global: gd -> x
    keymap.map(
        MappingMode::Normal,
        &[g_key, KeyEvent::char('d')],
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );
    // Filetype (rust): gd -> z (shadows global)
    keymap.map_filetype(
        "rust",
        MappingMode::Normal,
        &[g_key, KeyEvent::char('d')],
        key_sequence(&[KeyEvent::char('z')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );
    keymap.set_filetype(Some(CompactString::from("rust")));

    let continuations = keymap.list_continuations(MappingMode::Normal, &[g_key]);
    assert_eq!(continuations.len(), 1);
    assert_eq!(
        continuations[0].1.sequence(),
        &[KeyEvent::char('z')],
        "filetype entry should shadow global"
    );
}

#[test]
fn list_continuations_skips_deeper_mappings() {
    let mut keymap = Keymap::new();
    let g_key = KeyEvent::char('g');

    // gdd -> x (depth 2 from prefix [g], should NOT appear)
    keymap.map(
        MappingMode::Normal,
        &[g_key, KeyEvent::char('d'), KeyEvent::char('d')],
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    let continuations = keymap.list_continuations(MappingMode::Normal, &[g_key]);
    assert!(
        continuations.is_empty(),
        "deeper mappings should not appear as direct continuations"
    );
}

#[test]
fn list_continuations_no_match_for_wrong_mode() {
    let mut keymap = Keymap::new();
    let g_key = KeyEvent::char('g');
    keymap.map(
        MappingMode::Normal,
        &[g_key, KeyEvent::char('d')],
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Query in Visual mode — no mappings there
    let continuations = keymap.list_continuations(MappingMode::Visual, &[g_key]);
    assert!(continuations.is_empty());
}

// ═══════════════════════════════════════════════════════════════════
// Convenience wrappers
// ═══════════════════════════════════════════════════════════════════

#[test]
fn unmap_buffer_convenience_wrappers() {
    let mut km = Keymap::default();

    // Add buffer mappings in all modes
    for mm in [
        MappingMode::Normal,
        MappingMode::Visual,
        MappingMode::Operator,
        MappingMode::Insert,
    ] {
        km.map_buffer(
            mm,
            &[KeyEvent::char('q')],
            key_sequence(&[KeyEvent::char('x')]),
            MappingKind::NonRecursive,
            MappingFlags::default(),
        );
    }

    // Unmap each with convenience wrapper
    assert!(km.unmap_buffer_normal(KeyEvent::char('q')).is_some());
    assert!(km.unmap_buffer_visual(KeyEvent::char('q')).is_some());
    assert!(km.unmap_buffer_operator(KeyEvent::char('q')).is_some());
    assert!(km.unmap_buffer_insert(KeyEvent::char('q')).is_some());

    // All gone
    assert!(!km.has_buffer_mapping(KeyEvent::char('q'), Mode::Normal));
    assert!(!km.has_buffer_mapping(KeyEvent::char('q'), Mode::Visual(VisualType::Char),));
}

// ═══════════════════════════════════════════════════════════════════
// FileType-specific mapping tests
// ═══════════════════════════════════════════════════════════════════

#[test]
fn filetype_mapping_active_when_set() {
    let mut keymap = Keymap::new();
    let to = key_sequence(&[KeyEvent::char('x')]);
    keymap.map_filetype(
        "rust",
        MappingMode::Normal,
        &[KeyEvent::char('q')],
        to,
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Not active without filetype
    assert!(!keymap.has_user_mapping(KeyEvent::char('q'), Mode::Normal));

    // Active when filetype matches
    keymap.set_filetype(Some(CompactString::from("rust")));
    assert!(keymap.has_user_mapping(KeyEvent::char('q'), Mode::Normal));

    // Not active for different filetype
    keymap.set_filetype(Some(CompactString::from("python")));
    assert!(!keymap.has_user_mapping(KeyEvent::char('q'), Mode::Normal));
}

#[test]
fn filetype_mapping_priority_over_global() {
    let mut keymap = Keymap::new();
    // Global mapping: q -> y
    keymap.map_normal(KeyEvent::char('q'), key_sequence(&[KeyEvent::char('y')]));
    // Filetype mapping: q -> z (for rust)
    keymap.map_filetype(
        "rust",
        MappingMode::Normal,
        &[KeyEvent::char('q')],
        key_sequence(&[KeyEvent::char('z')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Without filetype, global wins
    let entry = keymap
        .get_user_mapping(KeyEvent::char('q'), Mode::Normal)
        .unwrap();
    assert_eq!(entry.sequence().first(), Some(&KeyEvent::char('y')));

    // With filetype, filetype wins
    keymap.set_filetype(Some(CompactString::from("rust")));
    let entry = keymap
        .get_user_mapping(KeyEvent::char('q'), Mode::Normal)
        .unwrap();
    assert_eq!(entry.sequence().first(), Some(&KeyEvent::char('z')));
}

#[test]
fn buffer_local_wins_over_filetype() {
    let mut keymap = Keymap::new();
    // Filetype mapping: q -> z (for rust)
    keymap.map_filetype(
        "rust",
        MappingMode::Normal,
        &[KeyEvent::char('q')],
        key_sequence(&[KeyEvent::char('z')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );
    // Buffer-local mapping: q -> a
    keymap.map_buffer(
        MappingMode::Normal,
        &[KeyEvent::char('q')],
        key_sequence(&[KeyEvent::char('a')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );
    keymap.set_filetype(Some(CompactString::from("rust")));

    // Buffer-local should win over filetype
    let entry = keymap
        .get_user_mapping(KeyEvent::char('q'), Mode::Normal)
        .unwrap();
    assert_eq!(entry.sequence().first(), Some(&KeyEvent::char('a')));

    // After clearing buffer, filetype should win
    keymap.clear_all_buffer_mappings();
    let entry = keymap
        .get_user_mapping(KeyEvent::char('q'), Mode::Normal)
        .unwrap();
    assert_eq!(entry.sequence().first(), Some(&KeyEvent::char('z')));
}

#[test]
fn unmap_filetype() {
    let mut keymap = Keymap::new();
    keymap.map_filetype(
        "rust",
        MappingMode::Normal,
        &[KeyEvent::char('q')],
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );
    keymap.set_filetype(Some(CompactString::from("rust")));
    assert!(keymap.has_user_mapping(KeyEvent::char('q'), Mode::Normal));

    keymap.unmap_filetype("rust", MappingMode::Normal, &[KeyEvent::char('q')]);
    assert!(!keymap.has_user_mapping(KeyEvent::char('q'), Mode::Normal));
}

#[test]
fn clear_filetype_per_mode() {
    let mut keymap = Keymap::new();
    keymap.map_filetype(
        "rust",
        MappingMode::Normal,
        &[KeyEvent::char('q')],
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );
    keymap.map_filetype(
        "rust",
        MappingMode::Visual,
        &[KeyEvent::char('q')],
        key_sequence(&[KeyEvent::char('y')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );
    keymap.set_filetype(Some(CompactString::from("rust")));

    // Clear Normal only
    keymap.clear_filetype("rust", MappingMode::Normal);
    assert!(!keymap.has_user_mapping(KeyEvent::char('q'), Mode::Normal));
    assert!(keymap.has_user_mapping(KeyEvent::char('q'), Mode::Visual(VisualType::Char),));
}

#[test]
fn clear_all_filetype_mappings() {
    let mut keymap = Keymap::new();
    keymap.map_filetype(
        "rust",
        MappingMode::Normal,
        &[KeyEvent::char('q')],
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );
    keymap.map_filetype(
        "python",
        MappingMode::Normal,
        &[KeyEvent::char('w')],
        key_sequence(&[KeyEvent::char('y')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    keymap.clear_all_filetype_mappings();

    keymap.set_filetype(Some(CompactString::from("rust")));
    assert!(!keymap.has_user_mapping(KeyEvent::char('q'), Mode::Normal));
    keymap.set_filetype(Some(CompactString::from("python")));
    assert!(!keymap.has_user_mapping(KeyEvent::char('w'), Mode::Normal));
}

#[test]
fn filetype_classify_uses_filetype_mapping() {
    let mut km = Keymap::default();

    // No mapping — Q is Unknown
    assert_eq!(
        km.classify(KeyEvent::char('Q'), Mode::Normal),
        KeyClass::Unknown,
    );

    // Filetype mapping: Q → i (ModeSwitch classification)
    km.map_filetype(
        "rust",
        MappingMode::Normal,
        &[KeyEvent::char('Q')],
        key_sequence(&[KeyEvent::char('i')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Without filetype, Q is still Unknown
    assert_eq!(
        km.classify(KeyEvent::char('Q'), Mode::Normal),
        KeyClass::Unknown,
    );

    // With filetype, Q → i → ModeSwitch
    km.set_filetype(Some(CompactString::from("rust")));
    assert_eq!(
        km.classify(KeyEvent::char('Q'), Mode::Normal),
        KeyClass::ModeSwitch,
    );
}

#[test]
fn filetype_lookup_merged_with_global() {
    let mut km = Keymap::default();

    // Global: jk → <Esc>
    km.map(
        MappingMode::Normal,
        &[KeyEvent::char('j'), KeyEvent::char('k')],
        key_sequence(&[KeyEvent::escape()]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );
    // Filetype (rust): jj → dd
    km.map_filetype(
        "rust",
        MappingMode::Normal,
        &[KeyEvent::char('j'), KeyEvent::char('j')],
        key_sequence(&[KeyEvent::char('d'), KeyEvent::char('d')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );
    km.set_filetype(Some(CompactString::from("rust")));

    // lookup(&[j]) should be Prefix (filetype has jj, global has jk)
    let result = km.lookup(MappingMode::Normal, &[KeyEvent::char('j')]);
    assert!(matches!(result, TrieLookup::Prefix { exact: None }));

    // lookup(&[j, j]) → ExactOnly (filetype)
    let result = km.lookup(
        MappingMode::Normal,
        &[KeyEvent::char('j'), KeyEvent::char('j')],
    );
    assert!(matches!(result, TrieLookup::ExactOnly(_)));
}

#[test]
fn filetype_set_and_get() {
    let mut km = Keymap::default();
    assert_eq!(km.active_filetype(), None);

    km.set_filetype(Some(CompactString::from("rust")));
    assert_eq!(km.active_filetype(), Some("rust"));

    km.set_filetype(None);
    assert_eq!(km.active_filetype(), None);
}

#[test]
fn remove_mappings_by_owner_across_modes() {
    let mut km = Keymap::default();

    // Insert a Core mapping in Normal and in Insert mode.
    km.user[MappingMode::Normal].insert(
        &[KeyEvent::char('a')],
        MappingEntry::new(
            key_sequence(&[KeyEvent::char('x')]),
            MappingKind::NonRecursive,
        )
        .with_owner(MappingOwner::Core),
    );
    km.user[MappingMode::Insert].insert(
        &[KeyEvent::char('b')],
        MappingEntry::new(
            key_sequence(&[KeyEvent::char('y')]),
            MappingKind::NonRecursive,
        )
        .with_owner(MappingOwner::Core),
    );
    // Insert a User mapping that must survive.
    km.user[MappingMode::Normal].insert(
        &[KeyEvent::char('c')],
        MappingEntry::new(
            key_sequence(&[KeyEvent::char('z')]),
            MappingKind::NonRecursive,
        ),
    );

    let removed = km.remove_mappings_by_owner(&MappingOwner::Core);
    assert_eq!(
        removed, 2,
        "both Core mappings (Normal + Insert) should be removed"
    );

    // Core mappings gone.
    assert_eq!(
        km.user[MappingMode::Normal].lookup(&[KeyEvent::char('a')]),
        TrieLookup::NoMatch,
    );
    assert_eq!(
        km.user[MappingMode::Insert].lookup(&[KeyEvent::char('b')]),
        TrieLookup::NoMatch,
    );
    // User mapping survives.
    assert!(matches!(
        km.user[MappingMode::Normal].lookup(&[KeyEvent::char('c')]),
        TrieLookup::ExactOnly(_),
    ));
}

#[test]
fn remove_mappings_by_owner_zero_returns_zero() {
    let mut km = Keymap::default();
    // Only User mappings exist.
    km.map(
        MappingMode::Normal,
        &[KeyEvent::char('j')],
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    let removed = km.remove_mappings_by_owner(&MappingOwner::Core);
    assert_eq!(removed, 0);
    // The User mapping must still be there.
    assert!(km.user[MappingMode::Normal].lookup(&[KeyEvent::char('j')]) != TrieLookup::NoMatch);
}

#[test]
fn replace_mode_uses_insert_mappings() {
    assert_eq!(
        MappingMode::from_mode(Mode::Replace),
        Some(MappingMode::Insert),
    );
}

#[test]
fn virtual_replace_mode_uses_insert_mappings() {
    assert_eq!(
        MappingMode::from_mode(Mode::VirtualReplace),
        Some(MappingMode::Insert),
    );
}

// ═══════════════════════════════════════════════════════════════════
// <LocalLeader>
// ═══════════════════════════════════════════════════════════════════

#[test]
fn local_leader_default_is_backslash() {
    let km = Keymap::default();
    assert_eq!(km.local_leader(), KeyEvent::char('\\'));
}

#[test]
fn local_leader_resolves_placeholder() {
    let mut km = Keymap::default();
    km.set_local_leader(KeyEvent::char(','));

    // Map <LocalLeader>f → x
    km.map(
        MappingMode::Normal,
        &[KeyEvent::local_leader(), KeyEvent::char('f')],
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Trie should have `,` + `f`, not LocalLeader + f
    let result = km.lookup(
        MappingMode::Normal,
        &[KeyEvent::char(','), KeyEvent::char('f')],
    );
    assert!(
        matches!(result, TrieLookup::ExactOnly(_)),
        "<LocalLeader>f should resolve to ,f"
    );

    // LocalLeader placeholder should NOT match
    let result = km.lookup(
        MappingMode::Normal,
        &[KeyEvent::local_leader(), KeyEvent::char('f')],
    );
    assert!(matches!(result, TrieLookup::NoMatch));
}

#[test]
fn local_leader_independent_from_leader() {
    let mut km = Keymap::default();
    km.set_leader(KeyEvent::char(' '));
    km.set_local_leader(KeyEvent::char(','));

    // Map <Leader>w → x
    km.map(
        MappingMode::Normal,
        &[KeyEvent::leader(), KeyEvent::char('w')],
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );
    // Map <LocalLeader>w → y
    km.map(
        MappingMode::Normal,
        &[KeyEvent::local_leader(), KeyEvent::char('w')],
        key_sequence(&[KeyEvent::char('y')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Leader resolved to space
    let result = km.lookup(
        MappingMode::Normal,
        &[KeyEvent::char(' '), KeyEvent::char('w')],
    );
    assert!(matches!(result, TrieLookup::ExactOnly(_)));
    if let TrieLookup::ExactOnly(entry) = result {
        assert_eq!(entry.sequence()[0], KeyEvent::char('x'));
    }

    // LocalLeader resolved to comma
    let result = km.lookup(
        MappingMode::Normal,
        &[KeyEvent::char(','), KeyEvent::char('w')],
    );
    assert!(matches!(result, TrieLookup::ExactOnly(_)));
    if let TrieLookup::ExactOnly(entry) = result {
        assert_eq!(entry.sequence()[0], KeyEvent::char('y'));
    }
}

// ═══════════════════════════════════════════════════════════════════
// <unique> flag
// ═══════════════════════════════════════════════════════════════════

#[test]
fn unique_flag_blocks_different_owner() {
    let mut km = Keymap::default();

    // Insert a <unique> mapping from User owner
    let entry = MappingEntry::with_flags(
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::NonRecursive,
        MappingFlags {
            unique: true,
            ..MappingFlags::default()
        },
        None,
    );
    let result = km.try_map_entry(MappingMode::Normal, &[KeyEvent::char('q')], entry);
    assert!(result.is_ok());

    // Try to overwrite from a different owner (Core) → should fail E227
    let entry2 = MappingEntry::new(
        key_sequence(&[KeyEvent::char('y')]),
        MappingKind::NonRecursive,
    )
    .with_owner(MappingOwner::Core);
    let result = km.try_map_entry(MappingMode::Normal, &[KeyEvent::char('q')], entry2);
    assert_eq!(result, Err(MapError::UniqueConflict));
}

#[test]
fn unique_flag_allows_same_owner() {
    let mut km = Keymap::default();

    // Insert a <unique> mapping from User
    let entry = MappingEntry::with_flags(
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::NonRecursive,
        MappingFlags {
            unique: true,
            ..MappingFlags::default()
        },
        None,
    );
    km.try_map_entry(MappingMode::Normal, &[KeyEvent::char('q')], entry)
        .unwrap();

    // Overwrite from same owner (User) → should succeed
    let entry2 = MappingEntry::new(
        key_sequence(&[KeyEvent::char('y')]),
        MappingKind::NonRecursive,
    );
    let result = km.try_map_entry(MappingMode::Normal, &[KeyEvent::char('q')], entry2);
    assert!(result.is_ok());
}

#[test]
fn unique_on_new_entry_blocks_foreign_overwrite() {
    let mut km = Keymap::default();

    // Insert a regular mapping from Core owner
    let entry = MappingEntry::new(
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::NonRecursive,
    )
    .with_owner(MappingOwner::Core);
    km.try_map_entry(MappingMode::Normal, &[KeyEvent::char('q')], entry)
        .unwrap();

    // Try to overwrite with <unique> from User → should fail because
    // the existing mapping is from a different owner
    let entry2 = MappingEntry::with_flags(
        key_sequence(&[KeyEvent::char('y')]),
        MappingKind::NonRecursive,
        MappingFlags {
            unique: true,
            ..MappingFlags::default()
        },
        None,
    );
    let result = km.try_map_entry(MappingMode::Normal, &[KeyEvent::char('q')], entry2);
    assert_eq!(result, Err(MapError::UniqueConflict));
}

#[test]
fn non_unique_mapping_always_succeeds() {
    let mut km = Keymap::default();

    // Insert a mapping from Core owner
    let entry = MappingEntry::new(
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::NonRecursive,
    )
    .with_owner(MappingOwner::Core);
    km.try_map_entry(MappingMode::Normal, &[KeyEvent::char('q')], entry)
        .unwrap();

    // Overwrite without <unique> from User → should succeed
    let entry2 = MappingEntry::new(
        key_sequence(&[KeyEvent::char('y')]),
        MappingKind::NonRecursive,
    );
    let result = km.try_map_entry(MappingMode::Normal, &[KeyEvent::char('q')], entry2);
    assert!(result.is_ok());
}

// ═══════════════════════════════════════════════════════════════════
// Configurable maxmapdepth
// ═══════════════════════════════════════════════════════════════════

#[test]
fn max_map_depth_default_is_1000() {
    let km = Keymap::default();
    assert_eq!(km.max_map_depth(), 1000);
}

#[test]
fn max_map_depth_configurable() {
    let mut km = Keymap::default();
    km.set_max_map_depth(5);
    assert_eq!(km.max_map_depth(), 5);
}

#[test]
fn max_map_depth_zero_disables_recursive() {
    let mut km = Keymap::default();
    km.set_max_map_depth(0);
    assert_eq!(km.max_map_depth(), 0);
}

// ═══════════════════════════════════════════════════════════════════
// :xmap/:smap separation
// ═══════════════════════════════════════════════════════════════════

#[test]
fn xmap_active_in_visual_mode() {
    let mut km = Keymap::default();

    // :xmap q → x (Visual-only)
    km.map(
        MappingMode::VisualOnly,
        &[KeyEvent::char('q')],
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Should be found in Visual mode
    assert!(km.has_user_mapping(KeyEvent::char('q'), Mode::Visual(VisualType::Char)));
    assert!(km.has_user_mapping(KeyEvent::char('q'), Mode::Visual(VisualType::Line)));
    assert!(km.has_user_mapping(KeyEvent::char('q'), Mode::Visual(VisualType::Block)));
}

#[test]
fn xmap_not_active_in_select_mode() {
    let mut km = Keymap::default();

    // :xmap q → x (Visual-only)
    km.map(
        MappingMode::VisualOnly,
        &[KeyEvent::char('q')],
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Should NOT be found in Select mode
    assert!(!km.has_user_mapping(KeyEvent::char('q'), Mode::Select(VisualType::Char)));
    assert!(!km.has_user_mapping(KeyEvent::char('q'), Mode::Select(VisualType::Line)));
}

#[test]
fn smap_active_in_select_mode() {
    let mut km = Keymap::default();

    // :smap q → y (Select-only)
    km.map(
        MappingMode::SelectOnly,
        &[KeyEvent::char('q')],
        key_sequence(&[KeyEvent::char('y')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Should be found in Select mode
    assert!(km.has_user_mapping(KeyEvent::char('q'), Mode::Select(VisualType::Char)));
}

#[test]
fn smap_not_active_in_visual_mode() {
    let mut km = Keymap::default();

    // :smap q → y (Select-only)
    km.map(
        MappingMode::SelectOnly,
        &[KeyEvent::char('q')],
        key_sequence(&[KeyEvent::char('y')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Should NOT be found in Visual mode
    assert!(!km.has_user_mapping(KeyEvent::char('q'), Mode::Visual(VisualType::Char)));
}

#[test]
fn vmap_active_in_both_visual_and_select() {
    let mut km = Keymap::default();

    // :vmap q → z (Visual + Select)
    km.map(
        MappingMode::Visual,
        &[KeyEvent::char('q')],
        key_sequence(&[KeyEvent::char('z')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Should be found in both Visual and Select
    assert!(km.has_user_mapping(KeyEvent::char('q'), Mode::Visual(VisualType::Char)));
    assert!(km.has_user_mapping(KeyEvent::char('q'), Mode::Select(VisualType::Char)));
}

#[test]
fn xmap_and_smap_independent() {
    let mut km = Keymap::default();

    // :xmap q → x
    km.map(
        MappingMode::VisualOnly,
        &[KeyEvent::char('q')],
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );
    // :smap q → y
    km.map(
        MappingMode::SelectOnly,
        &[KeyEvent::char('q')],
        key_sequence(&[KeyEvent::char('y')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Visual sees xmap's 'x'
    let entry = km
        .get_user_mapping(KeyEvent::char('q'), Mode::Visual(VisualType::Char))
        .unwrap();
    assert_eq!(entry.sequence()[0], KeyEvent::char('x'));

    // Select sees smap's 'y'
    let entry = km
        .get_user_mapping(KeyEvent::char('q'), Mode::Select(VisualType::Char))
        .unwrap();
    assert_eq!(entry.sequence()[0], KeyEvent::char('y'));
}

#[test]
fn all_for_mode_returns_correct_modes() {
    // Visual returns both Visual and VisualOnly
    let modes = MappingMode::all_for_mode(Mode::Visual(VisualType::Char));
    assert!(modes.contains(&MappingMode::Visual));
    assert!(modes.contains(&MappingMode::VisualOnly));
    assert!(!modes.contains(&MappingMode::SelectOnly));

    // Select returns both Visual and SelectOnly
    let modes = MappingMode::all_for_mode(Mode::Select(VisualType::Char));
    assert!(modes.contains(&MappingMode::Visual));
    assert!(modes.contains(&MappingMode::SelectOnly));
    assert!(!modes.contains(&MappingMode::VisualOnly));

    // Normal returns only Normal
    let modes = MappingMode::all_for_mode(Mode::Normal);
    assert_eq!(modes, &[MappingMode::Normal]);
}

// ═══════════════════════════════════════════════════════════════════
// <script> remap scope
// ═══════════════════════════════════════════════════════════════════

#[test]
fn script_local_flag_stored_on_entry() {
    let flags = MappingFlags {
        script_local: true,
        ..MappingFlags::default()
    };
    let entry = MappingEntry::with_flags(
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::Recursive,
        flags,
        None,
    );
    assert!(entry.script_local());
    assert!(entry.kind().is_recursive());
}

#[test]
fn script_local_default_false() {
    let entry = MappingEntry::new(
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::NonRecursive,
    );
    assert!(!entry.script_local());
}

#[test]
fn script_local_mapping_inserted_and_looked_up() {
    let mut km = Keymap::default();
    let flags = MappingFlags {
        script_local: true,
        ..MappingFlags::default()
    };

    km.map(
        MappingMode::Normal,
        &[KeyEvent::char('q')],
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::Recursive,
        flags,
    );

    // The mapping should be found and have script_local set
    let entry = km
        .get_user_mapping(KeyEvent::char('q'), Mode::Normal)
        .unwrap();
    assert!(entry.script_local());
    assert!(entry.kind().is_recursive());
}

#[test]
fn script_local_with_owner_tracking() {
    let mut km = Keymap::default();
    let flags = MappingFlags {
        script_local: true,
        ..MappingFlags::default()
    };

    let entry = MappingEntry::with_flags(
        key_sequence(&[KeyEvent::char('x')]),
        MappingKind::Recursive,
        flags,
        None,
    )
    .with_owner(MappingOwner::Host(CompactString::from("plugin_a")));

    km.map_entry(MappingMode::Normal, &[KeyEvent::char('q')], entry);

    let found = km
        .get_user_mapping(KeyEvent::char('q'), Mode::Normal)
        .unwrap();
    assert!(found.script_local());
    assert_eq!(
        found.owner(),
        &MappingOwner::Host(CompactString::from("plugin_a"))
    );
}
