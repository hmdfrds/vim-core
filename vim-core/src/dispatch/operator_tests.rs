use super::*;
use crate::primitives::{MotionType, Offset, Range};

#[test]
fn test_dispatch_delete() {
    let text = "hello world";
    let range = Range::from_raw(0, 5);
    let ctx = OperatorContext::new(text, range, MotionType::CharWise, None, 1, Offset::new(0));

    let result = dispatch_operator(Operator::Delete, &ctx);
    assert!(!result.is_empty());
    assert!(
        result
            .effects
            .iter()
            .any(|e| matches!(e, crate::effects::Effect::Delete { .. })),
        "delete should produce Delete effect"
    );
}

#[test]
fn test_dispatch_yank() {
    let text = "hello world";
    let range = Range::from_raw(0, 5);
    let ctx = OperatorContext::new(text, range, MotionType::CharWise, None, 1, Offset::new(0));

    let result = dispatch_operator(Operator::Yank, &ctx);
    assert!(!result.is_empty());
    assert!(
        result
            .effects
            .iter()
            .any(|e| matches!(e, crate::effects::Effect::SetRegister { .. })),
        "yank should produce SetRegister effect"
    );
}

// ── dispatch_operator_textobject ────────────────────────────────────

#[test]
fn test_operator_textobject_delete_word() {
    use crate::grammar::types::{TextObject, TextObjectKind, TextObjectScope};

    let text = "hello world";
    let result = dispatch_operator_textobject(&OperatorTextObjectInput {
        operator: Operator::Delete,
        textobject: TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Word,
            seek: None,
        },
        count: 1,
        register: None,
        text,
        options: &crate::primitives::VimOptions::default(),
        cursor: 0, // cursor on 'h'
        shiftwidth: 4,
        tabstop: 4,
        expandtab: true,
        textwidth: 80,
        providers: crate::document::Providers::new(),
        commentstring: "// %s",
        custom_operators: None,
    });
    assert!(!result.is_empty(), "diw on 'hello' should produce effects");
}

#[test]
fn test_operator_textobject_yank_paren() {
    use crate::grammar::types::{TextObject, TextObjectKind, TextObjectScope};

    let text = "foo(bar)baz";
    let result = dispatch_operator_textobject(&OperatorTextObjectInput {
        operator: Operator::Yank,
        textobject: TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Paren,
            seek: None,
        },
        count: 1,
        register: None,
        text,
        options: &crate::primitives::VimOptions::default(),
        cursor: 4, // cursor on 'b' inside parens
        shiftwidth: 4,
        tabstop: 4,
        expandtab: true,
        textwidth: 80,
        providers: crate::document::Providers::new(),
        commentstring: "// %s",
        custom_operators: None,
    });
    assert!(!result.is_empty(), "yi( on 'bar' should produce effects");
}

#[test]
fn test_operator_textobject_no_match_empty() {
    use crate::grammar::types::{TextObject, TextObjectKind, TextObjectScope};

    let text = "hello";
    let result = dispatch_operator_textobject(&OperatorTextObjectInput {
        operator: Operator::Delete,
        textobject: TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Paren,
            seek: None,
        },
        count: 1,
        register: None,
        text,
        options: &crate::primitives::VimOptions::default(),
        cursor: 0,
        shiftwidth: 4,
        tabstop: 4,
        expandtab: true,
        textwidth: 80,
        providers: crate::document::Providers::new(),
        commentstring: "// %s",
        custom_operators: None,
    });
    assert!(
        result.is_empty(),
        "di( with no parens should produce empty effects"
    );
}

// ── Custom operator dispatch ──────────────────────────────────────

#[test]
fn custom_operator_without_provider_falls_back_to_call_operator_func() {
    let text = "hello world";
    let range = Range::from_raw(0, 5);
    let ctx = OperatorContext::new(text, range, MotionType::CharWise, None, 1, Offset::new(0));

    let result = dispatch_operator(Operator::Custom(42), &ctx);
    assert!(!result.is_empty());
    // Should produce CallOperatorFunc effect
    assert!(
        result
            .effects
            .iter()
            .any(|e| matches!(e, crate::effects::Effect::CallOperatorFunc { .. })),
        "Custom without provider should fall back to CallOperatorFunc"
    );
}

