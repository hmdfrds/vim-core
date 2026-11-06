//! A `|` line gives what its commands give typed one by one: each runs on
//! the text, cursor, marks and registers the one before left, and an error
//! ends the line (ex_docmd.c `do_cmdline()`).

use proptest::prelude::*;
use vim_test::session::TestSession;

/// `PROPTEST_CASES` overrides the case count.
fn config() -> ProptestConfig {
    ProptestConfig {
        cases: std::env::var("PROPTEST_CASES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(256),
        ..ProptestConfig::default()
    }
}

fn text() -> impl Strategy<Value = String> {
    let line = prop::collection::vec(
        prop::sample::select(vec![
            "a", "o", "x", " ", "\t", "é", "€", "😀", "ｘ", "\u{200b}",
        ]),
        0..6,
    )
    .prop_map(|chars| chars.concat());
    (prop::collection::vec(line, 1..5), any::<bool>()).prop_map(|(lines, newline)| {
        let mut text = lines.join("\n");
        if newline {
            text.push('\n');
        }
        text
    })
}

fn range() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec![
        "", "", "1", "2", "$", ".", ".+1", "1,2", ".,$", "%", "'a", "'a,.",
    ])
}

fn command() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec![
        "d",
        "d b",
        "s/a/€/",
        "s/o/é/g",
        "s/./😀/",
        "s/x\\|ｘ/\\r/",
        "m0",
        "m$",
        "m1",
        "t0",
        "t.",
        "t$",
        "j",
        "j!",
        ">",
        "<",
        "y c",
    ])
}

/// A command that may end the line: `:normal` takes the rest of it.
fn last_command() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => command().prop_map(String::from),
        1 => prop::sample::select(vec!["normal x", "normal dd", "normal A€", "normal >>"])
            .prop_map(String::from),
    ]
}

/// The keys that run `command`, with `<` as written.
fn ex(command: &str) -> String {
    format!(":{}<CR>", command.replace('<', "<lt>"))
}

/// `'a` is set on the second line, if there is one.
fn session(text: &str) -> TestSession {
    let mut session = TestSession::new(&format!("|{text}"));
    session.feed("jmak");
    session
}

fn state(session: &TestSession) -> (String, usize, Vec<Option<(String, String)>>) {
    let registers = ['"', '0', '1', '2', '3', '-', 'b', 'c']
        .iter()
        .map(|&name| {
            session
                .register(name)
                .map(|(text, kind)| (text, format!("{kind:?}")))
        })
        .collect();
    (
        session.text().to_owned(),
        session.cursor_offset(),
        registers,
    )
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn a_line_runs_its_commands_in_turn(
        text in text(),
        first_range in range(),
        first in command(),
        second_range in range(),
        second in last_command(),
    ) {
        let first = format!("{first_range}{first}");
        let second = format!("{second_range}{second}");

        let mut typed = session(&text);
        typed.feed(&ex(&first));
        // An error ends a typed line, so the second does not run.
        if typed.errmsg().is_none() {
            typed.feed(&ex(&second));
        }

        let mut line = session(&text);
        line.feed(&ex(&format!("{first}|{second}")));

        prop_assert_eq!(state(&line), state(&typed), "{:?} :{}|{}", text, first, second);
    }
}
