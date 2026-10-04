//! Vim options for the engine.
//!
//! `VimOptions` holds user-configurable settings that affect engine behavior.
//! Lives in `primitives/` so all layers can reference the type without
//! violating the architecture's downward-only dependency rule.

use compact_str::CompactString;
use smallvec::SmallVec;
use std::fmt;
use std::sync::Arc;

use super::byte_delta;
use super::clipboard_mode::UseSystemClipboard;
use super::comments::{CommentSpec, DEFAULT_COMMENTS};
use super::cursor_style::CursorShape;
use super::format_flags::FormatFlags;
use super::option_scope::{is_sentinel, OptionId, OptionOverrides, OptionScope, OptionValue};
use super::subword_config::SubwordConfig;
use super::word_char_set::WordCharSet;

/// A single auto-pair: opener and closer characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Pair {
    /// The opening character (e.g., `(`).
    pub open: char,
    /// The closing character (e.g., `)`).
    pub close: char,
}

impl Pair {
    /// True if opener and closer are the same character (quotes).
    #[must_use]
    pub const fn is_same_char(&self) -> bool {
        self.open as u32 == self.close as u32
    }
}

/// Auto-pair configuration.
///
/// When present in `VimOptions`, the engine handles auto-pairing.
/// When `None`, auto-pairing is disabled (host handles it).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AutoPairs {
    /// The configured pairs.
    pub pairs: SmallVec<[Pair; 8]>,
}

impl Default for AutoPairs {
    /// Default pairs: `()`, `[]`, `{}`, `''`, `""`, `` ` ` ``.
    fn default() -> Self {
        Self {
            pairs: SmallVec::from_slice(&[
                Pair {
                    open: '(',
                    close: ')',
                },
                Pair {
                    open: '[',
                    close: ']',
                },
                Pair {
                    open: '{',
                    close: '}',
                },
                Pair {
                    open: '\'',
                    close: '\'',
                },
                Pair {
                    open: '"',
                    close: '"',
                },
                Pair {
                    open: '`',
                    close: '`',
                },
            ]),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// SelectionMode enum
// ═══════════════════════════════════════════════════════════════════════════

/// Vim `selection` option: determines how the end of a visual selection is handled.
///
/// - `Inclusive` (default): the character under the cursor IS part of the selection.
/// - `Exclusive`: the character under the cursor is NOT part of the selection.
/// - `Old`: legacy Vim behavior, treated as inclusive for operator ranges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum SelectionMode {
    /// The character under the cursor is included in the selection (default).
    #[default]
    Inclusive,
    /// The character under the cursor is excluded from the selection.
    Exclusive,
    /// Legacy Vim behavior; treated as inclusive for operator ranges.
    Old,
}

impl SelectionMode {
    /// Parse from a Vim option string value.
    ///
    /// Returns `None` for unrecognized values (caller decides whether to
    /// ignore or report the error).
    #[must_use]
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "inclusive" => Some(Self::Inclusive),
            "exclusive" => Some(Self::Exclusive),
            "old" => Some(Self::Old),
            _ => None,
        }
    }

    /// The canonical Vim option string for this mode.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inclusive => "inclusive",
            Self::Exclusive => "exclusive",
            Self::Old => "old",
        }
    }
}

impl fmt::Display for SelectionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// IncCommandMode enum
// ═══════════════════════════════════════════════════════════════════════════

/// Vim `inccommand` option: controls live substitute preview behavior.
///
/// - `Off`: no preview (equivalent to `set inccommand=`).
/// - `NoSplit`: show preview highlights in the current window.
/// - `Split`: show preview highlights and a preview split window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum IncCommandMode {
    /// No inccommand preview (empty string).
    Off,
    /// Preview in current window (`"nosplit"`).
    #[default]
    NoSplit,
    /// Preview with split window (`"split"`).
    Split,
}

impl IncCommandMode {
    /// Parse from a Vim option string value.
    ///
    /// Returns `None` for unrecognized values.
    #[must_use]
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "" => Some(Self::Off),
            "nosplit" => Some(Self::NoSplit),
            "split" => Some(Self::Split),
            _ => None,
        }
    }

    /// The canonical Vim option string for this mode.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "",
            Self::NoSplit => "nosplit",
            Self::Split => "split",
        }
    }

    /// Whether preview is enabled (any non-Off variant).
    #[must_use]
    pub const fn is_enabled(self) -> bool {
        !matches!(self, Self::Off)
    }
}

impl fmt::Display for IncCommandMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// WordEraseStyle enum
// ═══════════════════════════════════════════════════════════════════════════

/// Controls Ctrl-W word-erase behavior in insert mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum WordEraseStyle {
    /// Standard vi Ctrl-W behavior (default).
    #[default]
    Vi,
    /// Alternative word-erase (altwerase) behavior.
    AltWerase,
    /// TTY word-erase (ttywerase) behavior.
    TtyWerase,
}

// ═══════════════════════════════════════════════════════════════════════════
// VimOptions struct
// ═══════════════════════════════════════════════════════════════════════════

/// User-configurable Vim options.
///
/// All fields have sensible defaults matching Vim's common configuration.
/// Invariants: `tabstop` and `shiftwidth` are always >= 1 (clamped at construction).
/// Use `VimOptions::default()` for standard defaults, then `set_*()` to override.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[allow(
    clippy::struct_excessive_bools,
    reason = "Vim options are independent orthogonal boolean flags (expandtab, smartindent, etc.)"
)]
pub struct VimOptions {
    // ── Indentation ──────────────────────────────────────────────────────
    /// Tab stop width in columns. Minimum 1.
    tabstop: usize,
    /// Shift width for indent/outdent. Minimum 1.
    shiftwidth: usize,
    /// Whether to expand tabs to spaces.
    expandtab: bool,
    /// Copy indent from current line when starting a new line.
    autoindent: bool,
    /// Smart autoindent (recognizes C-like syntax).
    smartindent: bool,

    // ── Search ───────────────────────────────────────────────────────────
    /// Ignore case in search patterns.
    ignorecase: bool,
    /// Override `ignorecase` when pattern contains uppercase.
    smartcase: bool,
    /// Highlight all search matches.
    hlsearch: bool,
    /// Show matches incrementally while typing search.
    incsearch: bool,
    /// Searches wrap around the end of the file.
    wrapscan: bool,
    /// In Visual mode, `*` and `#` search for the selected text instead
    /// of the word under cursor.
    visualstar: bool,

    // ── Scrolling ────────────────────────────────────────────────────────
    /// Minimum lines to keep above/below cursor when scrolling.
    scrolloff: usize,
    /// Minimum columns to keep left/right of cursor when scrolling.
    sidescrolloff: usize,

    // ── Display ──────────────────────────────────────────────────────────
    /// Show absolute line numbers.
    number: bool,
    /// Show relative line numbers.
    relativenumber: bool,

    // ── Formatting ───────────────────────────────────────────────────────
    /// Maximum line width for formatting (gq). 0 means no limit.
    textwidth: usize,
    /// Format options string (e.g. "tcqj"). Controls auto-formatting behavior.
    /// - `t`: auto-wrap text using textwidth
    /// - `c`: auto-wrap comments using textwidth
    /// Neovim default: "tcqj" (Vim's is "tcq")
    formatoptions: CompactString,
    /// Parsed flags from `formatoptions`. Rebuilt when formatoptions changes.
    #[cfg_attr(feature = "serde", serde(skip))]
    format_flags: FormatFlags,

    // ── Mapping ──────────────────────────────────────────────────────────
    /// Timeout for ambiguous mapping prefixes in milliseconds.
    timeoutlen_ms: u32,

    // ── Navigation behavior ──────────────────────────────────────────────
    /// Which keys wrap to next/prev line (e.g. "b,s,<,>,[,]").
    whichwrap: CompactString,
    /// What backspace can delete over ("indent,eol,start").
    backspace: CompactString,
    /// Where virtual editing is allowed ("", "block", "insert", "all", "onemore").
    virtualedit: CompactString,

    // ── Selection ────────────────────────────────────────────────────────
    /// Selection behavior: inclusive, exclusive, or old.
    selection: SelectionMode,

    // ── Clipboard ────────────────────────────────────────────────────────
    /// Clipboard integration: "", "unnamed", "unnamedplus".
    clipboard: CompactString,

    // ── Commenting ────────────────────────────────────────────────────────
    /// Comment string format (e.g., `"// %s"`, `"# %s"`).
    /// `%s` is replaced with the line content.
    commentstring: CompactString,
    /// Comment leaders for formatting (Vim `comments`), e.g. `"b:#,://"`.
    comments: CompactString,
    /// Parsed `comments`. Rebuilt when comments changes; shared so that
    /// resolving options per buffer does not copy the parts.
    #[cfg_attr(feature = "serde", serde(skip))]
    comment_spec: Arc<CommentSpec>,

    // ── Command-line preview ───────────────────────────────────────────────
    /// Live substitute preview mode.
    /// When enabled, `:s` shows preview highlights as you type.
    inccommand: IncCommandMode,

    // ── Substitution ────────────────────────────────────────────────────
    /// When set, `:s` substitutes globally by default; the `g` flag inverts.
    gdefault: bool,

    // ── Word boundaries ──────────────────────────────────────────────────
    /// Characters that are part of keywords (Vim iskeyword format).
    iskeyword: CompactString,
    /// Precomputed bitmap from `iskeyword`. Rebuilt when iskeyword changes.
    #[cfg_attr(feature = "serde", serde(skip))]
    word_char_set: WordCharSet,

    // ── Undo history ─────────────────────────────────────────────────────
    /// Maximum number of undo levels. `None` means unlimited (default).
    /// When set to `Some(N)`, the undo tree prunes the oldest branches
    /// whenever the live node count exceeds N.
    undolevels: Option<usize>,

    /// Auto-pair configuration. `None` means disabled (host handles it).
    auto_pairs: Option<AutoPairs>,

    // ── Langmap ─────────────────────────────────────────────────────────
    /// Raw langmap string from `:set langmap=...`. Parsed into LangmapTable on the engine.
    langmap: CompactString,
    /// Whether langmap applies to characters from mapping expansion.
    /// Default: false (Neovim convention — safer, prevents double-translation).
    langremap: bool,

    // ── Subword motions ─────────────────────────────────────────────────
    /// Subword motion boundary detection configuration.
    subword_config: SubwordConfig,

    // ── System clipboard ────────────────────────────────────────────────
    /// When the engine should auto-sync with the system clipboard.
    use_system_clipboard: UseSystemClipboard,

    // ── Macro safety ────────────────────────────────────────────────────
    /// Maximum number of effects a macro replay may produce before the
    /// engine aborts the replay. Prevents runaway recursive macros from
    /// locking up the host.
    max_macro_effects: usize,

    // ── Cursor shape overrides ─────────────────────────────────────────
    /// Per-mode cursor shape overrides. Indexed by [`mode_to_override_index`](super::cursor_style::mode_to_override_index).
    /// `None` entries (or out-of-bounds indices) fall back to the built-in default.
    /// Empty vec means no overrides (all modes use defaults).
    cursor_shape_overrides: Vec<Option<CursorShape>>,

    /// Time window (in milliseconds) for automatic undo grouping.
    /// `None` (default) disables auto-grouping.
    undo_auto_group_ms: Option<u32>,

