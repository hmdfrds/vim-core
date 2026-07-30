//! Vim registers storage.
//!
//! # Layering
//!
//! Imports `primitives` and `std`; must not import `commands`, `effects`,
//! `execution` or `dispatch`. State modules are pure data containers with
//! no execution logic.
//!
//! Implements all Vim register types.
//!
//! # Register Types
//!
//! | Register | Purpose |
//! |----------|---------|
//! | `"` | Unnamed - default for all operations |
//! | `0` | Yank register - last yank only |
//! | `1-9` | Numbered - delete history (1 = most recent) |
//! | `a-z` | Named - user storage (overwrite) |
//! | `A-Z` | Named append - appends to lowercase |
//! | `-` | Small delete - deletes < 1 line |
//! | `_` | Black hole - discards text |
//!
//! Read-only registers (%, #, ., /) are handled by the shell.
//! The `:` register is stored engine-side (set by ex command execution).

use ahash::AHashMap;

use crate::primitives::{MotionType, RegisterContent, RegisterName};

/// All Vim registers.
///
/// See the module docs for the table of register names and their purposes.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Registers {
    /// Unnamed register (") - default for all operations.
    unnamed: Option<RegisterContent>,
    /// Yank register (0) - last yank only.
    yank: Option<RegisterContent>,
    /// Numbered registers (1-9) - delete history.
    /// Index 0 = register 1, index 8 = register 9.
    numbered: [Option<RegisterContent>; 9],
    /// Small delete register (-) - deletes less than one line.
    small_delete: Option<RegisterContent>,
    /// Named registers a-z: fixed array indexed by `(char - b'a')`.
    /// Exactly 26 possible keys, so a hash map is unnecessary overhead.
    named_az: [Option<RegisterContent>; 26],
    /// Search register (/) and any other special registers that need dynamic storage.
    special_registers: AHashMap<RegisterName, RegisterContent>,
    /// Expression register (`=`) — last evaluated expression result.
    ///
    /// Set by the host via `VimEngine::set_expression_result()` after
    /// evaluating an expression prompt.
    expression: Option<RegisterContent>,
    /// Last command register (`:`) — last executed ex command.
    #[cfg_attr(feature = "serde", serde(default))]
    last_command: Option<RegisterContent>,
    /// Monotonically increasing version counter, bumped on every mutation.
    ///
    /// Used by [`crate::state::diff::StateSnapshot`] to detect register changes
    /// without requiring `PartialEq` on the full register set.
    #[cfg_attr(feature = "serde", serde(default))]
    version: u64,
    /// Last-known clipboard generation from the host.
    ///
    /// Updated via `HostNotification::ClipboardChanged`. On paste, the engine
    /// compares this to the host-provided generation to determine whether a
    /// fresh clipboard read is needed (stale = host clipboard changed since
    /// last engine-side yank/paste).
    #[cfg_attr(feature = "serde", serde(default))]
    clipboard_generation: u64,
}

impl Registers {
    /// Create empty registers.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Monotonically increasing version counter.
    ///
    /// Bumped on every mutation (set, yank, delete). Used by state diffing
    /// to detect changes without comparing all register contents.
    #[inline]
    #[must_use]
    pub const fn version(&self) -> u64 {
        self.version
    }

    /// Current clipboard generation.
    ///
    /// Returns the last generation value set by `set_clipboard_generation()`.
    /// A value of 0 means no clipboard change has been reported.
    #[inline]
    #[must_use]
    pub const fn clipboard_generation(&self) -> u64 {
        self.clipboard_generation
    }

    /// Update the clipboard generation counter.
    ///
    /// Called when the host reports that the system clipboard changed externally.
    /// Paste operations compare this to detect staleness and request fresh reads.
    #[inline]
    pub const fn set_clipboard_generation(&mut self, generation: u64) {
        self.clipboard_generation = generation;
    }

