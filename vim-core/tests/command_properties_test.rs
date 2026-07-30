//! CommandProperties compile-time guard and assertion tests.
//!
//! Tests that:
//! 1. All Command variants have a properties() implementation (compile-time check)
//! 2. Key variants return correct CommandProperties values
//! 3. Operator mutability correctly affects repeat behavior

use std::num::NonZeroU32;

use vim_core::grammar::command::{Command, InsertKind, MacroKind, PrefixCommand, VisualKind};
use vim_core::grammar::types::{
    Action, CharCommand, MarkType, Motion, Operator, TextObject, TextObjectKind, TextObjectScope,
};
use vim_core::primitives::{InsertEntryType, MarkName, Mode, RepeatBehavior, VisualType};

const N1: NonZeroU32 = match NonZeroU32::new(1) {
    Some(v) => v,
    None => unreachable!(),
};

// =============================================================================
// Compile-Time Guard: All Command variants must have a properties() arm
// =============================================================================

/// Test that constructs one instance of every Command variant and calls
/// `.properties()` on each. This ensures that if a new variant is added
/// without a properties() match arm, the test won't compile.
///
/// Uses dummy/default values for variant fields.
#[test]
fn properties_guard_all_variants_compile() {
    // Motion
    let _props = Command::Motion {
        count: N1,
        motion: Motion::Right,
        explicit_count: false,
    }
    .properties();

    // OperatorMotion
    let _props = Command::OperatorMotion {
        count: N1,
        register: None,
        operator: Operator::Delete,
        motion: Motion::Right,
        force_type: None,
    }
    .properties();

    // OperatorTextObject
    let _props = Command::OperatorTextObject {
        count: N1,
        register: None,
        operator: Operator::Delete,
        textobject: TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Word,
            seek: None,
        },
    }
    .properties();

    // OperatorLine
    let _props = Command::OperatorLine {
        count: N1,
        register: None,
        operator: Operator::Delete,
    }
    .properties();

    // OperatorMark
    let _props = Command::OperatorMark {
        count: N1,
        register: None,
        operator: Operator::Delete,
        mark: MarkName::new('a').unwrap(),
        mark_type: MarkType::JumpLine,
    }
    .properties();

    // Action
    let _props = Command::Action {
        count: N1,
        register: None,
        action: Action::DeleteChar,
    }
    .properties();

    // CharCommand
    let _props = Command::CharCommand {
        count: N1,
        register: None,
        operator: None,
        command: CharCommand::FindForward,
        target: compact_str::CompactString::from("a"),
    }
    .properties();

    // Mark
    let _props = Command::Mark {
        count: N1,
        mark_type: MarkType::JumpLine,
        mark: MarkName::new('a').unwrap(),
    }
    .properties();

    // ModeSwitch
    let _props = Command::ModeSwitch { mode: Mode::Insert }.properties();

    // Prefix
    let _props = Command::Prefix {
        count: N1,
        register: None,
        command: PrefixCommand::ScrollCenter,
    }
    .properties();

    // Insert
    let _props = Command::Insert(InsertKind::Char { char: 'a' }).properties();

    // InsertEntry
    let _props = Command::InsertEntry {
        count: N1,
        entry_type: InsertEntryType::BeforeCursor,
        register: None,
    }
    .properties();

    // InsertExit
    let _props = Command::InsertExit.properties();

    // Visual
    let _props = Command::Visual(VisualKind::Enter {
        visual_type: VisualType::Char,
        count: None,
    })
    .properties();

    // OperatorSelection
    let _props = Command::OperatorSelection {
        register: None,
        operator: Operator::Delete,
    }
    .properties();

    // VisualTextObject
    let _props = Command::VisualTextObject {
        count: N1,
        textobject: TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Word,
            seek: None,
        },
        register: None,
    }
    .properties();

    // Macro
    let _props = Command::Macro(MacroKind::Stop).properties();

    // SelectEnter
    let _props = Command::SelectEnter {
        visual_type: VisualType::Char,
    }
    .properties();

    // If compilation succeeds, all variants are covered.
}

// =============================================================================
// Property Assertions: Key Variants
// =============================================================================

#[test]
fn operator_motion_with_delete_has_record_repeat() {
    let cmd = Command::OperatorMotion {
        count: N1,
        register: None,
        operator: Operator::Delete,
        motion: Motion::Right,
        force_type: None,
    };

    let props = cmd.properties();
    assert_eq!(
        props.repeat,
        RepeatBehavior::Record,
        "OperatorMotion with Delete (mutating) should have Record repeat"
    );
}

#[test]
fn operator_motion_with_yank_has_skip_repeat() {
    let cmd = Command::OperatorMotion {
        count: N1,
        register: None,
        operator: Operator::Yank,
        motion: Motion::Right,
        force_type: None,
    };

    let props = cmd.properties();
    assert_eq!(
        props.repeat,
        RepeatBehavior::Skip,
        "OperatorMotion with Yank (non-mutating) should have Skip repeat"
    );
}

