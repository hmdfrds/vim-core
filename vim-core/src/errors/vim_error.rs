//! `VimError` enum — typed error variants with Vim error codes.
//!
//! Every variant maps to a real Vim error code (`:help error-messages`).
//! The `Display` impl produces the canonical `E{N}: {message}` format
//! that Vim displays in the status bar.
//!
//! **Exception**: `HostFailure` passes through unstructured host error
//! strings verbatim — these originate outside the engine and have no
//! Vim error code.
//!
//! # Usage
//!
//! Errors flow through the system as `Effect::ShowError { error: VimError }`.
//! The host reads `error.to_string()` to display in the status bar.
//!
//! # Adding New Errors
//!
//! 1. Add variant here with a doc comment showing the Vim error code
//! 2. Add the `Display` match arm with the `E{N}: {message}` format
//! 3. Add a unit test in the `tests` module verifying the Display output

use compact_str::CompactString;
use std::fmt;

/// Errors that can occur in vim-core.
///
/// Each variant carries a Vim error code in its Display representation,
/// matching real Vim/Neovim error behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum VimError {
    // ═══════════════════════════════════════════════════════════════════
    // Search errors
    // ═══════════════════════════════════════════════════════════════════
    /// E486: Pattern not found.
    PatternNotFound(CompactString),

    /// E35: No previous regular expression.
    NoPreviousPattern,

    /// E33: No previous substitute regular expression.
    NoPreviousSubstitute,

    /// E348: No string under cursor.
    NoStringUnderCursor,

    // ═══════════════════════════════════════════════════════════════════
    // Register errors
    // ═══════════════════════════════════════════════════════════════════
    /// E353: Nothing in register {0}.
    NothingInRegister(char),

    /// E748: No previously used register.
    NoPreviousRegister,

    /// E354: Invalid register name: '{0}'.
    InvalidRegisterName(CompactString),

    // ═══════════════════════════════════════════════════════════════════
    // Mark errors
    // ═══════════════════════════════════════════════════════════════════
    /// E20: Mark not set.
    MarkNotSet(char),

    // ═══════════════════════════════════════════════════════════════════
    // Changelist / jump list errors
    // ═══════════════════════════════════════════════════════════════════
    /// E662: At start of changelist.
    AtStartOfChangelist,

    /// E663: At end of changelist.
    AtEndOfChangelist,

    /// E664: Changelist is empty.
    ChangelistEmpty,

    /// E790: undojoin is not allowed after undo.
    E790,

    // ═══════════════════════════════════════════════════════════════════
    // Ex command errors
    // ═══════════════════════════════════════════════════════════════════
    /// E492: Not an editor command.
    NotEditorCommand(CompactString),

    /// E14: Invalid address.
    InvalidAddress(CompactString),

    /// E16: Invalid range.
    InvalidRange,

    /// E471: Argument required.
    ArgumentRequired,

    /// E493: Backwards range given.
    BackwardsRange,

    /// E134: Cannot move a range of lines into itself.
    MoveIntoItself,

    /// E521: Invalid argument (number required, wrong type, etc.).
    InvalidArgument(CompactString),

    /// E34: No previous command.
    NoPreviousCommand,

    /// E476: No command after g/v (global/vglobal).
    NoCommandAfterGlobal,

    /// E5100: :global command exceeded maximum line count.
    GlobalLineLimitExceeded {
        /// The configured limit that was exceeded.
        limit: usize,
    },

    /// E5101: :global recursion depth limit exceeded.
    GlobalRecursionLimitExceeded {
        /// The maximum allowed recursion depth.
        limit: u8,
    },

    /// E146: Regular expressions can't be delimited.
    InvalidRegexDelimiter,

    /// E682: Invalid search pattern or delimiter.
    InvalidSearchPattern,

    /// E476: Invalid command (e.g. empty pattern in `:s` with no previous).
    InvalidCommand,

    // ═══════════════════════════════════════════════════════════════════
    // Macro errors
    // ═══════════════════════════════════════════════════════════════════
    /// E223: Recursive mapping.
    RecursiveMacro {
        /// Register that caused the recursion.
        register: char,
        /// Current recursion depth.
        depth: usize,
    },

    /// Macro replay aborted: effect limit exceeded.
    MacroEffectLimitExceeded {
        /// The configured limit that was exceeded.
        limit: usize,
    },

    // ═══════════════════════════════════════════════════════════════════
    // Internal errors
    // ═══════════════════════════════════════════════════════════════════
    /// Internal engine error (should not occur in production).
    ///
    /// Fires when a new `Command` variant is added without a corresponding
    /// executor handler. Caught by `debug_assert!` in test builds;
    /// this variant provides graceful degradation in release.
    InternalError(CompactString),

    // ═══════════════════════════════════════════════════════════════════
    // Host boundary
    // ═══════════════════════════════════════════════════════════════════
    // ═══════════════════════════════════════════════════════════════════
    // Motion errors
    // ═══════════════════════════════════════════════════════════════════
    /// Motion failed (e.g. j at last line, k at first line).
    /// Causes macro playback to abort, matching Vim's beep-and-abort behavior.
    MotionFailed,

    /// All selections were filtered out (e.g. by `:keep` / `:remove`).
    NoSelectionsRemaining,

    /// Host-reported failure (e.g. file I/O error from the shell).
    ///
    /// Used exclusively at the FFI boundary when the host signals an
    /// unstructured error string. All vim-core-internal errors use
    /// typed variants above.
    HostFailure(CompactString),
}

