//! Find direction and last-find state.
//!
//! Pure value types for `f`/`F`/`t`/`T` find commands.
//! Used by effects (to record) and state (to store).

/// Find direction type for `LastFind` state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FindDirection {
    /// f{char} - find forward
    #[default]
    FindForward,
    /// F{char} - find backward
    FindBackward,
    /// t{char} - till forward
    TillForward,
    /// T{char} - till backward
    TillBackward,
    /// s{c1}{c2} - sneak forward (two-char cross-line find)
    SneakForward,
    /// S{c1}{c2} - sneak backward (two-char cross-line find)
    SneakBackward,
}

impl FindDirection {
    /// Get the reverse direction.
    #[must_use]
    pub const fn reverse(self) -> Self {
        match self {
            Self::FindForward => Self::FindBackward,
            Self::FindBackward => Self::FindForward,
            Self::TillForward => Self::TillBackward,
            Self::TillBackward => Self::TillForward,
            Self::SneakForward => Self::SneakBackward,
            Self::SneakBackward => Self::SneakForward,
        }
    }

    /// Whether this is a sneak direction.
    #[must_use]
    pub const fn is_sneak(self) -> bool {
        matches!(self, Self::SneakForward | Self::SneakBackward)
    }
}

/// State for repeat find commands (`;` and `,`).
///
/// Stores the last `f`, `F`, `t`, `T`, or sneak command so it can be repeated
/// with `;` (same direction) or `,` (reverse direction).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LastFind {
    /// The last find command and its target character (always paired).
    data: Option<(FindDirection, char)>,
    /// Second character for sneak motions. Only meaningful when direction is
    /// `SneakForward` or `SneakBackward`.
    sneak_c2: Option<char>,
    /// Resolved `ignorecase` flag at the time of the original find.
    /// Used by `;`/`,` to repeat with the same case sensitivity.
    resolved_ignorecase: bool,
    /// Resolved `smartcase` flag at the time of the original find.
    /// Used by `;`/`,` to repeat with the same case sensitivity.
    resolved_smartcase: bool,
}

impl LastFind {
    /// Create a new empty `LastFind`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            data: None,
            sneak_c2: None,
            resolved_ignorecase: false,
            resolved_smartcase: false,
        }
    }

    /// Get the find direction.
    #[inline]
    #[must_use]
    pub fn direction(self) -> Option<FindDirection> {
        self.data.map(|(d, _)| d)
    }

    /// Get the target character (first char for sneak).
    #[inline]
    #[must_use]
    pub fn target_char(self) -> Option<char> {
        self.data.map(|(_, c)| c)
    }

    /// Get the second sneak character, if this was a sneak find.
    #[inline]
    #[must_use]
    pub const fn sneak_c2(self) -> Option<char> {
        self.sneak_c2
    }

    /// Get the resolved `ignorecase` flag from the original find.
    #[inline]
    #[must_use]
    pub const fn resolved_ignorecase(self) -> bool {
        self.resolved_ignorecase
    }

    /// Get the resolved `smartcase` flag from the original find.
    #[inline]
    #[must_use]
    pub const fn resolved_smartcase(self) -> bool {
        self.resolved_smartcase
    }

    /// Record a find command.
    pub const fn record(&mut self, direction: FindDirection, c: char) {
        self.data = Some((direction, c));
        self.sneak_c2 = None;
    }

    /// Record a find command with resolved case flags.
    pub const fn record_with_case(
        &mut self,
        direction: FindDirection,
        c: char,
        ignorecase: bool,
        smartcase: bool,
    ) {
        self.data = Some((direction, c));
        self.sneak_c2 = None;
        self.resolved_ignorecase = ignorecase;
        self.resolved_smartcase = smartcase;
    }

    /// Record a sneak command (two characters).
    pub const fn record_sneak(&mut self, direction: FindDirection, c1: char, c2: char) {
        self.data = Some((direction, c1));
        self.sneak_c2 = Some(c2);
    }

    /// Record a sneak command with resolved case flags.
    pub const fn record_sneak_with_case(
        &mut self,
        direction: FindDirection,
        c1: char,
        c2: char,
        ignorecase: bool,
        smartcase: bool,
    ) {
        self.data = Some((direction, c1));
        self.sneak_c2 = Some(c2);
        self.resolved_ignorecase = ignorecase;
        self.resolved_smartcase = smartcase;
    }
}
