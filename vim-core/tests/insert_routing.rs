//! An Insert-mode command that waits for an argument must not leak its state
//! out of Insert mode, where the next key would complete it as an Insert
//! command in Normal mode.

use vim_core::execution::{parse_keys_from_string, HostSession};
use vim_core::primitives::Mode;

fn run(text: &str, config: &str, keys: &str) -> HostSession {
    let mut session = HostSession::new(text);
    if !config.is_empty() {
        let _ = session.source_config(config);
    }
    for key in parse_keys_from_string(keys) {
        let _ = session.process_key_host(key);
    }
    session
}

fn assert_insert(session: &HostSession, text: &str, cursor: usize) {
    assert_eq!(session.text(), text);
    assert_eq!(session.mode(), Mode::Insert);
    assert_eq!(session.cursor_offset(), cursor);
}

/// `<Esc>` and `<C-c>` are not register names: Vim beeps and stays in Insert
/// mode (edit.c `ins_reg()`).
#[test]
fn escape_after_ctrl_r_stays_in_insert() {
    assert_insert(&run("abc\n", "", "a<C-r><Esc>I"), "aIbc\n", 2);
    assert_insert(&run("abc\n", "", "a<C-r><C-c>I"), "aIbc\n", 2);
}

/// edit.c `ins_reg()` reads the register name after `<C-r><C-r>`,
/// `<C-r><C-o>` and `<C-r><C-p>` the same way.
#[test]
fn escape_after_literal_ctrl_r_stays_in_insert() {
    for prefix in ["<C-r><C-r>", "<C-r><C-o>", "<C-r><C-p>"] {
        let keys = format!("a{prefix}<Esc>I");
        assert_insert(&run("abc\n", "", &keys), "aIbc\n", 2);
    }
}

/// After `<C-r><C-r>` the next key is the register name whatever it is, so a
/// third `<C-r>` is used up and `<Esc>` leaves Insert mode (edit.c
/// `ins_reg()`).
#[test]
fn third_ctrl_r_is_used_up() {
    let session = run("abc\n", "", "a<C-r><C-r><C-r><Esc>");
    assert_eq!(session.mode(), Mode::Normal);
    assert_eq!(session.text(), "abc\n");
}

/// Leaving the `=` command line cancels the expression and stays in Insert
/// mode (register.c `get_expr_register()`, edit.c `ins_reg()`).
#[test]
fn escape_in_expression_register_stays_in_insert() {
    assert_insert(&run("abc\n", "", "a<C-r>=<Esc>1<CR>"), "a1\nbc\n", 3);
}

/// "ESC cancels CTRL-K", after either character (digraph.c `get_digraph()`).
#[test]
fn escape_in_digraph_stays_in_insert() {
    assert_insert(&run("abc\n", "", "a<C-k><Esc>.I"), "a.Ibc\n", 3);
    assert_insert(&run("abc\n", "", "a<C-k>.<Esc>I"), "aIbc\n", 2);
}

/// An unknown key after `<C-g>` beeps and stays in Insert mode (edit.c
/// `ins_ctrl_g()`).
#[test]
fn escape_after_ctrl_g_stays_in_insert() {
    assert_insert(&run("abc\n", "", "a<C-g><Esc>u"), "aubc\n", 2);
}

/// `<Esc>` after `<C-x>` leaves Insert mode (insexpand.c `set_ctrl_x_mode()`
/// passes it on), and the next key is a Normal command again.
#[test]
fn escape_after_ctrl_x_leaves_insert_and_the_pending_key() {
    let session = run("abc\n", "", "a<C-x><Esc><C-o>");
    assert_eq!(session.mode(), Mode::Normal);
    assert_insert(&run("abc\n", "", "a<C-x><Esc>Ix"), "xabc\n", 1);
}

#[test]
fn escape_after_ctrl_r_stays_in_replace() {
    let session = run("abc\n", "", "R<C-r><Esc>I");
    assert_eq!(session.text(), "Ibc\n");
    assert_eq!(session.mode(), Mode::Replace);
}

#[test]
fn mapped_escape_after_ctrl_r_stays_in_insert() {
    let session = run("abc\n", "inoremap <F2> <Esc>I", "a<C-r><F2>");
    assert_insert(&session, "aIbc\n", 2);
}

struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        let len = u64::try_from(items.len()).unwrap_or(1);
        items[usize::try_from(self.next() % len).unwrap_or(0)]
    }
}

const TOKENS: &[&str] = &[
    "i",
    "a",
    "o",
    "A",
    "I",
    ":",
    "/",
    "<C-r>",
    "=",
    "<C-k>",
    "a:",
    "e'",
    "\"",
    "x",
    "p",
    "<Esc>",
    "<CR>",
    "<C-o>",
    "<C-v>",
    "v",
    "V",
    "<C-u>",
    "1",
    "+",
    "-",
    "%",
    "<C-w>",
    "<BS>",
    "<Tab>",
    "j",
    "l",
    "c",
    "d",
    "w",
    "s",
    "R",
    "<C-x>",
    "<C-n>",
    "<C-a>",
    "<C-g>",
    "<C-\\><C-n>",
    "<C-\\><C-o>",
    "<C-r><C-w>",
    "<C-r><C-r>",
    "<C-r><C-o>",
    "<C-r><C-p>",
    "<C-r>=",
    "<C-k>a:",
    "<C-c>",
    "u",
    ".",
    "<Space>",
    "<C-]>",
    "<C-e>",
    "<C-y>",
    "<Insert>",
    "gR",
    "<C-[>",
    "<C-r>a",
    "<C-r>/",
    "<C-g>u",
    "<C-g>j",
    "<C-o>:",
    "<C-o>v",
    "S",
    "<Left>",
    "<Del>",
];
const MAP_COMMANDS: &[&str] = &[
    "nnoremap", "inoremap", "cnoremap", "xnoremap", "onoremap", "imap", "noremap!",
];
const BEFORE: &[&str] = &[
    "",
    "i",
    ":",
    "v",
    "R",
    "a<C-r>",
    "a<C-k>",
    "a<C-r>=",
    "a<C-g>",
    "a<C-x>",
    "a<C-r><C-r>",
];

/// Seeded random search for the states that let an Insert command reach the
/// executor: random mappings pressed, and random keys typed.
#[test]
#[ignore = "random search, run on demand"]
fn random_keys_never_route_an_insert_command_to_the_executor() {
    let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
    let mut hits = Vec::new();
    for case in 0..20_000 {
        let len = 1 + rng.next() % 12;
        let rhs: String = (0..len).map(|_| rng.pick(TOKENS)).collect();
        let typed = rng.next() % 3 == 0;
        let config = format!("{} <F2> {rhs}", rng.pick(MAP_COMMANDS));
        let before = rng.pick(BEFORE);
        let outcome = std::panic::catch_unwind(|| {
            if typed {
                run("abc def\nghi jkl\n", "", &format!("{before}{rhs}{rhs}"));
            } else {
                run(
                    "abc def\nghi jkl\n",
                    &config,
                    &format!("{before}<F2><F2><F2>"),
                );
            }
        });
        if let Err(payload) = outcome {
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .unwrap_or_default();
            if message.contains("engine must route directly") {
                hits.push(format!("case {case}: {before:?} {typed} {config:?}"));
            }
        }
    }
    assert!(hits.is_empty(), "{}", hits.join("\n"));
}