    /// Check whether the clipboard is stale relative to a host-provided generation.
    ///
    /// Returns `true` if `host_generation` is newer than the engine's last-seen
    /// generation, meaning the engine's cached clipboard content is outdated and
    /// a fresh read from the host is needed before pasting.
    #[inline]
    #[must_use]
    pub const fn clipboard_is_stale(&self, host_generation: u64) -> bool {
        host_generation > self.clipboard_generation
    }

    /// Get register content by name.
    ///
    /// Returns `None` for black hole (`_`) or empty registers.
    ///
    /// Read a register with clipboard aliasing.
    ///
    /// When `clipboard=unnamedplus`, the unnamed register `"` is transparently
    /// aliased to `+` (system clipboard). When `clipboard=unnamed`, it's
    /// aliased to `*` (primary selection). Explicit register names are never
    /// aliased — `"ap` always reads register `a`.
    #[must_use]
    pub fn get_aliased(
        &self,
        name: RegisterName,
        options: &crate::primitives::VimOptions,
    ) -> Option<&RegisterContent> {
        let effective = if name == RegisterName::UNNAMED {
            if options.clipboard_has_unnamedplus() {
                RegisterName::CLIPBOARD
            } else if options.clipboard_has_unnamed() {
                RegisterName::SELECTION
            } else {
                name
            }
        } else {
            name
        };
        self.get(effective)
    }

    /// Read a register by exact name (no clipboard aliasing).
    pub fn get(&self, name: RegisterName) -> Option<&RegisterContent> {
        use crate::primitives::RegisterCategory;
        match name.category() {
            RegisterCategory::Unnamed => self.unnamed.as_ref(),
            RegisterCategory::LastYank => self.yank.as_ref(),
            RegisterCategory::Numbered(idx) => self.numbered.get(idx).and_then(Option::as_ref),
            RegisterCategory::SmallDelete => self.small_delete.as_ref(),
            RegisterCategory::Named | RegisterCategory::Append => {
                let idx = (name.to_lowercase().char() as u8 - b'a') as usize;
                self.named_az.get(idx).and_then(Option::as_ref)
            }
            RegisterCategory::Search => self.special_registers.get(&RegisterName::SEARCH),
            RegisterCategory::Clipboard => self.special_registers.get(&name),
            RegisterCategory::Expression => self.expression.as_ref(),
            RegisterCategory::LastCommand => self.last_command.as_ref(),
            RegisterCategory::Blackhole | RegisterCategory::Other => None,
        }
    }

    /// Stamp a `RegisterContent` with the current version as its timestamp.
    const fn stamp(&self, content: &mut RegisterContent) {
        content.set_timestamp(self.version);
    }

    /// Set register content.
    ///
    /// For uppercase named registers (A-Z), appends to existing content.
    ///
    /// # Complexity
    ///
    /// Time: O(1) amortized for most categories. For append registers (A-Z),
    /// O(n) where n = length of existing content (string concatenation).
    /// Hash map insert is O(1) amortized.
    ///
    /// Space: O(n) where n = content text length (stored in the register).
    pub fn set(&mut self, name: RegisterName, mut content: RegisterContent) {
        use crate::primitives::RegisterCategory;
        self.version += 1;
        self.stamp(&mut content);
        match name.category() {
            RegisterCategory::Unnamed => {
                self.unnamed = Some(content);
            }
            RegisterCategory::LastYank => {
                self.yank = Some(content);
            }
            RegisterCategory::Numbered(idx) => {
                if let Some(slot) = self.numbered.get_mut(idx) {
                    *slot = Some(content);
                }
            }
            RegisterCategory::SmallDelete => {
                self.small_delete = Some(content);
            }
            RegisterCategory::Named => {
                let idx = (name.char() as u8 - b'a') as usize;
                if let Some(slot) = self.named_az.get_mut(idx) {
                    *slot = Some(content);
                }
            }
            RegisterCategory::Append => {
                let key = name.to_lowercase();
                let idx = (key.char() as u8 - b'a') as usize;
                if let Some(slot) = self.named_az.get_mut(idx) {
                    if let Some(existing) = slot.as_mut() {
                        existing.append(&content);
                        existing.set_timestamp(self.version);
                    } else {
                        *slot = Some(content);
                    }
                    // Also update unnamed register with the full appended content
                    // This matches Vim behavior: "Ayw sets " to the full appended result
                    if let Some(appended) = slot.as_ref() {
                        self.unnamed = Some(appended.clone());
                    }
                }
            }
            RegisterCategory::Search => {
                self.special_registers.insert(RegisterName::SEARCH, content);
            }
            RegisterCategory::Clipboard => {
                self.special_registers.insert(name, content);
            }
            RegisterCategory::Expression => {
                self.expression = Some(content);
            }
            RegisterCategory::LastCommand => {
                self.last_command = Some(content);
            }
            RegisterCategory::Blackhole | RegisterCategory::Other => {
                // discard / ignore
            }
        }
    }

