use vim_test::prelude::*;

// ── vim_spec! standalone ────────────────────────────────────

vim_spec!(spec_cursor_right, "|hello", "l" => "h|ello");
vim_spec!(spec_delete_word, "|hello world", "dw" => "|world");
vim_spec!(spec_smoke_test, "|hello", "jjj");
vim_spec!(spec_insert_mode, "|hello", "i", mode: Mode::Insert);
vim_spec!(spec_delete_with_reg, "|hello world", "dw" => "|world", reg('"' => "hello "));
vim_spec!(spec_change_word, "|hello world", "cw" => "| world", mode: Mode::Insert);

// ── vim_suite! ──────────────────────────────────────────────

vim_suite!(motions_h {
    basic:       "hel|lo",       "h"  => "he|llo";
    at_start:    "|hello",       "h"  => "|hello";
    at_end:      "hell|o",       "h"  => "hel|lo";
});

vim_suite!(motions_l {
    basic:       "|hello",       "l"  => "h|ello";
    at_end:      "hell|o",       "l"  => "hell|o";
});

vim_suite!(operators {
    dd_basic:    "|hello\nworld",  "dd" => "|world";
    x_basic:     "|hello",         "x"  => "|ello";
    yank_word:   "|hello world",   "yw" => "|hello world",
                 reg('"' => "hello ");
});

vim_suite!(mode_transitions {
    enter_insert:    "|hello", "i", mode: Mode::Insert;
    visual_char:     "|hello", "v", mode: Mode::Visual(VisualType::Char);
});
