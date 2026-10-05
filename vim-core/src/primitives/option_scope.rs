//! Option scoping types for per-buffer and per-window option overrides.
//!
//! Implements a cascade: local override → global, matching Vim's option
//! scoping semantics. Options can be global, buffer-local, window-local,
//! or global-with-local-fallback.

use ahash::AHashMap;
use compact_str::CompactString;

use super::VimOptions;

// ═══════════════════════════════════════════════════════════════════════════
// OptionScope
// ═══════════════════════════════════════════════════════════════════════════

/// The scope at which a Vim option lives.
///
/// Determines how the option participates in the local-override → global cascade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionScope {
    /// Option is purely global; per-buffer/window overrides are ignored.
    Global,
    /// Option is local to a buffer; falls back to global when no local override exists.
    LocalToBuffer,
    /// Option is local to a window; falls back to global when no local override exists.
    LocalToWindow,
    /// Option has both a global value and a window-local copy.
    ///
    /// The window-local copy is used when set; sentinel values (`-1` / `""`)
    /// mean "use the global value".
    GlobalOrLocalWindow,
    /// Option has both a global value and a buffer-local copy.
    ///
    /// The buffer-local copy is used when set; sentinel values (`-1` / `""`)
    /// mean "use the global value".
    GlobalOrLocalBuffer,
}

// ═══════════════════════════════════════════════════════════════════════════
// OptionKind
// ═══════════════════════════════════════════════════════════════════════════

/// The value shape of a Vim option, which decides what `:set` accepts for it.
///
/// Mirrors the distinctions Vim's `:set` makes (`:help set-option`): a
/// boolean takes `name`/`noname`/`name!`; a number takes `=`, `+=`, `-=` and
/// `^=` as arithmetic; a string takes `+=`/`^=`/`-=` as append, prepend and
/// remove; a flag list and a comma list add and remove whole flags or items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OptionKind {
    /// On or off (`ignorecase`).
    Bool,
    /// A number (`textwidth`).
    Number,
    /// A free-form string (`commentstring`).
    String,
    /// A string of single-letter flags (`formatoptions`).
    FlagList,
    /// A comma-separated list (`comments`, `backspace`).
    CommaList,
}

// ═══════════════════════════════════════════════════════════════════════════
// OptionId
// ═══════════════════════════════════════════════════════════════════════════

/// Identifier for each option tracked in [`VimOptions`].
///
/// `#[repr(u16)]` keeps the discriminant small for use as a hash map key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum OptionId {
    // ── Global scope ────────────────────────────────────────────────────
    /// `ignorecase` — ignore case in search patterns (global).
    IgnoreCase = 0,
    /// `smartcase` — override ignorecase when pattern contains uppercase (global).
    SmartCase = 1,
    /// `hlsearch` — highlight all search matches (global).
    HlSearch = 2,
    /// `incsearch` — show matches incrementally while typing (global).
    IncSearch = 3,
    /// `wrapscan` — searches wrap around end of file (global).
    WrapScan = 4,
    /// `gdefault` — `:s` substitutes globally by default (global).
    GDefault = 5,
    /// `clipboard` — clipboard integration mode (global).
    Clipboard = 6,
    /// `inccommand` — live substitute preview mode (global).
    IncCommand = 7,
    /// `timeoutlen` — mapping timeout in milliseconds (global).
    TimeoutLen = 8,
    /// `undolevels` — maximum number of undo levels (global).
    UndoLevels = 9,

    // ── LocalToBuffer ────────────────────────────────────────────────────
    /// `tabstop` — tab stop width in columns (local to buffer).
    TabStop = 10,
    /// `shiftwidth` — shift width for indent/outdent (local to buffer).
    ShiftWidth = 11,
    /// `expandtab` — expand tabs to spaces (local to buffer).
    ExpandTab = 12,
    /// `autoindent` — copy indent from current line on new line (local to buffer).
    AutoIndent = 13,
    /// `smartindent` — smart C-like autoindent (local to buffer).
    SmartIndent = 14,
    /// `commentstring` — comment string format, e.g. `"// %s"` (local to buffer).
    CommentString = 15,
    /// `iskeyword` — characters that form keywords (local to buffer).
    IsKeyword = 16,
    /// `textwidth` — maximum line width for formatting (local to buffer).
    TextWidth = 17,
    /// `softtabstop` — number of columns for Tab in insert mode (local to buffer).
    SoftTabStop = 29,
    /// `formatoptions`: flags that control automatic formatting (local to buffer).
    FormatOptions = 30,
    /// `comments`: comment leaders recognized when formatting (local to buffer).
    Comments = 31,

    // ── LocalToWindow ────────────────────────────────────────────────────
    /// `scrolloff` — minimum lines to keep above/below cursor (local to window).
    ScrollOff = 18,
    /// `number` — show absolute line numbers (local to window).
    Number = 19,
    /// `relativenumber` — show relative line numbers (local to window).
    RelativeNumber = 20,

    // ── GlobalOrLocalWindow ──────────────────────────────────────────────
    /// `sidescrolloff` — minimum columns to keep left/right of cursor (global-or-local window).
    SideScrollOff = 21,
    /// `virtualedit` — where virtual editing is allowed (global-or-local window).
    VirtualEdit = 22,
    /// `selection` — selection behavior: inclusive, exclusive, or old (global-or-local window).
    Selection = 23,

    // ── GlobalOrLocalBuffer ──────────────────────────────────────────────
    /// `backspace` — what backspace can delete over (global-or-local buffer).
    Backspace = 24,
    /// `whichwrap` — which keys wrap to next/previous line (global-or-local buffer).
    WhichWrap = 25,

    // ── Global (search options) ──────────────────────────────────────────
    /// `visualstar` — in Visual mode, `*` and `#` search for selected text (global).
    VisualStar = 26,

    /// `undoautogroupms` -- time window for automatic undo grouping in milliseconds (global).
    UndoAutoGroupMs = 27,

    /// `belloff` — suppress bell emissions (global).
    BellOff = 28,
}

