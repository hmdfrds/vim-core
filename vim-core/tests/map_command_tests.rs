//! Map and abbreviation commands as Vim's `do_map()` and `do_exmap()` run
//! them, typed and in a config.

mod common;

use common::document::TestDocument;
use vim_core::effects::Effect;
use vim_core::execution::{parse_keys_from_string, HostSession, InputContext, Response, VimEngine};
use vim_core::keymap::{KeyEvent, MappingMode};
use vim_core::primitives::Mode;

fn press(engine: &mut VimEngine, doc: &TestDocument, key: KeyEvent) -> Response {
    let ctx = InputContext::new(doc, doc.cursor_offset()).validate_clamped();
    engine.process(key, ctx)
}

fn errors(response: &Response) -> Vec<String> {
    response
        .effects()
        .iter()
        .filter_map(|e| match e {
            Effect::ShowError { error, .. } => Some(error.to_string()),
            _ => None,
        })
        .collect()
}

/// Type `:{command}<CR>` and return the errors it gave.
fn run_ex(engine: &mut VimEngine, command: &str) -> Vec<String> {
    let doc = TestDocument::new("hello", (0, 0));
    let _ = press(engine, &doc, KeyEvent::char(':'));
    for ch in command.chars() {
        let _ = press(engine, &doc, KeyEvent::char(ch));
    }
    errors(&press(engine, &doc, KeyEvent::enter()))
}

/// The Normal mode mappings: their keys as notation, their right-hand
/// side as notation, and whether they belong to the buffer.
fn normal_mappings(engine: &VimEngine) -> Vec<(String, String, bool)> {
    let notation = |keys: &[KeyEvent]| keys.iter().map(|k| k.to_vim_notation()).collect::<String>();
    let mut mappings: Vec<_> = engine
        .keymap()
        .list_mappings(MappingMode::Normal)
        .into_iter()
        .map(|(lhs, entry, buffer)| (notation(&lhs), notation(entry.sequence()), buffer))
        .collect();
    mappings.sort();
    mappings
}

fn mapping(lhs: &str, rhs: &str, buffer: bool) -> (String, String, bool) {
    (lhs.to_owned(), rhs.to_owned(), buffer)
}

fn type_keys(session: &mut HostSession, keys: &str) {
    for key in parse_keys_from_string(keys) {
        let _ = session.process_key_host(key);
    }
}

#[test]
fn a_typed_buffer_mapping_belongs_to_the_buffer() {
    let mut engine = VimEngine::new();
    assert!(run_ex(&mut engine, "nnoremap <silent><buffer> x y").is_empty());
    assert_eq!(normal_mappings(&engine), [mapping("x", "y", true)]);
    let entry = engine
        .keymap()
        .get_buffer_mapping(KeyEvent::char('x'), vim_core::primitives::Mode::Normal)
        .map(|entry| entry.silent());
    assert_eq!(entry, Some(true));

    assert!(run_ex(&mut engine, "nnoremap x z").is_empty());
    assert!(run_ex(&mut engine, "nunmap <buffer> x").is_empty());
    assert_eq!(normal_mappings(&engine), [mapping("x", "z", false)]);
}

#[test]
fn mapclear_buffer_clears_only_the_buffer_mappings() {
    let mut engine = VimEngine::new();
    run_ex(&mut engine, "nnoremap <buffer> x y");
    run_ex(&mut engine, "nnoremap z y");
    assert!(run_ex(&mut engine, "nmapclear <buffer>").is_empty());
    assert_eq!(normal_mappings(&engine), [mapping("z", "y", false)]);
    // map_clear(): any other argument is E474.
    assert_eq!(
        run_ex(&mut engine, "nmapclear x"),
        ["E474: Invalid argument"]
    );
    assert_eq!(normal_mappings(&engine), [mapping("z", "y", false)]);
}

