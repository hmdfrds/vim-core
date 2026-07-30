//! Effect filtering for command modifiers.
//!
//! Applies [`ModifierFlags`] as a post-processing pass on the effects output.
//! One call to [`apply_modifiers`] at the pipeline exit replaces hundreds of
//! scattered flag checks throughout the codebase.

use crate::effects::{Effect, Effects};
use crate::grammar::types::ModifierFlags;
use crate::primitives::RegisterName;

/// Remove effects that are suppressed by the given modifier flags.
///
/// This is the core of the "effect transformer" pattern: modifiers don't
/// change how commands execute — they filter the output afterward.
///
/// Operates on `Effects` by draining and re-collecting, using only
/// `pub(crate)` APIs (`drain`, `push`).
///
/// # Effect suppression rules
///
/// | Flag           | Suppressed effects                                        |
/// |----------------|----------------------------------------------------------|
/// | `SILENT`       | `ShowInfo`                                                |
/// | `SILENT_BANG`  | `ShowInfo`, `ShowError`                                   |
/// | `KEEPJUMPS`    | `PushJumpList`                                            |
/// | `KEEPPATTERNS` | `SetSearchPattern`, `SetSubstitutePattern`                |
/// | `LOCKMARKS`    | `SetMark`                                                 |
/// | `KEEPALT`      | `SetRegister` where register is `#` (alternate file)      |
/// | `NOAUTOCMD`    | (reserved for future autocmd/event effects)               |
pub(crate) fn apply_modifiers(effects: &mut Effects, flags: ModifierFlags) {
    if flags.is_empty() {
        return;
    }

    // Drain all effects, keep only non-suppressed ones.
    let kept: smallvec::SmallVec<[Effect; 4]> = effects
        .drain()
        .filter(|effect| !is_suppressed(effect, flags))
        .collect();
    effects.extend(kept);
}