impl OptionId {
    /// The value shape of this option.
    #[must_use]
    pub const fn kind(self) -> OptionKind {
        match self {
            Self::IgnoreCase
            | Self::SmartCase
            | Self::HlSearch
            | Self::IncSearch
            | Self::WrapScan
            | Self::GDefault
            | Self::ExpandTab
            | Self::AutoIndent
            | Self::SmartIndent
            | Self::Number
            | Self::RelativeNumber
            | Self::VisualStar
            | Self::BellOff => OptionKind::Bool,

            Self::TimeoutLen
            | Self::UndoLevels
            | Self::TabStop
            | Self::ShiftWidth
            | Self::TextWidth
            | Self::ScrollOff
            | Self::SideScrollOff
            | Self::UndoAutoGroupMs
            | Self::SoftTabStop => OptionKind::Number,

            Self::CommentString | Self::Selection | Self::IncCommand => OptionKind::String,

            Self::FormatOptions => OptionKind::FlagList,

            Self::Clipboard
            | Self::IsKeyword
            | Self::VirtualEdit
            | Self::Backspace
            | Self::WhichWrap
            | Self::Comments => OptionKind::CommaList,
        }
    }

    /// The scope this option belongs to.
    #[must_use]
    pub const fn scope(self) -> OptionScope {
        match self {
            Self::IgnoreCase
            | Self::SmartCase
            | Self::HlSearch
            | Self::IncSearch
            | Self::WrapScan
            | Self::GDefault
            | Self::Clipboard
            | Self::IncCommand
            | Self::TimeoutLen
            | Self::UndoLevels
            | Self::VisualStar
            | Self::UndoAutoGroupMs
            | Self::BellOff => OptionScope::Global,

            Self::TabStop
            | Self::ShiftWidth
            | Self::ExpandTab
            | Self::AutoIndent
            | Self::SmartIndent
            | Self::CommentString
            | Self::IsKeyword
            | Self::TextWidth
            | Self::SoftTabStop
            | Self::FormatOptions
            | Self::Comments => OptionScope::LocalToBuffer,

            Self::ScrollOff | Self::Number | Self::RelativeNumber => OptionScope::LocalToWindow,

            Self::SideScrollOff | Self::VirtualEdit | Self::Selection => {
                OptionScope::GlobalOrLocalWindow
            }

            Self::Backspace | Self::WhichWrap => OptionScope::GlobalOrLocalBuffer,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// OptionValue
// ═══════════════════════════════════════════════════════════════════════════

/// The runtime value of a Vim option.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionValue {
    /// Boolean option (e.g. `ignorecase`, `number`).
    Bool(bool),
    /// Non-negative integer option (e.g. `tabstop`, `scrolloff`).
    Unsigned(usize),
    /// Signed integer option; `-1` is the conventional sentinel meaning "inherit global".
    Signed(i64),
    /// String option (e.g. `clipboard`, `iskeyword`). `""` is the conventional sentinel.
    Str(CompactString),
}

// ═══════════════════════════════════════════════════════════════════════════
// OptionOverrides
// ═══════════════════════════════════════════════════════════════════════════

/// A set of option overrides keyed by [`OptionId`].
///
/// Used to represent the per-buffer or per-window option layer in the
/// local-override → global cascade.
#[derive(Debug, Clone, Default)]
pub struct OptionOverrides {
    map: AHashMap<OptionId, OptionValue>,
}

impl OptionOverrides {
    /// Create an empty override set.
    #[must_use]
    pub fn new() -> Self {
        Self {
            map: AHashMap::new(),
        }
    }

    /// Get the override value for `id`, if any.
    #[must_use]
    pub fn get(&self, id: OptionId) -> Option<&OptionValue> {
        self.map.get(&id)
    }

    /// Set (or replace) the override value for `id`.
    pub fn set(&mut self, id: OptionId, value: OptionValue) {
        self.map.insert(id, value);
    }

    /// Remove the override for `id`. Returns the old value, if any.
    pub fn remove(&mut self, id: OptionId) -> Option<OptionValue> {
        self.map.remove(&id)
    }

    /// Clear all overrides.
    pub fn clear(&mut self) {
        self.map.clear();
    }

    /// Returns `true` when there are no overrides.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Sentinel detection
// ═══════════════════════════════════════════════════════════════════════════

/// Returns `true` when `value` is a sentinel meaning "fall back to global".
///
/// Sentinels are `Signed(-1)` and `Str("")`.
#[must_use]
pub fn is_sentinel(value: &OptionValue) -> bool {
    match value {
        OptionValue::Signed(-1) => true,
        OptionValue::Str(s) => s.is_empty(),
        _ => false,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// resolve_option
// ═══════════════════════════════════════════════════════════════════════════

/// Resolve the effective value of `id` using the local-override → global cascade.
///
/// # Cascade rules
///
/// | Scope                | Resolution |
/// |----------------------|------------|
/// | `Global`             | Always returns the global value. |
/// | `LocalToBuffer`      | Buffer override if present, else global. |
/// | `LocalToWindow`      | Window override if present, else global. |
/// | `GlobalOrLocalBuffer`| Buffer override unless it is a sentinel or absent, else global. |
/// | `GlobalOrLocalWindow`| Window override unless it is a sentinel or absent, else global. |
#[must_use]
pub fn resolve_option(
    id: OptionId,
    global: &VimOptions,
    buffer_overrides: Option<&OptionOverrides>,
    window_overrides: Option<&OptionOverrides>,
) -> OptionValue {
    match id.scope() {
        OptionScope::Global => global.get_option(id),

        OptionScope::LocalToBuffer => buffer_overrides
            .and_then(|ov| ov.get(id))
            .cloned()
            .unwrap_or_else(|| global.get_option(id)),

        OptionScope::LocalToWindow => window_overrides
            .and_then(|ov| ov.get(id))
            .cloned()
            .unwrap_or_else(|| global.get_option(id)),

        OptionScope::GlobalOrLocalBuffer => {
            let local = buffer_overrides.and_then(|ov| ov.get(id));
            match local {
                Some(v) if !is_sentinel(v) => v.clone(),
                _ => global.get_option(id),
            }
        }

        OptionScope::GlobalOrLocalWindow => {
            let local = window_overrides.and_then(|ov| ov.get(id));
            match local {
                Some(v) if !is_sentinel(v) => v.clone(),
                _ => global.get_option(id),
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── OptionId::scope ──────────────────────────────────────────────────

    #[test]
    fn test_scope_global_variants() {
        assert_eq!(OptionId::IgnoreCase.scope(), OptionScope::Global);
        assert_eq!(OptionId::SmartCase.scope(), OptionScope::Global);
        assert_eq!(OptionId::HlSearch.scope(), OptionScope::Global);
        assert_eq!(OptionId::IncSearch.scope(), OptionScope::Global);
        assert_eq!(OptionId::WrapScan.scope(), OptionScope::Global);
        assert_eq!(OptionId::GDefault.scope(), OptionScope::Global);
        assert_eq!(OptionId::Clipboard.scope(), OptionScope::Global);
        assert_eq!(OptionId::IncCommand.scope(), OptionScope::Global);
        assert_eq!(OptionId::TimeoutLen.scope(), OptionScope::Global);
        assert_eq!(OptionId::UndoLevels.scope(), OptionScope::Global);
        assert_eq!(OptionId::UndoAutoGroupMs.scope(), OptionScope::Global);
        assert_eq!(OptionId::BellOff.scope(), OptionScope::Global);
    }

    #[test]
    fn test_scope_local_to_buffer_variants() {
        assert_eq!(OptionId::TabStop.scope(), OptionScope::LocalToBuffer);
        assert_eq!(OptionId::ShiftWidth.scope(), OptionScope::LocalToBuffer);
        assert_eq!(OptionId::ExpandTab.scope(), OptionScope::LocalToBuffer);
        assert_eq!(OptionId::AutoIndent.scope(), OptionScope::LocalToBuffer);
        assert_eq!(OptionId::SmartIndent.scope(), OptionScope::LocalToBuffer);
        assert_eq!(OptionId::CommentString.scope(), OptionScope::LocalToBuffer);
        assert_eq!(OptionId::IsKeyword.scope(), OptionScope::LocalToBuffer);
        assert_eq!(OptionId::TextWidth.scope(), OptionScope::LocalToBuffer);
        assert_eq!(OptionId::SoftTabStop.scope(), OptionScope::LocalToBuffer);
        assert_eq!(OptionId::FormatOptions.scope(), OptionScope::LocalToBuffer);
        assert_eq!(OptionId::Comments.scope(), OptionScope::LocalToBuffer);
    }

    #[test]
    fn test_scope_local_to_window_variants() {
        assert_eq!(OptionId::ScrollOff.scope(), OptionScope::LocalToWindow);
        assert_eq!(OptionId::Number.scope(), OptionScope::LocalToWindow);
        assert_eq!(OptionId::RelativeNumber.scope(), OptionScope::LocalToWindow);
    }

    #[test]
    fn test_scope_global_or_local_window_variants() {
        assert_eq!(
            OptionId::SideScrollOff.scope(),
            OptionScope::GlobalOrLocalWindow
        );
        assert_eq!(
            OptionId::VirtualEdit.scope(),
            OptionScope::GlobalOrLocalWindow
        );
        assert_eq!(
            OptionId::Selection.scope(),
            OptionScope::GlobalOrLocalWindow
        );
    }

    #[test]
    fn test_scope_global_or_local_buffer_variants() {
        assert_eq!(
            OptionId::Backspace.scope(),
            OptionScope::GlobalOrLocalBuffer
        );
        assert_eq!(
            OptionId::WhichWrap.scope(),
            OptionScope::GlobalOrLocalBuffer
        );
    }

    // ── OptionId::kind ───────────────────────────────────────────────────

    #[test]
    fn test_kind_agrees_with_value_type() {
        // Every Bool kind holds a Bool value and every Number kind holds a
        // number, so `:set` can trust the kind when parsing a value.
        let defaults = VimOptions::default();
        for id in [
            OptionId::IgnoreCase,
            OptionId::SmartCase,
            OptionId::HlSearch,
            OptionId::IncSearch,
            OptionId::WrapScan,
            OptionId::GDefault,
            OptionId::Clipboard,
            OptionId::IncCommand,
            OptionId::TimeoutLen,
            OptionId::UndoLevels,
            OptionId::TabStop,
            OptionId::ShiftWidth,
            OptionId::ExpandTab,
            OptionId::AutoIndent,
            OptionId::SmartIndent,
            OptionId::CommentString,
            OptionId::IsKeyword,
            OptionId::TextWidth,
            OptionId::SoftTabStop,
            OptionId::FormatOptions,
            OptionId::Comments,
            OptionId::ScrollOff,
            OptionId::Number,
            OptionId::RelativeNumber,
            OptionId::SideScrollOff,
            OptionId::VirtualEdit,
            OptionId::Selection,
            OptionId::Backspace,
            OptionId::WhichWrap,
            OptionId::VisualStar,
            OptionId::UndoAutoGroupMs,
            OptionId::BellOff,
        ] {
            let value = defaults.get_option(id);
            let consistent = match id.kind() {
                OptionKind::Bool => matches!(value, OptionValue::Bool(_)),
                OptionKind::Number => {
                    matches!(value, OptionValue::Unsigned(_) | OptionValue::Signed(_))
                }
                OptionKind::String | OptionKind::FlagList | OptionKind::CommaList => {
                    matches!(value, OptionValue::Str(_))
                }
            };
            assert!(consistent, "{id:?} kind {:?} vs {value:?}", id.kind());
        }
    }

    #[test]
    fn test_formatting_option_kinds() {
        assert_eq!(OptionId::FormatOptions.kind(), OptionKind::FlagList);
        assert_eq!(OptionId::Comments.kind(), OptionKind::CommaList);
        assert_eq!(OptionId::TextWidth.kind(), OptionKind::Number);
        assert_eq!(OptionId::CommentString.kind(), OptionKind::String);
    }

    // ── is_sentinel ──────────────────────────────────────────────────────

    #[test]
    fn test_is_sentinel_signed_minus_one() {
        assert!(is_sentinel(&OptionValue::Signed(-1)));
    }

    #[test]
    fn test_is_sentinel_empty_str() {
        assert!(is_sentinel(&OptionValue::Str(CompactString::new_inline(
            ""
        ))));
    }

    #[test]
    fn test_is_sentinel_non_sentinel_values() {
        assert!(!is_sentinel(&OptionValue::Signed(0)));
        assert!(!is_sentinel(&OptionValue::Signed(1)));
        assert!(!is_sentinel(&OptionValue::Signed(-2)));
        assert!(!is_sentinel(&OptionValue::Unsigned(0)));
        assert!(!is_sentinel(&OptionValue::Unsigned(5)));
        assert!(!is_sentinel(&OptionValue::Bool(true)));
        assert!(!is_sentinel(&OptionValue::Bool(false)));
        assert!(!is_sentinel(&OptionValue::Str(CompactString::new_inline(
            "foo"
        ))));
    }

    // ── OptionOverrides ──────────────────────────────────────────────────

    #[test]
    fn test_option_overrides_new_is_empty() {
        let ov = OptionOverrides::new();
        assert!(ov.is_empty());
        assert!(ov.get(OptionId::TabStop).is_none());
    }

    #[test]
    fn test_option_overrides_default_is_empty() {
        let ov = OptionOverrides::default();
        assert!(ov.is_empty());
    }

    #[test]
    fn test_option_overrides_set_and_get() {
        let mut ov = OptionOverrides::new();
        ov.set(OptionId::TabStop, OptionValue::Unsigned(2));
        assert!(!ov.is_empty());
        assert_eq!(ov.get(OptionId::TabStop), Some(&OptionValue::Unsigned(2)));
    }

    #[test]
    fn test_option_overrides_set_replaces() {
        let mut ov = OptionOverrides::new();
        ov.set(OptionId::TabStop, OptionValue::Unsigned(2));
        ov.set(OptionId::TabStop, OptionValue::Unsigned(8));
        assert_eq!(ov.get(OptionId::TabStop), Some(&OptionValue::Unsigned(8)));
    }

    #[test]
    fn test_option_overrides_remove() {
        let mut ov = OptionOverrides::new();
        ov.set(OptionId::TabStop, OptionValue::Unsigned(2));
        let old = ov.remove(OptionId::TabStop);
        assert_eq!(old, Some(OptionValue::Unsigned(2)));
        assert!(ov.is_empty());
        assert!(ov.get(OptionId::TabStop).is_none());
    }

    #[test]
    fn test_option_overrides_remove_missing_returns_none() {
        let mut ov = OptionOverrides::new();
        assert!(ov.remove(OptionId::TabStop).is_none());
    }

    #[test]
    fn test_option_overrides_clear() {
        let mut ov = OptionOverrides::new();
        ov.set(OptionId::TabStop, OptionValue::Unsigned(2));
        ov.set(OptionId::ExpandTab, OptionValue::Bool(false));
        ov.clear();
        assert!(ov.is_empty());
        assert!(ov.get(OptionId::TabStop).is_none());
        assert!(ov.get(OptionId::ExpandTab).is_none());
    }

    #[test]
    fn test_option_overrides_get_missing_returns_none() {
        let ov = OptionOverrides::new();
        assert!(ov.get(OptionId::Number).is_none());
    }

    #[test]
    fn test_option_overrides_multiple_keys() {
        let mut ov = OptionOverrides::new();
        ov.set(OptionId::TabStop, OptionValue::Unsigned(2));
        ov.set(OptionId::Number, OptionValue::Bool(true));
        ov.set(
            OptionId::Clipboard,
            OptionValue::Str(CompactString::new_inline("unnamed")),
        );

        assert_eq!(ov.get(OptionId::TabStop), Some(&OptionValue::Unsigned(2)));
        assert_eq!(ov.get(OptionId::Number), Some(&OptionValue::Bool(true)));
        assert_eq!(
            ov.get(OptionId::Clipboard),
            Some(&OptionValue::Str(CompactString::new_inline("unnamed")))
        );
    }
}
