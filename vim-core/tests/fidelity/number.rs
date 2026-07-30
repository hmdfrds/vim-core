// Number increment/decrement fidelity tests (Ctrl-A, Ctrl-X).

// ═══════════════════════════════════════════════════════════════════════════════
// BASIC INCREMENT (Ctrl-A)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(number, ctrl_a_basic, "42", "<C-a>");
neovim_test!(number, ctrl_a_zero, "0", "<C-a>");
neovim_test!(number, ctrl_a_negative, "-5", "<C-a>");
neovim_test!(number, ctrl_a_with_count, "10", "5<C-a>");
neovim_test!(number, ctrl_a_cursor_before_number, "foo 42 bar", "<C-a>");
neovim_test!(number, ctrl_a_cursor_on_number, "foo 42 bar", cursor(0, 5), "<C-a>");
neovim_test!(number, ctrl_a_no_number, "hello", "<C-a>");
neovim_test!(number, ctrl_a_99, "99", "<C-a>");
neovim_test!(number, ctrl_a_negative_to_zero, "-1", "<C-a>");
neovim_test!(number, ctrl_a_large_count, "0", "100<C-a>");

// ═══════════════════════════════════════════════════════════════════════════════
// BASIC DECREMENT (Ctrl-X)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(number, ctrl_x_basic, "42", "<C-x>");
neovim_test!(number, ctrl_x_zero, "0", "<C-x>");
neovim_test!(number, ctrl_x_negative, "-5", "<C-x>");
neovim_test!(number, ctrl_x_with_count, "10", "5<C-x>");
neovim_test!(number, ctrl_x_cursor_before_number, "foo 42 bar", "<C-x>");
neovim_test!(number, ctrl_x_one_to_zero, "1", "<C-x>");
neovim_test!(number, ctrl_x_zero_to_negative, "0", "<C-x>");

// ═══════════════════════════════════════════════════════════════════════════════
// HEX/OCTAL/BINARY
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(number, ctrl_a_hex, "0xff", "<C-a>");
neovim_test!(number, ctrl_x_hex, "0x10", "<C-x>");
neovim_test!(number, ctrl_a_octal, "0o7", "<C-a>");
neovim_test!(number, ctrl_a_binary, "0b11", "<C-a>");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT REPEAT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(number, ctrl_a_dot_repeat, "1\n2\n3", "<C-a>j.");
neovim_test!(number, ctrl_x_dot_repeat, "5\n5\n5", "<C-x>j.");
neovim_test!(number, ctrl_a_count_dot, "10", "5<C-a>.");

// ═══════════════════════════════════════════════════════════════════════════════
// BOUNDARY VALUES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(number, ctrl_a_max_i32, "2147483646", "<C-a>");
neovim_test!(number, ctrl_x_min_negative, "-2147483647", "<C-x>");
neovim_test!(number, ctrl_a_large_negative, "-100", "200<C-a>");
neovim_test!(number, ctrl_x_large_positive, "100", "200<C-x>");
neovim_test!(number, ctrl_a_single_digit, "9", "<C-a>");
neovim_test!(number, ctrl_x_single_digit, "1", "<C-x>");

// ═══════════════════════════════════════════════════════════════════════════════
// CURSOR POSITION EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(number, ctrl_a_cursor_after_number, "42 hello", cursor(0, 3), "<C-a>");
neovim_test!(number, ctrl_a_cursor_between_numbers, "10 20 30", cursor(0, 3), "<C-a>");
neovim_test!(number, ctrl_a_multiple_numbers_in_line, "a 10 b 20 c", "<C-a>");
neovim_test!(number, ctrl_x_cursor_end_of_line, "hello 42", cursor(0, 7), "<C-x>");
neovim_test!(number, ctrl_a_negative_in_text, "value is -5 here", cursor(0, 10), "<C-a>");

// ═══════════════════════════════════════════════════════════════════════════════
// HEX/OCTAL/BINARY EXTENDED
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(number, ctrl_a_hex_ff, "0xff", "<C-a>");
neovim_test!(number, ctrl_x_hex_to_zero, "0x01", "<C-x>");
neovim_test!(number, ctrl_a_hex_uppercase, "0xFF", "<C-a>");
neovim_test!(number, ctrl_a_octal_7_overflow, "0o77", "<C-a>");
neovim_test!(number, ctrl_x_octal_to_zero, "0o01", "<C-x>");
neovim_test!(number, ctrl_a_binary_carry, "0b111", "<C-a>");
neovim_test!(number, ctrl_x_binary_to_zero, "0b001", "<C-x>");
neovim_test!(number, ctrl_a_hex_with_count, "0x0a", "5<C-a>");
neovim_test!(number, ctrl_x_hex_with_count, "0xff", "16<C-x>");

// ═══════════════════════════════════════════════════════════════════════════════
// NUMBERS IN CONTEXT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(number, ctrl_a_in_code, "let x = 42;", "<C-a>");
neovim_test!(number, ctrl_x_in_code, "let x = 42;", "<C-x>");
neovim_test!(number, ctrl_a_in_array, "[1, 2, 3]", cursor(0, 1), "<C-a>");
neovim_test!(number, ctrl_a_in_comment, "// line 5", "<C-a>");
neovim_test!(number, ctrl_a_version, "v1.2.3", cursor(0, 1), "<C-a>");
neovim_test!(number, ctrl_a_ip_address, "192.168.1.1", "<C-a>");

// ═══════════════════════════════════════════════════════════════════════════════
// COMBINED WITH UNDO
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(number, ctrl_a_undo, "42", "<C-a>u");
neovim_test!(number, ctrl_x_undo, "42", "<C-x>u");
neovim_test!(number, ctrl_a_undo_redo, "42", "<C-a>u<C-r>");
neovim_test!(number, ctrl_a_multiple_undo, "10", "<C-a><C-a>uu");

// ═══════════════════════════════════════════════════════════════════════════════
// NEGATIVE NUMBERS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(number, ctrl_a_neg_to_positive, "-1", "2<C-a>");
neovim_test!(number, ctrl_x_pos_to_negative, "1", "2<C-x>");
neovim_test!(number, ctrl_a_deeply_negative, "-100", "<C-a>");
neovim_test!(number, ctrl_x_deeply_negative, "-100", "<C-x>");
neovim_test!(number, ctrl_a_negative_with_count, "-10", "20<C-a>");

// ═══════════════════════════════════════════════════════════════════════════════
// MULTIPLE NUMBERS PER LINE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(number, ctrl_a_first_of_many, "1 2 3", "<C-a>");
neovim_test!(number, ctrl_a_second_of_many, "1 2 3", cursor(0, 2), "<C-a>");
neovim_test!(number, ctrl_a_last_of_many, "1 2 3", cursor(0, 4), "<C-a>");

// ═══════════════════════════════════════════════════════════════════════════════
// REPEATED PRESSES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(number, ctrl_a_repeated_10_times, "10", "<C-a><C-a><C-a><C-a><C-a><C-a><C-a><C-a><C-a><C-a>");
neovim_test!(number, ctrl_x_repeated_11_times, "0", "<C-x><C-x><C-x><C-x><C-x><C-x><C-x><C-x><C-x><C-x><C-x>");
