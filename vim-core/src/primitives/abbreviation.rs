//! Abbreviation table and types for vim-core.
//!
//! Types representing abbreviation entries, their classification,
//! mode applicability, and the table used for O(1) exact-match lookup.

use compact_str::CompactString;
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

/// The abbreviation table. HashMap for O(1) exact-match lookup.
#[derive(Debug, Clone, Default)]
pub struct AbbrevTable {
    entries: HashMap<CompactString, AbbrevEntry>,
}

impl AbbrevTable {
    /// Add or replace an abbreviation entry.
    pub fn add(&mut self, entry: AbbrevEntry) {
        self.entries.insert(entry.trigger.clone(), entry);
    }

    /// Remove an abbreviation by trigger and mode.
    ///
    /// Returns `true` if an entry was removed.
    /// Matches if the stored mode equals `mode`, or either side is `Both`.
    pub fn remove(&mut self, trigger: &str, mode: AbbrevMode) -> bool {
        if let Some(existing) = self.entries.get(trigger) {
            if existing.mode == mode
                || existing.mode == AbbrevMode::Both
                || mode == AbbrevMode::Both
            {
                self.entries.remove(trigger);
                return true;
            }
        }
        false
    }

    /// Remove all abbreviations that apply to the given mode.
    ///
    /// Entries with mode `Both` are also removed since they cover `mode`.
    pub fn clear(&mut self, mode: AbbrevMode) {
        self.entries
            .retain(|_, entry| entry.mode != mode && entry.mode != AbbrevMode::Both);
    }

    /// Look up an abbreviation by exact trigger text.
    #[must_use]
    pub fn get(&self, trigger: &str) -> Option<&AbbrevEntry> {
        self.entries.get(trigger)
    }

    /// List all abbreviations that apply to the given mode.
    ///
    /// Includes entries whose mode is `Both`.
    #[must_use]
    pub fn list(&self, mode: AbbrevMode) -> Vec<&AbbrevEntry> {
        self.entries
            .values()
            .filter(|e| e.mode == mode || e.mode == AbbrevMode::Both)
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

        let entry = self.entries.get(candidate)?;

        // Check mode applicability
        if entry.mode != mode && entry.mode != AbbrevMode::Both && mode != AbbrevMode::Both {
            return None;
        }

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