    // ── Multiline find ──────────────────────────────────────────────────
    /// When true, `f`/`F`/`t`/`T` character motions cross line boundaries.
    /// Default: false (standard Vim behavior — single-line only).
    multiline_find: bool,
    /// Maximum lines to cross when `multiline_find` is enabled.
    /// Default: 5. Setting to 0 is equivalent to `multiline_find=false`.
    multiline_find_range: usize,

    // ── Reporting ────────────────────────────────────────────────────────
    /// Minimum number of lines changed before a message is shown.
    /// Equivalent to Vim's `report` option. Default: 2.
    report: u32,

    // ── Insert-mode word erase ───────────────────────────────────────────
    /// Controls Ctrl-W word-erase behavior in insert mode.
    word_erase_style: WordEraseStyle,

    // ── Match highlighting ────────────────────────────────────────────────
    /// When true, briefly jump to the matching bracket when one is inserted.
    showmatch: bool,
    /// Tenths of a second to show the matching bracket (used with `showmatch`).
    /// Clamped to 100 at most. Default: 5.
    matchtime: u8,

    // ── Line wrapping ─────────────────────────────────────────────────────
    /// Number of columns from the right margin at which to soft-wrap lines.
    /// 0 means no wrap margin. Default: 0.
    wrapmargin: u32,

    // ── Quote escape ─────────────────────────────────────────────────────
    /// Characters used to escape the quote character in quote text objects.
    /// Default: `"\\"` (single backslash). Buffer-local.
    quoteescape: CompactString,

    // ── Ed compatibility ──────────────────────────────────────────────────
    /// When true, substitute flags are persistent across `:s` calls.
    edcompatible: bool,

    // ── Key timeout (terminal) ────────────────────────────────────────────
    /// Timeout for key code sequences in milliseconds. -1 means use `timeoutlen`.
    ttimeoutlen_ms: i32,

    // ── Sneak mode ──────────────────────────────────────────────────────
    /// When true, `s`/`S` become two-character sneak motions (cross-line find).
    /// When false (default), `s`/`S` behave as substitute (normal Vim behavior).
    sneak_mode: bool,

    // ── Default mode ────────────────────────────────────────────────────
    /// Mode the engine starts in and returns to after `emergency_reset()`.
    /// Default: `Mode::Normal`.
    default_mode: super::Mode,

    // ── Yank highlight ──────────────────────────────────────────────────
    // ── Bell control ─────────────────────────────────────────────────
    /// When true, all bells are suppressed (`:set belloff=all`).
    /// Default: false (bells are emitted normally).
    belloff: bool,

    /// Duration in milliseconds for the yank highlight flash.
    /// Default: 150ms.
    yank_highlight_duration_ms: u64,

    /// When true, unmatched Alt-modified keys are decomposed to Esc + base key.
    /// Enable for terminal hosts where Alt sends Esc prefix.
    /// Default: false (GUI hosts handle Alt natively).
    alt_sends_esc: bool,

    // ── Soft tab stop ────────────────────────────────────────────────────
    /// Number of columns for a Tab key press in insert mode.
    /// - `0` means use `tabstop` (default).
    /// - `-1` means use `shiftwidth`.
    /// - `>0` means use this explicit column count.
    softtabstop: i32,

    /// When true, sentence boundaries require two spaces after `.`/`!`/`?`.
    /// Prevents false boundaries in "Dr. Smith" or "U.S. Army".
    /// Default: false (Neovim default — single space is sufficient).
    cpo_j: bool,
}

impl VimOptions {
    // ── Indentation getters ──────────────────────────────────────────────

    /// Tab stop width in columns. Always >= 1.
    #[inline]
    #[must_use]
    pub const fn tabstop(&self) -> usize {
        self.tabstop
    }

    /// Shift width for indent/outdent operations. Always >= 1.
    #[inline]
    #[must_use]
    pub const fn shiftwidth(&self) -> usize {
        self.shiftwidth
    }

    /// Whether tabs are expanded to spaces.
    #[inline]
    #[must_use]
    pub const fn expandtab(&self) -> bool {
        self.expandtab
    }

    /// Whether autoindent is enabled.
    #[inline]
    #[must_use]
    pub const fn autoindent(&self) -> bool {
        self.autoindent
    }

    /// Whether smartindent is enabled.
    #[inline]
    #[must_use]
    pub const fn smartindent(&self) -> bool {
        self.smartindent
    }

    // ── Search getters ───────────────────────────────────────────────────

    /// Whether to ignore case in search patterns.
    #[inline]
    #[must_use]
    pub const fn ignorecase(&self) -> bool {
        self.ignorecase
    }

    /// Whether to override ignorecase when pattern has uppercase.
    #[inline]
    #[must_use]
    pub const fn smartcase(&self) -> bool {
        self.smartcase
    }

    /// Whether to highlight all search matches.
    #[inline]
    #[must_use]
    pub const fn hlsearch(&self) -> bool {
        self.hlsearch
    }

    /// Whether to show matches incrementally while typing.
    #[inline]
    #[must_use]
    pub const fn incsearch(&self) -> bool {
        self.incsearch
    }

    /// Whether searches wrap around end of file.
    #[inline]
    #[must_use]
    pub const fn wrapscan(&self) -> bool {
        self.wrapscan
    }

    /// Whether `*` and `#` in Visual mode search for the selected text.
    #[inline]
    #[must_use]
    pub const fn visualstar(&self) -> bool {
        self.visualstar
    }

    // ── Scrolling getters ────────────────────────────────────────────────

    /// Minimum lines to keep above/below cursor.
    #[inline]
    #[must_use]
    pub const fn scrolloff(&self) -> usize {
        self.scrolloff
    }

    /// Minimum columns to keep left/right of cursor.
    #[inline]
    #[must_use]
    pub const fn sidescrolloff(&self) -> usize {
        self.sidescrolloff
    }

    // ── Display getters ──────────────────────────────────────────────────

    /// Whether absolute line numbers are shown.
    #[inline]
    #[must_use]
    pub const fn number(&self) -> bool {
        self.number
    }

    /// Whether relative line numbers are shown.
    #[inline]
    #[must_use]
    pub const fn relativenumber(&self) -> bool {
        self.relativenumber
    }

    // ── Formatting getters ───────────────────────────────────────────────

    /// Maximum line width for formatting.
    #[inline]
    #[must_use]
    pub const fn textwidth(&self) -> usize {
        self.textwidth
    }

    /// Format options string.
    #[inline]
    #[must_use]
    pub fn formatoptions(&self) -> &str {
        &self.formatoptions
    }

    /// Parsed `formatoptions` flags.
    #[inline]
    #[must_use]
    pub const fn format_flags(&self) -> FormatFlags {
        self.format_flags
    }

    /// Whether auto-format text wrapping is enabled (`t` in formatoptions).
    #[inline]
    #[must_use]
    pub const fn auto_format_text(&self) -> bool {
        self.format_flags.contains(FormatFlags::WRAP_TEXT)
    }

    // ── Mapping getters ──────────────────────────────────────────────────

    /// Mapping timeout in milliseconds.
    #[inline]
    #[must_use]
    pub const fn timeoutlen_ms(&self) -> u32 {
        self.timeoutlen_ms
    }

    // ── Navigation getters ───────────────────────────────────────────────

    /// Which keys wrap to next/previous line.
    #[inline]
    #[must_use]
    pub fn whichwrap(&self) -> &str {
        &self.whichwrap
    }

    /// What backspace can delete over.
    #[inline]
    #[must_use]
    pub fn backspace(&self) -> &str {
        &self.backspace
    }

    /// Where virtual editing is allowed.
    #[inline]
    #[must_use]
    pub fn virtualedit(&self) -> &str {
        &self.virtualedit
    }

    // ── Selection getters ────────────────────────────────────────────────

