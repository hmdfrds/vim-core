// Large-document fidelity tests for vim-core.
//
// These tests verify that vim-core handles larger documents correctly
// by exercising motions, jumps, searches, and ex commands on
// programmatically-generated multi-line buffers.
//
// Document sizes are kept practical (50-200 lines) to avoid slow
// golden file generation while still exercising boundary conditions
// that single-line tests miss.

// ═══════════════════════════════════════════════════════════════════════════════
// DOCUMENT GENERATORS
// ═══════════════════════════════════════════════════════════════════════════════

/// Generate a code-like document with the given number of lines.
///
/// The document has a repeating pattern of function-like blocks:
/// - Line 0 (mod 10): function header
/// - Lines 1-3: variable assignments
/// - Line 4: if condition
/// - Line 5: println statement
/// - Line 6: closing brace
/// - Line 7: blank line
/// - Line 8: comment
/// - Line 9: closing brace
fn code_document(lines: usize) -> String {
    let mut doc = String::new();
    for i in 0..lines {
        if i > 0 {
            doc.push('\n');
        }
        match i % 10 {
            0 => doc.push_str(&format!("fn function_{i}() {{")),
            1 => doc.push_str(&format!("    let x_{i} = {};", i * 42)),
            2 => doc.push_str(&format!("    let y_{i} = {};", i * 17)),
            3 => doc.push_str(&format!("    let z_{i} = {};", i * 7)),
            4 => doc.push_str("    if x > 0 {"),
            5 => doc.push_str("        println!(\"hello\");"),
            6 => doc.push_str("    }"),
            7 => {} // blank line (just the newline above)
            8 => doc.push_str("    // comment line"),
            9 => doc.push('}'),
            _ => unreachable!(),
        }
    }
    doc
}

/// Generate a numbered-line document for precise line-jump testing.
///
/// Each line is "Line NNN: content text here" so we can verify exact
/// cursor positions after G, gg, and {count}G commands.
fn numbered_document(lines: usize) -> String {
    let mut doc = String::new();
    for i in 1..=lines {
        if i > 1 {
            doc.push('\n');
        }
        doc.push_str(&format!("Line {i:03}: content text here"));
    }
    doc
}

/// Generate a document with a searchable pattern at known positions.
///
/// The word "MARKER" appears on lines at positions determined by `interval`.
/// Other lines contain filler text. Used for search-wrapping tests.
fn search_document(lines: usize, interval: usize) -> String {
    let mut doc = String::new();
    for i in 0..lines {
        if i > 0 {
            doc.push('\n');
        }
        if i % interval == 0 {
            doc.push_str(&format!("MARKER on line {i}"));
        } else {
            doc.push_str(&format!("filler text on line {i}"));
        }
    }
    doc
}

/// Generate a document with `let` declarations for substitution testing.
fn let_document(lines: usize) -> String {
    let mut doc = String::new();
    for i in 0..lines {
        if i > 0 {
            doc.push('\n');
        }
        if i % 3 == 0 {
            doc.push_str(&format!("let var_{i} = {i};"));
        } else if i % 3 == 1 {
            doc.push_str(&format!("const val_{i} = {i};"));
        } else {
            doc.push_str(&format!("// comment {i}"));
        }
    }
    doc
}

