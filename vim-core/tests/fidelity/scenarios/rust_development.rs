// Scenario fidelity tests: Rust Development Workflows
//
// Realistic multi-step editing sequences that arise during daily Rust
// development -- navigating structs, fixing compile errors, refactoring trait
// impls, editing match arms, wrangling closures, and the kind of small
// repetitive edits that make you reach for dot-repeat and visual-block mode.

// =============================================================================
// STRUCT FIELD EDITING
// =============================================================================

// Delete a single struct field
neovim_test!(scenarios, rs_dev_delete_struct_field,
    "struct Config {\n    name: String,\n    debug: bool,\n    port: u16,\n}",
    cursor(2, 0), "dd");

// Change a field's type from Vec<u8> to Vec<String>
neovim_test!(scenarios, rs_dev_change_field_type,
    "struct Packet {\n    data: Vec<u8>,\n}",
    cursor(1, 10), "ci<String<Esc>");

// Add a new field after the last one
neovim_test!(scenarios, rs_dev_add_struct_field,
    "struct Server {\n    host: String,\n}",
    cursor(1, 0), "oport: u16,<Esc>");

// Rename a struct field with ciw
neovim_test!(scenarios, rs_dev_rename_struct_field,
    "struct User {\n    name: String,\n}",
    cursor(1, 4), "ciwusername<Esc>");

// Add pub to a struct field
neovim_test!(scenarios, rs_dev_add_pub_to_field,
    "struct Api {\n    client: Client,\n}",
    cursor(1, 4), "Ipub <Esc>");

// =============================================================================
// FUNCTION SIGNATURES
// =============================================================================

// Change a function's return type
neovim_test!(scenarios, rs_dev_change_return_type,
    "fn parse(input: &str) -> Result<i32, Error> {",
    cursor(0, 25), "ct {Option<i32><Esc>");

// Add a return type to a function that has none
neovim_test!(scenarios, rs_dev_add_return_type,
    "fn validate(data: &[u8]) {",
    cursor(0, 24), "i -> bool<Esc>");

// Add a parameter to a function
neovim_test!(scenarios, rs_dev_add_parameter,
    "fn connect(host: &str) {",
    cursor(0, 21), "i, port: u16<Esc>");

// Change parameter type from owned to borrowed
neovim_test!(scenarios, rs_dev_borrow_parameter,
    "fn process(data: String) {",
    cursor(0, 17), "ciw&str<Esc>");

// Add mut to a parameter
neovim_test!(scenarios, rs_dev_add_mut_param,
    "fn update(state: &State) {",
    cursor(0, 17), "a mut<Esc>");

// Add a lifetime to a function signature
neovim_test!(scenarios, rs_dev_add_lifetime,
    "fn parse(s: &str) -> &str {",
    cursor(0, 7), "a<'a><Esc>f&a'a <Esc>f&a'a <Esc>");

// =============================================================================
// MATCH ARMS
// =============================================================================

// Add a new match arm
neovim_test!(scenarios, rs_dev_add_match_arm,
    "match color {\n    Red => \"red\",\n    Blue => \"blue\",\n}",
    cursor(2, 0), "oGreen => \"green\",<Esc>");

// Delete a match arm
neovim_test!(scenarios, rs_dev_delete_match_arm,
    "match dir {\n    Up => 1,\n    Down => -1,\n    Left => 0,\n}",
    cursor(3, 0), "dd");

// Change a match arm's body
neovim_test!(scenarios, rs_dev_change_match_body,
    "match cmd {\n    Quit => break,\n    Run => continue,\n}",
    cursor(1, 14), "C return,<Esc>");

// Add a wildcard arm at the end
neovim_test!(scenarios, rs_dev_add_wildcard_arm,
    "match event {\n    Click => handle_click(),\n}",
    cursor(1, 0), "o_ => {},<Esc>");

// Rename a pattern in a match arm
neovim_test!(scenarios, rs_dev_rename_match_pattern,
    "match token {\n    Ident(name) => name.clone(),\n}",
    cursor(1, 4), "ciwKeyword<Esc>");

// =============================================================================
// IMPL BLOCKS AND METHODS
// =============================================================================

// Add a new method to an impl block
neovim_test!(scenarios, rs_dev_add_method,
    "impl Parser {\n    fn parse(&self) -> Node {\n        todo!()\n    }\n}",
    cursor(3, 0), "o\n    fn reset(&mut self) {\n        todo!()\n    }<Esc>");

// Change &self to &mut self in a method
neovim_test!(scenarios, rs_dev_self_to_mut_self,
    "    fn update(&self) {",
    cursor(0, 15), "imu t <Esc>");