    /// Selection behavior mode as a string (for display / `:set` output).
    #[inline]
    #[must_use]
    pub const fn selection(&self) -> &'static str {
        self.selection.as_str()
    }

    /// Selection behavior mode as the typed enum.
    #[inline]
    #[must_use]
    pub const fn selection_mode(&self) -> SelectionMode {
        self.selection
    }

    /// Whether selection mode is exclusive.
    ///
    /// When true, `selection_to_operator_range` should NOT include the
    /// character under the cursor in the operator range. Single-character
    /// selections are always treated as inclusive (Neovim behavior).
    #[must_use]
    pub const fn selection_is_exclusive(&self) -> bool {
        matches!(self.selection, SelectionMode::Exclusive)
    }

    /// Whether selection mode is inclusive (`"inclusive"` or `"old"`).
    ///
    /// When true, the character under the cursor IS included in the operator range.
    /// This is the default Vim behavior.
    #[must_use]
    pub const fn selection_is_inclusive(&self) -> bool {
        !self.selection_is_exclusive()
    }

    // ── Clipboard getters ────────────────────────────────────────────────

    /// Clipboard integration mode.
    #[inline]
    #[must_use]
    pub fn clipboard(&self) -> &str {
        &self.clipboard
    }

    /// Whether `"unnamed"` is in the clipboard option.
    ///
    /// When true, unnamed register writes are mirrored to the system primary
    /// selection (`*` register / `CopyToClipboard` effect).
    #[must_use]
    pub fn clipboard_has_unnamed(&self) -> bool {
        has_csv_token(&self.clipboard, "unnamed")
    }

    /// Whether `"unnamedplus"` is in the clipboard option.
    ///
    /// When true, unnamed register writes are mirrored to the system clipboard
    /// (`+` register / `CopyToClipboard` effect).
    #[must_use]
    pub fn clipboard_has_unnamedplus(&self) -> bool {
        has_csv_token(&self.clipboard, "unnamedplus")
    }

    /// Comment string format.
    #[inline]
    #[must_use]
    pub fn commentstring(&self) -> &str {
        &self.commentstring
    }

    /// Comment leaders for formatting (Vim `comments`).
    #[inline]
    #[must_use]
    pub fn comments(&self) -> &str {
        &self.comments
    }

    /// Parsed `comments`, for matching comment leaders.
    #[inline]
    #[must_use]
    pub fn comment_spec(&self) -> &CommentSpec {
        &self.comment_spec
    }

    // ── Quote escape getter ──────────────────────────────────────────────

    /// Characters used to escape the quote character in quote text objects.
    /// Default: `"\\"` (single backslash). Buffer-local.
    #[inline]
    #[must_use]
    pub fn quoteescape(&self) -> &str {
        &self.quoteescape
    }

    // ── Word boundary getters ────────────────────────────────────────────

    /// Characters that are part of keywords.
    #[inline]
    #[must_use]
    pub fn iskeyword(&self) -> &str {
        &self.iskeyword
    }

    /// Precomputed word character set from `iskeyword`.
    ///
    /// Used by `CharClass::classify` for word boundary detection.
    #[inline]
    #[must_use]
    pub const fn word_char_set(&self) -> &WordCharSet {
        &self.word_char_set
    }

    // ── Command-line preview getters ────────────────────────────────────

    /// Live substitute preview mode as a string (for display / `:set` output).
    #[inline]
    #[must_use]
    pub const fn inccommand(&self) -> &'static str {
        self.inccommand.as_str()
    }

    /// Live substitute preview mode as the typed enum.
    #[inline]
    #[must_use]
    pub const fn inccommand_mode(&self) -> IncCommandMode {
        self.inccommand
    }

    /// Whether inccommand preview is enabled (non-Off value).
    #[inline]
    #[must_use]
    pub const fn inccommand_enabled(&self) -> bool {
        self.inccommand.is_enabled()
    }

    // ── Substitution getters ────────────────────────────────────────────

    /// Whether `:s` defaults to global replacement (`gdefault`).
    #[inline]
    #[must_use]
    pub const fn gdefault(&self) -> bool {
        self.gdefault
    }

    // ── Setters ──────────────────────────────────────────────────────────

    /// Set tab stop width. Clamped to minimum 1.
    #[inline]
    pub fn set_tabstop(&mut self, value: usize) {
        self.tabstop = value.max(1);
    }

    /// Set shift width. Clamped to minimum 1.
    #[inline]
    pub fn set_shiftwidth(&mut self, value: usize) {
        self.shiftwidth = value.max(1);
    }

    /// Set expandtab.
    #[inline]
    pub const fn set_expandtab(&mut self, value: bool) {
        self.expandtab = value;
    }

    /// Set autoindent.
    #[inline]
    pub const fn set_autoindent(&mut self, value: bool) {
        self.autoindent = value;
    }

    /// Set smartindent.
    #[inline]
    pub const fn set_smartindent(&mut self, value: bool) {
        self.smartindent = value;
    }

    /// Set ignorecase.
    #[inline]
    pub const fn set_ignorecase(&mut self, value: bool) {
        self.ignorecase = value;
    }

    /// Set smartcase.
    #[inline]
    pub const fn set_smartcase(&mut self, value: bool) {
        self.smartcase = value;
    }

    /// Set hlsearch.
    #[inline]
    pub const fn set_hlsearch(&mut self, value: bool) {
        self.hlsearch = value;
    }

    /// Set incsearch.
    #[inline]
    pub const fn set_incsearch(&mut self, value: bool) {
        self.incsearch = value;
    }

    /// Set wrapscan.
    #[inline]
    pub const fn set_wrapscan(&mut self, value: bool) {
        self.wrapscan = value;
    }

    /// Set visualstar.
    #[inline]
    pub const fn set_visualstar(&mut self, value: bool) {
        self.visualstar = value;
    }

    /// Set scrolloff.
    #[inline]
    pub const fn set_scrolloff(&mut self, value: usize) {
        self.scrolloff = value;
    }

    /// Set sidescrolloff.
    #[inline]
    pub const fn set_sidescrolloff(&mut self, value: usize) {
        self.sidescrolloff = value;
    }

    /// Set number.
    #[inline]
    pub const fn set_number(&mut self, value: bool) {
        self.number = value;
    }

    /// Set relativenumber.
    #[inline]
    pub const fn set_relativenumber(&mut self, value: bool) {
        self.relativenumber = value;
    }

    /// Set textwidth.
    #[inline]
    pub const fn set_textwidth(&mut self, value: usize) {
        self.textwidth = value;
    }

    /// Set formatoptions. Rebuilds the cached [`FormatFlags`].
    ///
    /// The value is stored as given. Characters that are not `formatoptions`
    /// flags are kept in the string but have no effect; `:set` rejects them
    /// with E539 before they get here.
    #[inline]
    pub fn set_formatoptions(&mut self, value: impl Into<CompactString>) {
        self.formatoptions = value.into();
        self.format_flags = FormatFlags::parse_lossy(&self.formatoptions);
    }

    /// Set mapping timeout in milliseconds.
    #[inline]
    pub const fn set_timeoutlen_ms(&mut self, value: u32) {
        self.timeoutlen_ms = value;
    }

    /// Set whichwrap.
    #[inline]
    pub fn set_whichwrap(&mut self, value: impl Into<CompactString>) {
        self.whichwrap = value.into();
    }

    /// Set quoteescape.
    #[inline]
    pub fn set_quoteescape(&mut self, value: impl Into<CompactString>) {
        self.quoteescape = value.into();
    }

    /// Set backspace. Invalid values are silently ignored.
    #[inline]
    pub fn set_backspace(&mut self, value: impl Into<CompactString>) {
        let v = value.into();
        let valid = matches!(v.as_str(), "" | "0" | "1" | "2" | "3")
            || v.split(',')
                .all(|t| matches!(t.trim(), "indent" | "eol" | "start" | "nostop"));
        if valid {
            self.backspace = v;
        }
    }

    /// Set virtualedit. Invalid values are silently ignored.
    #[inline]
    pub fn set_virtualedit(&mut self, value: impl Into<CompactString>) {
        let v = value.into();
        let valid = v.is_empty()
            || v.split(',')
                .all(|t| matches!(t.trim(), "block" | "insert" | "all" | "onemore" | "none"));
        if valid {
            self.virtualedit = v;
        }
    }

    /// Set selection mode from a string value. Invalid values are silently ignored.
    #[inline]
    pub fn set_selection(&mut self, value: &str) {
        if let Some(mode) = SelectionMode::from_str_opt(value) {
            self.selection = mode;
        }
    }

    /// Set selection mode directly from the enum.
    #[inline]
    pub const fn set_selection_mode(&mut self, mode: SelectionMode) {
        self.selection = mode;
    }

    /// Set clipboard mode. Invalid values are silently ignored.
    #[inline]
    pub fn set_clipboard(&mut self, value: impl Into<CompactString>) {
        let v = value.into();
        let valid = v.is_empty()
            || v.split(',')
                .all(|t| matches!(t.trim(), "unnamed" | "unnamedplus"));
        if valid {
            self.clipboard = v;
        }
    }

    /// Set commentstring.
    #[inline]
    pub fn set_commentstring(&mut self, value: impl Into<CompactString>) {
        self.commentstring = value.into();
    }

    /// Set comments. Rebuilds the cached [`CommentSpec`].
    ///
    /// The value is stored as given. Parts without a colon are skipped when
    /// matching; `:set` rejects a malformed value with E524, E525 or E539
    /// before it gets here.
    #[inline]
    pub fn set_comments(&mut self, value: impl Into<CompactString>) {
        self.comments = value.into();
        self.comment_spec = Arc::new(CommentSpec::parse_lossy(&self.comments));
    }

    /// Set iskeyword. Rebuilds the cached `WordCharSet` bitmap.
    #[inline]
    pub fn set_iskeyword(&mut self, value: impl Into<CompactString>) {
        self.iskeyword = value.into();
        self.word_char_set = WordCharSet::from_iskeyword(&self.iskeyword);
    }

    /// Set inccommand mode from a string value. Invalid values are silently ignored.
    #[inline]
    pub fn set_inccommand(&mut self, value: &str) {
        if let Some(mode) = IncCommandMode::from_str_opt(value) {
            self.inccommand = mode;
        }
    }

    /// Set inccommand mode directly from the enum.
    #[inline]
    pub const fn set_inccommand_mode(&mut self, mode: IncCommandMode) {
        self.inccommand = mode;
    }

    /// Set gdefault.
    #[inline]
    pub const fn set_gdefault(&mut self, value: bool) {
        self.gdefault = value;
    }

    /// Maximum undo levels. `None` means unlimited (default).
    #[inline]
    #[must_use]
    pub const fn undolevels(&self) -> Option<usize> {
        self.undolevels
    }

    /// Set the undo level limit. Pass `None` to disable limiting.
    #[inline]
    pub const fn set_undolevels(&mut self, value: Option<usize>) {
        self.undolevels = value;
    }

    /// Auto-pair configuration. `None` means auto-pairing is disabled.
    #[inline]
    #[must_use]
    pub const fn auto_pairs(&self) -> Option<&AutoPairs> {
        self.auto_pairs.as_ref()
    }

    /// Set auto-pair configuration. Pass `None` to disable.
    #[inline]
    pub fn set_auto_pairs(&mut self, value: Option<AutoPairs>) {
        self.auto_pairs = value;
    }

    // ── Langmap getters/setters ──────────────────────────────────────────

    /// Raw langmap string (e.g. `"аА-zZ,..."`). Empty means no translation.
    #[inline]
    #[must_use]
    pub fn langmap(&self) -> &str {
        &self.langmap
    }

    /// Set the raw langmap string.
    #[inline]
    pub fn set_langmap(&mut self, value: &str) {
        self.langmap = value.into();
    }

    /// Whether langmap applies to characters produced by mapping expansion.
    /// Default: `false` (Neovim convention — safer, prevents double-translation).
    #[inline]
    #[must_use]
    pub const fn langremap(&self) -> bool {
        self.langremap
    }

    /// Set langremap.
    #[inline]
    pub const fn set_langremap(&mut self, value: bool) {
        self.langremap = value;
    }

    // ── Subword motion getters/setters ──────────────────────────────────

    /// Subword motion boundary detection configuration.
    #[inline]
    #[must_use]
    pub const fn subword_config(&self) -> &SubwordConfig {
        &self.subword_config
    }

    /// Set subword motion configuration.
    #[inline]
    pub fn set_subword_config(&mut self, value: SubwordConfig) {
        self.subword_config = value;
    }

    // ── System clipboard getters/setters ────────────────────────────────

    /// When the engine should auto-sync with the system clipboard.
    #[inline]
    #[must_use]
    pub const fn use_system_clipboard(&self) -> UseSystemClipboard {
        self.use_system_clipboard
    }

    /// Set the system clipboard synchronization policy.
    #[inline]
    pub const fn set_use_system_clipboard(&mut self, value: UseSystemClipboard) {
        self.use_system_clipboard = value;
    }

    // ── Macro safety getters/setters ────────────────────────────────────

    /// Maximum effects a macro replay may produce before aborting.
    #[inline]
    #[must_use]
    pub const fn max_macro_effects(&self) -> usize {
        self.max_macro_effects
    }

    /// Set the macro effect limit.
    #[inline]
    pub const fn set_max_macro_effects(&mut self, value: usize) {
        self.max_macro_effects = value;
    }

    // ── Cursor shape override getters/setters ──────────────────────────

    /// Per-mode cursor shape overrides.
    ///
    /// Indexed by [`mode_to_override_index`](super::cursor_style::mode_to_override_index).
    /// An empty slice means all modes use the built-in default cursor shapes.
    #[inline]
    #[must_use]
    pub fn cursor_shape_overrides(&self) -> &[Option<CursorShape>] {
        &self.cursor_shape_overrides
    }

    /// Set per-mode cursor shape overrides.
    #[inline]
    pub fn set_cursor_shape_overrides(&mut self, value: Vec<Option<CursorShape>>) {
        self.cursor_shape_overrides = value;
    }

    /// Time window for automatic undo grouping, in milliseconds.
    #[inline]
    #[must_use]
    pub const fn undo_auto_group_ms(&self) -> Option<u32> {
        self.undo_auto_group_ms
    }

    /// Set the undo auto-grouping time window. `None` to disable.
    #[inline]
    pub const fn set_undo_auto_group_ms(&mut self, value: Option<u32>) {
        self.undo_auto_group_ms = value;
    }

    // ── Multiline find getters/setters ──────────────────────────────────

    /// Whether `f`/`F`/`t`/`T` character motions cross line boundaries.
    #[inline]
    #[must_use]
    pub const fn multiline_find(&self) -> bool {
        self.multiline_find
    }

    /// Set whether find motions cross line boundaries.
    #[inline]
    pub const fn set_multiline_find(&mut self, value: bool) {
        self.multiline_find = value;
    }

    /// Maximum lines to cross when `multiline_find` is enabled.
    #[inline]
    #[must_use]
    pub const fn multiline_find_range(&self) -> usize {
        self.multiline_find_range
    }

    /// Set the maximum multiline find range. Clamped to minimum 0.
    #[inline]
    pub const fn set_multiline_find_range(&mut self, value: usize) {
        self.multiline_find_range = value;
    }

    // ── Reporting getters/setters ────────────────────────────────────────

    /// Minimum lines changed before a message is shown (Vim `report` option).
    #[inline]
    #[must_use]
    pub const fn report(&self) -> u32 {
        self.report
    }

    /// Set the `report` option.
    #[inline]
    pub const fn set_report(&mut self, value: u32) {
        self.report = value;
    }

    // ── Insert-mode word erase getters/setters ───────────────────────────

    /// Ctrl-W word-erase behavior in insert mode.
    #[inline]
    #[must_use]
    pub const fn word_erase_style(&self) -> WordEraseStyle {
        self.word_erase_style
    }

    /// Set the word-erase style.
    #[inline]
    pub const fn set_word_erase_style(&mut self, value: WordEraseStyle) {
        self.word_erase_style = value;
    }

    // ── Match highlighting getters/setters ───────────────────────────────

    /// Whether to briefly show the matching bracket when one is inserted.
    #[inline]
    #[must_use]
    pub const fn showmatch(&self) -> bool {
        self.showmatch
    }

    /// Set `showmatch`.
    #[inline]
    pub const fn set_showmatch(&mut self, value: bool) {
        self.showmatch = value;
    }

    /// Tenths of a second to show the matching bracket (`matchtime`).
    #[inline]
    #[must_use]
    pub const fn matchtime(&self) -> u8 {
        self.matchtime
    }

    /// Set `matchtime`. Clamped to a maximum of 100.
    #[inline]
    pub fn set_matchtime(&mut self, value: u8) {
        self.matchtime = value.min(100);
    }

    // ── Line wrapping getters/setters ────────────────────────────────────

    /// Columns from the right at which to soft-wrap lines (`wrapmargin`). 0 = disabled.
    #[inline]
    #[must_use]
    pub const fn wrapmargin(&self) -> u32 {
        self.wrapmargin
    }

    /// Set `wrapmargin`.
    #[inline]
    pub const fn set_wrapmargin(&mut self, value: u32) {
        self.wrapmargin = value;
    }

    // ── Ed compatibility getters/setters ─────────────────────────────────

    /// Whether substitute flags are persistent (`edcompatible`).
    #[inline]
    #[must_use]
    pub const fn edcompatible(&self) -> bool {
        self.edcompatible
    }

    /// Set `edcompatible`.
    #[inline]
    pub const fn set_edcompatible(&mut self, value: bool) {
        self.edcompatible = value;
    }

    // ── Key timeout getters/setters ──────────────────────────────────────

    /// Timeout for key code sequences in milliseconds. -1 means use `timeoutlen`.
    #[inline]
    #[must_use]
    pub const fn ttimeoutlen_ms(&self) -> i32 {
        self.ttimeoutlen_ms
    }

    /// Set `ttimeoutlen_ms`. -1 means use `timeoutlen`.
    #[inline]
    pub const fn set_ttimeoutlen_ms(&mut self, value: i32) {
        self.ttimeoutlen_ms = value;
    }

    // ── Sneak mode getters/setters ──────────────────────────────────────

    /// Whether `s`/`S` are sneak motions (two-character cross-line find).
    #[inline]
    #[must_use]
    pub const fn sneak_mode(&self) -> bool {
        self.sneak_mode
    }

    /// Set sneak mode.
    #[inline]
    pub const fn set_sneak_mode(&mut self, value: bool) {
        self.sneak_mode = value;
    }

    // ── Default mode getters/setters ────────────────────────────────────

    /// The mode the engine starts in and returns to after `emergency_reset()`.
    #[inline]
    #[must_use]
    pub const fn default_mode(&self) -> super::Mode {
        self.default_mode
    }

    /// Set the default mode. Only `Normal` and `Insert` are accepted;
    /// other values are silently ignored.
    #[inline]
    pub const fn set_default_mode(&mut self, mode: super::Mode) {
        if mode.is_normal() || mode.is_insert() {
            self.default_mode = mode;
        }
    }

    /// Parse default mode from a `:set` string value.
    #[inline]
    pub fn set_default_mode_str(&mut self, value: &str) {
        match value {
            "normal" => self.default_mode = super::Mode::Normal,
            "insert" => self.default_mode = super::Mode::Insert,
            _ => {} // silently ignore invalid values
        }
    }

    /// String representation for `:set` output.
    #[inline]
    #[must_use]
    pub const fn default_mode_str(&self) -> &'static str {
        if self.default_mode.is_insert() {
            "insert"
        } else {
            "normal"
        }
    }

    // ── Yank highlight getters/setters ──────────────────────────────────

    /// Duration in milliseconds for the yank highlight flash.
    #[inline]
    #[must_use]
    pub const fn yank_highlight_duration_ms(&self) -> u64 {
        self.yank_highlight_duration_ms
    }

    /// Set the yank highlight duration in milliseconds.
    #[inline]
    pub const fn set_yank_highlight_duration_ms(&mut self, value: u64) {
        self.yank_highlight_duration_ms = value;
    }

    // ── Bell control getters/setters ────────────────────────────────────

    /// Whether all bells are suppressed (`belloff=all`).
    #[inline]
    #[must_use]
    pub const fn belloff(&self) -> bool {
        self.belloff
    }

    /// Set `belloff`. `true` = suppress all bells.
    #[inline]
    pub const fn set_belloff(&mut self, value: bool) {
        self.belloff = value;
    }

    // ── Alt-key decomposition ────────────────────────────────────────────

    /// Whether unmatched Alt-keys decompose to Esc + base key.
    #[inline]
    #[must_use]
    pub const fn alt_sends_esc(&self) -> bool {
        self.alt_sends_esc
    }

    /// Set `alt_sends_esc`.
    #[inline]
    pub const fn set_alt_sends_esc(&mut self, value: bool) {
        self.alt_sends_esc = value;
    }

    /// Soft tab stop value.
    ///
    /// - `0` means use `tabstop` (default).
    /// - `-1` means use `shiftwidth`.
    /// - `>0` means use this explicit column count.
    #[inline]
    #[must_use]
    pub const fn softtabstop(&self) -> i32 {
        self.softtabstop
    }

    /// Set `softtabstop`.
    #[inline]
    pub const fn set_softtabstop(&mut self, value: i32) {
        self.softtabstop = value;
    }

    /// Resolve the effective column count for a Tab key press in insert mode.
    ///
    /// Follows Vim's `softtabstop` semantics:
    /// - `sts == 0` => use `tabstop`
    /// - `sts == -1` => use `shiftwidth`
    /// - `sts > 0` => use `sts` directly
    #[inline]
    #[must_use]
    pub fn effective_tab_columns(&self) -> usize {
        match self.softtabstop {
            0 => self.tabstop,
            -1 => self.shiftwidth,
            // Vim treats negative values other than -1 as 0 (use tabstop),
            // which is exactly what the failed conversion falls back to.
            n => usize::try_from(n).unwrap_or(self.tabstop),
        }
    }

    /// Whether sentence boundaries require two spaces after punctuation.
    #[inline]
    #[must_use]
    pub const fn cpo_j(&self) -> bool {
        self.cpo_j
    }

    /// Set `cpo_j`.
    #[inline]
    pub const fn set_cpo_j(&mut self, value: bool) {
        self.cpo_j = value;
    }

    // ── Convenience queries ──────────────────────────────────────────────

    // ── Option-id bridge ─────────────────────────────────────────────────

    /// Return the value of the option identified by `id`.
    #[must_use]
    pub fn get_option(&self, id: OptionId) -> OptionValue {
        match id {
            // Bool options
            OptionId::IgnoreCase => OptionValue::Bool(self.ignorecase()),
            OptionId::SmartCase => OptionValue::Bool(self.smartcase()),
            OptionId::HlSearch => OptionValue::Bool(self.hlsearch()),
            OptionId::IncSearch => OptionValue::Bool(self.incsearch()),
            OptionId::WrapScan => OptionValue::Bool(self.wrapscan()),
            OptionId::ExpandTab => OptionValue::Bool(self.expandtab()),
            OptionId::AutoIndent => OptionValue::Bool(self.autoindent()),
            OptionId::SmartIndent => OptionValue::Bool(self.smartindent()),
            OptionId::Number => OptionValue::Bool(self.number()),
            OptionId::RelativeNumber => OptionValue::Bool(self.relativenumber()),
            OptionId::GDefault => OptionValue::Bool(self.gdefault()),
            OptionId::VisualStar => OptionValue::Bool(self.visualstar()),
            OptionId::BellOff => OptionValue::Bool(self.belloff()),

            // Unsigned numeric options
            OptionId::TabStop => OptionValue::Unsigned(self.tabstop()),
            OptionId::ShiftWidth => OptionValue::Unsigned(self.shiftwidth()),
            OptionId::ScrollOff => OptionValue::Unsigned(self.scrolloff()),
            OptionId::SideScrollOff => OptionValue::Unsigned(self.sidescrolloff()),
            OptionId::TextWidth => OptionValue::Unsigned(self.textwidth()),
            OptionId::TimeoutLen => OptionValue::Unsigned(self.timeoutlen_ms() as usize),

            // Signed numeric options
            OptionId::SoftTabStop => OptionValue::Signed(i64::from(self.softtabstop())),

            // String options
            OptionId::WhichWrap => OptionValue::Str(CompactString::from(self.whichwrap())),
            OptionId::Backspace => OptionValue::Str(CompactString::from(self.backspace())),
            OptionId::VirtualEdit => OptionValue::Str(CompactString::from(self.virtualedit())),
            OptionId::Selection => OptionValue::Str(CompactString::from(self.selection())),
            OptionId::Clipboard => OptionValue::Str(CompactString::from(self.clipboard())),
            OptionId::IsKeyword => OptionValue::Str(CompactString::from(self.iskeyword())),
            OptionId::CommentString => OptionValue::Str(CompactString::from(self.commentstring())),
            OptionId::IncCommand => OptionValue::Str(CompactString::from(self.inccommand())),

            // Special: undolevels is Option<usize>; -1 means unlimited
            OptionId::UndoLevels => {
                OptionValue::Signed(self.undolevels().map_or(-1, byte_delta::to_i64))
            }

            OptionId::UndoAutoGroupMs => {
                OptionValue::Signed(self.undo_auto_group_ms().map_or(-1, i64::from))
            }
        }
    }

    /// Set the option identified by `id` to `value`.
    ///
    /// Type mismatches are silently ignored (no panic).
    pub fn set_option(&mut self, id: OptionId, value: &OptionValue) {
        match id {
            // Bool options
            OptionId::IgnoreCase => {
                if let OptionValue::Bool(v) = value {
                    self.set_ignorecase(*v);
                }
            }
            OptionId::SmartCase => {
                if let OptionValue::Bool(v) = value {
                    self.set_smartcase(*v);
                }
            }
            OptionId::HlSearch => {
                if let OptionValue::Bool(v) = value {
                    self.set_hlsearch(*v);
                }
            }
            OptionId::IncSearch => {
                if let OptionValue::Bool(v) = value {
                    self.set_incsearch(*v);
                }
            }
            OptionId::WrapScan => {
                if let OptionValue::Bool(v) = value {
                    self.set_wrapscan(*v);
                }
            }
            OptionId::ExpandTab => {
                if let OptionValue::Bool(v) = value {
                    self.set_expandtab(*v);
                }
            }
            OptionId::AutoIndent => {
                if let OptionValue::Bool(v) = value {
                    self.set_autoindent(*v);
                }
            }
            OptionId::SmartIndent => {
                if let OptionValue::Bool(v) = value {
                    self.set_smartindent(*v);
                }
            }
            OptionId::Number => {
                if let OptionValue::Bool(v) = value {
                    self.set_number(*v);
                }
            }
            OptionId::RelativeNumber => {
                if let OptionValue::Bool(v) = value {
                    self.set_relativenumber(*v);
                }
            }
            OptionId::GDefault => {
                if let OptionValue::Bool(v) = value {
                    self.set_gdefault(*v);
                }
            }
            OptionId::VisualStar => {
                if let OptionValue::Bool(v) = value {
                    self.set_visualstar(*v);
                }
            }
            OptionId::BellOff => {
                if let OptionValue::Bool(v) = value {
                    self.set_belloff(*v);
                }
            }

            // Unsigned numeric options
            OptionId::TabStop => {
                if let OptionValue::Unsigned(v) = value {
                    self.set_tabstop(*v);
                }
            }
            OptionId::ShiftWidth => {
                if let OptionValue::Unsigned(v) = value {
                    self.set_shiftwidth(*v);
                }
            }
            OptionId::ScrollOff => {
                if let OptionValue::Unsigned(v) = value {
                    self.set_scrolloff(*v);
                }
            }
            OptionId::SideScrollOff => {
                if let OptionValue::Unsigned(v) = value {
                    self.set_sidescrolloff(*v);
                }
            }
            OptionId::TextWidth => {
                if let OptionValue::Unsigned(v) = value {
                    self.set_textwidth(*v);
                }
            }
            OptionId::TimeoutLen => {
                if let OptionValue::Unsigned(v) = value {
                    // Timeoutlen is a millisecond value; values >u32::MAX
                    // (~49 days) are not meaningful and saturate to u32::MAX.
                    let ms = u32::try_from(*v).unwrap_or(u32::MAX);
                    self.set_timeoutlen_ms(ms);
                }
            }

            // String options
            OptionId::WhichWrap => {
                if let OptionValue::Str(v) = value {
                    self.set_whichwrap(v.clone());
                }
            }
            OptionId::Backspace => {
                if let OptionValue::Str(v) = value {
                    self.set_backspace(v.clone());
                }
            }
            OptionId::VirtualEdit => {
                if let OptionValue::Str(v) = value {
                    self.set_virtualedit(v.clone());
                }
            }
            OptionId::Selection => {
                if let OptionValue::Str(v) = value {
                    self.set_selection(v.as_str());
                }
            }
            OptionId::Clipboard => {
                if let OptionValue::Str(v) = value {
                    self.set_clipboard(v.clone());
                }
            }
            OptionId::IsKeyword => {
                if let OptionValue::Str(v) = value {
                    self.set_iskeyword(v.clone());
                }
            }
            OptionId::CommentString => {
                if let OptionValue::Str(v) = value {
                    self.set_commentstring(v.clone());
                }
            }
            OptionId::IncCommand => {
                if let OptionValue::Str(v) = value {
                    self.set_inccommand(v.as_str());
                }
            }

            // Special: undolevels — Signed(-1) means unlimited (None)
            OptionId::UndoLevels => {
                if let OptionValue::Signed(v) = value {
                    if *v < 0 {
                        self.set_undolevels(None);
                    } else {
                        // Sign loss is intentional: the negative-branch above
                        // returned; on 32-bit targets values >usize::MAX
                        // saturate to usize::MAX rather than wrapping.
                        let levels = usize::try_from(*v).unwrap_or(usize::MAX);
                        self.set_undolevels(Some(levels));
                    }
                }
            }

            OptionId::UndoAutoGroupMs => {
                if let OptionValue::Signed(v) = value {
                    if *v < 0 {
                        self.set_undo_auto_group_ms(None);
                    } else {
                        let ms = u32::try_from(*v).unwrap_or(u32::MAX);
                        self.set_undo_auto_group_ms(Some(ms));
                    }
                }
            }

            OptionId::SoftTabStop => {
                if let OptionValue::Signed(v) = value {
                    let sts = i32::try_from(*v).unwrap_or(if *v < 0 { i32::MIN } else { i32::MAX });
                    self.set_softtabstop(sts);
                }
            }
        }
    }

    /// Produce a merged `VimOptions` by applying buffer-local and window-local
    /// overrides on top of `global`.
    ///
    /// For each of the 26 known `OptionId` values:
    /// - If the option's scope is `LocalToBuffer` or `GlobalOrLocalBuffer`
    ///   (and the value is not a sentinel for `GlobalOrLocal*`), apply the
    ///   buffer override if present.
    /// - If the option's scope is `LocalToWindow` or `GlobalOrLocalWindow`
    ///   (and the value is not a sentinel for `GlobalOrLocal*`), apply the
    ///   window override if present.
    #[must_use]
    pub fn resolve_all(global: &Self, buffer: &OptionOverrides, window: &OptionOverrides) -> Self {
        const ALL_IDS: &[OptionId] = &[
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
            OptionId::SoftTabStop,
        ];

        let mut result = global.clone();

        for &id in ALL_IDS {
            let scope = id.scope();
            match scope {
                OptionScope::LocalToBuffer => {
                    if let Some(value) = buffer.get(id) {
                        result.set_option(id, value);
                    }
                }
                OptionScope::GlobalOrLocalBuffer => {
                    if let Some(value) = buffer.get(id) {
                        if !is_sentinel(value) {
                            result.set_option(id, value);
                        }
                    }
                }
                OptionScope::LocalToWindow => {
                    if let Some(value) = window.get(id) {
                        result.set_option(id, value);
                    }
                }
                OptionScope::GlobalOrLocalWindow => {
                    if let Some(value) = window.get(id) {
                        if !is_sentinel(value) {
                            result.set_option(id, value);
                        }
                    }
                }
                OptionScope::Global => {} // global options are not overridden by local overrides
            }
        }

        result
    }

    /// Determine effective case sensitivity for a given search pattern.
    #[must_use]
    pub fn effective_case_sensitive(&self, pattern: &str) -> bool {
        if !self.ignorecase {
            return true;
        }
        if self.smartcase && pattern.chars().any(char::is_uppercase) {
            return true;
        }
        false
    }

    /// Check if backspace can delete over indent.
    #[must_use]
    pub fn backspace_indent(&self) -> bool {
        has_csv_token(&self.backspace, "indent")
            || matches!(self.backspace.as_str(), "1" | "2" | "3")
    }

    /// Check if backspace can delete over end-of-line.
    #[must_use]
    pub fn backspace_eol(&self) -> bool {
        has_csv_token(&self.backspace, "eol") || matches!(self.backspace.as_str(), "2" | "3")
    }

    /// Check if backspace can delete past start of insert.
    #[must_use]
    pub fn backspace_start(&self) -> bool {
        has_csv_token(&self.backspace, "start") || matches!(self.backspace.as_str(), "2" | "3")
    }
}

