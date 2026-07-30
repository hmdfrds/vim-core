//! Mapping attribute flags.
//!
//! Consolidates the `<nowait>`, `<silent>`, and `<expr>` boolean attributes
//! of a Vim key mapping into a single struct. This eliminates boolean
//! blindness at call sites (e.g., `false, false, true, false` → `MappingFlags { expr: true, ..default() }`).
//!
//! The `noremap` / recursive distinction is handled separately by
//! [`MappingKind`](super::MappingKind), which predates this struct.

/// Boolean attribute flags for a key mapping.
///
/// These correspond to Vim's `<nowait>`, `<silent>`, and `<expr>` map-command
/// modifiers. The recursive/non-recursive distinction is represented
/// separately by [`MappingKind`](super::MappingKind).
///
/// # Default
///
/// All flags default to `false`, matching a bare `:map lhs rhs` with no
/// special modifiers.
///
/// # Examples
///
/// ```ignore
/// use vim_core::keymap::MappingFlags;
///
/// // Bare mapping — no special flags.
/// let flags = MappingFlags::default();
/// assert!(!flags.nowait);
/// assert!(!flags.silent);
/// assert!(!flags.expr);
///
/// // Silent expression mapping.
/// let flags = MappingFlags { silent: true, expr: true, ..MappingFlags::default() };
/// assert!(flags.silent);
/// assert!(flags.expr);
/// assert!(!flags.nowait);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MappingFlags {
    /// If `true`, the mapping fires immediately even when it is a prefix of
    /// a longer mapping (no timeout wait needed). Corresponds to `<nowait>`.
    pub nowait: bool,
    /// If `true`, `ShowMessage` effects are suppressed during the mapping's
    /// execution. `ShowError` effects are NOT suppressed (errors are always
    /// shown, matching Vim behavior). Corresponds to `<silent>`.
    pub silent: bool,
    /// If `true`, the RHS is an expression to be evaluated by the host at
    /// expansion time rather than a static key sequence. Corresponds to `<expr>`.
    pub expr: bool,
    /// If `true`, this mapping is `<unique>` — defining a mapping with the
    /// same LHS from a **different** owner is an error (E227). Overwriting
    /// from the **same** owner is allowed.
    pub unique: bool,
    /// If `true`, this mapping was defined with `<script>` — remapping during
    /// expansion is restricted to mappings from the same [`MappingOwner`].
    /// Mappings from other owners are not considered during re-expansion.
    ///
    /// [`MappingOwner`]: super::MappingOwner
    pub script_local: bool,
}

impl MappingFlags {
    /// All flags off (same as `Default`).
    pub const NONE: Self = Self {
        nowait: false,
        silent: false,
        expr: false,
        unique: false,
        script_local: false,
    };

    /// Whether any flag is set.
    #[inline]
    #[must_use]
    pub const fn any(self) -> bool {
        self.nowait || self.silent || self.expr || self.unique || self.script_local
    }
}
