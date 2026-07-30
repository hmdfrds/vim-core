//! Ex command dispatcher for core operations.

use crate::commands::ex::{self, types::ExContext};
use crate::effects::Effects;
use crate::errors::VimError;
use crate::grammar::types::{ExCommand, ExRange, LineSpec};
use crate::primitives::SubFlags;

/// Dispatch core ex commands that produce side effects without requiring host interaction.
/// Host commands are ignored by returning `None`.
#[must_use]
pub fn dispatch_ex_core(
    command: &ExCommand,
    ctx: &ExContext<'_>,
) -> Option<Result<Effects, VimError>> {
    match command {
        ExCommand::Substitute {
            range,
            pattern,
            replacement,
            flags,
        } => {
            // `&` flag: start from previous substitute flags, then layer in the
            // explicitly-given flags so that any flags in the current `:s` command
            // override the inherited ones.
            let effective_flags = if flags.reuse_flags() {
                merge_flags(ctx.last_substitute_flags, *flags)
            } else if ctx.edcompatible {
                apply_edcompatible_flags(ctx.last_substitute_flags, *flags)
            } else {
                *flags
            };
            Some(ex::substitute::substitute(
                range,
                pattern,
                replacement,
                effective_flags,
                ctx,
            ))
        }
        ExCommand::Global {
            range,
            pattern,
            command: inner,
            invert,
        } => Some(ex::global::global(range, pattern, inner, *invert, ctx)),
        ExCommand::Delete { range, register } => Some(ex::delete(range, *register, ctx)),
        ExCommand::Yank { range, register } => Some(ex::yank(range, *register, ctx)),
        ExCommand::Move { range, target } => Some(match resolve_target_line(target, ctx) {
            Ok(t) => ex::move_lines(range, t, ctx),
            Err(e) => Err(e),
        }),
        ExCommand::Copy { range, target } => Some(match resolve_target_line(target, ctx) {
            Ok(t) => ex::copy_lines(range, t, ctx),
            Err(e) => Err(e),
        }),
        ExCommand::Join { range, bang } => Some(ex::join(range, *bang, ctx)),
        ExCommand::Sort { range, options } => Some(ex::sort(range, options, ctx)),
        ExCommand::Norm { range, keys, remap } => Some(match ex::resolve_range(range, ctx) {
            Ok(resolved) => Ok(ex::effects::norm_command(
                resolved.start(),
                resolved.end(),
                keys.clone(),
                *remap,
            )),
            Err(e) => Err(e),
        }),
        ExCommand::StructuralExtract {
            pattern,
            command,
            flags,
        } => Some(ex::structural::execute_structural_extract(
            ctx.text, pattern, command, flags,
        )),
        ExCommand::StructuralComplement {
            pattern,
            command,
            flags,
        } => Some(ex::structural::execute_structural_complement(
            ctx.text, pattern, command, flags,
        )),
        ExCommand::RepeatSubstitute { range, .. } => {
            // Resolve the pattern from the substitute pattern store (never RE_LAST).
            let Some(pattern) = ctx.last_substitute_pattern else {
                return Some(Err(VimError::NoPreviousSubstitute));
            };
            let replacement = ctx.last_substitute.unwrap_or("");
            // Both `&` and `g&` repeat with the previous flags (Neovim behavior).
            let flags = ctx.last_substitute_flags.unwrap_or_default();
            let effective_range = range.clone().unwrap_or_else(ExRange::current_line);
            Some(ex::substitute::substitute(
                &effective_range,
                pattern,
                replacement,
                flags,
                ctx,
            ))
        }
        ExCommand::SubTilde { range, flags } => {
            // :~ uses the last search pattern (@/) NOT the last substitute pattern.
            let Some(pattern) = ctx.last_search_pattern else {
                return Some(Err(VimError::NoPreviousPattern));
            };
            let Some(replacement) = ctx.last_substitute else {
                return Some(Err(VimError::NoPreviousSubstitute));
            };
            Some(ex::substitute::substitute(
                range,
                pattern,
                replacement,
                *flags,
                ctx,
            ))
        }
        ExCommand::NoHighlight => Some(ex::nohighlight()),
        ExCommand::GotoLine { range } => Some(ex::goto_line(range, ctx)),
        ExCommand::PrintLines {
            range,
            number,
            list,
        } => Some(ex::print_lines(range, *number, *list, ctx)),
        ExCommand::ZWindow {
            range,
            style,
            count,
        } => Some(ex::z_window(range, *style, *count, ctx)),
        _ => None,
    }
}

