// Scenario fidelity tests: Bug Fixing
//
// Multi-step workflows for common bug-fixing patterns.

// ═══════════════════════════════════════════════════════════════════════════════
// OFF-BY-ONE AND OPERATOR FIXES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, fix_lt_to_le, "if x < 10", cursor(0, 5), "r<lt>a=<Esc>");
neovim_test!(scenarios, fix_gt_to_ge, "if x > 0", cursor(0, 5), "r>a=<Esc>");
neovim_test!(scenarios, fix_eq_to_ne, "if x == 0", cursor(0, 5), "cl!<Esc>");
neovim_test!(scenarios, fix_and_to_or, "if a && b", cursor(0, 5), "cl|<Esc>l.");
neovim_test!(scenarios, fix_plus_to_minus, "x + 1", cursor(0, 2), "r-");
neovim_test!(scenarios, fix_index_off_by_one, "arr[i + 1]", cursor(0, 8), "r-");

// ═══════════════════════════════════════════════════════════════════════════════
// REMOVE DEBUG CODE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, remove_debug_print, "code\nprintln!(\"debug\")\nmore", cursor(1, 0), "dd");
neovim_test!(scenarios, remove_console_log, "logic\nconsole.log(x)\nrest", cursor(1, 0), "dd");
neovim_test!(scenarios, remove_dbg_macro, "let x = dbg!(expr);", cursor(0, 8), "ciwexpr<Esc>f!d2l");
neovim_test!(scenarios, comment_debug_line, "  debug_print(x);", "I// <Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// SYNTAX ERROR FIXES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, add_missing_semicolon, "let x = 5", "A;<Esc>");
neovim_test!(scenarios, add_missing_colon, "fn foo() {}", cursor(0, 9), "i -> i32 <Esc>");
neovim_test!(scenarios, fix_missing_brace, "if true {\n}", cursor(0, 0), "");
neovim_test!(scenarios, add_closing_paren, "call(arg", "A)<Esc>");
neovim_test!(scenarios, fix_missing_comma, "let a = [1 2 3];", cursor(0, 11), "i,<Esc>li,<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// GUARD CLAUSES AND ERROR HANDLING
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, add_null_check, "process(data);", "Oif data.is_none() { return; }<Esc>");
neovim_test!(scenarios, add_return_guard, "fn foo(x: i32) {}", cursor(0, 16), "Oif x < 0 { return; }<Esc>");
neovim_test!(scenarios, wrap_in_try, "risky_call();", "ITry { <Esc>A }<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// WRONG NAME FIXES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, fix_wrong_func_name, "result.len()", cursor(0, 7), "ciwsize<Esc>");
neovim_test!(scenarios, fix_wrong_field, "obj.naem", cursor(0, 4), "ciwname<Esc>");
neovim_test!(scenarios, fix_wrong_import, "use std::io::Read;", cursor(0, 14), "ciwWrite<Esc>");
neovim_test!(scenarios, fix_wrong_method, "vec.push(x);", cursor(0, 4), "ciwextend<Esc>");
neovim_test!(scenarios, fix_capitalization, "let MyVar = 5;", cursor(0, 4), "ciwmy_var<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// UNDO WRONG FIXES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, undo_bad_delete, "important code", "ddp");
neovim_test!(scenarios, undo_bad_change, "correct code", "cwwrong<Esc>u");

// ═══════════════════════════════════════════════════════════════════════════════
// TYPE MISMATCH FIXES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, fix_i32_to_i64, "let x: i32 = 0;", cursor(0, 7), "ciwi64<Esc>");
neovim_test!(scenarios, fix_string_to_str, "fn foo(s: String)", cursor(0, 10), "ciw&str<Esc>");
neovim_test!(scenarios, fix_vec_to_slice, "data: Vec<i32>", cursor(0, 6), "ciw&[i32]<Esc>");
neovim_test!(scenarios, fix_bool_to_option, "valid: bool", cursor(0, 7), "ciwOption<bool><Esc>");
neovim_test!(scenarios, add_result_wrap, "fn parse() -> Data", cursor(0, 14), "C-> Result<Data, Error><Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// WRONG RETURN / VALUE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, fix_return_value, "    return 0;", "f0r1");
neovim_test!(scenarios, fix_return_expr, "    return old_val;", cursor(0, 11), "ciwnew_val<Esc>");
neovim_test!(scenarios, fix_true_to_false, "    return true;", "ftr5l");
neovim_test!(scenarios, fix_none_to_some, "    return None;", cursor(0, 11), "ciwSome(val)<Esc>");
neovim_test!(scenarios, fix_empty_to_default, "    return \"\";", cursor(0, 11), "ci\"default<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// MISSING CODE FIXES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, add_import, "fn main() {}", "Ouse std::io;<Esc>");
neovim_test!(scenarios, add_use_statement, "fn main() {}", "Oimport math<Esc>");
neovim_test!(scenarios, add_error_prop, "    let x = parse();", "$i?<Esc>");
neovim_test!(scenarios, add_mut, "let x = vec![];", "fxilet <Esc>");
neovim_test!(scenarios, bugfix_add_pub, "fn helper() {}", "Ipub <Esc>");
neovim_test!(scenarios, bugfix_add_async, "fn fetch() {}", "Iasync <Esc>");
neovim_test!(scenarios, fix_ownership_clone, "process(data);", "f(a.clone()<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// MULTI-FIX CHAINS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, fix_operator_and_value, "if x < 0", "f<lt>r>f0r1");
neovim_test!(scenarios, fix_name_and_semi, "let result = process()", "A;<Esc>^wciwoutput<Esc>");
neovim_test!(scenarios, comment_and_replace, "    old_call();", "I// <Esc>onew_call();<Esc>");

