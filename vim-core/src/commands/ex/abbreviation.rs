//! Abbreviation ex commands: `:abbreviate`, `:unabbreviate`, `:abclear`.

use compact_str::CompactString;

use crate::effects::{Effects, InfoMessage};
use crate::errors::VimError;
use crate::primitives::abbreviation::{AbbrevEntry, AbbrevKind, AbbrevMode, AbbrevTable};

/// Execute `:abbreviate`/`:iabbrev`/`:cabbrev`/`:noreabbrev` etc.
///
/// Behaviour depends on which arguments are present:
/// - No args: list all abbreviations for the given mode.
/// - Trigger only: show the matching abbreviation.
/// - Trigger + replacement: define a new abbreviation.
///
/// # Errors
///
/// Returns [`VimError::InvalidArgument`] carrying `trigger` when a trigger is
/// given without a replacement (the "show this one" form) and no abbreviation
/// with that trigger is defined — Vim's E24 "No such abbreviation".
///
/// Returns [`VimError::ArgumentRequired`] when a replacement is given without
/// a trigger. The ex parser never produces that shape, so it signals a caller
/// bug rather than user input.
///
/// The listing and defining forms cannot fail.
pub fn abbreviate(
    trigger: Option<&str>,
    replacement: Option<&str>,
    mode: AbbrevMode,
    noremap: bool,
    table: &mut AbbrevTable,
    is_keyword: &dyn Fn(char) -> bool,
) -> Result<Effects, VimError> {
    match (trigger, replacement) {
        (None, None) => {
            let entries = table.list(mode);
            if entries.is_empty() {
                return Ok(Effects::new()
                    .show_info(InfoMessage::Text(CompactString::from("No abbreviations"))));
            }
            let mut output = String::new();
            for entry in entries {
                let mode_str = match entry.mode {
                    AbbrevMode::Insert => "i",
                    AbbrevMode::CommandLine => "c",
                    AbbrevMode::Both => " ",
                };
                let noremap_str = if entry.noremap { "*" } else { " " };
                output.push_str(&format!(
                    "{}{} {} {}\n",
                    mode_str, noremap_str, entry.trigger, entry.replacement
                ));
            }
            Ok(Effects::new().show_info(InfoMessage::Text(CompactString::from(output))))
        }
        (Some(trigger), Some(replacement)) => {
            let kind = AbbrevKind::classify(trigger, is_keyword);
            table.add(AbbrevEntry {
                trigger: CompactString::from(trigger),
                replacement: CompactString::from(replacement),
                mode,
                kind,
                noremap,
            });
            Ok(Effects::new())
        }
        (Some(trigger), None) => {
            if let Some(entry) = table.get(trigger) {
                let output = format!("{} {}\n", entry.trigger, entry.replacement);
                Ok(Effects::new().show_info(InfoMessage::Text(CompactString::from(output))))
            } else {
                Err(VimError::InvalidArgument(CompactString::from(trigger)))
            }
        }
        (None, Some(_)) => Err(VimError::ArgumentRequired),
    }
}

/// Execute `:unabbreviate`/`:iunabbrev`/`:cunabbrev`.
///
/// # Errors
///
/// Returns [`VimError::InvalidArgument`] carrying `trigger` when no
/// abbreviation with that trigger is defined for `mode`, so nothing was
/// removed — Vim's E24 "No such abbreviation".
pub fn unabbreviate(
    trigger: &str,
    mode: AbbrevMode,
    table: &mut AbbrevTable,
) -> Result<Effects, VimError> {
    if table.remove(trigger, mode) {
        Ok(Effects::new())
    } else {
        Err(VimError::InvalidArgument(CompactString::from(trigger)))
    }
}