/// Merge previous substitute flags with explicitly-given flags.
///
/// Used by the `&` flag: start from the stored previous flags, then apply the
/// current flags on top.  For the case sensitivity enum, the explicit flag
/// wins when it is not `Default`; otherwise the base value is preserved.
/// Re-parsing a synthesised string ensures the `i`/`I` mutual-exclusion
/// logic in `SubFlags::parse` applies correctly (last-one-wins).
fn merge_flags(prev: Option<SubFlags>, explicit: SubFlags) -> SubFlags {
    use crate::primitives::CaseSensitivity;

    let base = prev.unwrap_or_default();
    // Build a merged flags value: start from base, then append explicit overrides.
    let mut parts = String::new();
    if base.global() {
        parts.push('g');
    }
    if base.confirm() {
        parts.push('c');
    }
    match base.case() {
        CaseSensitivity::Default => {}
        CaseSensitivity::IgnoreCase => parts.push('i'),
        CaseSensitivity::CaseSensitive => parts.push('I'),
    }
    if base.count_only() {
        parts.push('n');
    }
    if base.use_last_search() {
        parts.push('r');
    }
    // Append explicit flags after so they override base (last-one-wins for i/I).
    if explicit.global() {
        parts.push('g');
    }
    if explicit.confirm() {
        parts.push('c');
    }
    match explicit.case() {
        CaseSensitivity::Default => {}
        CaseSensitivity::IgnoreCase => parts.push('i'),
        CaseSensitivity::CaseSensitive => parts.push('I'),
    }
    if explicit.count_only() {
        parts.push('n');
    }
    if explicit.use_last_search() {
        parts.push('r');
    }
    // Note: we intentionally do NOT re-add `&` to avoid infinite recursion.
    SubFlags::parse(&parts)
}

/// Apply `edcompatible` sticky toggle logic for `:s` flags.
///
/// When `edcompatible` is enabled, the `g` and `c` flags are "sticky" —
/// they persist from the previous `:s` command. When one of those flags
/// appears in the current command it **toggles** the sticky state rather
/// than simply setting it.
///
/// All other flags (`i`, `I`, `n`, `r`) are taken as-is from the current
/// command and do not affect the sticky state.
fn apply_edcompatible_flags(prev: Option<SubFlags>, explicit: SubFlags) -> SubFlags {
    use crate::primitives::CaseSensitivity;

    let base = prev.unwrap_or_default();

    // For g and c: toggle if the explicit command specifies the flag.
    let sticky_global = base.global() ^ explicit.global();
    let sticky_confirm = base.confirm() ^ explicit.confirm();

    // Non-sticky flags come directly from the current command.
    let case = explicit.case();
    let count_only = explicit.count_only();
    let use_last_search = explicit.use_last_search();

    // Build the resolved flag string.  `i`/`I` mutual-exclusion is handled
    // by SubFlags::parse (last-one-wins), so we emit at most one of them.
    let mut parts = String::new();
    if sticky_global {
        parts.push('g');
    }
    if sticky_confirm {
        parts.push('c');
    }
    match case {
        CaseSensitivity::Default => {}
        CaseSensitivity::IgnoreCase => parts.push('i'),
        CaseSensitivity::CaseSensitive => parts.push('I'),
    }
    if count_only {
        parts.push('n');
    }
    if use_last_search {
        parts.push('r');
    }
    SubFlags::parse(&parts)
}

/// Resolve a `:move`/`:copy` target line spec.
///
/// Vim's `:co 0` (target line 0) means "before the first line".
/// [`resolve_line_spec`] clamps `Absolute(0)` to 0, losing the distinction
/// from `Absolute(1)` (first real line).  This helper returns `None` for the
/// "before first line" case so that `copy_lines`/`move_lines` can insert at
/// the beginning of the document.
fn resolve_target_line(spec: &LineSpec, ctx: &ExContext<'_>) -> Result<Option<usize>, VimError> {
    if matches!(spec, LineSpec::Absolute(0)) {
        return Ok(None);
    }
    ex::range::resolve_line_spec(spec, ctx).map(Some)
}