/// Check if a comma-separated value string contains an exact token.
fn has_csv_token(value: &str, token: &str) -> bool {
    value.split(',').any(|t| t.trim() == token)
}

impl VimOptions {
    /// Enforce all field invariants. Called after deserialization.
    #[cfg(feature = "serde")]
    fn sanitize(&mut self) {
        self.tabstop = self.tabstop.max(1);
        self.shiftwidth = self.shiftwidth.max(1);
    }
}

impl Default for VimOptions {
    /// Standard Vim-like defaults.
    fn default() -> Self {
        Self {
            tabstop: 4,
            shiftwidth: 4,
            expandtab: true,
            autoindent: true,
            smartindent: false,
            ignorecase: false,
            smartcase: false,
            hlsearch: true,
            incsearch: true,
            wrapscan: true,
            visualstar: false,
            scrolloff: 5,
            sidescrolloff: 0,
            number: false,
            relativenumber: false,
            textwidth: 0,
            formatoptions: CompactString::new_inline("tcqj"),
            format_flags: FormatFlags::WRAP_TEXT
                .union(FormatFlags::WRAP_COMMENTS)
                .union(FormatFlags::FORMAT_COMMENTS)
                .union(FormatFlags::REMOVE_COMMENT_LEADER),
            timeoutlen_ms: 500,
            whichwrap: CompactString::new_inline("b,s"),
            backspace: CompactString::new("indent,eol,start"),
            virtualedit: CompactString::new_inline(""),
            selection: SelectionMode::Inclusive,
            clipboard: CompactString::new_inline(""),
            commentstring: CompactString::new_inline("// %s"),
            comments: CompactString::new(DEFAULT_COMMENTS),
            comment_spec: Arc::new(CommentSpec::parse_lossy(DEFAULT_COMMENTS)),
            inccommand: IncCommandMode::NoSplit,
            iskeyword: CompactString::new("@,48-57,_,192-255"),
            word_char_set: WordCharSet::default_vim(),
            gdefault: false,
            undolevels: None,
            quoteescape: CompactString::new_inline("\\"),
            auto_pairs: None,
            langmap: CompactString::default(),
            langremap: false,
            subword_config: SubwordConfig::default(),
            use_system_clipboard: UseSystemClipboard::Never,
            max_macro_effects: 100_000,
            cursor_shape_overrides: Vec::new(),
            undo_auto_group_ms: None,
            multiline_find: false,
            multiline_find_range: 5,
            report: 2,
            word_erase_style: WordEraseStyle::Vi,
            showmatch: false,
            matchtime: 5,
            wrapmargin: 0,
            edcompatible: false,
            ttimeoutlen_ms: -1,
            sneak_mode: false,
            default_mode: super::Mode::Normal,
            belloff: false,
            yank_highlight_duration_ms: 150,
            alt_sends_esc: false,
            softtabstop: 0,
            cpo_j: false,
        }
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for VimOptions {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[allow(clippy::struct_excessive_bools)]
        struct Raw {
            tabstop: usize,
            shiftwidth: usize,
            expandtab: bool,
            autoindent: bool,
            smartindent: bool,
            ignorecase: bool,
            smartcase: bool,
            hlsearch: bool,
            incsearch: bool,
            wrapscan: bool,
            #[serde(default)]
            visualstar: bool,
            scrolloff: usize,
            sidescrolloff: usize,
            number: bool,
            relativenumber: bool,
            textwidth: usize,
            #[serde(default = "default_formatoptions")]
            formatoptions: CompactString,
            timeoutlen_ms: u32,
            whichwrap: CompactString,
            backspace: CompactString,
            virtualedit: CompactString,
            selection: SelectionMode,
            clipboard: CompactString,
            commentstring: CompactString,
            #[serde(default = "default_comments")]
            comments: CompactString,
            inccommand: IncCommandMode,
            iskeyword: CompactString,
            gdefault: bool,
            undolevels: Option<usize>,
            #[serde(default = "default_quoteescape")]
            quoteescape: CompactString,
            #[serde(default)]
            langmap: CompactString,
            #[serde(default)]
            langremap: bool,
            #[serde(default)]
            subword_config: SubwordConfig,
            #[serde(default)]
            use_system_clipboard: UseSystemClipboard,
            #[serde(default = "default_max_macro_effects")]
            max_macro_effects: usize,
            #[serde(default)]
            cursor_shape_overrides: Vec<Option<CursorShape>>,
            #[serde(default)]
            undo_auto_group_ms: Option<u32>,
            #[serde(default)]
            multiline_find: bool,
            #[serde(default = "default_multiline_find_range")]
            multiline_find_range: usize,
            #[serde(default = "default_report")]
            report: u32,
            #[serde(default)]
            word_erase_style: WordEraseStyle,
            #[serde(default)]
            showmatch: bool,
            #[serde(default = "default_matchtime")]
            matchtime: u8,
            #[serde(default)]
            wrapmargin: u32,
            #[serde(default)]
            edcompatible: bool,
            #[serde(default = "default_ttimeoutlen_ms")]
            ttimeoutlen_ms: i32,
            #[serde(default)]
            sneak_mode: bool,
            #[serde(default = "default_default_mode")]
            default_mode: super::Mode,
            #[serde(default)]
            belloff: bool,
            #[serde(default = "default_yank_highlight_duration_ms")]
            yank_highlight_duration_ms: u64,
            #[serde(default)]
            alt_sends_esc: bool,
            #[serde(default)]
            softtabstop: i32,
            #[serde(default)]
            cpo_j: bool,
        }
        fn default_max_macro_effects() -> usize {
            100_000
        }
        fn default_multiline_find_range() -> usize {
            5
        }
        fn default_report() -> u32 {
            2
        }
        fn default_formatoptions() -> CompactString {
            CompactString::new_inline("tcqj")
        }
        fn default_comments() -> CompactString {
            CompactString::new(DEFAULT_COMMENTS)
        }
        fn default_quoteescape() -> CompactString {
            CompactString::new_inline("\\")
        }
        fn default_matchtime() -> u8 {
            5
        }
        fn default_ttimeoutlen_ms() -> i32 {
            -1
        }
        fn default_default_mode() -> super::Mode {
            super::Mode::Normal
        }
        fn default_yank_highlight_duration_ms() -> u64 {
            150
        }
        let raw = Raw::deserialize(deserializer)?;
        let mut opts = Self {
            tabstop: raw.tabstop,
            shiftwidth: raw.shiftwidth,
            expandtab: raw.expandtab,
            autoindent: raw.autoindent,
            smartindent: raw.smartindent,
            ignorecase: raw.ignorecase,
            smartcase: raw.smartcase,
            hlsearch: raw.hlsearch,
            incsearch: raw.incsearch,
            wrapscan: raw.wrapscan,
            visualstar: raw.visualstar,
            scrolloff: raw.scrolloff,
            sidescrolloff: raw.sidescrolloff,
            number: raw.number,
            relativenumber: raw.relativenumber,
            textwidth: raw.textwidth,
            format_flags: FormatFlags::parse_lossy(&raw.formatoptions),
            formatoptions: raw.formatoptions,
            timeoutlen_ms: raw.timeoutlen_ms,
            whichwrap: raw.whichwrap,
            backspace: raw.backspace,
            virtualedit: raw.virtualedit,
            selection: raw.selection,
            clipboard: raw.clipboard,
            commentstring: raw.commentstring,
            comment_spec: Arc::new(CommentSpec::parse_lossy(&raw.comments)),
            comments: raw.comments,
            inccommand: raw.inccommand,
            word_char_set: WordCharSet::from_iskeyword(&raw.iskeyword),
            iskeyword: raw.iskeyword,
            gdefault: raw.gdefault,
            undolevels: raw.undolevels,
            quoteescape: raw.quoteescape,
            auto_pairs: None,
            langmap: raw.langmap,
            langremap: raw.langremap,
            subword_config: raw.subword_config,
            use_system_clipboard: raw.use_system_clipboard,
            max_macro_effects: raw.max_macro_effects,
            cursor_shape_overrides: raw.cursor_shape_overrides,
            undo_auto_group_ms: raw.undo_auto_group_ms,
            multiline_find: raw.multiline_find,
            multiline_find_range: raw.multiline_find_range,
            report: raw.report,
            word_erase_style: raw.word_erase_style,
            showmatch: raw.showmatch,
            matchtime: raw.matchtime,
            wrapmargin: raw.wrapmargin,
            edcompatible: raw.edcompatible,
            ttimeoutlen_ms: raw.ttimeoutlen_ms,
            sneak_mode: raw.sneak_mode,
            default_mode: raw.default_mode,
            belloff: raw.belloff,
            yank_highlight_duration_ms: raw.yank_highlight_duration_ms,
            alt_sends_esc: raw.alt_sends_esc,
            softtabstop: raw.softtabstop,
            cpo_j: raw.cpo_j,
        };
        opts.sanitize();
        Ok(opts)
    }
}

#[cfg(test)]
mod tests {
    use super::super::option_scope::{OptionId, OptionOverrides, OptionValue};
    use super::*;