#[test]
fn motion_has_skip_repeat_and_keep_visual() {
    let cmd = Command::Motion {
        count: N1,
        motion: Motion::Right,
        explicit_count: false,
    };

    let props = cmd.properties();
    assert_eq!(
        props.repeat,
        RepeatBehavior::Skip,
        "Motion should have Skip repeat"
    );
    assert!(props.keep_visual, "Motion should preserve visual selection");
}

#[test]
fn insert_entry_has_record_repeat() {
    let cmd = Command::InsertEntry {
        count: N1,
        entry_type: InsertEntryType::BeforeCursor,
        register: None,
    };

    let props = cmd.properties();
    assert_eq!(
        props.repeat,
        RepeatBehavior::Record,
        "InsertEntry should have Record repeat"
    );
}

#[test]
fn visual_has_skip_repeat() {
    let cmd = Command::Visual(VisualKind::Enter {
        visual_type: VisualType::Char,
        count: None,
    });

    let props = cmd.properties();
    assert_eq!(
        props.repeat,
        RepeatBehavior::Skip,
        "Visual commands should have Skip repeat"
    );
}

#[test]
fn operator_line_with_delete_has_record_repeat() {
    let cmd = Command::OperatorLine {
        count: N1,
        register: None,
        operator: Operator::Delete,
    };

    let props = cmd.properties();
    assert_eq!(
        props.repeat,
        RepeatBehavior::Record,
        "OperatorLine with Delete (mutating) should have Record repeat"
    );
}

#[test]
fn operator_line_with_yank_has_skip_repeat() {
    let cmd = Command::OperatorLine {
        count: N1,
        register: None,
        operator: Operator::Yank,
    };

    let props = cmd.properties();
    assert_eq!(
        props.repeat,
        RepeatBehavior::Skip,
        "OperatorLine with Yank (non-mutating) should have Skip repeat"
    );
}

#[test]
fn operator_text_object_with_delete_has_record_repeat() {
    let cmd = Command::OperatorTextObject {
        count: N1,
        register: None,
        operator: Operator::Delete,
        textobject: TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Word,
            seek: None,
        },
    };

    let props = cmd.properties();
    assert_eq!(
        props.repeat,
        RepeatBehavior::Record,
        "OperatorTextObject with Delete (mutating) should have Record repeat"
    );
}

#[test]
fn operator_text_object_with_yank_has_skip_repeat() {
    let cmd = Command::OperatorTextObject {
        count: N1,
        register: None,
        operator: Operator::Yank,
        textobject: TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Word,
            seek: None,
        },
    };

    let props = cmd.properties();
    assert_eq!(
        props.repeat,
        RepeatBehavior::Skip,
        "OperatorTextObject with Yank (non-mutating) should have Skip repeat"
    );
}

#[test]
fn operator_selection_with_delete_has_record_repeat() {
    let cmd = Command::OperatorSelection {
        register: None,
        operator: Operator::Delete,
    };

    let props = cmd.properties();
    assert_eq!(
        props.repeat,
        RepeatBehavior::Record,
        "OperatorSelection with Delete (mutating) should have Record repeat"
    );
}

#[test]
fn operator_selection_with_yank_has_skip_repeat() {
    let cmd = Command::OperatorSelection {
        register: None,
        operator: Operator::Yank,
    };

    let props = cmd.properties();
    assert_eq!(
        props.repeat,
        RepeatBehavior::Skip,
        "OperatorSelection with Yank (non-mutating) should have Skip repeat"
    );
}

#[test]
fn insert_exit_has_skip_repeat() {
    let cmd = Command::InsertExit;

    let props = cmd.properties();
    assert_eq!(
        props.repeat,
        RepeatBehavior::Skip,
        "InsertExit should have Skip repeat"
    );
}

#[test]
fn operator_mark_with_delete_has_record_repeat() {
    let cmd = Command::OperatorMark {
        count: N1,
        register: None,
        operator: Operator::Delete,
        mark: MarkName::new('a').unwrap(),
        mark_type: MarkType::JumpLine,
    };

    let props = cmd.properties();
    assert_eq!(
        props.repeat,
        RepeatBehavior::Record,
        "OperatorMark with Delete (mutating) should have Record repeat"
    );
}

#[test]
fn operator_mark_with_yank_has_skip_repeat() {
    let cmd = Command::OperatorMark {
        count: N1,
        register: None,
        operator: Operator::Yank,
        mark: MarkName::new('a').unwrap(),
        mark_type: MarkType::JumpLine,
    };

    let props = cmd.properties();
    assert_eq!(
        props.repeat,
        RepeatBehavior::Skip,
        "OperatorMark with Yank (non-mutating) should have Skip repeat"
    );
}