/// Severity level of a [`VimError`], used by hosts to choose how to display it.
///
/// | Severity | Host behavior |
/// |----------|---------------|
/// | `Brief`  | Bell only — no status-bar text (boundary navigation noise) |
/// | `Normal` | Always shown in the status bar |
/// | `System` | Shown with host/system context (file I/O, FFI errors) |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ErrorSeverity {
    /// Bell-only errors: the cursor hit a boundary, nothing dramatic happened.
    Brief,
    /// Standard Vim errors displayed in the status bar.
    Normal,
    /// Host-originated or system-level errors that warrant extra context.
    System,
}

impl VimError {
    /// Returns the [`ErrorSeverity`] for this error.
    ///
    /// Hosts use this to decide display strategy: bell-only for `Brief`,
    /// status-bar for `Normal`, and system context for `System`.
    #[must_use]
    #[inline]
    pub const fn severity(&self) -> ErrorSeverity {
        match self {
            // Brief: boundary navigation — the cursor simply hit an edge.
            Self::NothingInRegister(_) | Self::MotionFailed => ErrorSeverity::Brief,

            // System: host-originated errors with no Vim error code.
            Self::HostFailure(_) => ErrorSeverity::System,

            // Normal: all other errors go to the status bar.
            _ => ErrorSeverity::Normal,
        }
    }
}