    #[test]
    fn test_defaults() {
        let opts = VimOptions::default();
        assert_eq!(opts.tabstop(), 4);
        assert_eq!(opts.shiftwidth(), 4);
        assert!(opts.expandtab());
        assert!(opts.autoindent());
        assert!(!opts.smartindent());
        assert_eq!(opts.scrolloff(), 5);
        assert_eq!(opts.sidescrolloff(), 0);
        assert_eq!(opts.textwidth(), 0);
        assert_eq!(opts.timeoutlen_ms(), 500);
        assert!(!opts.ignorecase());
        assert!(!opts.smartcase());
        assert!(opts.hlsearch());
        assert!(opts.incsearch());
        assert!(opts.wrapscan());
        assert!(!opts.number());
        assert!(!opts.relativenumber());
        assert_eq!(opts.whichwrap(), "b,s");
        assert_eq!(opts.backspace(), "indent,eol,start");
        assert_eq!(opts.virtualedit(), "");
        assert_eq!(opts.selection(), "inclusive");
        assert_eq!(opts.selection_mode(), SelectionMode::Inclusive);
        assert_eq!(opts.clipboard(), "");
        assert_eq!(opts.iskeyword(), "@,48-57,_,192-255");
        assert!(!opts.gdefault());
        assert_eq!(opts.inccommand(), "nosplit");
        assert_eq!(opts.inccommand_mode(), IncCommandMode::NoSplit);
        assert!(!opts.belloff());
        assert_eq!(opts.softtabstop(), 0);
    }

