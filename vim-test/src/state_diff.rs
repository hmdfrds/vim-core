//! State comparison for fidelity testing.
//!
//! Compares actual vim-core state against expected Neovim golden state,
//! producing readable diffs. Contains tolerance logic for known legitimate
//! differences between the two engines.

use crate::golden::{GoldenState, RegisterSnapshot};

/// Compare actual state with expected golden state.
///
/// Returns `None` if states match (within tolerances), `Some(StateDiff)` if they differ.
pub fn compare_states(actual: &GoldenState, expected: &GoldenState) -> Option<StateDiff> {
    let mut diffs = Vec::new();

    // KEEP: Trailing newline normalization.
    // Neovim buffers always have an implicit trailing newline that is part of
    // its data model. vim-core does not model this implicit newline. This is a
    // fundamental architectural difference, not a bug.
    let actual_text = actual.text.strip_suffix('\n').unwrap_or(&actual.text);
    let expected_text = expected.text.strip_suffix('\n').unwrap_or(&expected.text);
    if actual_text != expected_text {
        diffs.push(FieldDiff {
            field: "text".to_string(),
            expected: format!("{:?}", expected.text),
            actual: format!("{:?}", actual.text),
        });
    }

    if actual.cursor_offset != expected.cursor_offset {
        diffs.push(FieldDiff {
            field: "cursor_offset".to_string(),
            expected: expected.cursor_offset.to_string(),
            actual: actual.cursor_offset.to_string(),
        });
    }

    if actual.cursor_line != expected.cursor_line {
        diffs.push(FieldDiff {
            field: "cursor_line".to_string(),
            expected: expected.cursor_line.to_string(),
            actual: actual.cursor_line.to_string(),
        });
    }

    if actual.cursor_col != expected.cursor_col {
        diffs.push(FieldDiff {
            field: "cursor_col".to_string(),
            expected: expected.cursor_col.to_string(),
            actual: actual.cursor_col.to_string(),
        });
    }

    if actual.mode != expected.mode {
        diffs.push(FieldDiff {
            field: "mode".to_string(),
            expected: expected.mode.clone(),
            actual: actual.mode.clone(),
        });
    }

    // KEEP: Clipboard register skip.
    // Clipboard registers (+, *) are external system state — they depend on the
    // OS clipboard, which vim-core cannot control in a test environment.
    //
    // INVESTIGATE: Command register (:) skip.
    // Neovim stores the last ex command in the ':' register. The vim-core engine
    // does not emit SetRegister for ':'. Requires engine-level fix.
    for (name, expected_reg) in &expected.registers {
        if name == "+" || name == "*" || name == ":" {
            continue;
        }

        // KEEP: K_SPECIAL normalization.
        // Neovim stores macro/dot-repeat registers with internal K_SPECIAL byte
        // sequences. When decoded as UTF-8 they become replacement characters.
        // vim-core stores keystrokes as plain text. This is a serialization
        // format difference, not a behavioral difference.
        let normalized_expected_text = normalize_neovim_register(&expected_reg.text);

        // INVESTIGATE: Linewise register trailing newline.
        // Neovim's implicit trailing newline causes linewise deletes at EOF to
        // include an extra \n in register content. vim-core doesn't model the
        // implicit newline. This may mask a bug — linewise paste may behave
        // differently without the trailing newline.
        let normalized_expected = match actual.registers.get(name) {
            Some(actual_reg)
                if expected_reg.regtype == "V"
                    && normalized_expected_text == format!("{}\n", actual_reg.text) =>
            {
                RegisterSnapshot {
                    text: actual_reg.text.clone(),
                    regtype: expected_reg.regtype.clone(),
                }
            }
            _ => RegisterSnapshot {
                text: normalized_expected_text.clone(),
                regtype: expected_reg.regtype.clone(),
            },
        };

        match actual.registers.get(name) {
            Some(actual_reg) if *actual_reg != normalized_expected => {
                diffs.push(FieldDiff {
                    field: format!("register '{name}'"),
                    expected: format!("{:?}", normalized_expected.text),
                    actual: format!("{:?}", actual_reg.text),
                });
            }
            None if !normalized_expected.text.is_empty() => {
                diffs.push(FieldDiff {
                    field: format!("register '{name}'"),
                    expected: format!("{:?}", normalized_expected.text),
                    actual: "(empty)".to_string(),
                });
            }
            _ => {}
        }
    }

    // KEEP: Dangling mark skip.
    // After operations that shorten lines, the oracle's (line,col) → byte offset
    // conversion can produce values past end of text. These are oracle artifacts,
    // not behavioral differences.
    let actual_text_len = actual_text.len();
    for (name, &expected_offset) in &expected.marks {
        if expected_offset >= actual_text_len {
            continue;
        }
        match actual.marks.get(name) {
            Some(&actual_offset) if actual_offset != expected_offset => {
                // KEEP: v:maxcol sentinel for > and ] marks.
                // Neovim stores these marks with column v:maxcol (2147483647) for
                // linewise visual. vim-core stores the real end-of-line offset.
                // Both represent the same semantic intent ("end of line").
                if (name == ">" || name == "]") && expected_offset > (1 << 30) {
                    continue;
                }
                // INVESTIGATE: Same-line tolerance for edit marks.
                // Likely needed for . and ^ (Neovim's per-batch insert tracking
                // vs our per-keystroke model), but may mask column-level bugs
                // for [ and ] marks which should precisely bracket the changed
                // region.
                if matches!(name.as_str(), "." | "^" | "[" | "]")
                    && same_line_or_close(expected_offset, actual_offset, actual_text)
                {
                    continue;
                }
                diffs.push(FieldDiff {
                    field: format!("mark '{name}'"),
                    expected: expected_offset.to_string(),
                    actual: actual_offset.to_string(),
                });
            }
            None => {
                diffs.push(FieldDiff {
                    field: format!("mark '{name}'"),
                    expected: expected_offset.to_string(),
                    actual: "(not set)".to_string(),
                });
            }
            _ => {}
        }
    }

    if expected.selection_anchor.is_some() && actual.selection_anchor != expected.selection_anchor {
        diffs.push(FieldDiff {
            field: "selection_anchor".to_string(),
            expected: format!("{:?}", expected.selection_anchor),
            actual: format!("{:?}", actual.selection_anchor),
        });
    }

    if let (Some(expected_win), Some(actual_win)) = (&expected.window, &actual.window) {
        if expected_win.topline != actual_win.topline {
            diffs.push(FieldDiff {
                field: "window.topline".to_string(),
                expected: expected_win.topline.to_string(),
                actual: actual_win.topline.to_string(),
            });
        }
    }

    // KEEP: curswant MAXCOL normalization.
    // Neovim uses i32::MAX (2147483647) for end-of-line stickiness, vim-core
    // uses usize::MAX. Both represent the same semantic intent. This is a
    // type-system difference, not a behavioral difference.
    if let (Some(expected_cw), Some(actual_cw)) = (expected.curswant, actual.curswant) {
        let nvim_maxcol = 2_147_483_647_usize;
        let is_both_eol = (expected_cw == nvim_maxcol && actual_cw == usize::MAX)
            || (actual_cw == nvim_maxcol && expected_cw == usize::MAX);
        if expected_cw != actual_cw && !is_both_eol {
            diffs.push(FieldDiff {
                field: "curswant".to_string(),
                expected: expected_cw.to_string(),
                actual: actual_cw.to_string(),
            });
        }
    }

    // INVESTIGATE: Jumplist subsequence tolerance.
    // This tolerance is very broad — it accepts subsequence matches in both
    // directions with same-line tolerance. The golden jumplist data may have
    // been manually created, making the comparison weak. Consider tightening
    // once the oracle captures jumplist data accurately.
    if let (Some(expected_jl), Some(actual_jl)) = (&expected.jumplist, &actual.jumplist) {
        let jl_matches = expected_jl == actual_jl
            || is_subsequence_tolerant(expected_jl, actual_jl, actual_text)
            || is_subsequence_tolerant(actual_jl, expected_jl, actual_text);
        if !jl_matches {
            diffs.push(FieldDiff {
                field: "jumplist".to_string(),
                expected: format!("{expected_jl:?}"),
                actual: format!("{actual_jl:?}"),
            });
        }
    }
    if let (Some(expected_idx), Some(actual_idx)) = (expected.jumplist_idx, actual.jumplist_idx) {
        let jl_passed = match (&expected.jumplist, &actual.jumplist) {
            (Some(ejl), Some(ajl)) => {
                ejl == ajl
                    || is_subsequence_tolerant(ejl, ajl, actual_text)
                    || is_subsequence_tolerant(ajl, ejl, actual_text)
            }
            _ => false,
        };
        let idx_matches = expected_idx == actual_idx || jl_passed;
        if !idx_matches {
            diffs.push(FieldDiff {
                field: "jumplist_idx".to_string(),
                expected: expected_idx.to_string(),
                actual: actual_idx.to_string(),
            });
        }
    }

    // Changelist comparison — multi-tier tolerance.
    // See changelists_match() for tier documentation.
    if let (Some(expected_cl), Some(actual_cl)) = (&expected.changelist, &actual.changelist) {
        let changelist_matches = changelists_match(expected_cl, actual_cl, actual_text);
        if !changelist_matches {
            diffs.push(FieldDiff {
                field: "changelist".to_string(),
                expected: format!("{expected_cl:?}"),
                actual: format!("{actual_cl:?}"),
            });
        }
    }
    if let (Some(expected_idx), Some(actual_idx)) = (expected.changelist_idx, actual.changelist_idx)
    {
        let cl_passed = match (&expected.changelist, &actual.changelist) {
            (Some(ecl), Some(acl)) => changelists_match(ecl, acl, actual_text),
            _ => false,
        };
        let idx_matches = expected_idx == actual_idx || cl_passed;
        if !idx_matches {
            diffs.push(FieldDiff {
                field: "changelist_idx".to_string(),
                expected: expected_idx.to_string(),
                actual: actual_idx.to_string(),
            });
        }
    }

    // INVESTIGATE: errmsg one-directional comparison.
    // Only reports differences when Neovim produced an error. If vim-core
    // produces an error that Neovim doesn't, it's silently ignored. This
    // may hide spurious vim-core errors.
    if expected.errmsg.is_some() && actual.errmsg != expected.errmsg {
        diffs.push(FieldDiff {
            field: "errmsg".to_string(),
            expected: format!("{:?}", expected.errmsg),
            actual: format!("{:?}", actual.errmsg),
        });
    }

    if diffs.is_empty() {
        None
    } else {
        Some(StateDiff { diffs })
    }
}

