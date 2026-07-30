//! Position types for vim-core.
//!
//! These are newtype wrappers providing type safety for positions.
//! Prevents mixing byte offsets with line numbers at compile time.
//!
//! All arithmetic is saturating — no panics on overflow/underflow.

use derive_more::{Display, From, Into};
use nonmax::NonMaxUsize;

/// Generate the common newtype methods shared by all position newtypes.
///
/// Each position newtype (`LineNumber`, `Column`) wraps a `usize` and
/// needs the same core set of constructor, accessor, and saturating-arithmetic
/// methods. This macro eliminates that boilerplate while keeping the generated
/// API identical to the hand-written versions it replaces.
///
/// Note: `Offset` does NOT use this macro — it wraps `NonMaxUsize` for niche
/// optimization (`Option<Offset>` = 8 bytes instead of 16).
macro_rules! impl_newtype_methods {
    ($T:ident) => {
        impl $T {
            /// Create a new value.
            #[inline]
            #[must_use]
            pub const fn new(value: usize) -> Self {
                Self(value)
            }

            /// Get the raw value.
            #[inline]
            #[must_use]
            pub const fn get(self) -> usize {
                self.0
            }

            /// Saturating addition.
            #[inline]
            #[must_use]
            pub const fn saturating_add(self, other: Self) -> Self {
                Self(self.0.saturating_add(other.0))
            }

            /// Saturating subtraction.
            #[inline]
            #[must_use]
            pub const fn saturating_sub(self, other: Self) -> Self {
                Self(self.0.saturating_sub(other.0))
            }

            /// Return the smaller of two values.
            #[inline]
            #[must_use]
            pub const fn min(self, other: Self) -> Self {
                if self.0 <= other.0 {
                    self
                } else {
                    other
                }
            }

            /// Return the larger of two values.
            #[inline]
            #[must_use]
            pub const fn max(self, other: Self) -> Self {
                if self.0 >= other.0 {
                    self
                } else {
                    other
                }
            }
        }
    };
}

/// Maximum valid value for `Offset` — `usize::MAX - 1`.
///
/// `usize::MAX` is reserved as the niche sentinel so `Option<Offset>` fits in
/// 8 bytes on 64-bit targets.
const OFFSET_MAX: usize = usize::MAX - 1;

/// Byte offset between characters in a document (gap indexing).
///
/// Points *between* characters — i.e., at the gap before or after a character.
/// This makes selection ranges naturally half-open `[start, end)` without
/// off-by-one ambiguity.
///
/// # Niche optimization
///
/// Wraps [`NonMaxUsize`] so that `Option<Offset>` is 8 bytes on 64-bit
/// (instead of 16). `usize::MAX` is the niche value and cannot be stored;
/// the maximum representable offset is `usize::MAX - 1`.
///
/// All arithmetic is **saturating**: overflow clamps to `OFFSET_MAX`,
/// underflow clamps to `0`. This matches Vim semantics where positions
/// are always non-negative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Offset(NonMaxUsize);

// Manual serde: serialize/deserialize as a plain `usize` for wire compatibility.
// `NonMaxUsize` doesn't implement serde traits, so we go through the raw value.
#[cfg(feature = "serde")]
impl serde::Serialize for Offset {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.get().serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Offset {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = usize::deserialize(deserializer)?;
        NonMaxUsize::new(value)
            .map(Self)
            .ok_or_else(|| serde::de::Error::custom("usize::MAX is not a valid Offset"))
    }
}

impl Offset {
    /// Zero offset.
    // SAFETY: 0 != usize::MAX, so `NonMaxUsize::new(0)` always succeeds.
    pub const ZERO: Self = Self(NonMaxUsize::ZERO);

    /// Maximum representable offset (`usize::MAX - 1`).
    ///
    /// `usize::MAX` is reserved as the niche sentinel for `Option<Offset>`.
    // SAFETY: usize::MAX - 1 != usize::MAX.
    pub const MAX: Self = Self(NonMaxUsize::MAX);

