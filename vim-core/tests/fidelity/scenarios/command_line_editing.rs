// Command-line mode editing fidelity tests.
//
// Tests for editing within : command line, search (/ and ?) prompts,
// and command-line control keys.

// ═══════════════════════════════════════════════════════════════════════════════
// BASIC EX COMMAND ENTRY AND EXECUTION
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cmdline_basic_colon, "hello world", ":noh<CR>");
neovim_test!(scenarios, cmdline_escape_cancel, "hello world", ":d<Esc>");
neovim_test!(scenarios, cmdline_ctrl_c_cancel, "hello world", ":d<C-c>");
neovim_test!(scenarios, cmdline_ctrl_bracket_cancel, "hello world", ":d<C-[>");

// ═══════════════════════════════════════════════════════════════════════════════
// BACKSPACE/DELETE IN COMMAND LINE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cmdline_backspace, "hello", ":dx<BS><CR>");
neovim_test!(scenarios, cmdline_ctrl_h_backspace, "hello", ":dx<C-h><CR>");
neovim_test!(scenarios, cmdline_backspace_all, "hello", ":abc<BS><BS><BS><CR>");
neovim_test!(scenarios, cmdline_backspace_past_start, "hello", ":<BS>");
neovim_test!(scenarios, cmdline_backspace_in_search, "hello hello", "/hellx<BS>o<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// Ctrl-U — DELETE TO LINE START
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cmdline_ctrl_u, "hello", ":hello<C-u><CR>");
neovim_test!(scenarios, cmdline_ctrl_u_retype, "hello", ":old<C-u>d<CR>");
neovim_test!(scenarios, cmdline_ctrl_u_empty, "hello", ":<C-u><CR>");
neovim_test!(scenarios, cmdline_ctrl_u_in_search, "hello world", "/old<C-u>hello<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// Ctrl-W — DELETE WORD BACKWARD
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cmdline_ctrl_w, "hello", ":s/hello/world<C-w><CR>");
neovim_test!(scenarios, cmdline_ctrl_w_single_word, "hello", ":hello<C-w><CR>");
neovim_test!(scenarios, cmdline_ctrl_w_in_search, "hello world", "/hello world<C-w><CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// SEARCH PROMPT EDITING
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, search_fwd_basic, "hello world hello", "/hello<CR>");
neovim_test!(scenarios, search_bwd_basic, "hello world hello", cursor(0, 12), "?hello<CR>");
neovim_test!(scenarios, search_fwd_escape, "hello world", "/test<Esc>");
neovim_test!(scenarios, search_bwd_escape, "hello world", "?test<Esc>");
neovim_test!(scenarios, search_fwd_ctrl_c, "hello world", "/test<C-c>");
neovim_test!(scenarios, search_fwd_backspace, "hello world hello", "/helloo<BS><CR>");
neovim_test!(scenarios, search_fwd_ctrl_u, "hello world hello", "/wrong<C-u>hello<CR>");
neovim_test!(scenarios, search_bwd_backspace, "hello world hello", cursor(0, 12), "?helloo<BS><CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// COMMAND LINE AFTER MOTIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cmdline_after_motion, "l1\nl2\nl3", "j:d<CR>");
neovim_test!(scenarios, cmdline_after_search, "hello world", "/world<CR>:d<CR>");
neovim_test!(scenarios, cmdline_after_mark, "l1\nl2\nl3", "jma:d<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL RANGE COMMANDS (:'<,'>)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cmdline_visual_range_d, "l1\nl2\nl3\nl4", "Vj:d<CR>");
neovim_test!(scenarios, cmdline_visual_range_sub, "old\nold\nold", "Vj:s/old/new/g<CR>");
neovim_test!(scenarios, cmdline_visual_range_norm, "hello\nworld", "Vj:norm A!<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// EX COMMAND WITH SPECIAL CHARACTERS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cmdline_sub_slash, "a/b", ":s/\\//X/g<CR>");
neovim_test!(scenarios, cmdline_sub_backslash, "a\\b", ":s/\\\\/X/g<CR>");
neovim_test!(scenarios, cmdline_sub_pipe, "a|b", ":s/|/X/g<CR>");
neovim_test!(scenarios, cmdline_sub_dot, "a.b.c", ":s/\\./X/g<CR>");
neovim_test!(scenarios, cmdline_sub_star, "a***b", ":s/\\*/X/g<CR>");
neovim_test!(scenarios, cmdline_sub_caret, "^hello", ":s/\\^/X<CR>");
neovim_test!(scenarios, cmdline_sub_dollar_sign, "hello$", ":s/\\$/X<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// EMPTY COMMAND / NO-OP
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cmdline_empty_enter, "hello", ":<CR>");
neovim_test!(scenarios, cmdline_spaces_only, "hello", ":   <CR>");
neovim_test!(scenarios, cmdline_unknown_cmd, "hello", ":unknown<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// Ctrl-R IN COMMAND LINE (INSERT REGISTER)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cmdline_ctrl_r_yank, "hello world", "yiw:s/<C-r>0/bye<CR>");
neovim_test!(scenarios, cmdline_ctrl_r_named, "hello world", "\"ayw:s/<C-r>a/bye<CR>");
neovim_test!(scenarios, search_ctrl_r_yank, "hello world hello", "yiw/<C-r>0<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// COMMAND LINE HISTORY NAVIGATION
// ═══════════════════════════════════════════════════════════════════════════════

// Up/Down arrow to navigate history — second command after first
neovim_test!(scenarios, cmdline_history_up, "hello\nhello", ":s/hello/world<CR>j:<Up><CR>");
neovim_test!(scenarios, search_history_up, "hello world hello", "/hello<CR>/<Up><CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// SUBSTITUTION EDGE CASES IN COMMAND LINE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cmdline_sub_empty_pattern, "hello hello", ":%s//bye/g<CR>");
neovim_test!(scenarios, cmdline_sub_empty_replace, "hello world", ":s/hello//g<CR>");
neovim_test!(scenarios, cmdline_sub_same, "hello", ":s/hello/hello<CR>");
neovim_test!(scenarios, cmdline_sub_multiline, "hello\nworld", ":%s/hello/bye/g<CR>");
neovim_test!(scenarios, cmdline_sub_no_match, "hello", ":s/xyz/abc<CR>");
neovim_test!(scenarios, cmdline_sub_regex_star, "aaa bbb", ":s/a*/X/g<CR>");
neovim_test!(scenarios, cmdline_sub_regex_plus, "aaa bbb", ":s/a\\+/X/g<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// COMMAND LINE LENGTH / EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cmdline_long_cmd, "hello", ":s/hello/a_very_long_replacement_string_that_goes_on_and_on<CR>");
neovim_test!(scenarios, cmdline_many_backspaces, "hello", ":abcde<BS><BS><BS><BS><BS><CR>");
neovim_test!(scenarios, cmdline_ctrl_u_then_type, "hello", ":wrong<C-u>d<CR>");
neovim_test!(scenarios, cmdline_multiple_ctrl_u, "hello", ":abc<C-u>def<C-u>d<CR>");
