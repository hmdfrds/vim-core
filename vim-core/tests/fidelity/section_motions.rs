// Section and method motion fidelity tests.
//
// [[, ]], [m, ]m, [M, ]M, [], ][

// ═══════════════════════════════════════════════════════════════════════════════
// [[ — PREVIOUS SECTION ('{' IN COLUMN 0)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(section_motions, prev_section_basic, "{\n  body\n}\n{\n  body2\n}", cursor(4, 0), "[[");
neovim_test!(section_motions, prev_section_from_middle, "{\n  body\n}\n{\n  body2\n}", cursor(4, 2), "[[");
neovim_test!(section_motions, prev_section_at_start, "{\n  body\n}", "[[");
neovim_test!(section_motions, prev_section_multiple, "{\na\n}\n{\nb\n}\n{\nc\n}", cursor(7, 0), "[[");
neovim_test!(section_motions, prev_section_2_count, "{\na\n}\n{\nb\n}\n{\nc\n}", cursor(7, 0), "2[[");
neovim_test!(section_motions, prev_section_no_match, "no braces here", "[[");
neovim_test!(section_motions, prev_section_indented_brace, "  {\n  body\n  }", cursor(1, 0), "[[");

// ═══════════════════════════════════════════════════════════════════════════════
// ]] — NEXT SECTION ('{' IN COLUMN 0)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(section_motions, next_section_basic, "{\n  body\n}\n{\n  body2\n}", "]]");
neovim_test!(section_motions, next_section_from_middle, "{\n  body\n}\n{\n  body2\n}", cursor(1, 0), "]]");
neovim_test!(section_motions, next_section_at_end, "{\n  body\n}", cursor(2, 0), "]]");
neovim_test!(section_motions, next_section_multiple, "{\na\n}\n{\nb\n}\n{\nc\n}", "]]");
neovim_test!(section_motions, next_section_2_count, "{\na\n}\n{\nb\n}\n{\nc\n}", "2]]");
neovim_test!(section_motions, next_section_no_match, "no braces here", "]]");

// ═══════════════════════════════════════════════════════════════════════════════
// [] — PREVIOUS SECTION ('}' IN COLUMN 0)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(section_motions, prev_section_close_basic, "{\n  body\n}\n{\n  body2\n}", cursor(4, 0), "[]");
neovim_test!(section_motions, prev_section_close_multiple, "{\na\n}\n{\nb\n}\n{\nc\n}", cursor(7, 0), "[]");
neovim_test!(section_motions, prev_section_close_count, "{\na\n}\n{\nb\n}\n{\nc\n}", cursor(7, 0), "2[]");

// ═══════════════════════════════════════════════════════════════════════════════
// ][ — NEXT SECTION ('}' IN COLUMN 0)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(section_motions, next_section_close_basic, "{\n  body\n}\n{\n  body2\n}", "][");
neovim_test!(section_motions, next_section_close_multiple, "{\na\n}\n{\nb\n}\n{\nc\n}", "][");
neovim_test!(section_motions, next_section_close_count, "{\na\n}\n{\nb\n}\n{\nc\n}", "2][");

// ═══════════════════════════════════════════════════════════════════════════════
// [m — PREVIOUS METHOD START
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(section_motions, prev_method_basic, "fn foo() {\n  body\n}\nfn bar() {\n  body2\n}", cursor(4, 0), "[m");
neovim_test!(section_motions, prev_method_nested, "fn outer() {\n  fn inner() {\n    body\n  }\n}", cursor(2, 0), "[m");
neovim_test!(section_motions, prev_method_count, "fn a() {\n}\nfn b() {\n}\nfn c() {\n}", cursor(5, 0), "2[m");

// ═══════════════════════════════════════════════════════════════════════════════
// ]m — NEXT METHOD START
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(section_motions, next_method_basic, "fn foo() {\n  body\n}\nfn bar() {\n  body2\n}", "]m");
neovim_test!(section_motions, next_method_from_middle, "fn foo() {\n  body\n}\nfn bar() {\n  body2\n}", cursor(1, 0), "]m");
neovim_test!(section_motions, next_method_count, "fn a() {\n}\nfn b() {\n}\nfn c() {\n}", "2]m");

// ═══════════════════════════════════════════════════════════════════════════════
// [M — PREVIOUS METHOD END
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(section_motions, prev_method_end_basic, "fn foo() {\n  body\n}\nfn bar() {\n  body2\n}", cursor(4, 0), "[M");
neovim_test!(section_motions, prev_method_end_count, "fn a() {\n}\nfn b() {\n}\nfn c() {\n}", cursor(5, 0), "2[M");

// ═══════════════════════════════════════════════════════════════════════════════
// ]M — NEXT METHOD END
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(section_motions, next_method_end_basic, "fn foo() {\n  body\n}\nfn bar() {\n  body2\n}", "]M");
neovim_test!(section_motions, next_method_end_count, "fn a() {\n}\nfn b() {\n}\nfn c() {\n}", "2]M");

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION MOTIONS WITH OPERATORS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(section_motions, delete_to_next_section, "{\na\n}\n{\nb\n}", "d]]");
neovim_test!(section_motions, yank_to_prev_section, "{\na\n}\n{\nb\n}", cursor(4, 0), "y[[p");
neovim_test!(section_motions, change_to_next_section, "{\na\n}\n{\nb\n}", "c]]X<Esc>");
neovim_test!(section_motions, delete_to_next_close, "{\na\n}\n{\nb\n}", "d][");
neovim_test!(section_motions, visual_section, "{\na\n}\n{\nb\n}", "v]]d");

// ═══════════════════════════════════════════════════════════════════════════════
// [m / ]m / [M / ]M — DEPTH-ZERO BRACE MATCHING (migrated from neovim_fidelity)
// ═══════════════════════════════════════════════════════════════════════════════

// ]m finds first depth-zero `{` forward from cursor
neovim_test!(section_motions, next_method_start_depth_zero, "fn a() {\n    fn b() {\n        x\n    }\n}", "]m");

// 2]m skips to second depth-zero `{`
neovim_test!(section_motions, next_method_start_count_two, "fn a() {\n}\nfn b() {\n}", "2]m");

// [m scans backward for previous depth-zero `{`
neovim_test!(section_motions, prev_method_start_depth_zero, "fn a() {\n    x\n}", cursor(1, 4), "[m");

// ]M finds next depth-zero `}` forward
neovim_test!(section_motions, next_method_end_depth_zero, "{\n  {\n    x\n  }\n}", cursor(2, 4), "]M");

// [M finds previous depth-zero `}` backward
neovim_test!(section_motions, prev_method_end_depth_zero, "fn a() {\n    x\n}\nafter", cursor(3, 0), "[M");
