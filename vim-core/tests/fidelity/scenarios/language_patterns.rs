// Scenario fidelity tests: Language-Specific Patterns
//
// Multi-step workflows for common patterns in different languages.

// ═══════════════════════════════════════════════════════════════════════════════
// PYTHON
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, py_add_self, "        name = value", "^Iself.<Esc>");
neovim_test!(scenarios, py_add_async, "def fetch():", "Iasync <Esc>");
neovim_test!(scenarios, py_add_decorator, "def handler():", "O@app.route('/')<Esc>");
neovim_test!(scenarios, py_add_type_hint, "def f(data):", "f)i: str<Esc>");
neovim_test!(scenarios, py_add_return_hint, "def f():", "f:i -> int<Esc>");
neovim_test!(scenarios, py_indent_body, "def foo():\npass", cursor(1, 0), "I    <Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// RUST
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, rs_add_borrow, "process(data)", cursor(0, 8), "i&<Esc>");
neovim_test!(scenarios, rs_add_mut, "let x = 5;", cursor(0, 3), "ea mut<Esc>");
neovim_test!(scenarios, rs_add_unwrap, "parse(s);", cursor(0, 7), "i.unwrap()<Esc>");
neovim_test!(scenarios, rs_add_pub, "fn helper() {}", "Ipub <Esc>");
neovim_test!(scenarios, rs_add_derive, "struct Foo {}", "O#[derive(Debug)]<Esc>");
neovim_test!(scenarios, rs_add_lifetime, "fn parse(s: &str)", cursor(0, 13), "a'a <Esc>");
neovim_test!(scenarios, rs_wrap_some, "return value;", cursor(0, 7), "ciwSome(value)<Esc>");
neovim_test!(scenarios, rs_add_question, "read()?;", cursor(0, 5), "");

// ═══════════════════════════════════════════════════════════════════════════════
// JAVASCRIPT / TYPESCRIPT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, js_add_const, "x = 42;", "Iconst <Esc>");
neovim_test!(scenarios, js_add_async, "function fetch() {}", "Iasync <Esc>");
neovim_test!(scenarios, js_add_await, "const d = fetch(url);", "/fetch<CR>iawait <Esc>");
neovim_test!(scenarios, js_add_export, "const API = {};", "Iexport <Esc>");
neovim_test!(scenarios, js_arrow_fn, "function(x) { return x; }", "cwconst fn =<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// HTML
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, html_add_class, "<div>", "f>i class=\"container\"<Esc>");
neovim_test!(scenarios, html_change_attr, "<img src=\"old.png\">", cursor(0, 10), "ci\"new.png<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// GDSCRIPT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, gd_add_export, "var speed: float", "O@export<Esc>");
neovim_test!(scenarios, gd_add_onready, "var label = $Label", "O@onready<Esc>");
neovim_test!(scenarios, gd_rename_func, "func _process(delta):", cursor(0, 5), "ciw_physics_process<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// C / C++
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, c_add_include, "int main() {}", "O#include <stdio.h><Esc>");
neovim_test!(scenarios, c_add_const, "int x = 0;", "Iconst <Esc>");
neovim_test!(scenarios, c_add_pointer, "int data", "fdiint *data<Esc>");
neovim_test!(scenarios, c_add_sizeof, "malloc(100)", "f1cisizeof(struct Data)<Esc>");
neovim_test!(scenarios, cpp_add_virtual, "void draw() {}", "Ivirtual <Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// GO
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, go_add_error_check, "    result := Call()", "oif err != nil {<CR>    return err<CR>}<Esc>");
neovim_test!(scenarios, go_add_package, "func main() {}", "Opackage main<Esc>");
neovim_test!(scenarios, go_change_receiver, "func (s *Server) Handle()", "f*ciwClient<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// MARKDOWN
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, md_promote_heading, "## Section", "0x");
neovim_test!(scenarios, md_demote_heading, "# Section", "I#<Esc>");
neovim_test!(scenarios, md_add_todo, "finish task", "I- [ ] <Esc>");
neovim_test!(scenarios, md_toggle_done, "- [ ] task", "f[lr x<Esc>");
neovim_test!(scenarios, md_add_code_block, "code here", "O```<Esc>jo```<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// CSS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, css_change_value, "  color: red;", "f:lcwblue<Esc>");
neovim_test!(scenarios, css_add_prop, "  color: red;", "o  font-size: 16px;<Esc>");
neovim_test!(scenarios, css_comment_prop, "  display: flex;", "I/* <Esc>A */<Esc>");

