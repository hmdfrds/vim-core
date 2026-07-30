//! Search pattern modifier parsing (`\c`/`\C`/`\v`/`\V`/`\m`/`\M`).
//!
//! Extracts per-pattern modifiers and computes effective [`SearchFlags`]
//! by merging modifiers with [`VimOptions`] defaults.

use crate::primitives::{MagicMode, SearchFlags, VimOptions};

/// Per-pattern modifiers extracted from `\c`/`\C`/`\v`/`\V` sequences.
#[derive(Debug, Default)]
pub(super) struct PatternModifiers {
    /// Case override: `Some(false)` = `\c` (insensitive), `Some(true)` = `\C` (sensitive).
    pub case_override: Option<bool>,
    /// Magic override from `\v`/`\V`/`\m`/`\M`.
    pub magic_override: Option<MagicMode>,
}

/// Extract `\c`/`\C`/`\v`/`\V`/`\m`/`\M` modifiers from a pattern.
///
/// Vim allows `\c`/`\C` **anywhere** in the pattern — last one wins.
/// `\v`/`\V`/`\m`/`\M` are stripped from the prefix only (they affect
/// the regex syntax mode for everything that follows).
///
/// All `\c`/`\C` occurrences are stripped from the returned pattern so
/// that both the regex engine AND the substring/`SearchProvider` fallback
/// paths receive a clean pattern with a resolved `case_override`.
///
/// Returns `(stripped_pattern, modifiers)`. The stripped pattern is
/// heap-allocated only when `\c`/`\C` appears mid-pattern; prefix-only
/// modifiers return a borrowed slice (zero allocation).
pub(super) fn parse_search_modifiers(
    pattern: &str,
) -> (compact_str::CompactString, PatternModifiers) {
    let mut mods = PatternModifiers::default();

    // Fast path: no backslash → no modifiers.
    if !pattern.contains('\\') {
        return (compact_str::CompactString::from(pattern), mods);
    }

    // Phase 1: strip leading \v/\V/\m/\M/\c/\C prefix modifiers.
    let after_prefix = strip_modifier_prefix(pattern, &mut mods);

    // Phase 2: scan the rest of the pattern for \c/\C at any position.
    // If none found, return the prefix-stripped slice directly.
    if !after_prefix.contains('\\')
        || (!after_prefix.contains("\\c") && !after_prefix.contains("\\C"))
    {
        return (compact_str::CompactString::from(after_prefix), mods);
    }

    // There are mid/suffix \c or \C occurrences — build a stripped copy.
    let mut result = compact_str::CompactString::with_capacity(after_prefix.len());
    let mut chars = after_prefix.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('c') => mods.case_override = Some(false),
                Some('C') => mods.case_override = Some(true),
                Some(escaped) => {
                    result.push('\\');
                    result.push(escaped);
                }
                None => result.push('\\'), // trailing backslash
            }
        } else {
            result.push(ch);
        }
    }

    (result, mods)
}

/// Compute [`SearchFlags`] from `VimOptions` + per-pattern modifiers.
///
/// Priority: modifier override > smartcase > ignorecase > default.
pub(super) fn compute_search_flags(
    options: &VimOptions,
    stripped_pattern: &str,
    modifiers: &PatternModifiers,
) -> SearchFlags {
    let case_sensitive = match modifiers.case_override {
        Some(cs) => cs,
        None => options.effective_case_sensitive(stripped_pattern),
    };

    let magic = modifiers.magic_override.unwrap_or(MagicMode::Magic);

    SearchFlags::new()
        .with_case_sensitive(case_sensitive)
        .with_wrap(options.wrapscan())
        .with_magic(magic)
}