    #[test]
    fn test_tabstop_minimum_clamped() {
        let mut o = VimOptions::default();
        o.set_tabstop(0);
        assert_eq!(o.tabstop(), 1);
    }
    #[test]
    fn test_shiftwidth_minimum_clamped() {
        let mut o = VimOptions::default();
        o.set_shiftwidth(0);
        assert_eq!(o.shiftwidth(), 1);
    }

    #[test]
    fn default_format_flags_match_formatoptions_string() {
        let opts = VimOptions::default();
        assert_eq!(
            opts.format_flags(),
            FormatFlags::parse(opts.formatoptions()).unwrap()
        );
        assert!(opts.auto_format_text());
    }

    #[test]
    fn set_formatoptions_rebuilds_flags() {
        let mut opts = VimOptions::default();
        opts.set_formatoptions("cq");
        assert_eq!(opts.formatoptions(), "cq");
        assert!(!opts.auto_format_text());
        assert_eq!(
            opts.format_flags(),
            FormatFlags::WRAP_COMMENTS | FormatFlags::FORMAT_COMMENTS
        );
        // Unknown letters stay in the string but set no flag.
        opts.set_formatoptions("tZ");
        assert_eq!(opts.formatoptions(), "tZ");
        assert_eq!(opts.format_flags(), FormatFlags::WRAP_TEXT);
    }

    #[test]
    fn default_comments_is_vim_default() {
        let opts = VimOptions::default();
        assert_eq!(
            opts.comments(),
            "s1:/*,mb:*,ex:*/,://,b:#,:%,:XCOMM,n:>,fb:-"
        );
        assert_eq!(opts.comment_spec().parts().len(), 9);
        assert!(opts.comment_spec().match_line("# note").is_some());
    }

    #[test]
    fn set_comments_rebuilds_spec() {
        let mut opts = VimOptions::default();
        opts.set_comments("b:##,b:#");
        assert_eq!(opts.comments(), "b:##,b:#");
        assert_eq!(opts.comment_spec().parts().len(), 2);
        assert!(opts.comment_spec().match_line("// x").is_none());
        opts.set_comments("");
        assert!(opts.comment_spec().is_empty());
    }

    #[test]
    fn test_setters() {
        let mut opts = VimOptions::default();
        opts.set_tabstop(8);
        assert_eq!(opts.tabstop(), 8);
        opts.set_shiftwidth(2);
        assert_eq!(opts.shiftwidth(), 2);
        opts.set_expandtab(false);
        assert!(!opts.expandtab());
        opts.set_scrolloff(10);
        assert_eq!(opts.scrolloff(), 10);
        opts.set_textwidth(120);
        assert_eq!(opts.textwidth(), 120);
        opts.set_timeoutlen_ms(500);
        assert_eq!(opts.timeoutlen_ms(), 500);
        opts.set_ignorecase(true);
        assert!(opts.ignorecase());
        opts.set_smartcase(true);
        assert!(opts.smartcase());
        opts.set_hlsearch(false);
        assert!(!opts.hlsearch());
        opts.set_wrapscan(false);
        assert!(!opts.wrapscan());
        opts.set_number(true);
        assert!(opts.number());
        opts.set_relativenumber(true);
        assert!(opts.relativenumber());
        opts.set_sidescrolloff(5);
        assert_eq!(opts.sidescrolloff(), 5);
        opts.set_whichwrap("b,s,<,>,[,]");
        assert_eq!(opts.whichwrap(), "b,s,<,>,[,]");
        opts.set_backspace("2");
        assert_eq!(opts.backspace(), "2");
        opts.set_virtualedit("onemore");
        assert_eq!(opts.virtualedit(), "onemore");
        opts.set_selection("exclusive");
        assert_eq!(opts.selection(), "exclusive");
        assert_eq!(opts.selection_mode(), SelectionMode::Exclusive);
        opts.set_clipboard("unnamedplus");
        assert_eq!(opts.clipboard(), "unnamedplus");
        opts.set_iskeyword("@,48-57,_");
        assert_eq!(opts.iskeyword(), "@,48-57,_");
    }

    #[test]
    fn test_equality() {
        assert_eq!(VimOptions::default(), VimOptions::default());
    }
    #[test]
    fn test_clone() {
        let a = VimOptions::default();
        assert_eq!(a, a.clone());
    }

    #[test]
    fn test_effective_case_sensitive() {
        let mut opts = VimOptions::default();
        assert!(opts.effective_case_sensitive("foo"));
        assert!(opts.effective_case_sensitive("Foo"));
        opts.set_ignorecase(true);
        assert!(!opts.effective_case_sensitive("foo"));
        assert!(!opts.effective_case_sensitive("Foo"));
        opts.set_smartcase(true);
        assert!(!opts.effective_case_sensitive("foo"));
        assert!(opts.effective_case_sensitive("Foo"));
        assert!(opts.effective_case_sensitive("fOo"));
    }

