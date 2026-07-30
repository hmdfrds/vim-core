//! Runtime command property overrides.
//!
//! `PropertyOverlay` allows shells to override the behavioral properties of
//! specific `Command` variants at runtime — without modifying the grammar layer.
//!
//! # Use Case
//!
//! A shell may want to make a normally non-repeatable command repeatable, or
//! prevent a mutating command from recording to the dot-repeat buffer. Rather
//! than forking the grammar, the shell registers an override keyed by the
//! command's discriminant (a stable `u16`).
//!
//! # Example
//!
//! ```ignore
//! use vim_core::execution::PropertyOverlay;
//! use vim_core::primitives::{CommandProperties, RepeatBehavior};
//! use vim_core::grammar::Command;
//!
//! let mut overlay = PropertyOverlay::new();
//! // Make Motion commands record for repeat (non-standard, but demonstrative):
//! let disc = Command::Motion { count: NonZeroU32::MIN, motion: ..., explicit_count: false }.discriminant();
//! overlay.set(disc, CommandProperties { repeat: RepeatBehavior::Record, ..Default::default() });
//! ```

use crate::grammar::Command;
use crate::primitives::CommandProperties;
use ahash::AHashMap;

/// Runtime overrides for [`CommandProperties`] keyed by command discriminant.
///
/// Maps a `u16` discriminant (one per `Command` variant) to a replacement
/// [`CommandProperties`]. When the engine resolves properties for a command,
/// it checks this overlay first and falls back to `cmd.properties()` if no
/// override is registered.
#[derive(Debug, Default, Clone)]
pub struct PropertyOverlay {
    map: AHashMap<u16, CommandProperties>,
}

impl PropertyOverlay {
    /// Create an empty overlay (no overrides).
    #[must_use]
    pub fn new() -> Self {
        Self {
            map: AHashMap::new(),
        }
    }

    /// Resolve properties for the given command.
    ///
    /// Returns the overridden [`CommandProperties`] if one is registered for
    /// this command's discriminant; otherwise falls back to `cmd.properties()`.
    #[must_use]
    pub fn resolve(&self, cmd: &Command) -> CommandProperties {
        let disc = cmd.discriminant();
        if let Some(&props) = self.map.get(&disc) {
            props
        } else {
            cmd.properties()
        }
    }

    /// Register a property override for the given discriminant.
    ///
    /// Replaces any previously registered override for `discriminant`.
    pub fn set(&mut self, discriminant: u16, props: CommandProperties) {
        self.map.insert(discriminant, props);
    }

    /// Remove the override for the given discriminant, if any.
    ///
    /// After removal, `resolve()` will fall back to `cmd.properties()`.
    pub fn remove(&mut self, discriminant: u16) {
        self.map.remove(&discriminant);
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;
    use crate::grammar::types::{Action, Motion};
    use crate::grammar::Command;
    use crate::primitives::{CommandProperties, RepeatBehavior};

    #[test]
    fn new_is_empty() {
        let overlay = PropertyOverlay::new();
        // A Motion command should fall back to cmd.properties()
        let cmd = Command::Motion {
            count: NonZeroU32::MIN,
            motion: Motion::Down,
            explicit_count: false,
        };
        let resolved = overlay.resolve(&cmd);
        assert_eq!(resolved, cmd.properties());
    }

    #[test]
    fn set_and_resolve_override() {
        let mut overlay = PropertyOverlay::new();
        let cmd = Command::Motion {
            count: NonZeroU32::MIN,
            motion: Motion::Down,
            explicit_count: false,
        };
        let custom = CommandProperties {
            repeat: RepeatBehavior::Record,
            ..Default::default()
        };
        overlay.set(cmd.discriminant(), custom);
        assert_eq!(overlay.resolve(&cmd), custom);
    }

    #[test]
    fn remove_restores_default() {
        let mut overlay = PropertyOverlay::new();
        let cmd = Command::Motion {
            count: NonZeroU32::MIN,
            motion: Motion::Down,
            explicit_count: false,
        };
        let custom = CommandProperties {
            repeat: RepeatBehavior::Record,
            ..Default::default()
        };
        overlay.set(cmd.discriminant(), custom);
        overlay.remove(cmd.discriminant());
        assert_eq!(overlay.resolve(&cmd), cmd.properties());
    }

    #[test]
    fn different_variants_have_different_discriminants() {
        let motion = Command::Motion {
            count: NonZeroU32::MIN,
            motion: Motion::Down,
            explicit_count: false,
        };
        let action = Command::Action {
            count: NonZeroU32::MIN,
            register: None,
            action: Action::Undo,
        };
        assert_ne!(motion.discriminant(), action.discriminant());
    }

    #[test]
    fn same_variant_same_discriminant() {
        let cmd1 = Command::Motion {
            count: NonZeroU32::MIN,
            motion: Motion::Down,
            explicit_count: false,
        };
        let cmd2 = Command::Motion {
            count: NonZeroU32::new(42).unwrap(),
            motion: Motion::Up,
            explicit_count: true,
        };
        assert_eq!(cmd1.discriminant(), cmd2.discriminant());
    }
}
