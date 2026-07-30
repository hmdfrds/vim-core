// Scenario fidelity tests: Long Editing Sessions
//
// Simulates realistic, sustained editing sequences of 20-50+ keystrokes each.
// These model what a technical writer or developer actually does during a long
// session: writing prose, fixing it, restructuring, copy-modify patterns,
// search-and-replace workflows, macro-driven bulk edits, register juggling,
// interleaved insert/normal transitions, and complex undo/redo choreography.

// =============================================================================
// WRITE A PARAGRAPH FROM SCRATCH
// =============================================================================

// Type a three-line paragraph in insert mode with <CR> line breaks
neovim_test!(scenarios, les_write_paragraph_from_scratch,
    "",
    "iThis is the first line of a paragraph.<CR>It continues with a second sentence here.<CR>And the third line wraps it all up.<Esc>");

// Type a Rust doc comment block from an empty buffer
neovim_test!(scenarios, les_write_doc_comment_block,
    "",
    "i/// Processes the input buffer and returns parsed tokens.<CR>/// <CR>/// # Arguments<CR>/// <CR>/// * `input` - A byte slice containing raw data.<Esc>");

// Type a markdown list then go back and add a title above it
neovim_test!(scenarios, les_write_list_then_add_title,
    "",
    "i- First item in the list<CR>- Second item in the list<CR>- Third item in the list<Esc>ggO# Shopping List<CR><Esc>");

// =============================================================================
// WRITE THEN EDIT: TYPE, ESCAPE, NAVIGATE BACK, FIX
// =============================================================================

// Write a sentence, escape, go back to fix "teh" -> "the", then continue
neovim_test!(scenarios, les_write_fix_typo_continue,
    "",
    "iTeh quick brown fox jumps over the lazy dog.<Esc>0cwThe<Esc>A And so it goes.<Esc>");

// Type two lines, realize first line has a mistake, go fix it, come back
neovim_test!(scenarios, les_write_two_lines_fix_first,
    "",
    "iFunction signaure is wrong.<CR>But the body is fine.<Esc>gg$bbciwsignature<Esc>GA Totally fine.<Esc>");

// Write a function signature, escape, go back and add "pub" then add the body
neovim_test!(scenarios, les_write_fn_then_add_pub_and_body,
    "",
    "ifn process(data: &[u8]) -> Result<()> {<CR>    todo!()<CR>}<Esc>ggIpub <Esc>jA // implement later<Esc>");

// Type a struct, realize a field name is wrong, go fix it
neovim_test!(scenarios, les_write_struct_fix_field,
    "",
    "istruct Config {<CR>    nme: String,<CR>    port: u16,<CR>}<Esc>2ggwciwname<Esc>");

// =============================================================================
// BUILD UP TEXT INCREMENTALLY
// =============================================================================

// Add three lines one at a time with o, then go back and edit each
neovim_test!(scenarios, les_incremental_three_lines_edit_each,
    "",
    "iFirst<Esc>oSecond<Esc>oThird<Esc>ggciwAlpha<Esc>jciwBeta<Esc>jciwGamma<Esc>");

// Build a list incrementally, then number each item
neovim_test!(scenarios, les_build_list_then_number,
    "",
    "iApples<Esc>oOranges<Esc>oBananas<Esc>ggI1. <Esc>jI2. <Esc>jI3. <Esc>");

// Write a TODO list, mark items as done by prepending [x]
neovim_test!(scenarios, les_build_todo_mark_done,
    "",
    "i[ ] Write tests<CR>[ ] Fix linter<CR>[ ] Ship release<Esc>ggf r x<Esc>jf r x<Esc>");

// =============================================================================
// SEARCH-EDIT-CONTINUE: /word, cw, n.n.
// =============================================================================

// Search for "old" and replace each occurrence with "new" using n.
neovim_test!(scenarios, les_search_replace_with_dot,
    "The old bridge crosses the old river near the old town.",
    "/old<CR>cwnew<Esc>n.n.");

// Search for a function name, change it, repeat across occurrences
neovim_test!(scenarios, les_search_rename_function,
    "call process_data() then process_data() and process_data() again",
    "/process_data<CR>ciwhandle_input<Esc>n.n.");

// Search for "TODO" comments and replace with "DONE"
neovim_test!(scenarios, les_search_todo_to_done,
    "// TODO: fix parser\ncode();\n// TODO: add tests\nmore();\n// TODO: update docs",
    "/TODO<CR>cwDONE<Esc>n.n.");

// =============================================================================
// REFORMAT PROSE: BREAK LONG LINES
// =============================================================================

// Type a long line, then break it at word boundaries with i<CR><Esc>
neovim_test!(scenarios, les_break_long_line_at_boundaries,
    "This is a very long line that should be broken into multiple shorter lines for readability.",
    "f,la<CR><Esc>jf f a<CR><Esc>");

// Join three short lines into one, then re-break at a better point
neovim_test!(scenarios, les_join_then_rebreak,
    "The quick\nbrown fox\njumps over",
    "JJFbi<CR><Esc>");

