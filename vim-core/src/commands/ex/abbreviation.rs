//! Abbreviation ex commands: `:abbreviate`, `:unabbreviate`, `:abclear`.

use compact_str::CompactString;

use crate::effects::{Effects, InfoMessage};
use crate::errors::VimError;
use crate::primitives::abbreviation::{AbbrevEntry, AbbrevKind, AbbrevMode, AbbrevTable};
use crate::primitives::TrustTag;

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
    let defined = Defined {
        mode,
        noremap,
        trust: TrustTag::default(),
    };
    abbreviate_tagged(trigger, replacement, defined, table, is_keyword)
}

/// What an abbreviation is defined with besides its text.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Defined {
    pub(crate) mode: AbbrevMode,
    pub(crate) noremap: bool,
    /// The tag of the config that defines it.
    pub(crate) trust: TrustTag,
}

/// [`abbreviate`], keeping `defined.trust` with an abbreviation it defines.
pub(crate) fn abbreviate_tagged(
    trigger: Option<&str>,
    replacement: Option<&str>,
    defined: Defined,
    table: &mut AbbrevTable,
    is_keyword: &dyn Fn(char) -> bool,
) -> Result<Effects, VimError> {
    let Defined {
        mode,
        noremap,
        trust,
    } = defined;
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
                trust,
            });
            Ok(Effects::new())
        }
        (Some(trigger), None) => {
            let mut output = String::new();
            for entry in table.list(mode) {
                if entry.trigger == trigger {
                    output.push_str(&entry.trigger);
                    output.push(' ');
                    output.push_str(&entry.replacement);
                    output.push('\n');
                }
            }
            if output.is_empty() {
                return Err(VimError::InvalidArgument(CompactString::from(trigger)));
            }
            Ok(Effects::new().show_info(InfoMessage::Text(CompactString::from(output))))
        }
        (None, Some(_)) => Err(VimError::ArgumentRequired),
    }
}

/// Execute `:unabbreviate`/`:iunabbrev`/`:cunabbrev`.
///
/// # Errors
///
/// Returns [`VimError::NoSuchAbbreviation`] (E24) when no abbreviation of
/// `mode` has `trigger` as its trigger or its replacement, so nothing was
/// removed.
pub fn unabbreviate(
    trigger: &str,
    mode: AbbrevMode,
    table: &mut AbbrevTable,
) -> Result<Effects, VimError> {
    // do_map(): when no trigger matches, a second round removes the
    // abbreviations that expand to the argument, so ":unab foo" still
    // works when typing it expanded "foo".
    if table.remove(trigger, mode) || table.remove_expanding_to(trigger, mode) {
        Ok(Effects::new())
    } else {
        Err(VimError::NoSuchAbbreviation)
    }
}

/// Execute `:abclear`/`:iabclear`/`:cabclear`: each abbreviation loses
/// the modes of `mode`, as in Vim's `map_clear_mode()`, so `:abclear`
/// clears them all.
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
        // map_clear_mode() takes the Insert and Command-line bits off each.
        let mut table = AbbrevTable::default();
        define(&mut table, "a", "b", AbbrevMode::Insert);
        define(&mut table, "c", "d", AbbrevMode::CommandLine);
        define(&mut table, "e", "f", AbbrevMode::Both);

        ab_clear(AbbrevMode::Both, &mut table);
        assert!(table.is_empty());
    }

    #[test]
    fn ab_clear_one_mode_keeps_the_other_part() {
        let mut table = AbbrevTable::default();
        define(&mut table, "e", "f", AbbrevMode::Both);

        ab_clear(AbbrevMode::Insert, &mut table);
        assert!(!table.defines("e", AbbrevMode::Insert));
        assert!(table.defines("e", AbbrevMode::CommandLine));
    }

    #[test]
    fn unabbreviate_one_mode_keeps_the_other_part() {
        // do_map() resets the mode bits the command names.
        let mut table = AbbrevTable::default();
        define(&mut table, "e", "f", AbbrevMode::Both);

        unabbreviate("e", AbbrevMode::Insert, &mut table).unwrap();
        assert!(!table.defines("e", AbbrevMode::Insert));
        assert_eq!(
            table
                .find("e", AbbrevMode::CommandLine)
                .unwrap()
                .replacement,
            "f"
        );
        assert!(unabbreviate("e", AbbrevMode::Insert, &mut table).is_err());
    }

    #[test]
    fn listing_every_abbreviation_shows_each_mode() {
        let mut table = AbbrevTable::default();
        define(&mut table, "a", "b", AbbrevMode::Insert);
        define(&mut table, "a", "c", AbbrevMode::CommandLine);

        let listed = abbreviate(None, None, AbbrevMode::Both, false, &mut table, &is_kw).unwrap();
        let shown =
            abbreviate(Some("a"), None, AbbrevMode::Both, false, &mut table, &is_kw).unwrap();
        for effects in [listed, shown] {
            let text = effects
                .as_slice()
                .iter()
                .find_map(|effect| match effect {
                    crate::effects::Effect::ShowInfo {
                        info: InfoMessage::Text(text),
                    } => Some(text.clone()),
                    _ => None,
                })
                .unwrap();
            assert!(text.contains("a b") && text.contains("a c"), "{text:?}");
        }
    }

    fn define(table: &mut AbbrevTable, trigger: &str, replacement: &str, mode: AbbrevMode) {
        abbreviate(Some(trigger), Some(replacement), mode, false, table, &is_kw).unwrap();
    }
}