impl fmt::Display for VimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Search
            Self::PatternNotFound(pat) => write!(f, "E486: Pattern not found: {pat}"),
            Self::NoPreviousPattern => write!(f, "E35: No previous regular expression"),
            Self::NoPreviousSubstitute => {
                write!(f, "E33: No previous substitute regular expression")
            }
            Self::NoStringUnderCursor => write!(f, "E348: No string under cursor"),

            // Registers
            Self::NothingInRegister(c) => write!(f, "E353: Nothing in register {c}"),
            Self::NoPreviousRegister => write!(f, "E748: No previously used register"),
            Self::InvalidRegisterName(name) => write!(f, "E354: Invalid register name: '{name}'"),

            // Marks
            Self::MarkNotSet(_mark) => write!(f, "E20: Mark not set"),

            // Changelist
            Self::AtStartOfChangelist => write!(f, "E662: At start of changelist"),
            Self::AtEndOfChangelist => write!(f, "E663: At end of changelist"),
            Self::ChangelistEmpty => write!(f, "E664: Changelist is empty"),
            Self::E790 => write!(f, "E790: undojoin is not allowed after undo"),

            // Ex commands
            Self::NotEditorCommand(cmd) => write!(f, "E492: Not an editor command: {cmd}"),
            Self::InvalidAddress(addr) => write!(f, "E14: Invalid address: {addr}"),
            Self::InvalidRange => write!(f, "E16: Invalid range"),
            Self::ArgumentRequired => write!(f, "E471: Argument required"),
            Self::BackwardsRange => write!(f, "E493: Backwards range given"),
            Self::MoveIntoItself => {
                write!(f, "E134: Cannot move a range of lines into itself")
            }
            Self::InvalidArgument(msg) => write!(f, "E521: {msg}"),
            Self::NoPreviousCommand => write!(f, "E34: No previous command"),
            Self::NoCommandAfterGlobal => write!(f, "E476: No command after g/v"),
            Self::GlobalLineLimitExceeded { limit } => write!(
                f,
                "E5100: :global command exceeded maximum line count ({limit})"
            ),
            Self::GlobalRecursionLimitExceeded { limit } => {
                write!(f, "E5101: :global recursion depth limit exceeded ({limit})")
            }
            Self::InvalidRegexDelimiter => {
                write!(f, "E146: Regular expressions can't be delimited")
            }
            Self::InvalidSearchPattern => {
                write!(f, "E682: Invalid search pattern or delimiter")
            }
            Self::InvalidCommand => write!(f, "E476: Invalid command"),

            // Macros
            Self::RecursiveMacro { register, depth } => {
                write!(f, "E223: recursive mapping for @{register} (depth {depth})")
            }
            Self::MacroEffectLimitExceeded { limit } => {
                write!(
                    f,
                    "E5765: macro replay aborted: effect limit ({limit}) exceeded"
                )
            }

            // Motions
            Self::MotionFailed => write!(f, "motion failed"),

            // Selections
            Self::NoSelectionsRemaining => write!(f, "No selections remaining"),

            // Internal
            Self::InternalError(msg) => write!(f, "E0: internal error - {msg}"),

            // Host boundary
            Self::HostFailure(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for VimError {}

#[cfg(test)]
mod tests {
    use super::*;

    // ── ErrorSeverity tests ───────────────────────────────────────────────

    #[test]
    fn severity_brief_for_boundary_errors() {
        assert_eq!(
            VimError::NothingInRegister('"').severity(),
            ErrorSeverity::Brief
        );
        assert_eq!(VimError::MotionFailed.severity(), ErrorSeverity::Brief);
    }

    #[test]
    fn severity_system_for_host_errors() {
        assert_eq!(
            VimError::HostFailure("disk full".into()).severity(),
            ErrorSeverity::System
        );
    }

    #[test]
    fn severity_normal_for_standard_errors() {
        assert_eq!(
            VimError::NoPreviousPattern.severity(),
            ErrorSeverity::Normal
        );
        assert_eq!(VimError::InvalidRange.severity(), ErrorSeverity::Normal);
        assert_eq!(
            VimError::PatternNotFound("foo".into()).severity(),
            ErrorSeverity::Normal
        );
    }

    #[test]
    fn all_variants_have_severity() {
        // Exhaustively verify every currently-known variant returns a severity
        // (ensures the match compiles and is exhaustive at the type level).
        let variants: &[VimError] = &[
            VimError::PatternNotFound("x".into()),
            VimError::NoPreviousPattern,
            VimError::NoPreviousSubstitute,
            VimError::NoStringUnderCursor,
            VimError::NothingInRegister('a'),
            VimError::NoPreviousRegister,
            VimError::InvalidRegisterName("bad".into()),
            VimError::MarkNotSet('a'),
            VimError::AtStartOfChangelist,
            VimError::AtEndOfChangelist,
            VimError::ChangelistEmpty,
            VimError::NotEditorCommand("foo".into()),
            VimError::InvalidAddress("999".into()),
            VimError::InvalidRange,
            VimError::ArgumentRequired,
            VimError::BackwardsRange,
            VimError::MoveIntoItself,
            VimError::InvalidArgument("bad".into()),
            VimError::NoPreviousCommand,
            VimError::NoCommandAfterGlobal,
            VimError::GlobalLineLimitExceeded { limit: 100 },
            VimError::GlobalRecursionLimitExceeded { limit: 10 },
            VimError::InvalidRegexDelimiter,
            VimError::InvalidSearchPattern,
            VimError::InvalidCommand,
            VimError::RecursiveMacro {
                register: 'a',
                depth: 1,
            },
            VimError::MacroEffectLimitExceeded { limit: 100 },
            VimError::InternalError("oops".into()),
            VimError::MotionFailed,
            VimError::NoSelectionsRemaining,
            VimError::HostFailure("err".into()),
        ];
        for v in variants {
            // Just calling severity() is enough — the compiler ensures all
            // arms are covered; this loop ensures no panic at runtime.
            let _ = v.severity();
        }
    }

    #[test]
    fn display_pattern_not_found() {
        let e = VimError::PatternNotFound("foo".into());
        assert_eq!(e.to_string(), "E486: Pattern not found: foo");
    }

    #[test]
    fn display_no_previous_pattern() {
        assert_eq!(
            VimError::NoPreviousPattern.to_string(),
            "E35: No previous regular expression"
        );
    }

    #[test]
    fn display_nothing_in_register() {
        let e = VimError::NothingInRegister('"');
        assert_eq!(e.to_string(), "E353: Nothing in register \"");
    }

    #[test]
    fn display_mark_not_set() {
        let e = VimError::MarkNotSet('a');
        assert_eq!(e.to_string(), "E20: Mark not set");
    }

    #[test]
    fn display_not_editor_command() {
        let e = VimError::NotEditorCommand("foo".into());
        assert_eq!(e.to_string(), "E492: Not an editor command: foo");
    }

    #[test]
    fn display_recursive_macro() {
        let e = VimError::RecursiveMacro {
            register: 'a',
            depth: 200,
        };
        assert_eq!(e.to_string(), "E223: recursive mapping for @a (depth 200)");
    }

    #[test]
    fn display_changelist_errors() {
        assert_eq!(
            VimError::AtStartOfChangelist.to_string(),
            "E662: At start of changelist"
        );
        assert_eq!(
            VimError::AtEndOfChangelist.to_string(),
            "E663: At end of changelist"
        );
        assert_eq!(
            VimError::ChangelistEmpty.to_string(),
            "E664: Changelist is empty"
        );
    }

    #[test]
    fn display_host_failure() {
        let e = VimError::HostFailure("disk full".into());
        assert_eq!(e.to_string(), "disk full");
    }

    #[test]
    fn display_no_previous_register() {
        assert_eq!(
            VimError::NoPreviousRegister.to_string(),
            "E748: No previously used register"
        );
    }

    #[test]
    fn display_invalid_address() {
        let e = VimError::InvalidAddress("999".into());
        assert_eq!(e.to_string(), "E14: Invalid address: 999");
    }

    #[test]
    fn display_invalid_range() {
        assert_eq!(VimError::InvalidRange.to_string(), "E16: Invalid range");
    }

    #[test]
    fn display_argument_required() {
        assert_eq!(
            VimError::ArgumentRequired.to_string(),
            "E471: Argument required"
        );
    }

    #[test]
    fn display_backwards_range() {
        assert_eq!(
            VimError::BackwardsRange.to_string(),
            "E493: Backwards range given"
        );
    }

    #[test]
    fn display_no_previous_command() {
        assert_eq!(
            VimError::NoPreviousCommand.to_string(),
            "E34: No previous command"
        );
    }

    #[test]
    fn display_no_command_after_global() {
        assert_eq!(
            VimError::NoCommandAfterGlobal.to_string(),
            "E476: No command after g/v"
        );
    }

    #[test]
    fn display_invalid_regex_delimiter() {
        assert_eq!(
            VimError::InvalidRegexDelimiter.to_string(),
            "E146: Regular expressions can't be delimited"
        );
    }

    #[test]
    fn display_invalid_search_pattern() {
        assert_eq!(
            VimError::InvalidSearchPattern.to_string(),
            "E682: Invalid search pattern or delimiter"
        );
    }

    #[test]
    fn display_internal_error() {
        let e = VimError::InternalError("unhandled command".into());
        assert_eq!(e.to_string(), "E0: internal error - unhandled command");
    }

    #[test]
    fn display_macro_effect_limit_exceeded() {
        let e = VimError::MacroEffectLimitExceeded { limit: 100_000 };
        assert_eq!(
            e.to_string(),
            "E5765: macro replay aborted: effect limit (100000) exceeded"
        );
    }

    #[test]
    fn display_global_line_limit_exceeded() {
        let e = VimError::GlobalLineLimitExceeded { limit: 100_000 };
        assert_eq!(
            e.to_string(),
            "E5100: :global command exceeded maximum line count (100000)"
        );
    }

    #[test]
    fn display_global_recursion_limit_exceeded() {
        let e = VimError::GlobalRecursionLimitExceeded { limit: 10 };
        assert_eq!(
            e.to_string(),
            "E5101: :global recursion depth limit exceeded (10)"
        );
    }

    #[test]
    fn display_no_selections_remaining() {
        let e = VimError::NoSelectionsRemaining;
        assert_eq!(e.to_string(), "No selections remaining");
    }
}