#[test]
fn a_buffer_mapping_in_a_config_is_not_allowed() {
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("nnoremap <buffer> x y | set ic\nnunmap <buffer> x\n");
    assert_eq!(
        errors(&response),
        [
            "E523: Not allowed here: nnoremap <buffer> x y",
            "E523: Not allowed here: nunmap <buffer> x"
        ]
    );
    assert!(!engine.options().ignorecase());
    assert!(normal_mappings(&engine).is_empty());
}

#[test]
fn unique_refuses_keys_that_are_mapped() {
    let mut engine = VimEngine::new();
    run_ex(&mut engine, "nnoremap x z");
    assert_eq!(
        run_ex(&mut engine, "nnoremap <unique> x y"),
        ["E227: Mapping already exists for x"]
    );
    // A mapping that only starts with the keys is no conflict.
    assert!(run_ex(&mut engine, "nnoremap <unique> xy w").is_empty());
    // Nor is one in another mode.
    assert!(run_ex(&mut engine, "inoremap <unique> x y").is_empty());
    assert_eq!(
        run_ex(&mut engine, "nnoremap <buffer><unique> x y"),
        ["E225: Global mapping already exists for x"]
    );
    assert_eq!(
        normal_mappings(&engine),
        [mapping("x", "z", false), mapping("xy", "w", false)]
    );
    // <unique> is not kept with the mapping.
    assert!(run_ex(&mut engine, "nnoremap xy v").is_empty());
}

#[test]
fn unique_refuses_an_abbreviation_that_is_defined() {
    let mut session = HostSession::new("");
    type_keys(
        &mut session,
        ":iabbrev teh the<CR>:iabbrev <unique> teh tea<CR>",
    );
    type_keys(&mut session, "ateh <Esc>");
    assert_eq!(session.text(), "the ");

    let mut engine = VimEngine::new();
    run_ex(&mut engine, "iabbrev teh the");
    assert_eq!(
        run_ex(&mut engine, "iabbrev <unique> teh tea"),
        ["E226: Abbreviation already exists for teh"]
    );
}

#[test]
fn buffer_and_expr_abbreviations_are_not_available() {
    let mut engine = VimEngine::new();
    assert_eq!(
        run_ex(&mut engine, "iabbrev <buffer> teh the"),
        ["E319: Sorry, the command is not available in this version: iabbrev <buffer> teh the"]
    );
    assert_eq!(
        run_ex(&mut engine, "cnoreabbrev <expr> x y"),
        ["E319: Sorry, the command is not available in this version: cnoreabbrev <expr> x y"]
    );
}

#[test]
fn script_mappings_do_not_remap() {
    // <script> remaps only script-local mappings, and there are none.
    let mut session = HostSession::new("abc");
    type_keys(&mut session, ":nnoremap l x<CR>:nmap <script> q l<CR>q");
    assert_eq!(session.text(), "abc");
    let mut session = HostSession::new("abc");
    type_keys(&mut session, ":nnoremap l x<CR>:nmap q l<CR>q");
    assert_eq!(session.text(), "bc");
}

#[test]
fn a_right_hand_side_keeps_its_trailing_blanks() {
    // `nnoremap x l ` moves right twice: `l` and the space.
    let mut session = HostSession::new("abcd");
    type_keys(&mut session, ":nnoremap x l <CR>x");
    assert_eq!(session.cursor_offset(), 2);

    // The blank before a `|` belongs to the right-hand side.
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("nnoremap x y | set ic\n");
    assert!(errors(&response).is_empty(), "{:?}", errors(&response));
    assert_eq!(normal_mappings(&engine), [mapping("x", "y<Space>", false)]);
    assert!(engine.options().ignorecase());
}

#[test]
fn an_abbreviation_keeps_its_trailing_blanks() {
    let mut session = HostSession::new("");
    type_keys(&mut session, ":iabbrev teh the <CR>ateh <Esc>");
    assert_eq!(session.text(), "the  ");
}

#[test]
fn unmap_with_a_trailing_blank_names_other_keys() {
    let mut engine = VimEngine::new();
    run_ex(&mut engine, "nnoremap x y");
    assert_eq!(run_ex(&mut engine, "unmap x "), ["E31: No such mapping"]);
    assert_eq!(normal_mappings(&engine), [mapping("x", "y", false)]);
    assert!(run_ex(&mut engine, "unmap x").is_empty());
    assert!(normal_mappings(&engine).is_empty());
}