// =============================================================================
// MOVE PARAGRAPHS: YANK, NAVIGATE, PASTE, DELETE ORIGINAL
// =============================================================================

// Yank a line, move down, paste it, delete the original
neovim_test!(scenarios, les_move_line_to_bottom,
    "move this line\nkeep first\nkeep second\nkeep third",
    "yyGpggdd");

// Move a two-line block from top to bottom of document
neovim_test!(scenarios, les_move_block_to_end,
    "relocate me\nalso me\nstay here\nstay too\nanchor",
    "Vjy4jpggjdd");

// Cut the last line and paste it at the top
neovim_test!(scenarios, les_cut_last_paste_top,
    "second line\nthird line\nfirst line",
    "GddggP");

// =============================================================================
// MULTI-LINE MACRO EDIT: ADD PREFIX WITH qa...q@a
// =============================================================================

// Record a macro to add "- " prefix, replay it on 4 more lines
neovim_test!(scenarios, les_macro_add_bullet_prefix,
    "Apples\nBananas\nCherries\nDates\nElderberries",
    "qaI- <Esc>jq4@a");

// Record a macro to comment out lines with //, replay on next lines
neovim_test!(scenarios, les_macro_comment_lines,
    "let a = 1;\nlet b = 2;\nlet c = 3;\nlet d = 4;\nlet e = 5;",
    "qaI// <Esc>jq4@a");

// Record a macro to append semicolons and move down, replay
neovim_test!(scenarios, les_macro_append_semicolons,
    "let a = 1\nlet b = 2\nlet c = 3\nlet d = 4",
    "qaA;<Esc>jq3@a");

// Record a macro to wrap each line in quotes
neovim_test!(scenarios, les_macro_wrap_in_quotes,
    "alpha\nbeta\ngamma\ndelta",
    "qaI\"<Esc>A\"<Esc>jq3@a");

// =============================================================================
// WRITE CODE THEN REFACTOR WITH :%s
// =============================================================================

// Type a function using a bad name, then rename with substitute
neovim_test!(scenarios, les_write_fn_then_substitute_rename,
    "",
    "ifn foo(x: i32) -> i32 {<CR>    foo(x - 1) + x<CR>}<Esc>:%s/foo/factorial/g<CR>");

// Write code with a placeholder then replace it globally
neovim_test!(scenarios, les_write_with_placeholder_then_replace,
    "",
    "ilet XXX = get_value();<CR>process(XXX);<CR>println!(\"{}\", XXX);<Esc>:%s/XXX/result/g<CR>");

// =============================================================================
// DRAFT EDITING: WRITE, DELETE, REWRITE, UNDO, KEEP
// =============================================================================

// Write three lines, delete the middle one, rewrite it, undo that, keep original
neovim_test!(scenarios, les_draft_delete_rewrite_undo,
    "",
    "iLine one is good.<CR>Line two is bad.<CR>Line three is fine.<Esc>2Gddoorrr Line two is okay.<Esc>u");

// Write text, delete a word, undo the delete, then delete a different word
neovim_test!(scenarios, les_draft_undo_partial_rework,
    "The quick brown fox jumps over the lazy dog.",
    "wdawu2wdaw");

// Write a sentence, regret it entirely, undo back to start, type something new
neovim_test!(scenarios, les_draft_undo_everything_retype,
    "",
    "iThis was a bad idea.<Esc>uiThis is much better.<Esc>");

// =============================================================================
// COPY-MODIFY PATTERN: yyp THEN cw ON EACH LINE
// =============================================================================

// Duplicate a line 3 times, change the key word on each copy
neovim_test!(scenarios, les_copy_modify_enum_variants,
    "    Red => \"#FF0000\",",
    "yy2pjwciwGreen<Esc>$bcw\"#00FF00\"<Esc>jwciwBlue<Esc>$bcw\"#0000FF\"<Esc>");

// Duplicate a struct field line and change name and type
neovim_test!(scenarios, les_copy_modify_struct_fields,
    "    width: f32,",
    "yypwciwheight<Esc>yypwciwdepth<Esc>");

// Duplicate a test function skeleton and rename
neovim_test!(scenarios, les_copy_modify_test_fns,
    "#[test]\nfn test_add() {\n    assert!(true);\n}",
    "Vjjjyp4Gwciwtest_sub<Esc>");

// =============================================================================
// INTERLEAVED INSERT/NORMAL: BUILD UP EDITS IN-PLACE
// =============================================================================

// Wrap a word in parentheses: go before, insert (, go after, insert )
neovim_test!(scenarios, les_interleave_wrap_parens,
    "let x = value + 1;",
    "wwi(<Esc>ea)<Esc>");

// Add quotes around a word, then add a function call wrapper
neovim_test!(scenarios, les_interleave_quote_then_wrap_fn,
    "let name = hello;",
    "f=lllwi\"<Esc>ea\"<Esc>F\"iString::from(<Esc>f\"a)<Esc>");

// Insert text at multiple positions: prefix, middle, suffix
neovim_test!(scenarios, les_interleave_prefix_middle_suffix,
    "fn process data",
    "Ipub <Esc>f ea(<Esc>A)<Esc>");

