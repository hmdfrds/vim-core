//! Abbreviation table and types for vim-core.
//!
//! Types representing abbreviation entries, their classification,
//! mode applicability, and the table used for O(1) exact-match lookup.

use compact_str::CompactString;
use smallvec::SmallVec;
use std::collections::HashMap;

/// Where an abbreviation applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AbbrevMode {
    /// Insert mode only (:iabbrev)
    Insert,
    /// Command-line mode only (:cabbrev)
    CommandLine,
    /// Both modes (:abbreviate)
    Both,
}

/// Classification of the trigger word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AbbrevKind {
    /// All chars are keyword chars. Example: "teh" → "the"
    FullId,
    /// Last char is keyword, earlier may not be. Example: "#i" → "#include"
    EndId,
    /// Last char is NOT keyword. Example: "..." → "…"
    NonId,
}

impl AbbrevKind {
    /// Classify a trigger word based on its character composition.
    pub fn classify(trigger: &str, is_keyword: impl Fn(char) -> bool) -> Self {
        let mut chars = trigger.chars();
        let last = match chars.next_back() {
            Some(c) => c,
            None => return Self::FullId,
        };
        let last_is_keyword = is_keyword(last);
        if !last_is_keyword {
            return Self::NonId;
        }
        // last is keyword — check if ALL are keyword
        if trigger.chars().all(&is_keyword) {
            Self::FullId
        } else {
            Self::EndId
        }
    }
}

/// A single abbreviation entry.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AbbrevEntry {
    /// The trigger text that activates the abbreviation.
    pub trigger: CompactString,
    /// The text the trigger expands to.
    pub replacement: CompactString,
    /// Which mode(s) the abbreviation is active in.
    pub mode: AbbrevMode,
    /// Classification of the trigger based on its character composition.
    pub kind: AbbrevKind,
    /// Whether remapping is suppressed (noremap variant).
    pub noremap: bool,
}

impl AbbrevMode {
    const fn bits(self) -> u8 {
        match self {
            Self::Insert => 1,
            Self::CommandLine => 2,
            Self::Both => 3,
        }
    }

    const fn from_bits(bits: u8) -> Option<Self> {
        match bits & 3 {
            1 => Some(Self::Insert),
            2 => Some(Self::CommandLine),
            3 => Some(Self::Both),
            _ => None,
        }
    }

    const fn overlaps(self, other: Self) -> bool {
        self.bits() & other.bits() != 0
    }

    /// `self` without the modes of `other`; `None` when nothing is left.
    const fn without(self, other: Self) -> Option<Self> {
        Self::from_bits(self.bits() & !other.bits())
    }
}

/// The abbreviation table. HashMap for O(1) exact-match lookup.
///
/// A trigger has one entry per mode, as in Vim, where each abbreviation has
/// mode bits: `:iabbrev a b` and `:cabbrev a c` keep both, and
/// `:iunabbrev` on an `:abbreviate` entry leaves its command-line part.
#[derive(Debug, Clone, Default)]
pub struct AbbrevTable {
    entries: HashMap<CompactString, SmallVec<[AbbrevEntry; 1]>>,
}

impl AbbrevTable {
    /// Add an abbreviation. An entry with the same trigger loses the modes
    /// of the new one, and goes when it has none left, as in Vim's
    /// `do_map()`.
    pub fn add(&mut self, entry: AbbrevEntry) {
        let mode = entry.mode;
        let same = self.entries.entry(entry.trigger.clone()).or_default();
        same.retain(|old| narrow(old, mode));
        same.push(entry);
    }

    /// Take the modes of `mode` off the abbreviations of `trigger`; one
    /// left with no mode is removed, as in Vim's `do_map()`.
    ///
    /// Returns `true` if an abbreviation had one of those modes.
    pub fn remove(&mut self, trigger: &str, mode: AbbrevMode) -> bool {
        let Some(same) = self.entries.get_mut(trigger) else {
            return false;
        };
        let found = same.iter().any(|entry| entry.mode.overlaps(mode));
        same.retain(|entry| narrow(entry, mode));
        if same.is_empty() {
            self.entries.remove(trigger);
        }
        found
    }