    /// Create a new offset.
    ///
    /// # Panics
    ///
    /// Panics if `value == usize::MAX` (reserved as the niche sentinel for
    /// `Option<Offset>`). In practice, no document ever reaches `usize::MAX`
    /// bytes so this is unreachable in normal use.
    #[inline]
    #[must_use]
    pub const fn new(value: usize) -> Self {
        match NonMaxUsize::new(value) {
            Some(v) => Self(v),
            #[expect(
                clippy::panic,
                reason = "no document reaches usize::MAX bytes; the sentinel value cannot occur in practice and indicates a memory-corruption bug if it does"
            )]
            None => panic!("usize::MAX is not a valid Offset (reserved as niche sentinel)"),
        }
    }

    /// Get the raw `usize` value.
    #[inline]
    #[must_use]
    pub const fn get(self) -> usize {
        self.0.get()
    }

    /// Next gap position (saturating at `OFFSET_MAX`).
    #[inline]
    #[must_use]
    pub const fn next(self) -> Self {
        Self::saturating_new(self.0.get().saturating_add(1))
    }

    /// Previous gap position (saturating at 0).
    #[inline]
    #[must_use]
    pub const fn prev(self) -> Self {
        Self::saturating_new(self.0.get().saturating_sub(1))
    }

    /// Saturating addition.
    #[inline]
    #[must_use]
    pub const fn saturating_add(self, other: Self) -> Self {
        Self::saturating_new(self.0.get().saturating_add(other.0.get()))
    }

    /// Saturating subtraction.
    #[inline]
    #[must_use]
    pub const fn saturating_sub(self, other: Self) -> Self {
        Self::saturating_new(self.0.get().saturating_sub(other.0.get()))
    }

    /// Saturating addition with a raw usize.
    #[inline]
    #[must_use]
    pub const fn saturating_add_raw(self, n: usize) -> Self {
        Self::saturating_new(self.0.get().saturating_add(n))
    }

    /// Saturating subtraction with a raw usize.
    #[inline]
    #[must_use]
    pub const fn saturating_sub_raw(self, n: usize) -> Self {
        Self::saturating_new(self.0.get().saturating_sub(n))
    }

    /// Absolute distance between two offsets.
    #[inline]
    #[must_use]
    pub const fn distance(self, other: Self) -> usize {
        self.0.get().abs_diff(other.0.get())
    }

    /// Return the smaller of two values.
    #[inline]
    #[must_use]
    pub const fn min(self, other: Self) -> Self {
        if self.0.get() <= other.0.get() {
            self
        } else {
            other
        }
    }

    /// Return the larger of two values.
    #[inline]
    #[must_use]
    pub const fn max(self, other: Self) -> Self {
        if self.0.get() >= other.0.get() {
            self
        } else {
            other
        }
    }

    /// Construct an `Offset`, clamping `usize::MAX` to `OFFSET_MAX`.
    ///
    /// This is the workhorse behind all saturating arithmetic: the raw `usize`
    /// result of `saturating_add` / `saturating_sub` may be `usize::MAX`, which
    /// is not representable. We clamp it down by one.
    #[inline]
    const fn saturating_new(value: usize) -> Self {
        // Branchless clamp: if value == usize::MAX, use usize::MAX - 1.
        let clamped = if value == usize::MAX {
            OFFSET_MAX
        } else {
            value
        };
        // SAFETY: clamped is at most usize::MAX - 1, so NonMaxUsize::new succeeds.
        match NonMaxUsize::new(clamped) {
            Some(v) => Self(v),
            None => unreachable!(),
        }
    }
}

impl Default for Offset {
    #[inline]
    fn default() -> Self {
        Self::ZERO
    }
}

impl std::fmt::Display for Offset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.get())
    }
}

impl std::ops::Add for Offset {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self {
        self.saturating_add(rhs)
    }
}

impl std::ops::Sub for Offset {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self {
        self.saturating_sub(rhs)
    }
}

impl std::ops::AddAssign for Offset {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        *self = self.saturating_add(rhs);
    }
}

impl std::ops::SubAssign for Offset {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        *self = self.saturating_sub(rhs);
    }
}

/// 0-indexed line number.
///
/// Line 0 is the first line of the document.
///
/// All arithmetic is **saturating**.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, From, Into, Display,
)]
#[display(fmt = "{_0}")]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct LineNumber(usize);

