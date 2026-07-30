//! Ex command modifier flags.
//!
//! Modifiers like `:silent`, `:keepjumps`, etc. are parsed before the command
//! name and act as post-processing filters on the effects output.
//!
//! This is a grammar-only type — no effect or execution imports.

use bitflags::bitflags;

bitflags! {
    /// Modifier flags parsed from ex command prefixes.
    ///
    /// These flags are NOT checked at individual call sites. Instead, a single
    /// `filter_effects()` call at the pipeline exit removes effects that match
    /// the active flags. This replaces 200+ scattered flag checks with one
    /// post-processing pass.
    ///
    /// # Examples
    ///
    /// ```text
    /// :silent echo "hi"          → SILENT
    /// :silent! echo "hi"         → SILENT | SILENT_BANG
    /// :keepjumps normal dd       → KEEPJUMPS
    /// :silent keepjumps d3j      → SILENT | KEEPJUMPS
    /// ```
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct ModifierFlags: u16 {
        /// `:silent` — suppress `ShowInfo` effects.
        const SILENT       = 0x0001;
        /// `:silent!` — also suppress `ShowError` effects (implies SILENT).
        const SILENT_BANG  = 0x0002;
        /// `:keepjumps` — suppress `PushJumpList` effects.
        const KEEPJUMPS    = 0x0004;
        /// `:keeppatterns` — suppress `SetSearchPattern` and `SetSubstitutePattern` effects.
        const KEEPPATTERNS = 0x0008;
        /// `:lockmarks` — suppress `SetMark` effects.
        const LOCKMARKS    = 0x0010;
        /// `:keepalt` — suppress alternate file register changes.
        const KEEPALT      = 0x0020;
        /// Reserved for future `:noautocmd`.
        const NOAUTOCMD    = 0x0040;
        /// `:tab` — open in a new tab.
        const TAB          = 0x0080;
        /// `:vertical` — split vertically.
        const VERTICAL     = 0x0100;
        /// `:horizontal` — split horizontally.
        const HORIZONTAL   = 0x0200;
        /// `:topleft` — position split at top/left.
        const TOPLEFT      = 0x0400;
        /// `:botright` — position split at bottom/right.
        const BOTRIGHT     = 0x0800;
        /// `:aboveleft` — position split above/left of current.
        const ABOVELEFT    = 0x1000;
        /// `:belowright` — position split below/right of current.
        const BELOWRIGHT   = 0x2000;
        /// `:browse` — use file browser.
        const BROWSE       = 0x4000;
        /// `:confirm` — prompt for confirmation.
        const CONFIRM      = 0x8000;
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for ModifierFlags {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.bits().serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for ModifierFlags {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bits = u16::deserialize(deserializer)?;
        Self::from_bits(bits).ok_or_else(|| {
            serde::de::Error::custom(format!("invalid ModifierFlags bits: {bits:#x}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_empty() {
        assert!(ModifierFlags::default().is_empty());
    }

    #[test]
    fn silent_bang_is_separate_from_silent() {
        let flags = ModifierFlags::SILENT | ModifierFlags::SILENT_BANG;
        assert!(flags.contains(ModifierFlags::SILENT));
        assert!(flags.contains(ModifierFlags::SILENT_BANG));
    }

    #[test]
    fn compose_multiple_flags() {
        let flags = ModifierFlags::SILENT | ModifierFlags::KEEPJUMPS | ModifierFlags::LOCKMARKS;
        assert!(flags.contains(ModifierFlags::SILENT));
        assert!(flags.contains(ModifierFlags::KEEPJUMPS));
        assert!(flags.contains(ModifierFlags::LOCKMARKS));
        assert!(!flags.contains(ModifierFlags::KEEPPATTERNS));
    }
}