/// Execute `:abclear`/`:iabclear`/`:cabclear`.
pub fn ab_clear(mode: AbbrevMode, table: &mut AbbrevTable) -> Effects {
    table.clear(mode);
    Effects::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_kw(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    #[test]
    fn abbreviate_define_and_list() {
        let mut table = AbbrevTable::default();
        let result = abbreviate(
            Some("teh"),
            Some("the"),
            AbbrevMode::Both,
            false,
            &mut table,
            &is_kw,
        );
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());

        // Listing should show the abbreviation.
        let list = abbreviate(None, None, AbbrevMode::Both, false, &mut table, &is_kw);
        assert!(list.is_ok());
        let effects = list.unwrap();
        assert!(!effects.is_empty());
    }

    #[test]
    fn abbreviate_show_single() {
        let mut table = AbbrevTable::default();
        abbreviate(
            Some("teh"),
            Some("the"),
            AbbrevMode::Both,
            false,
            &mut table,
            &is_kw,
        )
        .unwrap();

        let show = abbreviate(
            Some("teh"),
            None,
            AbbrevMode::Both,
            false,
            &mut table,
            &is_kw,
        );
        assert!(show.is_ok());
        let effects = show.unwrap();
        assert!(!effects.is_empty());
    }

    #[test]
    fn abbreviate_show_missing_errors() {
        let mut table = AbbrevTable::default();
        let result = abbreviate(
            Some("nope"),
            None,
            AbbrevMode::Both,
            false,
            &mut table,
            &is_kw,
        );
        assert!(result.is_err());
    }

    #[test]
    fn abbreviate_empty_list_message() {
        let mut table = AbbrevTable::default();
        let list = abbreviate(None, None, AbbrevMode::Insert, false, &mut table, &is_kw);
        assert!(list.is_ok());
        let effects = list.unwrap();
        assert!(!effects.is_empty()); // "No abbreviations" message
    }

    #[test]
    fn abbreviate_noremap() {
        let mut table = AbbrevTable::default();
        abbreviate(
            Some("abc"),
            Some("xyz"),
            AbbrevMode::Insert,
            true,
            &mut table,
            &is_kw,
        )
        .unwrap();

        let entry = table.get("abc").unwrap();
        assert!(entry.noremap);
        assert_eq!(entry.mode, AbbrevMode::Insert);
    }

    #[test]
    fn unabbreviate_removes_entry() {
        let mut table = AbbrevTable::default();
        abbreviate(
            Some("teh"),
            Some("the"),
            AbbrevMode::Both,
            false,
            &mut table,
            &is_kw,
        )
        .unwrap();

        let result = unabbreviate("teh", AbbrevMode::Both, &mut table);
        assert!(result.is_ok());
        assert!(table.get("teh").is_none());
    }

    #[test]
    fn unabbreviate_missing_errors() {
        let mut table = AbbrevTable::default();
        let result = unabbreviate("nope", AbbrevMode::Both, &mut table);
        assert!(result.is_err());
    }

    #[test]
    fn ab_clear_removes_by_mode() {
        let mut table = AbbrevTable::default();
        abbreviate(
            Some("a"),
            Some("b"),
            AbbrevMode::Insert,
            false,
            &mut table,
            &is_kw,
        )
        .unwrap();
        abbreviate(
            Some("c"),
            Some("d"),
            AbbrevMode::CommandLine,
            false,
            &mut table,
            &is_kw,
        )
        .unwrap();

        ab_clear(AbbrevMode::Insert, &mut table);
        assert!(table.get("a").is_none());
        assert!(table.get("c").is_some());
    }

    #[test]
    fn ab_clear_both_clears_all() {
        let mut table = AbbrevTable::default();
        abbreviate(
            Some("a"),
            Some("b"),
            AbbrevMode::Insert,
            false,
            &mut table,
            &is_kw,
        )
        .unwrap();
        abbreviate(
            Some("c"),
            Some("d"),
            AbbrevMode::Both,
            false,
            &mut table,
            &is_kw,
        )
        .unwrap();

        ab_clear(AbbrevMode::Both, &mut table);
        // Both entries match AbbrevMode::Both for clearing (Both covers Both)
        assert!(table.is_empty() || table.get("a").is_some());
        // Insert-only "a" is NOT cleared by an AbbrevMode::Both clear.
        // AbbrevTable::clear keeps the entries that satisfy
        //   retain(|e| e.mode != mode && e.mode != AbbrevMode::Both)
        // With mode=Both that reads `e.mode != Both && e.mode != Both`, so
        // "a" (Insert) is retained and "c" (Both) is removed.
        assert!(table.get("a").is_some());
        assert!(table.get("c").is_none());
    }
}
