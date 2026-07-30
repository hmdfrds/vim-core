#![allow(non_snake_case)]
//! Fidelity tests for vim-core.
//!
//! These tests compare vim-core output against Neovim oracle.
//!
//! # Test Suites
//!
//! - basics: Basic navigation and mode changes
//! - motions: Character, line, word, document, find, scroll motions
//! - operators: Delete, change, yank, etc.
//! - textobjects: Word, sentence, paragraph, bracket text objects
//! - dotrepeat: Dot command (.) for repeating changes
//! - undo: Undo (u) and redo (Ctrl-R)
//! - macros: Macro recording (q) and playback (@)
//! - insert: Insert mode entry, text input, and exit
//! - visual: Visual mode (v, V, Ctrl-V) and operations

mod fidelity {
    use vim_test::neovim_test;

    // Core tests
    include!("fidelity/basics.rs");
    include!("fidelity/motions.rs");
    include!("fidelity/operators.rs");
    include!("fidelity/textobjects.rs");

    // New test suites
    include!("fidelity/dotrepeat.rs");
    include!("fidelity/undo.rs");
    include!("fidelity/macros.rs");
    include!("fidelity/insert.rs");
    include!("fidelity/visual.rs");

    // Registers, marks, jump list
    include!("fidelity/registers.rs");
    include!("fidelity/marks.rs");
    include!("fidelity/jumplist.rs");
    include!("fidelity/number.rs");
    include!("fidelity/bracket_motions.rs");
    include!("fidelity/replace_mode.rs");
    include!("fidelity/search.rs");

    // Extended feature tests
    include!("fidelity/put_commands.rs");
    include!("fidelity/autoindent.rs");
    include!("fidelity/section_motions.rs");
    include!("fidelity/misc_commands.rs");
    include!("fidelity/visual_increment.rs");

    // Scenario tests: realistic multi-step workflows
    include!("fidelity/scenarios/editing_code.rs");
    include!("fidelity/scenarios/refactoring.rs");
    include!("fidelity/scenarios/navigation_edit.rs");
    include!("fidelity/scenarios/prose_editing.rs");
    include!("fidelity/scenarios/bug_fixing.rs");
    include!("fidelity/scenarios/data_structures.rs");
    include!("fidelity/scenarios/multiline_ops.rs");
    include!("fidelity/scenarios/search_replace.rs");
    include!("fidelity/scenarios/copy_paste.rs");
    include!("fidelity/scenarios/error_recovery.rs");
    include!("fidelity/scenarios/language_patterns.rs");
    include!("fidelity/scenarios/complex_chains.rs");

    // Expanded scenario tests: additional categories
    include!("fidelity/scenarios/insert_workflows.rs");
    include!("fidelity/scenarios/ex_commands.rs");
    include!("fidelity/scenarios/counts_workflows.rs");
    include!("fidelity/scenarios/indent_format.rs");
    include!("fidelity/scenarios/case_operations.rs");
    include!("fidelity/scenarios/join_split.rs");
    include!("fidelity/scenarios/paragraph_sentence.rs");
    include!("fidelity/scenarios/unicode_workflows.rs");
    include!("fidelity/scenarios/find_till_repeat.rs");
    include!("fidelity/scenarios/visual_advanced.rs");
    include!("fidelity/scenarios/mark_advanced.rs");
    include!("fidelity/scenarios/register_advanced.rs");
    include!("fidelity/scenarios/boundary_edge_cases.rs");

    // Comprehensive edge case and coverage tests
    include!("fidelity/scenarios/operator_motion_matrix.rs");
    include!("fidelity/scenarios/text_object_edge_cases.rs");
    include!("fidelity/scenarios/empty_single_char_stress.rs");
    include!("fidelity/scenarios/command_line_editing.rs");
    include!("fidelity/scenarios/tab_whitespace_handling.rs");
    include!("fidelity/scenarios/operator_cancel.rs");
    include!("fidelity/scenarios/paste_edge_cases.rs");
    include!("fidelity/scenarios/word_boundary_edge.rs");
    include!("fidelity/scenarios/count_edge_cases.rs");
    include!("fidelity/scenarios/repeat_comprehensive.rs");
    include!("fidelity/scenarios/scroll_viewport.rs");
    include!("fidelity/scenarios/cursor_position_tests.rs");

    // Failure hunting: targeted edge cases to find bugs
    include!("fidelity/scenarios/failure_hunting.rs");

    // Rust development workflow tests
    include!("fidelity/scenarios/rust_development.rs");

    // Web development workflow tests
    include!("fidelity/scenarios/web_development.rs");

    // Power-user tricks: obscure commands most users don't know
    include!("fidelity/scenarios/power_user_tricks.rs");

    // Unicode stress tests: multi-byte boundaries, combining chars, RTL, astral plane
    include!("fidelity/scenarios/unicode_stress.rs");

    // Adversarial tests: fuzzer-style inputs designed to crash/corrupt
    include!("fidelity/scenarios/adversarial.rs");

    // Exhaustive text object tests: realistic code snippets
    include!("fidelity/scenarios/textobject_exhaustive.rs");

    // Beginner mistakes: realistic novice confusion and recovery
    include!("fidelity/scenarios/beginner_mistakes.rs");

    // Sysadmin workflows: config files, logs, scripts, YAML, Dockerfiles
    include!("fidelity/scenarios/sysadmin_workflows.rs");

    // Deep undo/redo: undo branches, counted undo/redo, undo after macros/visual/substitute, U
    include!("fidelity/scenarios/undo_deep.rs");

    // Regression tests: date-stamped, one test per bug fixed
    include!("fidelity/scenarios/regression.rs");

    // Long editing sessions: 20-50+ keystroke realistic sustained workflows
    include!("fidelity/scenarios/long_editing_sessions.rs");

    // Large-document tests: generated multi-line documents
    include!("fidelity/large_document.rs");
}
