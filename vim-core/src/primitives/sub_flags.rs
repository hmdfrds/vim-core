//! Substitute-command flags and case-sensitivity override.
//!
//! These are pure post-parse data carried by effects, state, and commands.
//! They live in `primitives` because they have no parser semantics — they
//! describe engine state (which substitute flags were set), not user intent.
//!
//! These types used to live in `grammar/types/ex_command.rs`. They moved here
//! because the effects layer needs them, and effects must not import from the
//! grammar layer — that edge would invert the dependency direction.

/// Case sensitivity override for substitute commands.
///
/// Replaces the old mutually-exclusive `ignore_case: bool` + `case_sensitive: bool`
/// pair with a three-state enum that makes illegal states unrepresentable.
///
/// - `Default` — no override; use the `ignorecase`/`smartcase` options.
/// - `IgnoreCase` — the `i` flag was set; force case-insensitive matching.
/// - `CaseSensitive` — the `I` flag was set; force case-sensitive matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum CaseSensitivity {
    /// No override — defer to `ignorecase`/`smartcase` options.
    #[default]
    Default,
    /// Force case-insensitive matching (`i` flag).
    IgnoreCase,
    /// Force case-sensitive matching (`I` flag).
    CaseSensitive,
}

/// Substitute command flags (`:s/pat/rep/flags`).
///
/// When both `i` and `I` appear in the flag string, last-one-wins (Vim behavior).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[allow(
    clippy::struct_excessive_bools,
    reason = "flags are inherently boolean"
)]
pub struct SubFlags {
    /// Replace all occurrences on each line (`g`).
    global: bool,
    /// Confirm each substitution (`c`).
    confirm: bool,
    /// Case sensitivity override (`i` for ignore-case, `I` for force case-sensitive).
    case: CaseSensitivity,
    /// Count matches only, don't replace (`n`).
    count_only: bool,
    /// Use the last search pattern (from `/`/`?`) as the substitute pattern (`r`).
    ///
    /// When set, the pattern field of `:s` is ignored and the last `/`-search
    /// pattern is used instead.  This mirrors Vim's `:s///r` behaviour.
    use_last_search: bool,
    /// Reuse the flags from the previous `:s` command (`&`).
    ///
    /// When set, the flags from the last substitute are merged in before
    /// the explicitly-given flags.  Mirrors Vim's `:s///&` behaviour.
    reuse_flags: bool,
    /// Suppress "Pattern not found" errors when no match (`e`).
    ///
    /// When set, a substitute that finds no matches silently succeeds
    /// instead of emitting `ShowError(PatternNotFound)`.  Essential for
    /// `:g/pat/s/old/new/e` chains and macro replay.
    #[cfg_attr(feature = "serde", serde(default))]
    suppress_error: bool,
}

impl SubFlags {
    /// Parse flags from a string (e.g., "gi").
    ///
    /// When both `i` and `I` appear, last-one-wins — matching Vim behavior.
    #[must_use]
    pub fn parse(s: &str) -> Self {
        let mut flags = Self::default();
        for c in s.chars() {
            match c {
                'g' => flags.global = true,
                'c' => flags.confirm = true,
                'i' => flags.case = CaseSensitivity::IgnoreCase,
                'I' => flags.case = CaseSensitivity::CaseSensitive,
                'n' => flags.count_only = true,
                'r' => flags.use_last_search = true,
                '&' => flags.reuse_flags = true,
                'e' => flags.suppress_error = true,
                _ => {} // Ignore unknown
            }
        }
        flags
    }

    /// Replace all occurrences on each line (`g` flag).
    #[must_use]
    pub const fn global(self) -> bool {
        self.global
    }
    /// Confirm each substitution (`c` flag).
    #[must_use]
    pub const fn confirm(self) -> bool {
        self.confirm
    }
    /// Case sensitivity override (`i` / `I` flag).
    #[must_use]
    pub const fn case(self) -> CaseSensitivity {
        self.case
    }
    /// Count matches only, don't replace (`n` flag).
    #[must_use]
    pub const fn count_only(self) -> bool {
        self.count_only
    }
    /// Use the last search pattern (from `/`/`?`) as the substitute pattern (`r` flag).
    #[must_use]
    pub const fn use_last_search(self) -> bool {
        self.use_last_search
    }
    /// Reuse flags from the previous `:s` command (`&` flag).
    #[must_use]
    pub const fn reuse_flags(self) -> bool {
        self.reuse_flags
    }
    /// Suppress "Pattern not found" errors when no match (`e` flag).
    #[must_use]
    pub const fn suppress_error(self) -> bool {
        self.suppress_error
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_e_flag() {
        let flags = SubFlags::parse("e");
        assert!(flags.suppress_error());
        assert!(!flags.global());
    }

    #[test]
    fn parse_ge_flags() {
        let flags = SubFlags::parse("ge");
        assert!(flags.global());
        assert!(flags.suppress_error());
    }
}