#[test]
fn unabbreviate_ignores_trailing_blanks() {
    let mut session = HostSession::new("");
    type_keys(
        &mut session,
        ":iabbrev teh the<CR>:iunabbrev teh <CR>ateh <Esc>",
    );
    assert_eq!(session.text(), "teh ");
}

#[test]
fn nop_maps_keys_to_nothing() {
    // do_map() reads `<Nop>` in any case as an empty right-hand side, and
    // handle_mapping() then inserts nothing.
    for nop in ["<Nop>", "<NOP>", "<nop>"] {
        let mut session = HostSession::new("abc");
        type_keys(&mut session, &format!(":nnoremap x {nop}<CR>xl"));
        assert_eq!(session.text(), "abc", "{nop}");
        assert_eq!(session.cursor_offset(), 1, "{nop}");
    }
}

#[test]
fn nop_leaves_the_mode_as_it_is() {
    let mut session = HostSession::new("");
    type_keys(&mut session, ":inoremap x <Nop><CR>iaxb");
    assert_eq!(session.mode(), Mode::Insert);
    type_keys(&mut session, "<Esc>");
    assert_eq!(session.text(), "ab");

    let mut session = HostSession::new("abc");
    type_keys(&mut session, ":vnoremap x <Nop><CR>vx");
    assert!(
        matches!(session.mode(), Mode::Visual(_)),
        "{:?}",
        session.mode()
    );

    let mut session = HostSession::new("abc");
    type_keys(&mut session, ":cnoremap x <Nop><CR>:s/b/xX/<CR>");
    assert_eq!(session.text(), "aXc");
}

#[test]
fn nop_with_more_text_is_keys() {
    // The whole right-hand side must be `<Nop>`.
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("nnoremap x <Nop> | nnoremap y <Nop>x\n");
    assert!(errors(&response).is_empty(), "{:?}", errors(&response));
    let mappings = normal_mappings(&engine);
    assert_eq!(mappings.len(), 2);
    assert!(
        mappings.iter().all(|(_, rhs, _)| rhs.contains("Nop")),
        "{mappings:?}"
    );
}

#[test]
fn nop_abbreviation_expands_to_nothing() {
    let mut session = HostSession::new("");
    type_keys(&mut session, ":iabbrev foo <Nop><CR>afoo bar<Esc>");
    assert_eq!(session.text(), " bar");
}

#[test]
fn ctrl_v_makes_the_next_character_a_key() {
    // A Ctrl-V keeps a "|" in the right-hand side, and replace_termcodes()
    // then drops it: "a^V|b<Esc>" appends "|b".
    let mut session = HostSession::new("");
    let _ = session.source_config("nnoremap x a\x16|b<Esc>\nnnoremap y a\x16<CR><Esc>\n");
    type_keys(&mut session, "x");
    assert_eq!(session.text(), "|b");
    type_keys(&mut session, "y");
    assert_eq!(session.text(), "|b<CR>");

    // A Ctrl-V before a blank keeps it in the keys.
    let mut session = HostSession::new("abc");
    let _ = session.source_config("nnoremap \x16 x l\n");
    type_keys(&mut session, " x");
    assert_eq!(session.cursor_offset(), 1);
}

#[test]
fn unmap_reports_a_missing_mapping() {
    // do_exmap() gives E31 when no mode of the command had the keys.
    let mut engine = VimEngine::new();
    assert_eq!(run_ex(&mut engine, "unmap x"), ["E31: No such mapping"]);
    run_ex(&mut engine, "vnoremap x y");
    assert_eq!(run_ex(&mut engine, "nunmap x"), ["E31: No such mapping"]);
    assert!(run_ex(&mut engine, "unmap x").is_empty());
    assert_eq!(run_ex(&mut engine, "vunmap x"), ["E31: No such mapping"]);

    // `@:` repeats the error.
    let doc = TestDocument::new("hello", (0, 0));
    let _ = press(&mut engine, &doc, KeyEvent::char('@'));
    assert_eq!(
        errors(&press(&mut engine, &doc, KeyEvent::char(':'))),
        ["E31: No such mapping"]
    );
}