    /// Remove the abbreviations that expand to `replacement`, by mode as
    /// [`remove`](Self::remove) does. Whether there was one.
    pub(crate) fn remove_expanding_to(&mut self, replacement: &str, mode: AbbrevMode) -> bool {
        self.narrow_where(mode, |entry| entry.replacement == replacement)
    }

    /// Take the modes of `mode` off every abbreviation, as Vim's
    /// `map_clear_mode()` does: `Both` removes them all.
    pub fn clear(&mut self, mode: AbbrevMode) {
        self.narrow_where(mode, |_| true);
    }

    fn narrow_where(&mut self, mode: AbbrevMode, pick: impl Fn(&AbbrevEntry) -> bool) -> bool {
        let mut found = false;
        self.entries.retain(|_, same| {
            same.retain(|entry| {
                if !pick(entry) || !entry.mode.overlaps(mode) {
                    return true;
                }
                found = true;
                narrow(entry, mode)
            });
            !same.is_empty()
        });
        found
    }

    /// Whether `trigger` is an abbreviation in one of the modes of `mode`.
    #[must_use]
    pub fn defines(&self, trigger: &str, mode: AbbrevMode) -> bool {
        self.find(trigger, mode).is_some()
    }

    /// Look up an abbreviation by exact trigger text, in any mode.
    #[must_use]
    pub fn get(&self, trigger: &str) -> Option<&AbbrevEntry> {
        self.entries.get(trigger).and_then(|same| same.first())
    }

    /// The abbreviation of `trigger` in one of the modes of `mode`.
    pub(crate) fn find(&self, trigger: &str, mode: AbbrevMode) -> Option<&AbbrevEntry> {
        self.entries
            .get(trigger)?
            .iter()
            .find(|entry| entry.mode.overlaps(mode))
    }

    /// List the abbreviations that apply in one of the modes of `mode`,
    /// as Vim lists them: `Both` lists every one.
    #[must_use]
    pub fn list(&self, mode: AbbrevMode) -> Vec<&AbbrevEntry> {
        self.entries
            .values()
            .flatten()
            .filter(|e| e.mode.overlaps(mode))
            .collect()
    }

    /// Returns `true` if the table contains no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Check if the just-inserted character triggers an abbreviation expansion.
    ///
    /// Mirrors Vim's `check_abbr()` two-pass candidate extraction:
    ///
    /// 1. **Keyword walk**: walk backward through keyword chars to get a short
    ///    candidate. This handles FullId abbreviations (e.g., `"teh"` -> `"the"`).
    ///
    /// 2. **Full walk**: continue backward through non-keyword non-whitespace chars
    ///    to get a longer candidate. This handles EndId abbreviations where the
    ///    trigger starts with non-keyword chars (e.g., `"#i"` -> `"#include"`).
    ///
    /// 3. **Non-keyword walk**: if the last char is non-keyword, walk backward
    ///    through non-keyword non-whitespace chars. This handles NonId abbreviations
    ///    (e.g., `"..."` -> `"\u{2026}"`).
    ///
    /// After finding a match, boundary conditions are verified:
    ///
    /// - **FullId/EndId**: trigger char must be non-keyword; preceding char must be
    ///   non-keyword, whitespace, or start-of-text.
    /// - **NonId**: trigger char must be keyword; preceding char must be keyword,
    ///   whitespace, or start-of-text.
    ///
    /// Returns `Some((byte_count_to_delete, replacement_text))` if expansion should
    /// occur, or `None` if no abbreviation matches.
    ///
    /// `mode` selects which abbreviations are eligible (Insert, CommandLine, or Both).
    pub fn try_expand(
        &self,
        inserted_char: char,
        text_before_cursor: &str,
        mode: AbbrevMode,
        is_keyword: &dyn Fn(char) -> bool,
    ) -> Option<(usize, CompactString)> {
        if self.entries.is_empty() || text_before_cursor.is_empty() {
            return None;
        }

        let last_char = text_before_cursor.chars().next_back()?;
        if last_char.is_whitespace() {
            return None;
        }

        if is_keyword(last_char) {
            // Step 1: keyword walk — extract keyword-only suffix.
            let kw_start = text_before_cursor
                .char_indices()
                .rev()
                .take_while(|&(_, c)| is_keyword(c))
                .last()
                .map_or(0, |(i, _)| i);

            // Try FullId match with keyword-only candidate.
            let kw_candidate = &text_before_cursor[kw_start..];
            if let Some(result) = self.try_match_candidate(
                kw_candidate,
                kw_start,
                text_before_cursor,
                inserted_char,
                mode,
                is_keyword,
            ) {
                return Some(result);
            }

            // Step 2: full walk — extend backward through non-keyword non-whitespace
            // chars to handle EndId triggers (e.g., "#i").
            if kw_start > 0 {
                let full_start = text_before_cursor[..kw_start]
                    .char_indices()
                    .rev()
                    .take_while(|&(_, c)| !is_keyword(c) && !c.is_whitespace())
                    .last()
                    .map_or(kw_start, |(i, _)| i);

                if full_start < kw_start {
                    let full_candidate = &text_before_cursor[full_start..];
                    if let Some(result) = self.try_match_candidate(
                        full_candidate,
                        full_start,
                        text_before_cursor,
                        inserted_char,
                        mode,
                        is_keyword,
                    ) {
                        return Some(result);
                    }
                }
            }
        } else {
            // Step 3: non-keyword walk — extract non-keyword non-whitespace suffix.
            let nkw_start = text_before_cursor
                .char_indices()
                .rev()
                .take_while(|&(_, c)| !is_keyword(c) && !c.is_whitespace())
                .last()
                .map_or(0, |(i, _)| i);

            let nkw_candidate = &text_before_cursor[nkw_start..];
            if let Some(result) = self.try_match_candidate(
                nkw_candidate,
                nkw_start,
                text_before_cursor,
                inserted_char,
                mode,
                is_keyword,
            ) {
                return Some(result);
            }
        }

        None
    }