impl_newtype_methods!(LineNumber);

impl LineNumber {
    /// Next line (saturating).
    #[inline]
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }

    /// Previous line (saturating at 0).
    #[inline]
    #[must_use]
    pub const fn prev(self) -> Self {
        Self(self.0.saturating_sub(1))
    }
}

/// 0-indexed column offset.
///
/// Column 0 is the first character of a line.
///
/// All arithmetic is **saturating**.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, From, Into, Display,
)]
#[display(fmt = "{_0}")]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct Column(usize);

impl_newtype_methods!(Column);

/// A position in a document (line + column).
///
/// This is a higher-level representation than byte offset.
/// Conversion to/from Offset requires a Document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Display)]
#[display(fmt = "{}:{}", "line.get()", "col.get()")]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Position {
    /// 0-indexed line number.
    line: LineNumber,
    /// 0-indexed column.
    col: Column,
}

impl Position {
    /// Create a new position.
    #[inline]
    #[must_use]
    pub const fn new(line: LineNumber, col: Column) -> Self {
        Self { line, col }
    }

    /// Create from raw values.
    #[inline]
    #[must_use]
    pub const fn from_raw(line: usize, col: usize) -> Self {
        Self {
            line: LineNumber(line),
            col: Column(col),
        }
    }

    /// Origin (0, 0).
    pub const ORIGIN: Self = Self {
        line: LineNumber(0),
        col: Column(0),
    };

    /// Get the line number.
    #[inline]
    #[must_use]
    pub const fn line(self) -> LineNumber {
        self.line
    }

    /// Get the column.
    #[inline]
    #[must_use]
    pub const fn col(self) -> Column {
        self.col
    }
}

