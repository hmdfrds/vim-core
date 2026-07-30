// Scenario fidelity tests: Insert Mode Workflows
//
// Real-world insert mode patterns with special keys and control chars.

// ═══════════════════════════════════════════════════════════════════════════════
// BASIC INSERT + ESCAPE PATTERNS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, insert_word_esc, "hello", "iworld <Esc>");
neovim_test!(scenarios, append_word_esc, "hello", "a world<Esc>");
neovim_test!(scenarios, insert_line_start, "  hello", "Istart <Esc>");
neovim_test!(scenarios, append_line_end, "hello", "A world<Esc>");
neovim_test!(scenarios, open_below_type, "above", "obelow<Esc>");
neovim_test!(scenarios, open_above_type, "below", "Oabove<Esc>");
neovim_test!(scenarios, open_below_indented, "  above", "obelow<Esc>");
neovim_test!(scenarios, open_above_indented, "  below", "Oabove<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// INSERT MODE CTRL KEYS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, ctrl_w_delete_word, "hello", "A world<C-w><Esc>");
neovim_test!(scenarios, ctrl_w_mid_word, "hello", "Atest<C-w><Esc>");
neovim_test!(scenarios, ctrl_w_multiple, "hello", "A one two<C-w><C-w><Esc>");
neovim_test!(scenarios, ctrl_u_delete_to_start, "hello", "A world test<C-u><Esc>");
neovim_test!(scenarios, insert_ctrl_h_backspace, "hello", "A world<C-h><C-h><C-h><Esc>");
neovim_test!(scenarios, backspace_in_insert, "abc", "A<BS><BS><Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// INSERT FROM REGISTER (Ctrl-R)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, insert_reg_unnamed, "hello world", "yiwA <C-r>\"<Esc>");
neovim_test!(scenarios, insert_reg_named, "hello world", "\"aywA <C-r>a<Esc>");
neovim_test!(scenarios, insert_reg_zero, "hello world", "yiwdiwA<C-r>0<Esc>");
neovim_test!(scenarios, insert_reg_in_middle, "text", "bi<C-r>\"<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// MULTILINE INSERT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, insert_newline, "hello world", "fwi<CR><Esc>");
neovim_test!(scenarios, insert_two_lines, "content", "oline2<CR>line3<Esc>");
neovim_test!(scenarios, insert_above_multiple, "end", "Oline1<CR>line2<Esc>");
neovim_test!(scenarios, type_function_body, "fn foo() {}", "f{a<CR>    body();<CR><Esc>");
neovim_test!(scenarios, type_block, "", "iif x > 0 {<CR>    do_thing();<CR>}<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// INSERT + MOTION COMBOS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, insert_esc_move_insert, "abc def", "iX<Esc>wwiY<Esc>");
neovim_test!(scenarios, append_esc_move_append, "abc def", "aX<Esc>waY<Esc>");
neovim_test!(scenarios, insert_esc_find_insert, "a = b", "ilet <Esc>f=a: i32 <Esc>");
neovim_test!(scenarios, open_type_move_open, "line1\nline3", "oline2<Esc>joline4<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// INDENT IN INSERT MODE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, ctrl_t_indent, "hello", "I<C-t><Esc>");
neovim_test!(scenarios, ctrl_d_outdent, "    hello", "I<C-d><Esc>");
neovim_test!(scenarios, ctrl_t_twice, "hello", "I<C-t><C-t><Esc>");