// ═══════════════════════════════════════════════════════════════════════════════
// 1. JUMP-TO-LINE on 200-line document
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn large_go_to_line_42() {
    let text = numbered_document(200);
    vim_test::fidelity::run_neovim_test(
        "large_go_to_line_42",
        "large_document",
        &text,
        (0, 0),
        "42G",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_go_to_first_line() {
    let text = numbered_document(200);
    vim_test::fidelity::run_neovim_test(
        "large_go_to_first_line",
        "large_document",
        &text,
        (100, 0),
        "gg",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_go_to_last_line() {
    let text = numbered_document(200);
    vim_test::fidelity::run_neovim_test(
        "large_go_to_last_line",
        "large_document",
        &text,
        (0, 0),
        "G",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_go_to_line_1() {
    let text = numbered_document(200);
    vim_test::fidelity::run_neovim_test(
        "large_go_to_line_1",
        "large_document",
        &text,
        (100, 0),
        "1G",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_go_to_line_200() {
    let text = numbered_document(200);
    vim_test::fidelity::run_neovim_test(
        "large_go_to_line_200",
        "large_document",
        &text,
        (0, 0),
        "200G",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_go_to_middle() {
    let text = numbered_document(200);
    vim_test::fidelity::run_neovim_test(
        "large_go_to_middle",
        "large_document",
        &text,
        (0, 0),
        "100G",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_gg_then_g_roundtrip() {
    let text = numbered_document(200);
    vim_test::fidelity::run_neovim_test(
        "large_gg_then_g_roundtrip",
        "large_document",
        &text,
        (100, 0),
        "ggG",
        env!("CARGO_MANIFEST_DIR"),
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. LARGE MOTION SEQUENCES on 100-line document
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn large_50j() {
    let text = code_document(100);
    vim_test::fidelity::run_neovim_test(
        "large_50j",
        "large_document",
        &text,
        (0, 0),
        "50j",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_gg_then_g_code() {
    let text = code_document(100);
    vim_test::fidelity::run_neovim_test(
        "large_gg_then_g_code",
        "large_document",
        &text,
        (50, 0),
        "ggG",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_motion_j_k_sequence() {
    let text = code_document(100);
    vim_test::fidelity::run_neovim_test(
        "large_motion_j_k_sequence",
        "large_document",
        &text,
        (0, 0),
        "30j10k5j",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_word_motion_across_lines() {
    let text = code_document(50);
    vim_test::fidelity::run_neovim_test(
        "large_word_motion_across_lines",
        "large_document",
        &text,
        (0, 0),
        "20w",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_word_backward_motion() {
    let text = code_document(50);
    vim_test::fidelity::run_neovim_test(
        "large_word_backward_motion",
        "large_document",
        &text,
        (49, 0),
        "20b",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_end_word_motion() {
    let text = code_document(50);
    vim_test::fidelity::run_neovim_test(
        "large_end_word_motion",
        "large_document",
        &text,
        (0, 0),
        "15e",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_paragraph_forward() {
    let text = code_document(100);
    vim_test::fidelity::run_neovim_test(
        "large_paragraph_forward",
        "large_document",
        &text,
        (0, 0),
        "5}",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_paragraph_backward() {
    let text = code_document(100);
    vim_test::fidelity::run_neovim_test(
        "large_paragraph_backward",
        "large_document",
        &text,
        (99, 0),
        "5{",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_count_exceeds_lines() {
    let text = code_document(50);
    vim_test::fidelity::run_neovim_test(
        "large_count_exceeds_lines",
        "large_document",
        &text,
        (0, 0),
        "999j",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_percent_motion() {
    // Go to 50% of a 100-line document
    let text = code_document(100);
    vim_test::fidelity::run_neovim_test(
        "large_percent_motion",
        "large_document",
        &text,
        (0, 0),
        "50%",
        env!("CARGO_MANIFEST_DIR"),
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. SEARCH WRAPPING on 50-line document
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn large_search_forward() {
    let text = search_document(50, 10);
    vim_test::fidelity::run_neovim_test(
        "large_search_forward",
        "large_document",
        &text,
        (0, 0),
        "/MARKER<CR>",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_search_forward_n() {
    let text = search_document(50, 10);
    vim_test::fidelity::run_neovim_test(
        "large_search_forward_n",
        "large_document",
        &text,
        (0, 0),
        "/MARKER<CR>n",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_search_forward_nn() {
    let text = search_document(50, 10);
    vim_test::fidelity::run_neovim_test(
        "large_search_forward_nn",
        "large_document",
        &text,
        (0, 0),
        "/MARKER<CR>nn",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_search_wrap_around() {
    // Start near the bottom, search should wrap to the top
    let text = search_document(50, 10);
    vim_test::fidelity::run_neovim_test(
        "large_search_wrap_around",
        "large_document",
        &text,
        (45, 0),
        "/MARKER<CR>n",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_search_backward() {
    let text = search_document(50, 10);
    vim_test::fidelity::run_neovim_test(
        "large_search_backward",
        "large_document",
        &text,
        (49, 0),
        "?MARKER<CR>",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_search_backward_wrap() {
    // Start near the top, backward search should wrap to the bottom
    let text = search_document(50, 10);
    vim_test::fidelity::run_neovim_test(
        "large_search_backward_wrap",
        "large_document",
        &text,
        (5, 0),
        "?MARKER<CR>n",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_star_search() {
    let text = search_document(50, 10);
    vim_test::fidelity::run_neovim_test(
        "large_star_search",
        "large_document",
        &text,
        (0, 0),
        "*",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_star_search_n() {
    let text = search_document(50, 10);
    vim_test::fidelity::run_neovim_test(
        "large_star_search_n",
        "large_document",
        &text,
        (0, 0),
        "*n",
        env!("CARGO_MANIFEST_DIR"),
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. RANGE COMMANDS on 50-line document
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn large_ex_delete_range() {
    let text = let_document(50);
    vim_test::fidelity::run_neovim_test(
        "large_ex_delete_range",
        "large_document",
        &text,
        (0, 0),
        ":10,20d<CR>",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_ex_delete_single_line() {
    let text = let_document(50);
    vim_test::fidelity::run_neovim_test(
        "large_ex_delete_single_line",
        "large_document",
        &text,
        (0, 0),
        ":25d<CR>",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_ex_delete_last_lines() {
    let text = let_document(50);
    vim_test::fidelity::run_neovim_test(
        "large_ex_delete_last_lines",
        "large_document",
        &text,
        (0, 0),
        ":45,50d<CR>",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_ex_sub_global() {
    let text = let_document(50);
    vim_test::fidelity::run_neovim_test(
        "large_ex_sub_global",
        "large_document",
        &text,
        (0, 0),
        ":%s/let/const/g<CR>",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_ex_sub_range() {
    let text = let_document(50);
    vim_test::fidelity::run_neovim_test(
        "large_ex_sub_range",
        "large_document",
        &text,
        (0, 0),
        ":1,10s/let/const/g<CR>",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_ex_move_line() {
    let text = numbered_document(50);
    vim_test::fidelity::run_neovim_test(
        "large_ex_move_line",
        "large_document",
        &text,
        (0, 0),
        ":1m25<CR>",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_ex_copy_range() {
    let text = numbered_document(50);
    vim_test::fidelity::run_neovim_test(
        "large_ex_copy_range",
        "large_document",
        &text,
        (0, 0),
        ":1,5co40<CR>",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_ex_global_delete() {
    let text = let_document(50);
    vim_test::fidelity::run_neovim_test(
        "large_ex_global_delete",
        "large_document",
        &text,
        (0, 0),
        ":g/comment/d<CR>",
        env!("CARGO_MANIFEST_DIR"),
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5. OPERATOR SEQUENCES on larger documents
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn large_delete_lines() {
    let text = code_document(50);
    vim_test::fidelity::run_neovim_test(
        "large_delete_lines",
        "large_document",
        &text,
        (10, 0),
        "5dd",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_yank_paste() {
    let text = numbered_document(50);
    vim_test::fidelity::run_neovim_test(
        "large_yank_paste",
        "large_document",
        &text,
        (0, 0),
        "3yyGp",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_delete_to_end() {
    let text = code_document(50);
    vim_test::fidelity::run_neovim_test(
        "large_delete_to_end",
        "large_document",
        &text,
        (25, 0),
        "dG",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_delete_to_beginning() {
    let text = code_document(50);
    vim_test::fidelity::run_neovim_test(
        "large_delete_to_beginning",
        "large_document",
        &text,
        (25, 0),
        "dgg",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_change_lines() {
    let text = code_document(50);
    vim_test::fidelity::run_neovim_test(
        "large_change_lines",
        "large_document",
        &text,
        (20, 0),
        "3ccnew content<Esc>",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_visual_line_delete() {
    let text = numbered_document(50);
    vim_test::fidelity::run_neovim_test(
        "large_visual_line_delete",
        "large_document",
        &text,
        (10, 0),
        "V4jd",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_indent_range() {
    let text = code_document(50);
    vim_test::fidelity::run_neovim_test(
        "large_indent_range",
        "large_document",
        &text,
        (5, 0),
        "5>>",
        env!("CARGO_MANIFEST_DIR"),
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 6. COMBINED WORKFLOWS on larger documents
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn large_search_delete_to_match() {
    let text = search_document(50, 10);
    vim_test::fidelity::run_neovim_test(
        "large_search_delete_to_match",
        "large_document",
        &text,
        (0, 0),
        "d/MARKER<CR>",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_jump_and_edit() {
    let text = numbered_document(100);
    vim_test::fidelity::run_neovim_test(
        "large_jump_and_edit",
        "large_document",
        &text,
        (0, 0),
        "50Gdd",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_multi_step_navigation() {
    let text = numbered_document(100);
    vim_test::fidelity::run_neovim_test(
        "large_multi_step_navigation",
        "large_document",
        &text,
        (0, 0),
        "20G$b",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_mark_and_jump() {
    let text = numbered_document(100);
    vim_test::fidelity::run_neovim_test(
        "large_mark_and_jump",
        "large_document",
        &text,
        (0, 0),
        "30Gma50G'a",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_yank_mark_range() {
    let text = numbered_document(50);
    vim_test::fidelity::run_neovim_test(
        "large_yank_mark_range",
        "large_document",
        &text,
        (0, 0),
        "10Gma20Gy'a",
        env!("CARGO_MANIFEST_DIR"),
    );
}

#[test]
fn large_dot_repeat_across_lines() {
    let text = numbered_document(50);
    vim_test::fidelity::run_neovim_test(
        "large_dot_repeat_across_lines",
        "large_document",
        &text,
        (0, 0),
        "ddj.j.",
        env!("CARGO_MANIFEST_DIR"),
    );
}