    #[test]
    fn test_backspace_flags() {
        let mut opts = VimOptions::default();
        assert!(opts.backspace_indent());
        assert!(opts.backspace_eol());
        assert!(opts.backspace_start());
        opts.set_backspace("2");
        assert!(opts.backspace_indent());
        assert!(opts.backspace_eol());
        assert!(opts.backspace_start());
        opts.set_backspace("");
        assert!(!opts.backspace_indent());
        assert!(!opts.backspace_eol());
        assert!(!opts.backspace_start());
        opts.set_backspace("eol");
        assert!(!opts.backspace_indent());
        assert!(opts.backspace_eol());
        assert!(!opts.backspace_start());
    }

    #[test]
    fn test_inccommand_default_is_nosplit() {
        let o = VimOptions::default();
        assert_eq!(o.inccommand(), "nosplit");
    }
    #[test]
    fn test_inccommand_enabled_for_nosplit() {
        assert!(VimOptions::default().inccommand_enabled());
    }
    #[test]
    fn test_inccommand_enabled_for_split() {
        let mut o = VimOptions::default();
        o.set_inccommand("split");
        assert!(o.inccommand_enabled());
        assert_eq!(o.inccommand(), "split");
    }
    #[test]
    fn test_inccommand_disabled_for_empty() {
        let mut o = VimOptions::default();
        o.set_inccommand("");
        assert!(!o.inccommand_enabled());
        assert_eq!(o.inccommand(), "");
    }

    #[test]
    fn test_set_inccommand_valid_values() {
        let mut o = VimOptions::default();
        o.set_inccommand("nosplit");
        assert_eq!(o.inccommand(), "nosplit");
        o.set_inccommand("split");
        assert_eq!(o.inccommand(), "split");
        o.set_inccommand("");
        assert_eq!(o.inccommand(), "");
    }

    #[test]
    fn test_set_inccommand_invalid_values_ignored() {
        let mut o = VimOptions::default();
        assert_eq!(o.inccommand(), "nosplit");
        o.set_inccommand("invalid");
        assert_eq!(o.inccommand(), "nosplit");
        o.set_inccommand("foo");
        assert_eq!(o.inccommand(), "nosplit");
    }

    #[test]
    fn test_clipboard_has_unnamed_default_false() {
        let o = VimOptions::default();
        assert!(!o.clipboard_has_unnamed());
        assert!(!o.clipboard_has_unnamedplus());
    }
    #[test]
    fn test_clipboard_has_unnamed_when_set() {
        let mut o = VimOptions::default();
        o.set_clipboard("unnamed");
        assert!(o.clipboard_has_unnamed());
        assert!(!o.clipboard_has_unnamedplus());
    }
    #[test]
    fn test_clipboard_has_unnamedplus_when_set() {
        let mut o = VimOptions::default();
        o.set_clipboard("unnamedplus");
        assert!(!o.clipboard_has_unnamed());
        assert!(o.clipboard_has_unnamedplus());
    }
    #[test]
    fn test_clipboard_has_both_when_comma_separated() {
        let mut o = VimOptions::default();
        o.set_clipboard("unnamed,unnamedplus");
        assert!(o.clipboard_has_unnamed());
        assert!(o.clipboard_has_unnamedplus());
    }
    #[test]
    fn test_clipboard_has_token_with_whitespace() {
        let mut o = VimOptions::default();
        o.set_clipboard("unnamed, unnamedplus");
        assert!(o.clipboard_has_unnamed());
        assert!(o.clipboard_has_unnamedplus());
    }
    #[test]
    fn test_clipboard_empty_has_neither() {
        let mut o = VimOptions::default();
        o.set_clipboard("");
        assert!(!o.clipboard_has_unnamed());
        assert!(!o.clipboard_has_unnamedplus());
    }

    #[test]
    fn test_selection_is_inclusive_by_default() {
        let o = VimOptions::default();
        assert!(!o.selection_is_exclusive());
        assert!(o.selection_is_inclusive());
    }
    #[test]
    fn test_selection_exclusive_mode() {
        let mut o = VimOptions::default();
        o.set_selection("exclusive");
        assert!(o.selection_is_exclusive());
        assert!(!o.selection_is_inclusive());
    }
    #[test]
    fn test_selection_inclusive_mode_explicit() {
        let mut o = VimOptions::default();
        o.set_selection("inclusive");
        assert!(!o.selection_is_exclusive());
        assert!(o.selection_is_inclusive());
    }
    #[test]
    fn test_selection_old_is_treated_as_inclusive() {
        let mut o = VimOptions::default();
        o.set_selection("old");
        assert!(!o.selection_is_exclusive());
        assert!(o.selection_is_inclusive());
    }
    #[test]
    fn test_selection_invalid_leaves_inclusive() {
        let mut o = VimOptions::default();
        o.set_selection("invalid");
        assert!(!o.selection_is_exclusive());
        assert!(o.selection_is_inclusive());
    }
    #[test]
    fn test_selection_exclusive_round_trip() {
        let mut o = VimOptions::default();
        o.set_selection("exclusive");
        assert!(o.selection_is_exclusive());
        o.set_selection("inclusive");
        assert!(!o.selection_is_exclusive());
        assert!(o.selection_is_inclusive());
    }

    #[test]
    fn test_selection_mode_from_str_opt() {
        assert_eq!(
            SelectionMode::from_str_opt("inclusive"),
            Some(SelectionMode::Inclusive)
        );
        assert_eq!(
            SelectionMode::from_str_opt("exclusive"),
            Some(SelectionMode::Exclusive)
        );
        assert_eq!(SelectionMode::from_str_opt("old"), Some(SelectionMode::Old));
        assert_eq!(SelectionMode::from_str_opt("invalid"), None);
        assert_eq!(SelectionMode::from_str_opt(""), None);
    }
    #[test]
    fn test_selection_mode_as_str() {
        assert_eq!(SelectionMode::Inclusive.as_str(), "inclusive");
        assert_eq!(SelectionMode::Exclusive.as_str(), "exclusive");
        assert_eq!(SelectionMode::Old.as_str(), "old");
    }
    #[test]
    fn test_selection_mode_display() {
        assert_eq!(format!("{}", SelectionMode::Inclusive), "inclusive");
        assert_eq!(format!("{}", SelectionMode::Exclusive), "exclusive");
        assert_eq!(format!("{}", SelectionMode::Old), "old");
    }
    #[test]
    fn test_selection_mode_default() {
        assert_eq!(SelectionMode::default(), SelectionMode::Inclusive);
    }

    #[test]
    fn test_inccommand_mode_from_str_opt() {
        assert_eq!(IncCommandMode::from_str_opt(""), Some(IncCommandMode::Off));
        assert_eq!(
            IncCommandMode::from_str_opt("nosplit"),
            Some(IncCommandMode::NoSplit)
        );
        assert_eq!(
            IncCommandMode::from_str_opt("split"),
            Some(IncCommandMode::Split)
        );
        assert_eq!(IncCommandMode::from_str_opt("invalid"), None);
    }
    #[test]
    fn test_inccommand_mode_as_str() {
        assert_eq!(IncCommandMode::Off.as_str(), "");
        assert_eq!(IncCommandMode::NoSplit.as_str(), "nosplit");
        assert_eq!(IncCommandMode::Split.as_str(), "split");
    }
    #[test]
    fn test_inccommand_mode_is_enabled() {
        assert!(!IncCommandMode::Off.is_enabled());
        assert!(IncCommandMode::NoSplit.is_enabled());
        assert!(IncCommandMode::Split.is_enabled());
    }
    #[test]
    fn test_inccommand_mode_display() {
        assert_eq!(format!("{}", IncCommandMode::Off), "");
        assert_eq!(format!("{}", IncCommandMode::NoSplit), "nosplit");
        assert_eq!(format!("{}", IncCommandMode::Split), "split");
    }
    #[test]
    fn test_inccommand_mode_default() {
        assert_eq!(IncCommandMode::default(), IncCommandMode::NoSplit);
    }

    #[test]
    fn test_set_selection_mode_direct() {
        let mut o = VimOptions::default();
        o.set_selection_mode(SelectionMode::Exclusive);
        assert_eq!(o.selection_mode(), SelectionMode::Exclusive);
        assert!(o.selection_is_exclusive());
    }
    #[test]
    fn test_set_inccommand_mode_direct() {
        let mut o = VimOptions::default();
        o.set_inccommand_mode(IncCommandMode::Split);
        assert_eq!(o.inccommand_mode(), IncCommandMode::Split);
        assert!(o.inccommand_enabled());
        assert_eq!(o.inccommand(), "split");
    }

    #[test]
    fn test_gdefault_default_is_false() {
        assert!(!VimOptions::default().gdefault());
    }
    #[test]
    fn test_gdefault_setter() {
        let mut o = VimOptions::default();
        o.set_gdefault(true);
        assert!(o.gdefault());
        o.set_gdefault(false);
        assert!(!o.gdefault());
    }

    // ── get_option / set_option round-trip tests ────────────────────────

    #[test]
    fn test_get_option_bool_round_trip() {
        let mut o = VimOptions::default();
        o.set_ignorecase(true);
        assert_eq!(o.get_option(OptionId::IgnoreCase), OptionValue::Bool(true));
        o.set_ignorecase(false);
        assert_eq!(o.get_option(OptionId::IgnoreCase), OptionValue::Bool(false));
    }

    #[test]
    fn test_get_option_unsigned_round_trip() {
        let mut o = VimOptions::default();
        o.set_tabstop(8);
        assert_eq!(o.get_option(OptionId::TabStop), OptionValue::Unsigned(8));
        o.set_scrolloff(3);
        assert_eq!(o.get_option(OptionId::ScrollOff), OptionValue::Unsigned(3));
    }

    #[test]
    fn test_get_option_string_round_trip() {
        let mut o = VimOptions::default();
        o.set_clipboard("unnamed");
        assert_eq!(
            o.get_option(OptionId::Clipboard),
            OptionValue::Str(CompactString::from("unnamed"))
        );
        o.set_iskeyword("@,48-57");
        assert_eq!(
            o.get_option(OptionId::IsKeyword),
            OptionValue::Str(CompactString::from("@,48-57"))
        );
    }

    #[test]
    fn test_get_option_undolevels_none_returns_minus_one() {
        let o = VimOptions::default();
        assert_eq!(o.get_option(OptionId::UndoLevels), OptionValue::Signed(-1));
    }

    #[test]
    fn test_get_option_undolevels_some_returns_value() {
        let mut o = VimOptions::default();
        o.set_undolevels(Some(100));
        assert_eq!(o.get_option(OptionId::UndoLevels), OptionValue::Signed(100));
    }

    #[test]
    fn test_set_option_bool_then_typed_getter() {
        let mut o = VimOptions::default();
        o.set_option(OptionId::HlSearch, &OptionValue::Bool(false));
        assert!(!o.hlsearch());
        o.set_option(OptionId::Number, &OptionValue::Bool(true));
        assert!(o.number());
    }

    #[test]
    fn test_set_option_unsigned_then_typed_getter() {
        let mut o = VimOptions::default();
        o.set_option(OptionId::TabStop, &OptionValue::Unsigned(2));
        assert_eq!(o.tabstop(), 2);
        o.set_option(OptionId::TextWidth, &OptionValue::Unsigned(100));
        assert_eq!(o.textwidth(), 100);
    }

