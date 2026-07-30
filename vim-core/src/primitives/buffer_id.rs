//! Buffer identifier for cross-buffer jump list navigation.
//!
//! A lightweight, opaque handle that uniquely identifies a buffer (document)
//! within a host editor session. The host assigns `BufferId` values; the
//! engine stores them in jump list entries to enable Ctrl-O / Ctrl-I
//! navigation across buffers.

use derive_more::Display;

/// Opaque buffer identifier assigned by the host.
///
/// The engine never interprets the inner value — it is purely a token
/// for equality comparison. The host is free to use any scheme (sequential
/// integers, hash of file path, etc.) as long as each open buffer gets a
/// distinct `BufferId`.
///
/// # Examples
///
/// ```ignore
/// let buf = BufferId::new(1);
/// assert_eq!(buf.get(), 1);
/// assert_eq!(buf, BufferId::new(1));
/// assert_ne!(buf, BufferId::new(2));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Display)]
#[display(fmt = "{_0}")]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct BufferId(u64);

impl BufferId {
    /// Create a new buffer identifier.
    #[inline]
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Get the raw identifier value.
    #[inline]
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equality() {
        assert_eq!(BufferId::new(1), BufferId::new(1));
        assert_ne!(BufferId::new(1), BufferId::new(2));
    }

    #[test]
    fn ordering() {
        assert!(BufferId::new(1) < BufferId::new(2));
    }

    #[test]
    fn display() {
        assert_eq!(format!("{}", BufferId::new(42)), "42");
    }

    #[test]
    fn round_trip() {
        let id = BufferId::new(999);
        assert_eq!(id.get(), 999);
    }
}