// Add pub to a method
neovim_test!(scenarios, rs_dev_add_pub_method,
    "    fn new() -> Self {",
    cursor(0, 4), "Ipub <Esc>");

// =============================================================================
// FIXING COMPILE ERRORS
// =============================================================================

// Fix a missing semicolon at end of line
neovim_test!(scenarios, rs_dev_fix_missing_semicolon,
    "    let x = compute()\n    let y = 0;",
    cursor(0, 0), "A;<Esc>");

// Fix a typo in a variable name
neovim_test!(scenarios, rs_dev_fix_var_typo,
    "    let reuslt = parse(input);",
    cursor(0, 8), "ciwresult<Esc>");

// Add a missing borrow
neovim_test!(scenarios, rs_dev_fix_add_borrow,
    "    process(data);",
    cursor(0, 12), "i&<Esc>");

// Change `unwrap` to `?` for proper error propagation
neovim_test!(scenarios, rs_dev_unwrap_to_question,
    "    let val = parse(s).unwrap();",
    cursor(0, 21), "df(i?<Esc>");

// Wrap a return value in Ok()
neovim_test!(scenarios, rs_dev_wrap_in_ok,
    "    return value;",
    cursor(0, 11), "ciwOk(value)<Esc>");

// Fix wrong type: i32 -> usize
neovim_test!(scenarios, rs_dev_fix_wrong_type,
    "fn index(n: i32) -> usize {",
    cursor(0, 12), "ciwusize<Esc>");

// Add .clone() to satisfy the borrow checker
neovim_test!(scenarios, rs_dev_add_clone,
    "    send(name);",
    cursor(0, 12), "i.clone()<Esc>");

// Add `as usize` cast
neovim_test!(scenarios, rs_dev_add_as_cast,
    "    buffer[idx];",
    cursor(0, 14), "i as usize<Esc>");

// =============================================================================
// COMMENTING AND UNCOMMENTING
// =============================================================================

// Comment out a single line with //
neovim_test!(scenarios, rs_dev_comment_line,
    "    dbg!(value);",
    cursor(0, 0), "I// <Esc>");

// Uncomment a line (remove // prefix)
neovim_test!(scenarios, rs_dev_uncomment_line,
    "    // dbg!(value);",
    cursor(0, 4), "3x");

// Comment out with a TODO note
neovim_test!(scenarios, rs_dev_comment_todo,
    "    old_cleanup();",
    cursor(0, 0), "I// TODO: remove -- <Esc>");

// =============================================================================
// DOT-REPEAT WORKFLOWS
// =============================================================================

// Add semicolons to multiple lines using dot repeat
neovim_test!(scenarios, rs_dev_dot_add_semicolons,
    "    let a = 1\n    let b = 2\n    let c = 3",
    "A;<Esc>j.j.");

// Rename all occurrences using search + dot
neovim_test!(scenarios, rs_dev_dot_rename_search,
    "let old = old + old;",
    "/old<CR>ciwnew<Esc>n.n.");

// Add pub to multiple consecutive functions
neovim_test!(scenarios, rs_dev_dot_add_pub_fns,
    "fn alpha() {}\nfn beta() {}\nfn gamma() {}",
    "Ipub <Esc>j0.");

// Delete first word of multiple lines (remove `let`)
neovim_test!(scenarios, rs_dev_dot_delete_let,
    "let a = 1;\nlet b = 2;\nlet c = 3;",
    "dw.j0.j0.");

// =============================================================================
// VISUAL BLOCK OPERATIONS
// =============================================================================

// Add `pub ` prefix to multiple struct fields using visual block insert
neovim_test!(scenarios, rs_dev_vblock_pub_fields,
    "struct Data {\n    name: String,\n    age: u32,\n    active: bool,\n}",
    cursor(1, 4), "<C-v>2jIpub <Esc>");

// Comment out multiple lines using visual block
neovim_test!(scenarios, rs_dev_vblock_comment,
    "    let a = 1;\n    let b = 2;\n    let c = 3;",
    cursor(0, 0), "<C-v>2jI// <Esc>");

// Add trailing commas to multiple lines using visual block append
neovim_test!(scenarios, rs_dev_vblock_trailing_commas,
    "    Red\n    Green\n    Blue",
    cursor(0, 0), "<C-v>2j$A,<Esc>");

// =============================================================================
// INDENTATION
// =============================================================================

// Indent a block of code inside a function
neovim_test!(scenarios, rs_dev_indent_block,
    "fn main() {\nlet x = 1;\nlet y = 2;\n}",
    cursor(1, 0), "Vj>");

// Outdent over-indented code
neovim_test!(scenarios, rs_dev_outdent_block,
    "fn f() {\n        let x = 1;\n        let y = 2;\n}",
    cursor(1, 0), "Vj<");

