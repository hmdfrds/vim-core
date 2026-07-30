//! Effect provenance tracking.
//!
//! Tags a `Response` with metadata about which command produced its effects
//! and at what keystroke index. Enables debugging, replay analysis, and
//! session recording correlation.

use std::borrow::Cow;

/// Metadata about the origin of effects in a Response.
///
/// Tags a Response with the command that produced it and a monotonic
/// keystroke sequence number. Used by session recording, middleware
/// logging, and debugging to trace "this Replace came from dw at keystroke #7."
///
/// Placed on Response (not individual Effects) because one command produces
/// multiple effects that share the same origin. This is zero-cost when unused
/// (`Option<EffectProvenance>` is `None` for pending/ignored responses).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EffectProvenance {
    /// Command variant tag (e.g., "OperatorMotion", "Insert", "Action").
    ///
    /// Stored as `Cow<'static, str>` so that the hot path (passing a `&'static str`
    /// from `Command::tag()`) is zero-allocation, while still supporting owned
    /// strings when needed (e.g., after deserialization).
    command_name: Cow<'static, str>,
    /// Monotonic keystroke sequence number. Incremented on each `process()` call.
    /// Enables ordering and correlation across a session.
    keystroke_seq: u64,
}

impl EffectProvenance {
    /// Create a new provenance tag.
    ///
    /// Accepts a `&'static str` (the common hot-path case via `Command::tag()`)
    /// with zero heap allocation.
    ///
    /// # Arguments
    ///
    /// * `command_name` - Static variant tag from `Command::tag()`.
    /// * `keystroke_seq` - Monotonic keystroke sequence number.
    #[inline]
    #[must_use]
    pub const fn new(command_name: &'static str, keystroke_seq: u64) -> Self {
        Self {
            command_name: Cow::Borrowed(command_name),
            keystroke_seq,
        }
    }

    /// Command variant tag (e.g., `"Motion"`, `"OperatorMotion"`, `"Insert"`).
    ///
    /// Returns the static tag string from `Command::tag()`.
    #[inline]
    #[must_use]
    pub fn command_name(&self) -> &str {
        &self.command_name
    }

    /// Monotonic keystroke sequence number.
    ///
    /// Incremented on each `VimEngine::process()` call. Enables ordering
    /// and correlation across a session (e.g., "this effect came from
    /// keystroke #7").
    #[inline]
    #[must_use]
    pub const fn keystroke_seq(&self) -> u64 {
        self.keystroke_seq
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_stores_command_name_and_seq() {
        let prov = EffectProvenance::new("OperatorMotion(Delete, WordForward)", 42);
        assert_eq!(prov.command_name(), "OperatorMotion(Delete, WordForward)");
        assert_eq!(prov.keystroke_seq(), 42);
    }

    #[test]
    fn clone_and_eq() {
        let prov = EffectProvenance::new("Motion(Down)", 1);
        let cloned = prov.clone();
        assert_eq!(prov, cloned);
    }

    #[test]
    fn debug_format() {
        let prov = EffectProvenance::new("Action(Undo)", 7);
        let debug = format!("{prov:?}");
        assert!(debug.contains("Action(Undo)"));
        assert!(debug.contains('7'));
    }

    #[test]
    fn different_seq_not_equal() {
        let a = EffectProvenance::new("Motion(Down)", 1);
        let b = EffectProvenance::new("Motion(Down)", 2);
        assert_ne!(a, b);
    }

    #[test]
    fn different_name_not_equal() {
        let a = EffectProvenance::new("Motion(Up)", 1);
        let b = EffectProvenance::new("Motion(Down)", 1);
        assert_ne!(a, b);
    }

    #[test]
    fn accepts_static_str() {
        // The hot path: zero-allocation &'static str from Command::tag()
        let _a = EffectProvenance::new("Motion", 0);
        let _b = EffectProvenance::new("OperatorMotion", 1);
        let _c = EffectProvenance::new("InsertExit", 2);
    }
}
