//! Register types for vim-core.
//!
//! Types for register names and content (not storage implementation).

use crate::primitives::{ClipboardMetadata, MotionType};
use compact_str::CompactString;
use smallvec::SmallVec;
use smart_default::SmartDefault;
use std::fmt::Write as _;

/// A validated register name.
///
/// Valid registers: a-z, A-Z, 0-9, ", -, *, +, _, /, :, ., %, #, =
///
/// Stored as a `u8` because all valid register characters are ASCII,
/// saving 3 bytes per instance (4 bytes `char` → 1 byte `u8`).
/// `Option<RegisterName>` shrinks from 4 bytes to 2 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SmartDefault)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(try_from = "char", into = "char"))]
pub struct RegisterName(#[default = b'"'] u8);

impl std::fmt::Display for RegisterName {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_char(self.0 as char)
    }
}

impl RegisterName {
    /// Unnamed register (").
    pub const UNNAMED: Self = Self(b'"');
    /// Black hole register (_).
    pub const BLACKHOLE: Self = Self(b'_');
    /// Small delete register (-).
    pub const SMALL_DELETE: Self = Self(b'-');
    /// Last yank register (0).
    pub const LAST_YANK: Self = Self(b'0');
    /// System clipboard (+).
    pub const CLIPBOARD: Self = Self(b'+');
    /// Primary selection (*).
    pub const SELECTION: Self = Self(b'*');
    /// Last search pattern (/).
    pub const SEARCH: Self = Self(b'/');
    /// Current file (%).
    pub const FILENAME: Self = Self(b'%');
    /// Alternate file (#).
    pub const ALTERNATE: Self = Self(b'#');
    /// Last inserted text (.).
    pub const LAST_INSERT: Self = Self(b'.');
    /// Last command (:).
    pub const LAST_COMMAND: Self = Self(b':');
    /// Expression register (=).
    pub const EXPRESSION: Self = Self(b'=');
    /// First numbered (delete history) register (1).
    pub const NUMBERED_1: Self = Self(b'1');
    /// Last-played macro sentinel (`@`) — used internally for `@@`.
    ///
    /// This is an **internal-only sentinel** that intentionally bypasses
    /// `is_valid('@')` (which returns `false`). It is never exposed to users
    /// as a writable register name; it exists solely so the engine can track
    /// which macro register was most recently played for the `@@` command.
    /// It is constructed via `new_unchecked` in const context, not through
    /// the validating `RegisterName::new()` constructor.
    pub const LAST_MACRO: Self = Self(b'@');

    /// Create a new register name.
    ///
    /// Returns None if the character is not a valid register.
    #[must_use]
    pub const fn new(c: char) -> Option<Self> {
        if c.is_ascii() && Self::is_valid(c) {
            Some(Self(c as u8))
        } else {
            None
        }
    }

    /// Create a register name without validation (const-compatible).
    ///
    /// The caller must ensure `c` is a valid ASCII register character.
    /// Use only for well-known constants defined within this crate.
    #[inline]
    #[must_use]
    pub(crate) const fn new_unchecked(c: char) -> Self {
        Self(c as u8)
    }

    /// Check if a character is a valid register name.
    #[must_use]
    pub const fn is_valid(c: char) -> bool {
        matches!(c,
            'a'..='z' |
            'A'..='Z' |
            '0'..='9' |
            '"' | '-' | '*' | '+' | '_' | '/' | ':' | '.' | '%' | '#' | '='
        )
    }

    /// Get the character.
    #[inline]
    #[must_use]
    pub const fn char(self) -> char {
        self.0 as char
    }

    /// Check if this is a named register (a-z).
    #[inline]
    #[must_use]
    pub const fn is_named(self) -> bool {
        self.0.is_ascii_lowercase()
    }

    /// Check if this is an append register (A-Z).
    #[inline]
    #[must_use]
    pub const fn is_append(self) -> bool {
        self.0.is_ascii_uppercase()
    }

    /// Check if this is a numbered register (0-9).
    #[inline]
    #[must_use]
    pub const fn is_numbered(self) -> bool {
        self.0.is_ascii_digit()
    }

    /// Return the next numbered register for dot-repeat auto-increment.
    ///
    /// Per `:help .`: "If the command included a specification of a numbered
    /// register, the register number will be incremented."
    ///
    /// - Registers 0-8 increment to the next digit (0→1, 1→2, ..., 8→9).
    /// - Register 9 returns `None` — there is no register after 9, so the
    ///   auto-increment stops rather than wrapping or repeating.
    /// - Non-numbered registers return `None`.
    #[inline]
    #[must_use]
    pub const fn next_numbered(self) -> Option<Self> {
        match self.0 {
            b'0'..=b'8' => Some(Self(self.0 + 1)),
            b'9' => None,
            _ => None,
        }
    }

    /// Check if this is a read-only register.
    #[inline]
    #[must_use]
    pub const fn is_readonly(self) -> bool {
        matches!(self.0, b'%' | b'#' | b':' | b'.' | b'/')
    }

