//! The reach of every Ex command, checked against what running it gives.

use super::{InputContext, VimEngine};
use crate::grammar::parse_ex_command;
use crate::grammar::types::{ExCommand, Reach};
use crate::test_utils::SimpleDocument;

/// One command line for each [`ExCommand`] variant.
const SAMPLES: [&str; 117] = [
    "s/a/b/",
    "g/a/w",
    "d",
    "y",
    "m$",
    "t0",
    "j",
    "sort",
    "registers",
    "marks",
    "jumps",
    "changes",
    "messages",
    "put a",
    "retab",
    "left",
    "right",
    "center",
    "nnoremap x y",
    "nunmap x",
    "nmapclear",
    "nohlsearch",
    "w",
    "q",
    "wq",
    "e f",
    "r f",
    "1,2!sort",
    "!ls",
    "split",
    "vsplit",
    "close",
    "only",
    "new",
    "vnew",
    "wall",
    "qall",
    "wqall",
    "bdelete",
    "bwipeout",
    "iabbrev a b",
    "iunabbrev a",
    "abclear",
    "Foo",
    "action Save",
    "actionlist",
    "source f",
    "earlier",
    "later",
    "undolist",
    "undotree",
    "undo 1",
    "cnext",
    "cprevious",
    "clist",
    "cc 1",
    "buffer 1",
    "bnext",
    "bprevious",
    "bfirst",
    "blast",
    "ls",
    "tabnew",
    "tabnext",
    "tabprevious",
    "tabclose",
    "echo x",
    "let mapleader = ','",
    "print",
    "z",
    "normal x",
    "2",
    "sx/a/d",
    "sy/a/d",
    "select /a/",
    "split /a/",
    "keep /a/",
    "remove /a/",
    "trim",
    "align",
    "rotate",
    "cursorrotate fwd",
    "addnext",
    "addprev",
    "skipmatch",
    "addcursor below",
    "selectall",
    "cursorcollapse",
    "cursorremove",
    "cursorprimary next",
    "cursorsplit",
    "cursorflip",
    "cursorforward",
    "cursormerge",
    "&",
    "~",
    "set ic",
    "setlocal ic",
    "setglobal ic",
    "sethandler <C-a> n:vim",
    "delmarks a",
    "cquit",
    "update",
    "redo",
    "clearjumps",
    "fold",
    "foldopen",
    "foldclose",
    "windo w",
    "bufdo w",
    "tabdo w",
    "@a",
    "undojoin",
    "mkvimrc",
    "if 1",
    "finish",
    "mark a",
];

/// Lines that also run nested commands or other forms of a variant.
const MORE: [&str; 6] = [
    "ka",
    "v/a/w",
    "g/a/s/a/b/|w",
    "g/a/d|w !sh",
    "g/a/g/b/w",
    "g/a/d|@a",
];