// Build up a complex expression by repeatedly entering and leaving insert mode
neovim_test!(scenarios, les_interleave_build_expression,
    "x",
    "iresult = <Esc>ea.parse()<Esc>A.unwrap();<Esc>Ilet <Esc>");

// =============================================================================
// COMPLEX REGISTER DANCE: NAMED REGISTERS
// =============================================================================

// Yank two different words to named registers, paste both in new locations
neovim_test!(scenarios, les_register_dance_two_words,
    "alpha beta\ntarget: _ and _",
    "\"ayiww\"byiwj/_ <CR>\"bP/_ <CR>\"aP");

// Yank a line to "a, yank another to "b, then assemble from both
neovim_test!(scenarios, les_register_dance_assemble_lines,
    "first line\nsecond line\n",
    "\"ayyjj\"ayjj\"byyG\"ap\"bp");

// =============================================================================
// VISUAL BLOCK INSERT/APPEND
// =============================================================================

// Use visual block to add a prefix to multiple lines
neovim_test!(scenarios, les_vblock_add_prefix,
    "item one\nitem two\nitem three\nitem four\nitem five",
    "<C-v>4jI- <Esc>");

// Use visual block to add a suffix to lines of different lengths
neovim_test!(scenarios, les_vblock_add_suffix,
    "short\nmedium text\nvery long content\nok",
    "<C-v>3j$A;<Esc>");

// =============================================================================
// INDENTATION WORKFLOW
// =============================================================================

// Type code, indent it, add more above, align
neovim_test!(scenarios, les_indent_type_code_then_indent,
    "fn main() {\nlet x = 1;\nlet y = 2;\n}",
    "j>>j>>ggjolet z = x + y;<Esc>>>");

// Write a function body, indent all body lines, add a return
neovim_test!(scenarios, les_indent_fn_body_add_return,
    "fn compute(a: i32, b: i32) -> i32 {\na + b\n}",
    "j>>oreturn a * b;<Esc>>>");

// Indent a visual selection, then outdent one line that went too far
neovim_test!(scenarios, les_indent_visual_then_fix,
    "if true {\nfirst\nsecond\nthird\n}",
    "jV2j>jV>k<<");

// =============================================================================
// LONG REALISTIC SESSIONS: 30+ KEYSTROKES
// =============================================================================

// Simulate writing a changelog entry: add header, list items, fix typos
neovim_test!(scenarios, les_changelog_entry,
    "",
    "i## v2.1.0<CR><CR>- Added new parser backend<CR>- Fixed memoyr leak in tokenizer<CR>- Improved error messages<Esc>2kfmciwmemory<Esc>");

// Write an entire Rust function from scratch, with mistakes and fixes
neovim_test!(scenarios, les_write_fn_with_fixes,
    "",
    "ipub fn validate(input: &str) -> bool {<CR>    if input.is_empty() {<CR>        return flase;<CR>    }<CR>    true<CR>}<Esc>3gg$bbciwfalse<Esc>");

// Restructure a document: move the last paragraph to the top
neovim_test!(scenarios, les_restructure_document,
    "Introduction paragraph here.\n\nMiddle content stays.\n\nConclusion goes first.",
    "GVky1GP");

// Complex editing: duplicate a block, modify the copy, delete original
neovim_test!(scenarios, les_duplicate_modify_delete_original,
    "fn old_handler(req: Request) -> Response {\n    process(req)\n}",
    "Vjjyp3Gwcwnew_handler<Esc>jciwvalidate<Esc>4kVjjd");

// Write multiple struct variants from a template using yank-paste-modify
neovim_test!(scenarios, les_generate_variants_from_template,
    "    Success(String),",
    "yy3pjwciwFailure<Esc>jwciwPending<Esc>jwciwTimeout<Esc>");

// Fix multiple issues in existing code: rename, add return type, fix body
neovim_test!(scenarios, les_multi_fix_existing_code,
    "fn procss(data: &str) {\n    data.len()\n}",
    "wciwprocess<Esc>f{i -> usize <Esc>jI    return <Esc>A;<Esc>");

// Write a test, copy it, modify the copy for a different case
neovim_test!(scenarios, les_write_test_then_duplicate_variant,
    "",
    "i#[test]<CR>fn test_empty_input() {<CR>    assert_eq!(parse(\"\"), None);<CR>}<Esc>VkkkyjGpjwciwtest_valid_input<Esc>jf(ci(parse(\"hello\"), Some(\"hello\")<Esc>");

// Navigate through a file fixing the same typo in multiple places
neovim_test!(scenarios, les_fix_repeated_typo_throughout,
    "Use the recieve function.\nThe recieve buffer is full.\nCall recieve again.\nCheck recieve status.",
    "/recieve<CR>cwreceive<Esc>n.n.n.");

// Build a markdown table incrementally
neovim_test!(scenarios, les_build_markdown_table,
    "",
    "i| Name  | Value |<CR>|-------|-------|<CR>| alpha | 1     |<CR>| beta  | 2     |<CR>| gamma | 3     |<Esc>");