/// Returns `true` if the given effect should be removed under `flags`.
fn is_suppressed(effect: &Effect, flags: ModifierFlags) -> bool {
    match effect {
        // SILENT suppresses informational messages.
        Effect::ShowInfo { .. } => flags.intersects(ModifierFlags::SILENT),

        // SILENT_BANG also suppresses errors.
        Effect::ShowError { .. } => flags.contains(ModifierFlags::SILENT_BANG),

        // KEEPJUMPS suppresses jump list pushes.
        Effect::PushJumpList { .. } => flags.contains(ModifierFlags::KEEPJUMPS),

        // KEEPPATTERNS suppresses search/substitute pattern updates.
        Effect::SetSearchPattern { .. } | Effect::SetSubstitutePattern { .. } => {
            flags.contains(ModifierFlags::KEEPPATTERNS)
        }

        // LOCKMARKS suppresses mark setting.
        Effect::SetMark { .. } => flags.contains(ModifierFlags::LOCKMARKS),

        // KEEPALT suppresses alternate file register changes.
        // The alternate file is stored in register `#`.
        Effect::SetRegister { name, .. } => {
            flags.contains(ModifierFlags::KEEPALT) && *name == RegisterName::ALTERNATE
        }

        // Everything else passes through.
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::InfoMessage;
    use crate::errors::VimError;
    use crate::primitives::{Direction, MarkName, MotionType, Offset};
    use compact_str::CompactString;

    /// Build an `Effects` from a vec of raw effects.
    fn effects_from(items: Vec<Effect>) -> Effects {
        let mut e = Effects::new();
        e.extend(items);
        e
    }

    fn make_show_info() -> Effect {
        Effect::ShowInfo {
            info: InfoMessage::Text(CompactString::from("hello")),
        }
    }

    fn make_show_error() -> Effect {
        Effect::ShowError {
            error: VimError::PatternNotFound(CompactString::from("test")),
            source: None,
        }
    }

    fn make_push_jump() -> Effect {
        Effect::PushJumpList {
            offset: Offset::ZERO,
        }
    }

    fn make_set_search_pattern() -> Effect {
        Effect::SetSearchPattern {
            pattern: CompactString::from("foo"),
            direction: Direction::Forward,
        }
    }

    fn make_set_substitute_pattern() -> Effect {
        Effect::SetSubstitutePattern {
            pattern: CompactString::from("bar"),
        }
    }

    fn make_set_mark() -> Effect {
        Effect::SetMark {
            name: MarkName::new('a').unwrap(),
            offset: Offset::ZERO,
            topline_offset: None,
        }
    }

    fn make_set_cursor() -> Effect {
        Effect::SetCursor {
            offset: Offset::ZERO,
        }
    }

    fn make_set_alternate_register() -> Effect {
        Effect::SetRegister {
            name: RegisterName::ALTERNATE,
            text: CompactString::from("/tmp/other.txt"),
            motion_type: MotionType::CharWise,
        }
    }

    #[test]
    fn empty_flags_no_filtering() {
        let mut effects = effects_from(vec![make_show_info(), make_set_cursor()]);
        apply_modifiers(&mut effects, ModifierFlags::empty());
        assert_eq!(effects.len(), 2);
    }

    #[test]
    fn silent_removes_show_info() {
        let mut effects =
            effects_from(vec![make_show_info(), make_set_cursor(), make_show_error()]);
        apply_modifiers(&mut effects, ModifierFlags::SILENT);
        // ShowInfo removed, ShowError and SetCursor remain.
        let v = effects.into_vec();
        assert_eq!(v.len(), 2);
        assert!(matches!(v[0], Effect::SetCursor { .. }));
        assert!(matches!(v[1], Effect::ShowError { .. }));
    }

    #[test]
    fn silent_bang_removes_show_info_and_show_error() {
        let mut effects =
            effects_from(vec![make_show_info(), make_show_error(), make_set_cursor()]);
        apply_modifiers(
            &mut effects,
            ModifierFlags::SILENT | ModifierFlags::SILENT_BANG,
        );
        let v = effects.into_vec();
        assert_eq!(v.len(), 1);
        assert!(matches!(v[0], Effect::SetCursor { .. }));
    }

    #[test]
    fn keepjumps_removes_push_jump() {
        let mut effects = effects_from(vec![make_push_jump(), make_set_cursor(), make_show_info()]);
        apply_modifiers(&mut effects, ModifierFlags::KEEPJUMPS);
        let v = effects.into_vec();
        assert_eq!(v.len(), 2);
        assert!(matches!(v[0], Effect::SetCursor { .. }));
        assert!(matches!(v[1], Effect::ShowInfo { .. }));
    }

    #[test]
    fn keeppatterns_removes_search_and_substitute_patterns() {
        let mut effects = effects_from(vec![
            make_set_search_pattern(),
            make_set_substitute_pattern(),
            make_set_cursor(),
        ]);
        apply_modifiers(&mut effects, ModifierFlags::KEEPPATTERNS);
        let v = effects.into_vec();
        assert_eq!(v.len(), 1);
        assert!(matches!(v[0], Effect::SetCursor { .. }));
    }

    #[test]
    fn lockmarks_removes_set_mark() {
        let mut effects = effects_from(vec![make_set_mark(), make_set_cursor()]);
        apply_modifiers(&mut effects, ModifierFlags::LOCKMARKS);
        let v = effects.into_vec();
        assert_eq!(v.len(), 1);
        assert!(matches!(v[0], Effect::SetCursor { .. }));
    }

    #[test]
    fn keepalt_removes_alternate_register_only() {
        let normal_register = Effect::SetRegister {
            name: RegisterName::new('a').unwrap(),
            text: CompactString::from("text"),
            motion_type: MotionType::CharWise,
        };
        let mut effects = effects_from(vec![
            make_set_alternate_register(),
            normal_register,
            make_set_cursor(),
        ]);
        apply_modifiers(&mut effects, ModifierFlags::KEEPALT);
        // Only the alternate register is removed; the 'a' register remains.
        let v = effects.into_vec();
        assert_eq!(v.len(), 2);
        assert!(matches!(
            v[0],
            Effect::SetRegister {
                name,
                ..
            } if name == RegisterName::new('a').unwrap()
        ));
    }

    #[test]
    fn composed_silent_and_keepjumps() {
        let mut effects = effects_from(vec![make_show_info(), make_push_jump(), make_set_cursor()]);
        apply_modifiers(
            &mut effects,
            ModifierFlags::SILENT | ModifierFlags::KEEPJUMPS,
        );
        let v = effects.into_vec();
        assert_eq!(v.len(), 1);
        assert!(matches!(v[0], Effect::SetCursor { .. }));
    }

    #[test]
    fn all_flags_combined() {
        let mut effects = effects_from(vec![
            make_show_info(),
            make_show_error(),
            make_push_jump(),
            make_set_search_pattern(),
            make_set_substitute_pattern(),
            make_set_mark(),
            make_set_alternate_register(),
            make_set_cursor(), // should survive
        ]);
        let all_flags = ModifierFlags::SILENT
            | ModifierFlags::SILENT_BANG
            | ModifierFlags::KEEPJUMPS
            | ModifierFlags::KEEPPATTERNS
            | ModifierFlags::LOCKMARKS
            | ModifierFlags::KEEPALT;
        apply_modifiers(&mut effects, all_flags);
        let v = effects.into_vec();
        assert_eq!(v.len(), 1);
        assert!(matches!(v[0], Effect::SetCursor { .. }));
    }

    // ========== TASK 5.9: keepjumps behavior tests ==========

    #[test]
    fn task_5_9_keepjumps_strips_push_jump_from_delete() {
        // Simulates `:keepjumps d3j` — PushJumpList should be stripped.
        let mut effects = effects_from(vec![make_push_jump(), make_set_cursor()]);
        apply_modifiers(&mut effects, ModifierFlags::KEEPJUMPS);
        let v = effects.into_vec();
        assert_eq!(v.len(), 1, "PushJumpList must be removed by KEEPJUMPS");
        assert!(matches!(v[0], Effect::SetCursor { .. }));
    }

    #[test]
    fn task_5_9_without_keepjumps_push_jump_present() {
        // Without keepjumps, PushJumpList should remain.
        let mut effects = effects_from(vec![make_push_jump(), make_set_cursor()]);
        apply_modifiers(&mut effects, ModifierFlags::empty());
        let v = effects.into_vec();
        assert_eq!(v.len(), 2, "PushJumpList must remain without KEEPJUMPS");
        assert!(matches!(v[0], Effect::PushJumpList { .. }));
        assert!(matches!(v[1], Effect::SetCursor { .. }));
    }

    #[test]
    fn task_5_9_keepjumps_preserves_other_effects() {
        // KEEPJUMPS only strips PushJumpList, not SetMark or SetCursor.
        let mut effects = effects_from(vec![
            make_push_jump(),
            make_set_mark(),
            make_set_cursor(),
            make_show_info(),
        ]);
        apply_modifiers(&mut effects, ModifierFlags::KEEPJUMPS);
        let v = effects.into_vec();
        assert_eq!(v.len(), 3, "Only PushJumpList should be removed");
        assert!(matches!(v[0], Effect::SetMark { .. }));
        assert!(matches!(v[1], Effect::SetCursor { .. }));
        assert!(matches!(v[2], Effect::ShowInfo { .. }));
    }

    #[test]
    fn task_5_9_keepjumps_strips_multiple_push_jumps() {
        // Multiple PushJumpList effects should all be stripped.
        let mut effects = effects_from(vec![
            make_push_jump(),
            make_set_cursor(),
            Effect::PushJumpList {
                offset: Offset::new(42),
            },
            Effect::PushJumpList {
                offset: Offset::new(100),
            },
        ]);
        apply_modifiers(&mut effects, ModifierFlags::KEEPJUMPS);
        let v = effects.into_vec();
        assert_eq!(v.len(), 1);
        assert!(matches!(v[0], Effect::SetCursor { .. }));
    }

    // ========== TASK 5.10: lockmarks behavior tests ==========

    #[test]
    fn task_5_10_lockmarks_strips_set_mark_from_delete() {
        // Simulates `:lockmarks d3j` — SetMark should be stripped.
        let mut effects = effects_from(vec![make_set_mark(), make_set_cursor()]);
        apply_modifiers(&mut effects, ModifierFlags::LOCKMARKS);
        let v = effects.into_vec();
        assert_eq!(v.len(), 1, "SetMark must be removed by LOCKMARKS");
        assert!(matches!(v[0], Effect::SetCursor { .. }));
    }

    #[test]
    fn task_5_10_without_lockmarks_set_mark_present() {
        // Without lockmarks, SetMark should remain.
        let mut effects = effects_from(vec![make_set_mark(), make_set_cursor()]);
        apply_modifiers(&mut effects, ModifierFlags::empty());
        let v = effects.into_vec();
        assert_eq!(v.len(), 2, "SetMark must remain without LOCKMARKS");
        assert!(matches!(v[0], Effect::SetMark { .. }));
        assert!(matches!(v[1], Effect::SetCursor { .. }));
    }

    #[test]
    fn task_5_10_lockmarks_preserves_other_effects() {
        // LOCKMARKS only strips SetMark, not PushJumpList or SetCursor.
        let mut effects = effects_from(vec![
            make_set_mark(),
            make_push_jump(),
            make_set_cursor(),
            make_show_info(),
        ]);
        apply_modifiers(&mut effects, ModifierFlags::LOCKMARKS);
        let v = effects.into_vec();
        assert_eq!(v.len(), 3, "Only SetMark should be removed");
        assert!(matches!(v[0], Effect::PushJumpList { .. }));
        assert!(matches!(v[1], Effect::SetCursor { .. }));
        assert!(matches!(v[2], Effect::ShowInfo { .. }));
    }

    #[test]
    fn task_5_10_lockmarks_strips_multiple_set_marks() {
        // Multiple SetMark effects should all be stripped.
        let mark_b = Effect::SetMark {
            name: MarkName::new('b').unwrap(),
            offset: Offset::new(50),
            topline_offset: None,
        };
        let mut effects = effects_from(vec![make_set_mark(), mark_b, make_set_cursor()]);
        apply_modifiers(&mut effects, ModifierFlags::LOCKMARKS);
        let v = effects.into_vec();
        assert_eq!(v.len(), 1);
        assert!(matches!(v[0], Effect::SetCursor { .. }));
    }

    #[test]
    fn task_5_10_lockmarks_and_keepjumps_combined() {
        // Both LOCKMARKS and KEEPJUMPS active: both SetMark and PushJumpList stripped.
        let mut effects = effects_from(vec![make_set_mark(), make_push_jump(), make_set_cursor()]);
        apply_modifiers(
            &mut effects,
            ModifierFlags::LOCKMARKS | ModifierFlags::KEEPJUMPS,
        );
        let v = effects.into_vec();
        assert_eq!(v.len(), 1, "Only SetCursor should remain");
        assert!(matches!(v[0], Effect::SetCursor { .. }));
    }

    // ========== TASK 6.5: keeppatterns behavior tests ==========

    #[test]
    fn task_6_5_keeppatterns_strips_search_pattern_only() {
        // KEEPPATTERNS should strip SetSearchPattern but leave other effects intact.
        let mut effects = effects_from(vec![
            make_set_search_pattern(),
            make_set_cursor(),
            make_show_info(),
            make_push_jump(),
        ]);
        apply_modifiers(&mut effects, ModifierFlags::KEEPPATTERNS);
        let v = effects.into_vec();
        assert_eq!(v.len(), 3, "Only SetSearchPattern should be removed");
        assert!(matches!(v[0], Effect::SetCursor { .. }));
        assert!(matches!(v[1], Effect::ShowInfo { .. }));
        assert!(matches!(v[2], Effect::PushJumpList { .. }));
    }

    #[test]
    fn task_6_5_keeppatterns_strips_substitute_pattern_only() {
        // KEEPPATTERNS should strip SetSubstitutePattern but leave other effects intact.
        let mut effects = effects_from(vec![
            make_set_substitute_pattern(),
            make_set_cursor(),
            make_show_info(),
        ]);
        apply_modifiers(&mut effects, ModifierFlags::KEEPPATTERNS);
        let v = effects.into_vec();
        assert_eq!(v.len(), 2, "Only SetSubstitutePattern should be removed");
        assert!(matches!(v[0], Effect::SetCursor { .. }));
        assert!(matches!(v[1], Effect::ShowInfo { .. }));
    }

    #[test]
    fn task_6_5_without_keeppatterns_patterns_preserved() {
        // Without KEEPPATTERNS, both pattern effects should remain.
        let mut effects = effects_from(vec![
            make_set_search_pattern(),
            make_set_substitute_pattern(),
            make_set_cursor(),
        ]);
        apply_modifiers(&mut effects, ModifierFlags::empty());
        let v = effects.into_vec();
        assert_eq!(v.len(), 3, "All effects should remain without KEEPPATTERNS");
        assert!(matches!(v[0], Effect::SetSearchPattern { .. }));
        assert!(matches!(v[1], Effect::SetSubstitutePattern { .. }));
        assert!(matches!(v[2], Effect::SetCursor { .. }));
    }

    #[test]
    fn task_6_5_keeppatterns_with_search_simulates_keeppatterns_search() {
        // Simulates `:keeppatterns /foo/` — SetSearchPattern from the search
        // command should be stripped, leaving only the cursor movement.
        let search_pattern = Effect::SetSearchPattern {
            pattern: CompactString::from("foo"),
            direction: Direction::Forward,
        };
        let cursor = Effect::SetCursor {
            offset: Offset::new(42),
        };
        let mut effects = effects_from(vec![search_pattern, cursor]);
        apply_modifiers(&mut effects, ModifierFlags::KEEPPATTERNS);
        let v = effects.into_vec();
        assert_eq!(v.len(), 1, "Only cursor movement should remain");
        assert!(matches!(
            v[0],
            Effect::SetCursor { offset } if offset == Offset::new(42)
        ));
    }

    #[test]
    fn task_6_5_keeppatterns_combined_with_other_modifiers() {
        // KEEPPATTERNS + KEEPJUMPS: both patterns and jumps stripped.
        let mut effects = effects_from(vec![
            make_set_search_pattern(),
            make_set_substitute_pattern(),
            make_push_jump(),
            make_set_cursor(),
        ]);
        apply_modifiers(
            &mut effects,
            ModifierFlags::KEEPPATTERNS | ModifierFlags::KEEPJUMPS,
        );
        let v = effects.into_vec();
        assert_eq!(v.len(), 1, "Only SetCursor should remain");
        assert!(matches!(v[0], Effect::SetCursor { .. }));
    }

    // ========== TASK 8.7: :silent behavior verification ==========

    #[test]
    fn task_8_7_silent_strips_show_info_only() {
        // `:silent` should strip ShowInfo but preserve ShowError and everything else.
        let mut effects =
            effects_from(vec![make_show_info(), make_show_error(), make_set_cursor()]);
        apply_modifiers(&mut effects, ModifierFlags::SILENT);
        let v = effects.into_vec();
        assert_eq!(v.len(), 2, "Only ShowInfo should be removed");
        assert!(matches!(v[0], Effect::ShowError { .. }));
        assert!(matches!(v[1], Effect::SetCursor { .. }));
    }

    #[test]
    fn task_8_7_silent_bang_strips_both_info_and_error() {
        // `:silent!` should strip both ShowInfo AND ShowError.
        let mut effects =
            effects_from(vec![make_show_info(), make_show_error(), make_set_cursor()]);
        apply_modifiers(
            &mut effects,
            ModifierFlags::SILENT | ModifierFlags::SILENT_BANG,
        );
        let v = effects.into_vec();
        assert_eq!(v.len(), 1, "Both ShowInfo and ShowError should be removed");
        assert!(matches!(v[0], Effect::SetCursor { .. }));
    }

    #[test]
    fn task_8_7_silent_preserves_non_message_effects() {
        // `:silent` should not affect cursor, marks, registers, etc.
        let normal_register = Effect::SetRegister {
            name: RegisterName::new('a').unwrap(),
            text: CompactString::from("text"),
            motion_type: MotionType::CharWise,
        };
        let mut effects = effects_from(vec![
            make_show_info(),
            normal_register,
            make_set_cursor(),
            make_set_mark(),
            make_push_jump(),
        ]);
        apply_modifiers(&mut effects, ModifierFlags::SILENT);
        let v = effects.into_vec();
        assert_eq!(
            v.len(),
            4,
            "Only ShowInfo removed; register, cursor, mark, jump preserved"
        );
        assert!(matches!(v[0], Effect::SetRegister { .. }));
        assert!(matches!(v[1], Effect::SetCursor { .. }));
        assert!(matches!(v[2], Effect::SetMark { .. }));
        assert!(matches!(v[3], Effect::PushJumpList { .. }));
    }

    #[test]
    fn task_8_7_silent_without_bang_does_not_strip_errors() {
        // `:silent` (without `!`) must NOT strip ShowError effects.
        let mut effects = effects_from(vec![make_show_error(), make_set_cursor()]);
        apply_modifiers(&mut effects, ModifierFlags::SILENT);
        let v = effects.into_vec();
        assert_eq!(v.len(), 2, "ShowError must remain with SILENT (no bang)");
        assert!(matches!(v[0], Effect::ShowError { .. }));
        assert!(matches!(v[1], Effect::SetCursor { .. }));
    }

    #[test]
    fn task_8_7_silent_bang_strips_multiple_messages() {
        // Multiple ShowInfo and ShowError effects should all be stripped.
        let info2 = Effect::ShowInfo {
            info: InfoMessage::Text(CompactString::from("world")),
        };
        let error2 = Effect::ShowError {
            error: VimError::PatternNotFound(CompactString::from("test2")),
            source: None,
        };
        let mut effects = effects_from(vec![
            make_show_info(),
            info2,
            make_show_error(),
            error2,
            make_set_cursor(),
        ]);
        apply_modifiers(
            &mut effects,
            ModifierFlags::SILENT | ModifierFlags::SILENT_BANG,
        );
        let v = effects.into_vec();
        assert_eq!(v.len(), 1, "All messages should be stripped");
        assert!(matches!(v[0], Effect::SetCursor { .. }));
    }
}
