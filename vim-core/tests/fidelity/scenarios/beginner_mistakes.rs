// Scenario fidelity tests: Beginner Mistakes
//
// Simulates real mistakes a Vim beginner makes and their recovery attempts.
// These tests capture the chaotic, confused keystrokes of someone who doesn't
// yet have Vim's modal editing internalized.

// ═══════════════════════════════════════════════════════════════════════════════
// ACCIDENTALLY ENTERING INSERT MODE
// A beginner hits 'i' by accident, types garbage, then tries to get out.
// ═══════════════════════════════════════════════════════════════════════════════

// Hit i by accident, immediately Escape
neovim_test!(scenarios, beginner_accidental_i_escape, "Hello world\nThis is a test\nLine three", "i<Esc>");

// Hit i, type a few chars thinking they're commands, then Escape
neovim_test!(scenarios, beginner_i_then_jjj_escape, "Hello world\nThis is a test\nLine three", "ijjj<Esc>");

// Enter insert mode, try to navigate with arrow keys, then Escape
neovim_test!(scenarios, beginner_insert_arrows_all, "Hello world\nThis is a test\nLine three", "i<Down><Right><Right><Up><Left><Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// SAVING AND QUITTING (command-line mode)
// Beginners instinctively type :wq or :q! to exit.
// ═══════════════════════════════════════════════════════════════════════════════

// Type :wq<CR> — the classic "I'm done" sequence
neovim_test!(scenarios, beginner_wq, "Hello world\nThis is a test\nLine three", ":wq<CR>");

// Type :q!<CR> — force quit (exits Neovim, no golden state to compare)
// neovim_test!(scenarios, beginner_q_bang, "Hello world\nThis is a test\nLine three", ":q!<CR>");

// Enter command mode then abort with Escape
neovim_test!(scenarios, beginner_colon_escape, "Hello world\nThis is a test\nLine three", ":<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// COPY-PASTE CONFUSION
// Beginner yanks a line, wanders around, then tries to paste.
// ═══════════════════════════════════════════════════════════════════════════════

// yy then move around before pasting
neovim_test!(scenarios, beginner_yy_wander_paste, "Hello world\nThis is a test\nLine three", "yyjjp");

// yank a word, navigate away, come back, paste
neovim_test!(scenarios, beginner_yw_navigate_paste, "Hello world\nThis is a test\nLine three", "ywjj0p");

// yank in visual mode, move, paste
neovim_test!(scenarios, beginner_visual_yank_move_paste, "Hello world\nThis is a test\nLine three", "vey0p");

// Copy line, paste multiple times accidentally
neovim_test!(scenarios, beginner_yy_paste_paste_paste, "Hello world\nThis is a test\nLine three", "yyppp");

// ═══════════════════════════════════════════════════════════════════════════════
// ACCIDENTAL DELETE + PANIC UNDO
// Beginner hits dd, panics, immediately mashes undo.
// ═══════════════════════════════════════════════════════════════════════════════

// Delete a line, immediately undo
neovim_test!(scenarios, beginner_dd_panic_undo, "Hello world\nThis is a test\nLine three", "ddu");

// Delete a word, undo
neovim_test!(scenarios, beginner_dw_undo, "Hello world\nThis is a test\nLine three", "dwu");

// Delete with x multiple times, undo all
neovim_test!(scenarios, beginner_xxx_undo_all, "Hello world\nThis is a test\nLine three", "xxxuuu");

// ═══════════════════════════════════════════════════════════════════════════════
// ACCIDENTAL VISUAL MODE
// Beginner enters visual mode by accident, selects stuff, then Escapes.
// ═══════════════════════════════════════════════════════════════════════════════

// Enter visual, select a bit, Escape
neovim_test!(scenarios, beginner_v_select_escape, "Hello world\nThis is a test\nLine three", "vlll<Esc>");

// Enter visual line mode by accident, select lines, Escape
neovim_test!(scenarios, beginner_V_select_lines_escape, "Hello world\nThis is a test\nLine three", "Vjj<Esc>");

// Visual mode: select text then accidentally delete it, then undo
neovim_test!(scenarios, beginner_v_select_delete_undo, "Hello world\nThis is a test\nLine three", "vllldu");

// ═══════════════════════════════════════════════════════════════════════════════
// TYPING IN NORMAL MODE (characters interpreted as commands)
// Beginner tries to type words in normal mode without entering insert first.
// ═══════════════════════════════════════════════════════════════════════════════

// Type "hello" in normal mode: h(left) e(word-end) l(right) l(right) o(open below)
neovim_test!(scenarios, beginner_type_hello_normal, "Hello world\nThis is a test\nLine three", "hello<Esc>");

// Type "the" in normal mode: t(till) h(char h) e(word-end)
neovim_test!(scenarios, beginner_type_the_normal, "Hello world\nThis is a test\nLine three", "the");

// Type "fix" in normal mode: f(find) i(char i) x(delete char)
neovim_test!(scenarios, beginner_type_fix_normal, "Hello world\nThis is a test\nLine three", "fix");

// Type "does" in normal mode: d(delete operator) o — Neovim produces E99 (diff mode error)
// which is Neovim-specific, not applicable to our single-buffer model.
// neovim_test!(scenarios, beginner_type_does_normal, "Hello world\nThis is a test\nLine three", "does<Esc>");

// Type "jar" in normal mode: j(down) a(append) r(literal 'r' in insert)
neovim_test!(scenarios, beginner_type_jar_normal, "Hello world\nThis is a test\nLine three", "jar<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// MIXED CASE COMMANDS (accidentally hitting Shift)
// D instead of d, J instead of j, A instead of a, etc.
// ═══════════════════════════════════════════════════════════════════════════════

// D instead of dd: deletes to end of line instead of whole line
neovim_test!(scenarios, beginner_shift_d_instead_of_dd, "Hello world\nThis is a test\nLine three", "D");

// J instead of j: joins lines instead of moving down
neovim_test!(scenarios, beginner_shift_j_instead_of_j, "Hello world\nThis is a test\nLine three", "J");

// O instead of o: opens line above instead of below
neovim_test!(scenarios, beginner_shift_o_instead_of_o, "Hello world\nThis is a test\nLine three", "Onew line<Esc>");

// P instead of p: pastes above instead of below
neovim_test!(scenarios, beginner_shift_p_instead_of_p, "Hello world\nThis is a test\nLine three", "yyP");

// C instead of c: changes to end of line
neovim_test!(scenarios, beginner_shift_c_instead_of_cw, "Hello world\nThis is a test\nLine three", "Creplaced<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// FORGET TO EXIT INSERT MODE BEFORE NAVIGATING
// Classic: insert some text, then try to navigate with hjkl still in insert.
// ═══════════════════════════════════════════════════════════════════════════════

// Insert text, Esc, then navigate normally
neovim_test!(scenarios, beginner_insert_forget_esc_j, "Hello world\nThis is a test\nLine three", "isome text<Esc>jjj");

// Type in insert, Esc, navigate, re-enter insert
neovim_test!(scenarios, beginner_insert_esc_nav_insert, "Hello world\nThis is a test\nLine three", "ifirst <Esc>jisecond <Esc>");

// Open line below, type, Esc, try to go back up
neovim_test!(scenarios, beginner_o_type_esc_go_up, "Hello world\nThis is a test\nLine three", "onew line<Esc>kk");

// ═══════════════════════════════════════════════════════════════════════════════
// MULTIPLE ESCAPE PRESSES (mashing Esc to "make sure")
// ═══════════════════════════════════════════════════════════════════════════════

// Triple Escape from normal mode (all no-ops)
neovim_test!(scenarios, beginner_triple_escape_normal, "Hello world\nThis is a test\nLine three", "<Esc><Esc><Esc>");

// Insert then triple Escape
neovim_test!(scenarios, beginner_insert_triple_escape, "Hello world\nThis is a test\nLine three", "itext<Esc><Esc><Esc>");

// Chain of mode entries and Escapes
neovim_test!(scenarios, beginner_mode_chain_escapes, "Hello world\nThis is a test\nLine three", "i<Esc>v<Esc>:<Esc><Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// ACCIDENTAL MACRO RECORDING AND REPLAY
// Beginner hits q and doesn't know what happened, then q again to stop,
// then accidentally hits @q and replays whatever they recorded.
// ═══════════════════════════════════════════════════════════════════════════════

// Record empty macro by hitting qq then q
neovim_test!(scenarios, beginner_accidental_qq_q, "Hello world\nThis is a test\nLine three", "qqq");

// Record a macro with some accidental keystrokes, then replay it
neovim_test!(scenarios, beginner_record_junk_replay, "Hello world\nThis is a test\nLine three", "qqlljq@q");

// Record a macro with insert mode content, replay
neovim_test!(scenarios, beginner_record_insert_replay, "Hello world\nThis is a test\nLine three", "qqihi <Esc>q@q");

// Accidentally record a destructive macro (dd), replay it
neovim_test!(scenarios, beginner_record_dd_replay, "Hello world\nThis is a test\nLine three", "qqddq@q");

// Record destructive macro, replay, panic undo
neovim_test!(scenarios, beginner_record_dd_replay_undo, "Hello world\nThis is a test\nLine three", "qqddq@quu");

// ═══════════════════════════════════════════════════════════════════════════════
// UNDO TOO MUCH / REDO RECOVERY
// Beginner mashes u thinking "go back" then needs to redo.
// ═══════════════════════════════════════════════════════════════════════════════

// Make changes, undo way too many times
neovim_test!(scenarios, beginner_undo_too_many, "Hello world\nThis is a test\nLine three", "xddxuuuuuu");

// Undo too much, then redo to recover
neovim_test!(scenarios, beginner_undo_then_redo, "Hello world\nThis is a test\nLine three", "xddxuuuuuu<C-r><C-r><C-r>");

// Make one change, undo it, redo it, undo it again (flip-flopping)
neovim_test!(scenarios, beginner_undo_redo_flipflop, "Hello world\nThis is a test\nLine three", "xu<C-r>u<C-r>u");

// Undo with count
neovim_test!(scenarios, beginner_undo_with_count, "Hello world\nThis is a test\nLine three", "xxxx4u");

// ═══════════════════════════════════════════════════════════════════════════════
// ACCIDENTAL SUBSTITUTION / SEARCH CONFUSION
// Beginner types :s when they meant / for search, or vice versa.
// ═══════════════════════════════════════════════════════════════════════════════

// Meant to search for "test" with / but typed :s/test/... instead
neovim_test!(scenarios, beginner_sub_instead_of_search, "Hello world\nThis is a test\nLine three", ":s/test/TEST/<CR>");

// Start a search, type partial pattern, abort
neovim_test!(scenarios, beginner_search_abort, "Hello world\nThis is a test\nLine three", "/wor<Esc>");

// Search backwards when they meant forwards
neovim_test!(scenarios, beginner_search_backwards, "Hello world\nThis is a test\nLine three", cursor(2, 0), "?world<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// REALISTIC MULTI-STEP BEGINNER SESSIONS
// Full scenarios of a confused beginner trying to accomplish a task.
// ═══════════════════════════════════════════════════════════════════════════════

// Beginner tries to add a word: enters insert, types, Esc, wrong spot, undo, move, re-insert
neovim_test!(scenarios, beginner_insert_wrong_spot_redo, "Hello world\nThis is a test\nLine three", "iextra <Esc>u$aextra<Esc>");

// Beginner tries to go to end of file then back to top
neovim_test!(scenarios, beginner_G_then_gg, "Hello world\nThis is a test\nLine three", "Ggg");

// Beginner accidentally enters replace mode with R, types over text, Esc, undo
neovim_test!(scenarios, beginner_replace_mode_undo, "Hello world\nThis is a test\nLine three", "RXYZ<Esc>u");

// Full confused session: try to type (normal mode), realize mistake, Esc, i, type, Esc
neovim_test!(scenarios, beginner_confused_session, "Hello world\nThis is a test\nLine three", "hello<Esc>u<Esc>iHi there<Esc>");

// Beginner makes an edit, saves, makes another edit
neovim_test!(scenarios, beginner_edit_save_edit, "Hello world\nThis is a test\nLine three", "itext <Esc>:w<CR>imore <Esc>");

// Beginner does :%s to replace globally, then undoes it
neovim_test!(scenarios, beginner_global_sub_undo, "Hello world\nThis is a test\nLine three", ":%s/is/was/g<CR>u");
