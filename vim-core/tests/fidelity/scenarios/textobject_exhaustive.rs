// Exhaustive Text Object fidelity tests.
//
// Realistic code snippets exercising every text object category with
// diverse operators, counts, visual mode, and boundary conditions.
//
// Categories covered:
//   iw/aw      — inner/a word in code contexts
//   iW/aW      — WORD on URLs, file paths, email addresses
//   i"/a"      — double quotes: nested, escaped, empty
//   i'/a'      — single quotes: nested, escaped, empty
//   i(/a(      — parentheses: nested, empty, multiline
//   i[/a[      — square brackets: nested, empty, multiline
//   i{/a{      — braces: nested, empty, multiline
//   it/at      — HTML tags: nested, self-closing, attributes
//   ip/ap      — paragraphs: multiple blank-line separators
//   is/as      — sentences: period boundaries, abbreviations
//   i</a<      — angle brackets: generics, XML tags
//   Operators  — d/c/y/v/gU/gu with text objects
//   Counts     — d2iw, c3aw, etc.
//   Boundaries — cursor on delimiter, inside empty delimiters
//   Visual ext — viw then extend with iw

// =============================================================================
// iw / aw  --  WORD OBJECTS IN REALISTIC CODE
// =============================================================================

// Cursor at start of identifier in a function signature
neovim_test!(scenarios, tobj_iw_fn_param_start,
    "fn process(input: &str) -> Result<(), Error> {",
    cursor(0, 3), "diw");

// Cursor in the middle of a long identifier
neovim_test!(scenarios, tobj_iw_middle_of_ident,
    "let configuration_manager = ConfigManager::new();",
    cursor(0, 12), "diw");

// Cursor at end of word just before punctuation
neovim_test!(scenarios, tobj_iw_before_punct,
    "vec.push(value);",
    cursor(0, 2), "diw");

// aw grabs trailing space in a comma-separated list
neovim_test!(scenarios, tobj_aw_comma_list,
    "let items = vec![alpha, beta, gamma];",
    cursor(0, 17), "daw");

// iw on punctuation character itself (comma)
neovim_test!(scenarios, tobj_iw_on_comma,
    "foo(a, b, c)",
    cursor(0, 5), "diw");

// aw on the last word of a line (no trailing space -- takes leading)
neovim_test!(scenarios, tobj_aw_last_word_of_line,
    "let result = compute(x, y)",
    cursor(0, 25), "daw");

// ciw to rename a variable in an assignment
neovim_test!(scenarios, tobj_ciw_rename_variable,
    "let counter = 0;",
    cursor(0, 4), "ciwindex<Esc>");

// yiw + paste: duplicate a word
neovim_test!(scenarios, tobj_yiw_duplicate,
    "fn handle_request(request: Request) {",
    cursor(0, 3), "yiwea_<Esc>p");

// =============================================================================
// iW / aW  --  WORD OBJECTS ON URLS, PATHS, EMAILS
// =============================================================================

// WORD on a full URL (punctuation stays attached)
neovim_test!(scenarios, tobj_iW_url,
    "visit https://docs.rs/vim-core/latest for details",
    cursor(0, 10), "diW");

// WORD on a Unix file path
neovim_test!(scenarios, tobj_iW_filepath,
    "source /home/user/.config/nvim/init.lua here",
    cursor(0, 10), "diW");

// aW on an email address (grabs trailing space)
neovim_test!(scenarios, tobj_aW_email,
    "contact admin@example.com for support",
    cursor(0, 10), "daW");

// ciW to replace a qualified Rust path
neovim_test!(scenarios, tobj_ciW_rust_path,
    "use std::collections::HashMap;",
    cursor(0, 8), "ciWstd::collections::BTreeMap<Esc>");

// =============================================================================
// i" / a"  --  DOUBLE QUOTE OBJECTS
// =============================================================================

// ci" inside a string literal in a function call
neovim_test!(scenarios, tobj_ci_dquote_string_literal,
    "println!(\"Hello, world!\");",
    cursor(0, 15), "ci\"Goodbye!<Esc>");

// di" with escaped quotes inside
neovim_test!(scenarios, tobj_di_dquote_escaped_inner,
    "let s = \"she said \\\"hi\\\"\";",
    cursor(0, 12), "di\"");

// a" on empty string literal
neovim_test!(scenarios, tobj_da_dquote_empty_string,
    "let s = \"\";",
    cursor(0, 8), "da\"");