    /// Update registers after a yank operation.
    ///
    /// Per Vim spec:
    /// - Always updates `"` (unnamed)
    /// - If no explicit register: also updates `0` (yank)
    ///
    /// # Complexity
    ///
    /// Time: O(1) — clones the content (O(n) for the string data, but
    /// considered O(1) in terms of register operations since it's a fixed
    /// number of field assignments).
    ///
    /// Space: O(n) where n = content text length (one clone for unnamed).
    pub fn on_yank(
        &mut self,
        mut content: RegisterContent,
        explicit_register: Option<RegisterName>,
    ) {
        self.version += 1;
        self.stamp(&mut content);
        // Always update unnamed
        self.unnamed = Some(content.clone());

        // Update yank register only if no explicit register
        if explicit_register.is_none() {
            self.yank = Some(content);
        }
    }

    /// Update registers after a delete operation.
    ///
    /// Per Vim spec:
    /// - Always updates `"` (unnamed)
    /// - If linewise OR the deleted text contains a newline: shifts numbered registers 1-9
    /// - If charwise/blockwise and no embedded newline: updates `-` (small delete)
    ///
    /// The text-content check handles the case where a charwise motion deletes
    /// across a line boundary (e.g., `dw` at end of line), which Vim routes to
    /// the numbered registers rather than the small-delete register.
    ///
    /// # Complexity
    ///
    /// Time: O(n) for the `contains('\n')` scan; the numbered register shift is
    /// a fixed 8-iteration loop (registers 8->9 through 1->2).  Content clone
    /// for unnamed is O(n) for text length.
    ///
    /// Space: O(n) where n = content text length (one clone for unnamed).
    pub fn on_delete(&mut self, mut content: RegisterContent, motion_type: MotionType) {
        self.version += 1;
        self.stamp(&mut content);
        // Always update unnamed
        self.unnamed = Some(content.clone());

        if motion_type.is_line_wise() || content.text().contains('\n') {
            // Shift numbered registers: 8→9, 7→8, ..., 1→2, new→1
            self.shift_numbered(content);
        } else {
            // Small delete
            self.small_delete = Some(content);
        }
    }