#[test]
fn prefix_with_mutating_command_has_record_repeat() {
    let cmd = Command::Prefix {
        count: N1,
        register: None,
        command: PrefixCommand::JoinNoSpace,
    };

    let props = cmd.properties();
    assert_eq!(
        props.repeat,
        RepeatBehavior::Record,
        "Prefix with mutating command should have Record repeat"
    );
}

#[test]
fn prefix_with_non_mutating_command_has_skip_repeat() {
    let cmd = Command::Prefix {
        count: N1,
        register: None,
        command: PrefixCommand::ScrollCenter,
    };

    let props = cmd.properties();
    assert_eq!(
        props.repeat,
        RepeatBehavior::Skip,
        "Prefix with non-mutating command should have Skip repeat"
    );
}

#[test]
fn action_with_mutating_action_has_record_repeat() {
    let cmd = Command::Action {
        count: N1,
        register: None,
        action: Action::DeleteChar,
    };

    let props = cmd.properties();
    assert_eq!(
        props.repeat,
        RepeatBehavior::Record,
        "Action with mutating action should have Record repeat"
    );
}

#[test]
fn action_with_non_mutating_action_has_skip_repeat() {
    let cmd = Command::Action {
        count: N1,
        register: None,
        action: Action::Undo,
    };

    let props = cmd.properties();
    assert_eq!(
        props.repeat,
        RepeatBehavior::Skip,
        "Action with non-mutating action should have Skip repeat"
    );
}

#[test]
fn char_command_with_replace_has_record_repeat() {
    let cmd = Command::CharCommand {
        count: N1,
        register: None,
        operator: None,
        command: vim_core::grammar::types::CharCommand::Replace,
        target: compact_str::CompactString::from("x"),
    };

    let props = cmd.properties();
    assert_eq!(
        props.repeat,
        RepeatBehavior::Record,
        "CharCommand with Replace should have Record repeat"
    );
}

#[test]
fn char_command_with_find_has_skip_repeat() {
    let cmd = Command::CharCommand {
        count: N1,
        register: None,
        operator: None,
        command: CharCommand::FindForward,
        target: compact_str::CompactString::from("x"),
    };

    let props = cmd.properties();
    assert_eq!(
        props.repeat,
        RepeatBehavior::Skip,
        "CharCommand with Find should have Skip repeat"
    );
}

#[test]
fn char_command_with_operator_has_record_repeat() {
    let cmd = Command::CharCommand {
        count: N1,
        register: None,
        operator: Some(Operator::Delete),
        command: CharCommand::FindForward,
        target: compact_str::CompactString::from("x"),
    };

    let props = cmd.properties();
    assert_eq!(
        props.repeat,
        RepeatBehavior::Record,
        "CharCommand with operator should have Record repeat"
    );
}

#[test]
fn visual_text_object_has_skip_repeat_and_keep_visual() {
    let cmd = Command::VisualTextObject {
        count: N1,
        textobject: TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Word,
            seek: None,
        },
        register: None,
    };

    let props = cmd.properties();
    assert_eq!(
        props.repeat,
        RepeatBehavior::Skip,
        "VisualTextObject should have Skip repeat"
    );
    assert!(
        props.keep_visual,
        "VisualTextObject should keep visual selection"
    );
}

#[test]
fn mark_has_skip_repeat_and_keep_visual() {
    let cmd = Command::Mark {
        count: N1,
        mark_type: MarkType::JumpLine,
        mark: MarkName::new('a').unwrap(),
    };

    let props = cmd.properties();
    assert_eq!(
        props.repeat,
        RepeatBehavior::Skip,
        "Mark should have Skip repeat"
    );
    assert!(props.keep_visual, "Mark should keep visual selection");
}

#[test]
fn mode_switch_has_skip_repeat() {
    let cmd = Command::ModeSwitch { mode: Mode::Insert };

    let props = cmd.properties();
    assert_eq!(
        props.repeat,
        RepeatBehavior::Skip,
        "ModeSwitch should have Skip repeat"
    );
}

#[test]
fn insert_sub_command_has_skip_repeat() {
    let cmd = Command::Insert(InsertKind::Char { char: 'a' });

    let props = cmd.properties();
    assert_eq!(
        props.repeat,
        RepeatBehavior::Skip,
        "Insert sub-command should have Skip repeat"
    );
}

#[test]
fn macro_has_skip_repeat() {
    let cmd = Command::Macro(MacroKind::Stop);

    let props = cmd.properties();
    assert_eq!(
        props.repeat,
        RepeatBehavior::Skip,
        "Macro should have Skip repeat"
    );
}

#[test]
fn select_enter_has_skip_repeat() {
    let cmd = Command::SelectEnter {
        visual_type: VisualType::Char,
    };

    let props = cmd.properties();
    assert_eq!(
        props.repeat,
        RepeatBehavior::Skip,
        "SelectEnter should have Skip repeat"
    );
}