    /// Get the lowercase version for append registers.
    #[inline]
    #[must_use]
    pub const fn to_lowercase(self) -> Self {
        Self(self.0.to_ascii_lowercase())
    }

    /// Check if this is the black hole register (_).
    #[inline]
    #[must_use]
    pub const fn is_blackhole(self) -> bool {
        self.0 == b'_'
    }

    /// Check if this is a clipboard register (+ or *).
    #[inline]
    #[must_use]
    pub const fn is_clipboard(self) -> bool {
        matches!(self.0, b'+' | b'*')
    }

    /// Check if this is the expression register (=).
    #[inline]
    #[must_use]
    pub const fn is_expression(self) -> bool {
        self.0 == b'='
    }

    /// Classify this register into a storage category.
    ///
    /// Used by `Registers::get()` / `set()` to dispatch via match.
    #[inline]
    #[must_use]
    pub const fn category(self) -> RegisterCategory {
        match self.0 {
            b'"' => RegisterCategory::Unnamed,
            b'0' => RegisterCategory::LastYank,
            b'1'..=b'9' => RegisterCategory::Numbered((self.0 as usize) - (b'1' as usize)),
            b'-' => RegisterCategory::SmallDelete,
            b'a'..=b'z' => RegisterCategory::Named,
            b'A'..=b'Z' => RegisterCategory::Append,
            b'/' => RegisterCategory::Search,
            b'=' => RegisterCategory::Expression,
            b'_' => RegisterCategory::Blackhole,
            b'+' | b'*' => RegisterCategory::Clipboard,
            b':' => RegisterCategory::LastCommand,
            _ => RegisterCategory::Other,
        }
    }
}

/// Convert `RegisterName` to `char` — used by serde serialization.
impl From<RegisterName> for char {
    #[inline]
    fn from(r: RegisterName) -> Self {
        r.0 as Self
    }
}

/// Convert `char` to `RegisterName` — used by serde deserialization.
impl TryFrom<char> for RegisterName {
    type Error = &'static str;

    #[inline]
    fn try_from(c: char) -> Result<Self, Self::Error> {
        Self::new(c).ok_or("invalid register character")
    }
}

/// Category of a register, used to dispatch storage operations.
///
/// Produced by [`RegisterName::category()`]. Each variant maps to
/// a distinct storage slot inside `Registers`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RegisterCategory {
    /// `"` — default unnamed register.
    Unnamed,
    /// `0` — last yank register.
    LastYank,
    /// `1`–`9` — numbered delete-history registers. Carries the 0-based index.
    Numbered(usize),
    /// `-` — small delete register.
    SmallDelete,
    /// `a`–`z` — named registers.
    Named,
    /// `A`–`Z` — append to the lowercase counterpart.
    Append,
    /// `/` — last search pattern register.
    Search,
    /// `=` — expression register (host-evaluated).
    Expression,
    /// `_` — black hole (discard).
    Blackhole,
    /// `+` / `*` — system clipboard registers.
    Clipboard,
    /// `:` — last ex command register.
    LastCommand,
    /// Any other valid register character.
    Other,
}

/// Content stored in a register.
///
/// Stores one or more text entries. Single-cursor operations produce one entry
/// (inline via `SmallVec` — zero heap allocation). Multi-cursor operations
/// produce N entries, enabling paste-zipping (cursor i gets entry i).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RegisterContent {
    /// Text entries. Single-cursor: 1 entry (inline). Multi-cursor: N entries.
    entries: SmallVec<[CompactString; 1]>,
    /// How the content was yanked/deleted.
    motion_type: MotionType,
    /// Optional clipboard metadata for intelligent paste behavior.
    metadata: Option<ClipboardMetadata>,
    /// Monotonic timestamp set on write. Used for multi-instance merge
    /// (forward-looking). Sourced from a simple counter, not wall-clock time.
    #[cfg_attr(feature = "serde", serde(default))]
    timestamp: u64,
    /// Visual block width in screen columns. Set on block yank/delete.
    ///
    /// Used by block paste to restore the original column width even when
    /// the yanked lines have different lengths. `None` for non-block content.
    #[cfg_attr(feature = "serde", serde(default))]
    block_width: Option<u32>,
}

impl Default for RegisterContent {
    fn default() -> Self {
        Self {
            entries: SmallVec::from_elem(CompactString::default(), 1),
            motion_type: MotionType::default(),
            metadata: None,
            timestamp: 0,
            block_width: None,
        }
    }
}

impl RegisterContent {
    /// Create new register content (single entry).
    ///
    /// # Linewise invariant
    ///
    /// When `motion_type` is `LineWise`, the stored text always ends with `\n`.
    /// If the provided text does not already end with `\n`, one is appended.
    /// This matches Neovim's `op_yank` behavior where linewise register content
    /// unconditionally terminates with a newline.
    #[must_use]
    pub fn new(text: impl Into<CompactString>, motion_type: MotionType) -> Self {
        let mut text = text.into();
        if motion_type == MotionType::LineWise && !text.ends_with('\n') {
            text.push('\n');
        }
        Self {
            entries: SmallVec::from_elem(text, 1),
            motion_type,
            metadata: None,
            timestamp: 0,
            block_width: None,
        }
    }