    #[test]
    fn test_set_option_string_then_typed_getter() {
        let mut o = VimOptions::default();
        o.set_option(
            OptionId::CommentString,
            &OptionValue::Str(CompactString::from("# %s")),
        );
        assert_eq!(o.commentstring(), "# %s");
    }

    #[test]
    fn test_set_option_undolevels_minus_one_sets_none() {
        let mut o = VimOptions::default();
        o.set_undolevels(Some(50));
        o.set_option(OptionId::UndoLevels, &OptionValue::Signed(-1));
        assert_eq!(o.undolevels(), None);
    }

    #[test]
    fn test_set_option_undolevels_positive_sets_some() {
        let mut o = VimOptions::default();
        o.set_option(OptionId::UndoLevels, &OptionValue::Signed(200));
        assert_eq!(o.undolevels(), Some(200));
    }

    #[test]
    fn test_set_option_type_mismatch_silently_ignored() {
        let mut o = VimOptions::default();
        let orig_tabstop = o.tabstop();
        // TabStop expects Unsigned, passing Bool — should be silently ignored
        o.set_option(OptionId::TabStop, &OptionValue::Bool(true));
        assert_eq!(o.tabstop(), orig_tabstop);
    }

    // ── resolve_all tests ───────────────────────────────────────────────

    #[test]
    fn test_resolve_all_buffer_override_applied() {
        let global = VimOptions::default();
        let mut buffer = OptionOverrides::new();
        buffer.set(OptionId::TabStop, OptionValue::Unsigned(2));
        let window = OptionOverrides::new();

        let resolved = VimOptions::resolve_all(&global, &buffer, &window);
        assert_eq!(resolved.tabstop(), 2);
    }

    #[test]
    fn test_resolve_all_window_override_applied() {
        let global = VimOptions::default();
        let buffer = OptionOverrides::new();
        let mut window = OptionOverrides::new();
        window.set(OptionId::ScrollOff, OptionValue::Unsigned(10));

        let resolved = VimOptions::resolve_all(&global, &buffer, &window);
        assert_eq!(resolved.scrolloff(), 10);
    }

    #[test]
    fn test_resolve_all_global_only_option_not_overridden_by_buffer() {
        let mut global = VimOptions::default();
        global.set_ignorecase(true);
        let mut buffer = OptionOverrides::new();
        // IgnoreCase is Global scope — buffer override should be ignored
        buffer.set(OptionId::IgnoreCase, OptionValue::Bool(false));
        let window = OptionOverrides::new();

        let resolved = VimOptions::resolve_all(&global, &buffer, &window);
        // global value should be preserved because scope is Global
        assert!(resolved.ignorecase());
    }

    #[test]
    fn test_resolve_all_sentinel_falls_through_to_global() {
        let mut global = VimOptions::default();
        global.set_backspace("indent,eol,start");
        let mut buffer = OptionOverrides::new();
        // Sentinel empty string for GlobalOrLocalBuffer should not override
        buffer.set(
            OptionId::Backspace,
            OptionValue::Str(CompactString::from("")),
        );
        let window = OptionOverrides::new();

        let resolved = VimOptions::resolve_all(&global, &buffer, &window);
        assert_eq!(resolved.backspace(), "indent,eol,start");
    }

    #[test]
    fn test_resolve_all_global_unchanged() {
        let global = VimOptions::default();
        let mut buffer = OptionOverrides::new();
        buffer.set(OptionId::TabStop, OptionValue::Unsigned(2));
        let window = OptionOverrides::new();

        let _resolved = VimOptions::resolve_all(&global, &buffer, &window);
        // original global should be unchanged
        assert_eq!(global.tabstop(), 4);
    }

    #[test]
    fn test_resolve_all_non_sentinel_global_or_local_buffer_applied() {
        let global = VimOptions::default();
        let mut buffer = OptionOverrides::new();
        // WhichWrap is GlobalOrLocalBuffer — non-sentinel value should apply
        buffer.set(
            OptionId::WhichWrap,
            OptionValue::Str(CompactString::from("b,s,<,>")),
        );
        let window = OptionOverrides::new();

        let resolved = VimOptions::resolve_all(&global, &buffer, &window);
        assert_eq!(resolved.whichwrap(), "b,s,<,>");
    }

    // ── cursor_shape_overrides tests ────────────────────────────────────

    #[test]
    fn test_cursor_shape_overrides_default_empty() {
        let opts = VimOptions::default();
        assert!(opts.cursor_shape_overrides().is_empty());
    }

    #[test]
    fn test_cursor_shape_overrides_setter_getter() {
        let mut opts = VimOptions::default();
        let overrides = vec![
            Some(CursorShape::Block),
            None,
            Some(CursorShape::VerticalBar),
        ];
        opts.set_cursor_shape_overrides(overrides.clone());
        assert_eq!(opts.cursor_shape_overrides(), &overrides[..]);
    }

    #[test]
    fn test_cursor_shape_overrides_clone_preserves() {
        let mut opts = VimOptions::default();
        opts.set_cursor_shape_overrides(vec![None, Some(CursorShape::HorizontalBar)]);
        let cloned = opts.clone();
        assert_eq!(
            cloned.cursor_shape_overrides(),
            opts.cursor_shape_overrides()
        );
    }

    #[test]
    fn test_undo_auto_group_ms_default_is_none() {
        let opts = VimOptions::default();
        assert_eq!(opts.undo_auto_group_ms(), None);
    }

    #[test]
    fn test_undo_auto_group_ms_setter() {
        let mut opts = VimOptions::default();
        opts.set_undo_auto_group_ms(Some(300));
        assert_eq!(opts.undo_auto_group_ms(), Some(300));
        opts.set_undo_auto_group_ms(None);
        assert_eq!(opts.undo_auto_group_ms(), None);
    }

    #[test]
    fn test_get_option_undo_auto_group_ms_none_returns_minus_one() {
        let opts = VimOptions::default();
        assert_eq!(
            opts.get_option(OptionId::UndoAutoGroupMs),
            OptionValue::Signed(-1)
        );
    }

    #[test]
    fn test_set_option_undo_auto_group_ms_positive() {
        let mut opts = VimOptions::default();
        opts.set_option(OptionId::UndoAutoGroupMs, &OptionValue::Signed(200));
        assert_eq!(opts.undo_auto_group_ms(), Some(200));
    }

    #[test]
    fn test_set_option_undo_auto_group_ms_minus_one_disables() {
        let mut opts = VimOptions::default();
        opts.set_undo_auto_group_ms(Some(300));
        opts.set_option(OptionId::UndoAutoGroupMs, &OptionValue::Signed(-1));
        assert_eq!(opts.undo_auto_group_ms(), None);
    }

    // ── default_mode tests ──────────────────────────────────────────────

    #[test]
    fn test_default_mode_is_normal() {
        let opts = VimOptions::default();
        assert!(opts.default_mode().is_normal());
        assert_eq!(opts.default_mode_str(), "normal");
    }

    #[test]
    fn test_set_default_mode_insert() {
        let mut opts = VimOptions::default();
        opts.set_default_mode(super::super::Mode::Insert);
        assert!(opts.default_mode().is_insert());
        assert_eq!(opts.default_mode_str(), "insert");
    }

    #[test]
    fn test_set_default_mode_str_normal() {
        let mut opts = VimOptions::default();
        opts.set_default_mode(super::super::Mode::Insert);
        opts.set_default_mode_str("normal");
        assert!(opts.default_mode().is_normal());
    }

    #[test]
    fn test_set_default_mode_str_insert() {
        let mut opts = VimOptions::default();
        opts.set_default_mode_str("insert");
        assert!(opts.default_mode().is_insert());
    }

    #[test]
    fn test_set_default_mode_str_invalid_ignored() {
        let mut opts = VimOptions::default();
        opts.set_default_mode_str("visual");
        assert!(opts.default_mode().is_normal()); // unchanged
    }

    #[test]
    fn test_set_default_mode_rejects_visual() {
        let mut opts = VimOptions::default();
        opts.set_default_mode(super::super::Mode::Visual(super::super::VisualType::Char));
        assert!(opts.default_mode().is_normal()); // unchanged
    }

    // ── yank_highlight_duration_ms tests ────────────────────────────────

    #[test]
    fn test_yank_highlight_duration_default() {
        let opts = VimOptions::default();
        assert_eq!(opts.yank_highlight_duration_ms(), 150);
    }

    #[test]
    fn test_yank_highlight_duration_setter() {
        let mut opts = VimOptions::default();
        opts.set_yank_highlight_duration_ms(300);
        assert_eq!(opts.yank_highlight_duration_ms(), 300);
    }

    // ── softtabstop ─────────────────────────────────────────────────────

    #[test]
    fn test_softtabstop_default_is_zero() {
        let opts = VimOptions::default();
        assert_eq!(opts.softtabstop(), 0);
    }

    #[test]
    fn test_softtabstop_setter() {
        let mut opts = VimOptions::default();
        opts.set_softtabstop(4);
        assert_eq!(opts.softtabstop(), 4);
        opts.set_softtabstop(-1);
        assert_eq!(opts.softtabstop(), -1);
        opts.set_softtabstop(0);
        assert_eq!(opts.softtabstop(), 0);
    }

    #[test]
    fn test_effective_tab_columns_zero_uses_tabstop() {
        let mut opts = VimOptions::default();
        opts.set_tabstop(8);
        opts.set_softtabstop(0);
        assert_eq!(opts.effective_tab_columns(), 8);
    }

    #[test]
    fn test_effective_tab_columns_positive_uses_sts() {
        let mut opts = VimOptions::default();
        opts.set_tabstop(8);
        opts.set_softtabstop(4);
        assert_eq!(opts.effective_tab_columns(), 4);
    }

    #[test]
    fn test_effective_tab_columns_minus_one_uses_shiftwidth() {
        let mut opts = VimOptions::default();
        opts.set_tabstop(8);
        opts.set_shiftwidth(2);
        opts.set_softtabstop(-1);
        assert_eq!(opts.effective_tab_columns(), 2);
    }

    #[test]
    fn test_effective_tab_columns_other_negative_uses_tabstop() {
        let mut opts = VimOptions::default();
        opts.set_tabstop(8);
        opts.set_softtabstop(-5);
        // Vim treats negative values other than -1 as 0 (use tabstop)
        assert_eq!(opts.effective_tab_columns(), 8);
    }

    #[test]
    fn test_softtabstop_option_id_get_set() {
        let mut opts = VimOptions::default();
        // Default: 0
        let val = opts.get_option(OptionId::SoftTabStop);
        assert_eq!(val, OptionValue::Signed(0));

        // Set to 4 via option id
        opts.set_option(OptionId::SoftTabStop, &OptionValue::Signed(4));
        assert_eq!(opts.softtabstop(), 4);
        assert_eq!(
            opts.get_option(OptionId::SoftTabStop),
            OptionValue::Signed(4)
        );

        // Set to -1 via option id
        opts.set_option(OptionId::SoftTabStop, &OptionValue::Signed(-1));
        assert_eq!(opts.softtabstop(), -1);
        assert_eq!(
            opts.get_option(OptionId::SoftTabStop),
            OptionValue::Signed(-1)
        );
    }
}
