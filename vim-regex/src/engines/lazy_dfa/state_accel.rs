//! State acceleration via memchr.
//!
//! A state is "accelerable" when 3 or fewer distinct byte values can cause
//! a transition to a different state (all other bytes loop back to self).
//! For such states, we use memchr/memchr2/memchr3 to SIMD-skip directly
//! to the next byte that could leave the state.

use super::byte_classes::ByteClasses;
use super::TaggedStateId;

/// Acceleration metadata for a single DFA state.
#[derive(Debug, Clone, Copy)]
pub(super) struct AccelInfo {
    /// The bytes that can cause a non-self transition (1-3 bytes).
    pub(super) bytes: [u8; 3],
    /// Number of valid entries in `bytes` (1, 2, or 3).
    pub(super) count: u8,
}

impl AccelInfo {
    /// Maximum number of non-self transitions for a state to be accelerable.
    pub(super) const MAX_ACCEL_BYTES: usize = 3;

    /// Try to compute acceleration info for a state.
    ///
    /// `self_premul`: the premultiplied index of the state being analyzed.
    /// `num_classes`: number of byte equivalence classes.
    /// `get_target`: closure mapping class_id -> target TaggedStateId.
    /// `classes`: byte classes for reverse-mapping class -> representative bytes.
    pub(super) fn analyze(
        self_premul: u32,
        num_classes: usize,
        get_target: impl Fn(u8) -> TaggedStateId,
        classes: &ByteClasses,
    ) -> Option<Self> {
        let _ = num_classes; // used implicitly via get_target bounds
        let mut non_self_bytes: Vec<u8> = Vec::new();

        // For each byte value 0..=255, check if its transition leaves this state.
        for byte in 0..=255u8 {
            let class = classes.classify(byte);
            let target = get_target(class);
            // A transition is "non-self" if it goes anywhere other than back
            // to this state (or if the target is tagged, since tagged states
            // trigger the slow path anyway).
            if (target.index() != self_premul as usize || target.is_tagged())
                && !non_self_bytes.contains(&byte)
            {
                non_self_bytes.push(byte);
                if non_self_bytes.len() > Self::MAX_ACCEL_BYTES {
                    return None;
                }
            }
        }

        if non_self_bytes.is_empty() {
            return None; // All transitions are self-loops (dead-like state).
        }

        let count = non_self_bytes.len() as u8;
        let mut bytes = [0u8; 3];
        for (i, &b) in non_self_bytes.iter().enumerate() {
            bytes[i] = b;
        }

        Some(AccelInfo { bytes, count })
    }
}

/// Use memchr to skip forward to the next byte that could leave the current state.
///
/// Returns the position of the first byte in `text[pos..]` that matches one
/// of the acceleration bytes, or `text.len()` if none found.
#[inline]
pub(super) fn accel_skip(accel: &AccelInfo, text: &[u8], pos: usize) -> usize {
    match accel.count {
        1 => memchr::memchr(accel.bytes[0], &text[pos..]).map_or(text.len(), |i| pos + i),
        2 => memchr::memchr2(accel.bytes[0], accel.bytes[1], &text[pos..])
            .map_or(text.len(), |i| pos + i),
        3 => memchr::memchr3(accel.bytes[0], accel.bytes[1], accel.bytes[2], &text[pos..])
            .map_or(text.len(), |i| pos + i),
        _ => pos,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accel_skip_single_byte() {
        let accel = AccelInfo {
            bytes: [b'x', 0, 0],
            count: 1,
        };
        let text = b"aaaaaaxbbb";
        assert_eq!(accel_skip(&accel, text, 0), 6);
    }

    #[test]
    fn accel_skip_two_bytes() {
        let accel = AccelInfo {
            bytes: [b'x', b'y', 0],
            count: 2,
        };
        let text = b"aaaaybbb";
        assert_eq!(accel_skip(&accel, text, 0), 4);
    }

    #[test]
    fn accel_skip_three_bytes() {
        let accel = AccelInfo {
            bytes: *b"xyz",
            count: 3,
        };
        let text = b"aaazaaa";
        assert_eq!(accel_skip(&accel, text, 0), 3);
    }

    #[test]
    fn accel_skip_not_found() {
        let accel = AccelInfo {
            bytes: [b'x', 0, 0],
            count: 1,
        };
        let text = b"aaaaaaa";
        assert_eq!(accel_skip(&accel, text, 0), 7);
    }

    #[test]
    fn accel_skip_from_offset() {
        let accel = AccelInfo {
            bytes: [b'x', 0, 0],
            count: 1,
        };
        let text = b"xaaaxaa";
        assert_eq!(accel_skip(&accel, text, 1), 4);
    }
}
