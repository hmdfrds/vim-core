//! Macro recording and playback state.
//!
//! # Layering
//!
//! State holds pure data containers with no execution logic. Imports
//! `primitives` and `std`; must not import `commands`, `effects`,
//! `execution` or `dispatch`.
//!
//! Stores macro recording state for `q{a-z}` recording and `@{a-z}` playback.
//!
//! # Vim Macro Semantics
//!
//! - `q{a-z}` - Start recording keystrokes to register
//! - `q` (while recording) - Stop recording
//! - `@{a-z}` - Play macro from register
//! - `@@` - Repeat last played macro
//!
//! # Recursion Guard
//!
//! Macro replay tracks depth to prevent infinite loops (e.g. `qa@aq @a`).
//! Maximum depth matches Vim's limit of 1000.

use crate::primitives::RegisterName;

/// Maximum macro recursion depth (matches Vim).
pub const MAX_MACRO_DEPTH: u32 = 1000;

/// Error returned when macro recursion exceeds [`MAX_MACRO_DEPTH`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MacroRecursionError {
    /// The register that was being replayed.
    register: RegisterName,
    /// The depth at which recursion was detected.
    depth: u32,
}

impl MacroRecursionError {
    /// Get the register that caused the error.
    #[inline]
    #[must_use]
    pub const fn register(&self) -> RegisterName {
        self.register
    }

    /// Get the depth at which recursion was detected.
    #[inline]
    #[must_use]
    pub const fn depth(&self) -> u32 {
        self.depth
    }
}

impl std::fmt::Display for MacroRecursionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "E223: recursive mapping for @{} (depth {})",
            self.register, self.depth
        )
    }
}

impl std::error::Error for MacroRecursionError {}

/// Macro recording and playback state.
///
/// Tracks whether macro recording is active, which register was last played,
/// and the current replay recursion depth.
/// The actual keystroke storage is handled by the shell (via effects).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MacroState {
    /// Register currently being recorded to, if any.
    recording_register: Option<RegisterName>,

    /// Last register used for playback (for `@@`).
    last_played: Option<RegisterName>,

    /// Current macro replay recursion depth.
    ///
    /// Incremented by [`begin_replay`], decremented by [`end_replay`].
    /// Capped at [`MAX_MACRO_DEPTH`] to prevent infinite loops.
    replay_depth: u32,
}

impl MacroState {
    /// Create new empty macro state.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if currently recording a macro.
    #[inline]
    #[must_use]
    pub const fn is_recording(&self) -> bool {
        self.recording_register.is_some()
    }

    /// Get the register currently being recorded to.
    #[inline]
    #[must_use]
    pub const fn recording_register(&self) -> Option<RegisterName> {
        self.recording_register
    }

    /// Start recording to a register.
    ///
    /// If already recording, this is a no-op (caller should check `is_recording()`).
    #[inline]
    pub fn start_recording(&mut self, register: RegisterName) {
        debug_assert!(
            !self.is_recording(),
            "Already recording macro to register '{}'",
            self.recording_register.map_or('?', RegisterName::char)
        );
        if self.is_recording() {
            return;
        }
        self.recording_register = Some(register);
    }

    /// Stop recording.
    ///
    /// Returns the register that was being recorded to.
    #[inline]
    pub const fn stop_recording(&mut self) -> Option<RegisterName> {
        self.recording_register.take()
    }

    /// Get the last played register (for `@@`).
    #[inline]
    #[must_use]
    pub const fn last_played(&self) -> Option<RegisterName> {
        self.last_played
    }

    /// Set the last played register.
    #[inline]
    pub const fn set_last_played(&mut self, register: RegisterName) {
        self.last_played = Some(register);
    }

    /// Current macro replay recursion depth.
    #[inline]
    #[must_use]
    pub const fn replay_depth(&self) -> u32 {
        self.replay_depth
    }

    /// Begin a macro replay, incrementing the recursion depth.
    ///
    /// Returns `Err` if the recursion depth would exceed [`MAX_MACRO_DEPTH`].
    /// The caller should display the error message and abort the replay.
    ///
    /// # Errors
    ///
    /// Returns `MacroRecursionError` if max recursion depth is exceeded.
    #[inline]
    pub const fn begin_replay(
        &mut self,
        register: RegisterName,
    ) -> Result<(), MacroRecursionError> {
        if self.replay_depth >= MAX_MACRO_DEPTH {
            return Err(MacroRecursionError {
                register,
                depth: self.replay_depth,
            });
        }
        self.replay_depth += 1;
        self.last_played = Some(register);
        Ok(())
    }