#[test]
fn custom_operator_with_provider_replace() {
    use crate::document::{CustomOperatorProvider, CustomOperatorResult};

    struct ReplaceProvider;
    impl CustomOperatorProvider for ReplaceProvider {
        fn compute_operator(
            &self,
            _id: u32,
            _text: &str,
            _range: (usize, usize),
            _count: u32,
        ) -> Option<CustomOperatorResult> {
            Some(CustomOperatorResult::Replace("REPLACED".into()))
        }
    }

    let text = "hello world";
    let range = Range::from_raw(0, 5);
    let provider = ReplaceProvider;
    let ctx = OperatorContext::new(text, range, MotionType::CharWise, None, 1, Offset::new(0))
        .with_custom_operators(&provider);

    let result = dispatch_operator(Operator::Custom(1), &ctx);
    assert!(
        result
            .effects
            .iter()
            .any(|e| matches!(e, crate::effects::Effect::Replace { .. })),
        "Replace result should produce Replace effect"
    );
}

#[test]
fn custom_operator_with_provider_delete() {
    use crate::document::{CustomOperatorProvider, CustomOperatorResult};

    struct DeleteProvider;
    impl CustomOperatorProvider for DeleteProvider {
        fn compute_operator(
            &self,
            _id: u32,
            _text: &str,
            _range: (usize, usize),
            _count: u32,
        ) -> Option<CustomOperatorResult> {
            Some(CustomOperatorResult::Delete)
        }
    }

    let text = "hello world";
    let range = Range::from_raw(0, 5);
    let provider = DeleteProvider;
    let ctx = OperatorContext::new(text, range, MotionType::CharWise, None, 1, Offset::new(0))
        .with_custom_operators(&provider);

    let result = dispatch_operator(Operator::Custom(1), &ctx);
    assert!(
        result
            .effects
            .iter()
            .any(|e| matches!(e, crate::effects::Effect::Delete { .. })),
        "Delete result should produce Delete effect"
    );
}

#[test]
fn custom_operator_with_provider_defer() {
    use crate::document::{CustomOperatorProvider, CustomOperatorResult};

    struct DeferProvider;
    impl CustomOperatorProvider for DeferProvider {
        fn compute_operator(
            &self,
            _id: u32,
            _text: &str,
            _range: (usize, usize),
            _count: u32,
        ) -> Option<CustomOperatorResult> {
            Some(CustomOperatorResult::Defer)
        }
    }

    let text = "hello world";
    let range = Range::from_raw(0, 5);
    let provider = DeferProvider;
    let ctx = OperatorContext::new(text, range, MotionType::CharWise, None, 1, Offset::new(0))
        .with_custom_operators(&provider);

    let result = dispatch_operator(Operator::Custom(1), &ctx);
    assert!(
        result
            .effects
            .iter()
            .any(|e| matches!(e, crate::effects::Effect::CallOperatorFunc { .. })),
        "Defer result should fall back to CallOperatorFunc"
    );
}

#[test]
fn custom_operator_provider_returns_none_falls_back() {
    use crate::document::{CustomOperatorProvider, CustomOperatorResult};

    struct NoneProvider;
    impl CustomOperatorProvider for NoneProvider {
        fn compute_operator(
            &self,
            _id: u32,
            _text: &str,
            _range: (usize, usize),
            _count: u32,
        ) -> Option<CustomOperatorResult> {
            None // Don't handle this ID
        }
    }

    let text = "hello world";
    let range = Range::from_raw(0, 5);
    let provider = NoneProvider;
    let ctx = OperatorContext::new(text, range, MotionType::CharWise, None, 1, Offset::new(0))
        .with_custom_operators(&provider);

    let result = dispatch_operator(Operator::Custom(99), &ctx);
    assert!(
        result
            .effects
            .iter()
            .any(|e| matches!(e, crate::effects::Effect::CallOperatorFunc { .. })),
        "Provider returning None should fall back to CallOperatorFunc"
    );
}

// ── Rot47 operator dispatch ─────────────────────────────────────

