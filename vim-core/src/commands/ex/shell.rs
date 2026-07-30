//! Ex commands that remain effect-only.
//!
//! Shell/file operations now cross the execution host boundary via
//! `HostRequest` (see `execution/host.rs`). This module only contains ex
//! commands that can still be expressed as pure effects.

use super::types::ExResult;
use crate::effects::Effects;

/// Execute nohlsearch command (`:noh`).
///
/// Clears search highlights.
///
/// # Errors
///
/// Returns `ExError` if the operation fails.
pub fn nohighlight() -> ExResult {
    Ok(Effects::new().clear_highlights())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;

    #[test]
    fn test_nohighlight() {
        let effects = nohighlight().unwrap();
        assert!(matches!(effects.as_slice()[0], Effect::ClearHighlights));
    }
}