    /// Try matching a candidate against the table and verify boundary conditions.
    ///
    /// Returns `Some((byte_count_to_delete, replacement))` on success.
    fn try_match_candidate(
        &self,
        candidate: &str,
        word_start: usize,
        text_before_cursor: &str,
        inserted_char: char,
        mode: AbbrevMode,
        is_keyword: &dyn Fn(char) -> bool,
    ) -> Option<(usize, CompactString)> {
        if candidate.is_empty() {
            return None;
        }

        let entry = self.find(candidate, mode)?;

        // Verify boundary conditions per kind
        match entry.kind {
            AbbrevKind::FullId | AbbrevKind::EndId => {
                // Trigger char must be non-keyword
                if is_keyword(inserted_char) {
                    return None;
                }
                // Must be preceded by non-keyword, whitespace, or start of text
                if word_start > 0 {
                    let before = text_before_cursor[..word_start].chars().next_back()?;
                    if is_keyword(before) {
                        return None;
                    }
                }
            }
            AbbrevKind::NonId => {
                // Trigger char must be keyword
                if !is_keyword(inserted_char) {
                    return None;
                }
                // Must be preceded by keyword, whitespace, or start of text
                if word_start > 0 {
                    let before = text_before_cursor[..word_start].chars().next_back()?;
                    if !is_keyword(before) && !before.is_whitespace() {
                        return None;
                    }
                }
            }
        }

        Some((candidate.len(), entry.replacement.clone()))
    }
}

