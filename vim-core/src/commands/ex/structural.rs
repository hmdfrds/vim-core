//! Structural regular expressions (`:sx`, `:sy`).
//!
//! Implements Plan 9-inspired structural regex commands:
//! - `:sx/pattern/command` — extract: apply command to each match
//! - `:sy/pattern/command` — complement: apply command to gaps between matches
//!
//! Currently only `:sx/pat/d` and `:sy/pat/d` (delete) are supported as
//! sub-commands. Other sub-commands are future work.

use crate::effects::Effects;
use crate::errors::VimError;
use crate::grammar::types::ExCommand;
use crate::primitives::{Range, StructuralFlags};
use crate::regex::{MatchContext, VimRegex};
use compact_str::CompactString;

/// Execute structural extract: apply sub-command to each regex match.
///
/// Finds all matches of `pattern` in `text` and applies `sub_command` to each.
/// Matches are processed in reverse order so that byte offsets remain stable.
///
/// # Errors
///
/// Returns `VimError::PatternNotFound` if the pattern is invalid or has no matches.
/// Returns `VimError::NotEditorCommand` if the sub-command is unsupported.
pub fn execute_structural_extract(
    text: &str,
    pattern: &str,
    sub_command: &ExCommand,
    _flags: &StructuralFlags,
) -> Result<Effects, VimError> {
    let re =
        VimRegex::new(pattern).map_err(|e| VimError::PatternNotFound(format!("{e}").into()))?;

    let ctx = MatchContext::simple(text);
    let matches = re.find_all(&ctx).unwrap_or_default();

    if matches.is_empty() {
        return Err(VimError::PatternNotFound(CompactString::from(pattern)));
    }

    match sub_command {
        ExCommand::Delete { .. } => {
            let mut effects = Effects::new();
            // Process in reverse order so offsets stay stable
            for m in matches.iter().rev() {
                effects = effects.delete(Range::from_raw(m.range.start, m.range.end));
            }
            effects = effects.show_message(CompactString::from(format!(
                "{} regions deleted",
                matches.len()
            )));
            Ok(effects)
        }
        // TODO: Support Substitute sub-command (`:sx/pat/s/old/new/`)
        // TODO: Support other sub-commands
        _ => Err(VimError::NotEditorCommand(CompactString::from(
            "structural regex: only :d sub-command is currently supported",
        ))),
    }
}