/// The place of `command`'s variant in [`SAMPLES`]; with no wildcard, a
/// new variant fails to compile here.
const fn variant(command: &ExCommand) -> usize {
    match command {
        ExCommand::Substitute { .. } => 0,
        ExCommand::Global { .. } => 1,
        ExCommand::Delete { .. } => 2,
        ExCommand::Yank { .. } => 3,
        ExCommand::Move { .. } => 4,
        ExCommand::Copy { .. } => 5,
        ExCommand::Join { .. } => 6,
        ExCommand::Sort { .. } => 7,
        ExCommand::Registers { .. } => 8,
        ExCommand::Marks { .. } => 9,
        ExCommand::Jumps => 10,
        ExCommand::Changes => 11,
        ExCommand::Messages { .. } => 12,
        ExCommand::Put { .. } => 13,
        ExCommand::Retab { .. } => 14,
        ExCommand::Left { .. } => 15,
        ExCommand::Right { .. } => 16,
        ExCommand::Center { .. } => 17,
        ExCommand::Map { .. } => 18,
        ExCommand::Unmap { .. } => 19,
        ExCommand::MapClear { .. } => 20,
        ExCommand::NoHighlight => 21,
        ExCommand::Write { .. } => 22,
        ExCommand::Quit { .. } => 23,
        ExCommand::WriteQuit { .. } => 24,
        ExCommand::Edit { .. } => 25,
        ExCommand::Read { .. } => 26,
        ExCommand::Filter { .. } => 27,
        ExCommand::External { .. } => 28,
        ExCommand::Split { .. } => 29,
        ExCommand::VSplit { .. } => 30,
        ExCommand::Close { .. } => 31,
        ExCommand::Only { .. } => 32,
        ExCommand::New => 33,
        ExCommand::VNew => 34,
        ExCommand::WriteAll => 35,
        ExCommand::QuitAll { .. } => 36,
        ExCommand::WriteQuitAll => 37,
        ExCommand::BufferDelete { .. } => 38,
        ExCommand::BufferWipeout { .. } => 39,
        ExCommand::Abbreviate { .. } => 40,
        ExCommand::Unabbreviate { .. } => 41,
        ExCommand::AbClear { .. } => 42,
        ExCommand::Custom { .. } => 43,
        ExCommand::Action { .. } => 44,
        ExCommand::ActionList { .. } => 45,
        ExCommand::Source { .. } => 46,
        ExCommand::Earlier { .. } => 47,
        ExCommand::Later { .. } => 48,
        ExCommand::UndoList => 49,
        ExCommand::UndoTree => 50,
        ExCommand::UndoSequence { .. } => 51,
        ExCommand::CNext { .. } => 52,
        ExCommand::CPrev { .. } => 53,
        ExCommand::CList => 54,
        ExCommand::CC { .. } => 55,
        ExCommand::Buffer { .. } => 56,
        ExCommand::BufferNext { .. } => 57,
        ExCommand::BufferPrev { .. } => 58,
        ExCommand::BufferFirst => 59,
        ExCommand::BufferLast => 60,
        ExCommand::BufferList => 61,
        ExCommand::TabNew { .. } => 62,
        ExCommand::TabNext { .. } => 63,
        ExCommand::TabPrev { .. } => 64,
        ExCommand::TabClose { .. } => 65,
        ExCommand::Echo { .. } => 66,
        ExCommand::LetLeader { .. } => 67,
        ExCommand::PrintLines { .. } => 68,
        ExCommand::ZWindow { .. } => 69,
        ExCommand::Norm { .. } => 70,
        ExCommand::GotoLine { .. } => 71,
        ExCommand::StructuralExtract { .. } => 72,
        ExCommand::StructuralComplement { .. } => 73,
        ExCommand::SelectMatches { .. } => 74,
        ExCommand::SplitMatches { .. } => 75,
        ExCommand::KeepMatches { .. } => 76,
        ExCommand::RemoveMatches { .. } => 77,
        ExCommand::TrimSelections => 78,
        ExCommand::AlignSelections => 79,
        ExCommand::RotateContents => 80,
        ExCommand::RotateContentsDir { .. } => 81,
        ExCommand::AddNext { .. } => 82,
        ExCommand::AddPrev { .. } => 83,
        ExCommand::SkipMatch => 84,
        ExCommand::AddCursorDir { .. } => 85,
        ExCommand::SelectAll => 86,
        ExCommand::CursorCollapse => 87,
        ExCommand::CursorRemove => 88,
        ExCommand::CursorPrimary { .. } => 89,
        ExCommand::CursorSplitBlock => 90,
        ExCommand::CursorFlip => 91,
        ExCommand::CursorForward => 92,
        ExCommand::CursorMerge => 93,
        ExCommand::RepeatSubstitute { .. } => 94,
        ExCommand::SubTilde { .. } => 95,
        ExCommand::Set { .. } => 96,
        ExCommand::SetLocal { .. } => 97,
        ExCommand::SetGlobal { .. } => 98,
        ExCommand::SetHandler { .. } => 99,
        ExCommand::DelMarks { .. } => 100,
        ExCommand::CQuit { .. } => 101,
        ExCommand::Update { .. } => 102,
        ExCommand::Redo => 103,
        ExCommand::ClearJumps => 104,
        ExCommand::Fold { .. } => 105,
        ExCommand::FoldOpen { .. } => 106,
        ExCommand::FoldClose { .. } => 107,
        ExCommand::WinDo { .. } => 108,
        ExCommand::BufDo { .. } => 109,
        ExCommand::TabDo { .. } => 110,
        ExCommand::ExecuteRegister { .. } => 111,
        ExCommand::UndoJoin => 112,
        ExCommand::MkVimrc { .. } => 113,
        ExCommand::Block { .. } => 114,
        ExCommand::Finish => 115,
        ExCommand::Mark { .. } => 116,
    }
}

/// The host requests running `line` on a small buffer gives, with
/// register `a` holding `:w`.
fn requests(line: &str) -> usize {
    let doc = SimpleDocument::new("w\na\nb\n");
    let mut engine = VimEngine::new();
    engine.execute_ex("1y a", InputContext::new(&doc, 0).validate_clamped());
    let input = InputContext::new(&doc, 0).validate_clamped();
    engine.execute_ex(line, input).host_requests.len()
}

#[test]
fn every_variant_has_a_sample() {
    let mut seen = [false; SAMPLES.len()];
    for line in SAMPLES {
        let command = parse_ex_command(line).unwrap_or_else(|e| panic!("{line}: {e}"));
        let at = variant(&command);
        assert!(!seen[at], "{line}: a second sample of {command:?}");
        seen[at] = true;
    }
    assert!(seen.iter().all(|&s| s));
}

#[test]
fn a_command_that_asks_the_host_reaches_it() {
    assert!(requests("@a") > 0 && requests("g/a/s/a/b/|w") > 0);
    for line in SAMPLES.iter().chain(&MORE) {
        let command = parse_ex_command(line).unwrap_or_else(|e| panic!("{line}: {e}"));
        if requests(line) > 0 {
            assert!(command.reach().contains(Reach::HOST), "{line}");
        }
    }
}

#[test]
fn global_reaches_every_command_of_its_line() {
    let reach = |line: &str| {
        parse_ex_command(line)
            .map(|c| c.reach())
            .unwrap_or_default()
    };
    let set = reach("g/a/s/a/b/|set ic|nnoremap x y");
    assert!(set.contains(Reach::BUFFER | Reach::NESTED | Reach::OPTIONS | Reach::MAPPING));
    assert!(reach("g/a/d|w !sh").contains(Reach::SHELL | Reach::HOST));
    assert!(reach("v/a/g/b/set lmap=x").contains(Reach::SECURE_OPTION));
    assert_eq!(reach("ka"), Reach::BUFFER);
    assert_eq!(reach("'a,'bmark b"), Reach::BUFFER);
}