#[test]
fn dispatch_rot47_transforms_text() {
    let text = "Hello";
    let range = Range::from_raw(0, 5);
    let ctx = OperatorContext::new(text, range, MotionType::CharWise, None, 1, Offset::new(0));

    let result = dispatch_operator(Operator::Rot47, &ctx);
    assert!(!result.is_empty());
    let has_replace = result.effects.iter().any(|e| {
        if let crate::effects::Effect::Replace { text, .. } = e {
            text.as_str() == "w6==@"
        } else {
            false
        }
    });
    assert!(
        has_replace,
        "dispatch_operator(Rot47) should produce Replace with ROT47-encoded text"
    );
}

#[test]
fn dispatch_rot47_involution_via_dispatch() {
    // Applying Rot47 twice via dispatch should round-trip to original text.
    let text = "Hello, World! 123";
    let range = Range::from_raw(0, text.len());
    let ctx = OperatorContext::new(text, range, MotionType::CharWise, None, 1, Offset::new(0));

    let result = dispatch_operator(Operator::Rot47, &ctx);
    let first_pass = result.effects.iter().find_map(|e| {
        if let crate::effects::Effect::Replace { text, .. } = e {
            Some(text.as_str().to_owned())
        } else {
            None
        }
    });
    let first_pass = first_pass.expect("First Rot47 pass should produce Replace effect");

    // Apply Rot47 again on the transformed text
    let range2 = Range::from_raw(0, first_pass.len());
    let ctx2 = OperatorContext::new(
        &first_pass,
        range2,
        MotionType::CharWise,
        None,
        1,
        Offset::new(0),
    );
    let result2 = dispatch_operator(Operator::Rot47, &ctx2);
    let second_pass = result2.effects.iter().find_map(|e| {
        if let crate::effects::Effect::Replace { text, .. } = e {
            Some(text.as_str().to_owned())
        } else {
            None
        }
    });
    let second_pass = second_pass.expect("Second Rot47 pass should produce Replace effect");
    assert_eq!(
        second_pass, text,
        "Rot47 applied twice via dispatch should return original text"
    );
}

#[test]
fn dispatch_rot47_is_mutating() {
    assert!(
        Operator::Rot47.is_mutating(),
        "Rot47 must report is_mutating() = true"
    );
}

// ── Fold-aware operator range tests ───────────────────────────────

mod fold_aware_operator {
    use crate::commands::operators::OperatorMotionInput;
    use crate::dispatch::operator::dispatch_operator_with_motion;
    use crate::document::FoldProvider;
    use crate::grammar::types::{Motion, Operator};
    use crate::primitives::{Direction, LineNumber, Offset, VimOptions};

    /// Test fold: lines 2-4 are folded (0-indexed).
    struct FoldLines2To4;
    impl FoldProvider for FoldLines2To4 {
        fn next_visible_line(&self, line: LineNumber, dir: Direction) -> LineNumber {
            if (2..=4).contains(&line.get()) {
                match dir {
                    Direction::Forward => LineNumber::new(5),
                    Direction::Backward => LineNumber::new(1),
                }
            } else {
                line
            }
        }
        fn is_folded(&self, line: LineNumber) -> bool {
            (2..=4).contains(&line.get())
        }
    }

    #[test]
    fn operator_range_expands_to_include_fold() {
        // "L0\nL1\nL2\nL3\nL4\nL5\n"
        // Cursor on L1 (offset 3), delete down (dj) targets L2 which is folded.
        // Without fold awareness: range = L1..L2 (3..8)
        // With fold awareness: range should expand to include L2-L4.
        let text = "L0\nL1\nL2\nL3\nL4\nL5\n";
        let opts = VimOptions::default();
        let fold = FoldLines2To4;

        let result = dispatch_operator_with_motion(&OperatorMotionInput {
            operator: Operator::Yank,
            motion: Motion::Down,
            count: 1,
            register: None,
            text,
            cursor: Offset::new(3), // 'L' of L1
            search: None,
            last_find: None,
            shiftwidth: 4,
            textwidth: 80,
            options: &opts,
            force_type: None,
            custom_operators: None,
            viewport: None,
            sticky_column: None,
            fold_provider: Some(&fold),
        });

        // The yank should produce a SetRegister effect with text that
        // includes lines L1 through L4 (since L2-L4 are folded together).
        let has_register = result.effects.iter().any(|e| {
            if let crate::effects::Effect::SetRegister { text: yanked, .. } = e {
                // Should include L1, L2, L3, L4
                yanked.contains("L1") && yanked.contains("L4")
            } else {
                false
            }
        });
        assert!(
            has_register,
            "Yank should include fold-expanded range (L1 through L4)"
        );
    }
}

