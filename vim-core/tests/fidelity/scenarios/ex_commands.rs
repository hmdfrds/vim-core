// Scenario fidelity tests: Ex Commands
//
// Command-line mode operations for editing.

// ═══════════════════════════════════════════════════════════════════════════════
// SUBSTITUTION (:s)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, sub_basic, "old and old", ":s/old/new<CR>");
neovim_test!(scenarios, sub_global, "old and old", ":s/old/new/g<CR>");
neovim_test!(scenarios, sub_all_lines, "old\nold\nold", ":%s/old/new/g<CR>");
neovim_test!(scenarios, sub_confirm, "old old old", ":s/old/new/gc<CR>");
neovim_test!(scenarios, sub_regex_word, "foo foobar", ":s/\\<foo\\>/bar<CR>");
neovim_test!(scenarios, sub_case_insensitive, "Hello hello HELLO", ":s/hello/hi/gi<CR>");
neovim_test!(scenarios, sub_empty_replace, "remove_me rest", ":s/remove_me //g<CR>");
neovim_test!(scenarios, sub_special_chars, "a.b.c", ":s/\\./,/g<CR>");
neovim_test!(scenarios, sub_range, "aa\nbb\ncc\ndd", ":2,3s/./X/g<CR>");
neovim_test!(scenarios, sub_current_line, "aaa bbb aaa", ":s/aaa/ccc<CR>");
neovim_test!(scenarios, sub_last_line, "first\nlast", ":$s/last/end<CR>");
neovim_test!(scenarios, sub_with_ampersand, "hello", ":s/hello/&_world<CR>");
neovim_test!(scenarios, sub_capture_group, "foo123", ":s/\\(foo\\)\\(123\\)/\\2\\1<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// DELETE / MOVE / COPY COMMANDS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, ex_delete_line, "l1\nl2\nl3", ":2d<CR>");
neovim_test!(scenarios, ex_delete_range, "l1\nl2\nl3\nl4", ":2,3d<CR>");
neovim_test!(scenarios, ex_delete_last, "l1\nl2\nl3", ":$d<CR>");
neovim_test!(scenarios, ex_move_line, "l1\nl2\nl3", ":1m3<CR>");
neovim_test!(scenarios, ex_move_range, "l1\nl2\nl3\nl4", ":1,2m4<CR>");
neovim_test!(scenarios, ex_copy_line, "l1\nl2\nl3", ":1co3<CR>");
neovim_test!(scenarios, ex_copy_range, "l1\nl2\nl3", ":1,2co3<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// GLOBAL COMMAND (:g)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, global_delete, "keep\nremove\nkeep\nremove", ":g/remove/d<CR>");
neovim_test!(scenarios, global_inverse, "keep\nremove\nkeep\nremove", ":v/keep/d<CR>");
neovim_test!(scenarios, global_sub, "aa\nbb\naa\nbb", ":g/aa/s/aa/cc<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// NORMAL COMMAND (:norm)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, norm_append, "l1\nl2\nl3", ":%norm A!<CR>");
neovim_test!(scenarios, norm_prepend, "l1\nl2\nl3", ":%norm I# <CR>");
neovim_test!(scenarios, norm_range, "l1\nl2\nl3\nl4", ":2,3norm A!<CR>");
neovim_test!(scenarios, norm_delete_first, "- l1\n- l2\n- l3", ":%norm 2x<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// LINE NUMBER OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, goto_line_ex, "l1\nl2\nl3\nl4\nl5", ":3<CR>");
neovim_test!(scenarios, goto_last_line_ex, "l1\nl2\nl3", ":$<CR>");
neovim_test!(scenarios, goto_first_line_ex, "l1\nl2\nl3", cursor(2, 0), ":1<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// :noh (CLEAR SEARCH HIGHLIGHTS)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, ex_noh_basic, "foo bar foo", "/foo<CR>:noh<CR>");
neovim_test!(scenarios, ex_noh_no_search, "hello", ":noh<CR>");
neovim_test!(scenarios, ex_nohlsearch, "foo bar foo", "/foo<CR>:nohlsearch<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// :y (EX YANK)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, ex_yank_current, "line1\nline2\nline3", ":y<CR>p");
neovim_test!(scenarios, ex_yank_range, "line1\nline2\nline3", ":1,2y<CR>Gp");
neovim_test!(scenarios, ex_yank_to_register, "line1\nline2", ":y a<CR>\"ap");
neovim_test!(scenarios, ex_yank_all, "line1\nline2\nline3", ":%y<CR>Gp");

// ═══════════════════════════════════════════════════════════════════════════════
// :sort (STANDALONE SORT)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, ex_sort_all, "cherry\napple\nbanana", ":%sort<CR>");
neovim_test!(scenarios, ex_sort_reverse, "apple\nbanana\ncherry", ":%sort!<CR>");
neovim_test!(scenarios, ex_sort_range, "cherry\napple\nbanana\ndate", ":1,3sort<CR>");
neovim_test!(scenarios, ex_sort_numeric, "10\n2\n1\n20", ":%sort n<CR>");
neovim_test!(scenarios, ex_sort_unique, "a\nb\na\nc\nb", ":%sort u<CR>");
neovim_test!(scenarios, ex_sort_ignorecase, "Banana\napple\nCherry", ":%sort i<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// :j (EX JOIN)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, ex_join_basic, "hello\nworld", ":j<CR>");
neovim_test!(scenarios, ex_join_range, "a\nb\nc\nd", ":1,3j<CR>");
neovim_test!(scenarios, ex_join_all, "a\nb\nc", ":%j<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// :put (PUT FROM REGISTER)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, ex_put_default, "hello\nworld", "yy:put<CR>");
neovim_test!(scenarios, ex_put_register, "hello\nworld", "\"ayy:put a<CR>");
neovim_test!(scenarios, ex_put_before, "hello\nworld", "yy:put!<CR>");
neovim_test!(scenarios, ex_put_at_line, "l1\nl2\nl3", "yy:2put<CR>");
neovim_test!(scenarios, ex_put_last_line, "l1\nl2", "yy:$put<CR>");
neovim_test!(scenarios, ex_put_first_line, "l1\nl2", "yy:0put<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// :retab (CONVERT TABS/SPACES)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, ex_retab_basic, "\thello", ":retab<CR>");
neovim_test!(scenarios, ex_retab_with_width, "\thello", ":retab 4<CR>");
neovim_test!(scenarios, ex_retab_multiple_tabs, "\t\thello", ":retab<CR>");
neovim_test!(scenarios, ex_retab_mixed, "\t  hello", ":retab<CR>");
neovim_test!(scenarios, ex_retab_range, "\thello\n\tworld\nfoo", ":1,2retab<CR>");
neovim_test!(scenarios, ex_retab_all, "\ta\n\tb\n\tc", ":%retab<CR>");
neovim_test!(scenarios, ex_retab_bang, "    hello", ":retab! 8<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// MARK RANGES IN EX COMMANDS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, ex_mark_range_delete, "l1\nl2\nl3\nl4", "maGmb:'a,'bd<CR>");
neovim_test!(scenarios, ex_mark_range_sub, "old\nold\nnew", "majma:'a,'bs/old/new/g<CR>");
neovim_test!(scenarios, ex_mark_range_yank, "l1\nl2\nl3", "majjma:'a,'by<CR>Gp");
neovim_test!(scenarios, ex_mark_range_norm, "l1\nl2\nl3", "majjma:'a,'bnorm A!<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL RANGE EX COMMANDS (:'<,'>)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, ex_visual_sub, "old old\nold old\nnew", "Vj:s/old/new/g<CR>");
neovim_test!(scenarios, ex_visual_delete, "l1\nl2\nl3\nl4", "Vj:d<CR>");
neovim_test!(scenarios, ex_visual_norm, "l1\nl2\nl3", "Vj:norm A!<CR>");
neovim_test!(scenarios, ex_visual_sort, "cherry\napple\nbanana", "Vjj:sort<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// MORE :s PATTERNS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, sub_delete_matches, "foo bar foo", ":s/foo//g<CR>");
neovim_test!(scenarios, sub_newline_in_pattern, "hello world", ":s/ /\\r/g<CR>");
neovim_test!(scenarios, sub_tilde_repeat, "old old", ":s/old/new<CR>:s/old/~<CR>");
neovim_test!(scenarios, sub_percent_all, "aaa\nbbb\naaa", ":%s/aaa/xxx<CR>");
neovim_test!(scenarios, sub_count_only, "old old old", ":s/old/new/gn<CR>");
neovim_test!(scenarios, sub_first_line, "first\nsecond", ":1s/first/new<CR>");
neovim_test!(scenarios, sub_dollar_ref, "hello world", ":s/\\(hello\\) \\(world\\)/\\2 \\1<CR>");
neovim_test!(scenarios, sub_whole_line, "replace me entirely", ":s/.*/new content<CR>");
neovim_test!(scenarios, sub_tab_char, "hello\tworld", ":s/\\t/ /g<CR>");
neovim_test!(scenarios, sub_multiline_range, "a\nb\nc\nd", ":1,3s/./X<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// MORE :g (GLOBAL) PATTERNS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, global_move, "a\nb\nc\nd", ":g/b/m$<CR>");
neovim_test!(scenarios, global_copy, "a\nb\nc", ":g/a/co$<CR>");
neovim_test!(scenarios, global_norm, "a\nb\nc", ":g/./norm A!<CR>");
neovim_test!(scenarios, global_join, "a\n1\nb\n2\nc", ":g/^[a-z]/j<CR>");
neovim_test!(scenarios, global_inverse_norm, "keep\ndel\nkeep", ":v/keep/d<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// MORE :norm PATTERNS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, norm_delete_word, "hello world\nfoo bar", ":%norm dw<CR>");
neovim_test!(scenarios, norm_change_word, "old\nold\nold", ":%norm cwne w<CR>");
neovim_test!(scenarios, norm_indent, "a\nb\nc", ":%norm >><CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// EX COMMAND CHAINING / EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, ex_delete_current, "l1\nl2\nl3", ":d<CR>");
neovim_test!(scenarios, ex_delete_all, "l1\nl2\nl3", ":%d<CR>");
neovim_test!(scenarios, ex_move_to_zero, "l1\nl2\nl3", ":3m0<CR>");
neovim_test!(scenarios, ex_copy_to_zero, "l1\nl2\nl3", ":3co0<CR>");
neovim_test!(scenarios, ex_join_with_bang, "hello\nworld", ":j!<CR>");
neovim_test!(scenarios, ex_sort_with_pattern, "b:2\na:1\nc:3", ":%sort /:\\d/<CR>");
neovim_test!(scenarios, ex_line_range_relative, "l1\nl2\nl3\nl4", ":2,+1d<CR>");
neovim_test!(scenarios, ex_current_line_relative, "l1\nl2\nl3\nl4", cursor(1, 0), ":.,.+1d<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// :w, :q, :wq (WRITE/QUIT — TESTING PARSE ONLY)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, ex_write, "hello", ":w<CR>");
neovim_test!(scenarios, ex_quit, "hello", ":q<CR>");
neovim_test!(scenarios, ex_wq, "hello", ":wq<CR>");
neovim_test!(scenarios, ex_quit_force, "hello", ":q!<CR>");
neovim_test!(scenarios, ex_write_quit, "hello", ":x<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// :registers, :marks, :jumps (DISPLAY COMMANDS)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, ex_registers, "hello", "yy:registers<CR>");
neovim_test!(scenarios, ex_reg, "hello", "yy:reg<CR>");
neovim_test!(scenarios, ex_marks, "hello", "ma:marks<CR>");
neovim_test!(scenarios, ex_jumps, "hello\nworld", "/world<CR>:jumps<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// :earlier / :later (TIME-BASED UNDO)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, ex_earlier_count, "hello", "cwworld<Esc>:earlier 1<CR>");
neovim_test!(scenarios, ex_later_count, "hello", "cwworld<Esc>:earlier 1<CR>:later 1<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// :left / :right / :center (TEXT ALIGNMENT)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, ex_left_basic, "    hello", ":%left<CR>");
neovim_test!(scenarios, ex_left_with_indent, "    hello", ":%left 2<CR>");
neovim_test!(scenarios, ex_left_multiline, "    hello\n    world", ":%left<CR>");
neovim_test!(scenarios, ex_right_basic, "hello", ":%right 20<CR>");
neovim_test!(scenarios, ex_right_multiline, "hello\nworld", ":%right 20<CR>");
neovim_test!(scenarios, ex_center_basic, "hello", ":%center 20<CR>");
neovim_test!(scenarios, ex_center_multiline, "hello\nworld", ":%center 20<CR>");
neovim_test!(scenarios, ex_left_range, "    line1\n    line2\n    line3", ":1,2left<CR>");
neovim_test!(scenarios, ex_right_range, "line1\nline2\nline3", ":1,2right 20<CR>");
neovim_test!(scenarios, ex_center_range, "line1\nline2\nline3", ":1,2center 20<CR>");

// E2E gap verification against Neovim oracle
neovim_test!(scenarios, dot_repeat_O_above, "footer\n", "Oheader<Esc>G.");
neovim_test!(scenarios, sub_regex_star_greedy, "aabbb", ":s/ab*/replaced/<CR>");
neovim_test!(scenarios, global_delete_all_match, "aaa\naaa\naaa", ":g/aaa/d<CR>");
neovim_test!(scenarios, sub_global_then_undo, "foo foo\nfoo foo", ":%s/foo/bar/g<CR>u");
neovim_test!(scenarios, macro_search_replay, "foo bar foo baz foo", "qa/foo<CR>qw@a");