#[test]
fn unmap_removes_a_mapping_to_its_argument() {
    // do_map(): a second round matches the right-hand side.
    let mut engine = VimEngine::new();
    run_ex(&mut engine, "nmap x y");
    run_ex(&mut engine, "nmap z y");
    assert!(run_ex(&mut engine, "unmap y").is_empty());
    assert!(normal_mappings(&engine).is_empty());

    // Keys come first.
    run_ex(&mut engine, "nmap x y");
    run_ex(&mut engine, "nmap y w");
    assert!(run_ex(&mut engine, "unmap y").is_empty());
    assert_eq!(normal_mappings(&engine), [mapping("x", "y", false)]);
}

#[test]
fn a_missing_mapping_ends_a_config_line() {
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("unmap x | set ic\nsilent! unmap x | set tw=33\n");
    assert_eq!(errors(&response), ["E31: No such mapping"]);
    assert!(!engine.options().ignorecase());
    assert_eq!(engine.options().textwidth(), 33);
}

#[test]
fn unabbreviate_reports_a_missing_abbreviation() {
    let mut engine = VimEngine::new();
    assert_eq!(
        run_ex(&mut engine, "iunabbrev foo"),
        ["E24: No such abbreviation"]
    );
    run_ex(&mut engine, "iabbrev foo bar");
    assert_eq!(
        run_ex(&mut engine, "cunabbrev foo"),
        ["E24: No such abbreviation"]
    );
    assert!(run_ex(&mut engine, "iunabbrev foo").is_empty());
}

#[test]
fn unabbreviate_removes_an_abbreviation_to_its_argument() {
    // do_map(): ":unab foo" after ":ab foo bar" may have been expanded.
    let mut session = HostSession::new("");
    type_keys(
        &mut session,
        ":iabbrev foo bar<CR>:iunabbrev bar<CR>afoo <Esc>",
    );
    assert_eq!(session.text(), "foo ");
}

fn has_mapping(engine: &VimEngine, mode: MappingMode, lhs: &str) -> bool {
    engine
        .keymap()
        .list_mappings(mode)
        .iter()
        .any(|(keys, _, _)| keys.iter().map(|k| k.to_vim_notation()).collect::<String>() == lhs)
}

#[test]
fn map_bang_maps_insert_and_command_line_mode() {
    // get_map_mode() gives the "!" forms MODE_INSERT | MODE_CMDLINE. A
    // config, so that the command-line mappings leave its lines alone.
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("noremap! a b | map! c d | nnoremap a b\n");
    assert!(errors(&response).is_empty(), "{:?}", errors(&response));
    for mode in [MappingMode::Insert, MappingMode::Command] {
        assert!(has_mapping(&engine, mode, "a") && has_mapping(&engine, mode, "c"));
    }
    assert!(!has_mapping(&engine, MappingMode::Visual, "a"));
    assert!(!has_mapping(&engine, MappingMode::Normal, "c"));

    let response = engine.source_config_text("unmap! a\nunmap! a\n");
    assert_eq!(errors(&response), ["E31: No such mapping"]);
    assert!(!has_mapping(&engine, MappingMode::Insert, "a"));
    assert!(!has_mapping(&engine, MappingMode::Command, "a"));
    assert!(has_mapping(&engine, MappingMode::Normal, "a"));

    let response = engine.source_config_text("mapclear!\n");
    assert!(errors(&response).is_empty(), "{:?}", errors(&response));
    assert!(!has_mapping(&engine, MappingMode::Insert, "c"));
    assert!(!has_mapping(&engine, MappingMode::Command, "c"));
    assert!(has_mapping(&engine, MappingMode::Normal, "a"));

    let mut session = HostSession::new("");
    type_keys(&mut session, ":noremap! jk <lt>Esc><CR>ajk");
    assert_eq!(session.mode(), Mode::Normal);
}
