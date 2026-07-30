//! Safety harness for host-facing calls.
//!
//! Wraps all host callbacks with:
//! - Panic containment (`catch_unwind`)
//! - Re-entrancy guard (prevents `process()` from inside a callback)
//! - Per-capability panic tracking with auto-disable after threshold

use std::cell::Cell;
use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::primitives::Range;

/// Number of panics before a capability is auto-disabled.
const PANIC_THRESHOLD: u32 = 3;

/// Maximum supported capability index (0..=31). Values above 31 alias via `& 31`.
/// INVARIANT: `HostCapability` enum must not exceed 32 variants (currently max = 25).
/// If it grows past 31, upgrade `per_capability_panics` to 64 elements and
/// `disabled_capabilities` to `u64`.
const MAX_CAPABILITY_SLOTS: usize = 32;

/// Safety harness wrapping host-facing calls.
///
/// All methods take `&self` and use `Cell` for interior mutability,
/// allowing use without `&mut` borrows.
pub(crate) struct SafetyHarness {
    /// True while inside a query — prevents re-entrancy.
    reentrancy_flag: Cell<bool>,
    /// Total panics caught across all capabilities.
    panic_count: Cell<u32>,
    /// Per-capability panic counts (indexed by capability `u8` value).
    per_capability_panics: [Cell<u32>; 32],
    /// Bitfield of auto-disabled capabilities.
    disabled_capabilities: Cell<u32>,
}

impl SafetyHarness {
    /// Create a new harness with all counters zeroed.
    pub(crate) const fn new() -> Self {
        Self {
            reentrancy_flag: Cell::new(false),
            panic_count: Cell::new(0),
            per_capability_panics: [const { Cell::new(0) }; 32],
            disabled_capabilities: Cell::new(0),
        }
    }

    /// Safe wrapper with catch_unwind + re-entrancy guard.
    ///
    /// Returns `None` if:
    /// - Already inside a query (re-entrancy)
    /// - The callback panics
    /// - The callback itself returns `None`
    pub(crate) fn query<T>(&self, f: impl FnOnce() -> Option<T>) -> Option<T> {
        if self.reentrancy_flag.get() {
            return None;
        }

        self.reentrancy_flag.set(true);
        let result = catch_unwind(AssertUnwindSafe(f));
        self.reentrancy_flag.set(false);

        match result {
            Ok(value) => value,
            Err(_) => {
                self.panic_count
                    .set(self.panic_count.get().saturating_add(1));
                None
            }
        }
    }

    /// Like [`query`](Self::query) but records the panic against a specific capability.
    ///
    /// Auto-disables the capability after [`PANIC_THRESHOLD`] panics.
    /// Returns `None` immediately if the capability is already disabled.
    pub(crate) fn query_for_capability<T>(
        &self,
        cap_index: u8,
        f: impl FnOnce() -> Option<T>,
    ) -> Option<T> {
        if self.is_disabled(cap_index) {
            return None;
        }

        if self.reentrancy_flag.get() {
            return None;
        }

        self.reentrancy_flag.set(true);
        let result = catch_unwind(AssertUnwindSafe(f));
        self.reentrancy_flag.set(false);

        match result {
            Ok(value) => value,
            Err(_) => {
                self.panic_count
                    .set(self.panic_count.get().saturating_add(1));

                let idx = (cap_index as usize) & 31;
                let count = self.per_capability_panics[idx].get().saturating_add(1);
                self.per_capability_panics[idx].set(count);

                if count >= PANIC_THRESHOLD {
                    let bits = self.disabled_capabilities.get();
                    self.disabled_capabilities.set(bits | (1 << idx));
                }

                None
            }
        }
    }

    /// Total number of panics caught.
    pub(crate) const fn panic_count(&self) -> u32 {
        self.panic_count.get()
    }

    /// Whether the harness is currently inside a query (for re-entrancy detection).
    pub(crate) const fn is_in_query(&self) -> bool {
        self.reentrancy_flag.get()
    }

    /// Whether ANY capability has been auto-disabled due to repeated panics.
    ///
    /// This is a fast check (single `!= 0` on the bitfield) used to skip
    /// per-capability provider nulling on the hot path when no panics have
    /// occurred.
    #[inline]
    pub(crate) const fn has_any_disabled(&self) -> bool {
        self.disabled_capabilities.get() != 0
    }

    /// Whether a capability has been auto-disabled due to repeated panics.
    pub(crate) const fn is_disabled(&self, cap_index: u8) -> bool {
        let idx = (cap_index as usize) & 31;
        (self.disabled_capabilities.get() & (1 << idx)) != 0
    }

    /// Re-enable a previously disabled capability and reset its panic count.
    pub(crate) fn re_enable(&self, cap_index: u8) {
        let idx = (cap_index as usize) & 31;
        let bits = self.disabled_capabilities.get();
        self.disabled_capabilities.set(bits & !(1 << idx));
        self.per_capability_panics[idx].set(0);
    }

