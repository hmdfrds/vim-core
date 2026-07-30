//! Sticky sub-mode target identification.
//!
//! Identifies which built-in prefix group a sticky sub-mode session re-enters.

/// Which prefix group a sticky sub-mode session targets.
///
/// When sticky mode is active, each keypress is implicitly prefixed with the
/// group's leader key, so the user can chain commands without re-pressing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum StickyTarget {
    /// Ctrl-W window commands.
    Window,
    /// z-prefix scroll/fold commands.
    ZPrefix,
}

impl StickyTarget {
    /// Human-readable mode name for status-line display.
    #[inline]
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Window => "WINDOW",
            Self::ZPrefix => "SCROLL",
        }
    }

    /// The prefix key sequence shown in pending-key displays.
    #[inline]
    #[must_use]
    pub const fn prefix_display(self) -> &'static str {
        match self {
            Self::Window => "^W",
            Self::ZPrefix => "z",
        }
    }
}
