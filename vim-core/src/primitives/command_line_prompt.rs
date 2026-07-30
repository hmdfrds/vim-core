//! Command-line prompt kind — pure domain enum.
//!
//! Describes which prompt is active (`:`, `/`, `?`). This type lives
//! at the `primitives` layer because it has zero internal dependencies
//! and is consumed by state, grammar, and execution.

/// Active command-line prompt kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CommandLinePrompt {
    /// Ex prompt (`:`).\
    #[default]
    Ex,
    /// Forward search prompt (`/`).
    SearchForward,
    /// Backward search prompt (`?`).
    SearchBackward,
    /// Ex prompt from visual mode — pre-fills `'<,'>`.
    ExVisual,
}

impl CommandLinePrompt {
    /// All variants, in declaration order.
    pub const ALL: [Self; 4] = [
        Self::Ex,
        Self::SearchForward,
        Self::SearchBackward,
        Self::ExVisual,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn command_line_prompt_all_no_duplicates() {
        let unique: HashSet<CommandLinePrompt> = CommandLinePrompt::ALL.iter().copied().collect();
        assert_eq!(
            unique.len(),
            CommandLinePrompt::ALL.len(),
            "Duplicate in CommandLinePrompt::ALL"
        );
    }
}