/// Strip leading modifier prefixes from the pattern. Multiple are allowed.
fn strip_modifier_prefix<'a>(pattern: &'a str, mods: &mut PatternModifiers) -> &'a str {
    let mut rest = pattern;

    loop {
        if let Some(after) = rest.strip_prefix("\\c") {
            mods.case_override = Some(false); // case-insensitive
            rest = after;
        } else if let Some(after) = rest.strip_prefix("\\C") {
            mods.case_override = Some(true); // case-sensitive
            rest = after;
        } else if let Some(after) = rest.strip_prefix("\\v") {
            mods.magic_override = Some(MagicMode::VeryMagic);
            rest = after;
        } else if let Some(after) = rest.strip_prefix("\\V") {
            mods.magic_override = Some(MagicMode::VeryNoMagic);
            rest = after;
        } else if let Some(after) = rest.strip_prefix("\\m") {
            mods.magic_override = Some(MagicMode::Magic);
            rest = after;
        } else if let Some(after) = rest.strip_prefix("\\M") {
            mods.magic_override = Some(MagicMode::NoMagic);
            rest = after;
        } else {
            break;
        }
    }

    rest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_modifiers() {
        let (stripped, mods) = parse_search_modifiers("hello");
        assert_eq!(stripped, "hello");
        assert!(mods.case_override.is_none());
        assert!(mods.magic_override.is_none());
    }

    #[test]
    fn case_insensitive_modifier() {
        let (stripped, mods) = parse_search_modifiers("\\chello");
        assert_eq!(stripped, "hello");
        assert_eq!(mods.case_override, Some(false));
    }

    #[test]
    fn case_sensitive_modifier() {
        let (stripped, mods) = parse_search_modifiers("\\Chello");
        assert_eq!(stripped, "hello");
        assert_eq!(mods.case_override, Some(true));
    }

    #[test]
    fn very_nomagic_modifier() {
        let (stripped, mods) = parse_search_modifiers("\\Vfoo.bar");
        assert_eq!(stripped, "foo.bar");
        assert_eq!(mods.magic_override, Some(MagicMode::VeryNoMagic));
    }

    #[test]
    fn very_magic_modifier() {
        let (stripped, mods) = parse_search_modifiers("\\vfoo");
        assert_eq!(stripped, "foo");
        assert_eq!(mods.magic_override, Some(MagicMode::VeryMagic));
    }

    #[test]
    fn combined_modifiers() {
        let (stripped, mods) = parse_search_modifiers("\\c\\Vfoo");
        assert_eq!(stripped, "foo");
        assert_eq!(mods.case_override, Some(false));
        assert_eq!(mods.magic_override, Some(MagicMode::VeryNoMagic));
    }

    #[test]
    fn compute_flags_default_options() {
        let opts = VimOptions::default();
        let mods = PatternModifiers::default();
        let flags = compute_search_flags(&opts, "hello", &mods);
        assert!(flags.case_sensitive()); // default: ignorecase off
        assert!(flags.wrap()); // default: wrapscan on
    }

    #[test]
    fn compute_flags_ignorecase() {
        let mut opts = VimOptions::default();
        opts.set_ignorecase(true);
        let mods = PatternModifiers::default();
        let flags = compute_search_flags(&opts, "hello", &mods);
        assert!(!flags.case_sensitive());
    }

    #[test]
    fn compute_flags_smartcase_lowercase() {
        let mut opts = VimOptions::default();
        opts.set_ignorecase(true);
        opts.set_smartcase(true);
        let mods = PatternModifiers::default();
        let flags = compute_search_flags(&opts, "hello", &mods);
        assert!(!flags.case_sensitive()); // all lowercase → insensitive
    }

    #[test]
    fn compute_flags_smartcase_uppercase() {
        let mut opts = VimOptions::default();
        opts.set_ignorecase(true);
        opts.set_smartcase(true);
        let mods = PatternModifiers::default();
        let flags = compute_search_flags(&opts, "Hello", &mods);
        assert!(flags.case_sensitive()); // uppercase → sensitive
    }

    #[test]
    fn modifier_overrides_smartcase() {
        let mut opts = VimOptions::default();
        opts.set_ignorecase(true);
        opts.set_smartcase(true);
        let mods = PatternModifiers {
            case_override: Some(false), // \c forces insensitive
            magic_override: None,
        };
        // Even though pattern has uppercase, \c wins.
        let flags = compute_search_flags(&opts, "Hello", &mods);
        assert!(!flags.case_sensitive());
    }

    #[test]
    fn compute_flags_nowrapscan() {
        let mut opts = VimOptions::default();
        opts.set_wrapscan(false);
        let mods = PatternModifiers::default();
        let flags = compute_search_flags(&opts, "hello", &mods);
        assert!(!flags.wrap());
    }

    // --- Mid-pattern \c/\C tests ---

    #[test]
    fn mid_pattern_case_insensitive() {
        let (stripped, mods) = parse_search_modifiers("foo\\cbar");
        assert_eq!(stripped, "foobar");
        assert_eq!(mods.case_override, Some(false));
    }

    #[test]
    fn mid_pattern_case_sensitive() {
        let (stripped, mods) = parse_search_modifiers("foo\\Cbar");
        assert_eq!(stripped, "foobar");
        assert_eq!(mods.case_override, Some(true));
    }

    #[test]
    fn suffix_case_modifier() {
        let (stripped, mods) = parse_search_modifiers("hello\\c");
        assert_eq!(stripped, "hello");
        assert_eq!(mods.case_override, Some(false));
    }

    #[test]
    fn last_one_wins_c_then_big_c() {
        // \c (insensitive) then \C (sensitive) — last one wins → sensitive
        let (stripped, mods) = parse_search_modifiers("\\cfoo\\Cbar");
        assert_eq!(stripped, "foobar");
        assert_eq!(mods.case_override, Some(true));
    }

    #[test]
    fn last_one_wins_big_c_then_c() {
        // \C (sensitive) then \c (insensitive) — last one wins → insensitive
        let (stripped, mods) = parse_search_modifiers("\\Cfoo\\cbar");
        assert_eq!(stripped, "foobar");
        assert_eq!(mods.case_override, Some(false));
    }

    #[test]
    fn multiple_mid_pattern_modifiers() {
        // Three \c/\C in pattern — last wins
        let (stripped, mods) = parse_search_modifiers("a\\cb\\Cc\\cd");
        assert_eq!(stripped, "abcd");
        assert_eq!(mods.case_override, Some(false)); // last \c wins
    }

    #[test]
    fn mid_pattern_preserves_other_backslash_escapes() {
        // \n, \t etc. should be preserved; only \c/\C stripped
        let (stripped, mods) = parse_search_modifiers("foo\\nbar\\c");
        assert_eq!(stripped, "foo\\nbar");
        assert_eq!(mods.case_override, Some(false));
    }

    #[test]
    fn magic_prefix_with_mid_case() {
        // \v prefix (very magic) + mid-pattern \c
        let (stripped, mods) = parse_search_modifiers("\\vfoo\\cbar");
        assert_eq!(stripped, "foobar");
        assert_eq!(mods.magic_override, Some(MagicMode::VeryMagic));
        assert_eq!(mods.case_override, Some(false));
    }

    #[test]
    fn no_false_positive_on_literal_backslash() {
        // Pattern with backslash followed by something other than c/C
        let (stripped, mods) = parse_search_modifiers("foo\\dbar");
        assert_eq!(stripped, "foo\\dbar");
        assert!(mods.case_override.is_none());
    }

    #[test]
    fn empty_pattern() {
        let (stripped, mods) = parse_search_modifiers("");
        assert_eq!(stripped, "");
        assert!(mods.case_override.is_none());
        assert!(mods.magic_override.is_none());
    }

    #[test]
    fn only_case_modifier() {
        let (stripped, mods) = parse_search_modifiers("\\c");
        assert_eq!(stripped, "");
        assert_eq!(mods.case_override, Some(false));
    }
}
