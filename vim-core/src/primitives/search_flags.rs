//! Search flags for controlling search behavior.
//!
//! [`SearchFlags`] bundles case-sensitivity, wrapping, and magic mode
//! into a single value passed through the search pipeline: from options
//! + per-pattern modifiers → built-in search / `SearchProvider`.

use vim_regex::MagicMode;

/// Flags controlling search behavior.
///
/// Constructed from [`super::VimOptions`] + per-pattern modifiers
/// (`\c`, `\C`, `\v`, `\V`), then passed to both the built-in
/// substring search and the host's `SearchProvider::find_match`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SearchFlags {
    /// Whether the search is case-sensitive.
    ///
    /// - `true` = match case exactly (default when `ignorecase` is off).
    /// - `false` = match case-insensitively (`ignorecase` on, or `\c` modifier).
    case_sensitive: bool,

    /// Whether to wrap around document boundaries.
    ///
    /// - `true` = wrap (default, `wrapscan` on).
    /// - `false` = stop at document start/end.
    wrap: bool,

    /// Regex interpretation mode.
    magic: MagicMode,
}

impl SearchFlags {
    /// Default search flags: case-sensitive, wrapping, magic mode.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            case_sensitive: true,
            wrap: true,
            magic: MagicMode::Magic,
        }
    }

    /// Builder: set case sensitivity.
    #[inline]
    #[must_use]
    pub const fn with_case_sensitive(mut self, v: bool) -> Self {
        self.case_sensitive = v;
        self
    }

    /// Builder: set wrap-around.
    #[inline]
    #[must_use]
    pub const fn with_wrap(mut self, v: bool) -> Self {
        self.wrap = v;
        self
    }

    /// Builder: set magic mode.
    #[inline]
    #[must_use]
    pub const fn with_magic(mut self, v: MagicMode) -> Self {
        self.magic = v;
        self
    }

    /// Whether the search is case-sensitive.
    #[inline]
    #[must_use]
    pub const fn case_sensitive(&self) -> bool {
        self.case_sensitive
    }

    /// Whether to wrap around document boundaries.
    #[inline]
    #[must_use]
    pub const fn wrap(&self) -> bool {
        self.wrap
    }

    /// Regex interpretation mode.
    #[inline]
    #[must_use]
    pub const fn magic(&self) -> MagicMode {
        self.magic
    }
}

impl Default for SearchFlags {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Construction ─────────────────────────────────────────────────────

    #[test]
    fn new_returns_default_flags() {
        let f = SearchFlags::new();
        assert!(f.case_sensitive());
        assert!(f.wrap());
        assert_eq!(f.magic(), MagicMode::Magic);
    }

    #[test]
    fn default_matches_new() {
        assert_eq!(SearchFlags::default(), SearchFlags::new());
    }

    // ── Builder chaining ─────────────────────────────────────────────────

    #[test]
    fn with_case_sensitive_false() {
        let f = SearchFlags::new().with_case_sensitive(false);
        assert!(!f.case_sensitive());
        // other flags unchanged
        assert!(f.wrap());
        assert_eq!(f.magic(), MagicMode::Magic);
    }

    #[test]
    fn with_wrap_false() {
        let f = SearchFlags::new().with_wrap(false);
        assert!(!f.wrap());
        assert!(f.case_sensitive());
    }

    #[test]
    fn with_magic_very_magic() {
        let f = SearchFlags::new().with_magic(MagicMode::VeryMagic);
        assert_eq!(f.magic(), MagicMode::VeryMagic);
    }

    #[test]
    fn with_magic_no_magic() {
        let f = SearchFlags::new().with_magic(MagicMode::NoMagic);
        assert_eq!(f.magic(), MagicMode::NoMagic);
    }

    #[test]
    fn with_magic_very_no_magic() {
        let f = SearchFlags::new().with_magic(MagicMode::VeryNoMagic);
        assert_eq!(f.magic(), MagicMode::VeryNoMagic);
    }

    // ── Combining multiple builders ──────────────────────────────────────

    #[test]
    fn combine_all_builders() {
        let f = SearchFlags::new()
            .with_case_sensitive(false)
            .with_wrap(false)
            .with_magic(MagicMode::VeryMagic);
        assert!(!f.case_sensitive());
        assert!(!f.wrap());
        assert_eq!(f.magic(), MagicMode::VeryMagic);
    }

    #[test]
    fn builder_last_wins() {
        let f = SearchFlags::new()
            .with_case_sensitive(false)
            .with_case_sensitive(true);
        assert!(f.case_sensitive());
    }

    // ── Individual flag presence ─────────────────────────────────────────

    #[test]
    fn case_sensitive_accessor() {
        assert!(SearchFlags::new()
            .with_case_sensitive(true)
            .case_sensitive());
        assert!(!SearchFlags::new()
            .with_case_sensitive(false)
            .case_sensitive());
    }

    #[test]
    fn wrap_accessor() {
        assert!(SearchFlags::new().with_wrap(true).wrap());
        assert!(!SearchFlags::new().with_wrap(false).wrap());
    }

    #[test]
    fn magic_accessor() {
        assert_eq!(
            SearchFlags::new().with_magic(MagicMode::Magic).magic(),
            MagicMode::Magic
        );
    }

    // ── MagicMode default ────────────────────────────────────────────────

    #[test]
    fn magic_mode_default_is_magic() {
        assert_eq!(MagicMode::default(), MagicMode::Magic);
    }
}