// ── Ctrl-V force operator block path tests ─────────────────────────

mod ctrl_v_force_block {
    use crate::commands::operators::OperatorMotionInput;
    use crate::dispatch::operator::dispatch_operator_with_motion;
    use crate::grammar::types::{Motion, Operator};
    use crate::primitives::{MotionType, Offset, VimOptions};

    /// `d<C-v>j` on "hello\nworld" with cursor on 'l' (offset 2) should delete
    /// a block rectangle: column 2 on both lines, producing "helo\nwold".
    ///
    /// Without the BlockWise intercept, this would fall through to the charwise
    /// delete path and delete "llo\nwo" (a linear range), which is wrong.
    #[test]
    fn ctrl_v_force_delete_produces_block_delete_effects() {
        let text = "hello\nworld";
        let opts = VimOptions::default();

        let result = dispatch_operator_with_motion(&OperatorMotionInput {
            operator: Operator::Delete,
            motion: Motion::Down,
            count: 1,
            register: None,
            text,
            cursor: Offset::new(2), // 'l' of "hello"
            search: None,
            last_find: None,
            shiftwidth: 4,
            textwidth: 80,
            options: &opts,
            force_type: Some(MotionType::BlockWise),
            custom_operators: None,
            viewport: None,
            sticky_column: None,
            fold_provider: None,
        });

        // Block delete should produce Delete effects (one per line).
        let delete_count = result
            .effects
            .iter()
            .filter(|e| matches!(e, crate::effects::Effect::Delete { .. }))
            .count();
        assert!(
            delete_count >= 2,
            "d<C-v>j should produce at least 2 Delete effects (one per line), got {delete_count}"
        );

        // Block operations should produce ClearSelection (from block visual exit).
        let has_clear = result
            .effects
            .iter()
            .any(|e| matches!(e, crate::effects::Effect::ClearSelection));
        assert!(
            has_clear,
            "d<C-v>j block path should produce ClearSelection"
        );
    }

    /// `y<C-v>j` should yank a block region and set the register with
    /// MotionType::BlockWise.
    #[test]
    fn ctrl_v_force_yank_produces_block_register() {
        let text = "hello\nworld";
        let opts = VimOptions::default();

        let result = dispatch_operator_with_motion(&OperatorMotionInput {
            operator: Operator::Yank,
            motion: Motion::Down,
            count: 1,
            register: None,
            text,
            cursor: Offset::new(2), // 'l' of "hello"
            search: None,
            last_find: None,
            shiftwidth: 4,
            textwidth: 80,
            options: &opts,
            force_type: Some(MotionType::BlockWise),
            custom_operators: None,
            viewport: None,
            sticky_column: None,
            fold_provider: None,
        });

        // Block yank should produce SetRegister effects.
        let has_register = result
            .effects
            .iter()
            .any(|e| matches!(e, crate::effects::Effect::SetRegister { .. }));
        assert!(
            has_register,
            "y<C-v>j should produce SetRegister effect for block yank"
        );
    }

    /// Without force_type, `dj` should NOT produce block effects — it should
    /// produce a single linewise delete, verifying the BlockWise intercept
    /// only fires for Ctrl-V forced motions.
    #[test]
    fn non_forced_dj_does_not_produce_block_effects() {
        let text = "hello\nworld";
        let opts = VimOptions::default();

        let result = dispatch_operator_with_motion(&OperatorMotionInput {
            operator: Operator::Delete,
            motion: Motion::Down,
            count: 1,
            register: None,
            text,
            cursor: Offset::new(2), // 'l' of "hello"
            search: None,
            last_find: None,
            shiftwidth: 4,
            textwidth: 80,
            options: &opts,
            force_type: None, // No force — natural linewise
            custom_operators: None,
            viewport: None,
            sticky_column: None,
            fold_provider: None,
        });

        // Without force, dj is linewise: should NOT produce ClearSelection.
        let has_clear = result
            .effects
            .iter()
            .any(|e| matches!(e, crate::effects::Effect::ClearSelection));
        assert!(
            !has_clear,
            "dj (no force) must NOT produce ClearSelection (not block path)"
        );
    }
}
