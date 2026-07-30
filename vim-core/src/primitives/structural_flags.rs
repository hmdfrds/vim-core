//! Flags for structural regular expression commands (`:sx`, `:sy`).

/// Flags controlling structural regex command execution.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StructuralFlags {
    /// Show a preview of changes before applying.
    pub preview: bool,
    /// Confirm each change interactively.
    pub confirm: bool,
    /// Process matches in reverse order.
    pub reverse: bool,
}