// =============================================================================
// MOVING LINES
// =============================================================================

// Move a let-binding up above another
neovim_test!(scenarios, rs_dev_move_line_up,
    "    let b = a + 1;\n    let a = 0;",
    cursor(1, 0), "ddkP");

// Swap two match arms
neovim_test!(scenarios, rs_dev_swap_match_arms,
    "    Some(v) => v,\n    None => 0,",
    "ddp");

// =============================================================================
// COPY AND MODIFY (DUPLICATE-THEN-EDIT)
// =============================================================================

// Duplicate a function signature and rename it
neovim_test!(scenarios, rs_dev_dup_fn_rename,
    "fn process_input(data: &[u8]) -> Result<(), Error> {",
    "yyp^wciwvalidate_input<Esc>");

// Duplicate a struct field and change its name and type
neovim_test!(scenarios, rs_dev_dup_field_modify,
    "    width: f32,",
    "yypwciwheight<Esc>");

// Duplicate a use statement and change the imported item
neovim_test!(scenarios, rs_dev_dup_use_stmt,
    "use std::collections::HashMap;",
    "yypf:lllciwBTreeMap<Esc>");

// =============================================================================
// UNDO AFTER MISTAKES
// =============================================================================

// Type the wrong variable name, undo, retype
neovim_test!(scenarios, rs_dev_undo_wrong_name,
    "    let  = 0;",
    cursor(0, 8), "icountt<Esc>uicount<Esc>");

// Accidentally delete a line, undo immediately
neovim_test!(scenarios, rs_dev_undo_deleted_line,
    "    let x = 1;\n    let y = 2;",
    cursor(1, 0), "ddu");

// Change a type incorrectly, undo and redo correctly
neovim_test!(scenarios, rs_dev_undo_wrong_type,
    "fn get() -> String {",
    cursor(0, 12), "ciw&stir<Esc>uciw&str<Esc>");

// =============================================================================
// CLOSURES AND ITERATORS
// =============================================================================

// Change a closure parameter name
neovim_test!(scenarios, rs_dev_change_closure_param,
    "    .map(|x| x * 2)",
    cursor(0, 10), "ciwitem<Esc>wciw item<Esc>");

// Add a filter call after a map
neovim_test!(scenarios, rs_dev_add_filter_chain,
    "    data.iter().map(|x| x + 1).collect();",
    cursor(0, 30), "i.filter(|x| *x > 0)<Esc>");

// Wrap a closure body in braces (single-line to multi-line)
neovim_test!(scenarios, rs_dev_expand_closure,
    "    .map(|s| s.trim())",
    cursor(0, 12), "C {\n        s.trim()\n    })<Esc>");

// =============================================================================
// USE STATEMENTS AND IMPORTS
// =============================================================================

// Add a new use statement above current code
neovim_test!(scenarios, rs_dev_add_use_stmt,
    "fn main() {\n    let map = HashMap::new();\n}",
    cursor(0, 0), "Ouse std::collections::HashMap;<Esc>");

// Change an import path
neovim_test!(scenarios, rs_dev_change_import,
    "use crate::old_module::Thing;",
    cursor(0, 11), "ciwnew_module<Esc>");

// =============================================================================
// SEARCH AND REPLACE IN RUST CODE
// =============================================================================

// Replace all occurrences of a type alias
neovim_test!(scenarios, rs_dev_substitute_type,
    "type Res = Result<(), Error>;\nfn a() -> Res {}\nfn b() -> Res {}",
    ":%s/Res/AppResult/g<CR>");

// Replace unwrap with expect across a file
neovim_test!(scenarios, rs_dev_substitute_unwrap,
    "    a.unwrap();\n    b.unwrap();\n    c.unwrap();",
    ":%s/unwrap()/expect(\"failed\")/g<CR>");

// =============================================================================
// MACRO AND ATTRIBUTE EDITING
// =============================================================================

// Add a derive attribute above a struct
neovim_test!(scenarios, rs_dev_add_derive,
    "struct Point {\n    x: f64,\n    y: f64,\n}",
    cursor(0, 0), "O#[derive(Debug, Clone, PartialEq)]<Esc>");

// Add #[cfg(test)] above a module
neovim_test!(scenarios, rs_dev_add_cfg_test,
    "mod tests {\n    use super::*;\n}",
    cursor(0, 0), "O#[cfg(test)]<Esc>");

// Change a derive list -- add Serialize
neovim_test!(scenarios, rs_dev_extend_derive,
    "#[derive(Debug, Clone)]",
    cursor(0, 21), "i, Serialize<Esc>");
