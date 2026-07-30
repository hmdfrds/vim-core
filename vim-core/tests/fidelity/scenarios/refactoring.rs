// Scenario fidelity tests: Refactoring
//
// Multi-step workflows for refactoring code.

// ═══════════════════════════════════════════════════════════════════════════════
// RENAME IDENTIFIERS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, rename_with_n_dot, "foo + foo", "ciwbar<Esc>ww.");
neovim_test!(scenarios, rename_local, "let x = x + 1;", "ciwcount<Esc>wwciw count<Esc>");
neovim_test!(scenarios, rename_method, "self.process()", cursor(0, 5), "ciwhandle<Esc>");
neovim_test!(scenarios, rename_const, "MAX_SIZE", "ciwMAX_COUNT<Esc>");
neovim_test!(scenarios, rename_type_param, "fn foo<T>(x: T)", cursor(0, 7), "rU/T<CR>rU");

// ═══════════════════════════════════════════════════════════════════════════════
// TYPE CHANGES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, change_type_i32_to_i64, "let x: i32 = 0;", cursor(0, 7), "ciwi64<Esc>");
neovim_test!(scenarios, change_type_str_to_string, "fn foo(s: &str)", cursor(0, 11), "ct)String<Esc>");
neovim_test!(scenarios, add_option_wrapper, "fn get() -> Value", cursor(0, 13), "ciwOption<Value><Esc>");
neovim_test!(scenarios, add_reference, "fn foo(data: Vec<u8>)", cursor(0, 13), "i&<Esc>");
neovim_test!(scenarios, add_mut_reference, "fn foo(data: &Vec<u8>)", cursor(0, 14), "imut <Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// MODIFIERS AND VISIBILITY
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, add_pub, "fn helper() {}", "Ipub <Esc>");
neovim_test!(scenarios, add_pub_crate, "fn internal() {}", "Ipub(crate) <Esc>");
neovim_test!(scenarios, add_async, "fn fetch() {}", cursor(0, 0), "Iasync <Esc>");
neovim_test!(scenarios, add_static, "let DATA: &str = \"\";", "Istatic <Esc>");
neovim_test!(scenarios, add_mut_to_let, "let buffer = vec![];", cursor(0, 3), "ea mut<Esc>");
neovim_test!(scenarios, remove_mut, "let mut x = 5;", cursor(0, 4), "daw");

// ═══════════════════════════════════════════════════════════════════════════════
// EXTRACT / INLINE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, inline_const, "const N: usize = 10;", "dd");
neovim_test!(scenarios, extract_to_variable, "process(compute(x))", "f(ldt)Ilet val = <Esc>");
neovim_test!(scenarios, duplicate_then_modify, "fn process(x: i32) {}", "yyp0ciwvalidate<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// WRAP / UNWRAP
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, wrap_in_some, "return value;", cursor(0, 7), "ciwSome(value)<Esc>");
neovim_test!(scenarios, wrap_in_ok, "return data;", cursor(0, 7), "ciwOk(data)<Esc>");
neovim_test!(scenarios, wrap_function_call, "value", "ISome(<Esc>A)<Esc>");
neovim_test!(scenarios, add_unwrap, "parse(s);", cursor(0, 7), "i.unwrap()<Esc>");
neovim_test!(scenarios, add_question_mark, "parse(s);", cursor(0, 7), "i?<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// SIGNATURE CHANGES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, add_param, "fn foo() {}", "f)i x: i32<Esc>");
neovim_test!(scenarios, add_return_type_refactor, "fn foo() {}", "f{i-> bool <Esc>");
neovim_test!(scenarios, change_return, "fn foo() -> i32 {}", cursor(0, 13), "ciwbool<Esc>");
neovim_test!(scenarios, add_lifetime, "fn foo(s: &str)", cursor(0, 10), "ci&&'a str<Esc>");
