//! Typed Vim events — deterministic, zero-overhead event system.
//!
//! Events are typed Rust enums (not strings like autocmd). They appear
//! ordered in the effect stream (deterministic) and are zero-overhead
//! when the host ignores them — no allocation, no callback registration.
//!
//! This is an innovation: no other embeddable vim engine provides this.

use compact_str::CompactString;

use super::{Mode, MotionType, Offset, RegisterName, SearchDirection, VarScope, VimValue};

/// Direction of an undo/redo operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum UndoDirection {
    /// An undo operation (`u`).
    Undo,
    /// A redo operation (`Ctrl-R`).
    Redo,
}

/// Payload for the [`VimEvent::OptionSet`] variant.
///
/// Boxed to keep `VimEvent` within the `Effect` size budget since `VimValue`
/// can be arbitrarily large (nested lists/maps).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OptionSetPayload {
    /// Name of the option that changed.
    pub name: CompactString,
    /// Previous value of the option.
    pub old: VimValue,
    /// New value of the option.
    pub new: VimValue,
}

/// A typed Vim event emitted as an effect.
///
/// Events notify the host about state transitions and cursor movements
/// without requiring callback registration. The host inspects the effect
/// stream and acts on events it cares about.
///
/// # Note on trait bounds
///
/// `VimEvent` implements `Clone` and `PartialEq` but NOT `Copy`, `Eq`, or `Hash`.
/// Some variants contain `VimValue` (which wraps `f64`) making `Eq`/`Hash` unsound.
/// For subscription matching, use [`std::mem::discriminant()`] to compare by variant
/// without inspecting payload data.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum VimEvent {
    /// Entering insert mode (triggered after `i`, `a`, `o`, etc.).
    InsertEnter,
    /// Leaving insert mode (triggered after `<Esc>` in insert mode).
    InsertLeave,
    /// Mode changed from one mode to another.
    ModeChanged {
        /// The mode we're leaving.
        from: Mode,
        /// The mode we're entering.
        to: Mode,
    },
    /// Cursor moved in normal or visual mode.
    CursorMoved,
    /// Cursor moved in insert mode.
    CursorMovedI,
    /// Text was changed in normal mode (after an edit command completes).
    TextChanged,
    /// Text was changed in insert mode.
    TextChangedI,
    /// Search wrapped around the document boundary.
    ///
    /// The direction tells the host which boundary was crossed so it can
    /// display the canonical Vim messages:
    /// - `Forward`  → "search hit BOTTOM, continuing at TOP"
    /// - `Backward` → "search hit TOP, continuing at BOTTOM"
    SearchWrapped {
        /// The effective search direction at the time the wrap occurred.
        direction: SearchDirection,
    },
    /// Macro recording started.
    RecordingEnter,
    /// Macro recording stopped.
    RecordingLeave,
    /// Command-line mode entered (`:`, `/`, `?`).
    CmdlineEnter,
    /// Command-line mode exited.
    CmdlineLeave,
    /// Visual mode entered.
    VisualEnter,
    /// Visual mode exited.
    VisualLeave,
    /// Select mode entered.
    SelectEnter,
    /// Select mode exited.
    SelectLeave,
    /// A yank operation completed.
    ///
    /// Fired after `y{motion}`, `Y`, visual-mode `y`, and `:yank`.
    /// NOT fired during macro replay or dot-repeat of yank
    /// (matches Neovim's TextYankPost behavior).
    YankPost {
        /// Start byte offset of the yanked range.
        start: Offset,
        /// End byte offset of the yanked range (exclusive).
        end: Offset,
        /// Register the text was yanked into.
        register: RegisterName,
        /// How the range was interpreted.
        motion_type: MotionType,
    },
    /// A buffer was entered (became the active buffer).
    BufEnter,
    /// A buffer was left (another buffer became active).
    BufLeave,
    /// A buffer is about to be written.
    BufWrite,
    /// A buffer was successfully written.
    BufWritePost,
    /// A Vim option was changed.
    ///
    /// The payload is boxed to keep `VimEvent` within the `Effect` size budget
    /// (VimValue can be arbitrarily large due to nested lists/maps).
    OptionSet(Box<OptionSetPayload>),
    /// A variable in the variable store changed.
    VariableChanged {
        /// Scope of the variable (global or buffer-local).
        scope: VarScope,
        /// Name of the variable that changed.
        name: CompactString,
    },
    /// An undo or redo operation was performed.
    UndoRedo {
        /// Whether this was an undo or redo.
        direction: UndoDirection,
    },
    /// Cursor has not moved for `updatetime` milliseconds in Normal mode.
    ///
    /// Emitted by the engine in response to a `TimerFired` host notification
    /// when the timer was a CursorHold timer. Hosts use this for background
    /// tasks like LSP hover, swap file writing, etc.
    CursorHold,
    /// About to insert a character in insert mode.
    ///
    /// Emitted before the character is actually inserted into the buffer.
    /// Hosts can inspect `char` and potentially modify behavior. This is
    /// the typed-event equivalent of Vim's `InsertCharPre` autocmd.
    InsertCharPre {
        /// The character about to be inserted.
        char: char,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_hold_variant_exists_and_is_constructable() {
        let event = VimEvent::CursorHold;
        assert_eq!(event, VimEvent::CursorHold);
    }

    #[test]
    fn insert_char_pre_variant_exists_and_carries_char() {
        let event = VimEvent::InsertCharPre { char: 'a' };
        assert_eq!(event, VimEvent::InsertCharPre { char: 'a' });
        assert_ne!(event, VimEvent::InsertCharPre { char: 'b' });
    }

    #[test]
    fn cursor_hold_is_distinct_from_other_events() {
        assert_ne!(VimEvent::CursorHold, VimEvent::CursorMoved);
        assert_ne!(VimEvent::CursorHold, VimEvent::InsertEnter);
    }

    #[test]
    fn insert_char_pre_clone_and_debug() {
        let event = VimEvent::InsertCharPre { char: 'z' };
        let cloned = event.clone();
        assert_eq!(event, cloned);
        let debug = format!("{event:?}");
        assert!(debug.contains("InsertCharPre"));
        assert!(debug.contains('z'));
    }

    #[test]
    fn cursor_hold_debug() {
        let debug = format!("{:?}", VimEvent::CursorHold);
        assert!(debug.contains("CursorHold"));
    }
}
