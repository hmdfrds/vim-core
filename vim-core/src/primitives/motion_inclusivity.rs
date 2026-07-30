//! Motion range inclusivity.
//!
//! Describes whether the end position of a motion range is included or excluded.
//! Distinct from `MotionType` which describes register paste behavior.
//!
//! Per Vim `:help inclusive-motion`:
//! - Inclusive: cursor ends on last char to change (e, f, $)
//! - Exclusive: cursor ends on first char NOT to change (w, t, 0)
//! - Linewise: entire lines are operated on (j, k, G)

/// How the end position of a motion range is treated.
///
/// Canonical type shared by motion computation and operator application.
/// Previously duplicated as `MotionInclusivity` (motions) and `Inclusivity` (operators).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum MotionInclusivity {
    /// End position NOT included in range (w, b, t).
    #[default]
    Exclusive,
    /// End position IS included in range (e, f, $).
    Inclusive,
    /// Operates on whole lines (j, k, gg, G).
    Linewise,
}

impl MotionInclusivity {
    /// Is this motion exclusive?
    #[inline]
    #[must_use]
    pub const fn is_exclusive(self) -> bool {
        matches!(self, Self::Exclusive)
    }

    /// Is this motion inclusive?
    #[inline]
    #[must_use]
    pub const fn is_inclusive(self) -> bool {
        matches!(self, Self::Inclusive)
    }

    /// Is this motion linewise?
    #[inline]
    #[must_use]
    pub const fn is_linewise(self) -> bool {
        matches!(self, Self::Linewise)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Default ──────────────────────────────────────────────────────────

    #[test]
    fn default_is_exclusive() {
        assert_eq!(MotionInclusivity::default(), MotionInclusivity::Exclusive);
    }

    // ── is_exclusive ─────────────────────────────────────────────────────

    #[test]
    fn exclusive_is_exclusive() {
        assert!(MotionInclusivity::Exclusive.is_exclusive());
    }

    #[test]
    fn inclusive_is_not_exclusive() {
        assert!(!MotionInclusivity::Inclusive.is_exclusive());
    }

    #[test]
    fn linewise_is_not_exclusive() {
        assert!(!MotionInclusivity::Linewise.is_exclusive());
    }

    // ── is_inclusive ─────────────────────────────────────────────────────

    #[test]
    fn inclusive_is_inclusive() {
        assert!(MotionInclusivity::Inclusive.is_inclusive());
    }

    #[test]
    fn exclusive_is_not_inclusive() {
        assert!(!MotionInclusivity::Exclusive.is_inclusive());
    }

    #[test]
    fn linewise_is_not_inclusive() {
        assert!(!MotionInclusivity::Linewise.is_inclusive());
    }

    // ── is_linewise ──────────────────────────────────────────────────────

    #[test]
    fn linewise_is_linewise() {
        assert!(MotionInclusivity::Linewise.is_linewise());
    }

    #[test]
    fn exclusive_is_not_linewise() {
        assert!(!MotionInclusivity::Exclusive.is_linewise());
    }

    #[test]
    fn inclusive_is_not_linewise() {
        assert!(!MotionInclusivity::Inclusive.is_linewise());
    }
}
