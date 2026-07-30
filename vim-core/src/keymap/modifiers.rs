//! Keyboard modifiers using bitflags.
//!
//! Supports Ctrl, Alt, Shift, and Meta modifier keys.

use bitflags::bitflags;
use compact_str::CompactString;

bitflags! {
    /// Keyboard modifiers.
    ///
    /// Can be combined using bitwise OR:
    /// ```
    /// use vim_core::keymap::Modifiers;
    /// let mods = Modifiers::CTRL | Modifiers::SHIFT;
    /// assert!(mods.contains(Modifiers::CTRL));
    /// ```
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct Modifiers: u8 {
        /// No modifiers.
        const NONE  = 0b0000;
        /// Control key.
        const CTRL  = 0b0001;
        /// Alt/Option key.
        const ALT   = 0b0010;
        /// Shift key.
        const SHIFT = 0b0100;
        /// Meta/Super/Windows key.
        const META  = 0b1000;
    }
}

impl Modifiers {
    /// Parse modifier prefix from Vim notation.
    ///
    /// Returns (modifiers, remaining string).
    ///
    /// # Examples
    /// - `C-w` → (CTRL, "w")
    /// - `M-x` → (ALT, "x")
    /// - `S-Tab` → (SHIFT, "Tab")
    /// - `C-S-f` → (CTRL | SHIFT, "f")
    #[must_use]
    pub fn from_vim_prefix(s: &str) -> (Self, &str) {
        let mut mods = Self::NONE;
        let mut remaining = s;

        loop {
            match remaining.get(..2) {
                Some(p) if p.eq_ignore_ascii_case("C-") => {
                    mods |= Self::CTRL;
                    remaining = &remaining[2..];
                }
                Some(p) if p.eq_ignore_ascii_case("M-") || p.eq_ignore_ascii_case("A-") => {
                    mods |= Self::ALT;
                    remaining = &remaining[2..];
                }
                Some(p) if p.eq_ignore_ascii_case("S-") => {
                    mods |= Self::SHIFT;
                    remaining = &remaining[2..];
                }
                Some(p) if p.eq_ignore_ascii_case("D-") => {
                    mods |= Self::META;
                    remaining = &remaining[2..];
                }
                _ => break,
            }
        }

        (mods, remaining)
    }

    /// Convert to Vim modifier prefix.
    ///
    /// Returns a `CompactString` — maximum output is `"C-D-M-S-"` (8 bytes),
    /// always fits in SSO without heap allocation.
    #[must_use]
    pub fn to_vim_prefix(&self) -> CompactString {
        let mut result = CompactString::default();

        if self.contains(Self::CTRL) {
            result.push_str("C-");
        }
        if self.contains(Self::META) {
            result.push_str("D-");
        }
        if self.contains(Self::ALT) {
            result.push_str("M-");
        }
        if self.contains(Self::SHIFT) {
            result.push_str("S-");
        }

        result
    }
}

impl std::fmt::Display for Modifiers {
    /// Human-readable standalone display.
    ///
    /// Transforms the Vim prefix notation into a friendly form:
    /// - `""` (NONE) → `""`
    /// - `"C-"` → `"Ctrl"`
    /// - `"S-"` → `"Shift"`
    /// - `"M-"` → `"Alt"`
    /// - `"D-"` → `"Meta"`
    /// - `"C-S-"` → `"Ctrl+Shift"`
    /// - `"C-D-M-S-"` → `"Ctrl+Meta+Alt+Shift"`
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut first = true;