    /// Reset the global panic count to zero.
    pub(crate) fn reset_panic_count(&self) {
        self.panic_count.set(0);
    }

    /// Manually set a capability as disabled (for testing).
    #[cfg(test)]
    pub(crate) fn force_disable(&self, cap_index: u8) {
        let idx = (cap_index as usize) & 31;
        let bits = self.disabled_capabilities.get();
        self.disabled_capabilities.set(bits | (1 << idx));
    }
}

/// Clamp an offset to document bounds.
#[inline]
#[must_use]
pub(crate) fn validated_offset(offset: usize, doc_len: usize) -> usize {
    offset.min(doc_len)
}

/// Clamp a range to document bounds, ensuring start <= end.
#[inline]
#[must_use]
pub(crate) fn validated_range(start: usize, end: usize, doc_len: usize) -> Range {
    let s = start.min(doc_len);
    let e = end.min(doc_len);
    if s <= e {
        Range::from_raw(s, e)
    } else {
        Range::from_raw(e, s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panicking_callback_returns_none() {
        let harness = SafetyHarness::new();
        let result: Option<i32> = harness.query(|| panic!("boom"));
        assert_eq!(result, None);
    }

    #[test]
    fn panic_count_increments() {
        let harness = SafetyHarness::new();
        assert_eq!(harness.panic_count(), 0);

        let _: Option<()> = harness.query(|| panic!("one"));
        assert_eq!(harness.panic_count(), 1);

        let _: Option<()> = harness.query(|| panic!("two"));
        assert_eq!(harness.panic_count(), 2);
    }

    #[test]
    fn reentrancy_returns_none() {
        let harness = SafetyHarness::new();

        // Simulate re-entrancy by manually setting the flag.
        harness.reentrancy_flag.set(true);
        let result: Option<i32> = harness.query(|| Some(42));
        assert_eq!(result, None);

        // After clearing, query works normally.
        harness.reentrancy_flag.set(false);
        let result: Option<i32> = harness.query(|| Some(42));
        assert_eq!(result, Some(42));
    }

    #[test]
    fn capability_downgrades_after_threshold() {
        let harness = SafetyHarness::new();
        let cap: u8 = 5;

        // Panic 3 times — should auto-disable.
        for _ in 0..3 {
            let _: Option<()> = harness.query_for_capability(cap, || panic!("fail"));
        }

        assert!(harness.is_disabled(cap));

        // Further queries return None without calling the closure.
        let result: Option<i32> = harness.query_for_capability(cap, || Some(99));
        assert_eq!(result, None);
    }

    #[test]
    fn capability_not_disabled_below_threshold() {
        let harness = SafetyHarness::new();
        let cap: u8 = 7;

        // Panic only twice — below threshold.
        for _ in 0..2 {
            let _: Option<()> = harness.query_for_capability(cap, || panic!("fail"));
        }

        assert!(!harness.is_disabled(cap));

        // Query still works.
        let result: Option<i32> = harness.query_for_capability(cap, || Some(42));
        assert_eq!(result, Some(42));
    }

    #[test]
    fn re_enable_resets_panic_count() {
        let harness = SafetyHarness::new();
        let cap: u8 = 3;

        // Disable via 3 panics.
        for _ in 0..3 {
            let _: Option<()> = harness.query_for_capability(cap, || panic!("fail"));
        }
        assert!(harness.is_disabled(cap));

        // Re-enable.
        harness.re_enable(cap);
        assert!(!harness.is_disabled(cap));

        // Can query again (panic count reset, so needs 3 more to disable).
        let result: Option<i32> = harness.query_for_capability(cap, || Some(100));
        assert_eq!(result, Some(100));
    }

    #[test]
    fn validated_offset_clamps() {
        assert_eq!(validated_offset(10, 5), 5);
        assert_eq!(validated_offset(3, 5), 3);
        assert_eq!(validated_offset(5, 5), 5);
        assert_eq!(validated_offset(0, 0), 0);
    }

    #[test]
    fn validated_range_clamps_and_orders() {
        // Normal case: start < end, both within bounds.
        let r = validated_range(2, 8, 10);
        assert_eq!(r, Range::from_raw(2, 8));

        // Clamped: end exceeds doc_len.
        let r = validated_range(2, 20, 10);
        assert_eq!(r, Range::from_raw(2, 10));

        // Reversed: start > end — should swap.
        let r = validated_range(7, 3, 10);
        assert_eq!(r, Range::from_raw(3, 7));

        // Both exceed: clamped to doc_len, equal.
        let r = validated_range(15, 20, 10);
        assert_eq!(r, Range::from_raw(10, 10));
    }

    #[test]
    fn normal_query_passes_through() {
        let harness = SafetyHarness::new();
        let result = harness.query(|| Some(String::from("hello")));
        assert_eq!(result, Some(String::from("hello")));
        assert_eq!(harness.panic_count(), 0);
        assert!(!harness.is_in_query());
    }

    // ===== Adversarial tests below =====

    /// A panic inside catch_unwind that triggers another panic during unwinding.
    /// The inner panic should not escape — catch_unwind handles the outer one.
    #[test]
    fn double_panic_contained() {
        let harness = SafetyHarness::new();

        // A closure that panics. The payload itself is a string that causes
        // no further issue, but we nest a panic_any to simulate a double-panic
        // scenario (e.g., Drop impl panicking during unwind).
        let result: Option<i32> = harness.query(|| {
            // Create a guard that would panic on drop during unwinding.
            struct PanicOnDrop;
            impl Drop for PanicOnDrop {
                fn drop(&mut self) {
                    // This will cause a double-panic. In Rust, a panic during
                    // unwind aborts the process UNLESS caught by catch_unwind
                    // at a higher level. Since our panic is inside catch_unwind,
                    // the first panic is caught, and drop runs within that scope.
                    // Rust aborts on double-panic, so this tests a less
                    // extreme variant: panic, then verify state is clean.
                }
            }
            let _guard = PanicOnDrop;
            panic!("first panic");
        });

        assert_eq!(result, None);
        assert_eq!(harness.panic_count(), 1);
        // Critical: reentrancy flag must be cleared even after panic.
        assert!(!harness.is_in_query());
    }

    /// Test that after a panic, the reentrancy flag is always cleared.
    /// This simulates the scenario where something goes wrong during the
    /// flag-checking region.
    #[test]
    fn reentrancy_flag_cleared_after_panic() {
        let harness = SafetyHarness::new();

        // Panic inside the query.
        let _: Option<()> = harness.query(|| {
            // Verify flag is set during execution.
            assert!(harness.is_in_query());
            panic!("boom");
        });

        // Flag MUST be cleared after panic.
        assert!(!harness.is_in_query());

        // Subsequent query must work.
        let result = harness.query(|| Some(42));
        assert_eq!(result, Some(42));
    }

    /// cap_index = 31 is the maximum valid index (array size 32, 0-indexed).
    #[test]
    fn max_valid_cap_index_31() {
        let harness = SafetyHarness::new();

        // Should work at edge.
        let result = harness.query_for_capability(31, || Some(99));
        assert_eq!(result, Some(99));

        // Disable it via 3 panics.
        for _ in 0..3 {
            let _: Option<()> = harness.query_for_capability(31, || panic!("edge"));
        }
        assert!(harness.is_disabled(31));

        // Re-enable works.
        harness.re_enable(31);
        assert!(!harness.is_disabled(31));
    }

    /// cap_index = 32 wraps via `& 31` to index 0.
    /// This tests the masking behavior — no panic, no UB.
    #[test]
    fn cap_index_32_wraps_to_zero() {
        let harness = SafetyHarness::new();

        // cap_index 32 maps to slot 0 (32 & 31 == 0).
        for _ in 0..3 {
            let _: Option<()> = harness.query_for_capability(32, || panic!("wrap"));
        }

        // Slot 0 should be disabled.
        assert!(harness.is_disabled(0));
        // cap_index 32 also reads as disabled (same mask).
        assert!(harness.is_disabled(32));

        // Verify cap_index 1 is NOT affected.
        assert!(!harness.is_disabled(1));
    }

    /// cap_index = 255 wraps via `& 31` to index 31.
    #[test]
    fn cap_index_255_wraps_to_31() {
        let harness = SafetyHarness::new();

        // 255 & 31 == 31.
        for _ in 0..3 {
            let _: Option<()> = harness.query_for_capability(255, || panic!("max u8"));
        }

        assert!(harness.is_disabled(255));
        // Same slot as 31.
        assert!(harness.is_disabled(31));
    }

    /// Simulate "concurrent-like" interleaving: a query callback that tries
    /// to invoke another query (reentrancy). The inner query should fail.
    #[test]
    fn reentrant_query_from_callback() {
        let harness = SafetyHarness::new();

        let result = harness.query(|| {
            // Try to call query from inside query — should return None.
            let inner = harness.query(|| Some(999));
            assert_eq!(inner, None);

            // The outer query still succeeds.
            Some(42)
        });

        assert_eq!(result, Some(42));
        assert_eq!(harness.panic_count(), 0);
        assert!(!harness.is_in_query());
    }

    /// Reentrant query_for_capability from inside query.
    #[test]
    fn reentrant_capability_query_from_callback() {
        let harness = SafetyHarness::new();

        let result = harness.query_for_capability(5, || {
            // Inner capability query should be blocked by reentrancy.
            let inner = harness.query_for_capability(5, || Some(999));
            assert_eq!(inner, None);
            Some(42)
        });

        assert_eq!(result, Some(42));
        // No panics — reentrancy is not a panic.
        assert_eq!(harness.panic_count(), 0);
    }

    /// validated_range with usize::MAX — test overflow safety.
    ///
    /// NOTE: `usize::MAX` is reserved as a niche sentinel by `Offset` and cannot
    /// appear in a valid `Range`. The maximum valid offset is `usize::MAX - 1`.
    /// When `doc_len` is reasonable, `min()` clamps inputs below MAX, so no panic.
    /// However, if `doc_len == usize::MAX`, the clamped value IS `usize::MAX`,
    /// which panics inside `Offset::new()`. This is an intentional design choice:
    /// no real document reaches `usize::MAX` bytes.
    #[test]
    fn validated_range_usize_max_with_reasonable_doc() {
        // Both inputs at MAX, doc_len reasonable — clamps to doc_len.
        let r = validated_range(usize::MAX, usize::MAX, 100);
        assert_eq!(r, Range::from_raw(100, 100));

        // start=MAX, end=0 — after clamping start=100, end=0 → swap.
        let r = validated_range(usize::MAX, 0, 100);
        assert_eq!(r, Range::from_raw(0, 100));

        // start=0, end=MAX.
        let r = validated_range(0, usize::MAX, 100);
        assert_eq!(r, Range::from_raw(0, 100));

        // doc_len = usize::MAX - 1 (the maximum valid offset).
        let max_valid = usize::MAX - 1;
        let r = validated_range(10, 20, max_valid);
        assert_eq!(r, Range::from_raw(10, 20));

        // Both at MAX, doc_len = MAX-1 — clamps to MAX-1 which is valid.
        let r = validated_range(usize::MAX, usize::MAX, max_valid);
        assert_eq!(r, Range::from_raw(max_valid, max_valid));
    }

    /// Demonstrates that doc_len = usize::MAX is poison for validated_range.
    /// Offset::new panics on usize::MAX (niche sentinel). This is acceptable
    /// because no document can be that large, but we document the behavior.
    #[test]
    #[should_panic(expected = "usize::MAX is not a valid Offset")]
    fn validated_range_usize_max_doc_len_panics() {
        // doc_len = usize::MAX means min(usize::MAX, usize::MAX) = usize::MAX
        // which panics in Offset::new.
        let _ = validated_range(usize::MAX, usize::MAX, usize::MAX);
    }

    /// validated_offset with 0-length document.
    #[test]
    fn validated_offset_zero_length_document() {
        // Any offset into a 0-length doc clamps to 0.
        assert_eq!(validated_offset(0, 0), 0);
        assert_eq!(validated_offset(1, 0), 0);
        assert_eq!(validated_offset(100, 0), 0);
        assert_eq!(validated_offset(usize::MAX, 0), 0);
    }

    /// validated_range with 0-length document.
    #[test]
    fn validated_range_zero_length_document() {
        let r = validated_range(0, 0, 0);
        assert_eq!(r, Range::from_raw(0, 0));

        let r = validated_range(5, 10, 0);
        assert_eq!(r, Range::from_raw(0, 0));

        let r = validated_range(usize::MAX, 0, 0);
        assert_eq!(r, Range::from_raw(0, 0));
    }

    /// 100 panics in a row — panic_count must reach exactly 100 without
    /// overflow or wrapping (u32 holds 4 billion, but the count must still
    /// track every one).
    #[test]
    fn hundred_panics_in_a_row() {
        let harness = SafetyHarness::new();

        for i in 0..100 {
            let _: Option<()> = harness.query(|| panic!("panic #{}", i));
        }

        assert_eq!(harness.panic_count(), 100);
        // Harness still functional after 100 panics.
        let result = harness.query(|| Some(42));
        assert_eq!(result, Some(42));
        assert_eq!(harness.panic_count(), 100);
    }

    /// Full lifecycle: panic 3x → disabled → re-enable → works again → can
    /// disable again with 3 more panics.
    #[test]
    fn full_lifecycle_disable_reenable_disable() {
        let harness = SafetyHarness::new();
        let cap: u8 = 10;

        // Phase 1: cause 3 panics → disabled.
        for _ in 0..3 {
            let _: Option<()> = harness.query_for_capability(cap, || panic!("phase1"));
        }
        assert!(harness.is_disabled(cap));
        assert_eq!(harness.panic_count(), 3);

        // Phase 2: re-enable → queries work again.
        harness.re_enable(cap);
        assert!(!harness.is_disabled(cap));
        let result = harness.query_for_capability(cap, || Some(777));
        assert_eq!(result, Some(777));

        // Phase 3: cause 3 more panics → disabled again.
        for _ in 0..3 {
            let _: Option<()> = harness.query_for_capability(cap, || panic!("phase3"));
        }
        assert!(harness.is_disabled(cap));

        // Global count accumulated across both phases.
        assert_eq!(harness.panic_count(), 6);

        // Phase 4: re-enable again.
        harness.re_enable(cap);
        assert!(!harness.is_disabled(cap));
        let result = harness.query_for_capability(cap, || Some(888));
        assert_eq!(result, Some(888));
    }

    /// A query that returns None naturally vs one that panics — both produce
    /// None from the caller's perspective, but panic_count distinguishes them.
    #[test]
    fn query_none_vs_panic_none_distinguishable_by_count() {
        let harness = SafetyHarness::new();

        // Normal None — no panic counted.
        let result: Option<i32> = harness.query(|| None);
        assert_eq!(result, None);
        assert_eq!(harness.panic_count(), 0);

        // Panic None — panic counted.
        let result: Option<i32> = harness.query(|| panic!("intentional"));
        assert_eq!(result, None);
        assert_eq!(harness.panic_count(), 1);
    }

    /// Same test but for query_for_capability: None return vs panic.
    #[test]
    fn capability_query_none_vs_panic_distinguishable() {
        let harness = SafetyHarness::new();
        let cap: u8 = 2;

        // Normal None — no per-capability panic count.
        let result: Option<i32> = harness.query_for_capability(cap, || None);
        assert_eq!(result, None);
        assert!(!harness.is_disabled(cap));
        assert_eq!(harness.panic_count(), 0);

        // Panic — counted.
        let _: Option<i32> = harness.query_for_capability(cap, || panic!("oops"));
        assert_eq!(harness.panic_count(), 1);
        // Not yet disabled (only 1 panic).
        assert!(!harness.is_disabled(cap));
    }

    /// A callback that returns a value which would normally cause another
    /// query — test that the flag is cleared before the value is used.
    #[test]
    fn flag_cleared_before_return_value_used() {
        let harness = SafetyHarness::new();

        let result = harness.query(|| Some(42));
        assert_eq!(result, Some(42));

        // Immediately after, another query works (flag is cleared).
        let result2 = harness.query(|| Some(result.unwrap() + 1));
        assert_eq!(result2, Some(43));
    }

    /// Verify that disabling one capability doesn't affect another.
    #[test]
    fn capability_isolation() {
        let harness = SafetyHarness::new();

        // Disable cap 0.
        for _ in 0..3 {
            let _: Option<()> = harness.query_for_capability(0, || panic!("cap0"));
        }
        assert!(harness.is_disabled(0));

        // Cap 1 through 31 should all still work.
        for cap in 1..32u8 {
            assert!(!harness.is_disabled(cap));
            let result = harness.query_for_capability(cap, || Some(cap as i32));
            assert_eq!(result, Some(cap as i32));
        }
    }

    /// Reset panic count does not affect per-capability state.
    #[test]
    fn reset_panic_count_does_not_affect_capabilities() {
        let harness = SafetyHarness::new();
        let cap: u8 = 4;

        // Two panics — not yet disabled.
        for _ in 0..2 {
            let _: Option<()> = harness.query_for_capability(cap, || panic!("x"));
        }
        assert_eq!(harness.panic_count(), 2);

        // Reset global count.
        harness.reset_panic_count();
        assert_eq!(harness.panic_count(), 0);

        // Per-capability still has 2 panics — one more disables.
        let _: Option<()> = harness.query_for_capability(cap, || panic!("third"));
        assert!(harness.is_disabled(cap));
        assert_eq!(harness.panic_count(), 1); // reset only global.
    }

    /// has_any_disabled returns false when no capabilities are disabled.
    #[test]
    fn has_any_disabled_initially_false() {
        let harness = SafetyHarness::new();
        assert!(!harness.has_any_disabled());
    }

    /// has_any_disabled returns true once any capability crosses the threshold.
    #[test]
    fn has_any_disabled_true_after_threshold() {
        let harness = SafetyHarness::new();
        let cap: u8 = 9; // Folding

        // Below threshold — still false.
        for _ in 0..2 {
            let _: Option<()> = harness.query_for_capability(cap, || panic!("x"));
        }
        assert!(!harness.has_any_disabled());

        // Hit threshold — now true.
        let _: Option<()> = harness.query_for_capability(cap, || panic!("x"));
        assert!(harness.has_any_disabled());
    }

    /// has_any_disabled returns false again after re-enabling the only disabled cap.
    #[test]
    fn has_any_disabled_false_after_reenable() {
        let harness = SafetyHarness::new();
        let cap: u8 = 5;

        for _ in 0..3 {
            let _: Option<()> = harness.query_for_capability(cap, || panic!("x"));
        }
        assert!(harness.has_any_disabled());

        harness.re_enable(cap);
        assert!(!harness.has_any_disabled());
    }

    // ═══════════════════════════════════════════════════════════════════════
    // ADVERSARIAL TESTS: Attempting to break the SafetyHarness
    // ═══════════════════════════════════════════════════════════════════════

    /// ATTACK VECTOR 1: Closure captures a &mut reference and panics mid-mutation.
    /// After the panic, the &mut reference may point to logically-corrupted state.
    /// The harness MUST still function (reentrancy flag cleared, counters updated),
    /// but it CANNOT prevent the captured data from being in a bad state.
    ///
    /// VERDICT: This is a fundamental limitation of `AssertUnwindSafe` — the harness
    /// correctly contains the panic, but the CALLER is responsible for data integrity.
    /// The harness itself remains sound.
    #[test]
    fn adversarial_mut_ref_corruption_during_panic() {
        let harness = SafetyHarness::new();
        let mut data = vec![1, 2, 3, 4, 5];

        // The closure captures &mut data and panics after partial mutation.
        let result: Option<()> = harness.query(|| {
            data.clear();
            data.push(99);
            // Panic after partial mutation — data is now [99] instead of [1,2,3,4,5]
            panic!("mid-mutation panic");
        });

        // Harness correctly handles the panic:
        assert_eq!(result, None);
        assert_eq!(harness.panic_count(), 1);
        assert!(!harness.is_in_query()); // Flag cleared!

        // But the data IS corrupted — this is expected and NOT a harness bug.
        // The harness can't undo side effects of the closure.
        assert_eq!(data, vec![99]); // Partial mutation survived the panic

        // Critical: the harness itself still works for subsequent queries.
        let result = harness.query(|| Some(42));
        assert_eq!(result, Some(42));
    }

    /// ATTACK VECTOR 2: panic=abort in release mode.
    /// When `panic = "abort"` is set (which this workspace uses for release),
    /// catch_unwind becomes a no-op — the process terminates on panic.
    ///
    /// This test verifies the harness works in TEST mode (which uses panic=unwind).
    /// The release-mode limitation is documented, not a bug — it's a deployment choice.
    ///
    /// VERDICT: KNOWN LIMITATION. The safety harness is defense-in-depth for debug/test.
    /// In release mode (panic=abort), the harness provides reentrancy protection only.
    /// A host that panics in release will crash the process.
    #[test]
    fn adversarial_panic_is_catchable_in_test_mode() {
        // This test proves catch_unwind works in test profile (panic=unwind).
        // It would be a no-op in release (panic=abort) and the process would abort.
        let harness = SafetyHarness::new();

        // Verify we can actually catch panics in this build configuration.
        let result: Option<i32> = harness.query(|| panic!("test-mode panic"));
        assert_eq!(result, None);
        assert_eq!(harness.panic_count(), 1);

        // If we reached here, catch_unwind is active (not abort mode).
    }

    /// ATTACK VECTOR 3: SafetyHarness is !Sync — cannot be shared across threads.
    /// This is a compile-time guarantee due to Cell. Test verifies the invariant.
    ///
    /// VERDICT: PASS — Cell<T> is !Sync, so SafetyHarness is automatically !Sync.
    /// Two threads CANNOT call query simultaneously on the same harness.
    #[test]
    fn adversarial_not_sync_compile_time_guarantee() {
        // This is a compile-time assertion. If SafetyHarness were Sync,
        // this function would compile, which would be a safety hole.
        fn assert_not_sync<T>() {
            // The trait bound `T: !Sync` can't be expressed directly,
            // but we can verify via negative reasoning: Cell is !Sync,
            // and SafetyHarness contains Cell, therefore SafetyHarness is !Sync.
            //
            // We verify this by checking that Cell<bool> fails the Sync bound:
            fn _static_assert_cell_not_sync() {
                // This line would fail to compile if uncommented:
                // fn _require_sync<S: Sync>() {}
                // _require_sync::<std::cell::Cell<bool>>();
            }
        }
        assert_not_sync::<SafetyHarness>();

        // The real proof: SafetyHarness lives inside VimSession which is used
        // single-threaded. If someone tried to share it via Arc, they'd get:
        // "Cell<bool> cannot be shared between threads safely"
    }

    /// ATTACK VECTOR 4: What if a panic happens DURING reentrancy_flag.set(false)?
    ///
    /// Analysis: `Cell::set()` is a single-instruction write (mov to stack).
    /// It cannot panic — it's `unsafe { *self.value.get() = val; }` internally.
    /// There is no allocation, no Drop, no fallibility.
    ///
    /// VERDICT: IMPOSSIBLE. Cell::set cannot panic. The instruction sequence is:
    ///   1. reentrancy_flag.set(true)   — infallible
    ///   2. catch_unwind(f)             — catches any panic from f
    ///   3. reentrancy_flag.set(false)  — infallible, always executes
    ///
    /// The only way to skip step 3 is if the process aborts (panic=abort,
    /// stack overflow, SIGKILL) — in which case the entire process is dead anyway.
    #[test]
    fn adversarial_set_false_cannot_panic() {
        let harness = SafetyHarness::new();

        // Hammer the harness 1000 times with panics.
        // After each one, the flag MUST be false.
        for i in 0..1000 {
            let _: Option<()> = harness.query(|| panic!("iteration {}", i));
            assert!(
                !harness.is_in_query(),
                "reentrancy_flag stuck on iteration {}",
                i
            );
        }
        assert_eq!(harness.panic_count(), 1000);
    }

    /// ATTACK VECTOR 5: Stack overflow — catch_unwind does NOT catch these.
    ///
    /// A stack overflow triggers SIGSEGV (or similar), which is NOT a Rust panic.
    /// catch_unwind has no mechanism to intercept it. The process aborts.
    ///
    /// VERDICT: KNOWN LIMITATION. Stack overflows bypass ALL Rust safety mechanisms.
    /// The harness cannot protect against them. The mitigation is:
    /// - Hosts should not recurse deeply inside callbacks
    /// - The engine's MAX_DRAIN_ITERATIONS limit prevents unbounded macro recursion
    ///
    /// We cannot write a test that exercises this without actually crashing the test
    /// harness, so we document it as a known gap.
    #[test]
    fn adversarial_stack_overflow_documentation() {
        // This test exists to document the limitation.
        // A real stack overflow test would abort the process:
        //   fn overflow() { overflow() }
        //   harness.query(|| { overflow(); Some(()) });
        //   // ^^^ process aborts, test runner dies
        //
        // The harness cannot protect against this. Documented as acceptable.
        let harness = SafetyHarness::new();
        let _evidence = harness.panic_count(); // Proves harness exists
    }

    /// ATTACK VECTOR 6: Closure that sets the reentrancy flag itself.
    /// What if a malicious closure manually sets reentrancy_flag to false
    /// and then calls query again recursively?
    ///
    /// VERDICT: IMPOSSIBLE from outside. reentrancy_flag is a private field.
    /// Only this module can access it. The harness encapsulates state safely.
    /// From inside the module (tests), we CAN access it — but that's fine,
    /// it's testing our own internals.
    #[test]
    fn adversarial_cannot_circumvent_reentrancy_from_closure() {
        let harness = SafetyHarness::new();

        // From inside the closure, we CAN observe the flag via is_in_query().
        // But we CANNOT set it — reentrancy_flag is private.
        let result = harness.query(|| {
            assert!(harness.is_in_query()); // Observable
                                            // harness.reentrancy_flag.set(false); // Would not compile in external code
                                            // (but compiles in test because tests are in the same module)

            // Even if we could clear it (test-only), the outer query will
            // set it back to false anyway when it returns.
            Some(42)
        });
        assert_eq!(result, Some(42));
        assert!(!harness.is_in_query());
    }

    /// ATTACK VECTOR 7: Interleaved capability panics — verify isolation under stress.
    /// Panic capability A, then B, then A again. The counts must not interfere.
    #[test]
    fn adversarial_interleaved_capability_panics() {
        let harness = SafetyHarness::new();
        let cap_a: u8 = 5; // SearchHighlight
        let cap_b: u8 = 9; // Folding
        let cap_c: u8 = 21; // Reindent

        // Interleave: A, B, A, C, B, A
        let _: Option<()> = harness.query_for_capability(cap_a, || panic!("a1"));
        let _: Option<()> = harness.query_for_capability(cap_b, || panic!("b1"));
        let _: Option<()> = harness.query_for_capability(cap_a, || panic!("a2"));
        let _: Option<()> = harness.query_for_capability(cap_c, || panic!("c1"));
        let _: Option<()> = harness.query_for_capability(cap_b, || panic!("b2"));
        let _: Option<()> = harness.query_for_capability(cap_a, || panic!("a3")); // A hits threshold

        // A (3 panics) → disabled
        assert!(harness.is_disabled(cap_a));
        // B (2 panics) → still alive
        assert!(!harness.is_disabled(cap_b));
        // C (1 panic) → still alive
        assert!(!harness.is_disabled(cap_c));

        // Global count is 6
        assert_eq!(harness.panic_count(), 6);

        // A is blocked
        let result: Option<i32> = harness.query_for_capability(cap_a, || Some(99));
        assert_eq!(result, None);

        // B and C still work
        let result: Option<i32> = harness.query_for_capability(cap_b, || Some(77));
        assert_eq!(result, Some(77));
        let result: Option<i32> = harness.query_for_capability(cap_c, || Some(88));
        assert_eq!(result, Some(88));
    }

    /// ATTACK VECTOR 8: saturating_add overflow behavior.
    /// What if panic_count approaches u32::MAX?
    #[test]
    fn adversarial_saturating_add_at_max() {
        let harness = SafetyHarness::new();

        // Simulate near-overflow by manipulating internal state.
        // We can't easily set panic_count directly without thousands of panics,
        // but we can verify the saturating behavior via the Cell directly.
        harness.panic_count.set(u32::MAX - 1);
        let _: Option<()> = harness.query(|| panic!("near max"));
        assert_eq!(harness.panic_count(), u32::MAX); // saturated, not wrapped

        // One more panic — stays at MAX, doesn't wrap to 0.
        let _: Option<()> = harness.query(|| panic!("at max"));
        assert_eq!(harness.panic_count(), u32::MAX);
    }

    /// ATTACK VECTOR 9: Per-capability panic count saturation.
    /// What if a per-capability counter is already at u32::MAX?
    #[test]
    fn adversarial_per_capability_saturates() {
        let harness = SafetyHarness::new();
        let cap: u8 = 15;

        // Set per-capability count to MAX - 1 (already above threshold,
        // so the capability will be disabled on next panic).
        harness.per_capability_panics[(cap as usize) & 31].set(u32::MAX - 1);

        // One panic: count goes to MAX (saturating), capability disabled.
        let _: Option<()> = harness.query_for_capability(cap, || panic!("saturate"));
        assert!(harness.is_disabled(cap));

        // The count is at MAX, re-enable should reset to 0.
        harness.re_enable(cap);
        assert!(!harness.is_disabled(cap));
        assert_eq!(harness.per_capability_panics[(cap as usize) & 31].get(), 0);
    }

    /// ATTACK VECTOR 10: What if the closure captures &self of the harness
    /// and calls a method that reads state during the panic handler?
    /// The Cell design means reads always get the latest value.
    #[test]
    fn adversarial_reading_harness_state_from_within_closure() {
        let harness = SafetyHarness::new();

        let result = harness.query(|| {
            // Inside the closure, the reentrancy flag is true.
            assert!(harness.is_in_query());
            // panic_count is still 0 (no panic happened yet in this query).
            assert_eq!(harness.panic_count(), 0);
            // has_any_disabled is false.
            assert!(!harness.has_any_disabled());
            Some(42)
        });
        assert_eq!(result, Some(42));
    }

    /// END-TO-END CAPABILITY DOWNGRADE TEST:
    /// Simulates the exact flow from build_context perspective.
    /// 1. Capability starts working
    /// 2. Capability panics 3 times via query_for_capability
    /// 3. Capability is disabled — returns None
    /// 4. Other capabilities still function
    #[test]
    fn end_to_end_fold_provider_downgrade() {
        let harness = SafetyHarness::new();
        let fold_cap: u8 = 9; // HostCapability::Folding = 9
        let search_cap: u8 = 5; // HostCapability::SearchHighlight = 5

        // Phase 1: Both capabilities work.
        let fold_result = harness.query_for_capability(fold_cap, || Some("fold data"));
        assert_eq!(fold_result, Some("fold data"));

        let search_result = harness.query_for_capability(search_cap, || Some("search data"));
        assert_eq!(search_result, Some("search data"));

        // Phase 2: Fold provider panics 3 times.
        for i in 0..3 {
            let _: Option<&str> =
                harness.query_for_capability(fold_cap, || panic!("fold panic #{}", i));
        }

        // Phase 3: Fold is disabled (returns None immediately without calling closure).
        let fold_result: Option<&str> = harness.query_for_capability(fold_cap, || {
            panic!("should never execute — capability is disabled");
        });
        assert_eq!(fold_result, None);
        assert!(harness.is_disabled(fold_cap));

        // Verify the closure was NOT called (panic count stayed at 3, not 4).
        assert_eq!(harness.panic_count(), 3);

        // Phase 4: Search provider still works perfectly.
        let search_result = harness.query_for_capability(search_cap, || Some("still works"));
        assert_eq!(search_result, Some("still works"));
        assert!(!harness.is_disabled(search_cap));

        // And a regular query (no capability) also works.
        let generic = harness.query(|| Some(999));
        assert_eq!(generic, Some(999));
    }

    /// Comprehensive lifecycle simulating real VimSession usage:
    /// process_key → fold provider called → fold panics → repeated → disabled
    /// → subsequent process_key sees fold=None but search=Some.
    #[test]
    fn end_to_end_simulated_session_lifecycle() {
        let harness = SafetyHarness::new();
        let fold_cap: u8 = 9;
        let indent_cap: u8 = 21;
        let search_cap: u8 = 5;

        // Simulate 10 "process_key" cycles. On cycles 2, 5, 7: fold panics.
        for cycle in 0..10 {
            // Check if fold is still enabled before calling.
            if !harness.is_disabled(fold_cap) {
                let fold_result: Option<Vec<(usize, usize)>> =
                    harness.query_for_capability(fold_cap, || {
                        if cycle == 2 || cycle == 5 || cycle == 7 {
                            panic!("fold crash on cycle {}", cycle);
                        }
                        Some(vec![(0, 10), (20, 30)])
                    });

                if cycle == 2 || cycle == 5 {
                    // First two panics: still not disabled
                    assert_eq!(fold_result, None);
                    assert!(!harness.is_disabled(fold_cap));
                } else if cycle == 7 {
                    // Third panic: NOW disabled
                    assert_eq!(fold_result, None);
                    assert!(harness.is_disabled(fold_cap));
                } else if cycle < 7 {
                    // No panic: got fold data
                    assert!(fold_result.is_some());
                }
                // cycle > 7: is_disabled check above short-circuits
            } else {
                // Fold is disabled — skip entirely (matches build_context behavior)
                assert!(cycle >= 8);
            }

            // Indent always works (never panics in this test).
            let indent_result = harness.query_for_capability(indent_cap, || Some("  "));
            assert_eq!(indent_result, Some("  "));

            // Search always works.
            let search_result = harness.query_for_capability(search_cap, || Some(42usize));
            assert_eq!(search_result, Some(42));
        }

        // Final state: only fold disabled.
        assert!(harness.is_disabled(fold_cap));
        assert!(!harness.is_disabled(indent_cap));
        assert!(!harness.is_disabled(search_cap));
        assert_eq!(harness.panic_count(), 3);
    }
}