    /// Create multi-entry register content (for multi-cursor yank).
    ///
    /// Each entry corresponds to text yanked/deleted at one cursor position.
    /// Paste-zipping distributes entry i to cursor i.
    ///
    /// # Panics
    ///
    /// Panics if `entries` is empty (at least one entry is required).
    #[must_use]
    pub fn from_entries(entries: SmallVec<[CompactString; 1]>, motion_type: MotionType) -> Self {
        assert!(
            !entries.is_empty(),
            "RegisterContent requires at least one entry"
        );
        Self {
            entries,
            motion_type,
            metadata: None,
            timestamp: 0,
            block_width: None,
        }
    }

    /// Create character-wise content.
    #[inline]
    #[must_use]
    pub fn char_wise(text: impl Into<CompactString>) -> Self {
        Self::new(text, MotionType::CharWise)
    }

    /// Create line-wise content.
    #[inline]
    #[must_use]
    pub fn line_wise(text: impl Into<CompactString>) -> Self {
        Self::new(text, MotionType::LineWise)
    }

    /// Create block-wise content.
    #[inline]
    #[must_use]
    pub fn block_wise(text: impl Into<CompactString>) -> Self {
        Self::new(text, MotionType::BlockWise)
    }

    /// Get the primary text content (first entry).
    ///
    /// For single-entry registers (the common case), this is the full text.
    /// For multi-entry registers, this returns only the first cursor's text.
    /// Use [`Self::entry()`] for cursor-index-aware access.
    #[inline]
    #[must_use]
    pub fn text(&self) -> &str {
        &self.entries[0]
    }

    /// Get all entries for multi-cursor distribution.
    ///
    /// Single-cursor yank: returns a slice of length 1.
    /// Multi-cursor yank: returns a slice of length N.
    #[inline]
    #[must_use]
    pub fn entries(&self) -> &[CompactString] {
        &self.entries
    }

    /// Number of entries (1 for single-cursor, N for multi-cursor).
    #[inline]
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Get entry at cursor index, clamping to the available range.
    ///
    /// If `cursor_index >= entry_count()`, returns the last entry.
    /// This handles the case where there are more cursors than entries
    /// (extra cursors get the last entry repeated).
    #[inline]
    #[must_use]
    pub fn entry(&self, cursor_index: usize) -> &str {
        &self.entries[cursor_index.min(self.entries.len() - 1)]
    }

    /// Get the motion type.
    #[inline]
    #[must_use]
    pub const fn motion_type(&self) -> MotionType {
        self.motion_type
    }

    /// Append another register's content to this one.
    ///
    /// Used by uppercase (append) registers (A-Z). For multi-entry content,
    /// appends to the primary entry only.
    ///
    /// # Type promotion
    ///
    /// Neovim promotes the motion type when the appended content has a
    /// "stronger" type. The promotion algebra is:
    /// - `LineWise` always wins (`Line + Char = Line`, `Line + Block = Line`)
    /// - `BlockWise` beats `CharWise` (`Char + Block = Block`)
    /// - Appending charwise to linewise or blockwise keeps the original
    ///
    /// After promotion, the linewise invariant is enforced: if the result
    /// is `LineWise`, the text must end with `\n`.
    pub fn append(&mut self, other: &Self) {
        self.entries[0].push_str(&other.entries[0]);

        // Type promotion: stronger type wins
        match (self.motion_type, other.motion_type) {
            // LineWise always wins
            (_, MotionType::LineWise) => self.motion_type = MotionType::LineWise,
            // BlockWise beats CharWise
            (MotionType::CharWise, MotionType::BlockWise) => {
                self.motion_type = MotionType::BlockWise;
            }
            // Otherwise keep self's type (Line+Block=Line, Line+Char=Line,
            // Block+Char=Block, Block+Block=Block, Char+Char=Char)
            _ => {}
        }

        // Enforce linewise invariant: text must end with \n
        if self.motion_type == MotionType::LineWise && !self.entries[0].ends_with('\n') {
            self.entries[0].push('\n');
        }
    }

