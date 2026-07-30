// Scenario fidelity tests: Advanced Mark Workflows
//
// Using marks for navigation, editing ranges, and complex patterns.

// ═══════════════════════════════════════════════════════════════════════════════
// BASIC MARK SET AND JUMP
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, mark_set_jump, "l1\nl2\nl3\nl4\nl5", "ma2j'a");
neovim_test!(scenarios, mark_backtick, "hello world", cursor(0, 5), "maw`a");
neovim_test!(scenarios, mark_multiple, "l1\nl2\nl3\nl4", "maj2jmb'a'b");
neovim_test!(scenarios, mark_persist_edit, "hello\nworld\nfoo", "jmayyddGp'a");

// ═══════════════════════════════════════════════════════════════════════════════
// DELETE/YANK TO MARK
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, ma_delete_to_mark, "l1\nl2\nl3\nl4\nl5", "3Gma1Gd'a");
neovim_test!(scenarios, ma_yank_to_mark, "l1\nl2\nl3\nl4", "3Gma1Gy'aGp");
neovim_test!(scenarios, ma_change_to_mark, "l1\nl2\nl3\nl4", "2Gma0Gc'anew<Esc>");
neovim_test!(scenarios, delete_backtick, "hello cruel world", cursor(0, 6), "maw0d`a");

// ═══════════════════════════════════════════════════════════════════════════════
// SPECIAL MARKS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, last_edit_mark, "hello world", "cwfoo<Esc>G`.");
neovim_test!(scenarios, last_jump_mark, "l1\nl2\nl3\nl4\nl5", "4G1G''");
neovim_test!(scenarios, last_insert_mark, "hello", "Aworld<Esc>0`^");
neovim_test!(scenarios, start_of_last_yank, "hello world foo", "wyw`[");
neovim_test!(scenarios, end_of_last_yank, "hello world foo", "wyw`]");

// ═══════════════════════════════════════════════════════════════════════════════
// MARK-BASED EDITING WORKFLOWS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, mark_bookmark_edit, "fn start() {}\n\nfn target() {}\n\nfn end() {}", "2jmawcwmodified<Esc>'a");
neovim_test!(scenarios, ma_mark_yank_range, "header\ndata1\ndata2\ndata3\nfooter", "jma2jmby'aGp");
neovim_test!(scenarios, mark_swap_lines, "line_b\nline_a", "majdd'aP");
neovim_test!(scenarios, mark_delete_between, "keep\ndelete1\ndelete2\nkeep", "jma2j'ad'a");
neovim_test!(scenarios, mark_insert_at, "remember\nforget\nrecall", "majo inserted<Esc>`a");

// ═══════════════════════════════════════════════════════════════════════════════
// MARKS WITH VISUAL MODE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, visual_to_mark, "l1\nl2\nl3\nl4", "2Gmav'ad");
neovim_test!(scenarios, visual_mark_indent, "l1\nl2\nl3", "2Gmav'a>");
neovim_test!(scenarios, visual_mark_uppercase, "l1\nl2\nl3", "mav2j'agU");
