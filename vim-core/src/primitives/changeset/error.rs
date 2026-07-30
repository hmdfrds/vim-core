//! Error types for `ChangeSet` operations.
//!
//! All fallible changeset operations return `Result<_, ChangeSetError>`.
//! This crate has `#![deny(clippy::panic)]` — we never panic on bad input.

/// Errors produced by `ChangeSet` operations.
///
/// Every variant carries enough context to diagnose the mismatch
/// without needing a debugger.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum ChangeSetError {
    /// Document length does not match the changeset's expected input length.
    ///
    /// Returned by [`ChangeSet::apply`](crate::primitives::ChangeSet::apply) and
    /// [`ChangeSet::invert`](crate::primitives::ChangeSet::invert) when the
    /// provided text has a different byte length than `input_len`.
    LengthMismatch {
        /// The input length the changeset was built for.
        expected: usize,
        /// The actual byte length of the provided text.
        actual: usize,
    },

    /// Two changesets cannot be composed because their lengths don't align.
    ///
    /// `compose(A, B)` requires `A.output_len == B.input_len`.
    ComposeMismatch {
        /// Output length of the first changeset (A).
        a_output: usize,
        /// Input length of the second changeset (B).
        b_input: usize,
    },

    /// Two changesets cannot be transformed because they don't share
    /// the same base document length.
    ///
    /// `transform(A, B)` requires `A.input_len == B.input_len`.
    TransformMismatch {
        /// Input length of changeset A.
        a_input: usize,
        /// Input length of changeset B.
        b_input: usize,
    },
}

impl std::fmt::Display for ChangeSetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LengthMismatch { expected, actual } => {
                write!(
                    f,
                    "changeset length mismatch: expected {expected} bytes, got {actual}"
                )
            }
            Self::ComposeMismatch { a_output, b_input } => {
                write!(
                    f,
                    "compose mismatch: first changeset outputs {a_output} bytes, \
                     second expects {b_input}"
                )
            }
            Self::TransformMismatch { a_input, b_input } => {
                write!(
                    f,
                    "transform mismatch: changeset A has input_len {a_input}, \
                     changeset B has input_len {b_input}"
                )
            }
        }
    }
}

impl std::error::Error for ChangeSetError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_length_mismatch() {
        let err = ChangeSetError::LengthMismatch {
            expected: 100,
            actual: 42,
        };
        let msg = err.to_string();
        assert!(msg.contains("100"));
        assert!(msg.contains("42"));
    }

    #[test]
    fn display_compose_mismatch() {
        let err = ChangeSetError::ComposeMismatch {
            a_output: 50,
            b_input: 30,
        };
        let msg = err.to_string();
        assert!(msg.contains("50"));
        assert!(msg.contains("30"));
    }

    #[test]
    fn clone_and_eq() {
        let err = ChangeSetError::LengthMismatch {
            expected: 10,
            actual: 20,
        };
        let cloned = err.clone();
        assert_eq!(err, cloned);
    }

    #[test]
    fn different_variants_not_equal() {
        let a = ChangeSetError::LengthMismatch {
            expected: 10,
            actual: 20,
        };
        let b = ChangeSetError::ComposeMismatch {
            a_output: 10,
            b_input: 20,
        };
        assert_ne!(a, b);
    }

    #[test]
    fn display_transform_mismatch() {
        let err = ChangeSetError::TransformMismatch {
            a_input: 20,
            b_input: 15,
        };
        let msg = err.to_string();
        assert!(msg.contains("20"));
        assert!(msg.contains("15"));
    }

    #[test]
    fn transform_mismatch_not_equal_to_compose_mismatch() {
        let a = ChangeSetError::TransformMismatch {
            a_input: 10,
            b_input: 20,
        };
        let b = ChangeSetError::ComposeMismatch {
            a_output: 10,
            b_input: 20,
        };
        assert_ne!(a, b);
    }

    #[test]
    fn error_trait_impl() {
        let err: Box<dyn std::error::Error> = Box::new(ChangeSetError::LengthMismatch {
            expected: 1,
            actual: 2,
        });
        // Just verify it compiles and has a Display message
        assert!(!err.to_string().is_empty());
    }
}
