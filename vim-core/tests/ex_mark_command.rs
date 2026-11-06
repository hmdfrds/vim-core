//! `:mark` and `:k`, typed and in a config.

use vim_core::execution::{parse_keys_from_string, HostResponse, HostSession};
use vim_core::state::StatusMessage;

const TEXT: &str = "  one\n\ttwo\nthree\n";

fn typed(session: &mut HostSession, keys: &str) -> Option<HostResponse> {
    let mut last = None;
    for key in parse_keys_from_string(keys) {
        last = Some(session.process_key_host(key));
    }
    last
}

/// Each form sets the mark at the first non-blank of the cursor line or of
/// the range's last line (ex_docmd.c `ex_mark()`, `one_letter_cmd()`).
#[test]
fn mark_ma_k_and_ka_set_a_mark() {
    let mut session = HostSession::new(TEXT);
    typed(
        &mut session,
        ":mark a<CR>:ma b<CR>j:k c<CR>:kd<CR>:1,3mark e<CR>",
    );
    assert_eq!(session.get_mark('a'), Some(2));
    assert_eq!(session.get_mark('b'), Some(2));
    assert_eq!(session.get_mark('c'), Some(7));
    assert_eq!(session.get_mark('d'), Some(7));
    assert_eq!(session.get_mark('e'), Some(11));
}

/// ex_docmd.c `ex_mark()`'s errors.
#[test]
fn mark_errors_as_vim_does() {
    let mut session = HostSession::new(TEXT);
    for (keys, error) in [
        (":mark<CR>", "E471: Argument required"),
        (":mark ab<CR>", "E488: Trailing characters: ab"),
        (
            ":mark !<CR>",
            "E191: Argument must be a letter or forward/backward quote",
        ),
    ] {
        typed(&mut session, keys);
        assert_eq!(session.errmsg().as_deref(), Some(error), "{keys}");
    }
}

#[test]
fn marks_still_lists() {
    let mut session = HostSession::new(TEXT);
    let response = typed(&mut session, ":mark a<CR>:marks<CR>");
    let listed = match response.and_then(|r| r.message) {
        Some(StatusMessage::Info(text)) => text.to_string(),
        other => format!("{other:?}"),
    };
    assert!(listed.contains("\n a "), "{listed}");
}
