//! A blank at the end of an Ex command does not change the command it is,
//! typed or in a config.

use vim_core::execution::{parse_keys_from_string, HostSession};
use vim_core::primitives::MotionType;

fn typed(text: &str, keys: &str) -> HostSession {
    let mut session = HostSession::new(text);
    session.set_register('a', "s/b/c/", MotionType::CharWise);
    for key in parse_keys_from_string(keys) {
        let _ = session.process_key_host(key);
    }
    session
}

fn config_error(line: &str) -> Option<String> {
    let mut session = HostSession::new("a\nb\n");
    let _ = session.source_config(line);
    session.errmsg()
}

/// Vim drops the blanks after an argument (ex_docmd.c `separate_nextcmd()`,
/// `del_trailing_spaces()`).
#[test]
fn copy_with_a_trailing_blank() {
    assert_eq!(typed("a\nb\n", ":t. <CR>").text(), "a\na\nb\n");
    assert_eq!(typed("a\nb\n", ":co$  <CR>").text(), "a\nb\na\n");
}

#[test]
fn execute_register_with_a_trailing_blank() {
    assert_eq!(typed("a\nb\n", "j:@a <CR>").text(), "a\nc\n");
}

#[test]
fn trailing_blank_in_a_config() {
    assert_eq!(config_error("t. "), config_error("t."));
    assert_eq!(config_error("@a "), config_error("@a"));
}