/// Take the modes of `mode` off `entry`; whether it keeps one.
const fn narrow(entry: &mut AbbrevEntry, mode: AbbrevMode) -> bool {
    match entry.mode.without(mode) {
        Some(rest) => {
            entry.mode = rest;
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_kw(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    #[test]
    fn classify_full_id() {
        assert_eq!(AbbrevKind::classify("teh", is_kw), AbbrevKind::FullId);
        assert_eq!(AbbrevKind::classify("abc123", is_kw), AbbrevKind::FullId);
    }

    #[test]
    fn classify_end_id() {
        assert_eq!(AbbrevKind::classify("#i", is_kw), AbbrevKind::EndId);
        assert_eq!(AbbrevKind::classify("..a", is_kw), AbbrevKind::EndId);
    }

    #[test]
    fn classify_non_id() {
        assert_eq!(AbbrevKind::classify("...", is_kw), AbbrevKind::NonId);
        assert_eq!(AbbrevKind::classify("a.", is_kw), AbbrevKind::NonId);
    }

    #[test]
    fn add_and_get() {
        let mut table = AbbrevTable::default();
        table.add(AbbrevEntry {
            trigger: "teh".into(),
            replacement: "the".into(),
            mode: AbbrevMode::Both,
            kind: AbbrevKind::FullId,
            noremap: false,
        });
        assert!(table.get("teh").is_some());
        assert!(table.get("nope").is_none());
    }

    #[test]
    fn remove_by_trigger() {
        let mut table = AbbrevTable::default();
        table.add(AbbrevEntry {
            trigger: "teh".into(),
            replacement: "the".into(),
            mode: AbbrevMode::Both,
            kind: AbbrevKind::FullId,
            noremap: false,
        });
        assert!(table.remove("teh", AbbrevMode::Both));
        assert!(table.get("teh").is_none());
    }

    #[test]
    fn clear_by_mode() {
        let mut table = AbbrevTable::default();
        table.add(AbbrevEntry {
            trigger: "a".into(),
            replacement: "b".into(),
            mode: AbbrevMode::Insert,
            kind: AbbrevKind::FullId,
            noremap: false,
        });
        table.add(AbbrevEntry {
            trigger: "c".into(),
            replacement: "d".into(),
            mode: AbbrevMode::CommandLine,
            kind: AbbrevKind::FullId,
            noremap: false,
        });
        table.clear(AbbrevMode::Insert);
        assert!(table.get("a").is_none());
        assert!(table.get("c").is_some());
    }

    #[test]
    fn list_filters_by_mode() {
        let mut table = AbbrevTable::default();
        table.add(AbbrevEntry {
            trigger: "a".into(),
            replacement: "b".into(),
            mode: AbbrevMode::Insert,
            kind: AbbrevKind::FullId,
            noremap: false,
        });
        table.add(AbbrevEntry {
            trigger: "c".into(),
            replacement: "d".into(),
            mode: AbbrevMode::Both,
            kind: AbbrevKind::FullId,
            noremap: false,
        });
        let insert_list = table.list(AbbrevMode::Insert);
        assert_eq!(insert_list.len(), 2); // "a" (Insert) + "c" (Both)
        let cmdline_list = table.list(AbbrevMode::CommandLine);
        assert_eq!(cmdline_list.len(), 1); // only "c" (Both)
    }

    fn entry(trigger: &str, replacement: &str, mode: AbbrevMode) -> AbbrevEntry {
        AbbrevEntry {
            trigger: trigger.into(),
            replacement: replacement.into(),
            mode,
            kind: AbbrevKind::FullId,
            noremap: false,
        }
    }

    fn replacement(table: &AbbrevTable, trigger: &str, mode: AbbrevMode) -> Option<String> {
        table
            .find(trigger, mode)
            .map(|entry| entry.replacement.to_string())
    }

    #[test]
    fn each_mode_keeps_its_own_entry() {
        // do_map() adds an entry when no entry of the trigger has the mode.
        let mut table = AbbrevTable::default();
        table.add(entry("a", "b", AbbrevMode::Insert));
        table.add(entry("a", "c", AbbrevMode::CommandLine));
        assert_eq!(
            replacement(&table, "a", AbbrevMode::Insert).as_deref(),
            Some("b")
        );
        assert_eq!(
            replacement(&table, "a", AbbrevMode::CommandLine).as_deref(),
            Some("c")
        );
        assert_eq!(table.list(AbbrevMode::Both).len(), 2);
        assert_eq!(
            table.try_expand(' ', "a", AbbrevMode::Insert, &is_kw),
            Some((1, "b".into()))
        );
    }

    #[test]
    fn defining_one_mode_narrows_an_entry_of_both() {
        // do_map() takes the new mode bits off the old entry.
        let mut table = AbbrevTable::default();
        table.add(entry("a", "b", AbbrevMode::Both));
        table.add(entry("a", "x", AbbrevMode::Insert));
        assert_eq!(
            replacement(&table, "a", AbbrevMode::Insert).as_deref(),
            Some("x")
        );
        assert_eq!(
            replacement(&table, "a", AbbrevMode::CommandLine).as_deref(),
            Some("b")
        );

        table.add(entry("a", "y", AbbrevMode::Both));
        assert_eq!(table.list(AbbrevMode::Both).len(), 1);
        assert_eq!(
            replacement(&table, "a", AbbrevMode::CommandLine).as_deref(),
            Some("y")
        );
    }

    #[test]
    fn remove_takes_off_only_the_modes_named() {
        let mut table = AbbrevTable::default();
        table.add(entry("e", "f", AbbrevMode::Both));
        assert!(table.remove("e", AbbrevMode::Insert));
        assert!(!table.defines("e", AbbrevMode::Insert));
        assert!(table.defines("e", AbbrevMode::CommandLine));
        assert!(!table.remove("e", AbbrevMode::Insert));
        assert!(table.remove("e", AbbrevMode::Both));
        assert!(table.is_empty());
    }

    #[test]
    fn remove_expanding_to_takes_off_only_the_modes_named() {
        let mut table = AbbrevTable::default();
        table.add(entry("e", "f", AbbrevMode::Both));
        assert!(table.remove_expanding_to("f", AbbrevMode::CommandLine));
        assert_eq!(
            replacement(&table, "e", AbbrevMode::Insert).as_deref(),
            Some("f")
        );
        assert!(!table.defines("e", AbbrevMode::CommandLine));
    }

    #[test]
    fn clear_both_removes_every_entry() {
        // map_clear_mode() with INSERT and CMDLINE frees each entry.
        let mut table = AbbrevTable::default();
        table.add(entry("a", "b", AbbrevMode::Insert));
        table.add(entry("c", "d", AbbrevMode::CommandLine));
        table.add(entry("e", "f", AbbrevMode::Both));
        table.clear(AbbrevMode::Both);
        assert!(table.is_empty());
    }

    #[test]
    fn clear_one_mode_keeps_the_other_part_of_both() {
        let mut table = AbbrevTable::default();
        table.add(entry("e", "f", AbbrevMode::Both));
        table.clear(AbbrevMode::Insert);
        assert!(!table.defines("e", AbbrevMode::Insert));
        assert!(table.defines("e", AbbrevMode::CommandLine));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // try_expand tests
    // ═══════════════════════════════════════════════════════════════════════

    fn make_table_with(trigger: &str, replacement: &str, kind: AbbrevKind) -> AbbrevTable {
        let mut table = AbbrevTable::default();
        table.add(AbbrevEntry {
            trigger: trigger.into(),
            replacement: replacement.into(),
            mode: AbbrevMode::Both,
            kind,
            noremap: false,
        });
        table
    }

    #[test]
    fn expand_full_id_on_space() {
        let table = make_table_with("teh", "the", AbbrevKind::FullId);
        let result = table.try_expand(' ', "teh", AbbrevMode::Insert, &is_kw);
        assert_eq!(result, Some((3, "the".into())));
    }

    #[test]
    fn no_expand_when_followed_by_keyword() {
        let table = make_table_with("teh", "the", AbbrevKind::FullId);
        // 'x' is a keyword char, should NOT trigger FullId expansion
        let result = table.try_expand('x', "teh", AbbrevMode::Insert, &is_kw);
        assert_eq!(result, None);
    }

    #[test]
    fn expand_end_id() {
        let table = make_table_with("#i", "#include", AbbrevKind::EndId);
        let result = table.try_expand(' ', "#i", AbbrevMode::Insert, &is_kw);
        assert_eq!(result, Some((2, "#include".into())));
    }

    #[test]
    fn no_expand_full_id_after_keyword() {
        let table = make_table_with("teh", "the", AbbrevKind::FullId);
        // "xteh" — preceded by keyword 'x', should NOT expand
        let result = table.try_expand(' ', "xteh", AbbrevMode::Insert, &is_kw);
        assert_eq!(result, None);
    }

    #[test]
    fn expand_at_start_of_line() {
        let table = make_table_with("teh", "the", AbbrevKind::FullId);
        // Start of line (no text before candidate)
        let result = table.try_expand(' ', "teh", AbbrevMode::Insert, &is_kw);
        assert_eq!(result, Some((3, "the".into())));
    }

    #[test]
    fn expand_full_id_after_space() {
        let table = make_table_with("teh", "the", AbbrevKind::FullId);
        // "hello teh" — preceded by space (non-keyword), should expand
        let result = table.try_expand(' ', "hello teh", AbbrevMode::Insert, &is_kw);
        assert_eq!(result, Some((3, "the".into())));
    }

    #[test]
    fn expand_non_id_after_keyword() {
        let table = make_table_with("...", "\u{2026}", AbbrevKind::NonId);
        // NonId: "x..." — backward walk through non-keyword chars extracts "...",
        // preceded by keyword 'x', trigger char 'a' is keyword. Should expand.
        let result = table.try_expand('a', "x...", AbbrevMode::Insert, &is_kw);
        assert_eq!(result, Some((3, "\u{2026}".into())));
    }

    #[test]
    fn no_expand_non_id_after_non_keyword() {
        let table = make_table_with("...", "\u{2026}", AbbrevKind::NonId);
        // NonId: "!..." — preceded by non-keyword '!', should NOT expand
        let result = table.try_expand('a', "!...", AbbrevMode::Insert, &is_kw);
        assert_eq!(result, None);
    }

    #[test]
    fn expand_non_id_at_start_of_text() {
        let table = make_table_with("...", "\u{2026}", AbbrevKind::NonId);
        // Start of text — NonId allows this
        let result = table.try_expand('a', "...", AbbrevMode::Insert, &is_kw);
        assert_eq!(result, Some((3, "\u{2026}".into())));
    }

    #[test]
    fn no_expand_non_id_with_non_keyword_trigger() {
        let table = make_table_with("...", "\u{2026}", AbbrevKind::NonId);
        // NonId requires keyword trigger char; space is non-keyword
        let result = table.try_expand(' ', "...", AbbrevMode::Insert, &is_kw);
        assert_eq!(result, None);
    }

    #[test]
    fn no_expand_empty_text_before_cursor() {
        let table = make_table_with("teh", "the", AbbrevKind::FullId);
        let result = table.try_expand(' ', "", AbbrevMode::Insert, &is_kw);
        assert_eq!(result, None);
    }

    #[test]
    fn no_expand_empty_table() {
        let table = AbbrevTable::default();
        let result = table.try_expand(' ', "teh", AbbrevMode::Insert, &is_kw);
        assert_eq!(result, None);
    }

    #[test]
    fn no_expand_wrong_mode() {
        let mut table = AbbrevTable::default();
        table.add(AbbrevEntry {
            trigger: "teh".into(),
            replacement: "the".into(),
            mode: AbbrevMode::CommandLine,
            kind: AbbrevKind::FullId,
            noremap: false,
        });
        // Insert mode, but entry is CommandLine-only
        let result = table.try_expand(' ', "teh", AbbrevMode::Insert, &is_kw);
        assert_eq!(result, None);
    }

    #[test]
    fn expand_both_mode_in_insert() {
        let table = make_table_with("teh", "the", AbbrevKind::FullId);
        // Both mode should match Insert
        let result = table.try_expand(' ', "teh", AbbrevMode::Insert, &is_kw);
        assert_eq!(result, Some((3, "the".into())));
    }

    #[test]
    fn expand_both_mode_in_command_line() {
        let table = make_table_with("teh", "the", AbbrevKind::FullId);
        // Both mode should match CommandLine
        let result = table.try_expand(' ', "teh", AbbrevMode::CommandLine, &is_kw);
        assert_eq!(result, Some((3, "the".into())));
    }

    #[test]
    fn no_expand_no_match() {
        let table = make_table_with("teh", "the", AbbrevKind::FullId);
        let result = table.try_expand(' ', "xyz", AbbrevMode::Insert, &is_kw);
        assert_eq!(result, None);
    }

    #[test]
    fn expand_on_punctuation_trigger() {
        let table = make_table_with("teh", "the", AbbrevKind::FullId);
        // '.' is non-keyword, should trigger FullId expansion
        let result = table.try_expand('.', "teh", AbbrevMode::Insert, &is_kw);
        assert_eq!(result, Some((3, "the".into())));
    }

    #[test]
    fn expand_end_id_preceded_by_non_keyword() {
        let table = make_table_with("#i", "#include", AbbrevKind::EndId);
        // EndId: "#i" preceded by space
        let result = table.try_expand(' ', "code #i", AbbrevMode::Insert, &is_kw);
        assert_eq!(result, Some((2, "#include".into())));
    }

    #[test]
    fn expand_delete_count_matches_trigger_len() {
        let table = make_table_with("abbr", "abbreviation", AbbrevKind::FullId);
        let result = table.try_expand(' ', "abbr", AbbrevMode::Insert, &is_kw);
        assert_eq!(result, Some((4, "abbreviation".into())));
    }

    #[test]
    fn expand_multibyte_replacement() {
        let table = make_table_with("--", "\u{2014}", AbbrevKind::NonId);
        // NonId: "a--" — backward walk extracts "--" (non-keyword chars),
        // preceded by keyword 'a', trigger char 'b' is keyword. Should expand.
        let result = table.try_expand('b', "a--", AbbrevMode::Insert, &is_kw);
        assert_eq!(result, Some((2, "\u{2014}".into())));
    }

    #[test]
    fn no_expand_non_id_preceded_by_non_keyword_char() {
        let table = make_table_with(">>", "->", AbbrevKind::NonId);
        // NonId: "!>>" — backward walk extracts "!>>" (all non-keyword non-whitespace).
        // Candidate "!>>" doesn't match ">>" in table. No expansion.
        let result = table.try_expand('a', "!>>", AbbrevMode::Insert, &is_kw);
        assert_eq!(result, None);
    }

    #[test]
    fn expand_non_id_preceded_by_whitespace() {
        let table = make_table_with(">>", "->", AbbrevKind::NonId);
        // NonId: " >>" — backward walk stops at whitespace, candidate is ">>".
        // Preceded by whitespace = OK (like start of text). Trigger 'a' is keyword.
        let result = table.try_expand('a', " >>", AbbrevMode::Insert, &is_kw);
        assert_eq!(result, Some((2, "->".into())));
    }

    #[test]
    fn expand_full_id_preceded_by_whitespace() {
        let table = make_table_with("teh", "the", AbbrevKind::FullId);
        // Preceded by whitespace = OK for FullId
        let result = table.try_expand(' ', "  teh", AbbrevMode::Insert, &is_kw);
        assert_eq!(result, Some((3, "the".into())));
    }

    #[test]
    fn no_expand_whitespace_only_before_cursor() {
        let table = make_table_with("teh", "the", AbbrevKind::FullId);
        // Only whitespace before cursor, no candidate
        let result = table.try_expand(' ', "   ", AbbrevMode::Insert, &is_kw);
        assert_eq!(result, None);
    }

    #[test]
    fn expand_end_id_at_start_of_text() {
        let table = make_table_with("#i", "#include", AbbrevKind::EndId);
        // Start of text, EndId "#i"
        let result = table.try_expand(' ', "#i", AbbrevMode::Insert, &is_kw);
        assert_eq!(result, Some((2, "#include".into())));
    }

    #[test]
    fn no_expand_end_id_after_keyword() {
        let table = make_table_with("#i", "#include", AbbrevKind::EndId);
        // "x#i" — keyword walk extracts "i" (no match), full walk tries
        // to extend but '#' is preceded by 'x' (keyword). EndId boundary
        // check: preceded by keyword 'x' → fail.
        let result = table.try_expand(' ', "x#i", AbbrevMode::Insert, &is_kw);
        assert_eq!(result, None);
    }

    #[test]
    fn expand_insert_mode_only_entry() {
        let mut table = AbbrevTable::default();
        table.add(AbbrevEntry {
            trigger: "iab".into(),
            replacement: "insert_only".into(),
            mode: AbbrevMode::Insert,
            kind: AbbrevKind::FullId,
            noremap: false,
        });
        // Insert mode: should match
        let result = table.try_expand(' ', "iab", AbbrevMode::Insert, &is_kw);
        assert_eq!(result, Some((3, "insert_only".into())));
        // CommandLine mode: should NOT match
        let result = table.try_expand(' ', "iab", AbbrevMode::CommandLine, &is_kw);
        assert_eq!(result, None);
    }
}
