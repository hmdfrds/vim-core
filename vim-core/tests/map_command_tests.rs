//! Map and abbreviation commands as Vim's `do_map()` and `do_exmap()` run
//! them, typed and in a config.

mod common;

use common::document::TestDocument;
use vim_core::effects::Effect;
use vim_core::execution::{parse_keys_from_string, HostSession, InputContext, Response, VimEngine};
use vim_core::keymap::{KeyEvent, MappingMode};

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