    /// Clear (remove) the content of a register by name.
    ///
    /// For named registers (`a-z`), removes the entry from the hash map.
    /// For other register categories, sets the slot to `None`.
    /// Append registers (`A-Z`) clear the lowercase counterpart.
    /// Black-hole and read-only registers are no-ops.
    pub fn clear(&mut self, name: RegisterName) {
        use crate::primitives::RegisterCategory;
        self.version += 1;
        match name.category() {
            RegisterCategory::Unnamed => {
                self.unnamed = None;
            }
            RegisterCategory::LastYank => {
                self.yank = None;
            }
            RegisterCategory::Numbered(idx) => {
                if let Some(slot) = self.numbered.get_mut(idx) {
                    *slot = None;
                }
            }
            RegisterCategory::SmallDelete => {
                self.small_delete = None;
            }
            RegisterCategory::Named => {
                let idx = (name.char() as u8 - b'a') as usize;
                if let Some(slot) = self.named_az.get_mut(idx) {
                    *slot = None;
                }
            }
            RegisterCategory::Append => {
                let idx = (name.to_lowercase().char() as u8 - b'a') as usize;
                if let Some(slot) = self.named_az.get_mut(idx) {
                    *slot = None;
                }
            }
            RegisterCategory::Search => {
                self.special_registers.remove(&RegisterName::SEARCH);
            }
            RegisterCategory::Clipboard => {
                self.special_registers.remove(&name);
            }
            RegisterCategory::Expression => {
                self.expression = None;
            }
            RegisterCategory::LastCommand => {
                self.last_command = None;
            }
            RegisterCategory::Blackhole | RegisterCategory::Other => {
                // nothing to clear
            }
        }
    }

    /// Set the expression register result.
    ///
    /// Called by the engine after the host evaluates an expression
    /// for the `=` register.
    pub fn set_expression_result(&mut self, mut content: RegisterContent) {
        self.version += 1;
        self.stamp(&mut content);
        self.expression = Some(content);
    }