    /// End a macro replay, decrementing the recursion depth.
    ///
    /// # Panics
    /// Debug-asserts that depth > 0 (unbalanced end_replay).
    #[inline]
    pub const fn end_replay(&mut self) {
        debug_assert!(self.replay_depth > 0, "Unbalanced end_replay");
        self.replay_depth = self.replay_depth.saturating_sub(1);
    }

    /// Check if currently replaying a macro.
    #[inline]
    #[must_use]
    pub const fn is_replaying(&self) -> bool {
        self.replay_depth > 0
    }

    /// Reset the replay depth to zero without affecting other state.
    ///
    /// Used by [`VimEngine::abort_replay()`](crate::execution::VimEngine::abort_replay)
    /// when clearing the macro stack and typeahead buffer. Unlike [`Self::clear()`],
    /// this preserves `recording_register` and `last_played`.
    #[inline]
    pub const fn reset_replay_depth(&mut self) {
        self.replay_depth = 0;
    }

    /// Clear macro state.
    #[inline]
    pub const fn clear(&mut self) {
        self.recording_register = None;
        self.last_played = None;
        self.replay_depth = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::RegisterName;

    fn rn(c: char) -> RegisterName {
        RegisterName::new_unchecked(c)
    }

    #[test]
    fn test_recording_lifecycle() {
        let mut state = MacroState::new();

        assert!(!state.is_recording());
        assert_eq!(state.recording_register(), None);

        state.start_recording(rn('a'));
        assert!(state.is_recording());
        assert_eq!(state.recording_register(), Some(rn('a')));

        let stopped = state.stop_recording();
        assert_eq!(stopped, Some(rn('a')));
        assert!(!state.is_recording());
    }

    #[test]
    fn test_last_played() {
        let mut state = MacroState::new();

        assert_eq!(state.last_played(), None);

        state.set_last_played(rn('q'));
        assert_eq!(state.last_played(), Some(rn('q')));
    }

    #[test]
    fn test_replay_depth_lifecycle() {
        let mut state = MacroState::new();
        assert_eq!(state.replay_depth(), 0);
        assert!(!state.is_replaying());

        state.begin_replay(rn('a')).unwrap();
        assert_eq!(state.replay_depth(), 1);
        assert!(state.is_replaying());
        assert_eq!(state.last_played(), Some(rn('a')));

        state.begin_replay(rn('b')).unwrap();
        assert_eq!(state.replay_depth(), 2);
        assert_eq!(state.last_played(), Some(rn('b')));

        state.end_replay();
        assert_eq!(state.replay_depth(), 1);

        state.end_replay();
        assert_eq!(state.replay_depth(), 0);
        assert!(!state.is_replaying());
    }

    #[test]
    fn test_recursion_guard() {
        let mut state = MacroState::new();

        // Fill to max depth
        for i in 0..MAX_MACRO_DEPTH {
            state
                .begin_replay(rn('a'))
                .unwrap_or_else(|_| panic!("depth {i} should succeed"));
        }
        assert_eq!(state.replay_depth(), MAX_MACRO_DEPTH);

        // Next attempt should fail
        let err = state.begin_replay(rn('a')).unwrap_err();
        assert_eq!(err.register(), rn('a'));
        assert_eq!(err.depth(), MAX_MACRO_DEPTH);
        assert!(err.to_string().contains("E223"));

        // Depth unchanged after error
        assert_eq!(state.replay_depth(), MAX_MACRO_DEPTH);
    }

    #[test]
    fn test_clear_resets_depth() {
        let mut state = MacroState::new();
        state.begin_replay(rn('a')).unwrap();
        state.begin_replay(rn('b')).unwrap();
        assert_eq!(state.replay_depth(), 2);

        state.clear();
        assert_eq!(state.replay_depth(), 0);
        assert!(!state.is_replaying());
    }

    #[test]
    fn test_reset_replay_depth_preserves_other_state() {
        let mut state = MacroState::new();
        state.start_recording(rn('r'));
        state.set_last_played(rn('p'));
        state.begin_replay(rn('a')).unwrap();
        state.begin_replay(rn('b')).unwrap();
        assert_eq!(state.replay_depth(), 2);

        state.reset_replay_depth();

        assert_eq!(state.replay_depth(), 0);
        assert!(!state.is_replaying());
        // Recording and last_played are preserved (unlike clear())
        assert!(state.is_recording());
        assert_eq!(state.recording_register(), Some(rn('r')));
        // last_played was updated to 'b' by begin_replay, that's preserved
        assert_eq!(state.last_played(), Some(rn('b')));
    }
}