// di" with cursor before the first quote (forward seek)
neovim_test!(scenarios, tobj_di_dquote_seek_forward,
    "let msg = \"error\";",
    cursor(0, 4), "di\"");

// Nested single quotes inside double quotes
neovim_test!(scenarios, tobj_di_dquote_contains_squotes,
    "let cmd = \"echo 'hello'\";",
    cursor(0, 15), "di\"");

// =============================================================================
// i' / a'  --  SINGLE QUOTE OBJECTS
// =============================================================================

// ci' in a Python/shell-style string
neovim_test!(scenarios, tobj_ci_squote_shell_string,
    "LANG='en_US.UTF-8'",
    cursor(0, 8), "ci'C.UTF-8<Esc>");

// di' with adjacent pairs on the same line
neovim_test!(scenarios, tobj_di_squote_adjacent_pairs,
    "['first', 'second', 'third']",
    cursor(0, 12), "di'");

// a' removes the quotes and trailing space
neovim_test!(scenarios, tobj_da_squote_in_list,
    "items = ['alpha', 'beta']",
    cursor(0, 11), "da'");

// =============================================================================
// i( / a(  --  PARENTHESIS OBJECTS IN CODE
// =============================================================================

// di( on a nested function call (inner parens)
neovim_test!(scenarios, tobj_di_paren_nested_call,
    "result = outer(inner(x, y), z)",
    cursor(0, 22), "di(");

// da( removes an entire argument sub-expression
neovim_test!(scenarios, tobj_da_paren_remove_subexpr,
    "compute(transform(data), flag)",
    cursor(0, 20), "da(");

// ci( on empty parens (function call with no args)
neovim_test!(scenarios, tobj_ci_paren_empty_call,
    "let v = Vec::new();",
    cursor(0, 16), "ci(42<Esc>");

// Multiline function arguments -- cursor inside
neovim_test!(scenarios, tobj_di_paren_multiline_args,
    "fn build(\n    name: &str,\n    age: u32,\n) -> Self {",
    cursor(1, 8), "di(");

// Cursor on the opening paren
neovim_test!(scenarios, tobj_di_paren_on_open_delim,
    "call(arg1, arg2)",
    cursor(0, 4), "di(");

// Cursor on the closing paren
neovim_test!(scenarios, tobj_di_paren_on_close_delim,
    "call(arg1, arg2)",
    cursor(0, 15), "di(");

// =============================================================================
// i[ / a[  --  SQUARE BRACKET OBJECTS
// =============================================================================

// di[ inside a JSON array
neovim_test!(scenarios, tobj_di_bracket_json_array,
    "{\"tags\": [\"rust\", \"vim\", \"editor\"]}",
    cursor(0, 14), "di[");

// a[ removes the brackets and content in a Rust index
neovim_test!(scenarios, tobj_da_bracket_rust_index,
    "let ch = text[idx];",
    cursor(0, 15), "da[");

// Multiline array literal
neovim_test!(scenarios, tobj_di_bracket_multiline_array,
    "let arr = [\n    1,\n    2,\n    3,\n];",
    cursor(2, 4), "di[");

// =============================================================================
// i{ / a{  --  BRACE OBJECTS IN CODE
// =============================================================================

// di{ inside a Rust function body
neovim_test!(scenarios, tobj_di_brace_fn_body,
    "fn main() {\n    println!(\"hi\");\n}",
    cursor(1, 4), "di{");

// ci{ to rewrite a match arm body
neovim_test!(scenarios, tobj_ci_brace_match_arm,
    "match x {\n    1 => { first(); }\n    _ => { other(); }\n}",
    cursor(1, 14), "ci{replaced<Esc>");

// Nested braces: cursor in inner braces
neovim_test!(scenarios, tobj_di_brace_nested_inner,
    "if cond { if nested { value } }",
    cursor(0, 23), "di{");

// a{ on the outer brace from just inside
neovim_test!(scenarios, tobj_da_brace_outer_from_inner,
    "{ outer { inner } }",
    cursor(0, 3), "da{");

// =============================================================================
// it / at  --  HTML TAG OBJECTS
// =============================================================================

// dit on a tag with attributes
neovim_test!(scenarios, tobj_dit_with_attrs,
    "<div class=\"container\" id=\"main\">content</div>",
    cursor(0, 35), "dit");

// dat removes the entire element including tags
neovim_test!(scenarios, tobj_dat_full_element,
    "<ul><li class=\"item\">Entry</li></ul>",
    cursor(0, 22), "dat");

// Nested tags: operate on the inner tag
neovim_test!(scenarios, tobj_dit_nested_tags,
    "<section><article><p>text</p></article></section>",
    cursor(0, 21), "dit");