impl PartialOrd for Position {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Position {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let pack = |p: &Position| (p.line.get() as u64) << 32 | p.col.get() as u64;
        debug_assert!(
            self.line.get() <= u32::MAX as usize && self.col.get() <= u32::MAX as usize,
            "Position fields must fit in u32 for packed comparison"
        );
        debug_assert!(
            other.line.get() <= u32::MAX as usize && other.col.get() <= u32::MAX as usize,
            "Position fields must fit in u32 for packed comparison"
        );
        pack(self).cmp(&pack(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === Offset ===

    #[test]
    fn offset_new_and_get() {
        assert_eq!(Offset::new(42).get(), 42);
    }

    #[test]
    fn offset_default_is_zero() {
        assert_eq!(Offset::default().get(), 0);
        assert_eq!(Offset::default(), Offset::ZERO);
    }

    #[test]
    fn offset_zero_constant() {
        assert_eq!(Offset::ZERO.get(), 0);
    }

    #[test]
    fn offset_add_sub_saturating() {
        assert_eq!(Offset::new(10) + Offset::new(5), Offset::new(15));
        // Saturates at Offset::MAX (usize::MAX - 1), not usize::MAX
        assert_eq!(Offset::MAX + Offset::new(1), Offset::MAX);
        assert_eq!(Offset::new(10) - Offset::new(3), Offset::new(7));
        assert_eq!(Offset::new(3) - Offset::new(5), Offset::ZERO);
    }

    #[test]
    fn offset_add_assign_saturating() {
        let mut o = Offset::new(5);
        o += Offset::new(3);
        assert_eq!(o, Offset::new(8));
        let mut o = Offset::MAX;
        o += Offset::new(1);
        assert_eq!(o, Offset::MAX);
    }

    #[test]
    fn offset_sub_assign_saturating() {
        let mut o = Offset::new(5);
        o -= Offset::new(3);
        assert_eq!(o, Offset::new(2));
        let mut o = Offset::new(3);
        o -= Offset::new(5);
        assert_eq!(o, Offset::ZERO);
    }

    #[test]
    fn offset_next_prev() {
        assert_eq!(Offset::new(5).next(), Offset::new(6));
        assert_eq!(Offset::new(5).prev(), Offset::new(4));
        assert_eq!(Offset::ZERO.prev(), Offset::ZERO);
        // Saturates at Offset::MAX (usize::MAX - 1)
        assert_eq!(Offset::MAX.next(), Offset::MAX);
    }

    #[test]
    fn offset_distance() {
        assert_eq!(Offset::new(10).distance(Offset::new(3)), 7);
        assert_eq!(Offset::new(3).distance(Offset::new(10)), 7);
        assert_eq!(Offset::new(5).distance(Offset::new(5)), 0);
    }

    #[test]
    fn offset_saturating_add_sub_typed() {
        assert_eq!(
            Offset::new(5).saturating_add(Offset::new(3)),
            Offset::new(8)
        );
        // Saturates at Offset::MAX (usize::MAX - 1)
        assert_eq!(Offset::MAX.saturating_add(Offset::new(1)), Offset::MAX);
        assert_eq!(
            Offset::new(5).saturating_sub(Offset::new(3)),
            Offset::new(2)
        );
        assert_eq!(
            Offset::new(3).saturating_sub(Offset::new(5)),
            Offset::new(0)
        );
    }

    #[test]
    fn offset_saturating_add_sub_raw() {
        assert_eq!(Offset::new(5).saturating_add_raw(3), Offset::new(8));
        // Saturates at Offset::MAX (usize::MAX - 1)
        assert_eq!(Offset::MAX.saturating_add_raw(1), Offset::MAX);
        assert_eq!(Offset::new(5).saturating_sub_raw(3), Offset::new(2));
        assert_eq!(Offset::new(3).saturating_sub_raw(5), Offset::ZERO);
    }

    #[test]
    fn offset_max_constant() {
        assert_eq!(Offset::MAX.get(), usize::MAX - 1);
    }

    #[test]
    #[should_panic(expected = "usize::MAX is not a valid Offset")]
    fn offset_new_usize_max_panics() {
        let _ = Offset::new(usize::MAX);
    }

    #[test]
    fn offset_min_max() {
        assert_eq!(Offset::new(5).min(Offset::new(3)), Offset::new(3));
        assert_eq!(Offset::new(5).max(Offset::new(3)), Offset::new(5));
    }

    #[test]
    fn offset_ordering() {
        assert!(Offset::new(5) > Offset::new(3));
        assert!(Offset::new(3) < Offset::new(5));
        assert!(Offset::new(5) == Offset::new(5));
    }

    // === LineNumber ===

    #[test]
    fn line_number_new_and_get() {
        assert_eq!(LineNumber::new(10).get(), 10);
    }

    #[test]
    fn line_number_next() {
        assert_eq!(LineNumber::new(5).next(), LineNumber::new(6));
    }

    #[test]
    fn line_number_prev() {
        assert_eq!(LineNumber::new(5).prev(), LineNumber::new(4));
    }

    #[test]
    fn line_number_prev_saturates_at_zero() {
        assert_eq!(LineNumber::new(0).prev(), LineNumber::new(0));
    }

    #[test]
    fn line_number_saturating_sub() {
        assert_eq!(
            LineNumber::new(5).saturating_sub(LineNumber::new(3)),
            LineNumber::new(2)
        );
        assert_eq!(
            LineNumber::new(3).saturating_sub(LineNumber::new(5)),
            LineNumber::new(0)
        );
    }

    #[test]
    fn line_number_saturating_add() {
        assert_eq!(
            LineNumber::new(5).saturating_add(LineNumber::new(3)),
            LineNumber::new(8)
        );
    }

    #[test]
    fn line_number_min_max() {
        assert_eq!(
            LineNumber::new(5).min(LineNumber::new(3)),
            LineNumber::new(3)
        );
        assert_eq!(
            LineNumber::new(5).max(LineNumber::new(3)),
            LineNumber::new(5)
        );
    }

    // === Column ===

    #[test]
    fn column_new_and_get() {
        assert_eq!(Column::new(7).get(), 7);
    }

    #[test]
    fn column_saturating_sub() {
        assert_eq!(
            Column::new(5).saturating_sub(Column::new(3)),
            Column::new(2)
        );
        assert_eq!(
            Column::new(3).saturating_sub(Column::new(5)),
            Column::new(0)
        );
    }

    #[test]
    fn column_saturating_add() {
        assert_eq!(
            Column::new(5).saturating_add(Column::new(3)),
            Column::new(8)
        );
    }

    #[test]
    fn column_min_max() {
        assert_eq!(Column::new(5).min(Column::new(3)), Column::new(3));
        assert_eq!(Column::new(5).max(Column::new(3)), Column::new(5));
    }

    // === Position ===

    #[test]
    fn position_from_raw() {
        let pos = Position::from_raw(3, 7);
        assert_eq!(pos.line, LineNumber::new(3));
        assert_eq!(pos.col, Column::new(7));
    }

    #[test]
    fn position_origin() {
        assert_eq!(Position::ORIGIN, Position::from_raw(0, 0));
    }

    #[test]
    fn position_ordering_by_line() {
        let a = Position::from_raw(1, 5);
        let b = Position::from_raw(2, 0);
        assert!(a < b);
    }

    #[test]
    fn position_ordering_same_line_by_column() {
        let a = Position::from_raw(1, 3);
        let b = Position::from_raw(1, 7);
        assert!(a < b);
    }

    #[test]
    fn position_equality() {
        assert_eq!(Position::from_raw(3, 5), Position::from_raw(3, 5));
        assert_ne!(Position::from_raw(3, 5), Position::from_raw(3, 6));
    }

    #[test]
    fn position_ordering_same_line_different_col() {
        let a = Position::from_raw(5, 10);
        let b = Position::from_raw(5, 20);
        assert!(a < b);
        assert!(b > a);
        assert_eq!(a.cmp(&b), std::cmp::Ordering::Less);
        assert_eq!(b.cmp(&a), std::cmp::Ordering::Greater);
    }

    #[test]
    fn position_ordering_same_col_different_line() {
        let a = Position::from_raw(3, 10);
        let b = Position::from_raw(7, 10);
        assert!(a < b);
        assert!(b > a);
        assert_eq!(a.cmp(&b), std::cmp::Ordering::Less);
        assert_eq!(b.cmp(&a), std::cmp::Ordering::Greater);
    }

    #[test]
    fn position_ordering_equal() {
        let a = Position::from_raw(4, 8);
        let b = Position::from_raw(4, 8);
        assert_eq!(a.cmp(&b), std::cmp::Ordering::Equal);
        assert_eq!(a.partial_cmp(&b), Some(std::cmp::Ordering::Equal));
        assert!(!(a < b));
        assert!(!(a > b));
    }

    #[test]
    fn position_ordering_near_u32_boundary() {
        let max32 = u32::MAX as usize;

        // Both line and col near u32::MAX
        let a = Position::from_raw(max32 - 1, max32 - 1);
        let b = Position::from_raw(max32, 0);
        assert!(a < b);

        // Same line at u32::MAX, different cols
        let c = Position::from_raw(max32, 100);
        let d = Position::from_raw(max32, 200);
        assert!(c < d);

        // Line at u32::MAX, col at u32::MAX — equal to itself
        let e = Position::from_raw(max32, max32);
        assert_eq!(e.cmp(&e), std::cmp::Ordering::Equal);

        // Large line beats large col on smaller line
        let f = Position::from_raw(1, max32);
        let g = Position::from_raw(2, 0);
        assert!(f < g);
    }

    /// Line always dominates: max line + min col > min line + max col.
    #[test]
    fn position_line_dominates_col() {
        let max32 = u32::MAX as usize;
        // Maximum line with minimum column must be greater than
        // minimum line with maximum column.
        let high_line = Position::from_raw(max32, 0);
        let high_col = Position::from_raw(0, max32);
        assert!(high_line > high_col);
        assert_eq!(high_line.cmp(&high_col), std::cmp::Ordering::Greater);
        assert_eq!(high_col.cmp(&high_line), std::cmp::Ordering::Less);

        // Also check with smaller but still distinct values.
        let a = Position::from_raw(1, 0);
        let b = Position::from_raw(0, usize::MAX & (u32::MAX as usize));
        assert!(a > b, "line=1,col=0 must beat line=0,col=max");
    }

    /// Position (0,0) is the minimum possible position.
    #[test]
    fn position_origin_is_minimum() {
        let origin = Position::ORIGIN;
        // Origin equals itself.
        assert_eq!(origin.cmp(&origin), std::cmp::Ordering::Equal);
        assert_eq!(origin.partial_cmp(&origin), Some(std::cmp::Ordering::Equal));

        // Origin is less than any non-zero position.
        assert!(origin < Position::from_raw(0, 1));
        assert!(origin < Position::from_raw(1, 0));
        assert!(origin < Position::from_raw(1, 1));
        assert!(origin < Position::from_raw(u32::MAX as usize, u32::MAX as usize));
    }

    /// Large column values: ordering correctness for positions with big cols.
    #[test]
    fn position_ordering_large_col_values() {
        let max32 = u32::MAX as usize;

        // Same line, large cols — column ordering still correct.
        let a = Position::from_raw(100, max32 - 1);
        let b = Position::from_raw(100, max32);
        assert!(a < b);

        // Different lines, both with large cols.
        let c = Position::from_raw(50, max32);
        let d = Position::from_raw(51, 0);
        assert!(
            c < d,
            "next line with col=0 beats previous line with col=max"
        );

        // Both at large col, different lines — line still dominates.
        let e = Position::from_raw(1000, max32);
        let f = Position::from_raw(1001, max32);
        assert!(e < f);
    }

    /// PartialOrd must be consistent with Ord (derived from cmp).
    #[test]
    fn position_partial_ord_consistency() {
        let positions = [
            Position::ORIGIN,
            Position::from_raw(0, 1),
            Position::from_raw(0, u32::MAX as usize),
            Position::from_raw(1, 0),
            Position::from_raw(1, 1),
            Position::from_raw(100, 50),
            Position::from_raw(100, 51),
            Position::from_raw(u32::MAX as usize, 0),
            Position::from_raw(u32::MAX as usize, u32::MAX as usize),
        ];

        for (i, a) in positions.iter().enumerate() {
            for (j, b) in positions.iter().enumerate() {
                let ord = a.cmp(b);
                let partial = a.partial_cmp(b);
                // PartialOrd must return Some(cmp result).
                assert_eq!(
                    partial,
                    Some(ord),
                    "PartialOrd inconsistent with Ord at ({i}, {j})"
                );
                // Verify transitivity: if i < j in our sorted array, a < b.
                if i < j {
                    assert_eq!(
                        ord,
                        std::cmp::Ordering::Less,
                        "Expected positions[{i}] < positions[{j}]"
                    );
                } else if i == j {
                    assert_eq!(
                        ord,
                        std::cmp::Ordering::Equal,
                        "Expected positions[{i}] == positions[{j}]"
                    );
                } else {
                    assert_eq!(
                        ord,
                        std::cmp::Ordering::Greater,
                        "Expected positions[{i}] > positions[{j}]"
                    );
                }
            }
        }
    }

    /// The packed comparison must not overflow u64 when both fields are at u32::MAX.
    #[test]
    fn position_packed_no_overflow_at_max() {
        let max32 = u32::MAX as usize;
        let pos = Position::from_raw(max32, max32);
        // The packed value should be: (0xFFFF_FFFF << 32) | 0xFFFF_FFFF = 0xFFFF_FFFF_FFFF_FFFF = u64::MAX.
        // This must not wrap or panic.
        assert_eq!(pos.cmp(&pos), std::cmp::Ordering::Equal);

        // A position with max line but col=max-1 is less than max line + max col.
        let almost = Position::from_raw(max32, max32 - 1);
        assert!(almost < pos);
    }

    /// On 64-bit targets, usize > u32::MAX triggers the debug_assert.
    /// This test verifies the assertion fires (debug builds only).
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Position fields must fit in u32 for packed comparison")]
    fn position_overflow_debug_assert_line() {
        let overflow_line = Position::from_raw((u32::MAX as usize) + 1, 0);
        let _ = overflow_line.cmp(&Position::ORIGIN);
    }

    /// Column overflow also triggers the debug_assert.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Position fields must fit in u32 for packed comparison")]
    fn position_overflow_debug_assert_col() {
        let overflow_col = Position::from_raw(0, (u32::MAX as usize) + 1);
        let _ = overflow_col.cmp(&Position::ORIGIN);
    }

    /// Naive two-field comparison for cross-checking the packed version.
    fn naive_cmp(a: &Position, b: &Position) -> std::cmp::Ordering {
        match a.line.get().cmp(&b.line.get()) {
            std::cmp::Ordering::Equal => a.col.get().cmp(&b.col.get()),
            other => other,
        }
    }

    /// Property-based: packed comparison matches naive two-field comparison
    /// across a deterministic pseudo-random sweep of position pairs.
    /// Covers all three outcomes (Less, Equal, Greater).
    #[test]
    fn position_packed_matches_naive_random_sweep() {
        // Simple LCG PRNG (deterministic, no external dep).
        let mut rng_state: u64 = 0xDEAD_BEEF_CAFE_BABEu64;
        let mut next_u32 = || -> u32 {
            rng_state = rng_state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (rng_state >> 33) as u32
        };

        let mut saw_less = false;
        let mut saw_equal = false;
        let mut saw_greater = false;

        for _ in 0..10_000 {
            let l1 = next_u32() as usize;
            let c1 = next_u32() as usize;
            let l2 = next_u32() as usize;
            let c2 = next_u32() as usize;

            let a = Position::from_raw(l1, c1);
            let b = Position::from_raw(l2, c2);

            let packed_result = a.cmp(&b);
            let naive_result = naive_cmp(&a, &b);

            assert_eq!(
                packed_result, naive_result,
                "Mismatch: ({l1},{c1}) vs ({l2},{c2}): packed={packed_result:?}, naive={naive_result:?}"
            );

            match packed_result {
                std::cmp::Ordering::Less => saw_less = true,
                std::cmp::Ordering::Equal => saw_equal = true,
                std::cmp::Ordering::Greater => saw_greater = true,
            }
        }

        // Also force equal cases (unlikely from random).
        for line in [0, 1, 1000, u32::MAX as usize] {
            for col in [0, 1, 999, u32::MAX as usize] {
                let a = Position::from_raw(line, col);
                let b = Position::from_raw(line, col);
                assert_eq!(a.cmp(&b), std::cmp::Ordering::Equal);
                assert_eq!(naive_cmp(&a, &b), std::cmp::Ordering::Equal);
                saw_equal = true;
            }
        }

        assert!(saw_less, "No Less outcomes in random sweep");
        assert!(saw_equal, "No Equal outcomes in random sweep");
        assert!(saw_greater, "No Greater outcomes in random sweep");
    }

    /// Property: antisymmetry — if a < b then b > a.
    #[test]
    fn position_ordering_antisymmetry() {
        let cases = [
            (Position::from_raw(0, 0), Position::from_raw(0, 1)),
            (Position::from_raw(0, 0), Position::from_raw(1, 0)),
            (Position::from_raw(5, 99), Position::from_raw(5, 100)),
            (
                Position::from_raw(100, u32::MAX as usize),
                Position::from_raw(101, 0),
            ),
            (
                Position::from_raw(0, u32::MAX as usize),
                Position::from_raw(u32::MAX as usize, 0),
            ),
        ];
        for (a, b) in &cases {
            assert_eq!(
                a.cmp(b),
                std::cmp::Ordering::Less,
                "{a:?} should be < {b:?}"
            );
            assert_eq!(
                b.cmp(a),
                std::cmp::Ordering::Greater,
                "{b:?} should be > {a:?}"
            );
        }
    }

    /// Property: transitivity — if a < b and b < c then a < c.
    #[test]
    fn position_ordering_transitivity() {
        let a = Position::from_raw(10, 20);
        let b = Position::from_raw(10, 30);
        let c = Position::from_raw(11, 0);
        assert!(a < b);
        assert!(b < c);
        assert!(a < c, "Transitivity violated: a < b < c but not a < c");

        // Edge case: boundary values.
        let x = Position::from_raw(0, u32::MAX as usize - 1);
        let y = Position::from_raw(0, u32::MAX as usize);
        let z = Position::from_raw(1, 0);
        assert!(x < y);
        assert!(y < z);
        assert!(x < z, "Transitivity violated at u32 boundary");
    }

    /// BTreeMap uses Ord to maintain sorted key order. Verify positions
    /// inserted in arbitrary order are retrieved in line-then-col order.
    #[test]
    fn position_btreemap_sorted_iteration() {
        use std::collections::BTreeMap;

        let mut map: BTreeMap<Position, u32> = BTreeMap::new();
        // Insert in scrambled order.
        map.insert(Position::from_raw(5, 10), 4);
        map.insert(Position::from_raw(0, 0), 0);
        map.insert(Position::from_raw(2, 100), 3);
        map.insert(Position::from_raw(0, 50), 1);
        map.insert(Position::from_raw(0, 51), 2);
        map.insert(Position::from_raw(100, 0), 5);

        let values: Vec<u32> = map.values().copied().collect();
        assert_eq!(values, vec![0, 1, 2, 3, 4, 5]);
    }

    /// Vec::sort relies on Ord. Verify a shuffled vec sorts to line-major order.
    #[test]
    fn position_vec_sort_line_major() {
        let mut positions = vec![
            Position::from_raw(3, 0),
            Position::from_raw(0, 99),
            Position::from_raw(1, 50),
            Position::from_raw(1, 0),
            Position::from_raw(0, 0),
            Position::from_raw(3, 0), // duplicate
        ];
        positions.sort();

        let expected = vec![
            Position::from_raw(0, 0),
            Position::from_raw(0, 99),
            Position::from_raw(1, 0),
            Position::from_raw(1, 50),
            Position::from_raw(3, 0),
            Position::from_raw(3, 0),
        ];
        assert_eq!(positions, expected);
    }

    /// Hash must agree with Eq: equal positions produce equal hashes.
    #[test]
    fn position_hash_agrees_with_eq() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let pairs = [
            (Position::from_raw(0, 0), Position::from_raw(0, 0)),
            (Position::from_raw(42, 7), Position::from_raw(42, 7)),
            (
                Position::from_raw(u32::MAX as usize, u32::MAX as usize),
                Position::from_raw(u32::MAX as usize, u32::MAX as usize),
            ),
        ];
        for (a, b) in &pairs {
            assert_eq!(a, b);
            let mut ha = DefaultHasher::new();
            a.hash(&mut ha);
            let mut hb = DefaultHasher::new();
            b.hash(&mut hb);
            assert_eq!(
                ha.finish(),
                hb.finish(),
                "Hash inconsistent with Eq for {a:?}"
            );
        }
    }

    /// Positions used in real vim operations: marks, jump list entries,
    /// and visual selection boundaries must sort correctly.
    #[test]
    fn position_vim_operations_ordering() {
        // Jump list: navigating between positions in a file.
        let jump_list = vec![
            Position::from_raw(0, 0),   // file start
            Position::from_raw(10, 4),  // function header
            Position::from_raw(25, 8),  // inside loop
            Position::from_raw(100, 0), // end of file
        ];
        for window in jump_list.windows(2) {
            assert!(window[0] < window[1]);
        }

        // Visual selection: start must be <= end after normalization.
        let sel_start = Position::from_raw(5, 3);
        let sel_end = Position::from_raw(5, 15);
        assert!(sel_start <= sel_end);

        // Multi-line visual: start line < end line.
        let vl_start = Position::from_raw(10, 0);
        let vl_end = Position::from_raw(15, 79);
        assert!(vl_start < vl_end);

        // Mark positions: comparing cursor vs mark for `` jump.
        let cursor = Position::from_raw(50, 10);
        let mark = Position::from_raw(20, 5);
        assert!(mark < cursor);
    }

    // NOTE on overflow safety (64-bit release builds):
    //
    // If line or col > u32::MAX, the `as u64` cast preserves all 64 bits, but
    // `(0x1_0000_0000u64) << 32` wraps to 0 (needs 65 bits, Rust wraps the
    // result). This produces silently wrong comparison results in release mode.
    //
    // Protection: only the debug_assert (fires in debug/test builds). There is
    // NO compile-time static guarantee. This is acceptable because:
    // 1. No real document has > 4 billion lines or columns.
    // 2. On wasm32, usize is 32 bits so overflow is structurally impossible.
    // 3. All code paths that create Positions go through the document layer,
    //    which cannot produce values exceeding u32::MAX in practice.
}
