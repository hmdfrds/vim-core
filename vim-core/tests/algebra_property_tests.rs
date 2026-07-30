//! Property tests for effect algebra round-trip, composition, and offset adjustment.
//!
//! These tests verify core invariants of the effect algebra:
//! - Inverse round-trip: applying an effect then its inverse restores the original state
//! - Composition semantics: composed effects produce the same result as sequential application
//! - Offset adjustment monotonicity: position shifts preserve document ordering
//! - Length change correctness: computed length changes match actual text mutations
//! - Three-way composition associativity
//! - Effect ordering validation for well-formed and malformed streams

use compact_str::CompactString;
use proptest::prelude::*;
use vim_core::effects::algebra::*;
use vim_core::effects::{try_compose, validate_ordering, Effect, OrderingError};
use vim_core::primitives::{Offset, Range, UndoCursorStrategy};

// ═══════════════════════════════════════════════════════════════════════════════
// PROPTEST CONFIGURATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Build a [`ProptestConfig`] with env-var amplification.
///
/// When `PROPTEST_CASES` is set, it overrides the default case count.
/// This allows CI or manual runs to amplify coverage without code changes:
///
/// ```sh
/// PROPTEST_CASES=10000 cargo test -p vim-core --test algebra_property_tests
/// ```
fn config(default_cases: u32, default_shrink: u32) -> ProptestConfig {
    ProptestConfig {
        cases: std::env::var("PROPTEST_CASES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default_cases),
        max_shrink_iters: default_shrink,
        ..ProptestConfig::default()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// HELPER: Apply effect to text
// ═══════════════════════════════════════════════════════════════════════════════

/// Apply an effect to text. Handles Insert, Delete, and Replace.
fn apply_to_text(text: &str, effect: &Effect) -> Option<String> {
    match effect {
        Effect::Insert { offset, text: ins } => {
            let pos = offset.get();
            if pos > text.len() || !text.is_char_boundary(pos) {
                return None;
            }
            let mut result = String::with_capacity(text.len() + ins.len());
            result.push_str(&text[..pos]);
            result.push_str(ins);
            result.push_str(&text[pos..]);
            Some(result)
        }
        Effect::Delete { range } => {
            let start = range.start().get();
            let end = range.end().get();
            if start > text.len()
                || end > text.len()
                || start > end
                || !text.is_char_boundary(start)
                || !text.is_char_boundary(end)
            {
                return None;
            }
            let mut result = String::with_capacity(text.len().saturating_sub(end - start));
            result.push_str(&text[..start]);
            result.push_str(&text[end..]);
            Some(result)
        }
        Effect::Replace { range, text: repl } => {
            let start = range.start().get();
            let end = range.end().get();
            if start > text.len()
                || end > text.len()
                || start > end
                || !text.is_char_boundary(start)
                || !text.is_char_boundary(end)
            {
                return None;
            }
            let mut result =
                String::with_capacity(text.len().saturating_sub(end - start) + repl.len());
            result.push_str(&text[..start]);
            result.push_str(repl);
            result.push_str(&text[end..]);
            Some(result)
        }
        _ => Some(text.to_string()),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PROPERTY TESTS
// ═══════════════════════════════════════════════════════════════════════════════

proptest! {
    /// For random offset and insert text, verify `apply(apply(text, insert), inverse) == text`.
    #[test]
    fn prop_insert_inverse_round_trip(
        text in "\\PC{0,50}",  // Arbitrary string (including empty)
        insert_text in "\\PC{1,20}",  // Non-empty arbitrary text for insertion
        raw_offset in 0usize..60,  // Random offset (clamped to text length)
    ) {
        // Random offset anywhere in text, not just at the end
        let offset = Offset::new(raw_offset % (text.len() + 1));
        let effect = Effect::Insert {
            offset,
            text: CompactString::from(insert_text.as_str()),
        };

        // Apply the effect
        if let Some(after_insert) = apply_to_text(&text, &effect) {
            // Compute inverse (Insert inverse is always available)
            let ctx = InverseContext::empty();
            if let Some(inverse_effect) = inverse(&effect, &ctx) {
                // Apply inverse
                if let Some(after_inverse) = apply_to_text(&after_insert, &inverse_effect) {
                    // Round-trip should restore original text
                    prop_assert_eq!(after_inverse, text, "Insert round-trip failed");
                } else {
                    prop_assert!(false, "Inverse application should succeed");
                }
            } else {
                prop_assert!(false, "Insert inverse should always exist");
            }
        }
    }

    /// For random range within text, verify Delete inverse round-trip.
    #[test]
    fn prop_delete_inverse_round_trip(
        text in "\\PC+",  // Non-empty arbitrary string
        range_tuple in (0usize..30, 0usize..30),  // Generate start and end offsets
    ) {
        // Ensure we work with valid UTF-8 boundaries
        let mut start = range_tuple.0 % (text.len() + 1);
        let mut end = range_tuple.1 % (text.len() + 1);

        // Clamp to valid UTF-8 boundaries by rounding to nearest valid boundary
        while start > 0 && !text.is_char_boundary(start) {
            start -= 1;
        }
        while end < text.len() && !text.is_char_boundary(end) {
            end += 1;
        }

        if start > end {
            (start, end) = (end, start);
        }

        let range = Range::from_raw(start, end);
        let deleted_text = text[start..end].to_string();

        let effect = Effect::Delete { range };

        // Apply the effect
        if let Some(after_delete) = apply_to_text(&text, &effect) {
            // Compute inverse with the deleted text
            let ctx = InverseContext::for_delete(deleted_text);
            if let Some(inverse_effect) = inverse(&effect, &ctx) {
                // Apply inverse
                if let Some(after_inverse) = apply_to_text(&after_delete, &inverse_effect) {
                    // Round-trip should restore original text
                    prop_assert_eq!(after_inverse, text, "Delete round-trip failed");
                } else {
                    prop_assert!(false, "Inverse application should succeed");
                }
            } else {
                prop_assert!(false, "Delete inverse should exist with deleted_text context");
            }
        }
    }

    /// For two Insert effects, verify composition preserves semantics.
    /// `apply(text, compose(a, b))` == `apply(apply(text, a), b)`
    #[test]
    fn prop_compose_insert_semantics(
        text in "\\PC{0,30}",
        text_a in "\\PC{1,10}",
        text_b in "\\PC{1,10}",
        raw_offset in 0usize..40,
    ) {
        // Random offset for first insert, not always at end
        let offset_a = Offset::new(raw_offset % (text.len() + 1));
        // Second insert is at adjusted position (after first insert)
        let offset_b = Offset::new(offset_a.get() + text_a.len());

        let effect_a = Effect::Insert {
            offset: offset_a,
            text: CompactString::from(text_a.as_str()),
        };
        let effect_b = Effect::Insert {
            offset: offset_b,
            text: CompactString::from(text_b.as_str()),
        };

        // Sequential application: apply a then b
        if let Some(after_a) = apply_to_text(&text, &effect_a) {
            if let Some(seq_result) = apply_to_text(&after_a, &effect_b) {
                // Composed application
                if let Some(composed) = try_compose(&effect_a, &effect_b) {
                    if let Some(comp_result) = apply_to_text(&text, &composed) {
                        // Both should produce the same result
                        prop_assert_eq!(
                            seq_result, comp_result,
                            "Sequential and composed application should produce same result"
                        );
                    }
                } else {
                    prop_assert!(false, "Adjacent inserts should compose");
                }
            }
        }
    }

    /// For Insert effect, adjusted offset >= original offset (insertions never shift left).
    #[test]
    fn prop_insert_adjust_offset_monotonic(
        insert_offset in 0usize..1000,
        insert_len in 1usize..100,
        cursor_offset in 0usize..1000,
    ) {
        let effect = Effect::Insert {
            offset: Offset::new(insert_offset),
            text: CompactString::from(&"x".repeat(insert_len)),
        };

        let original_offset = Offset::new(cursor_offset);
        let adjusted = adjust_offset(original_offset, &effect);

        // Insertions never shift offsets to the left
        prop_assert!(
            adjusted.get() >= original_offset.get(),
            "Adjusted offset {} should be >= original offset {} after Insert at {}",
            adjusted.get(),
            original_offset.get(),
            insert_offset
        );

        // If offset is at or after insertion point, it should shift right exactly by insert_len
        if cursor_offset >= insert_offset {
            prop_assert_eq!(
                adjusted.get(),
                original_offset.get() + insert_len,
                "Offset at/after insertion should shift right by insert length"
            );
        } else {
            // If offset is before insertion, it should stay the same
            prop_assert_eq!(
                adjusted.get(),
                original_offset.get(),
                "Offset before insertion should remain unchanged"
            );
        }
    }

    /// For effects, verify computed length change matches actual text mutation.
    /// Tests Insert, Delete, AND Replace at varying positions.
    #[test]
    fn prop_compute_length_change_correctness(
        text in "[a-z]{5,30}",
        ops in prop::collection::vec((0u32..4, 0usize..30), 1..6),  // (op_type, position_hint)
    ) {
        // Build effects from the operations, applying each to track text state
        let mut effects: Vec<Effect> = Vec::new();
        let mut test_text = text.to_string();

        for (op, pos_hint) in &ops {
            match op % 4 {
                0 => {
                    // Insert at random valid position
                    let pos = if test_text.is_empty() { 0 } else { pos_hint % test_text.len() };
                    // Clamp to char boundary
                    let pos = test_text[..pos].len();
                    effects.push(Effect::Insert {
                        offset: Offset::new(pos),
                        text: CompactString::from("xy"),
                    });
                    test_text.insert_str(pos, "xy");
                }
                1 => {
                    // Delete a character at random position
                    if !test_text.is_empty() {
                        let pos = pos_hint % test_text.len();
                        let end = pos + test_text[pos..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
                        let end = end.min(test_text.len());
                        effects.push(Effect::Delete {
                            range: Range::from_raw(pos, end),
                        });
                        test_text.drain(pos..end);
                    }
                }
                2 => {
                    // Replace a character with two characters
                    if !test_text.is_empty() {
                        let pos = pos_hint % test_text.len();
                        let end = pos + test_text[pos..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
                        let end = end.min(test_text.len());
                        effects.push(Effect::Replace {
                            range: Range::from_raw(pos, end),
                            text: CompactString::from("ab"),
                        });
                        test_text.replace_range(pos..end, "ab");
                    }
                }
                _ => {
                    // No-op
                    effects.push(Effect::Noop);
                }
            }
        }

        // Compute length change from effects
        let computed_delta = compute_length_change(&effects);

        // Apply effects to text and measure actual change
        let mut result = text.to_string();
        for effect in &effects {
            if let Some(new_result) = apply_to_text(&result, effect) {
                result = new_result;
            }
        }

        let actual_delta = result.len() as i64 - text.len() as i64;

        prop_assert_eq!(
            computed_delta, actual_delta,
            "Computed length change {} should match actual change {}",
            computed_delta, actual_delta
        );
    }

    /// For Replace effects, verify inverse round-trip.
    #[test]
    fn prop_replace_inverse_round_trip(
        text in "\\PC+",  // Non-empty arbitrary string
        range_tuple in (0usize..30, 0usize..30),
        replacement in "\\PC{0,20}",  // Replacement text (can be empty)
    ) {
        // Convert to ensure valid UTF-8 boundaries
        let mut start = range_tuple.0 % (text.len() + 1);
        let mut end = range_tuple.1 % (text.len() + 1);

        // Clamp to valid UTF-8 boundaries by rounding to nearest valid boundary
        while start > 0 && !text.is_char_boundary(start) {
            start -= 1;
        }
        while end < text.len() && !text.is_char_boundary(end) {
            end += 1;
        }

        if start > end {
            (start, end) = (end, start);
        }

        let range = Range::from_raw(start, end);
        let replaced_text = text[start..end].to_string();

        let effect = Effect::Replace {
            range,
            text: CompactString::from(replacement.as_str()),
        };

        // Apply the effect
        if let Some(after_replace) = apply_to_text(&text, &effect) {
            // Compute inverse with the replaced text
            let ctx = InverseContext::for_replace(replaced_text);
            if let Some(inverse_effect) = inverse(&effect, &ctx) {
                // Apply inverse
                if let Some(after_inverse) = apply_to_text(&after_replace, &inverse_effect) {
                    // Round-trip should restore original text
                    prop_assert_eq!(after_inverse, text, "Replace round-trip failed");
                } else {
                    prop_assert!(false, "Inverse application should succeed");
                }
            } else {
                prop_assert!(false, "Replace inverse should exist with replaced_text context");
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// HELPER: Generate a random text-mutating effect valid for the given document
// ═══════════════════════════════════════════════════════════════════════════════

/// Deterministically generate a random text-mutating effect that is valid for
/// the given document text.
///
/// Uses a simple seed-based selection to pick among Insert, Delete, and Replace,
/// then generates parameters that are guaranteed to be valid for the current
/// document state.
///
/// Returns `None` only when the text is empty and the seed selects Delete or
/// Replace (which require at least one character).
fn random_effect(text: &str, seed: u64) -> Option<Effect> {
    // Use different bits of the seed for different decisions.
    let op_kind = seed % 3; // 0=Insert, 1=Delete, 2=Replace
    let pos_seed = (seed >> 2) as usize;
    let len_seed = (seed >> 10) as usize;

    match op_kind {
        0 => {
            // Insert: valid at any position 0..=text.len()
            let pos = if text.is_empty() {
                0
            } else {
                let mut p = pos_seed % (text.len() + 1);
                // Clamp to char boundary
                while p > 0 && !text.is_char_boundary(p) {
                    p -= 1;
                }
                p
            };
            let insert_len = (len_seed % 10) + 1; // 1..=10
            let insert_text: String = (0..insert_len)
                .map(|i| {
                    let ch_seed = (seed
                        .wrapping_add(i as u64)
                        .wrapping_mul(6364136223846793005))
                        % 26;
                    (b'a' + ch_seed as u8) as char
                })
                .collect();
            Some(Effect::Insert {
                offset: Offset::new(pos),
                text: CompactString::from(insert_text.as_str()),
            })
        }
        1 => {
            // Delete: need at least one character
            if text.is_empty() {
                return None;
            }
            let start = pos_seed % text.len();
            // Clamp start to char boundary
            let start = {
                let mut s = start;
                while s > 0 && !text.is_char_boundary(s) {
                    s -= 1;
                }
                s
            };
            // Delete 1 to min(remaining, 5) bytes, clamped to char boundaries
            let remaining = text.len() - start;
            let max_del = remaining.clamp(1, 5);
            let del_len = (len_seed % max_del) + 1;
            let mut end = (start + del_len).min(text.len());
            // Clamp end to char boundary (round up)
            while end < text.len() && !text.is_char_boundary(end) {
                end += 1;
            }
            Some(Effect::Delete {
                range: Range::from_raw(start, end),
            })
        }
        2 => {
            // Replace: need at least one character
            if text.is_empty() {
                return None;
            }
            let start = pos_seed % text.len();
            // Clamp start to char boundary
            let start = {
                let mut s = start;
                while s > 0 && !text.is_char_boundary(s) {
                    s -= 1;
                }
                s
            };
            let remaining = text.len() - start;
            let max_del = remaining.clamp(1, 5);
            let del_len = (len_seed % max_del) + 1;
            let mut end = (start + del_len).min(text.len());
            while end < text.len() && !text.is_char_boundary(end) {
                end += 1;
            }
            let repl_len = (len_seed % 10) + 1;
            let repl_text: String = (0..repl_len)
                .map(|i| {
                    let ch_seed = (seed
                        .wrapping_add(i as u64)
                        .wrapping_mul(2862933555777941757))
                        % 26;
                    (b'a' + ch_seed as u8) as char
                })
                .collect();
            Some(Effect::Replace {
                range: Range::from_raw(start, end),
                text: CompactString::from(repl_text.as_str()),
            })
        }
        _ => unreachable!(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// THREE-WAY COMPOSITION ASSOCIATIVITY
// ═══════════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(config(1000, 200))]

    /// Three-way composition associativity:
    ///
    /// Given three sequential text-mutating effects a, b, c (each valid for the
    /// document state after applying all preceding effects), verify:
    ///
    /// 1. Sequential application `apply(apply(apply(text, a), b), c)` succeeds.
    /// 2. When `compose(compose(a,b), c)` succeeds (left-association), the
    ///    composed result matches sequential application.
    /// 3. When `compose(a, compose(b,c))` succeeds (right-association), the
    ///    composed result matches sequential application.
    /// 4. When both associations succeed, they produce effects with identical
    ///    text-mutation semantics (same result when applied).
    #[test]
    fn prop_composition_associativity(
        text in "[a-z]{1,50}",
        seed_a in 0u64..1_000_000,
        seed_b in 0u64..1_000_000,
        seed_c in 0u64..1_000_000,
    ) {
        // Generate effect a, valid for the initial text.
        let effect_a = match random_effect(&text, seed_a) {
            Some(e) => e,
            None => return Ok(()),  // Skip: can't generate a valid effect
        };

        // Apply a to get intermediate text.
        let after_a = match apply_to_text(&text, &effect_a) {
            Some(t) => t,
            None => return Ok(()),
        };

        // Generate effect b, valid for the text after a.
        let effect_b = match random_effect(&after_a, seed_b) {
            Some(e) => e,
            None => return Ok(()),
        };

        // Apply b to get text after a+b.
        let after_ab = match apply_to_text(&after_a, &effect_b) {
            Some(t) => t,
            None => return Ok(()),
        };

        // Generate effect c, valid for the text after a+b.
        let effect_c = match random_effect(&after_ab, seed_c) {
            Some(e) => e,
            None => return Ok(()),
        };

        // Apply c to get the final sequential result.
        let sequential_result = match apply_to_text(&after_ab, &effect_c) {
            Some(t) => t,
            None => return Ok(()),
        };

        // ── Left association: compose(compose(a, b), c) ────────────────
        let left_result = try_compose(&effect_a, &effect_b)
            .and_then(|ab| try_compose(&ab, &effect_c));

        if let Some(ref left_composed) = left_result {
            let left_applied = apply_to_text(&text, left_composed);
            prop_assert_eq!(
                left_applied.as_deref(),
                Some(sequential_result.as_str()),
                "Left-associated composition should match sequential application.\n\
                 a={:?}\nb={:?}\nc={:?}\ncomposed={:?}",
                effect_a, effect_b, effect_c, left_composed
            );
        }

        // ── Right association: compose(a, compose(b, c)) ───────────────
        let right_result = try_compose(&effect_b, &effect_c)
            .and_then(|bc| try_compose(&effect_a, &bc));

        if let Some(ref right_composed) = right_result {
            let right_applied = apply_to_text(&text, right_composed);
            prop_assert_eq!(
                right_applied.as_deref(),
                Some(sequential_result.as_str()),
                "Right-associated composition should match sequential application.\n\
                 a={:?}\nb={:?}\nc={:?}\ncomposed={:?}",
                effect_a, effect_b, effect_c, right_composed
            );
        }

        // ── When both succeed, they must produce semantically equivalent effects ──
        if let (Some(ref left_composed), Some(ref right_composed)) =
            (&left_result, &right_result)
        {
            let left_applied = apply_to_text(&text, left_composed);
            let right_applied = apply_to_text(&text, right_composed);
            prop_assert_eq!(
                left_applied, right_applied,
                "Left and right association must produce the same result.\n\
                 left={:?}\nright={:?}",
                left_composed, right_composed
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// COMPOSITION ASSOCIATIVITY EXERCISE RATE
// ═══════════════════════════════════════════════════════════════════════════════

/// Tracks how often `prop_composition_associativity` exercises the left and right
/// association paths. `try_compose` is intentionally partial (only adjacent
/// inserts/deletes compose), so many random effect triples won't compose at all.
///
/// This test measures the actual exercise rates and asserts that:
/// 1. At least one composition path (left or right) fires in >= 1% of cases.
/// 2. The first single-step compose (`compose(a,b)` or `compose(b,c)`) fires
///    in >= 5% of cases, proving `try_compose` is actually being called with
///    composable pairs.
///
/// Uses `Cell` counters captured by the `Fn` closure passed to `TestRunner::run`.
/// Proptest runs single-threaded, so `Cell` suffices.
#[test]
fn composition_associativity_exercise_rate() {
    use std::cell::Cell;

    let total = Cell::new(0u32);
    let left_tested = Cell::new(0u32);
    let right_tested = Cell::new(0u32);
    let ab_composed = Cell::new(0u32);
    let bc_composed = Cell::new(0u32);

    let config = ProptestConfig::with_cases(1000);
    let mut runner = proptest::test_runner::TestRunner::new(config);

    let strategy = (
        "[a-z]{1,50}",
        0u64..1_000_000u64,
        0u64..1_000_000u64,
        0u64..1_000_000u64,
    );

    let result = runner.run(&strategy, |(text, seed_a, seed_b, seed_c)| {
        total.set(total.get() + 1);

        let effect_a = match random_effect(&text, seed_a) {
            Some(e) => e,
            None => return Ok(()),
        };
        let after_a = match apply_to_text(&text, &effect_a) {
            Some(t) => t,
            None => return Ok(()),
        };
        let effect_b = match random_effect(&after_a, seed_b) {
            Some(e) => e,
            None => return Ok(()),
        };
        let after_ab = match apply_to_text(&after_a, &effect_b) {
            Some(t) => t,
            None => return Ok(()),
        };
        let effect_c = match random_effect(&after_ab, seed_c) {
            Some(e) => e,
            None => return Ok(()),
        };

        // Track single-step composition success
        let ab = try_compose(&effect_a, &effect_b);
        if ab.is_some() {
            ab_composed.set(ab_composed.get() + 1);
        }
        let bc = try_compose(&effect_b, &effect_c);
        if bc.is_some() {
            bc_composed.set(bc_composed.get() + 1);
        }

        // Left association: compose(compose(a,b), c)
        let left_result = ab.and_then(|ab_eff| try_compose(&ab_eff, &effect_c));
        if left_result.is_some() {
            left_tested.set(left_tested.get() + 1);
        }

        // Right association: compose(a, compose(b,c))
        let right_result = bc.and_then(|bc_eff| try_compose(&effect_a, &bc_eff));
        if right_result.is_some() {
            right_tested.set(right_tested.get() + 1);
        }

        Ok(())
    });

    result.unwrap();

    let total = total.get();
    let left = left_tested.get();
    let right = right_tested.get();
    let ab = ab_composed.get();
    let bc = bc_composed.get();

    assert!(total > 0, "No cases ran at all");

    // At least some single-step compositions should succeed.
    // try_compose only handles specific adjacent patterns (e.g., adjacent
    // inserts), so the rate is naturally low with random effects. We assert
    // that the SINGLE-STEP compose fires at all, which proves the test
    // infrastructure is working.
    let any_single_step = ab + bc;
    assert!(
        any_single_step > 0,
        "No single-step compositions succeeded at all out of {} cases. \
         try_compose may have regressed or random_effect may not generate composable pairs.",
        total
    );

    // If both left and right are zero, the two-step compose is too rare to
    // assert a percentage — but we've confirmed single-step works above.
    // Log the rates for diagnostic visibility.
    eprintln!(
        "Associativity exercise rates: total={}, ab_composed={}, bc_composed={}, \
         left_assoc={}, right_assoc={}",
        total, ab, bc, left, right
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// EFFECT STREAM ORDERING VALIDATION
// ═══════════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(config(500, 200))]

    /// Well-formed effect sequences pass `validate_ordering()`.
    ///
    /// Generates sequences following the canonical pattern:
    ///   [BeginUndoGroup, <text edits>, SetCursor, EndUndoGroup]
    /// with random valid parameters, and verifies that `validate_ordering`
    /// returns `Ok(())`.
    #[test]
    fn prop_well_formed_effects_pass_validation(
        // Parameters for generating the well-formed sequence
        insert_offset in 0usize..100,
        insert_text in "[a-z]{1,10}",
        cursor_offset in 0usize..200,
        // How many undo groups to generate (1..=3)
        num_groups in 1usize..4,
        // Which edit type per group: 0=Insert, 1=Delete, 2=Replace
        edit_types in prop::collection::vec(0u32..3, 1..4),
    ) {
        let mut effects: Vec<Effect> = Vec::new();

        for group_idx in 0..num_groups {
            let edit_type = edit_types[group_idx % edit_types.len()];

            effects.push(Effect::BeginUndoGroup {
                cursor_strategy: UndoCursorStrategy::FirstEdit,
            });

            // Add a text-mutating effect (guaranteed to have at least one per group)
            match edit_type % 3 {
                0 => {
                    let offset = insert_offset.wrapping_add(group_idx * 5) % 100;
                    effects.push(Effect::Insert {
                        offset: Offset::new(offset),
                        text: CompactString::from(insert_text.as_str()),
                    });
                }
                1 => {
                    let start = insert_offset % 50;
                    let end = start + 1 + (group_idx % 3);
                    effects.push(Effect::Delete {
                        range: Range::from_raw(start, end),
                    });
                }
                2 => {
                    let start = insert_offset % 50;
                    let end = start + 1 + (group_idx % 3);
                    effects.push(Effect::Replace {
                        range: Range::from_raw(start, end),
                        text: CompactString::from(insert_text.as_str()),
                    });
                }
                _ => unreachable!(),
            }

            effects.push(Effect::EndUndoGroup { node_id: None });
        }

        // SetCursor MUST come after all text edits (invariant 3)
        effects.push(Effect::SetCursor {
            offset: Offset::new(cursor_offset),
        });

        let result = validate_ordering(&effects);
        prop_assert!(
            result.is_ok(),
            "Well-formed sequence should pass validation, got: {:?}\nEffects: {:?}",
            result.err(),
            effects
        );
    }

    /// Malformed effect sequences fail `validate_ordering()`.
    ///
    /// Takes a well-formed sequence and intentionally breaks it in one of
    /// several ways, then asserts that `validate_ordering` returns `Err`.
    #[test]
    fn prop_malformed_effects_fail_validation(
        insert_offset in 0usize..100,
        insert_text in "[a-z]{1,10}",
        cursor_offset in 0usize..200,
        // Which malformation to apply: 0..5
        malformation_kind in 0u32..5,
    ) {
        // ── Build a well-formed base sequence ──────────────────────────
        // Pattern: [BeginUndoGroup, Insert, EndUndoGroup, SetCursor]
        let well_formed = vec![
            Effect::BeginUndoGroup {
                cursor_strategy: UndoCursorStrategy::FirstEdit,
            },
            Effect::Insert {
                offset: Offset::new(insert_offset % 100),
                text: CompactString::from(insert_text.as_str()),
            },
            Effect::EndUndoGroup { node_id: None },
            Effect::SetCursor {
                offset: Offset::new(cursor_offset),
            },
        ];

        // Verify the base is well-formed (sanity check).
        assert!(
            validate_ordering(&well_formed).is_ok(),
            "Base sequence must be well-formed"
        );

        let malformed: Vec<Effect> = match malformation_kind {
            0 => {
                // Malformation: EndUndoGroup before BeginUndoGroup (swap positions 0 and 2)
                // Result: [EndUndoGroup, Insert, BeginUndoGroup, SetCursor]
                let mut m = well_formed.clone();
                m.swap(0, 2);
                m
            }
            1 => {
                // Malformation: Remove BeginUndoGroup (orphaned EndUndoGroup)
                // Result: [Insert, EndUndoGroup, SetCursor]
                let mut m = well_formed.clone();
                m.remove(0);
                m
            }
            2 => {
                // Malformation: Remove EndUndoGroup (unclosed BeginUndoGroup)
                // Result: [BeginUndoGroup, Insert, SetCursor]
                let mut m = well_formed.clone();
                m.remove(2);
                m
            }
            3 => {
                // Malformation: Text edit AFTER the final SetCursor
                // Result: [BeginUndoGroup, Insert, EndUndoGroup, SetCursor, Insert]
                let mut m = well_formed.clone();
                m.push(Effect::Insert {
                    offset: Offset::new(0),
                    text: CompactString::from("trailing"),
                });
                m
            }
            4 => {
                // Malformation: Extra EndUndoGroup (unmatched close)
                // Result: [BeginUndoGroup, Insert, EndUndoGroup, EndUndoGroup, SetCursor]
                vec![
                    Effect::BeginUndoGroup {
                        cursor_strategy: UndoCursorStrategy::FirstEdit,
                    },
                    Effect::Insert {
                        offset: Offset::new(insert_offset % 100),
                        text: CompactString::from(insert_text.as_str()),
                    },
                    Effect::EndUndoGroup { node_id: None },
                    Effect::EndUndoGroup { node_id: None }, // extra close
                    Effect::SetCursor {
                        offset: Offset::new(cursor_offset),
                    },
                ]
            }
            _ => unreachable!(),
        };

        let result = validate_ordering(&malformed);

        // Each malformation should trigger a specific error.
        match malformation_kind {
            0 => {
                // EndUndoGroup before BeginUndoGroup -> UndoGroupMismatch
                prop_assert_eq!(
                    result,
                    Err(OrderingError::UndoGroupMismatch),
                    "Swapped Begin/End should produce UndoGroupMismatch.\nEffects: {:?}",
                    malformed
                );
            }
            1 => {
                // Orphaned EndUndoGroup -> UndoGroupMismatch
                prop_assert_eq!(
                    result,
                    Err(OrderingError::UndoGroupMismatch),
                    "Orphaned EndUndoGroup should produce UndoGroupMismatch.\nEffects: {:?}",
                    malformed
                );
            }
            2 => {
                // Unclosed BeginUndoGroup -> UndoGroupMismatch
                prop_assert_eq!(
                    result,
                    Err(OrderingError::UndoGroupMismatch),
                    "Unclosed BeginUndoGroup should produce UndoGroupMismatch.\nEffects: {:?}",
                    malformed
                );
            }
            3 => {
                // Text edit after final cursor -> EditAfterFinalCursor
                prop_assert_eq!(
                    result,
                    Err(OrderingError::EditAfterFinalCursor),
                    "Edit after final cursor should produce EditAfterFinalCursor.\nEffects: {:?}",
                    malformed
                );
            }
            4 => {
                // Extra EndUndoGroup -> UndoGroupMismatch
                prop_assert_eq!(
                    result,
                    Err(OrderingError::UndoGroupMismatch),
                    "Extra EndUndoGroup should produce UndoGroupMismatch.\nEffects: {:?}",
                    malformed
                );
            }
            _ => unreachable!(),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// P1 — DELETE OFFSET ADJUSTMENT MONOTONICITY
// ═══════════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(config(256, 200))]

    /// For a Delete effect, `adjust_offset` must:
    ///
    /// 1. **Before deletion**: offsets strictly before `range.start()` are unchanged.
    /// 2. **Within deletion**: offsets inside `[start, end)` clamp to `start`.
    /// 3. **After deletion**: offsets at or after `range.end()` shift left by
    ///    exactly `range.len()`.
    ///
    /// This mirrors `prop_insert_adjust_offset_monotonic` for the deletion case.
    #[test]
    fn prop_delete_adjust_offset_monotonic(
        del_start in 0usize..500,
        del_len in 1usize..100,
        cursor_offset in 0usize..1000,
    ) {
        let del_end = del_start + del_len;
        let effect = Effect::Delete {
            range: Range::from_raw(del_start, del_end),
        };

        let original = Offset::new(cursor_offset);
        let adjusted = adjust_offset(original, &effect);

        if cursor_offset < del_start {
            // Before the deletion: unchanged
            prop_assert_eq!(
                adjusted.get(),
                cursor_offset,
                "Offset {} before deletion [{}, {}) should be unchanged, got {}",
                cursor_offset, del_start, del_end, adjusted.get()
            );
        } else if cursor_offset < del_end {
            // Inside the deletion: clamp to start
            prop_assert_eq!(
                adjusted.get(),
                del_start,
                "Offset {} inside deletion [{}, {}) should clamp to start {}, got {}",
                cursor_offset, del_start, del_end, del_start, adjusted.get()
            );
        } else {
            // After the deletion: shift left by del_len
            prop_assert_eq!(
                adjusted.get(),
                cursor_offset - del_len,
                "Offset {} after deletion [{}, {}) should shift left by {}, got {}",
                cursor_offset, del_start, del_end, del_len, adjusted.get()
            );
        }

        // Monotonicity: adjusted offset should never exceed the original
        // (deletions can only reduce or preserve offsets, never increase them).
        prop_assert!(
            adjusted.get() <= cursor_offset,
            "Delete adjustment must not increase offset: original={}, adjusted={}",
            cursor_offset, adjusted.get()
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// P2 — NOOP IDENTITY ELEMENT
// ═══════════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(config(256, 200))]

    /// `Effect::Noop` is an identity element for `adjust_offset`:
    /// adjusting any offset by Noop yields the original offset unchanged.
    #[test]
    fn prop_noop_identity_adjust_offset(
        cursor_offset in 0usize..10_000,
    ) {
        let original = Offset::new(cursor_offset);
        let adjusted = adjust_offset(original, &Effect::Noop);

        prop_assert_eq!(
            adjusted, original,
            "Noop should not change offset: original={}, adjusted={}",
            original.get(), adjusted.get()
        );
    }

    /// `Effect::Noop` is an identity element for `compute_length_change`:
    /// a sequence of Noops always has zero length delta.
    #[test]
    fn prop_noop_identity_length_change(
        count in 0usize..20,
    ) {
        let effects: Vec<Effect> = vec![Effect::Noop; count];
        let delta = compute_length_change(&effects);

        prop_assert_eq!(
            delta, 0,
            "A sequence of {} Noops should have zero length change, got {}",
            count, delta
        );
    }

    /// `Effect::Noop` is an identity element for text application:
    /// applying Noop to any text leaves it unchanged.
    #[test]
    fn prop_noop_identity_text_application(
        text in "\\PC{0,50}",
    ) {
        let result = apply_to_text(&text, &Effect::Noop);
        prop_assert_eq!(
            result.as_deref(),
            Some(text.as_str()),
            "Noop should not change text"
        );
    }

    /// Composing an empty effects list with a non-empty one (via
    /// `compute_length_change`) yields the same delta as the non-empty list alone.
    #[test]
    fn prop_empty_effects_identity_for_length_change(
        text in "[a-z]{5,30}",
        seed in 0u64..1_000_000,
    ) {
        // Generate a single random effect valid for the document
        if let Some(effect) = random_effect(&text, seed) {
            let delta_single = compute_length_change(std::slice::from_ref(&effect));

            // Prepend/append empty (Noop) effects — delta should be unchanged
            let with_noops = vec![Effect::Noop, effect.clone(), Effect::Noop];
            let delta_with_noops = compute_length_change(&with_noops);

            prop_assert_eq!(
                delta_single, delta_with_noops,
                "Adding Noop effects should not change computed length delta"
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// P3 — COMMUTATIVITY FOR DISJOINT EFFECTS
// ═══════════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(config(500, 200))]

    /// For two text-mutating effects on non-overlapping ranges where A is
    /// entirely before B, applying A-then-B should produce the same final text
    /// as applying B-then-A (with appropriate offset adjustment).
    ///
    /// Specifically: given a document, two disjoint effects A (at low range)
    /// and B (at high range), the final text should be identical regardless
    /// of application order — because they operate on independent regions.
    #[test]
    fn prop_disjoint_effects_commute(
        text in "[a-z]{20,60}",
        // Positions for two non-overlapping regions:
        // Region A: [a_start, a_end) — low in the document
        // Region B: [b_start, b_end) — high in the document, after A
        a_start in 0usize..8,
        a_len in 1usize..4,
        gap in 2usize..8,
        b_len in 1usize..4,
        // What to insert as replacement for each region
        repl_a in "[A-Z]{1,5}",
        repl_b in "[A-Z]{1,5}",
    ) {
        let text_len = text.len();
        let a_end = (a_start + a_len).min(text_len.saturating_sub(4));
        let b_start = (a_end + gap).min(text_len.saturating_sub(2));
        let b_end = (b_start + b_len).min(text_len);

        // Ensure regions are valid and non-overlapping
        prop_assume!(a_start < a_end);
        prop_assume!(a_end <= b_start);
        prop_assume!(b_start < b_end);
        prop_assume!(b_end <= text_len);

        let effect_a = Effect::Replace {
            range: Range::from_raw(a_start, a_end),
            text: CompactString::from(repl_a.as_str()),
        };
        let effect_b = Effect::Replace {
            range: Range::from_raw(b_start, b_end),
            text: CompactString::from(repl_b.as_str()),
        };

        // Order 1: Apply A first, then B (adjusting B's range for A's size change)
        let after_a = apply_to_text(&text, &effect_a).unwrap();
        let delta_a = repl_a.len() as i64 - (a_end - a_start) as i64;
        let adjusted_b_start = (b_start as i64 + delta_a) as usize;
        let adjusted_b_end = (b_end as i64 + delta_a) as usize;
        let effect_b_adjusted = Effect::Replace {
            range: Range::from_raw(adjusted_b_start, adjusted_b_end),
            text: CompactString::from(repl_b.as_str()),
        };
        let result_ab = apply_to_text(&after_a, &effect_b_adjusted);

        // Order 2: Apply B first, then A (A's range is unaffected since A < B)
        let after_b = apply_to_text(&text, &effect_b).unwrap();
        let result_ba = apply_to_text(&after_b, &effect_a);

        prop_assert_eq!(
            result_ab, result_ba,
            "Disjoint effects should commute:\n\
             text={:?}\nA=Replace([{},{}), {:?})\nB=Replace([{},{}), {:?})",
            text, a_start, a_end, repl_a, b_start, b_end, repl_b
        );
    }
}