    /// Check if empty (primary entry is empty).
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries[0].is_empty()
    }

    /// Get length of primary entry.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries[0].len()
    }

    /// Set clipboard metadata (builder pattern).
    #[inline]
    #[must_use]
    pub fn with_metadata(mut self, metadata: ClipboardMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Get the clipboard metadata, if any.
    #[inline]
    #[must_use]
    pub const fn metadata(&self) -> Option<&ClipboardMetadata> {
        self.metadata.as_ref()
    }

    /// Get the monotonic timestamp (set on register write).
    #[inline]
    #[must_use]
    pub const fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Set the monotonic timestamp.
    #[inline]
    pub const fn set_timestamp(&mut self, ts: u64) {
        self.timestamp = ts;
    }

    /// Get the block width in screen columns, if set.
    ///
    /// Only meaningful for `MotionType::BlockWise` content. Returns `None`
    /// for non-block content or block content where width was not recorded.
    #[inline]
    #[must_use]
    pub const fn block_width(&self) -> Option<u32> {
        self.block_width
    }

    /// Set the block width in screen columns (builder pattern).
    ///
    /// Called during block yank/delete to record the visual width of the
    /// block selection so that block paste can restore it.
    #[inline]
    #[must_use]
    pub const fn with_block_width(mut self, width: u32) -> Self {
        self.block_width = Some(width);
        self
    }

    /// Set the block width in screen columns (mutable setter).
    #[inline]
    pub const fn set_block_width(&mut self, width: Option<u32>) {
        self.block_width = width;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === RegisterName validation ===

    #[test]
    fn valid_registers_accepted() {
        for c in
            "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789\"-_+*/.:%#=".chars()
        {
            assert!(
                RegisterName::new(c).is_some(),
                "char '{}' should be valid",
                c
            );
        }
    }

    #[test]
    fn invalid_registers_rejected() {
        for c in "!$^&()[]{}|\\;',<>? \t\n".chars() {
            assert!(
                RegisterName::new(c).is_none(),
                "char '{}' should be invalid",
                c
            );
        }
    }

    // === Constants ===

    #[test]
    fn constants_round_trip() {
        assert_eq!(RegisterName::UNNAMED.char(), '"');
        assert_eq!(RegisterName::BLACKHOLE.char(), '_');
        assert_eq!(RegisterName::SMALL_DELETE.char(), '-');
        assert_eq!(RegisterName::LAST_YANK.char(), '0');
        assert_eq!(RegisterName::CLIPBOARD.char(), '+');
        assert_eq!(RegisterName::SELECTION.char(), '*');
        assert_eq!(RegisterName::SEARCH.char(), '/');
    }

    // === Predicates ===

    #[test]
    fn is_named() {
        assert!(RegisterName::new_unchecked('a').is_named());
        assert!(RegisterName::new_unchecked('z').is_named());
        assert!(!RegisterName::new_unchecked('A').is_named());
        assert!(!RegisterName::new_unchecked('0').is_named());
    }

    #[test]
    fn is_append() {
        assert!(RegisterName::new_unchecked('A').is_append());
        assert!(RegisterName::new_unchecked('Z').is_append());
        assert!(!RegisterName::new_unchecked('a').is_append());
    }

    #[test]
    fn is_numbered() {
        for c in '0'..='9' {
            assert!(RegisterName::new_unchecked(c).is_numbered());
        }
        assert!(!RegisterName::new_unchecked('a').is_numbered());
    }

    #[test]
    fn is_readonly() {
        for c in ['%', '#', ':', '.', '/'] {
            assert!(
                RegisterName::new_unchecked(c).is_readonly(),
                "'{}' should be readonly",
                c
            );
        }
        assert!(!RegisterName::new_unchecked('a').is_readonly());
    }

    #[test]
    fn is_blackhole() {
        assert!(RegisterName::BLACKHOLE.is_blackhole());
        assert!(!RegisterName::new_unchecked('a').is_blackhole());
    }

    #[test]
    fn is_clipboard() {
        assert!(RegisterName::CLIPBOARD.is_clipboard());
        assert!(RegisterName::SELECTION.is_clipboard());
        assert!(!RegisterName::new_unchecked('a').is_clipboard());
    }

    #[test]
    fn to_lowercase() {
        assert_eq!(
            RegisterName::new_unchecked('A').to_lowercase(),
            RegisterName::new_unchecked('a')
        );
        assert_eq!(
            RegisterName::new_unchecked('a').to_lowercase(),
            RegisterName::new_unchecked('a')
        );
    }

    // === RegisterCategory ===

    #[test]
    fn category_covers_all_variants() {
        use super::RegisterCategory;
        assert_eq!(RegisterName::UNNAMED.category(), RegisterCategory::Unnamed);
        assert_eq!(
            RegisterName::LAST_YANK.category(),
            RegisterCategory::LastYank
        );
        assert_eq!(
            RegisterName::SMALL_DELETE.category(),
            RegisterCategory::SmallDelete
        );
        assert_eq!(
            RegisterName::BLACKHOLE.category(),
            RegisterCategory::Blackhole
        );
        assert_eq!(RegisterName::SEARCH.category(), RegisterCategory::Search);

        // Named a-z
        assert_eq!(
            RegisterName::new_unchecked('a').category(),
            RegisterCategory::Named
        );
        assert_eq!(
            RegisterName::new_unchecked('z').category(),
            RegisterCategory::Named
        );

        // Append A-Z
        assert_eq!(
            RegisterName::new_unchecked('A').category(),
            RegisterCategory::Append
        );
        assert_eq!(
            RegisterName::new_unchecked('Z').category(),
            RegisterCategory::Append
        );

        // Numbered 1-9
        for (i, c) in ('1'..='9').enumerate() {
            assert_eq!(
                RegisterName::new_unchecked(c).category(),
                RegisterCategory::Numbered(i)
            );
        }

        // Expression
        assert_eq!(
            RegisterName::EXPRESSION.category(),
            RegisterCategory::Expression
        );

        // Clipboard registers
        assert_eq!(
            RegisterName::new_unchecked('+').category(),
            RegisterCategory::Clipboard
        );
        assert_eq!(
            RegisterName::new_unchecked('*').category(),
            RegisterCategory::Clipboard
        );
    }

    // === RegisterContent ===

    #[test]
    fn content_charwise() {
        let c = RegisterContent::char_wise("hello");
        assert_eq!(c.text(), "hello");
        assert_eq!(c.motion_type(), MotionType::CharWise);
    }

    #[test]
    fn content_linewise() {
        let c = RegisterContent::line_wise("line\n");
        assert_eq!(c.text(), "line\n");
        assert_eq!(c.motion_type(), MotionType::LineWise);
    }

    #[test]
    fn content_blockwise() {
        let c = RegisterContent::block_wise("block");
        assert_eq!(c.motion_type(), MotionType::BlockWise);
    }

    #[test]
    fn content_len_and_empty() {
        let empty = RegisterContent::char_wise("");
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);

        let nonempty = RegisterContent::char_wise("abc");
        assert!(!nonempty.is_empty());
        assert_eq!(nonempty.len(), 3);
    }

    #[test]
    fn content_default_is_empty_charwise() {
        let d = RegisterContent::default();
        assert!(d.is_empty());
        assert_eq!(d.motion_type(), MotionType::CharWise);
    }

    #[test]
    fn content_append() {
        let mut a = RegisterContent::char_wise("hello");
        // line_wise enforces trailing \n, so " world" becomes " world\n"
        let b = RegisterContent::line_wise(" world");
        a.append(&b);
        assert_eq!(a.text(), "hello world\n");
        // Linewise append promotes charwise to linewise (Neovim behavior)
        assert_eq!(a.motion_type(), MotionType::LineWise);
    }

    // === ClipboardMetadata ===

    #[test]
    fn default_content_has_no_metadata() {
        let c = RegisterContent::default();
        assert!(c.metadata().is_none());
    }

    #[test]
    fn constructors_have_no_metadata() {
        assert!(RegisterContent::char_wise("a").metadata().is_none());
        assert!(RegisterContent::line_wise("b\n").metadata().is_none());
        assert!(RegisterContent::block_wise("c").metadata().is_none());
        assert!(RegisterContent::new("d", MotionType::CharWise)
            .metadata()
            .is_none());
    }

    #[test]
    fn with_metadata_sets_metadata() {
        use crate::primitives::ClipboardMetadata;
        use compact_str::CompactString;

        let meta = ClipboardMetadata {
            is_entire_line: true,
            first_line_indent: 4,
            source_path: Some(CompactString::from("test.rs")),
        };
        let c = RegisterContent::char_wise("hello").with_metadata(meta.clone());
        let got = c.metadata().expect("metadata should be Some");
        assert_eq!(*got, meta);
        // Original content is preserved.
        assert_eq!(c.text(), "hello");
        assert_eq!(c.motion_type(), MotionType::CharWise);
    }

    #[test]
    fn metadata_getter_returns_ref() {
        use crate::primitives::ClipboardMetadata;

        let meta = ClipboardMetadata {
            is_entire_line: false,
            first_line_indent: 0,
            source_path: None,
        };
        let c = RegisterContent::line_wise("line\n").with_metadata(meta);
        let got = c.metadata().unwrap();
        assert!(!got.is_entire_line);
        assert_eq!(got.first_line_indent, 0);
        assert!(got.source_path.is_none());
    }

    // === Multi-entry (multi-cursor clipboard) ===

    #[test]
    fn single_entry_text_returns_content() {
        let c = RegisterContent::char_wise("hello");
        assert_eq!(c.text(), "hello");
        assert_eq!(c.entry_count(), 1);
        assert_eq!(c.entry(0), "hello");
        assert_eq!(c.entries(), &[CompactString::from("hello")]);
    }

    #[test]
    fn from_entries_multi() {
        use smallvec::smallvec;
        let entries: SmallVec<[CompactString; 1]> = smallvec![
            CompactString::from("alpha"),
            CompactString::from("beta"),
            CompactString::from("gamma"),
        ];
        let c = RegisterContent::from_entries(entries, MotionType::CharWise);
        assert_eq!(c.entry_count(), 3);
        assert_eq!(c.text(), "alpha"); // primary is first entry
        assert_eq!(c.entry(0), "alpha");
        assert_eq!(c.entry(1), "beta");
        assert_eq!(c.entry(2), "gamma");
        assert_eq!(c.motion_type(), MotionType::CharWise);
    }

    #[test]
    fn entry_clamping_kakoune_style() {
        use smallvec::smallvec;
        let entries: SmallVec<[CompactString; 1]> =
            smallvec![CompactString::from("first"), CompactString::from("second"),];
        let c = RegisterContent::from_entries(entries, MotionType::CharWise);
        // Index within bounds
        assert_eq!(c.entry(0), "first");
        assert_eq!(c.entry(1), "second");
        // Index beyond bounds clamps to last
        assert_eq!(c.entry(2), "second");
        assert_eq!(c.entry(100), "second");
    }

    #[test]
    fn from_entries_linewise_no_auto_newline() {
        // from_entries does NOT auto-append newline (caller is responsible)
        use smallvec::smallvec;
        let entries: SmallVec<[CompactString; 1]> = smallvec![
            CompactString::from("line1\n"),
            CompactString::from("line2\n"),
        ];
        let c = RegisterContent::from_entries(entries, MotionType::LineWise);
        assert_eq!(c.text(), "line1\n");
        assert_eq!(c.entry(1), "line2\n");
    }

    #[test]
    fn multi_entry_is_empty_checks_primary() {
        use smallvec::smallvec;
        let entries: SmallVec<[CompactString; 1]> =
            smallvec![CompactString::from(""), CompactString::from("non-empty"),];
        let c = RegisterContent::from_entries(entries, MotionType::CharWise);
        // is_empty checks primary entry only
        assert!(c.is_empty());
        assert_eq!(c.len(), 0);
    }

    #[test]
    fn multi_entry_with_metadata() {
        use crate::primitives::ClipboardMetadata;
        use smallvec::smallvec;

        let entries: SmallVec<[CompactString; 1]> =
            smallvec![CompactString::from("a"), CompactString::from("b"),];
        let meta = ClipboardMetadata {
            is_entire_line: false,
            first_line_indent: 2,
            source_path: None,
        };
        let c = RegisterContent::from_entries(entries, MotionType::CharWise)
            .with_metadata(meta.clone());
        assert_eq!(c.entry_count(), 2);
        assert_eq!(*c.metadata().unwrap(), meta);
    }

    #[test]
    #[should_panic(expected = "at least one entry")]
    fn from_entries_empty_panics() {
        let entries: SmallVec<[CompactString; 1]> = SmallVec::new();
        RegisterContent::from_entries(entries, MotionType::CharWise);
    }

    #[test]
    fn single_entry_inline_no_heap() {
        // SmallVec<[CompactString; 1]> stores 1 element inline
        let c = RegisterContent::char_wise("test");
        assert_eq!(c.entry_count(), 1);
        // Verify it behaves correctly (inline storage is an implementation detail)
        assert_eq!(c.text(), "test");
        assert_eq!(c.entry(0), "test");
    }

    #[test]
    fn append_on_multi_entry_modifies_primary() {
        use smallvec::smallvec;
        let mut a = RegisterContent::from_entries(
            smallvec![CompactString::from("hello"), CompactString::from("world")],
            MotionType::CharWise,
        );
        let b = RegisterContent::char_wise(" suffix");
        a.append(&b);
        // append modifies primary entry only
        assert_eq!(a.text(), "hello suffix");
        assert_eq!(a.entry(1), "world"); // second entry unchanged
    }

    // === Append type promotion (Neovim behavior) ===

    #[test]
    fn append_charwise_plus_linewise_promotes_to_linewise() {
        let mut a = RegisterContent::char_wise("hello");
        let b = RegisterContent::line_wise("world");
        a.append(&b);
        assert_eq!(a.motion_type(), MotionType::LineWise);
        // Linewise invariant: text ends with \n
        assert!(a.text().ends_with('\n'));
        assert_eq!(a.text(), "helloworld\n");
    }

    #[test]
    fn append_charwise_plus_blockwise_promotes_to_blockwise() {
        let mut a = RegisterContent::char_wise("hello");
        let b = RegisterContent::block_wise("block");
        a.append(&b);
        assert_eq!(a.motion_type(), MotionType::BlockWise);
        assert_eq!(a.text(), "helloblock");
    }

    #[test]
    fn append_linewise_plus_blockwise_stays_linewise() {
        let mut a = RegisterContent::line_wise("line");
        let b = RegisterContent::block_wise("block");
        a.append(&b);
        assert_eq!(a.motion_type(), MotionType::LineWise);
        // Linewise invariant preserved: original had \n, appended text after it
        assert!(a.text().ends_with('\n'));
    }

    #[test]
    fn append_linewise_plus_charwise_stays_linewise() {
        let mut a = RegisterContent::line_wise("line");
        let b = RegisterContent::char_wise("char");
        a.append(&b);
        assert_eq!(a.motion_type(), MotionType::LineWise);
        assert!(a.text().ends_with('\n'));
        assert_eq!(a.text(), "line\nchar\n");
    }

    #[test]
    fn append_linewise_invariant_no_double_newline() {
        // When both texts already end with \n, no extra \n is added
        let mut a = RegisterContent::line_wise("first\n");
        let b = RegisterContent::line_wise("second\n");
        a.append(&b);
        assert_eq!(a.motion_type(), MotionType::LineWise);
        assert_eq!(a.text(), "first\nsecond\n");
    }

    #[test]
    fn append_charwise_plus_charwise_stays_charwise() {
        let mut a = RegisterContent::char_wise("hello");
        let b = RegisterContent::char_wise(" world");
        a.append(&b);
        assert_eq!(a.motion_type(), MotionType::CharWise);
        assert_eq!(a.text(), "hello world");
    }

    #[test]
    fn append_blockwise_plus_blockwise_stays_blockwise() {
        let mut a = RegisterContent::block_wise("ab");
        let b = RegisterContent::block_wise("cd");
        a.append(&b);
        assert_eq!(a.motion_type(), MotionType::BlockWise);
        assert_eq!(a.text(), "abcd");
    }

    #[test]
    fn append_blockwise_plus_charwise_stays_blockwise() {
        let mut a = RegisterContent::block_wise("block");
        let b = RegisterContent::char_wise("char");
        a.append(&b);
        assert_eq!(a.motion_type(), MotionType::BlockWise);
        assert_eq!(a.text(), "blockchar");
    }

    #[test]
    fn append_blockwise_plus_linewise_promotes_to_linewise() {
        let mut a = RegisterContent::block_wise("block");
        let b = RegisterContent::line_wise("line");
        a.append(&b);
        assert_eq!(a.motion_type(), MotionType::LineWise);
        assert!(a.text().ends_with('\n'));
    }

    // === Multi-entry integration tests (multi-cursor clipboard storage) ===

    #[test]
    fn from_entries_single_identical_to_new_charwise() {
        use smallvec::smallvec;
        let via_new = RegisterContent::new("hello world", MotionType::CharWise);
        let via_from = RegisterContent::from_entries(
            smallvec![CompactString::from("hello world")],
            MotionType::CharWise,
        );
        assert_eq!(via_new, via_from);
        assert_eq!(via_new.text(), via_from.text());
        assert_eq!(via_new.entry_count(), via_from.entry_count());
        assert_eq!(via_new.entry(0), via_from.entry(0));
        assert_eq!(via_new.motion_type(), via_from.motion_type());
        assert_eq!(via_new.len(), via_from.len());
    }

    #[test]
    fn from_entries_single_identical_to_new_linewise() {
        use smallvec::smallvec;
        // new() auto-appends \n for linewise; from_entries does NOT.
        // So to be identical, the from_entries caller must provide the \n.
        let via_new = RegisterContent::new("some line", MotionType::LineWise);
        let via_from = RegisterContent::from_entries(
            smallvec![CompactString::from("some line\n")],
            MotionType::LineWise,
        );
        assert_eq!(via_new, via_from);
        assert_eq!(via_new.text(), "some line\n");
        assert_eq!(via_from.text(), "some line\n");
    }

    #[test]
    fn from_entries_preserves_linewise_without_adding_newline() {
        use smallvec::smallvec;
        // from_entries treats entries as pre-normalized: no trailing \n appended
        let entries: SmallVec<[CompactString; 1]> = smallvec![
            CompactString::from("no newline here"),
            CompactString::from("also none"),
        ];
        let c = RegisterContent::from_entries(entries, MotionType::LineWise);
        assert_eq!(c.motion_type(), MotionType::LineWise);
        // Entries stored verbatim — no \n appended by from_entries
        assert_eq!(c.entry(0), "no newline here");
        assert_eq!(c.entry(1), "also none");
        assert!(!c.text().ends_with('\n'));
    }

    #[test]
    fn entry_zero_equals_text_single_entry() {
        let c = RegisterContent::char_wise("testing123");
        assert_eq!(c.entry(0), c.text());
        // Also true for linewise
        let c2 = RegisterContent::line_wise("a line");
        assert_eq!(c2.entry(0), c2.text());
    }

    #[test]
    fn entry_zero_equals_text_multi_entry() {
        use smallvec::smallvec;
        let c = RegisterContent::from_entries(
            smallvec![
                CompactString::from("primary"),
                CompactString::from("secondary"),
                CompactString::from("tertiary"),
            ],
            MotionType::BlockWise,
        );
        assert_eq!(c.entry(0), c.text());
        assert_eq!(c.entry(0), "primary");
    }

    #[test]
    fn clone_multi_entry_preserves_all_entries() {
        use smallvec::smallvec;
        let original = RegisterContent::from_entries(
            smallvec![
                CompactString::from("alpha"),
                CompactString::from("beta"),
                CompactString::from("gamma"),
                CompactString::from("delta"),
            ],
            MotionType::CharWise,
        );
        let cloned = original.clone();
        assert_eq!(cloned.entry_count(), 4);
        assert_eq!(cloned.entry(0), "alpha");
        assert_eq!(cloned.entry(1), "beta");
        assert_eq!(cloned.entry(2), "gamma");
        assert_eq!(cloned.entry(3), "delta");
        assert_eq!(cloned.motion_type(), MotionType::CharWise);
        assert_eq!(original, cloned);
    }

    #[test]
    fn partial_eq_multi_entry_same() {
        use smallvec::smallvec;
        let a = RegisterContent::from_entries(
            smallvec![CompactString::from("x"), CompactString::from("y")],
            MotionType::CharWise,
        );
        let b = RegisterContent::from_entries(
            smallvec![CompactString::from("x"), CompactString::from("y")],
            MotionType::CharWise,
        );
        assert_eq!(a, b);
    }

    #[test]
    fn partial_eq_multi_entry_different_content() {
        use smallvec::smallvec;
        let a = RegisterContent::from_entries(
            smallvec![CompactString::from("x"), CompactString::from("y")],
            MotionType::CharWise,
        );
        let b = RegisterContent::from_entries(
            smallvec![CompactString::from("x"), CompactString::from("z")],
            MotionType::CharWise,
        );
        assert_ne!(a, b);
    }

    #[test]
    fn partial_eq_multi_entry_different_count() {
        use smallvec::smallvec;
        let a = RegisterContent::from_entries(
            smallvec![CompactString::from("x"), CompactString::from("y")],
            MotionType::CharWise,
        );
        let b = RegisterContent::from_entries(
            smallvec![CompactString::from("x")],
            MotionType::CharWise,
        );
        assert_ne!(a, b);
    }

    #[test]
    fn partial_eq_multi_entry_different_motion_type() {
        use smallvec::smallvec;
        let a = RegisterContent::from_entries(
            smallvec![CompactString::from("x"), CompactString::from("y")],
            MotionType::CharWise,
        );
        let b = RegisterContent::from_entries(
            smallvec![CompactString::from("x"), CompactString::from("y")],
            MotionType::BlockWise,
        );
        assert_ne!(a, b);
    }

    #[test]
    fn len_returns_primary_entry_length_multi_entry() {
        use smallvec::smallvec;
        let c = RegisterContent::from_entries(
            smallvec![
                CompactString::from("short"),
                CompactString::from("a much longer second entry"),
                CompactString::from("third"),
            ],
            MotionType::CharWise,
        );
        // len() returns primary (first) entry length for backward compat
        assert_eq!(c.len(), 5); // "short".len() == 5
        assert!(!c.is_empty());
    }

    #[test]
    fn with_metadata_on_multi_entry_preserves_entries() {
        use crate::primitives::ClipboardMetadata;
        use smallvec::smallvec;

        let meta = ClipboardMetadata {
            is_entire_line: true,
            first_line_indent: 8,
            source_path: Some(CompactString::from("/src/main.rs")),
        };
        let c = RegisterContent::from_entries(
            smallvec![
                CompactString::from("line1\n"),
                CompactString::from("line2\n"),
                CompactString::from("line3\n"),
            ],
            MotionType::LineWise,
        )
        .with_metadata(meta.clone());

        // Metadata is set
        assert_eq!(*c.metadata().unwrap(), meta);
        // All entries preserved
        assert_eq!(c.entry_count(), 3);
        assert_eq!(c.entry(0), "line1\n");
        assert_eq!(c.entry(1), "line2\n");
        assert_eq!(c.entry(2), "line3\n");
        assert_eq!(c.motion_type(), MotionType::LineWise);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_roundtrip_multi_entry() {
        use smallvec::smallvec;

        let original = RegisterContent::from_entries(
            smallvec![
                CompactString::from("cursor1_text"),
                CompactString::from("cursor2_text"),
                CompactString::from("cursor3_text"),
            ],
            MotionType::BlockWise,
        );

        let json = serde_json::to_string(&original).expect("serialize");
        let deserialized: RegisterContent = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.entry_count(), 3);
        assert_eq!(deserialized.entry(0), "cursor1_text");
        assert_eq!(deserialized.entry(1), "cursor2_text");
        assert_eq!(deserialized.entry(2), "cursor3_text");
        assert_eq!(deserialized.motion_type(), MotionType::BlockWise);
        assert_eq!(original, deserialized);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_roundtrip_multi_entry_with_metadata() {
        use crate::primitives::ClipboardMetadata;
        use smallvec::smallvec;

        let meta = ClipboardMetadata {
            is_entire_line: false,
            first_line_indent: 4,
            source_path: Some(CompactString::from("lib.rs")),
        };
        let original = RegisterContent::from_entries(
            smallvec![CompactString::from("a"), CompactString::from("b")],
            MotionType::CharWise,
        )
        .with_metadata(meta.clone());

        let json = serde_json::to_string(&original).expect("serialize");
        let deserialized: RegisterContent = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(original, deserialized);
        assert_eq!(*deserialized.metadata().unwrap(), meta);
        assert_eq!(deserialized.entry_count(), 2);
    }

    // === block_width ===

    #[test]
    fn block_width_default_is_none() {
        let c = RegisterContent::block_wise("abc");
        assert_eq!(c.block_width(), None);
    }

    #[test]
    fn block_width_with_builder() {
        let c = RegisterContent::block_wise("abc").with_block_width(10);
        assert_eq!(c.block_width(), Some(10));
    }

    #[test]
    fn block_width_set_and_clear() {
        let mut c = RegisterContent::block_wise("abc");
        c.set_block_width(Some(5));
        assert_eq!(c.block_width(), Some(5));
        c.set_block_width(None);
        assert_eq!(c.block_width(), None);
    }
}