        // Emit in the same order as to_vim_prefix: CTRL, META, ALT, SHIFT.
        for (flag, name) in [
            (Self::CTRL, "Ctrl"),
            (Self::META, "Meta"),
            (Self::ALT, "Alt"),
            (Self::SHIFT, "Shift"),
        ] {
            if self.contains(flag) {
                if !first {
                    f.write_str("+")?;
                }
                f.write_str(name)?;
                first = false;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_is_empty() {
        assert!(Modifiers::NONE.is_empty());
        assert!(!Modifiers::CTRL.is_empty());
    }

    #[test]
    fn bitflag_combinations() {
        let mods = Modifiers::CTRL | Modifiers::SHIFT;
        assert!(mods.contains(Modifiers::CTRL));
        assert!(mods.contains(Modifiers::SHIFT));
        assert!(!mods.contains(Modifiers::ALT));
        assert!(!mods.contains(Modifiers::META));
    }

    #[test]
    fn from_vim_prefix_ctrl() {
        let (mods, rest) = Modifiers::from_vim_prefix("C-w");
        assert_eq!(mods, Modifiers::CTRL);
        assert_eq!(rest, "w");
    }

    #[test]
    fn from_vim_prefix_alt_m() {
        let (mods, rest) = Modifiers::from_vim_prefix("M-x");
        assert_eq!(mods, Modifiers::ALT);
        assert_eq!(rest, "x");
    }

    #[test]
    fn from_vim_prefix_alt_a() {
        let (mods, rest) = Modifiers::from_vim_prefix("A-x");
        assert_eq!(mods, Modifiers::ALT);
        assert_eq!(rest, "x");
    }

    #[test]
    fn from_vim_prefix_shift() {
        let (mods, rest) = Modifiers::from_vim_prefix("S-Tab");
        assert_eq!(mods, Modifiers::SHIFT);
        assert_eq!(rest, "Tab");
    }

    #[test]
    fn from_vim_prefix_meta() {
        let (mods, rest) = Modifiers::from_vim_prefix("D-a");
        assert_eq!(mods, Modifiers::META);
        assert_eq!(rest, "a");
    }

    #[test]
    fn from_vim_prefix_combo() {
        let (mods, rest) = Modifiers::from_vim_prefix("C-S-f");
        assert_eq!(mods, Modifiers::CTRL | Modifiers::SHIFT);
        assert_eq!(rest, "f");
    }

    #[test]
    fn from_vim_prefix_no_modifier() {
        let (mods, rest) = Modifiers::from_vim_prefix("w");
        assert_eq!(mods, Modifiers::NONE);
        assert_eq!(rest, "w");
    }

    #[test]
    fn to_vim_prefix_roundtrip() {
        let cases = [
            Modifiers::CTRL,
            Modifiers::ALT,
            Modifiers::SHIFT,
            Modifiers::META,
            Modifiers::CTRL | Modifiers::SHIFT,
        ];
        for mods in cases {
            let prefix = mods.to_vim_prefix();
            let s = format!("{prefix}x");
            let (parsed, rest) = Modifiers::from_vim_prefix(&s);
            assert_eq!(parsed, mods);
            assert_eq!(rest, "x");
        }
    }

    #[test]
    fn to_vim_prefix_none_is_empty() {
        assert_eq!(Modifiers::NONE.to_vim_prefix(), "");
    }

    #[test]
    fn display_none_is_empty() {
        assert_eq!(format!("{}", Modifiers::NONE), "");
    }

    #[test]
    fn display_single_modifiers() {
        assert_eq!(format!("{}", Modifiers::CTRL), "Ctrl");
        assert_eq!(format!("{}", Modifiers::ALT), "Alt");
        assert_eq!(format!("{}", Modifiers::SHIFT), "Shift");
        assert_eq!(format!("{}", Modifiers::META), "Meta");
    }

    #[test]
    fn display_combined_modifiers() {
        assert_eq!(
            format!("{}", Modifiers::CTRL | Modifiers::SHIFT),
            "Ctrl+Shift"
        );
        assert_eq!(format!("{}", Modifiers::CTRL | Modifiers::ALT), "Ctrl+Alt");
        assert_eq!(
            format!("{}", Modifiers::ALT | Modifiers::SHIFT),
            "Alt+Shift"
        );
        assert_eq!(
            format!(
                "{}",
                Modifiers::CTRL | Modifiers::META | Modifiers::ALT | Modifiers::SHIFT
            ),
            "Ctrl+Meta+Alt+Shift"
        );
    }

    #[test]
    fn display_order_matches_vim_prefix_order() {
        // Both Display and to_vim_prefix emit in CTRL, META, ALT, SHIFT order.
        let all = Modifiers::CTRL | Modifiers::META | Modifiers::ALT | Modifiers::SHIFT;
        let prefix = all.to_vim_prefix();
        // prefix is "C-D-M-S-", display is "Ctrl+Meta+Alt+Shift"
        assert_eq!(prefix.as_str(), "C-D-M-S-");
        assert_eq!(format!("{all}"), "Ctrl+Meta+Alt+Shift");
    }
}
