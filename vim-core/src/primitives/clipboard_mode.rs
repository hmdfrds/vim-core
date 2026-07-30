//! System clipboard synchronization policy.

/// Controls when the engine auto-syncs with the system clipboard.
///
/// - `Always`: sync on every yank and delete.
/// - `OnYank`: sync on yank only; deletes stay local.
/// - `Never` (default): never auto-sync; host must read/write clipboard explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum UseSystemClipboard {
    /// Sync on every yank and delete.
    Always,
    /// Sync on yank only; deletes stay local.
    OnYank,
    /// Never auto-sync (default).
    #[default]
    Never,
}

impl UseSystemClipboard {
    /// Whether a yank operation should sync to the system clipboard.
    #[must_use]
    pub const fn should_sync_yank(self) -> bool {
        matches!(self, Self::Always | Self::OnYank)
    }

    /// Whether a delete operation should sync to the system clipboard.
    #[must_use]
    pub const fn should_sync_delete(self) -> bool {
        matches!(self, Self::Always)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_never() {
        assert_eq!(UseSystemClipboard::default(), UseSystemClipboard::Never);
    }

    #[test]
    fn never_syncs_nothing() {
        let mode = UseSystemClipboard::Never;
        assert!(!mode.should_sync_yank());
        assert!(!mode.should_sync_delete());
    }

    #[test]
    fn on_yank_syncs_yank_only() {
        let mode = UseSystemClipboard::OnYank;
        assert!(mode.should_sync_yank());
        assert!(!mode.should_sync_delete());
    }

    #[test]
    fn always_syncs_both() {
        let mode = UseSystemClipboard::Always;
        assert!(mode.should_sync_yank());
        assert!(mode.should_sync_delete());
    }
}
