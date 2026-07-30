//! Total conversions between byte offsets and the signed integer domains.
//!
//! Byte offsets and lengths are `usize`. The *difference* between two of them
//! is signed, and the external binding layer (Rhai/FFI/WASM) speaks `i64`.
//! Rust guarantees that no live object exceeds `isize::MAX` bytes, so every
//! length this crate handles fits losslessly in `isize` and `i64` — but that
//! fact is invisible to the type system, which is why an `as` cast used to be
//! written at each of these boundaries.
//!
//! The helpers below make the conversion *total* without an `as` cast. Each
//! one saturates at the target type's bound rather than wrapping, truncating
//! or losing a sign bit:
//!
//! - For a length or offset that came from a real document, the saturating
//!   branch is unreachable — `usize::MAX`-sized documents do not exist.
//! - For a value arriving from outside (a negative offset from a script),
//!   saturating to the far end of the range is the *documented* behaviour:
//!   the value is out of range, and every accessor that consumes it already
//!   treats "past the end" as out of range.
//!
//! # Layering
//!
//! Bottom of the dependency graph: imports only `core`.

/// A byte offset, length or count as `isize`, saturating at `isize::MAX`.
#[inline]
#[must_use]
pub(crate) fn to_isize(len: usize) -> isize {
    isize::try_from(len).unwrap_or(isize::MAX)
}

/// A byte offset, length or count as `i64`, saturating at `i64::MAX`.
#[inline]
#[must_use]
pub(crate) fn to_i64(len: usize) -> i64 {
    i64::try_from(len).unwrap_or(i64::MAX)
}

/// A byte offset, length or count as `u32`, saturating at `u32::MAX`.
#[inline]
#[must_use]
pub(crate) fn to_u32(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

/// The signed difference `new_len - old_len` of two byte lengths.
///
/// Saturates at `isize::MIN`/`isize::MAX`, which no real pair of lengths can
/// reach.
#[inline]
#[must_use]
pub(crate) fn delta(new_len: usize, old_len: usize) -> isize {
    if new_len >= old_len {
        to_isize(new_len - old_len)
    } else {
        -to_isize(old_len - new_len)
    }
}

/// The signed difference `new_len - old_len` of two byte lengths, as `i32`.
///
/// Saturates at `i32::MIN`/`i32::MAX`. Callers that store deltas in an `i32`
/// already assume documents below 2 GiB; saturating keeps the arithmetic
/// monotone instead of wrapping to the opposite sign if that assumption is
/// ever violated.
#[inline]
#[must_use]
pub(crate) fn delta_i32(new_len: usize, old_len: usize) -> i32 {
    if new_len >= old_len {
        i32::try_from(new_len - old_len).unwrap_or(i32::MAX)
    } else {
        i32::try_from(old_len - new_len).map_or(i32::MIN, i32::wrapping_neg)
    }
}

/// The signed difference `new_len - old_len` of two byte lengths, as `i64`.
#[inline]
#[must_use]
pub(crate) fn delta_i64(new_len: usize, old_len: usize) -> i64 {
    if new_len >= old_len {
        to_i64(new_len - old_len)
    } else {
        -to_i64(old_len - new_len)
    }
}

/// Apply a signed `i64` byte delta to an offset, saturating at `0`.
///
/// Equivalent to [`usize::saturating_add_signed`] for deltas that fit in
/// `isize`; deltas larger than that saturate the offset instead of wrapping.
#[inline]
#[must_use]
pub(crate) fn shift(base: usize, delta: i64) -> usize {
    if delta >= 0 {
        base.saturating_add(usize::try_from(delta).unwrap_or(usize::MAX))
    } else {
        base.saturating_sub(usize::try_from(delta.unsigned_abs()).unwrap_or(usize::MAX))
    }
}

/// An externally supplied `i64` byte offset as a `usize`.
///
/// Negative offsets are not valid positions; they saturate to `usize::MAX`,
/// which every buffer accessor already treats as "past the end".
#[inline]
#[must_use]
pub(crate) fn offset_from_i64(value: i64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}