/// Resolve an ex range through the dispatch boundary.
pub(crate) fn dispatch_resolve_ex_range(
    range: &ExRange,
    ctx: &ExContext<'_>,
) -> Result<crate::commands::ex::types::ResolvedRange, VimError> {
    ex::resolve_range(range, ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
    use crate::primitives::SubFlags;

    // ── apply_edcompatible_flags unit tests ──────────────────────────────

    #[test]
    fn edcompatible_no_prev_no_explicit_is_empty() {
        // No prior flags, no explicit flags → all off
        let result = apply_edcompatible_flags(None, SubFlags::default());
        assert!(!result.global());
        assert!(!result.confirm());
    }

    #[test]
    fn edcompatible_g_flag_persists_when_not_repeated() {
        // Prior: g=ON; current: no g → sticky g stays ON
        let prev = SubFlags::parse("g");
        let explicit = SubFlags::default(); // no g
        let result = apply_edcompatible_flags(Some(prev), explicit);
        assert!(result.global(), "sticky g should remain on");
        assert!(!result.confirm());
    }

    #[test]
    fn edcompatible_g_flag_toggles_off_when_repeated() {
        // Prior: g=ON; current: g → toggles to OFF
        let prev = SubFlags::parse("g");
        let explicit = SubFlags::parse("g");
        let result = apply_edcompatible_flags(Some(prev), explicit);
        assert!(!result.global(), "g should toggle off");
    }

    #[test]
    fn edcompatible_g_flag_toggles_on_from_off() {
        // Prior: g=OFF; current: g → toggles to ON
        let prev = SubFlags::default();
        let explicit = SubFlags::parse("g");
        let result = apply_edcompatible_flags(Some(prev), explicit);
        assert!(result.global(), "g should toggle on");
    }

    #[test]
    fn edcompatible_c_flag_persists_when_not_repeated() {
        // Prior: c=ON; current: no c → sticky c stays ON
        let prev = SubFlags::parse("c");
        let explicit = SubFlags::default();
        let result = apply_edcompatible_flags(Some(prev), explicit);
        assert!(result.confirm(), "sticky c should remain on");
    }

    #[test]
    fn edcompatible_c_flag_toggles_off_when_repeated() {
        // Prior: c=ON; current: c → toggles to OFF
        let prev = SubFlags::parse("c");
        let explicit = SubFlags::parse("c");
        let result = apply_edcompatible_flags(Some(prev), explicit);
        assert!(!result.confirm(), "c should toggle off");
    }

    #[test]
    fn edcompatible_gc_both_toggle_independently() {
        // Prior: g=ON, c=ON; current: g only → g toggles off, c stays on
        let prev = SubFlags::parse("gc");
        let explicit = SubFlags::parse("g");
        let result = apply_edcompatible_flags(Some(prev), explicit);
        assert!(!result.global(), "g should toggle off");
        assert!(result.confirm(), "c should stay on (not specified)");
    }

    #[test]
    fn edcompatible_non_sticky_i_flag_not_carried_forward() {
        // Prior: i=ON (case-insensitive); current: no i → i is NOT sticky, does NOT persist
        let prev = SubFlags::parse("i");
        let explicit = SubFlags::default(); // no i
        let result = apply_edcompatible_flags(Some(prev), explicit);
        // i is non-sticky: should not carry forward from prev
        assert_eq!(result.case(), crate::primitives::CaseSensitivity::Default);
    }

    #[test]
    fn edcompatible_non_sticky_i_flag_applies_when_explicit() {
        // Current: i → applied
        let prev = SubFlags::default();
        let explicit = SubFlags::parse("i");
        let result = apply_edcompatible_flags(Some(prev), explicit);
        assert_eq!(
            result.case(),
            crate::primitives::CaseSensitivity::IgnoreCase
        );
    }

    #[test]
    fn edcompatible_g_persists_across_three_calls() {
        // First call: set g → g=ON
        let flags1 = apply_edcompatible_flags(None, SubFlags::parse("g"));
        assert!(flags1.global());

        // Second call: no g → g stays ON (sticky)
        let flags2 = apply_edcompatible_flags(Some(flags1), SubFlags::default());
        assert!(flags2.global());

        // Third call: g again → toggles OFF
        let flags3 = apply_edcompatible_flags(Some(flags2), SubFlags::parse("g"));
        assert!(!flags3.global());
    }

    // ── Integration tests through dispatch_ex_core ────────────────────────

    /// Helper: run a substitute command via dispatch, returning the effective flags
    /// stored in `SetLastSubstituteFlags`.
    fn dispatch_substitute<'a>(
        pattern: &str,
        flags_str: &str,
        ctx: &ExContext<'a>,
    ) -> crate::effects::Effects {
        let command = ExCommand::Substitute {
            range: crate::grammar::types::ExRange::current_line(),
            pattern: compact_str::CompactString::from(pattern),
            replacement: compact_str::CompactString::from("X"),
            flags: SubFlags::parse(flags_str),
        };
        dispatch_ex_core(&command, ctx)
            .expect("Substitute is handled")
            .expect("substitute should not error")
    }

    #[test]
    fn edcompatible_g_sticky_persists_in_second_call() {
        // First call: :s/a/X/g with edcompatible — sets sticky g=ON, stores in
        // SetLastSubstituteFlags.  Second call: :s/a/X/ with no g — sticky g
        // from first call should mean the second call still replaces globally.
        let text = "aaa";
        let ctx1 = ExContext::new(text, 0).with_edcompatible(true);
        let effects1 = dispatch_substitute("a", "g", &ctx1);

        // Extract the last substitute flags saved after the first call.
        let saved_flags = effects1.iter().find_map(|e| match e {
            Effect::SetLastSubstituteFlags { flags } => Some(*flags),
            _ => None,
        });
        assert!(saved_flags.is_some(), "first :s should save flags");
        let saved = saved_flags.unwrap();
        assert!(saved.global(), "after first :s/g, sticky g should be ON");

        // Build context for second call using the saved flags.
        let ctx2 = ExContext::new(text, 0)
            .with_edcompatible(true)
            .with_last_substitute_flags(Some(saved));
        let effects2 = dispatch_substitute("a", "", &ctx2); // no g flag

        // Should still replace all — check the count message (3 subs)
        let msg = effects2.iter().find_map(|e| {
            if let Effect::ShowInfo {
                info: crate::effects::InfoMessage::Text(text),
                ..
            } = e
            {
                Some(text.as_str().to_owned())
            } else {
                None
            }
        });
        assert!(
            msg.as_deref().is_some_and(|m| m.contains('3')),
            "expected 3 substitutions (sticky g), got: {msg:?}"
        );
    }

    #[test]
    fn edcompatible_g_toggles_off_on_second_g() {
        // First call: :s/a/X/g → g=ON. Second call: :s/a/X/g → g toggles OFF.
        let text = "aaa";

        // First call sets g=ON.
        let ctx1 = ExContext::new(text, 0).with_edcompatible(true);
        let effects1 = dispatch_substitute("a", "g", &ctx1);
        let saved1 = effects1.iter().find_map(|e| match e {
            Effect::SetLastSubstituteFlags { flags } => Some(*flags),
            _ => None,
        });
        assert!(saved1.is_some_and(|f| f.global()), "first call: g=ON");

        // Second call with g again → should toggle off.
        let ctx2 = ExContext::new(text, 0)
            .with_edcompatible(true)
            .with_last_substitute_flags(saved1);
        let effects2 = dispatch_substitute("a", "g", &ctx2);

        // g is toggled OFF → only first occurrence replaced → 1 substitution
        let msg = effects2.iter().find_map(|e| {
            if let Effect::ShowInfo {
                info: crate::effects::InfoMessage::Text(text),
                ..
            } = e
            {
                Some(text.as_str().to_owned())
            } else {
                None
            }
        });
        assert!(
            msg.as_deref().is_some_and(|m| m.contains('1')),
            "expected 1 substitution (g toggled off), got: {msg:?}"
        );

        // Third call without g → g still OFF (sticky).
        let saved2 = effects2.iter().find_map(|e| match e {
            Effect::SetLastSubstituteFlags { flags } => Some(*flags),
            _ => None,
        });
        assert!(
            saved2.is_some_and(|f| !f.global()),
            "after toggle-off, sticky g should be OFF"
        );
    }

    #[test]
    fn without_edcompatible_flags_not_sticky() {
        // Without edcompatible, second :s/a/X/ with no g replaces first only.
        let text = "aaa";
        let ctx1 = ExContext::new(text, 0); // edcompatible=false (default)
        let effects1 = dispatch_substitute("a", "g", &ctx1);
        let saved1 = effects1.iter().find_map(|e| match e {
            Effect::SetLastSubstituteFlags { flags } => Some(*flags),
            _ => None,
        });

        // Second call: no g, no edcompatible → first only
        let ctx2 = ExContext::new(text, 0).with_last_substitute_flags(saved1);
        let effects2 = dispatch_substitute("a", "", &ctx2);
        let msg = effects2.iter().find_map(|e| {
            if let Effect::ShowInfo {
                info: crate::effects::InfoMessage::Text(text),
                ..
            } = e
            {
                Some(text.as_str().to_owned())
            } else {
                None
            }
        });
        assert!(
            msg.as_deref().is_some_and(|m| m.contains('1')),
            "without edcompatible, second :s with no g replaces first only: {msg:?}"
        );
    }
}