/// Difference between expected and actual state.
pub struct StateDiff {
    diffs: Vec<FieldDiff>,
}

/// Single field difference.
pub struct FieldDiff {
    /// Field name.
    pub field: String,
    /// Expected value (from Neovim golden file).
    pub expected: String,
    /// Actual value (from vim-core).
    pub actual: String,
}

impl std::fmt::Display for StateDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "┌────────────────────┬────────────────────┬────────────────────┐"
        )?;
        writeln!(
            f,
            "│ Field              │ Expected (nvim)    │ Actual (ours)      │"
        )?;
        writeln!(
            f,
            "├────────────────────┼────────────────────┼────────────────────┤"
        )?;

        for diff in &self.diffs {
            let field = truncate(&diff.field, 18);
            let expected = truncate(&diff.expected, 18);
            let actual = truncate(&diff.actual, 18);
            writeln!(f, "│ {field:<18} │ {expected:<18} │ {actual:<18} │ ✗")?;
        }

        writeln!(
            f,
            "└────────────────────┴────────────────────┴────────────────────┘"
        )?;
        Ok(())
    }
}

/// Multi-tier changelist comparison.
///
/// Returns `true` if the changelists are "close enough" given known systematic
/// differences between Neovim's and vim-core's changelist tracking.
///
/// Tiers:
/// - (a) Exact match
/// - (b) INVESTIGATE: Expected has one extra leading entry (old golden artifact)
/// - (c) Same length, same-line tolerance
/// - (d) Actual is subsequence of expected (missing intermediate entries)
/// - (e) Expected is subsequence of actual (extra macro entries)
/// - (f) INVESTIGATE: Leading zero strip (Neovim records pre-edit position)
fn changelists_match(expected: &[usize], actual: &[usize], text: &str) -> bool {
    // (a) Exact match.
    if expected == actual {
        return true;
    }

    // (b) INVESTIGATE: Old golden artifact — expected has one extra leading entry.
    // If this is a golden file generation bug, the golden files should be
    // regenerated rather than permanently tolerated.
    if expected.len() == actual.len() + 1
        && expected[1..]
            .iter()
            .zip(actual.iter())
            .all(|(e, a)| same_line_or_close(*e, *a, text))
    {
        return true;
    }

    // (c) Same length, same-line tolerance.
    if expected.len() == actual.len()
        && expected
            .iter()
            .zip(actual.iter())
            .all(|(e, a)| same_line_or_close(*e, *a, text))
    {
        return true;
    }

    // (d) Subsequence: actual entries are a subsequence of expected.
    if actual.len() < expected.len() && is_subsequence_tolerant(actual, expected, text) {
        return true;
    }

    // (e) Supersequence: expected entries are a subsequence of actual.
    if expected.len() < actual.len() && is_subsequence_tolerant(expected, actual, text) {
        return true;
    }

    // (f) INVESTIGATE: Leading zero strip.
    // Neovim often records the pre-edit cursor position (offset 0) as the first
    // changelist entry. vim-core doesn't always do this. This causes a user-visible
    // behavioral difference in g; navigation.
    if expected.first() == Some(&0) && actual.first() != Some(&0) && expected.len() >= 2 {
        let stripped = &expected[1..];
        if stripped.len() == actual.len()
            && stripped
                .iter()
                .zip(actual.iter())
                .all(|(e, a)| same_line_or_close(*e, *a, text))
        {
            return true;
        }
        if actual.len() < stripped.len() && is_subsequence_tolerant(actual, stripped, text) {
            return true;
        }
        if stripped.len() < actual.len() && is_subsequence_tolerant(stripped, actual, text) {
            return true;
        }
    }

    // Tier (g) REMOVED: Single entry, both valid — was too broad. Tolerating
    // completely different changelist positions masks real bugs.

    false
}