/// Execute structural complement: apply sub-command to gaps between matches.
///
/// Finds all matches of `pattern` in `text`, computes the gap regions
/// (text not matched), and applies `sub_command` to each gap.
/// Gaps are processed in reverse order so that byte offsets remain stable.
///
/// # Errors
///
/// Returns `VimError::PatternNotFound` if the pattern is invalid or has no matches.
/// Returns `VimError::NotEditorCommand` if the sub-command is unsupported.
pub fn execute_structural_complement(
    text: &str,
    pattern: &str,
    sub_command: &ExCommand,
    _flags: &StructuralFlags,
) -> Result<Effects, VimError> {
    let re =
        VimRegex::new(pattern).map_err(|e| VimError::PatternNotFound(format!("{e}").into()))?;

    let ctx = MatchContext::simple(text);
    let matches = re.find_all(&ctx).unwrap_or_default();

    if matches.is_empty() {
        return Err(VimError::PatternNotFound(CompactString::from(pattern)));
    }

    // Compute gap regions: [0..first_match_start, first_match_end..second_match_start, ...]
    let mut gaps: Vec<(usize, usize)> = Vec::new();
    let mut prev_end = 0;
    for m in &matches {
        if m.range.start > prev_end {
            gaps.push((prev_end, m.range.start));
        }
        prev_end = m.range.end;
    }
    if prev_end < text.len() {
        gaps.push((prev_end, text.len()));
    }

    if gaps.is_empty() {
        return Ok(Effects::new().show_message(CompactString::from("No gap regions found")));
    }

    match sub_command {
        ExCommand::Delete { .. } => {
            let mut effects = Effects::new();
            // Process in reverse order so offsets stay stable
            for &(start, end) in gaps.iter().rev() {
                effects = effects.delete(Range::from_raw(start, end));
            }
            effects = effects.show_message(CompactString::from(format!(
                "{} gap regions deleted",
                gaps.len()
            )));
            Ok(effects)
        }
        // TODO: Support Substitute sub-command
        // TODO: Support other sub-commands
        _ => Err(VimError::NotEditorCommand(CompactString::from(
            "structural regex: only :d sub-command is currently supported",
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
    use crate::grammar::types::ExRange;

    fn delete_cmd() -> ExCommand {
        ExCommand::Delete {
            range: ExRange::current_line(),
            register: None,
        }
    }

    #[test]
    fn extract_delete_single_match() {
        let text = "hello world";
        let result =
            execute_structural_extract(text, "world", &delete_cmd(), &StructuralFlags::default())
                .unwrap();
        let deletes: Vec<_> = result
            .iter()
            .filter(|e| matches!(e, Effect::Delete { .. }))
            .collect();
        assert_eq!(deletes.len(), 1);
        if let Effect::Delete { range } = deletes[0] {
            assert_eq!(range.start().get(), 6);
            assert_eq!(range.end().get(), 11);
        }
    }

    #[test]
    fn extract_delete_multiple_matches() {
        let text = "abcabc";
        let result =
            execute_structural_extract(text, "abc", &delete_cmd(), &StructuralFlags::default())
                .unwrap();
        let deletes: Vec<_> = result
            .iter()
            .filter(|e| matches!(e, Effect::Delete { .. }))
            .collect();
        assert_eq!(deletes.len(), 2);
        // Reverse order: second match first
        if let Effect::Delete { range } = deletes[0] {
            assert_eq!(range.start().get(), 3);
            assert_eq!(range.end().get(), 6);
        }
        if let Effect::Delete { range } = deletes[1] {
            assert_eq!(range.start().get(), 0);
            assert_eq!(range.end().get(), 3);
        }
    }

    #[test]
    fn extract_no_match_returns_error() {
        let text = "hello";
        let result =
            execute_structural_extract(text, "xyz", &delete_cmd(), &StructuralFlags::default());
        assert!(result.is_err());
    }

    #[test]
    fn extract_invalid_regex_returns_error() {
        let text = "hello";
        let result =
            execute_structural_extract(text, "[", &delete_cmd(), &StructuralFlags::default());
        assert!(result.is_err());
    }

    #[test]
    fn complement_delete_gaps() {
        // Text: "xxxFOOyyyFOOzzz"
        // Matches for "FOO": [3..6], [9..12]
        // Gaps: [0..3], [6..9], [12..15]
        let text = "xxxFOOyyyFOOzzz";
        let result =
            execute_structural_complement(text, "FOO", &delete_cmd(), &StructuralFlags::default())
                .unwrap();
        let deletes: Vec<_> = result
            .iter()
            .filter(|e| matches!(e, Effect::Delete { .. }))
            .collect();
        assert_eq!(deletes.len(), 3);
        // Reverse order
        if let Effect::Delete { range } = deletes[0] {
            assert_eq!(range.start().get(), 12);
            assert_eq!(range.end().get(), 15);
        }
        if let Effect::Delete { range } = deletes[1] {
            assert_eq!(range.start().get(), 6);
            assert_eq!(range.end().get(), 9);
        }
        if let Effect::Delete { range } = deletes[2] {
            assert_eq!(range.start().get(), 0);
            assert_eq!(range.end().get(), 3);
        }
    }

    #[test]
    fn complement_no_match_returns_error() {
        let text = "hello";
        let result =
            execute_structural_complement(text, "xyz", &delete_cmd(), &StructuralFlags::default());
        assert!(result.is_err());
    }

    #[test]
    fn extract_unsupported_subcommand() {
        let text = "hello world";
        let cmd = ExCommand::Join {
            range: ExRange::current_line(),
            bang: false,
        };
        let result = execute_structural_extract(text, "hello", &cmd, &StructuralFlags::default());
        assert!(result.is_err());
    }

    #[test]
    fn complement_contiguous_matches_no_gaps_between() {
        // Matches cover entire text → no gaps
        let text = "aaa";
        let result =
            execute_structural_complement(text, "a", &delete_cmd(), &StructuralFlags::default())
                .unwrap();
        let deletes: Vec<_> = result
            .iter()
            .filter(|e| matches!(e, Effect::Delete { .. }))
            .collect();
        // "a" matches at [0..1], [1..2], [2..3] → gaps: none
        assert_eq!(deletes.len(), 0);
    }
}