// cit to replace content of a multiline tag
neovim_test!(scenarios, tobj_cit_multiline_tag,
    "<div>\n  <span>old text</span>\n</div>",
    cursor(1, 10), "citnew text<Esc>");

// dit on self-closing tag (should be no-op or empty)
neovim_test!(scenarios, tobj_dit_self_closing,
    "<img src=\"photo.jpg\" />",
    cursor(0, 5), "dit");

// =============================================================================
// ip / ap  --  PARAGRAPH OBJECTS
// =============================================================================

// dip on the first paragraph of a multi-paragraph buffer
neovim_test!(scenarios, tobj_dip_first_of_three,
    "First paragraph\nstill first\n\nSecond paragraph\n\nThird paragraph",
    cursor(0, 0), "dip");

// dap includes the trailing blank lines
neovim_test!(scenarios, tobj_dap_includes_trailing_blanks,
    "Header line\n\n\n\nBody starts here\nand continues",
    cursor(0, 0), "dap");

// Cursor on blank line between paragraphs
neovim_test!(scenarios, tobj_dip_on_blank_separator,
    "Above\n\n\nBelow",
    cursor(1, 0), "dip");

// ap on the last paragraph (takes leading blank lines)
neovim_test!(scenarios, tobj_dap_last_para,
    "First\n\nLast paragraph here",
    cursor(2, 0), "dap");

// =============================================================================
// is / as  --  SENTENCE OBJECTS
// =============================================================================

// dis on a sentence after an abbreviation
neovim_test!(scenarios, tobj_dis_after_abbreviation,
    "Dr. Smith arrived.  He greeted everyone.",
    cursor(0, 22), "dis");

// das with multiple sentences on one line
neovim_test!(scenarios, tobj_das_multi_sentence_line,
    "Call init().  Then run().  Finally cleanup().",
    cursor(0, 15), "das");

// Sentence ending with closing paren then period
neovim_test!(scenarios, tobj_dis_paren_period,
    "See the docs (appendix A).  Next topic.",
    cursor(0, 5), "dis");

// =============================================================================
// i< / a<  --  ANGLE BRACKET OBJECTS
// =============================================================================

// di< on Rust generics
neovim_test!(scenarios, tobj_di_angle_rust_generic,
    "fn parse<T: FromStr>(input: &str) -> T {",
    cursor(0, 12), "di<lt>");

// a< removes the angle brackets and content
neovim_test!(scenarios, tobj_da_angle_remove_generic,
    "let map: HashMap<String, Vec<u8>> = HashMap::new();",
    cursor(0, 22), "da<lt>");

// Nested angle brackets in a complex generic
neovim_test!(scenarios, tobj_di_angle_nested_generic,
    "Box<dyn Iterator<Item = u8>>",
    cursor(0, 20), "di<lt>");

// =============================================================================
// OPERATORS WITH TEXT OBJECTS
// =============================================================================

// gUiw: uppercase a word in a mixed-case identifier
neovim_test!(scenarios, tobj_gU_iw_uppercase_ident,
    "let warning_message = \"alert\";",
    cursor(0, 4), "gUiw");

// guiw: lowercase a shouting constant
neovim_test!(scenarios, tobj_gu_iw_lowercase_const,
    "const MAX_RETRIES: u32 = 5;",
    cursor(0, 6), "guiw");

// viw then delete: visual select word then remove
neovim_test!(scenarios, tobj_viw_then_delete,
    "remove this_word from the line",
    cursor(0, 7), "viwd");

// =============================================================================
// COUNT + TEXT OBJECT
// =============================================================================

// d2iw deletes two words from a parameter list
neovim_test!(scenarios, tobj_d2iw_params,
    "fn run(alpha, beta, gamma) {",
    cursor(0, 7), "d2iw");

// c3aw changes three words
neovim_test!(scenarios, tobj_c3aw_rewrite,
    "the quick brown fox jumps",
    cursor(0, 0), "c3awsome<Esc>");

// =============================================================================
// VISUAL + TEXT OBJECT EXTENSION
// =============================================================================

// viw then iw to extend selection across words
neovim_test!(scenarios, tobj_viw_extend_iw,
    "one two three four five",
    cursor(0, 4), "viwiwld");

// vi( then i( to grow from inner to outer parens
neovim_test!(scenarios, tobj_vi_paren_extend_to_outer,
    "((deeply (nested)))",
    cursor(0, 10), "vi(i(d");