/// Check if two byte offsets are on the same or adjacent line, or within
/// a small absolute tolerance.
///
/// INVESTIGATE: The adjacent-line tolerance (`line_diff <= 1`) is broad.
/// Two offsets on adjacent lines could be many bytes apart. Consider
/// splitting into stricter and looser variants per use case.
fn same_line_or_close(a: usize, b: usize, text: &str) -> bool {
    if a == b {
        return true;
    }
    let abs_diff = (a as isize - b as isize).unsigned_abs();
    if abs_diff <= 3 {
        return true;
    }
    if text.is_empty() {
        return abs_diff <= 10;
    }
    let line_a = line_of_offset(text, a);
    let line_b = line_of_offset(text, b);
    let line_diff = (line_a as isize - line_b as isize).unsigned_abs();
    line_diff <= 1
}

fn is_subsequence_tolerant(needle: &[usize], haystack: &[usize], text: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let mut ni = 0;
    for &h in haystack {
        if ni < needle.len() && same_line_or_close(needle[ni], h, text) {
            ni += 1;
        }
    }
    ni == needle.len()
}

fn line_of_offset(text: &str, offset: usize) -> usize {
    let clamped = offset.min(text.len());
    text.as_bytes()[..clamped]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let end_idx = s
            .char_indices()
            .take_while(|(idx, _)| *idx < max_len)
            .last()
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        let slice = &s[..s.ceil_char_boundary(end_idx.saturating_add(1).min(s.len()))];
        format!("{}\u{2026}", slice.trim_end())
    }
}

fn normalize_neovim_register(text: &str) -> String {
    text.replace("\u{FFFD}\u{FFFD}5", "")
}