    /// Shift numbered registers (1-9) for delete operations.
    ///
    /// Register 9 falls off, 8→9, 7→8, ..., 1→2, new→1.
    fn shift_numbered(&mut self, new_content: RegisterContent) {
        // Shift from end: 8→9, 7→8, etc.
        for i in (1..9).rev() {
            let taken = self.numbered.get_mut(i - 1).and_then(Option::take);
            if let Some(slot) = self.numbered.get_mut(i) {
                *slot = taken;
            }
        }
        // Set register 1
        if let Some(slot) = self.numbered.first_mut() {
            *slot = Some(new_content);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shorthand for tests: wrap a char into a RegisterName.
    fn rn(c: char) -> RegisterName {
        RegisterName::new_unchecked(c)
    }

    #[test]
    fn test_unnamed_register() {
        let mut regs = Registers::new();
        let content = RegisterContent::char_wise("hello");

        regs.set(rn('"'), content.clone());
        assert_eq!(regs.get(rn('"')).unwrap().text(), "hello");
    }

    #[test]
    fn test_yank_register() {
        let mut regs = Registers::new();
        let content = RegisterContent::char_wise("yanked");

        regs.on_yank(content, None);

        // Both unnamed and 0 should be set
        assert_eq!(regs.get(rn('"')).unwrap().text(), "yanked");
        assert_eq!(regs.get(rn('0')).unwrap().text(), "yanked");
    }

    #[test]
    fn test_yank_explicit_register() {
        let mut regs = Registers::new();
        let content = RegisterContent::char_wise("yanked");

        regs.set(rn('a'), content.clone());
        regs.on_yank(content, Some(rn('a')));

        // Unnamed should be set, but not 0
        assert_eq!(regs.get(rn('"')).unwrap().text(), "yanked");
        assert!(regs.get(rn('0')).is_none());
        assert_eq!(regs.get(rn('a')).unwrap().text(), "yanked");
    }

    #[test]
    fn test_delete_linewise_shifts() {
        let mut regs = Registers::new();

        // Delete three lines in sequence
        regs.on_delete(RegisterContent::line_wise("line1\n"), MotionType::LineWise);
        regs.on_delete(RegisterContent::line_wise("line2\n"), MotionType::LineWise);
        regs.on_delete(RegisterContent::line_wise("line3\n"), MotionType::LineWise);

        // Most recent should be in register 1
        assert_eq!(regs.get(rn('1')).unwrap().text(), "line3\n");
        assert_eq!(regs.get(rn('2')).unwrap().text(), "line2\n");
        assert_eq!(regs.get(rn('3')).unwrap().text(), "line1\n");
    }

    #[test]
    fn test_delete_small() {
        let mut regs = Registers::new();
        let content = RegisterContent::char_wise("x");

        regs.on_delete(content, MotionType::CharWise);

        // Small delete should be set
        assert_eq!(regs.get(rn('-')).unwrap().text(), "x");
        // Numbered should not be affected
        assert!(regs.get(rn('1')).is_none());
    }

    #[test]
    fn test_named_register_append() {
        let mut regs = Registers::new();

        regs.set(rn('a'), RegisterContent::char_wise("hello"));
        regs.set(rn('A'), RegisterContent::char_wise(" world"));

        assert_eq!(regs.get(rn('a')).unwrap().text(), "hello world");
    }

    #[test]
    fn test_black_hole() {
        let mut regs = Registers::new();

        regs.set(rn('_'), RegisterContent::char_wise("discarded"));

        assert!(regs.get(rn('_')).is_none());
    }

    #[test]
    fn test_case_insensitive_get() {
        let mut regs = Registers::new();

        regs.set(rn('a'), RegisterContent::char_wise("test"));

        // Both 'a' and 'A' should return the same content
        assert_eq!(regs.get(rn('a')).unwrap().text(), "test");
        assert_eq!(regs.get(rn('A')).unwrap().text(), "test");
    }

    // ========== FIDELITY TESTS ==========

    /// Test that numbered registers 1-9 shift correctly with limit
    #[test]
    fn test_numbered_register_shift_limit() {
        let mut regs = Registers::new();

        // Delete 10 lines - only 1-9 should be kept
        for i in 1..=10 {
            regs.on_delete(
                RegisterContent::line_wise(format!("line{}\n", i)),
                MotionType::LineWise,
            );
        }

        // Register 1 should have the most recent (line10)
        assert_eq!(regs.get(rn('1')).unwrap().text(), "line10\n");
        // Register 9 should have line2 (line1 was shifted out)
        assert_eq!(regs.get(rn('9')).unwrap().text(), "line2\n");
    }

    /// Test that yank doesn't affect numbered registers
    #[test]
    fn test_yank_doesnt_affect_numbered() {
        let mut regs = Registers::new();

        // First delete to populate register 1
        regs.on_delete(
            RegisterContent::line_wise("deleted\n"),
            MotionType::LineWise,
        );
        assert_eq!(regs.get(rn('1')).unwrap().text(), "deleted\n");

        // Yank should not shift numbered registers
        regs.on_yank(RegisterContent::char_wise("yanked"), None);

        // Register 1 should still be the delete
        assert_eq!(regs.get(rn('1')).unwrap().text(), "deleted\n");
        // Register 0 should have the yank
        assert_eq!(regs.get(rn('0')).unwrap().text(), "yanked");
    }

    /// Test multiple small deletes overwrite (don't shift)
    #[test]
    fn test_small_delete_overwrites() {
        let mut regs = Registers::new();

        regs.on_delete(RegisterContent::char_wise("first"), MotionType::CharWise);
        regs.on_delete(RegisterContent::char_wise("second"), MotionType::CharWise);

        // Small delete register should have most recent
        assert_eq!(regs.get(rn('-')).unwrap().text(), "second");
        // Numbered registers should not be affected
        assert!(regs.get(rn('1')).is_none());
    }

    /// Test all 26 named registers a-z
    #[test]
    fn test_all_named_registers() {
        let mut regs = Registers::new();

        for c in 'a'..='z' {
            regs.set(rn(c), RegisterContent::char_wise(format!("content_{}", c)));
        }

        for c in 'a'..='z' {
            assert_eq!(regs.get(rn(c)).unwrap().text(), format!("content_{}", c));
        }
    }

    /// Test append mode with newline preservation
    #[test]
    fn test_append_preserves_linewise() {
        let mut regs = Registers::new();

        regs.set(rn('a'), RegisterContent::line_wise("first line\n"));
        regs.set(rn('A'), RegisterContent::line_wise("second line\n"));

        let content = regs.get(rn('a')).unwrap();
        assert_eq!(content.text(), "first line\nsecond line\n");
        // Should preserve linewise mode
        use crate::primitives::MotionType;
        assert_eq!(content.motion_type(), MotionType::LineWise);
    }

    /// Test yank register 0 doesn't change on delete
    #[test]
    fn test_yank_register_stable_after_delete() {
        let mut regs = Registers::new();

        // Yank first
        regs.on_yank(RegisterContent::char_wise("yanked text"), None);
        assert_eq!(regs.get(rn('0')).unwrap().text(), "yanked text");

        // Delete should not overwrite yank register
        regs.on_delete(
            RegisterContent::line_wise("deleted\n"),
            MotionType::LineWise,
        );

        // Yank register should be unchanged
        assert_eq!(regs.get(rn('0')).unwrap().text(), "yanked text");
    }

    /// Test unnamed register tracks both yank and delete
    #[test]
    fn test_unnamed_tracks_all_ops() {
        let mut regs = Registers::new();

        regs.on_yank(RegisterContent::char_wise("yanked"), None);
        assert_eq!(regs.get(rn('"')).unwrap().text(), "yanked");

        regs.on_delete(RegisterContent::char_wise("deleted"), MotionType::CharWise);
        assert_eq!(regs.get(rn('"')).unwrap().text(), "deleted");
    }

    /// Test explicit register preserves yank register 0
    #[test]
    fn test_explicit_register_yank() {
        let mut regs = Registers::new();

        // First yank to set register 0
        regs.on_yank(RegisterContent::char_wise("first"), None);
        assert_eq!(regs.get(rn('0')).unwrap().text(), "first");

        // Yank to explicit register should not change 0
        regs.set(rn('b'), RegisterContent::char_wise("explicit"));
        regs.on_yank(RegisterContent::char_wise("explicit"), Some(rn('b')));

        // Register 0 should be unchanged
        assert_eq!(regs.get(rn('0')).unwrap().text(), "first");
    }

    /// Test that setting a named register overwrites previous content
    #[test]
    fn test_named_register_overwrite() {
        let mut regs = Registers::new();

        regs.set(rn('a'), RegisterContent::char_wise("first"));
        assert_eq!(regs.get(rn('a')).unwrap().text(), "first");

        regs.set(rn('a'), RegisterContent::char_wise("second"));
        assert_eq!(regs.get(rn('a')).unwrap().text(), "second");
    }

    /// Test linewise vs charwise preservation
    #[test]
    fn test_register_type_preservation() {
        let mut regs = Registers::new();

        regs.set(rn('a'), RegisterContent::line_wise("line\n"));
        regs.set(rn('b'), RegisterContent::char_wise("char"));

        use crate::primitives::MotionType;
        assert_eq!(
            regs.get(rn('a')).unwrap().motion_type(),
            MotionType::LineWise
        );
        assert_eq!(
            regs.get(rn('b')).unwrap().motion_type(),
            MotionType::CharWise
        );
    }

    /// Test special read-only registers return None when written
    #[test]
    fn test_readonly_registers_reject_write() {
        let mut regs = Registers::new();

        // These registers are read-only (handled by shell)
        for c in ['%', '#', ':', '.', '/'] {
            regs.set(rn(c), RegisterContent::char_wise("attempt"));
            // Read-only registers should not store content locally
            // Shell handles reading these
        }
    }

    // Mutation-killing tests - targeted to catch surviving mutants

    /// Test blackhole register discards content (catches match arm deletion in set)
    #[test]
    fn test_blackhole_discards() {
        let mut regs = Registers::new();
        regs.set(rn('_'), RegisterContent::char_wise("should disappear"));
        assert!(regs.get(rn('_')).is_none(), "blackhole should return None");
    }

    /// Test yank register (0) explicitly set and retrieved (catches match arm deletion)
    #[test]
    fn test_yank_register_explicit_set() {
        let mut regs = Registers::new();
        regs.set(rn('0'), RegisterContent::char_wise("yanked content"));
        let result = regs.get(rn('0'));
        assert!(result.is_some(), "0 register should have content");
        assert_eq!(result.unwrap().text(), "yanked content");
    }

    /// Test numbered register arithmetic (catches - with + or /)
    #[test]
    fn test_numbered_register_indexing() {
        let mut regs = Registers::new();
        // Set each numbered register explicitly
        for (i, c) in ('1'..='9').enumerate() {
            let content = format!("content{}", i + 1);
            regs.set(rn(c), RegisterContent::char_wise(&content));
        }
        // Verify each is stored correctly
        for (i, c) in ('1'..='9').enumerate() {
            let result = regs.get(rn(c));
            assert!(result.is_some(), "register {} should exist", c);
            assert_eq!(result.unwrap().text(), format!("content{}", i + 1));
        }
    }

    /// Test small delete register (catches match arm deletion)
    #[test]
    fn test_small_delete_explicit() {
        let mut regs = Registers::new();
        regs.set(rn('-'), RegisterContent::char_wise("small delete"));
        let result = regs.get(rn('-'));
        assert!(result.is_some(), "- register should exist");
        assert_eq!(result.unwrap().text(), "small delete");
    }

    /// Test unknown register returns None (catches wildcard deletion in get)
    #[test]
    fn test_unknown_register_returns_none() {
        let regs = Registers::new();
        assert!(regs.get(rn('$')).is_none(), "$ is not a valid register");
        assert!(regs.get(rn('@')).is_none(), "@ is not a valid register");
    }

    /// Test version counter increments on mutations.
    #[test]
    fn test_version_counter() {
        let mut regs = Registers::new();
        assert_eq!(regs.version(), 0);

        regs.set(rn('a'), RegisterContent::char_wise("test"));
        assert_eq!(regs.version(), 1);

        regs.on_yank(RegisterContent::char_wise("yanked"), None);
        assert_eq!(regs.version(), 2);

        regs.on_delete(RegisterContent::char_wise("deleted"), MotionType::CharWise);
        assert_eq!(regs.version(), 3);
    }

    // ========== Expression register (=) tests ==========

    /// Test expression register set and get via set_expression_result.
    #[test]
    fn test_expression_register_set_and_get() {
        let mut regs = Registers::new();
        assert!(
            regs.get(rn('=')).is_none(),
            "expression register starts empty"
        );

        regs.set_expression_result(RegisterContent::char_wise("42"));
        let result = regs.get(rn('='));
        assert!(result.is_some(), "expression register should have content");
        assert_eq!(result.unwrap().text(), "42");
    }

    /// Test expression register via normal set() path.
    #[test]
    fn test_expression_register_via_set() {
        let mut regs = Registers::new();
        regs.set(rn('='), RegisterContent::char_wise("result"));
        let result = regs.get(rn('='));
        assert!(result.is_some());
        assert_eq!(result.unwrap().text(), "result");
    }

    /// Test expression register overwrites previous value.
    #[test]
    fn test_expression_register_overwrite() {
        let mut regs = Registers::new();
        regs.set_expression_result(RegisterContent::char_wise("first"));
        assert_eq!(regs.get(rn('=')).unwrap().text(), "first");

        regs.set_expression_result(RegisterContent::char_wise("second"));
        assert_eq!(regs.get(rn('=')).unwrap().text(), "second");
    }

    /// Test expression register increments version.
    #[test]
    fn test_expression_register_version() {
        let mut regs = Registers::new();
        let v0 = regs.version();
        regs.set_expression_result(RegisterContent::char_wise("expr"));
        assert_eq!(regs.version(), v0 + 1);
    }

    /// Test expression register is independent of other registers.
    #[test]
    fn test_expression_register_independent() {
        let mut regs = Registers::new();
        regs.set(rn('a'), RegisterContent::char_wise("named"));
        regs.set_expression_result(RegisterContent::char_wise("expr"));

        assert_eq!(regs.get(rn('a')).unwrap().text(), "named");
        assert_eq!(regs.get(rn('=')).unwrap().text(), "expr");

        // Setting named register doesn't affect expression
        regs.set(rn('a'), RegisterContent::char_wise("updated"));
        assert_eq!(regs.get(rn('=')).unwrap().text(), "expr");
    }

    // ========== REGISTER TIMESTAMPS TESTS ==========

    #[test]
    fn task_5_11_timestamp_nonzero_after_set() {
        let mut regs = Registers::new();
        regs.set(rn('a'), RegisterContent::char_wise("hello"));
        let ts = regs.get(rn('a')).unwrap().timestamp();
        assert!(ts > 0, "timestamp must be non-zero after write");
    }

    #[test]
    fn task_5_11_timestamp_monotonically_increasing() {
        let mut regs = Registers::new();
        regs.set(rn('a'), RegisterContent::char_wise("first"));
        let ts1 = regs.get(rn('a')).unwrap().timestamp();

        regs.set(rn('b'), RegisterContent::char_wise("second"));
        let ts2 = regs.get(rn('b')).unwrap().timestamp();

        regs.set(rn('c'), RegisterContent::char_wise("third"));
        let ts3 = regs.get(rn('c')).unwrap().timestamp();

        assert!(ts2 > ts1, "timestamps must be monotonically increasing");
        assert!(ts3 > ts2, "timestamps must be monotonically increasing");
    }

    #[test]
    fn task_5_11_timestamp_on_yank() {
        let mut regs = Registers::new();
        regs.on_yank(RegisterContent::char_wise("yanked"), None);
        let ts = regs.get(rn('"')).unwrap().timestamp();
        assert!(ts > 0, "yank should stamp timestamp");
        // Yank register and unnamed should have the same timestamp
        assert_eq!(
            regs.get(rn('0')).unwrap().timestamp(),
            ts,
            "yank register should have same timestamp as unnamed"
        );
    }

    #[test]
    fn task_5_11_timestamp_on_delete() {
        let mut regs = Registers::new();
        regs.on_delete(RegisterContent::char_wise("deleted"), MotionType::CharWise);
        let ts = regs.get(rn('-')).unwrap().timestamp();
        assert!(ts > 0, "delete should stamp timestamp");
    }

    #[test]
    fn task_5_11_timestamp_on_expression() {
        let mut regs = Registers::new();
        regs.set_expression_result(RegisterContent::char_wise("42"));
        let ts = regs.get(rn('=')).unwrap().timestamp();
        assert!(ts > 0, "expression register should stamp timestamp");
    }

    #[test]
    fn task_5_11_timestamp_default_is_zero() {
        let content = RegisterContent::char_wise("test");
        assert_eq!(content.timestamp(), 0, "default timestamp must be 0");
    }

    #[test]
    fn task_5_11_timestamp_increases_across_operations() {
        let mut regs = Registers::new();
        regs.set(rn('a'), RegisterContent::char_wise("first"));
        let ts_set = regs.get(rn('a')).unwrap().timestamp();

        regs.on_yank(RegisterContent::char_wise("yanked"), None);
        let ts_yank = regs.get(rn('"')).unwrap().timestamp();

        regs.on_delete(RegisterContent::line_wise("line\n"), MotionType::LineWise);
        let ts_delete = regs.get(rn('1')).unwrap().timestamp();

        assert!(ts_yank > ts_set);
        assert!(ts_delete > ts_yank);
    }
}
