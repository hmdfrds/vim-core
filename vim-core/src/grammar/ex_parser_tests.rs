use super::*;
use crate::grammar::types::{ModifierFlags, ZWindowStyle};
use crate::keymap::{MappingFlags, MappingKind};
use crate::primitives::AbbrevMode;

#[test]
fn parses_write_and_quit() {
    let write = parse_ex_command("w! /tmp/x").unwrap();
    assert!(matches!(
        write,
        ExCommand::Write {
            force: true,
            path: Some(_),
        }
    ));

    let quit = parse_ex_command("q!").unwrap();
    assert!(matches!(quit, ExCommand::Quit { force: true }));

    let edit = parse_ex_command("edit! /tmp/x.gd").unwrap();
    assert!(matches!(
        edit,
        ExCommand::Edit {
            force: true,
            path,
        } if path.as_str() == "/tmp/x.gd"
    ));
}

#[test]
fn parses_substitute() {
    let cmd = parse_ex_command("%s/foo/bar/g").unwrap();
    match cmd {
        ExCommand::Substitute { flags, .. } => assert!(flags.global()),
        other => panic!("expected Substitute, got {other:?}"),
    }
}

#[test]
fn parses_filter_and_external() {
    let filter = parse_ex_command("5,8!sort").unwrap();
    assert!(matches!(filter, ExCommand::Filter { .. }));

    let external = parse_ex_command("!ls -la").unwrap();
    assert!(matches!(external, ExCommand::External { .. }));
}

#[test]
fn parses_read_after_line_from_absolute_address() {
    let single = parse_ex_command("5r /tmp/in.txt").unwrap();
    assert!(matches!(
        single,
        ExCommand::Read {
            after_line: Some(5),
            ..
        }
    ));

    let ranged = parse_ex_command("3,7read /tmp/in.txt").unwrap();
    assert!(matches!(
        ranged,
        ExCommand::Read {
            after_line: Some(7),
            ..
        }
    ));
}

#[test]
fn parses_action_command() {
    let cmd = parse_ex_command("action ReformatCode").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Action { name } if name.as_str() == "ReformatCode"
    ));
}

#[test]
fn parses_actionlist_with_filter() {
    let cmd = parse_ex_command("actionlist Reformat").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::ActionList { filter: Some(f) } if f.as_str() == "Reformat"
    ));
}

#[test]
fn parses_actionlist_no_filter() {
    let cmd = parse_ex_command("actionlist").unwrap();
    assert!(matches!(cmd, ExCommand::ActionList { filter: None }));
}

#[test]
fn parses_normal_bang_as_non_recursive() {
    let remapped = parse_ex_command("normal dd").unwrap();
    assert!(matches!(
        remapped,
        ExCommand::Norm {
            remap: true,
            keys,
            ..
        } if keys.as_str() == "dd"
    ));

    let non_recursive = parse_ex_command("normal! dd").unwrap();
    assert!(matches!(
        non_recursive,
        ExCommand::Norm {
            remap: false,
            keys,
            ..
        } if keys.as_str() == "dd"
    ));
}

#[test]
fn parses_source_command() {
    let cmd = parse_ex_command("source ~/.vimrc").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Source { path } if path.as_str() == "~/.vimrc"
    ));

    let cmd2 = parse_ex_command("so /etc/vim/config").unwrap();
    assert!(matches!(
        cmd2,
        ExCommand::Source { path } if path.as_str() == "/etc/vim/config"
    ));
}

#[test]
fn source_requires_path() {
    let result = parse_ex_command("source");
    assert!(result.is_err());

    let result2 = parse_ex_command("so");
    assert!(result2.is_err());
}

// ─── :earlier / :later / :undolist ───────────────────────────────

#[test]
fn parses_earlier_changes() {
    let cmd = parse_ex_command("earlier 5").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Earlier {
            amount: TimeAmount::Changes(5)
        }
    ));
}

#[test]
fn parses_earlier_default_one_change() {
    let cmd = parse_ex_command("earlier").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Earlier {
            amount: TimeAmount::Changes(1)
        }
    ));
}

#[test]
fn parses_earlier_seconds() {
    let cmd = parse_ex_command("earlier 10s").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Earlier {
            amount: TimeAmount::Seconds(10)
        }
    ));
}

#[test]
fn parses_earlier_minutes() {
    let cmd = parse_ex_command("earlier 5m").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Earlier {
            amount: TimeAmount::Minutes(5)
        }
    ));
}

#[test]
fn parses_earlier_hours() {
    let cmd = parse_ex_command("earlier 2h").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Earlier {
            amount: TimeAmount::Hours(2)
        }
    ));
}

#[test]
fn parses_earlier_abbreviation() {
    let cmd = parse_ex_command("ea 3").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Earlier {
            amount: TimeAmount::Changes(3)
        }
    ));
}

#[test]
fn parses_later_changes() {
    let cmd = parse_ex_command("later 3").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Later {
            amount: TimeAmount::Changes(3)
        }
    ));
}

#[test]
fn parses_later_default_one_change() {
    let cmd = parse_ex_command("later").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Later {
            amount: TimeAmount::Changes(1)
        }
    ));
}

#[test]
fn parses_later_seconds() {
    let cmd = parse_ex_command("lat 30s").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Later {
            amount: TimeAmount::Seconds(30)
        }
    ));
}

#[test]
fn parses_undolist() {
    let cmd = parse_ex_command("undolist").unwrap();
    assert!(matches!(cmd, ExCommand::UndoList));

    let cmd2 = parse_ex_command("undol").unwrap();
    assert!(matches!(cmd2, ExCommand::UndoList));
}

#[test]
fn parses_undotree() {
    let cmd = parse_ex_command("undotree").unwrap();
    assert!(matches!(cmd, ExCommand::UndoTree));
}

#[test]
fn undotree_does_not_mutate_text() {
    let cmd = ExCommand::UndoTree;
    assert!(!cmd.mutates_text());
}

#[test]
fn earlier_invalid_argument() {
    let result = parse_ex_command("earlier abc");
    assert!(result.is_err());
}

// ─── Buffer/Tab commands ────────────────────────────────────────

#[test]
fn parses_buffer_with_number() {
    let cmd = parse_ex_command("buffer 3").unwrap();
    assert!(matches!(cmd, ExCommand::Buffer { number: 3 }));

    let cmd2 = parse_ex_command("b 1").unwrap();
    assert!(matches!(cmd2, ExCommand::Buffer { number: 1 }));
}

#[test]
fn buffer_requires_number() {
    let result = parse_ex_command("buffer");
    assert!(result.is_err());
}

#[test]
fn parses_bnext() {
    let cmd = parse_ex_command("bnext").unwrap();
    assert!(matches!(cmd, ExCommand::BufferNext { count: 1 }));

    let cmd2 = parse_ex_command("bn 3").unwrap();
    assert!(matches!(cmd2, ExCommand::BufferNext { count: 3 }));
}

#[test]
fn parses_bprev() {
    let cmd = parse_ex_command("bprev").unwrap();
    assert!(matches!(cmd, ExCommand::BufferPrev { count: 1 }));

    let cmd2 = parse_ex_command("bp 2").unwrap();
    assert!(matches!(cmd2, ExCommand::BufferPrev { count: 2 }));
}

#[test]
fn parses_bfirst_blast() {
    assert!(matches!(
        parse_ex_command("bfirst").unwrap(),
        ExCommand::BufferFirst
    ));
    assert!(matches!(
        parse_ex_command("bf").unwrap(),
        ExCommand::BufferFirst
    ));
    assert!(matches!(
        parse_ex_command("blast").unwrap(),
        ExCommand::BufferLast
    ));
    assert!(matches!(
        parse_ex_command("bl").unwrap(),
        ExCommand::BufferLast
    ));
}

#[test]
fn parses_ls_buffers() {
    assert!(matches!(
        parse_ex_command("ls").unwrap(),
        ExCommand::BufferList
    ));
    assert!(matches!(
        parse_ex_command("buffers").unwrap(),
        ExCommand::BufferList
    ));
    assert!(matches!(
        parse_ex_command("files").unwrap(),
        ExCommand::BufferList
    ));
}

#[test]
fn parses_tabnew() {
    let cmd = parse_ex_command("tabnew").unwrap();
    assert!(matches!(cmd, ExCommand::TabNew { path: None }));

    let cmd2 = parse_ex_command("tabnew foo.rs").unwrap();
    assert!(matches!(cmd2, ExCommand::TabNew { path: Some(p) } if p.as_str() == "foo.rs"));

    let cmd3 = parse_ex_command("tabe bar.rs").unwrap();
    assert!(matches!(cmd3, ExCommand::TabNew { path: Some(p) } if p.as_str() == "bar.rs"));
}

#[test]
fn parses_tabnext_tabprev() {
    assert!(matches!(
        parse_ex_command("tabn").unwrap(),
        ExCommand::TabNext { count: 1 }
    ));
    assert!(matches!(
        parse_ex_command("tabnext 3").unwrap(),
        ExCommand::TabNext { count: 3 }
    ));
    assert!(matches!(
        parse_ex_command("tabp").unwrap(),
        ExCommand::TabPrev { count: 1 }
    ));
    assert!(matches!(
        parse_ex_command("tabprev 2").unwrap(),
        ExCommand::TabPrev { count: 2 }
    ));
}

#[test]
fn parses_tabclose() {
    let cmd = parse_ex_command("tabc").unwrap();
    assert!(matches!(cmd, ExCommand::TabClose { force: false }));

    let cmd2 = parse_ex_command("tabclose!").unwrap();
    assert!(matches!(cmd2, ExCommand::TabClose { force: true }));
}

// ─── Display / Misc ─────────────────────────────────────────────

#[test]
fn parses_echo() {
    let cmd = parse_ex_command("echo Hello World").unwrap();
    assert!(matches!(cmd, ExCommand::Echo { message } if message.as_str() == "Hello World"));
}

#[test]
fn parses_let_mapleader() {
    let cmd = parse_ex_command(r#"let mapleader = ",""#).unwrap();
    assert!(matches!(cmd, ExCommand::LetMapleader { leader: ',' }));

    let cmd2 = parse_ex_command("let g:mapleader = ' '").unwrap();
    assert!(matches!(cmd2, ExCommand::LetMapleader { leader: ' ' }));
}

#[test]
fn parses_let_mapleader_bare() {
    let cmd = parse_ex_command(r"let mapleader = \").unwrap();
    assert!(matches!(cmd, ExCommand::LetMapleader { leader: '\\' }));
}

#[test]
fn parses_number() {
    let cmd = parse_ex_command("number").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::PrintLines {
            number: true,
            list: false,
            ..
        }
    ));

    let cmd2 = parse_ex_command("nu").unwrap();
    assert!(matches!(
        cmd2,
        ExCommand::PrintLines {
            number: true,
            list: false,
            ..
        }
    ));

    let cmd3 = parse_ex_command("#").unwrap();
    assert!(matches!(
        cmd3,
        ExCommand::PrintLines {
            number: true,
            list: false,
            ..
        }
    ));
}

#[test]
fn parses_print() {
    let cmd = parse_ex_command("print").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::PrintLines {
            number: false,
            list: false,
            ..
        }
    ));

    let cmd2 = parse_ex_command("p").unwrap();
    assert!(matches!(
        cmd2,
        ExCommand::PrintLines {
            number: false,
            list: false,
            ..
        }
    ));
}

#[test]
fn parses_list() {
    let cmd = parse_ex_command("list").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::PrintLines {
            number: false,
            list: true,
            ..
        }
    ));

    let cmd2 = parse_ex_command("l").unwrap();
    assert!(matches!(
        cmd2,
        ExCommand::PrintLines {
            number: false,
            list: true,
            ..
        }
    ));
}

#[test]
fn parses_z_window() {
    let cmd = parse_ex_command("z").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::ZWindow {
            style: ZWindowStyle::Below,
            count: None,
            ..
        }
    ));

    let cmd2 = parse_ex_command("z+").unwrap();
    assert!(matches!(
        cmd2,
        ExCommand::ZWindow {
            style: ZWindowStyle::Below,
            ..
        }
    ));

    let cmd3 = parse_ex_command("z-").unwrap();
    assert!(matches!(
        cmd3,
        ExCommand::ZWindow {
            style: ZWindowStyle::Above,
            ..
        }
    ));

    let cmd4 = parse_ex_command("z. 30").unwrap();
    assert!(matches!(
        cmd4,
        ExCommand::ZWindow {
            style: ZWindowStyle::Centered,
            count: Some(30),
            ..
        }
    ));

    let cmd5 = parse_ex_command("z=").unwrap();
    assert!(matches!(
        cmd5,
        ExCommand::ZWindow {
            style: ZWindowStyle::Highlighted,
            ..
        }
    ));

    let cmd6 = parse_ex_command("z#").unwrap();
    assert!(matches!(
        cmd6,
        ExCommand::ZWindow {
            style: ZWindowStyle::Numbered,
            ..
        }
    ));
}

#[test]
fn let_unsupported_forwards_as_custom() {
    let cmd = parse_ex_command("let foo = 42").unwrap();
    assert!(matches!(cmd, ExCommand::Custom { .. }));
}

// ─── :sethandler ─────────────────────────────────────────────────

#[test]
fn sethandler_with_key_and_single_assignment() {
    let cmd = parse_ex_command("sethandler <C-A> n:vim").unwrap();
    match cmd {
        ExCommand::SetHandler { key, assignments } => {
            assert_eq!(key.as_deref(), Some("<C-A>"));
            assert_eq!(assignments.len(), 1);
            assert_eq!(assignments[0].0.as_str(), "n");
            assert_eq!(assignments[0].1.as_str(), "vim");
        }
        other => panic!("expected SetHandler, got {other:?}"),
    }
}

#[test]
fn sethandler_with_key_and_multiple_assignments() {
    let cmd = parse_ex_command("sethandler <C-A> n:vim i:ide").unwrap();
    match cmd {
        ExCommand::SetHandler { key, assignments } => {
            assert_eq!(key.as_deref(), Some("<C-A>"));
            assert_eq!(assignments.len(), 2);
            assert_eq!(assignments[0].0.as_str(), "n");
            assert_eq!(assignments[0].1.as_str(), "vim");
            assert_eq!(assignments[1].0.as_str(), "i");
            assert_eq!(assignments[1].1.as_str(), "ide");
        }
        other => panic!("expected SetHandler, got {other:?}"),
    }
}

#[test]
fn sethandler_dash_separated_modes() {
    let cmd = parse_ex_command("sethandler <C-C> n-v:ide i:vim").unwrap();
    match cmd {
        ExCommand::SetHandler { key, assignments } => {
            assert_eq!(key.as_deref(), Some("<C-C>"));
            assert_eq!(assignments.len(), 2);
            assert_eq!(assignments[0].0.as_str(), "n-v");
            assert_eq!(assignments[0].1.as_str(), "ide");
            assert_eq!(assignments[1].0.as_str(), "i");
            assert_eq!(assignments[1].1.as_str(), "vim");
        }
        other => panic!("expected SetHandler, got {other:?}"),
    }
}

#[test]
fn sethandler_no_key_global() {
    let cmd = parse_ex_command("sethandler n:vim i:ide").unwrap();
    match cmd {
        ExCommand::SetHandler { key, assignments } => {
            assert!(key.is_none());
            assert_eq!(assignments.len(), 2);
            assert_eq!(assignments[0].0.as_str(), "n");
            assert_eq!(assignments[0].1.as_str(), "vim");
            assert_eq!(assignments[1].0.as_str(), "i");
            assert_eq!(assignments[1].1.as_str(), "ide");
        }
        other => panic!("expected SetHandler, got {other:?}"),
    }
}

#[test]
fn sethandler_abbreviated_command() {
    let cmd = parse_ex_command("seth <C-A> n:vim").unwrap();
    match cmd {
        ExCommand::SetHandler { key, assignments } => {
            assert_eq!(key.as_deref(), Some("<C-A>"));
            assert_eq!(assignments.len(), 1);
        }
        other => panic!("expected SetHandler, got {other:?}"),
    }
}

#[test]
fn sethandler_host_handler_name() {
    let cmd = parse_ex_command("sethandler <C-V> a:host").unwrap();
    match cmd {
        ExCommand::SetHandler { key, assignments } => {
            assert_eq!(key.as_deref(), Some("<C-V>"));
            assert_eq!(assignments.len(), 1);
            assert_eq!(assignments[0].0.as_str(), "a");
            assert_eq!(assignments[0].1.as_str(), "host");
        }
        other => panic!("expected SetHandler, got {other:?}"),
    }
}

#[test]
fn sethandler_requires_arguments() {
    let result = parse_ex_command("sethandler");
    assert!(result.is_err());
}

#[test]
fn sethandler_requires_mode_handler_format() {
    // A key notation alone without assignments
    let result = parse_ex_command("sethandler <C-A>");
    assert!(result.is_err());
}

#[test]
fn sethandler_invalid_format_no_colon() {
    let result = parse_ex_command("sethandler <C-A> vim");
    assert!(result.is_err());
}

#[test]
fn sethandler_does_not_mutate_text() {
    let cmd = parse_ex_command("sethandler <C-A> n:vim").unwrap();
    assert!(!cmd.mutates_text());
}

// ─── :messages ──────────────────────────────────────────────────

#[test]
fn parses_messages_no_args() {
    let cmd = parse_ex_command("messages").unwrap();
    assert!(matches!(cmd, ExCommand::Messages { clear: false }));
}

#[test]
fn parses_messages_abbreviated() {
    let cmd = parse_ex_command("mes").unwrap();
    assert!(matches!(cmd, ExCommand::Messages { clear: false }));
}

#[test]
fn parses_messages_clear() {
    let cmd = parse_ex_command("messages clear").unwrap();
    assert!(matches!(cmd, ExCommand::Messages { clear: true }));
}

#[test]
fn parses_messages_clear_case_insensitive() {
    let cmd = parse_ex_command("messages CLEAR").unwrap();
    assert!(matches!(cmd, ExCommand::Messages { clear: true }));

    let cmd2 = parse_ex_command("mes Clear").unwrap();
    assert!(matches!(cmd2, ExCommand::Messages { clear: true }));
}

#[test]
fn messages_does_not_mutate_text() {
    let cmd = ExCommand::Messages { clear: false };
    assert!(!cmd.mutates_text());

    let cmd_clear = ExCommand::Messages { clear: true };
    assert!(!cmd_clear.mutates_text());
}

#[test]
fn parses_nnoremap_nowait_flag() {
    let cmd = parse_ex_command("nnoremap <nowait> jk <Esc>").unwrap();
    assert!(
        matches!(
            cmd,
            ExCommand::Map {
                mode_prefix: MapModePrefix::Normal,
                kind: MappingKind::NonRecursive,
                flags: MappingFlags { nowait: true, .. },
                ..
            }
        ),
        "expected nowait: true for `<nowait>` flag, got: {cmd:?}"
    );
}

#[test]
fn parses_nmap_no_nowait_flag() {
    let cmd = parse_ex_command("nmap jk <Esc>").unwrap();
    assert!(
        matches!(
            cmd,
            ExCommand::Map {
                mode_prefix: MapModePrefix::Normal,
                kind: MappingKind::Recursive,
                flags: MappingFlags { nowait: false, .. },
                ..
            }
        ),
        "expected nowait: false when <nowait> flag absent, got: {cmd:?}"
    );
}

#[test]
fn parses_nowait_flag_case_insensitive() {
    let cmd = parse_ex_command("nmap <NOWAIT> jk <Esc>").unwrap();
    assert!(
        matches!(
            cmd,
            ExCommand::Map {
                flags: MappingFlags { nowait: true, .. },
                ..
            }
        ),
        "expected nowait: true for case-variant `<NOWAIT>`, got: {cmd:?}"
    );
}

// ─── <expr> mapping flag ─────────────────────────────────────────

#[test]
fn parses_nnoremap_expr_flag() {
    let cmd = parse_ex_command("nnoremap <expr> j v:count ? 'j' : 'gj'").unwrap();
    match cmd {
        ExCommand::Map {
            mode_prefix,
            kind,
            flags,
            lhs,
            rhs,
        } => {
            assert_eq!(mode_prefix, MapModePrefix::Normal);
            assert!(matches!(kind, MappingKind::NonRecursive));
            assert!(flags.expr);
            assert!(!flags.nowait);
            assert!(!flags.silent);
            assert_eq!(lhs.as_str(), "j");
            assert_eq!(rhs.as_deref(), Some("v:count ? 'j' : 'gj'"));
        }
        other => panic!("expected Map, got {other:?}"),
    }
}

#[test]
fn parses_nmap_expr_flag() {
    let cmd = parse_ex_command("nmap <expr> j v:count ? 'j' : 'gj'").unwrap();
    match cmd {
        ExCommand::Map { kind, flags, .. } => {
            assert!(matches!(kind, MappingKind::Recursive));
            assert!(flags.expr);
        }
        other => panic!("expected Map, got {other:?}"),
    }
}

#[test]
fn parses_expr_flag_case_insensitive() {
    let cmd = parse_ex_command("nmap <EXPR> j <Esc>").unwrap();
    assert!(
        matches!(
            cmd,
            ExCommand::Map {
                flags: MappingFlags { expr: true, .. },
                ..
            }
        ),
        "expected expr: true for case-variant `<EXPR>`, got: {cmd:?}"
    );

    let cmd2 = parse_ex_command("nmap <Expr> j <Esc>").unwrap();
    assert!(
        matches!(
            cmd2,
            ExCommand::Map {
                flags: MappingFlags { expr: true, .. },
                ..
            }
        ),
        "expected expr: true for case-variant `<Expr>`, got: {cmd2:?}"
    );
}

#[test]
fn parses_expr_and_nowait_together() {
    let cmd = parse_ex_command("nnoremap <nowait> <expr> j v:count ? 'j' : 'gj'").unwrap();
    match cmd {
        ExCommand::Map { flags, lhs, .. } => {
            assert!(flags.nowait);
            assert!(flags.expr);
            assert_eq!(lhs.as_str(), "j");
        }
        other => panic!("expected Map, got {other:?}"),
    }
}

#[test]
fn parses_expr_and_nowait_reversed_order() {
    let cmd = parse_ex_command("nnoremap <expr> <nowait> j v:count ? 'j' : 'gj'").unwrap();
    match cmd {
        ExCommand::Map { flags, lhs, .. } => {
            assert!(flags.nowait);
            assert!(flags.expr);
            assert_eq!(lhs.as_str(), "j");
        }
        other => panic!("expected Map, got {other:?}"),
    }
}

#[test]
fn parses_map_without_expr_flag() {
    let cmd = parse_ex_command("nmap j gj").unwrap();
    assert!(
        matches!(
            cmd,
            ExCommand::Map {
                flags: MappingFlags { expr: false, .. },
                ..
            }
        ),
        "expected expr: false when <expr> flag absent, got: {cmd:?}"
    );
}

#[test]
fn parses_expr_flag_all_mode_prefixes() {
    // :map (all modes)
    let cmd = parse_ex_command("map <expr> j gj").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Map {
            mode_prefix: MapModePrefix::All,
            flags: MappingFlags { expr: true, .. },
            ..
        }
    ));

    // :vmap (visual)
    let cmd = parse_ex_command("vmap <expr> j gj").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Map {
            mode_prefix: MapModePrefix::Visual,
            flags: MappingFlags { expr: true, .. },
            ..
        }
    ));

    // :imap (insert)
    let cmd = parse_ex_command("imap <expr> j gj").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Map {
            mode_prefix: MapModePrefix::Insert,
            flags: MappingFlags { expr: true, .. },
            ..
        }
    ));

    // :omap (operator)
    let cmd = parse_ex_command("omap <expr> j gj").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Map {
            mode_prefix: MapModePrefix::Operator,
            flags: MappingFlags { expr: true, .. },
            ..
        }
    ));
}

#[test]
fn parses_noremap_expr_flag_all_mode_prefixes() {
    let cmd = parse_ex_command("noremap <expr> j gj").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Map {
            mode_prefix: MapModePrefix::All,
            kind: MappingKind::NonRecursive,
            flags: MappingFlags { expr: true, .. },
            ..
        }
    ));

    let cmd = parse_ex_command("vnoremap <expr> j gj").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Map {
            mode_prefix: MapModePrefix::Visual,
            kind: MappingKind::NonRecursive,
            flags: MappingFlags { expr: true, .. },
            ..
        }
    ));

    let cmd = parse_ex_command("inoremap <expr> j gj").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Map {
            mode_prefix: MapModePrefix::Insert,
            kind: MappingKind::NonRecursive,
            flags: MappingFlags { expr: true, .. },
            ..
        }
    ));

    let cmd = parse_ex_command("onoremap <expr> j gj").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Map {
            mode_prefix: MapModePrefix::Operator,
            kind: MappingKind::NonRecursive,
            flags: MappingFlags { expr: true, .. },
            ..
        }
    ));
}

#[test]
fn parses_map_list_with_no_args_has_expr_false() {
    let cmd = parse_ex_command("map").unwrap();
    assert!(
        matches!(
            cmd,
            ExCommand::Map {
                flags: MappingFlags {
                    expr: false,
                    nowait: false,
                    ..
                },
                ..
            }
        ),
        "bare :map listing should have expr: false"
    );
}

#[test]
fn parses_expr_preserves_full_rhs_expression() {
    // The RHS should be the full expression text, including any complex syntax
    let cmd = parse_ex_command("nnoremap <expr> <CR> pumvisible() ? '<C-Y>' : '<CR>'").unwrap();
    match cmd {
        ExCommand::Map {
            lhs, rhs, flags, ..
        } => {
            assert!(flags.expr);
            assert_eq!(lhs.as_str(), "<CR>");
            assert_eq!(rhs.as_deref(), Some("pumvisible() ? '<C-Y>' : '<CR>'"));
        }
        other => panic!("expected Map, got {other:?}"),
    }
}

// ─── <silent> mapping flag ──────────────────────────────────────────

#[test]
fn parses_nnoremap_silent_flag() {
    let cmd = parse_ex_command("nnoremap <silent> j gj").unwrap();
    match cmd {
        ExCommand::Map {
            mode_prefix,
            kind,
            flags,
            lhs,
            rhs,
        } => {
            assert_eq!(mode_prefix, MapModePrefix::Normal);
            assert!(matches!(kind, MappingKind::NonRecursive));
            assert!(flags.silent);
            assert!(!flags.expr);
            assert!(!flags.nowait);
            assert_eq!(lhs.as_str(), "j");
            assert_eq!(rhs.as_deref(), Some("gj"));
        }
        other => panic!("expected Map, got {other:?}"),
    }
}

#[test]
fn parses_nmap_silent_flag() {
    let cmd = parse_ex_command("nmap <silent> j gj").unwrap();
    match cmd {
        ExCommand::Map { kind, flags, .. } => {
            assert!(matches!(kind, MappingKind::Recursive));
            assert!(flags.silent);
        }
        other => panic!("expected Map, got {other:?}"),
    }
}

#[test]
fn parses_silent_flag_case_insensitive() {
    let cmd = parse_ex_command("nmap <SILENT> j gj").unwrap();
    assert!(
        matches!(
            cmd,
            ExCommand::Map {
                flags: MappingFlags { silent: true, .. },
                ..
            }
        ),
        "expected silent: true for <SILENT>, got: {cmd:?}"
    );

    let cmd2 = parse_ex_command("nmap <Silent> j gj").unwrap();
    assert!(
        matches!(
            cmd2,
            ExCommand::Map {
                flags: MappingFlags { silent: true, .. },
                ..
            }
        ),
        "expected silent: true for <Silent>, got: {cmd2:?}"
    );
}

#[test]
fn parses_silent_with_nowait_and_expr() {
    // All three flags together, different orders
    let cmd = parse_ex_command("nnoremap <silent> <nowait> <expr> j v:count").unwrap();
    match cmd {
        ExCommand::Map { flags, lhs, .. } => {
            assert!(flags.silent);
            assert!(flags.nowait);
            assert!(flags.expr);
            assert_eq!(lhs.as_str(), "j");
        }
        other => panic!("expected Map, got {other:?}"),
    }

    let cmd2 = parse_ex_command("nnoremap <expr> <silent> <nowait> j v:count").unwrap();
    assert!(matches!(
        cmd2,
        ExCommand::Map {
            flags: MappingFlags {
                silent: true,
                nowait: true,
                expr: true,
                ..
            },
            ..
        }
    ));

    let cmd3 = parse_ex_command("nnoremap <nowait> <expr> <silent> j v:count").unwrap();
    assert!(matches!(
        cmd3,
        ExCommand::Map {
            flags: MappingFlags {
                silent: true,
                nowait: true,
                expr: true,
                ..
            },
            ..
        }
    ));
}

#[test]
fn parses_map_without_silent_flag() {
    let cmd = parse_ex_command("nmap j gj").unwrap();
    assert!(
        matches!(
            cmd,
            ExCommand::Map {
                flags: MappingFlags { silent: false, .. },
                ..
            }
        ),
        "expected silent: false when flag absent, got: {cmd:?}"
    );
}

#[test]
fn parses_silent_flag_all_mode_prefixes() {
    let cmd = parse_ex_command("map <silent> j gj").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Map {
            mode_prefix: MapModePrefix::All,
            flags: MappingFlags { silent: true, .. },
            ..
        }
    ));

    let cmd = parse_ex_command("vmap <silent> j gj").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Map {
            mode_prefix: MapModePrefix::Visual,
            flags: MappingFlags { silent: true, .. },
            ..
        }
    ));

    let cmd = parse_ex_command("imap <silent> j gj").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Map {
            mode_prefix: MapModePrefix::Insert,
            flags: MappingFlags { silent: true, .. },
            ..
        }
    ));

    let cmd = parse_ex_command("omap <silent> j gj").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Map {
            mode_prefix: MapModePrefix::Operator,
            flags: MappingFlags { silent: true, .. },
            ..
        }
    ));
}

#[test]
fn parses_noremap_silent_flag_all_mode_prefixes() {
    let cmd = parse_ex_command("noremap <silent> j gj").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Map {
            mode_prefix: MapModePrefix::All,
            kind: MappingKind::NonRecursive,
            flags: MappingFlags { silent: true, .. },
            ..
        }
    ));

    let cmd = parse_ex_command("vnoremap <silent> j gj").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Map {
            mode_prefix: MapModePrefix::Visual,
            kind: MappingKind::NonRecursive,
            flags: MappingFlags { silent: true, .. },
            ..
        }
    ));

    let cmd = parse_ex_command("inoremap <silent> j gj").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Map {
            mode_prefix: MapModePrefix::Insert,
            kind: MappingKind::NonRecursive,
            flags: MappingFlags { silent: true, .. },
            ..
        }
    ));

    let cmd = parse_ex_command("onoremap <silent> j gj").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Map {
            mode_prefix: MapModePrefix::Operator,
            kind: MappingKind::NonRecursive,
            flags: MappingFlags { silent: true, .. },
            ..
        }
    ));
}

#[test]
fn parses_map_list_with_no_args_has_silent_false() {
    let cmd = parse_ex_command("map").unwrap();
    assert!(
        matches!(
            cmd,
            ExCommand::Map {
                flags: MappingFlags { silent: false, .. },
                ..
            }
        ),
        "bare :map listing should have silent: false"
    );
}

// ─── Bracket abbreviation tests ─────────────────────────────────

#[test]
fn bracket_abbrev_delete_intermediates() {
    // d[elete]: d, de, del, dele, delet, delete all work
    for abbrev in ["d", "de", "del", "dele", "delet", "delete"] {
        let input = format!("{abbrev} a");
        let cmd = parse_ex_command(&input).unwrap();
        assert!(
            matches!(cmd, ExCommand::Delete { .. }),
            "{abbrev:?} should parse as Delete, got: {cmd:?}"
        );
    }
}

#[test]
fn bracket_abbrev_yank_intermediates() {
    for abbrev in ["y", "ya", "yan", "yank"] {
        let input = format!("{abbrev} a");
        let cmd = parse_ex_command(&input).unwrap();
        assert!(
            matches!(cmd, ExCommand::Yank { .. }),
            "{abbrev:?} should parse as Yank, got: {cmd:?}"
        );
    }
}

#[test]
fn bracket_abbrev_write_intermediates() {
    for abbrev in ["w", "wr", "wri", "writ", "write"] {
        let cmd = parse_ex_command(abbrev).unwrap();
        assert!(
            matches!(cmd, ExCommand::Write { .. }),
            "{abbrev:?} should parse as Write, got: {cmd:?}"
        );
    }
}

#[test]
fn bracket_abbrev_quit_intermediates() {
    for abbrev in ["q", "qu", "qui", "quit"] {
        let cmd = parse_ex_command(abbrev).unwrap();
        assert!(
            matches!(cmd, ExCommand::Quit { .. }),
            "{abbrev:?} should parse as Quit, got: {cmd:?}"
        );
    }
}

#[test]
fn bracket_abbrev_nohlsearch_intermediates() {
    for abbrev in [
        "noh",
        "nohl",
        "nohls",
        "nohlse",
        "nohlsea",
        "nohlsear",
        "nohlsearc",
        "nohlsearch",
    ] {
        let cmd = parse_ex_command(abbrev).unwrap();
        assert!(
            matches!(cmd, ExCommand::NoHighlight),
            "{abbrev:?} should parse as NoHighlight, got: {cmd:?}"
        );
    }
}

#[test]
fn bracket_abbrev_source_intermediates() {
    for abbrev in ["so", "sou", "sour", "sourc", "source"] {
        let input = format!("{abbrev} /tmp/x");
        let cmd = parse_ex_command(&input).unwrap();
        assert!(
            matches!(cmd, ExCommand::Source { .. }),
            "{abbrev:?} should parse as Source, got: {cmd:?}"
        );
    }
}

#[test]
fn bracket_abbrev_sort_requires_sor() {
    // "so" is source, not sort — sort requires at least "sor"
    let cmd = parse_ex_command("so /tmp/x").unwrap();
    assert!(matches!(cmd, ExCommand::Source { .. }));

    // "sor" and "sort" are sort
    for abbrev in ["sor", "sort"] {
        let cmd = parse_ex_command(abbrev).unwrap();
        assert!(
            matches!(cmd, ExCommand::Sort { .. }),
            "{abbrev:?} should parse as Sort, got: {cmd:?}"
        );
    }
}

#[test]
fn bracket_abbrev_sethandler_before_set() {
    // "se" and "set" are :set
    assert!(matches!(
        parse_ex_command("se").unwrap(),
        ExCommand::Set { .. }
    ));
    assert!(matches!(
        parse_ex_command("set").unwrap(),
        ExCommand::Set { .. }
    ));

    // "seth" through "sethandler" are :sethandler
    for abbrev in [
        "seth",
        "setha",
        "sethan",
        "sethand",
        "sethandl",
        "sethandle",
        "sethandler",
    ] {
        let input = format!("{abbrev} <C-A> n:vim");
        let cmd = parse_ex_command(&input).unwrap();
        assert!(
            matches!(cmd, ExCommand::SetHandler { .. }),
            "{abbrev:?} should parse as SetHandler, got: {cmd:?}"
        );
    }
}

#[test]
fn bracket_abbrev_earlier_intermediates() {
    for abbrev in ["ea", "ear", "earl", "earli", "earlie", "earlier"] {
        let cmd = parse_ex_command(abbrev).unwrap();
        assert!(
            matches!(cmd, ExCommand::Earlier { .. }),
            "{abbrev:?} should parse as Earlier, got: {cmd:?}"
        );
    }
}

#[test]
fn bracket_abbrev_later_intermediates() {
    for abbrev in ["lat", "late", "later"] {
        let cmd = parse_ex_command(abbrev).unwrap();
        assert!(
            matches!(cmd, ExCommand::Later { .. }),
            "{abbrev:?} should parse as Later, got: {cmd:?}"
        );
    }
}

#[test]
fn bracket_abbrev_bprevious_intermediates() {
    // bp[revious]: bp, bpr, bpre, bprev, bprevi, bprevio, bpreviou, bprevious
    for abbrev in [
        "bp",
        "bpr",
        "bpre",
        "bprev",
        "bprevi",
        "bprevio",
        "bpreviou",
        "bprevious",
    ] {
        let input = format!("{abbrev} 1");
        let cmd = parse_ex_command(&input).unwrap();
        assert!(
            matches!(cmd, ExCommand::BufferPrev { .. }),
            "{abbrev:?} should parse as BufferPrev, got: {cmd:?}"
        );
    }
}

#[test]
fn bracket_abbrev_tabprevious_intermediates() {
    for abbrev in [
        "tabp",
        "tabpr",
        "tabpre",
        "tabprev",
        "tabprevi",
        "tabprevio",
        "tabpreviou",
        "tabprevious",
    ] {
        let cmd = parse_ex_command(abbrev).unwrap();
        assert!(
            matches!(cmd, ExCommand::TabPrev { .. }),
            "{abbrev:?} should parse as TabPrev, got: {cmd:?}"
        );
    }
}

#[test]
fn bracket_abbrev_registers_intermediates() {
    for abbrev in [
        "reg",
        "regi",
        "regis",
        "regist",
        "registe",
        "register",
        "registers",
    ] {
        let cmd = parse_ex_command(abbrev).unwrap();
        assert!(
            matches!(cmd, ExCommand::Registers { .. }),
            "{abbrev:?} should parse as Registers, got: {cmd:?}"
        );
    }
}

#[test]
fn bracket_abbrev_display_intermediates() {
    for abbrev in ["di", "dis", "disp", "displ", "displa", "display"] {
        let cmd = parse_ex_command(abbrev).unwrap();
        assert!(
            matches!(cmd, ExCommand::Registers { .. }),
            "{abbrev:?} should parse as Registers (display alias), got: {cmd:?}"
        );
    }
}

#[test]
fn bracket_abbrev_marks_intermediates() {
    for abbrev in ["mar", "mark", "marks"] {
        let cmd = parse_ex_command(abbrev).unwrap();
        assert!(
            matches!(cmd, ExCommand::Marks { .. }),
            "{abbrev:?} should parse as Marks, got: {cmd:?}"
        );
    }
}

#[test]
fn bracket_abbrev_messages_intermediates() {
    for abbrev in ["mes", "mess", "messa", "messag", "message", "messages"] {
        let cmd = parse_ex_command(abbrev).unwrap();
        assert!(
            matches!(cmd, ExCommand::Messages { .. }),
            "{abbrev:?} should parse as Messages, got: {cmd:?}"
        );
    }
}

#[test]
fn bracket_abbrev_noremap_intermediates() {
    for abbrev in ["no", "nor", "nore", "norem", "norema", "noremap"] {
        let input = format!("{abbrev} j gj");
        let cmd = parse_ex_command(&input).unwrap();
        assert!(
            matches!(
                cmd,
                ExCommand::Map {
                    mode_prefix: MapModePrefix::All,
                    kind: MappingKind::NonRecursive,
                    ..
                }
            ),
            "{abbrev:?} should parse as noremap, got: {cmd:?}"
        );
    }
}

#[test]
fn bracket_abbrev_case_insensitive() {
    // Abbreviations should be case-insensitive
    assert!(matches!(
        parse_ex_command("DELETE a").unwrap(),
        ExCommand::Delete { .. }
    ));
    assert!(matches!(
        parse_ex_command("Del a").unwrap(),
        ExCommand::Delete { .. }
    ));
    assert!(matches!(
        parse_ex_command("WRITE").unwrap(),
        ExCommand::Write { .. }
    ));
    assert!(matches!(
        parse_ex_command("Wri").unwrap(),
        ExCommand::Write { .. }
    ));
}

#[test]
fn bracket_abbrev_undolist_undotree_disambiguated() {
    // undol[ist] vs undot[ree] — different min prefixes
    for abbrev in ["undol", "undoli", "undolis", "undolist"] {
        let cmd = parse_ex_command(abbrev).unwrap();
        assert!(
            matches!(cmd, ExCommand::UndoList),
            "{abbrev:?} should parse as UndoList, got: {cmd:?}"
        );
    }
    for abbrev in ["undot", "undotr", "undotre", "undotree"] {
        let cmd = parse_ex_command(abbrev).unwrap();
        assert!(
            matches!(cmd, ExCommand::UndoTree),
            "{abbrev:?} should parse as UndoTree, got: {cmd:?}"
        );
    }
}

#[test]
fn bracket_abbrev_changes_intermediates() {
    for abbrev in ["cha", "chan", "chang", "change", "changes"] {
        let cmd = parse_ex_command(abbrev).unwrap();
        assert!(
            matches!(cmd, ExCommand::Changes),
            "{abbrev:?} should parse as Changes, got: {cmd:?}"
        );
    }
}

#[test]
fn bracket_abbrev_copy_t_alias_still_works() {
    // "t" is a standalone alias for copy, not an abbreviation
    let cmd = parse_ex_command("t 5").unwrap();
    assert!(matches!(cmd, ExCommand::Copy { .. }));

    // "co" through "copy" are abbreviations
    for abbrev in ["co", "cop", "copy"] {
        let input = format!("{abbrev} 5");
        let cmd = parse_ex_command(&input).unwrap();
        assert!(
            matches!(cmd, ExCommand::Copy { .. }),
            "{abbrev:?} should parse as Copy, got: {cmd:?}"
        );
    }
}

#[test]
fn matches_abbrev_helper_unit_tests() {
    // Direct tests of the helper function
    assert!(matches_abbrev("d", "d", "delete"));
    assert!(matches_abbrev("del", "d", "delete"));
    assert!(matches_abbrev("delete", "d", "delete"));
    assert!(matches_abbrev("DELETE", "d", "delete")); // case insensitive
    assert!(!matches_abbrev("deletes", "d", "delete")); // too long
    assert!(!matches_abbrev("x", "d", "delete")); // wrong prefix
    assert!(!matches_abbrev("", "d", "delete")); // too short

    // Disambiguating sort/source
    assert!(matches_abbrev("so", "so", "source"));
    assert!(!matches_abbrev("so", "sor", "sort")); // below min
    assert!(matches_abbrev("sor", "sor", "sort"));
    assert!(!matches_abbrev("sor", "so", "source")); // "source"[..3] = "sou" != "sor"

    // set vs sethandler
    assert!(matches_abbrev("se", "se", "set"));
    assert!(matches_abbrev("set", "se", "set"));
    assert!(!matches_abbrev("seth", "se", "set")); // 4 > 3
    assert!(matches_abbrev("seth", "seth", "sethandler"));
}

#[test]
fn parses_repeat_substitute_bare() {
    // `:&` — repeat without flags
    let cmd = parse_ex_command("&").unwrap();
    assert!(
        matches!(
            cmd,
            ExCommand::RepeatSubstitute {
                range: None,
                use_previous_flags: false,
            }
        ),
        "got {cmd:?}"
    );
}

#[test]
fn parses_repeat_substitute_with_flags() {
    // `:&&` — repeat keeping previous flags
    let cmd = parse_ex_command("&&").unwrap();
    assert!(
        matches!(
            cmd,
            ExCommand::RepeatSubstitute {
                range: None,
                use_previous_flags: true,
            }
        ),
        "got {cmd:?}"
    );
}

#[test]
fn parses_repeat_substitute_with_range() {
    // `:%&` — repeat on whole file, no flags
    let cmd = parse_ex_command("%&").unwrap();
    assert!(
        matches!(
            cmd,
            ExCommand::RepeatSubstitute {
                range: Some(_),
                use_previous_flags: false,
            }
        ),
        "got {cmd:?}"
    );

    // `:%&&` — repeat on whole file, keep flags
    let cmd2 = parse_ex_command("%&&").unwrap();
    assert!(
        matches!(
            cmd2,
            ExCommand::RepeatSubstitute {
                range: Some(_),
                use_previous_flags: true,
            }
        ),
        "got {cmd2:?}"
    );
}

// ─── :~ (SubTilde) ────────────────────────────────────────────

#[test]
fn parses_subtilde_bare() {
    let cmd = parse_ex_command("~").unwrap();
    assert!(
        matches!(cmd, ExCommand::SubTilde { ref range, flags } if flags == SubFlags::default() && *range == ExRange::current_line()),
        "got {cmd:?}"
    );
}

#[test]
fn parses_subtilde_with_flags() {
    let cmd = parse_ex_command("~g").unwrap();
    match cmd {
        ExCommand::SubTilde { flags, .. } => {
            assert!(flags.global(), "expected global flag");
        }
        other => panic!("expected SubTilde, got {other:?}"),
    }
}

#[test]
fn parses_subtilde_with_range() {
    let cmd = parse_ex_command("%~gi").unwrap();
    match cmd {
        ExCommand::SubTilde { range, flags } => {
            assert_eq!(range, ExRange::entire_file());
            assert!(flags.global(), "expected global flag");
        }
        other => panic!("expected SubTilde, got {other:?}"),
    }
}

// ─── :setlocal / :setglobal ─────────────────────────────────────

#[test]
fn setlocal_tabstop_assign() {
    let cmd = parse_ex_command("setlocal tabstop=4").unwrap();
    match cmd {
        ExCommand::SetLocal { assignments } => {
            assert_eq!(assignments.len(), 1);
            assert!(
                matches!(&assignments[0], SetAssignment::Assign(name, val) if name.as_str() == "tabstop" && val.as_str() == "4"),
                "expected Assign(tabstop, 4), got {:?}",
                assignments[0]
            );
        }
        other => panic!("expected SetLocal, got {other:?}"),
    }
}

#[test]
fn setglobal_ignorecase_bool() {
    let cmd = parse_ex_command("setglobal ic").unwrap();
    match cmd {
        ExCommand::SetGlobal { assignments } => {
            assert_eq!(assignments.len(), 1);
            assert!(
                matches!(&assignments[0], SetAssignment::SetBool(name) if name.as_str() == "ic"),
                "expected SetBool(ic), got {:?}",
                assignments[0]
            );
        }
        other => panic!("expected SetGlobal, got {other:?}"),
    }
}

#[test]
fn setl_abbreviation_unsetbool() {
    let cmd = parse_ex_command("setl noai").unwrap();
    match cmd {
        ExCommand::SetLocal { assignments } => {
            assert_eq!(assignments.len(), 1);
            assert!(
                matches!(&assignments[0], SetAssignment::UnsetBool(name) if name.as_str() == "ai"),
                "expected UnsetBool(ai), got {:?}",
                assignments[0]
            );
        }
        other => panic!("expected SetLocal, got {other:?}"),
    }
}

#[test]
fn setg_abbreviation_assign() {
    let cmd = parse_ex_command("setg tw=80").unwrap();
    match cmd {
        ExCommand::SetGlobal { assignments } => {
            assert_eq!(assignments.len(), 1);
            assert!(
                matches!(&assignments[0], SetAssignment::Assign(name, val) if name.as_str() == "tw" && val.as_str() == "80"),
                "expected Assign(tw, 80), got {:?}",
                assignments[0]
            );
        }
        other => panic!("expected SetGlobal, got {other:?}"),
    }
}

#[test]
fn setlocal_no_args_shows_all() {
    let cmd = parse_ex_command("setlocal").unwrap();
    match cmd {
        ExCommand::SetLocal { assignments } => {
            assert_eq!(assignments.len(), 1);
            assert!(
                matches!(&assignments[0], SetAssignment::ShowAll),
                "expected ShowAll, got {:?}",
                assignments[0]
            );
        }
        other => panic!("expected SetLocal, got {other:?}"),
    }
}

#[test]
fn setlocal_query() {
    let cmd = parse_ex_command("setlocal tabstop?").unwrap();
    match cmd {
        ExCommand::SetLocal { assignments } => {
            assert_eq!(assignments.len(), 1);
            assert!(
                matches!(&assignments[0], SetAssignment::Query(name) if name.as_str() == "tabstop"),
                "expected Query(tabstop), got {:?}",
                assignments[0]
            );
        }
        other => panic!("expected SetLocal, got {other:?}"),
    }
}

#[test]
fn setlocal_abbreviations_all_accepted() {
    for abbrev in ["setl", "setlo", "setloc", "setloca", "setlocal"] {
        let input = format!("{abbrev} tabstop=2");
        let cmd = parse_ex_command(&input).unwrap();
        assert!(
            matches!(cmd, ExCommand::SetLocal { .. }),
            "{abbrev:?} should parse as SetLocal, got: {cmd:?}"
        );
    }
}

#[test]
fn setglobal_abbreviations_all_accepted() {
    for abbrev in [
        "setg",
        "setgl",
        "setglo",
        "setglob",
        "setgloba",
        "setglobal",
    ] {
        let input = format!("{abbrev} tabstop=2");
        let cmd = parse_ex_command(&input).unwrap();
        assert!(
            matches!(cmd, ExCommand::SetGlobal { .. }),
            "{abbrev:?} should parse as SetGlobal, got: {cmd:?}"
        );
    }
}

#[test]
fn set_still_works_after_setlocal_setglobal_added() {
    // Ensure "se" and "set" still parse as Set (not SetLocal/SetGlobal)
    assert!(matches!(
        parse_ex_command("se").unwrap(),
        ExCommand::Set { .. }
    ));
    assert!(matches!(
        parse_ex_command("set").unwrap(),
        ExCommand::Set { .. }
    ));
    assert!(matches!(
        parse_ex_command("set tabstop=4").unwrap(),
        ExCommand::Set { .. }
    ));
}

#[test]
fn setlocal_does_not_match_set_abbrev() {
    // "setl" must not be parsed as Set (which only accepts 2-3 char prefixes of "set")
    let cmd = parse_ex_command("setl tabstop=4").unwrap();
    assert!(
        matches!(cmd, ExCommand::SetLocal { .. }),
        "setl should be SetLocal, not Set; got: {cmd:?}"
    );
}

// ─── Window / session / buffer management ──────────────────────

#[test]
fn parses_split_no_path() {
    let cmd = parse_ex_command("sp").unwrap();
    assert!(matches!(cmd, ExCommand::Split { path: None }));
}

#[test]
fn parses_split_with_path() {
    let cmd = parse_ex_command("sp foo.rs").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Split { path: Some(p) } if p.as_str() == "foo.rs"
    ));
}

#[test]
fn parses_split_abbreviations() {
    for abbrev in ["sp", "spl", "spli", "split"] {
        let cmd = parse_ex_command(abbrev).unwrap();
        assert!(
            matches!(cmd, ExCommand::Split { .. }),
            "{abbrev:?} should parse as Split, got: {cmd:?}"
        );
    }
}

#[test]
fn parses_vsplit_no_path() {
    let cmd = parse_ex_command("vs").unwrap();
    assert!(matches!(cmd, ExCommand::VSplit { path: None }));
}

#[test]
fn parses_vsplit_with_path() {
    let cmd = parse_ex_command("vsplit bar.rs").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::VSplit { path: Some(p) } if p.as_str() == "bar.rs"
    ));
}

#[test]
fn parses_vsplit_abbreviations() {
    for abbrev in ["vs", "vsp", "vspl", "vspli", "vsplit"] {
        let cmd = parse_ex_command(abbrev).unwrap();
        assert!(
            matches!(cmd, ExCommand::VSplit { .. }),
            "{abbrev:?} should parse as VSplit, got: {cmd:?}"
        );
    }
}

#[test]
fn parses_close_without_bang() {
    let cmd = parse_ex_command("clo").unwrap();
    assert!(matches!(cmd, ExCommand::Close { force: false }));
}

#[test]
fn parses_close_with_bang() {
    let cmd = parse_ex_command("close!").unwrap();
    assert!(matches!(cmd, ExCommand::Close { force: true }));
}

#[test]
fn parses_close_abbreviations() {
    for abbrev in ["clo", "clos", "close"] {
        let cmd = parse_ex_command(abbrev).unwrap();
        assert!(
            matches!(cmd, ExCommand::Close { .. }),
            "{abbrev:?} should parse as Close, got: {cmd:?}"
        );
    }
}

#[test]
fn parses_only_without_bang() {
    let cmd = parse_ex_command("on").unwrap();
    assert!(matches!(cmd, ExCommand::Only { force: false }));
}

#[test]
fn parses_only_with_bang() {
    let cmd = parse_ex_command("only!").unwrap();
    assert!(matches!(cmd, ExCommand::Only { force: true }));
}

#[test]
fn parses_only_abbreviations() {
    for abbrev in ["on", "onl", "only"] {
        let cmd = parse_ex_command(abbrev).unwrap();
        assert!(
            matches!(cmd, ExCommand::Only { .. }),
            "{abbrev:?} should parse as Only, got: {cmd:?}"
        );
    }
}

#[test]
fn parses_new() {
    let cmd = parse_ex_command("new").unwrap();
    assert!(matches!(cmd, ExCommand::New));
}

#[test]
fn parses_vnew() {
    let cmd = parse_ex_command("vne").unwrap();
    assert!(matches!(cmd, ExCommand::VNew));
}

#[test]
fn parses_vnew_abbreviations() {
    for abbrev in ["vne", "vnew"] {
        let cmd = parse_ex_command(abbrev).unwrap();
        assert!(
            matches!(cmd, ExCommand::VNew),
            "{abbrev:?} should parse as VNew, got: {cmd:?}"
        );
    }
}

#[test]
fn parses_wall() {
    let cmd = parse_ex_command("wa").unwrap();
    assert!(matches!(cmd, ExCommand::WriteAll));
}

#[test]
fn parses_wall_abbreviations() {
    for abbrev in ["wa", "wal", "wall"] {
        let cmd = parse_ex_command(abbrev).unwrap();
        assert!(
            matches!(cmd, ExCommand::WriteAll),
            "{abbrev:?} should parse as WriteAll, got: {cmd:?}"
        );
    }
}

#[test]
fn parses_qall_without_bang() {
    let cmd = parse_ex_command("qa").unwrap();
    assert!(matches!(cmd, ExCommand::QuitAll { force: false }));
}

#[test]
fn parses_qall_with_bang() {
    let cmd = parse_ex_command("qa!").unwrap();
    assert!(matches!(cmd, ExCommand::QuitAll { force: true }));
}

#[test]
fn parses_qall_abbreviations() {
    for abbrev in ["qa", "qal", "qall"] {
        let cmd = parse_ex_command(abbrev).unwrap();
        assert!(
            matches!(cmd, ExCommand::QuitAll { .. }),
            "{abbrev:?} should parse as QuitAll, got: {cmd:?}"
        );
    }
}

#[test]
fn parses_wqall() {
    let cmd = parse_ex_command("wqall").unwrap();
    assert!(matches!(cmd, ExCommand::WriteQuitAll));
}

#[test]
fn parses_wqall_abbreviations() {
    for abbrev in ["wqa", "wqal", "wqall"] {
        let cmd = parse_ex_command(abbrev).unwrap();
        assert!(
            matches!(cmd, ExCommand::WriteQuitAll),
            "{abbrev:?} should parse as WriteQuitAll, got: {cmd:?}"
        );
    }
}

#[test]
fn parses_xall() {
    let cmd = parse_ex_command("xall").unwrap();
    assert!(matches!(cmd, ExCommand::WriteQuitAll));
}

#[test]
fn parses_xall_abbreviations() {
    for abbrev in ["xa", "xal", "xall"] {
        let cmd = parse_ex_command(abbrev).unwrap();
        assert!(
            matches!(cmd, ExCommand::WriteQuitAll),
            "{abbrev:?} should parse as WriteQuitAll, got: {cmd:?}"
        );
    }
}

#[test]
fn parses_bdelete_no_target() {
    let cmd = parse_ex_command("bd").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::BufferDelete {
            force: false,
            target: None
        }
    ));
}

#[test]
fn parses_bdelete_with_target() {
    let cmd = parse_ex_command("bd 3").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::BufferDelete { force: false, target: Some(t) } if t.as_str() == "3"
    ));
}

#[test]
fn parses_bdelete_with_bang() {
    let cmd = parse_ex_command("bd!").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::BufferDelete {
            force: true,
            target: None
        }
    ));
}

#[test]
fn parses_bdelete_with_bang_and_target() {
    let cmd = parse_ex_command("bd! main.rs").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::BufferDelete { force: true, target: Some(t) } if t.as_str() == "main.rs"
    ));
}

#[test]
fn parses_bdelete_abbreviations() {
    for abbrev in ["bd", "bde", "bdel", "bdele", "bdelet", "bdelete"] {
        let cmd = parse_ex_command(abbrev).unwrap();
        assert!(
            matches!(cmd, ExCommand::BufferDelete { .. }),
            "{abbrev:?} should parse as BufferDelete, got: {cmd:?}"
        );
    }
}

#[test]
fn parses_bwipeout_no_target() {
    let cmd = parse_ex_command("bw").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::BufferWipeout {
            force: false,
            target: None
        }
    ));
}

#[test]
fn parses_bwipeout_with_target() {
    let cmd = parse_ex_command("bw 5").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::BufferWipeout { force: false, target: Some(t) } if t.as_str() == "5"
    ));
}

#[test]
fn parses_bwipeout_with_bang() {
    let cmd = parse_ex_command("bw!").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::BufferWipeout {
            force: true,
            target: None
        }
    ));
}

#[test]
fn parses_bwipeout_abbreviations() {
    for abbrev in [
        "bw", "bwi", "bwip", "bwipe", "bwipeo", "bwipeou", "bwipeout",
    ] {
        let cmd = parse_ex_command(abbrev).unwrap();
        assert!(
            matches!(cmd, ExCommand::BufferWipeout { .. }),
            "{abbrev:?} should parse as BufferWipeout, got: {cmd:?}"
        );
    }
}

#[test]
fn new_commands_do_not_mutate_text() {
    // All these commands manage windows/buffers, not text
    let non_mutating: Vec<ExCommand> = vec![
        ExCommand::Split { path: None },
        ExCommand::VSplit { path: None },
        ExCommand::Close { force: false },
        ExCommand::Only { force: false },
        ExCommand::New,
        ExCommand::VNew,
        ExCommand::WriteAll,
        ExCommand::QuitAll { force: false },
        ExCommand::WriteQuitAll,
        ExCommand::BufferDelete {
            force: false,
            target: None,
        },
        ExCommand::BufferWipeout {
            force: false,
            target: None,
        },
    ];
    for cmd in &non_mutating {
        assert!(!cmd.mutates_text(), "{cmd:?} should NOT mutate text");
    }
}

#[test]
fn split_not_parsed_as_custom() {
    let cmd = parse_ex_command("sp").unwrap();
    assert!(
        !matches!(cmd, ExCommand::Custom { .. }),
        "sp should be Split, not Custom"
    );
}

#[test]
fn close_not_parsed_as_custom() {
    let cmd = parse_ex_command("clo").unwrap();
    assert!(
        !matches!(cmd, ExCommand::Custom { .. }),
        "clo should be Close, not Custom"
    );
}

#[test]
fn qall_not_parsed_as_custom() {
    let cmd = parse_ex_command("qa").unwrap();
    assert!(
        !matches!(cmd, ExCommand::Custom { .. }),
        "qa should be QuitAll, not Custom"
    );
}

#[test]
fn bdelete_trims_target_whitespace() {
    let cmd = parse_ex_command("bd  3 ").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::BufferDelete { target: Some(t), .. } if t.as_str() == "3"
    ));
}

// ─── Abbreviation ex commands ─────────────────────────────────

#[test]
fn parses_abbreviate_define() {
    let cmd = parse_ex_command("ab teh the").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Abbreviate {
            trigger: Some(ref t),
            replacement: Some(ref r),
            mode: AbbrevMode::Both,
            noremap: false,
        } if t.as_str() == "teh" && r.as_str() == "the"
    ));
}

#[test]
fn parses_abbreviate_full_name() {
    let cmd = parse_ex_command("abbreviate teh the").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Abbreviate {
            trigger: Some(ref t),
            replacement: Some(ref r),
            mode: AbbrevMode::Both,
            noremap: false,
        } if t.as_str() == "teh" && r.as_str() == "the"
    ));
}

#[test]
fn parses_abbreviate_list() {
    let cmd = parse_ex_command("ab").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Abbreviate {
            trigger: None,
            replacement: None,
            mode: AbbrevMode::Both,
            noremap: false,
        }
    ));
}

#[test]
fn parses_abbreviate_show_single() {
    let cmd = parse_ex_command("ab teh").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Abbreviate {
            trigger: Some(ref t),
            replacement: None,
            mode: AbbrevMode::Both,
            noremap: false,
        } if t.as_str() == "teh"
    ));
}

#[test]
fn parses_iabbrev() {
    let cmd = parse_ex_command("iab teh the").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Abbreviate {
            mode: AbbrevMode::Insert,
            noremap: false,
            ..
        }
    ));
}

#[test]
fn parses_iabbrev_full_name() {
    let cmd = parse_ex_command("iabbrev teh the").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Abbreviate {
            mode: AbbrevMode::Insert,
            noremap: false,
            ..
        }
    ));
}

#[test]
fn parses_cabbrev() {
    let cmd = parse_ex_command("cab teh the").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Abbreviate {
            mode: AbbrevMode::CommandLine,
            noremap: false,
            ..
        }
    ));
}

#[test]
fn parses_cabbrev_full_name() {
    let cmd = parse_ex_command("cabbrev teh the").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Abbreviate {
            mode: AbbrevMode::CommandLine,
            noremap: false,
            ..
        }
    ));
}

#[test]
fn parses_noreabbrev() {
    let cmd = parse_ex_command("norea teh the").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Abbreviate {
            mode: AbbrevMode::Both,
            noremap: true,
            ..
        }
    ));
}

#[test]
fn parses_noreabbrev_full_name() {
    let cmd = parse_ex_command("noreabbrev teh the").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Abbreviate {
            mode: AbbrevMode::Both,
            noremap: true,
            ..
        }
    ));
}

#[test]
fn parses_inoreabbrev() {
    let cmd = parse_ex_command("inorea teh the").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Abbreviate {
            mode: AbbrevMode::Insert,
            noremap: true,
            ..
        }
    ));
}

#[test]
fn parses_cnoreabbrev() {
    let cmd = parse_ex_command("cnorea teh the").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Abbreviate {
            mode: AbbrevMode::CommandLine,
            noremap: true,
            ..
        }
    ));
}

#[test]
fn parses_unabbreviate() {
    let cmd = parse_ex_command("una teh").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Unabbreviate {
            trigger: ref t,
            mode: AbbrevMode::Both,
        } if t.as_str() == "teh"
    ));
}

#[test]
fn parses_unabbreviate_full_name() {
    let cmd = parse_ex_command("unabbreviate teh").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Unabbreviate {
            trigger: ref t,
            mode: AbbrevMode::Both,
        } if t.as_str() == "teh"
    ));
}

#[test]
fn parses_iunabbrev() {
    let cmd = parse_ex_command("iuna teh").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Unabbreviate {
            mode: AbbrevMode::Insert,
            ..
        }
    ));
}

#[test]
fn parses_cunabbrev() {
    let cmd = parse_ex_command("cuna teh").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Unabbreviate {
            mode: AbbrevMode::CommandLine,
            ..
        }
    ));
}

#[test]
fn parses_unabbreviate_requires_arg() {
    let result = parse_ex_command("una");
    assert!(result.is_err());
}

#[test]
fn parses_abclear() {
    let cmd = parse_ex_command("abc").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::AbClear {
            mode: AbbrevMode::Both
        }
    ));
}

#[test]
fn parses_abclear_full_name() {
    let cmd = parse_ex_command("abclear").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::AbClear {
            mode: AbbrevMode::Both
        }
    ));
}

#[test]
fn parses_iabclear() {
    let cmd = parse_ex_command("iabc").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::AbClear {
            mode: AbbrevMode::Insert
        }
    ));
}

#[test]
fn parses_cabclear() {
    let cmd = parse_ex_command("cabc").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::AbClear {
            mode: AbbrevMode::CommandLine
        }
    ));
}

#[test]
fn abbreviate_does_not_mutate_text() {
    let cmd = ExCommand::Abbreviate {
        trigger: Some("teh".into()),
        replacement: Some("the".into()),
        mode: AbbrevMode::Both,
        noremap: false,
    };
    assert!(!cmd.mutates_text());
}

#[test]
fn unabbreviate_does_not_mutate_text() {
    let cmd = ExCommand::Unabbreviate {
        trigger: "teh".into(),
        mode: AbbrevMode::Both,
    };
    assert!(!cmd.mutates_text());
}

#[test]
fn abclear_does_not_mutate_text() {
    let cmd = ExCommand::AbClear {
        mode: AbbrevMode::Both,
    };
    assert!(!cmd.mutates_text());
}

#[test]
fn abbreviate_no_prefix_conflict_with_noremap() {
    // :noremap should still parse as noremap, not noreabbrev
    let cmd = parse_ex_command("noremap j gj").unwrap();
    assert!(matches!(cmd, ExCommand::Map { .. }));
}

#[test]
fn abbreviate_no_prefix_conflict_with_inoremap() {
    // :inoremap should still parse as inoremap, not inoreabbrev
    let cmd = parse_ex_command("inoremap jk <Esc>").unwrap();
    assert!(matches!(cmd, ExCommand::Map { .. }));
}

#[test]
fn abbreviate_no_prefix_conflict_with_cnoremap() {
    // :cnoremap should still parse as cnoremap, not cnoreabbrev
    let cmd = parse_ex_command("cnoremap jk <Esc>").unwrap();
    assert!(matches!(cmd, ExCommand::Map { .. }));
}

#[test]
fn abbreviate_no_prefix_conflict_with_unmap() {
    // :unmap should still parse as unmap, not unabbreviate
    let cmd = parse_ex_command("unmap j").unwrap();
    assert!(matches!(cmd, ExCommand::Unmap { .. }));
}

#[test]
fn abbreviate_no_prefix_conflict_with_iunmap() {
    // :iunmap should still parse as iunmap, not iunabbrev
    let cmd = parse_ex_command("iunmap j").unwrap();
    assert!(matches!(cmd, ExCommand::Unmap { .. }));
}

#[test]
fn abbreviate_no_prefix_conflict_with_cunmap() {
    // :cunmap should still parse as cunmap, not cunabbrev
    let cmd = parse_ex_command("cunmap j").unwrap();
    assert!(matches!(cmd, ExCommand::Unmap { .. }));
}

#[test]
fn abbreviate_replacement_with_spaces() {
    // Replacement can contain spaces (everything after first whitespace gap)
    let cmd = parse_ex_command("ab hw hello world").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Abbreviate {
            trigger: Some(ref t),
            replacement: Some(ref r),
            ..
        } if t.as_str() == "hw" && r.as_str() == "hello world"
    ));
}

#[test]
fn parses_ia_short_form() {
    // :ia is the shortest abbreviation for :iabbrev
    let cmd = parse_ex_command("ia teh the").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Abbreviate {
            mode: AbbrevMode::Insert,
            ..
        }
    ));
}

#[test]
fn parses_ca_short_form() {
    // :ca is the shortest abbreviation for :cabbrev
    let cmd = parse_ex_command("ca teh the").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Abbreviate {
            mode: AbbrevMode::CommandLine,
            ..
        }
    ));
}

// ─── Multi-cursor selection ex commands ────────────────────────

mod multi_cursor_ex {
    use super::*;

    #[test]
    fn parses_select_with_pattern() {
        let cmd = parse_ex_command("select /foo/").unwrap();
        assert!(matches!(
            cmd,
            ExCommand::SelectMatches { pattern, .. } if pattern == "foo"
        ));
    }

    #[test]
    fn parses_select_abbreviated() {
        let cmd = parse_ex_command("sel /bar/").unwrap();
        assert!(matches!(
            cmd,
            ExCommand::SelectMatches { pattern, .. } if pattern == "bar"
        ));
    }

    #[test]
    fn parses_select_all_abbreviations() {
        for abbrev in ["sel", "sele", "selec", "select"] {
            let input = format!("{abbrev} /x/");
            let cmd = parse_ex_command(&input).unwrap();
            assert!(
                matches!(cmd, ExCommand::SelectMatches { .. }),
                "{abbrev:?} should parse as SelectMatches, got: {cmd:?}"
            );
        }
    }

    #[test]
    fn parses_select_alternate_delimiter() {
        let cmd = parse_ex_command("select |foo|").unwrap();
        assert!(matches!(
            cmd,
            ExCommand::SelectMatches { pattern, .. } if pattern == "foo"
        ));
    }

    #[test]
    fn parses_split_with_pattern_as_split_matches() {
        let cmd = parse_ex_command("split /pattern/").unwrap();
        assert!(matches!(
            cmd,
            ExCommand::SplitMatches { pattern, .. } if pattern == "pattern"
        ));
    }

    #[test]
    fn parses_split_no_pattern_as_window_split() {
        let cmd = parse_ex_command("split").unwrap();
        assert!(matches!(cmd, ExCommand::Split { path: None }));
    }

    #[test]
    fn parses_split_with_path_as_window_split() {
        let cmd = parse_ex_command("split foo.rs").unwrap();
        assert!(matches!(
            cmd,
            ExCommand::Split { path: Some(p) } if p.as_str() == "foo.rs"
        ));
    }

    #[test]
    fn parses_sp_with_pattern_as_split_matches() {
        let cmd = parse_ex_command("sp /test/").unwrap();
        assert!(matches!(
            cmd,
            ExCommand::SplitMatches { pattern, .. } if pattern == "test"
        ));
    }

    #[test]
    fn parses_sp_no_pattern_as_window_split() {
        let cmd = parse_ex_command("sp").unwrap();
        assert!(matches!(cmd, ExCommand::Split { path: None }));
    }

    #[test]
    fn parses_keep_with_pattern() {
        let cmd = parse_ex_command("keep /test/").unwrap();
        assert!(matches!(
            cmd,
            ExCommand::KeepMatches { pattern, .. } if pattern == "test"
        ));
    }

    #[test]
    fn parses_keep_abbreviated() {
        let cmd = parse_ex_command("kee /x/").unwrap();
        assert!(matches!(
            cmd,
            ExCommand::KeepMatches { pattern, .. } if pattern == "x"
        ));
    }

    #[test]
    fn parses_remove_with_pattern() {
        let cmd = parse_ex_command("remove /dead/").unwrap();
        assert!(matches!(
            cmd,
            ExCommand::RemoveMatches { pattern, .. } if pattern == "dead"
        ));
    }

    #[test]
    fn parses_remove_abbreviated() {
        let cmd = parse_ex_command("remo /x/").unwrap();
        assert!(matches!(
            cmd,
            ExCommand::RemoveMatches { pattern, .. } if pattern == "x"
        ));
    }

    #[test]
    fn parses_trim() {
        let cmd = parse_ex_command("trim").unwrap();
        assert!(matches!(cmd, ExCommand::TrimSelections));
    }

    #[test]
    fn parses_trim_abbreviated() {
        let cmd = parse_ex_command("tri").unwrap();
        assert!(matches!(cmd, ExCommand::TrimSelections));
    }

    #[test]
    fn parses_align() {
        let cmd = parse_ex_command("align").unwrap();
        assert!(matches!(cmd, ExCommand::AlignSelections));
    }

    #[test]
    fn parses_align_abbreviated() {
        let cmd = parse_ex_command("ali").unwrap();
        assert!(matches!(cmd, ExCommand::AlignSelections));
    }

    #[test]
    fn parses_rotate() {
        let cmd = parse_ex_command("rotate").unwrap();
        assert!(matches!(cmd, ExCommand::RotateContents));
    }

    #[test]
    fn parses_rotate_abbreviated() {
        let cmd = parse_ex_command("rot").unwrap();
        assert!(matches!(cmd, ExCommand::RotateContents));
    }

    #[test]
    fn select_does_not_conflict_with_set() {
        // "se" and "set" should still parse as :set, not :select
        assert!(matches!(
            parse_ex_command("se").unwrap(),
            ExCommand::Set { .. }
        ));
        assert!(matches!(
            parse_ex_command("set").unwrap(),
            ExCommand::Set { .. }
        ));
    }

    #[test]
    fn remove_does_not_conflict_with_read() {
        // :r, :re, :rea, :read should still be :read
        assert!(matches!(
            parse_ex_command("r /tmp/x").unwrap(),
            ExCommand::Read { .. }
        ));
        assert!(matches!(
            parse_ex_command("read /tmp/x").unwrap(),
            ExCommand::Read { .. }
        ));
    }

    #[test]
    fn remove_does_not_conflict_with_registers() {
        assert!(matches!(
            parse_ex_command("reg").unwrap(),
            ExCommand::Registers { .. }
        ));
    }

    #[test]
    fn remove_does_not_conflict_with_retab() {
        assert!(matches!(
            parse_ex_command("ret").unwrap(),
            ExCommand::Retab { .. }
        ));
    }

    #[test]
    fn align_mutates_text() {
        let cmd = ExCommand::AlignSelections;
        assert!(cmd.mutates_text());
    }

    #[test]
    fn rotate_mutates_text() {
        let cmd = ExCommand::RotateContents;
        assert!(cmd.mutates_text());
    }

    #[test]
    fn rotate_dir_mutates_text() {
        let cmd = ExCommand::RotateContentsDir {
            direction: crate::primitives::Direction::Backward,
        };
        assert!(cmd.mutates_text());
    }

    #[test]
    fn select_does_not_mutate_text() {
        let cmd = ExCommand::SelectMatches {
            range: None,
            pattern: "foo".to_owned(),
        };
        assert!(!cmd.mutates_text());
    }

    #[test]
    fn trim_does_not_mutate_text() {
        let cmd = ExCommand::TrimSelections;
        assert!(!cmd.mutates_text());
    }

    #[test]
    fn select_requires_pattern() {
        let result = parse_ex_command("select");
        assert!(result.is_err());
    }

    #[test]
    fn keep_requires_pattern() {
        let result = parse_ex_command("keep");
        assert!(result.is_err());
    }

    // ── :addnext ──────────────────────────────────────────────────

    #[test]
    fn parses_addnext_no_count() {
        let cmd = parse_ex_command("addnext").unwrap();
        assert!(matches!(cmd, ExCommand::AddNext { count: None }));
    }

    #[test]
    fn parses_addnext_with_count() {
        let cmd = parse_ex_command("addnext 3").unwrap();
        assert!(matches!(cmd, ExCommand::AddNext { count: Some(3) }));
    }

    #[test]
    fn parses_addnext_abbreviated() {
        for abbrev in ["addn", "addne", "addnex", "addnext"] {
            let cmd = parse_ex_command(abbrev).unwrap();
            assert!(
                matches!(cmd, ExCommand::AddNext { count: None }),
                "{abbrev:?} should parse as AddNext, got: {cmd:?}"
            );
        }
    }

    // ── :addprev ──────────────────────────────────────────────────

    #[test]
    fn parses_addprev_no_count() {
        let cmd = parse_ex_command("addprev").unwrap();
        assert!(matches!(cmd, ExCommand::AddPrev { count: None }));
    }

    #[test]
    fn parses_addprev_with_count() {
        let cmd = parse_ex_command("addprev 5").unwrap();
        assert!(matches!(cmd, ExCommand::AddPrev { count: Some(5) }));
    }

    #[test]
    fn parses_addprev_abbreviated() {
        for abbrev in ["addp", "addpr", "addpre", "addprev"] {
            let cmd = parse_ex_command(abbrev).unwrap();
            assert!(
                matches!(cmd, ExCommand::AddPrev { count: None }),
                "{abbrev:?} should parse as AddPrev, got: {cmd:?}"
            );
        }
    }

    // ── :skipmatch ────────────────────────────────────────────────

    #[test]
    fn parses_skipmatch() {
        let cmd = parse_ex_command("skipmatch").unwrap();
        assert!(matches!(cmd, ExCommand::SkipMatch));
    }

    #[test]
    fn parses_skipmatch_abbreviated() {
        for abbrev in [
            "skip",
            "skipm",
            "skipma",
            "skipmat",
            "skipmatc",
            "skipmatch",
        ] {
            let cmd = parse_ex_command(abbrev).unwrap();
            assert!(
                matches!(cmd, ExCommand::SkipMatch),
                "{abbrev:?} should parse as SkipMatch, got: {cmd:?}"
            );
        }
    }

    // ── :addcursor ────────────────────────────────────────────────

    #[test]
    fn parses_addcursor_below() {
        let cmd = parse_ex_command("addcursor below").unwrap();
        assert!(matches!(
            cmd,
            ExCommand::AddCursorDir {
                direction: crate::primitives::Direction::Forward,
                count: None,
            }
        ));
    }

    #[test]
    fn parses_addcursor_above() {
        let cmd = parse_ex_command("addcursor above").unwrap();
        assert!(matches!(
            cmd,
            ExCommand::AddCursorDir {
                direction: crate::primitives::Direction::Backward,
                count: None,
            }
        ));
    }

    #[test]
    fn parses_addcursor_above_with_count() {
        let cmd = parse_ex_command("addcursor above 2").unwrap();
        assert!(matches!(
            cmd,
            ExCommand::AddCursorDir {
                direction: crate::primitives::Direction::Backward,
                count: Some(2),
            }
        ));
    }

    #[test]
    fn parses_addcursor_default_below() {
        // No direction arg defaults to below (Forward)
        let cmd = parse_ex_command("addcursor").unwrap();
        assert!(matches!(
            cmd,
            ExCommand::AddCursorDir {
                direction: crate::primitives::Direction::Forward,
                count: None,
            }
        ));
    }

    #[test]
    fn parses_addcursor_abbreviated() {
        for abbrev in [
            "addc",
            "addcu",
            "addcur",
            "addcurs",
            "addcurso",
            "addcursor",
        ] {
            let input = format!("{abbrev} below");
            let cmd = parse_ex_command(&input).unwrap();
            assert!(
                matches!(cmd, ExCommand::AddCursorDir { .. }),
                "{abbrev:?} should parse as AddCursorDir, got: {cmd:?}"
            );
        }
    }

    // ── :selectall ────────────────────────────────────────────────

    #[test]
    fn parses_selectall() {
        let cmd = parse_ex_command("selectall").unwrap();
        assert!(matches!(cmd, ExCommand::SelectAll));
    }

    #[test]
    fn parses_selectall_abbreviated() {
        for abbrev in ["selecta", "selectal", "selectall"] {
            let cmd = parse_ex_command(abbrev).unwrap();
            assert!(
                matches!(cmd, ExCommand::SelectAll),
                "{abbrev:?} should parse as SelectAll, got: {cmd:?}"
            );
        }
    }

    // ── :cursorcollapse ───────────────────────────────────────────

    #[test]
    fn parses_cursorcollapse() {
        let cmd = parse_ex_command("cursorcollapse").unwrap();
        assert!(matches!(cmd, ExCommand::CursorCollapse));
    }

    #[test]
    fn parses_cursorcollapse_abbreviated() {
        for abbrev in ["cursorco", "cursorcol", "cursorcoll", "cursorcolla"] {
            let cmd = parse_ex_command(abbrev).unwrap();
            assert!(
                matches!(cmd, ExCommand::CursorCollapse),
                "{abbrev:?} should parse as CursorCollapse, got: {cmd:?}"
            );
        }
    }

    // ── :cursorremove ─────────────────────────────────────────────

    #[test]
    fn parses_cursorremove() {
        let cmd = parse_ex_command("cursorremove").unwrap();
        assert!(matches!(cmd, ExCommand::CursorRemove));
    }

    #[test]
    fn parses_cursorremove_abbreviated() {
        for abbrev in ["cursorrem", "cursorremo", "cursorremov", "cursorremove"] {
            let cmd = parse_ex_command(abbrev).unwrap();
            assert!(
                matches!(cmd, ExCommand::CursorRemove),
                "{abbrev:?} should parse as CursorRemove, got: {cmd:?}"
            );
        }
    }

    // ── :cursorprimary ────────────────────────────────────────────

    #[test]
    fn parses_cursorprimary_next() {
        let cmd = parse_ex_command("cursorprimary next").unwrap();
        assert!(matches!(
            cmd,
            ExCommand::CursorPrimary {
                direction: crate::primitives::Direction::Forward,
            }
        ));
    }

    #[test]
    fn parses_cursorprimary_prev() {
        let cmd = parse_ex_command("cursorprimary prev").unwrap();
        assert!(matches!(
            cmd,
            ExCommand::CursorPrimary {
                direction: crate::primitives::Direction::Backward,
            }
        ));
    }

    #[test]
    fn parses_cursorprimary_default_next() {
        let cmd = parse_ex_command("cursorprimary").unwrap();
        assert!(matches!(
            cmd,
            ExCommand::CursorPrimary {
                direction: crate::primitives::Direction::Forward,
            }
        ));
    }

    #[test]
    fn parses_cursorprimary_abbreviated() {
        for abbrev in ["cursorp", "cursorpr", "cursorpri", "cursorprim"] {
            let input = format!("{abbrev} next");
            let cmd = parse_ex_command(&input).unwrap();
            assert!(
                matches!(cmd, ExCommand::CursorPrimary { .. }),
                "{abbrev:?} should parse as CursorPrimary, got: {cmd:?}"
            );
        }
    }

    // ── :cursorsplit ──────────────────────────────────────────────

    #[test]
    fn parses_cursorsplit() {
        let cmd = parse_ex_command("cursorsplit").unwrap();
        assert!(matches!(cmd, ExCommand::CursorSplitBlock));
    }

    #[test]
    fn parses_cursorsplit_abbreviated() {
        for abbrev in ["cursorsp", "cursorspl", "cursorsplit"] {
            let cmd = parse_ex_command(abbrev).unwrap();
            assert!(
                matches!(cmd, ExCommand::CursorSplitBlock),
                "{abbrev:?} should parse as CursorSplitBlock, got: {cmd:?}"
            );
        }
    }

    // ── :cursorflip ───────────────────────────────────────────────

    #[test]
    fn parses_cursorflip() {
        let cmd = parse_ex_command("cursorflip").unwrap();
        assert!(matches!(cmd, ExCommand::CursorFlip));
    }

    #[test]
    fn parses_cursorflip_abbreviated() {
        for abbrev in ["cursorfl", "cursorfli", "cursorflip"] {
            let cmd = parse_ex_command(abbrev).unwrap();
            assert!(
                matches!(cmd, ExCommand::CursorFlip),
                "{abbrev:?} should parse as CursorFlip, got: {cmd:?}"
            );
        }
    }

    // ── :cursorforward ────────────────────────────────────────────

    #[test]
    fn parses_cursorforward() {
        let cmd = parse_ex_command("cursorforward").unwrap();
        assert!(matches!(cmd, ExCommand::CursorForward));
    }

    #[test]
    fn parses_cursorforward_abbreviated() {
        for abbrev in ["cursorfo", "cursorfor", "cursorforw"] {
            let cmd = parse_ex_command(abbrev).unwrap();
            assert!(
                matches!(cmd, ExCommand::CursorForward),
                "{abbrev:?} should parse as CursorForward, got: {cmd:?}"
            );
        }
    }

    // ── :cursormerge ──────────────────────────────────────────────

    #[test]
    fn parses_cursormerge() {
        let cmd = parse_ex_command("cursormerge").unwrap();
        assert!(matches!(cmd, ExCommand::CursorMerge));
    }

    #[test]
    fn parses_cursormerge_abbreviated() {
        for abbrev in ["cursorm", "cursormer", "cursormerg", "cursormerge"] {
            let cmd = parse_ex_command(abbrev).unwrap();
            assert!(
                matches!(cmd, ExCommand::CursorMerge),
                "{abbrev:?} should parse as CursorMerge, got: {cmd:?}"
            );
        }
    }

    // ── :cursorfilter ─────────────────────────────────────────────

    #[test]
    fn parses_cursorfilter_as_keep() {
        let cmd = parse_ex_command("cursorfilter /pattern/").unwrap();
        assert!(matches!(
            cmd,
            ExCommand::KeepMatches { pattern, .. } if pattern == "pattern"
        ));
    }

    #[test]
    fn parses_cursorfilter_bang_as_remove() {
        let cmd = parse_ex_command("cursorfilter! /dead/").unwrap();
        assert!(matches!(
            cmd,
            ExCommand::RemoveMatches { pattern, .. } if pattern == "dead"
        ));
    }

    #[test]
    fn parses_cursorfilter_abbreviated() {
        for abbrev in ["cursorf", "cursorfi", "cursorfil", "cursorfilt"] {
            let input = format!("{abbrev} /x/");
            let cmd = parse_ex_command(&input).unwrap();
            assert!(
                matches!(cmd, ExCommand::KeepMatches { .. }),
                "{abbrev:?} should parse as KeepMatches, got: {cmd:?}"
            );
        }
    }

    // ── :cursorselect ─────────────────────────────────────────────

    #[test]
    fn parses_cursorselect() {
        let cmd = parse_ex_command("cursorselect /pat/").unwrap();
        assert!(matches!(
            cmd,
            ExCommand::SelectMatches { pattern, .. } if pattern == "pat"
        ));
    }

    #[test]
    fn parses_cursorselect_abbreviated() {
        for abbrev in ["cursorsel", "cursorsele", "cursorselec", "cursorselect"] {
            let input = format!("{abbrev} /x/");
            let cmd = parse_ex_command(&input).unwrap();
            assert!(
                matches!(cmd, ExCommand::SelectMatches { .. }),
                "{abbrev:?} should parse as SelectMatches, got: {cmd:?}"
            );
        }
    }

    // ── :cursorsplitsel ───────────────────────────────────────────

    #[test]
    fn parses_cursorsplitsel() {
        let cmd = parse_ex_command("cursorsplitsel /pat/").unwrap();
        assert!(matches!(
            cmd,
            ExCommand::SplitMatches { pattern, .. } if pattern == "pat"
        ));
    }

    #[test]
    fn parses_cursorsplitsel_abbreviated() {
        for abbrev in ["cursorsplits", "cursorsplitse", "cursorsplitsel"] {
            let input = format!("{abbrev} /x/");
            let cmd = parse_ex_command(&input).unwrap();
            assert!(
                matches!(cmd, ExCommand::SplitMatches { .. }),
                "{abbrev:?} should parse as SplitMatches, got: {cmd:?}"
            );
        }
    }

    // ── :cursortrim / :cursoralign / :cursorrotate (aliases) ─────

    #[test]
    fn parses_cursortrim() {
        let cmd = parse_ex_command("cursortrim").unwrap();
        assert!(matches!(cmd, ExCommand::TrimSelections));
    }

    #[test]
    fn parses_cursortrim_abbreviated() {
        for abbrev in ["cursort", "cursortr", "cursortri", "cursortrim"] {
            let cmd = parse_ex_command(abbrev).unwrap();
            assert!(
                matches!(cmd, ExCommand::TrimSelections),
                "{abbrev:?} should parse as TrimSelections, got: {cmd:?}"
            );
        }
    }

    #[test]
    fn parses_cursoralign() {
        let cmd = parse_ex_command("cursoralign").unwrap();
        assert!(matches!(cmd, ExCommand::AlignSelections));
    }

    #[test]
    fn parses_cursoralign_abbreviated() {
        for abbrev in [
            "cursora",
            "cursoral",
            "cursorali",
            "cursoralig",
            "cursoralign",
        ] {
            let cmd = parse_ex_command(abbrev).unwrap();
            assert!(
                matches!(cmd, ExCommand::AlignSelections),
                "{abbrev:?} should parse as AlignSelections, got: {cmd:?}"
            );
        }
    }

    #[test]
    fn parses_cursorrotate() {
        let cmd = parse_ex_command("cursorrotate").unwrap();
        assert!(matches!(cmd, ExCommand::RotateContents));
    }

    #[test]
    fn parses_cursorrotate_abbreviated() {
        for abbrev in ["cursorrot", "cursorrota", "cursorrotat", "cursorrotate"] {
            let cmd = parse_ex_command(abbrev).unwrap();
            assert!(
                matches!(cmd, ExCommand::RotateContents),
                "{abbrev:?} should parse as RotateContents, got: {cmd:?}"
            );
        }
    }

    #[test]
    fn parses_cursorrotate_fwd() {
        let cmd = parse_ex_command("cursorrotate fwd").unwrap();
        assert!(
            matches!(
                cmd,
                ExCommand::RotateContentsDir {
                    direction: crate::primitives::Direction::Forward
                }
            ),
            "cursorrotate fwd should parse as RotateContentsDir Forward, got: {cmd:?}"
        );
    }

    #[test]
    fn parses_cursorrotate_bwd() {
        let cmd = parse_ex_command("cursorrotate bwd").unwrap();
        assert!(
            matches!(
                cmd,
                ExCommand::RotateContentsDir {
                    direction: crate::primitives::Direction::Backward
                }
            ),
            "cursorrotate bwd should parse as RotateContentsDir Backward, got: {cmd:?}"
        );
    }

    #[test]
    fn parses_cursorrotate_backward() {
        let cmd = parse_ex_command("cursorrotate backward").unwrap();
        assert!(
            matches!(
                cmd,
                ExCommand::RotateContentsDir {
                    direction: crate::primitives::Direction::Backward
                }
            ),
            "cursorrotate backward should parse as RotateContentsDir Backward, got: {cmd:?}"
        );
    }

    #[test]
    fn parses_cursorrot_bwd() {
        let cmd = parse_ex_command("cursorrot bwd").unwrap();
        assert!(
            matches!(
                cmd,
                ExCommand::RotateContentsDir {
                    direction: crate::primitives::Direction::Backward
                }
            ),
            "cursorrot bwd should parse as RotateContentsDir Backward, got: {cmd:?}"
        );
    }

    // ── No-conflict checks ────────────────────────────────────────

    #[test]
    fn cursorsplit_does_not_conflict_with_cursorsplitsel() {
        // :cursorsplit → CursorSplitBlock, :cursorsplitsel → SplitMatches
        let cmd = parse_ex_command("cursorsplit").unwrap();
        assert!(matches!(cmd, ExCommand::CursorSplitBlock));

        let cmd = parse_ex_command("cursorsplitsel /x/").unwrap();
        assert!(matches!(cmd, ExCommand::SplitMatches { .. }));
    }

    #[test]
    fn cursorfilter_does_not_conflict_with_cursorflip() {
        let cmd = parse_ex_command("cursorfilter /x/").unwrap();
        assert!(matches!(cmd, ExCommand::KeepMatches { .. }));

        let cmd = parse_ex_command("cursorflip").unwrap();
        assert!(matches!(cmd, ExCommand::CursorFlip));
    }

    #[test]
    fn cursorfilter_does_not_conflict_with_cursorforward() {
        let cmd = parse_ex_command("cursorfilter /x/").unwrap();
        assert!(matches!(cmd, ExCommand::KeepMatches { .. }));

        let cmd = parse_ex_command("cursorforward").unwrap();
        assert!(matches!(cmd, ExCommand::CursorForward));
    }

    #[test]
    fn cursorfilter_requires_pattern() {
        let result = parse_ex_command("cursorfilter");
        assert!(result.is_err());
    }

    #[test]
    fn cursorselect_requires_pattern() {
        let result = parse_ex_command("cursorselect");
        assert!(result.is_err());
    }

    #[test]
    fn cursorsplitsel_requires_pattern() {
        let result = parse_ex_command("cursorsplitsel");
        assert!(result.is_err());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Pipeline splitting (split_ex_pipeline)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn pipeline_splits_on_bar() {
    let cmds = split_ex_pipeline("s/a/b/|w|q");
    assert_eq!(cmds, vec!["s/a/b/", "w", "q"]);
}

#[test]
fn pipeline_preserves_regex_bar() {
    // The | inside /a|b/ should NOT split
    let cmds = split_ex_pipeline("s/a|b/c/|w");
    assert_eq!(cmds, vec!["s/a|b/c/", "w"]);
}

#[test]
fn pipeline_single_command() {
    let cmds = split_ex_pipeline("w");
    assert_eq!(cmds, vec!["w"]);
}

#[test]
fn pipeline_empty_input() {
    let cmds = split_ex_pipeline("");
    assert!(cmds.is_empty());
}

#[test]
fn pipeline_whitespace_only() {
    let cmds = split_ex_pipeline("   ");
    assert!(cmds.is_empty());
}

#[test]
fn pipeline_trims_segments() {
    let cmds = split_ex_pipeline("w | q");
    assert_eq!(cmds, vec!["w", "q"]);
}

#[test]
fn pipeline_bang_command_not_split() {
    // :! takes everything -- | is part of the shell command
    let cmds = split_ex_pipeline("!ls | grep foo");
    assert_eq!(cmds, vec!["!ls | grep foo"]);
}

#[test]
fn pipeline_bang_with_range_not_split() {
    // :%! is a filter command -- | is part of the shell command
    let cmds = split_ex_pipeline("%!sort | uniq");
    assert_eq!(cmds, vec!["%!sort | uniq"]);
}

#[test]
fn pipeline_set_and_write() {
    let cmds = split_ex_pipeline("set hlsearch|w");
    assert_eq!(cmds, vec!["set hlsearch", "w"]);
}

#[test]
fn pipeline_multiple_substitutions() {
    let cmds = split_ex_pipeline("s/a/b/g|s/c/d/g");
    assert_eq!(cmds, vec!["s/a/b/g", "s/c/d/g"]);
}

#[test]
fn pipeline_skips_empty_segments() {
    let cmds = split_ex_pipeline("w||q");
    assert_eq!(cmds, vec!["w", "q"]);
}

#[test]
fn pipeline_question_mark_regex_preserved() {
    let cmds = split_ex_pipeline("g?foo|bar?d|w");
    assert_eq!(cmds, vec!["g?foo|bar?d", "w"]);
}

// ── Diagnostic navigation (:cn, :cp, :cl, :cc) ─────────────────────────

#[test]
fn parses_cnext() {
    let cmd = parse_ex_command("cnext").unwrap();
    assert!(matches!(cmd, ExCommand::CNext { count: 1 }));

    let cmd2 = parse_ex_command("cn").unwrap();
    assert!(matches!(cmd2, ExCommand::CNext { count: 1 }));

    let cmd3 = parse_ex_command("cn 3").unwrap();
    assert!(matches!(cmd3, ExCommand::CNext { count: 3 }));
}

#[test]
fn parses_cprev() {
    let cmd = parse_ex_command("cprevious").unwrap();
    assert!(matches!(cmd, ExCommand::CPrev { count: 1 }));

    let cmd2 = parse_ex_command("cp").unwrap();
    assert!(matches!(cmd2, ExCommand::CPrev { count: 1 }));

    let cmd3 = parse_ex_command("cp 3").unwrap();
    assert!(matches!(cmd3, ExCommand::CPrev { count: 3 }));

    let cmd4 = parse_ex_command("cprev").unwrap();
    assert!(matches!(cmd4, ExCommand::CPrev { count: 1 }));
}

#[test]
fn parses_clist() {
    let cmd = parse_ex_command("clist").unwrap();
    assert!(matches!(cmd, ExCommand::CList));

    let cmd2 = parse_ex_command("cl").unwrap();
    assert!(matches!(cmd2, ExCommand::CList));
}

#[test]
fn parses_cc() {
    let cmd = parse_ex_command("cc").unwrap();
    assert!(matches!(cmd, ExCommand::CC { index: None }));

    let cmd2 = parse_ex_command("cc 5").unwrap();
    assert!(matches!(cmd2, ExCommand::CC { index: Some(5) }));
}

#[test]
fn close_still_works_after_diagnostic_commands() {
    // Verify :close/:clo still parses correctly (no prefix collision).
    let cmd = parse_ex_command("close").unwrap();
    assert!(matches!(cmd, ExCommand::Close { force: false }));

    let cmd2 = parse_ex_command("clo").unwrap();
    assert!(matches!(cmd2, ExCommand::Close { force: false }));

    let cmd3 = parse_ex_command("close!").unwrap();
    assert!(matches!(cmd3, ExCommand::Close { force: true }));
}

#[test]
fn cnext_default_count_is_one() {
    // :cnext without explicit count should default to 1
    let cmd = parse_ex_command("cnext").unwrap();
    assert_eq!(cmd, ExCommand::CNext { count: 1 });
}

#[test]
fn cn_with_count_5() {
    let cmd = parse_ex_command("cn 5").unwrap();
    assert_eq!(cmd, ExCommand::CNext { count: 5 });
}

#[test]
fn cc_without_index_defaults_to_none() {
    let cmd = parse_ex_command("cc").unwrap();
    assert_eq!(cmd, ExCommand::CC { index: None });
}

#[test]
fn cc_with_index_3() {
    let cmd = parse_ex_command("cc 3").unwrap();
    assert_eq!(cmd, ExCommand::CC { index: Some(3) });
}

#[test]
fn cnext_invalid_count_defaults_to_one() {
    // Non-numeric count argument falls back to 1 via parse_count_or_default
    let cmd = parse_ex_command("cn abc").unwrap();
    assert_eq!(cmd, ExCommand::CNext { count: 1 });
}

#[test]
fn cprev_invalid_count_defaults_to_one() {
    let cmd = parse_ex_command("cp xyz").unwrap();
    assert_eq!(cmd, ExCommand::CPrev { count: 1 });
}

#[test]
fn cc_invalid_index_defaults_to_one() {
    // :cc with non-numeric argument falls back to Some(1)
    let cmd = parse_ex_command("cc abc").unwrap();
    assert_eq!(cmd, ExCommand::CC { index: Some(1) });
}

// ═══════════════════════════════════════════════════════════════════════════
// :delmarks
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn delmarks_parses_marks_string() {
    let cmd = parse_ex_command("delmarks abc").unwrap();
    assert_eq!(
        cmd,
        ExCommand::DelMarks {
            marks: CompactString::from("abc"),
            clear_all: false,
        }
    );
}

#[test]
fn delm_abbreviation_works() {
    let cmd = parse_ex_command("delm xy").unwrap();
    assert_eq!(
        cmd,
        ExCommand::DelMarks {
            marks: CompactString::from("xy"),
            clear_all: false,
        }
    );
}

#[test]
fn delmarks_bang_clears_all() {
    let cmd = parse_ex_command("delmarks!").unwrap();
    assert_eq!(
        cmd,
        ExCommand::DelMarks {
            marks: CompactString::from(""),
            clear_all: true,
        }
    );
}

#[test]
fn delmarks_bang_with_space_clears_all() {
    let cmd = parse_ex_command("delmarks! ignored").unwrap();
    assert_eq!(
        cmd,
        ExCommand::DelMarks {
            marks: CompactString::from("ignored"),
            clear_all: true,
        }
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// :g! (global inversion alias)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn g_bang_parses_as_inverted_global() {
    let cmd = parse_ex_command("g!/pattern/d").unwrap();
    match cmd {
        ExCommand::Global {
            invert, pattern, ..
        } => {
            assert!(invert);
            assert_eq!(pattern.as_str(), "pattern");
        }
        other => panic!("expected Global, got {other:?}"),
    }
}

#[test]
fn g_bang_with_different_delimiter() {
    let cmd = parse_ex_command("g!|foo|d").unwrap();
    match cmd {
        ExCommand::Global {
            invert, pattern, ..
        } => {
            assert!(invert);
            assert_eq!(pattern.as_str(), "foo");
        }
        other => panic!("expected Global, got {other:?}"),
    }
}

#[test]
fn g_bang_with_range() {
    let cmd = parse_ex_command("1,5g!/test/d").unwrap();
    match cmd {
        ExCommand::Global {
            invert,
            pattern,
            range,
            ..
        } => {
            assert!(invert);
            assert_eq!(pattern.as_str(), "test");
            assert!(range.end.is_some());
        }
        other => panic!("expected Global, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// :cquit
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn cquit_default_exit_code() {
    let cmd = parse_ex_command("cquit").unwrap();
    assert_eq!(cmd, ExCommand::CQuit { exit_code: 1 });
}

#[test]
fn cq_abbreviation() {
    let cmd = parse_ex_command("cq").unwrap();
    assert_eq!(cmd, ExCommand::CQuit { exit_code: 1 });
}

#[test]
fn cquit_with_exit_code() {
    let cmd = parse_ex_command("cquit 2").unwrap();
    assert_eq!(cmd, ExCommand::CQuit { exit_code: 2 });
}

#[test]
fn cquit_with_zero_exit_code() {
    let cmd = parse_ex_command("cq 0").unwrap();
    assert_eq!(cmd, ExCommand::CQuit { exit_code: 0 });
}

// ═══════════════════════════════════════════════════════════════════════════
// :update
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn update_no_args() {
    let cmd = parse_ex_command("update").unwrap();
    assert_eq!(
        cmd,
        ExCommand::Update {
            path: None,
            force: false,
        }
    );
}

#[test]
fn up_abbreviation() {
    let cmd = parse_ex_command("up").unwrap();
    assert_eq!(
        cmd,
        ExCommand::Update {
            path: None,
            force: false,
        }
    );
}

#[test]
fn update_with_path() {
    let cmd = parse_ex_command("update /tmp/file.txt").unwrap();
    assert_eq!(
        cmd,
        ExCommand::Update {
            path: Some(CompactString::from("/tmp/file.txt")),
            force: false,
        }
    );
}

#[test]
fn update_bang_force() {
    let cmd = parse_ex_command("update!").unwrap();
    assert_eq!(
        cmd,
        ExCommand::Update {
            path: None,
            force: true,
        }
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// :fold, :foldopen, :foldclose
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fold_parses() {
    let cmd = parse_ex_command("fold").unwrap();
    assert!(matches!(cmd, ExCommand::Fold { .. }));
}

#[test]
fn fo_abbreviation() {
    let cmd = parse_ex_command("fo").unwrap();
    assert!(matches!(cmd, ExCommand::Fold { .. }));
}

#[test]
fn foldopen_parses() {
    let cmd = parse_ex_command("foldopen").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::FoldOpen {
            recursive: false,
            ..
        }
    ));
}

#[test]
fn foldo_abbreviation() {
    let cmd = parse_ex_command("foldo").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::FoldOpen {
            recursive: false,
            ..
        }
    ));
}

#[test]
fn foldopen_bang_recursive() {
    let cmd = parse_ex_command("foldopen!").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::FoldOpen {
            recursive: true,
            ..
        }
    ));
}

#[test]
fn foldclose_parses() {
    let cmd = parse_ex_command("foldclose").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::FoldClose {
            recursive: false,
            ..
        }
    ));
}

#[test]
fn foldc_abbreviation() {
    let cmd = parse_ex_command("foldc").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::FoldClose {
            recursive: false,
            ..
        }
    ));
}

#[test]
fn foldclose_bang_recursive() {
    let cmd = parse_ex_command("foldclose!").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::FoldClose {
            recursive: true,
            ..
        }
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional :g! tests — different delimiters and nested commands
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn g_bang_with_hash_delimiter() {
    let cmd = parse_ex_command("g!#TODO#d").unwrap();
    match cmd {
        ExCommand::Global {
            invert, pattern, ..
        } => {
            assert!(invert);
            assert_eq!(pattern.as_str(), "TODO");
        }
        other => panic!("expected Global, got {other:?}"),
    }
}

#[test]
fn g_bang_with_at_delimiter() {
    let cmd = parse_ex_command("g!@error@d").unwrap();
    match cmd {
        ExCommand::Global {
            invert, pattern, ..
        } => {
            assert!(invert);
            assert_eq!(pattern.as_str(), "error");
        }
        other => panic!("expected Global, got {other:?}"),
    }
}

#[test]
fn g_bang_nested_substitute() {
    let cmd = parse_ex_command("g!/^#/s/old/new/g").unwrap();
    match cmd {
        ExCommand::Global {
            invert,
            pattern,
            command,
            ..
        } => {
            assert!(invert);
            assert_eq!(pattern.as_str(), "^#");
            assert!(matches!(*command, ExCommand::Substitute { .. }));
        }
        other => panic!("expected Global, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional :delmarks tests — range-like marks string (a-z), digits, specials
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn delmarks_single_mark() {
    let cmd = parse_ex_command("delmarks a").unwrap();
    assert_eq!(
        cmd,
        ExCommand::DelMarks {
            marks: CompactString::from("a"),
            clear_all: false,
        }
    );
}

#[test]
fn delmarks_mixed_marks() {
    // Vim allows any characters in the marks string; the executor filters valid ones.
    let cmd = parse_ex_command("delmarks aB1").unwrap();
    assert_eq!(
        cmd,
        ExCommand::DelMarks {
            marks: CompactString::from("aB1"),
            clear_all: false,
        }
    );
}

#[test]
fn delmarks_range_notation_passes_raw() {
    // Vim's :delmarks a-z passes "a-z" as the marks string; the executor
    // expands the range. Parser should pass through as-is.
    let cmd = parse_ex_command("delmarks a-z").unwrap();
    assert_eq!(
        cmd,
        ExCommand::DelMarks {
            marks: CompactString::from("a-z"),
            clear_all: false,
        }
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional :cquit tests — edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn cquit_invalid_code_defaults_to_1() {
    // Non-numeric argument should default to 1 (unwrap_or(1) in parser)
    let cmd = parse_ex_command("cquit abc").unwrap();
    assert_eq!(cmd, ExCommand::CQuit { exit_code: 1 });
}

#[test]
fn cq_with_large_exit_code() {
    let cmd = parse_ex_command("cq 127").unwrap();
    assert_eq!(cmd, ExCommand::CQuit { exit_code: 127 });
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional :update tests — difference from :write semantics
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn update_differs_from_write_variant() {
    // :update produces Update variant (host checks modified flag)
    // :write produces Write variant (always writes)
    let update = parse_ex_command("update").unwrap();
    let write = parse_ex_command("write").unwrap();
    assert!(matches!(update, ExCommand::Update { .. }));
    assert!(matches!(write, ExCommand::Write { .. }));
    // They are distinct enum variants
    assert_ne!(
        std::mem::discriminant(&update),
        std::mem::discriminant(&write)
    );
}

#[test]
fn update_up_abbreviation_with_path() {
    let cmd = parse_ex_command("up /tmp/out.txt").unwrap();
    assert_eq!(
        cmd,
        ExCommand::Update {
            path: Some(CompactString::from("/tmp/out.txt")),
            force: false,
        }
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional fold tests — with explicit ranges
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fold_with_range() {
    let cmd = parse_ex_command("1,10fold").unwrap();
    match cmd {
        ExCommand::Fold { range } => {
            assert_eq!(range.start, LineSpec::Absolute(1));
            assert_eq!(range.end, Some(LineSpec::Absolute(10)));
        }
        other => panic!("expected Fold, got {other:?}"),
    }
}

#[test]
fn foldopen_with_range() {
    let cmd = parse_ex_command("5,20foldopen").unwrap();
    match cmd {
        ExCommand::FoldOpen { range, recursive } => {
            assert_eq!(range.start, LineSpec::Absolute(5));
            assert_eq!(range.end, Some(LineSpec::Absolute(20)));
            assert!(!recursive);
        }
        other => panic!("expected FoldOpen, got {other:?}"),
    }
}

#[test]
fn foldclose_with_range_and_bang() {
    let cmd = parse_ex_command("3,8foldclose!").unwrap();
    match cmd {
        ExCommand::FoldClose { range, recursive } => {
            assert_eq!(range.start, LineSpec::Absolute(3));
            assert_eq!(range.end, Some(LineSpec::Absolute(8)));
            assert!(recursive);
        }
        other => panic!("expected FoldClose, got {other:?}"),
    }
}

// --- Star (*) range shorthand ---

#[test]
fn star_range_shorthand_delete() {
    // :*d should expand to :'<,'>d
    let cmd = parse_ex_command("*d").unwrap();
    match cmd {
        ExCommand::Delete { range, register } => {
            assert_eq!(range.start, LineSpec::Mark(MarkName::VISUAL_START));
            assert_eq!(range.end, Some(LineSpec::Mark(MarkName::VISUAL_END)));
            assert_eq!(range.separator, RangeSeparator::Comma);
            assert_eq!(register, None);
        }
        other => panic!("expected Delete, got {other:?}"),
    }
}

#[test]
fn star_range_shorthand_yank() {
    let cmd = parse_ex_command("*y").unwrap();
    match cmd {
        ExCommand::Yank { range, register } => {
            assert_eq!(range.start, LineSpec::Mark(MarkName::VISUAL_START));
            assert_eq!(range.end, Some(LineSpec::Mark(MarkName::VISUAL_END)));
            assert_eq!(register, None);
        }
        other => panic!("expected Yank, got {other:?}"),
    }
}

#[test]
fn star_range_shorthand_substitute() {
    let cmd = parse_ex_command("*s/foo/bar/").unwrap();
    match cmd {
        ExCommand::Substitute { range, .. } => {
            assert_eq!(range.start, LineSpec::Mark(MarkName::VISUAL_START));
            assert_eq!(range.end, Some(LineSpec::Mark(MarkName::VISUAL_END)));
        }
        other => panic!("expected Substitute, got {other:?}"),
    }
}

#[test]
fn star_range_shorthand_bare() {
    // Bare :* should go to the visual end line (like :'<,'>)
    let cmd = parse_ex_command("*").unwrap();
    match cmd {
        ExCommand::GotoLine { range } => {
            assert_eq!(range.start, LineSpec::Mark(MarkName::VISUAL_START));
            assert_eq!(range.end, Some(LineSpec::Mark(MarkName::VISUAL_END)));
        }
        other => panic!("expected GotoLine, got {other:?}"),
    }
}

#[test]
fn star_range_shorthand_with_whitespace_before_command() {
    // :* d (space between * and command) should still parse as visual-range delete
    let cmd = parse_ex_command("* d").unwrap();
    match cmd {
        ExCommand::Delete { range, register } => {
            assert_eq!(range.start, LineSpec::Mark(MarkName::VISUAL_START));
            assert_eq!(range.end, Some(LineSpec::Mark(MarkName::VISUAL_END)));
            assert_eq!(register, None);
        }
        other => panic!("expected Delete, got {other:?}"),
    }
}

#[test]
fn star_range_shorthand_global() {
    // :*g/pattern/d should parse as global with visual selection range
    let cmd = parse_ex_command("*g/foo/d").unwrap();
    match cmd {
        ExCommand::Global { range, invert, .. } => {
            assert_eq!(range.start, LineSpec::Mark(MarkName::VISUAL_START));
            assert_eq!(range.end, Some(LineSpec::Mark(MarkName::VISUAL_END)));
            assert!(!invert);
        }
        other => panic!("expected Global, got {other:?}"),
    }
}

// ─── :redo ─────────────────────────────────────────────────────────

#[test]
fn parses_redo_full() {
    let cmd = parse_ex_command("redo").unwrap();
    assert!(matches!(cmd, ExCommand::Redo));
}

#[test]
fn parses_redo_abbreviation() {
    let cmd = parse_ex_command("red").unwrap();
    assert!(matches!(cmd, ExCommand::Redo));
}

#[test]
fn redo_does_not_mutate_text() {
    let cmd = ExCommand::Redo;
    assert!(!cmd.mutates_text());
}

// ─── :clearjumps ───────────────────────────────────────────────────

#[test]
fn parses_clearjumps_full() {
    let cmd = parse_ex_command("clearjumps").unwrap();
    assert!(matches!(cmd, ExCommand::ClearJumps));
}

#[test]
fn parses_clearjumps_abbreviation() {
    let cmd = parse_ex_command("clearj").unwrap();
    assert!(matches!(cmd, ExCommand::ClearJumps));
}

#[test]
fn parses_clearjumps_intermediate_abbreviation() {
    // All intermediate abbreviations should work
    for abbrev in ["clearj", "clearju", "clearjum", "clearjump", "clearjumps"] {
        let cmd = parse_ex_command(abbrev).unwrap();
        assert!(
            matches!(cmd, ExCommand::ClearJumps),
            "expected ClearJumps for :{abbrev}"
        );
    }
}

#[test]
fn clearjumps_does_not_mutate_text() {
    let cmd = ExCommand::ClearJumps;
    assert!(!cmd.mutates_text());
}

// ── Command modifier parsing ─────────────────────────────────────────────

#[test]
fn modifier_silent_echo() {
    let (mods, cmd) = parse_ex_command_with_modifiers("silent echo hi").unwrap();
    assert_eq!(mods, ModifierFlags::SILENT);
    assert!(matches!(cmd, ExCommand::Echo { message } if message.as_str() == "hi"));
}

#[test]
fn modifier_silent_bang_echo() {
    let (mods, cmd) = parse_ex_command_with_modifiers("silent! echo hi").unwrap();
    assert_eq!(mods, ModifierFlags::SILENT | ModifierFlags::SILENT_BANG);
    assert!(matches!(cmd, ExCommand::Echo { message } if message.as_str() == "hi"));
}

#[test]
fn modifier_keepjumps_delete() {
    let (mods, cmd) = parse_ex_command_with_modifiers("keepjumps d3j").unwrap();
    assert_eq!(mods, ModifierFlags::KEEPJUMPS);
    assert!(matches!(cmd, ExCommand::Delete { .. }));
}

#[test]
fn modifier_silent_keepjumps_normal() {
    let (mods, cmd) = parse_ex_command_with_modifiers("silent keepjumps normal dd").unwrap();
    assert_eq!(mods, ModifierFlags::SILENT | ModifierFlags::KEEPJUMPS);
    assert!(matches!(cmd, ExCommand::Norm { keys, .. } if keys.as_str() == "dd"));
}

#[test]
fn modifier_keeppatterns_substitute() {
    let (mods, cmd) = parse_ex_command_with_modifiers("keeppatterns s/foo/bar/").unwrap();
    assert_eq!(mods, ModifierFlags::KEEPPATTERNS);
    assert!(matches!(cmd, ExCommand::Substitute { .. }));
}

#[test]
fn modifier_lockmarks_delete() {
    let (mods, cmd) = parse_ex_command_with_modifiers("lockmarks d").unwrap();
    assert_eq!(mods, ModifierFlags::LOCKMARKS);
    assert!(matches!(cmd, ExCommand::Delete { .. }));
}

#[test]
fn modifier_keepalt_edit() {
    let (mods, cmd) = parse_ex_command_with_modifiers("keepalt edit /tmp/x").unwrap();
    assert_eq!(mods, ModifierFlags::KEEPALT);
    assert!(matches!(cmd, ExCommand::Edit { .. }));
}

#[test]
fn modifier_noautocmd_not_parsed() {
    // :noautocmd is intentionally NOT parsed — it would be a silent no-op.
    // It should be treated as an unknown command, not a modifier.
    let (mods, _) = parse_ex_command_with_modifiers("noautocmd w").unwrap();
    assert_eq!(mods, ModifierFlags::empty());
}

#[test]
fn modifier_triple_composition() {
    let (mods, cmd) =
        parse_ex_command_with_modifiers("silent keepjumps keeppatterns s/a/b/").unwrap();
    assert_eq!(
        mods,
        ModifierFlags::SILENT | ModifierFlags::KEEPJUMPS | ModifierFlags::KEEPPATTERNS
    );
    assert!(matches!(cmd, ExCommand::Substitute { .. }));
}

#[test]
fn no_modifier_plain_command() {
    let (mods, cmd) = parse_ex_command_with_modifiers("echo hello").unwrap();
    assert!(mods.is_empty());
    assert!(matches!(cmd, ExCommand::Echo { .. }));
}

#[test]
fn modifier_does_not_eat_similar_command_names() {
    // "silently" should NOT be parsed as "silent" + "ly"
    // It should fall through to Custom or error
    let (mods, _) = parse_ex_command_with_modifiers("silently").unwrap();
    assert!(mods.is_empty());
}

#[test]
fn modifier_with_range() {
    let (mods, cmd) = parse_ex_command_with_modifiers("silent 5,10d").unwrap();
    assert_eq!(mods, ModifierFlags::SILENT);
    assert!(matches!(cmd, ExCommand::Delete { .. }));
}

#[test]
fn parse_ex_command_strips_modifiers_transparently() {
    // The old API should still work, just discarding modifiers.
    let cmd = parse_ex_command("silent echo hi").unwrap();
    assert!(matches!(cmd, ExCommand::Echo { message } if message.as_str() == "hi"));
}

#[test]
fn modifier_bare_silent_no_command() {
    // `:silent` alone (no command) should parse as a bare modifier with
    // the default no-op (GotoLine current line).
    let (mods, cmd) = parse_ex_command_with_modifiers("silent").unwrap();
    assert_eq!(mods, ModifierFlags::SILENT);
    assert!(matches!(cmd, ExCommand::GotoLine { .. }));
}

// ═══════════════════════════════════════════════════════════════════
// Ex command enhancements
// ═══════════════════════════════════════════════════════════════════

// ── :windo / :bufdo / :tabdo ────────────────────────────────────

#[test]
fn task_8_1_parse_windo() {
    let cmd = parse_ex_command("windo set nohlsearch").unwrap();
    assert!(matches!(cmd, ExCommand::WinDo { .. }));
    if let ExCommand::WinDo { command, .. } = cmd {
        assert!(matches!(*command, ExCommand::Set { .. }));
    }
}

#[test]
fn task_8_1_parse_bufdo() {
    let cmd = parse_ex_command("bufdo echo hello").unwrap();
    assert!(matches!(cmd, ExCommand::BufDo { .. }));
    if let ExCommand::BufDo { command, .. } = cmd {
        assert!(matches!(*command, ExCommand::Echo { .. }));
    }
}

#[test]
fn task_8_1_parse_tabdo() {
    let cmd = parse_ex_command("tabdo nohlsearch").unwrap();
    assert!(matches!(cmd, ExCommand::TabDo { .. }));
    if let ExCommand::TabDo { command, .. } = cmd {
        assert!(matches!(*command, ExCommand::NoHighlight));
    }
}

#[test]
fn task_8_1_windo_requires_command() {
    assert!(parse_ex_command("windo").is_err());
    assert!(parse_ex_command("bufdo").is_err());
    assert!(parse_ex_command("tabdo").is_err());
}

#[test]
fn task_8_1_windo_with_substitute() {
    let cmd = parse_ex_command("windo %s/old/new/g").unwrap();
    assert!(matches!(cmd, ExCommand::WinDo { .. }));
    if let ExCommand::WinDo { command, .. } = cmd {
        assert!(matches!(*command, ExCommand::Substitute { .. }));
    }
}

// ── :@{register} ───────────────────────────────────────────────

#[test]
fn task_8_2_parse_execute_register_a() {
    let cmd = parse_ex_command("@a").unwrap();
    assert!(matches!(cmd, ExCommand::ExecuteRegister { register }
        if register.char() == 'a'));
}

#[test]
fn task_8_2_parse_execute_register_zero() {
    let cmd = parse_ex_command("@0").unwrap();
    assert!(matches!(cmd, ExCommand::ExecuteRegister { register }
        if register.char() == '0'));
}

#[test]
fn task_8_2_parse_execute_register_unnamed() {
    let cmd = parse_ex_command("@\"").unwrap();
    assert!(matches!(cmd, ExCommand::ExecuteRegister { register }
        if register.char() == '"'));
}

// ── :undojoin ───────────────────────────────────────────────────

#[test]
fn task_8_3_parse_undojoin() {
    let cmd = parse_ex_command("undojoin").unwrap();
    assert!(matches!(cmd, ExCommand::UndoJoin));
}

#[test]
fn task_8_3_parse_undojoin_abbreviated() {
    let cmd = parse_ex_command("undoj").unwrap();
    assert!(matches!(cmd, ExCommand::UndoJoin));
}

// ── :silent behavior ────────────────────────────────────────────

#[test]
fn task_8_7_silent_strips_show_info() {
    let (mods, cmd) = parse_ex_command_with_modifiers("silent echo hello").unwrap();
    assert!(mods.contains(ModifierFlags::SILENT));
    assert!(matches!(cmd, ExCommand::Echo { .. }));
}

#[test]
fn task_8_7_silent_bang_strips_errors_too() {
    let (mods, cmd) = parse_ex_command_with_modifiers("silent! echo hello").unwrap();
    assert!(mods.contains(ModifierFlags::SILENT));
    assert!(mods.contains(ModifierFlags::SILENT_BANG));
    assert!(matches!(cmd, ExCommand::Echo { .. }));
}

#[test]
fn task_8_7_silent_chains_with_other_modifiers() {
    let (mods, cmd) = parse_ex_command_with_modifiers("silent keepjumps echo hi").unwrap();
    assert!(mods.contains(ModifierFlags::SILENT));
    assert!(mods.contains(ModifierFlags::KEEPJUMPS));
    assert!(matches!(cmd, ExCommand::Echo { .. }));
}

// ── Trailing comment stripping integration ─────────────────────

#[test]
fn task_9_set_strips_trailing_comment() {
    // `:set number " enable line numbers` — the `" enable...` is a comment
    // because :set has TRLBAR without NOTRLCOM.
    let cmd = parse_ex_command("set number \" enable line numbers").unwrap();
    match cmd {
        ExCommand::Set { assignments } => {
            assert_eq!(
                assignments.len(),
                1,
                "expected 1 assignment, got: {assignments:?}"
            );
            assert_eq!(
                assignments[0],
                SetAssignment::SetBool(compact_str::CompactString::from("number"))
            );
        }
        other => panic!("expected Set, got {other:?}"),
    }
}

#[test]
fn task_9_echo_preserves_quote_in_argument() {
    // `:echo "hello"` — :echo has NOTRLCOM, so `"` is part of the argument.
    let cmd = parse_ex_command("echo \"hello\"").unwrap();
    match cmd {
        ExCommand::Echo { message } => {
            assert_eq!(message.as_str(), "\"hello\"");
        }
        other => panic!("expected Echo, got {other:?}"),
    }
}

#[test]
fn task_9_delete_strips_trailing_comment() {
    // `:delete " remove this line` — :delete has TRLBAR without NOTRLCOM.
    let cmd = parse_ex_command("delete \" remove this line").unwrap();
    match cmd {
        ExCommand::Delete { register, .. } => {
            // The `"` followed by space+text is a comment, not a register arg.
            // After stripping, tail is empty → no register.
            assert!(
                register.is_none(),
                "register should be None after comment stripping, got: {register:?}"
            );
        }
        other => panic!("expected Delete, got {other:?}"),
    }
}

#[test]
fn task_9_normal_preserves_quote_in_keys() {
    // `:normal i"hello"` — :normal has NOTRLCOM, quotes are literal keys.
    let cmd = parse_ex_command("normal i\"hello\"").unwrap();
    match cmd {
        ExCommand::Norm { keys, .. } => {
            assert_eq!(keys.as_str(), "i\"hello\"");
        }
        other => panic!("expected Norm, got {other:?}"),
    }
}

#[test]
fn task_9_set_no_comment_no_change() {
    // `:set tabstop=4` — no trailing comment, should parse normally.
    let cmd = parse_ex_command("set tabstop=4").unwrap();
    match cmd {
        ExCommand::Set { assignments } => {
            assert_eq!(assignments.len(), 1);
            assert_eq!(
                assignments[0],
                SetAssignment::Assign(
                    compact_str::CompactString::from("tabstop"),
                    compact_str::CompactString::from("4"),
                )
            );
        }
        other => panic!("expected Set, got {other:?}"),
    }
}

#[test]
fn task_9_map_preserves_quote_notrlcom() {
    // `:nmap <leader>q :echo "hi"<CR>` — :nmap has NOTRLCOM, `"` is literal.
    let cmd = parse_ex_command("nmap <leader>q :echo \"hi\"<CR>").unwrap();
    match cmd {
        ExCommand::Map { rhs, .. } => {
            let rhs = rhs.expect("rhs should be present");
            assert!(
                rhs.contains('"'),
                "quote should be preserved in mapping rhs: {rhs}"
            );
        }
        other => panic!("expected Map, got {other:?}"),
    }
}

#[test]
fn task_9_unknown_command_no_stripping() {
    // Unknown commands have no metadata → no comment stripping.
    let cmd = parse_ex_command("nonexistent arg \" comment").unwrap();
    match cmd {
        ExCommand::Custom { command } => {
            assert!(
                command.contains('"'),
                "unknown commands should not strip comments: {command}"
            );
        }
        other => panic!("expected Custom, got {other:?}"),
    }
}

// ── xmap/smap (visual-only/select-only mappings) ─────────────────────────

#[test]
fn parses_xmap_recursive() {
    let cmd = parse_ex_command("xmap j gj").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Map {
            mode_prefix: MapModePrefix::VisualOnly,
            kind: MappingKind::Recursive,
            ..
        }
    ));
}

#[test]
fn parses_xnoremap_non_recursive() {
    let cmd = parse_ex_command("xnoremap j gj").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Map {
            mode_prefix: MapModePrefix::VisualOnly,
            kind: MappingKind::NonRecursive,
            ..
        }
    ));
}

#[test]
fn parses_xunmap() {
    let cmd = parse_ex_command("xunmap j").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Unmap {
            mode_prefix: MapModePrefix::VisualOnly,
            ..
        }
    ));
}

#[test]
fn parses_smap_recursive() {
    let cmd = parse_ex_command("smap j gj").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Map {
            mode_prefix: MapModePrefix::SelectOnly,
            kind: MappingKind::Recursive,
            ..
        }
    ));
}

#[test]
fn parses_snoremap_non_recursive() {
    let cmd = parse_ex_command("snoremap j gj").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Map {
            mode_prefix: MapModePrefix::SelectOnly,
            kind: MappingKind::NonRecursive,
            ..
        }
    ));
}

#[test]
fn parses_sunmap() {
    let cmd = parse_ex_command("sunmap j").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Unmap {
            mode_prefix: MapModePrefix::SelectOnly,
            ..
        }
    ));
}

#[test]
fn parses_xmap_abbreviation() {
    // Minimum abbreviation "xm" should work
    let cmd = parse_ex_command("xm j gj").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Map {
            mode_prefix: MapModePrefix::VisualOnly,
            kind: MappingKind::Recursive,
            ..
        }
    ));
}

#[test]
fn parses_xnoremap_abbreviation() {
    // Minimum abbreviation "xn" should work
    let cmd = parse_ex_command("xn j gj").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Map {
            mode_prefix: MapModePrefix::VisualOnly,
            kind: MappingKind::NonRecursive,
            ..
        }
    ));
}

#[test]
fn parses_snoremap_abbreviation() {
    // Minimum abbreviation "sn" should work
    let cmd = parse_ex_command("sn j gj").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Map {
            mode_prefix: MapModePrefix::SelectOnly,
            kind: MappingKind::NonRecursive,
            ..
        }
    ));
}

#[test]
fn parses_smap_abbreviation() {
    // Minimum abbreviation "sm" should work
    let cmd = parse_ex_command("sm j gj").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Map {
            mode_prefix: MapModePrefix::SelectOnly,
            kind: MappingKind::Recursive,
            ..
        }
    ));
}

#[test]
fn parses_xmap_with_flags() {
    let cmd = parse_ex_command("xnoremap <silent> <expr> j gj").unwrap();
    match cmd {
        ExCommand::Map {
            mode_prefix,
            kind,
            flags,
            ..
        } => {
            assert_eq!(mode_prefix, MapModePrefix::VisualOnly);
            assert_eq!(kind, MappingKind::NonRecursive);
            assert!(flags.silent);
            assert!(flags.expr);
        }
        other => panic!("expected Map, got {other:?}"),
    }
}

#[test]
fn parses_smap_list_no_args() {
    let cmd = parse_ex_command("smap").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Map {
            mode_prefix: MapModePrefix::SelectOnly,
            rhs: None,
            ..
        }
    ));
}

// ── mapclear commands ────────────────────────────────────────────────────

#[test]
fn parses_mapclear() {
    let cmd = parse_ex_command("mapclear").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::MapClear {
            mode: MapModePrefix::All,
            force: false,
        }
    ));
}

#[test]
fn parses_mapclear_bang() {
    let cmd = parse_ex_command("mapclear!").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::MapClear {
            mode: MapModePrefix::All,
            force: true,
        }
    ));
}

#[test]
fn parses_nmapclear() {
    let cmd = parse_ex_command("nmapclear").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::MapClear {
            mode: MapModePrefix::Normal,
            force: false,
        }
    ));
}

#[test]
fn parses_nmapclear_abbreviation() {
    let cmd = parse_ex_command("nmapc").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::MapClear {
            mode: MapModePrefix::Normal,
            force: false,
        }
    ));
}

#[test]
fn parses_vmapclear() {
    let cmd = parse_ex_command("vmapclear").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::MapClear {
            mode: MapModePrefix::Visual,
            force: false,
        }
    ));
}

#[test]
fn parses_imapclear() {
    let cmd = parse_ex_command("imapclear").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::MapClear {
            mode: MapModePrefix::Insert,
            force: false,
        }
    ));
}

#[test]
fn parses_omapclear() {
    let cmd = parse_ex_command("omapclear").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::MapClear {
            mode: MapModePrefix::Operator,
            force: false,
        }
    ));
}

#[test]
fn parses_cmapclear() {
    let cmd = parse_ex_command("cmapclear").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::MapClear {
            mode: MapModePrefix::Command,
            force: false,
        }
    ));
}

#[test]
fn parses_xmapclear() {
    let cmd = parse_ex_command("xmapclear").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::MapClear {
            mode: MapModePrefix::VisualOnly,
            force: false,
        }
    ));
}

#[test]
fn parses_smapclear() {
    let cmd = parse_ex_command("smapclear").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::MapClear {
            mode: MapModePrefix::SelectOnly,
            force: false,
        }
    ));
}

#[test]
fn parses_xmapclear_abbreviation() {
    let cmd = parse_ex_command("xmapc").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::MapClear {
            mode: MapModePrefix::VisualOnly,
            force: false,
        }
    ));
}

#[test]
fn parses_smapclear_abbreviation() {
    let cmd = parse_ex_command("smapc").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::MapClear {
            mode: MapModePrefix::SelectOnly,
            force: false,
        }
    ));
}

#[test]
fn mapclear_does_not_mutate_text() {
    let cmd = parse_ex_command("mapclear").unwrap();
    assert!(!cmd.mutates_text());
}

#[test]
fn xmap_no_prefix_conflict_with_xit() {
    // :xm should parse as xmap, not xit
    let cmd = parse_ex_command("xm j gj").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Map {
            mode_prefix: MapModePrefix::VisualOnly,
            ..
        }
    ));

    // :xit should still work
    let cmd = parse_ex_command("xit").unwrap();
    assert!(matches!(cmd, ExCommand::WriteQuit { .. }));
}

#[test]
fn smap_no_prefix_conflict_with_sort() {
    // :sm should parse as smap, not sort
    let cmd = parse_ex_command("sm j gj").unwrap();
    assert!(matches!(
        cmd,
        ExCommand::Map {
            mode_prefix: MapModePrefix::SelectOnly,
            ..
        }
    ));

    // :sort should still work
    let cmd = parse_ex_command("sort").unwrap();
    assert!(matches!(cmd, ExCommand::Sort { .. }));
}

// ─── :set operators and escapes ─────────────────────────────────

fn set_args(line: &str) -> Vec<SetAssignment> {
    match parse_ex_command(line).unwrap() {
        ExCommand::Set { assignments }
        | ExCommand::SetLocal { assignments }
        | ExCommand::SetGlobal { assignments } => assignments.into_vec(),
        other => panic!("expected a :set command, got {other:?}"),
    }
}

fn cs(s: &str) -> compact_str::CompactString {
    compact_str::CompactString::from(s)
}

#[test]
fn set_operators_parse_before_plain_assign() {
    assert_eq!(
        set_args("set fo-=t fo+=c com^=b:## tw+=4"),
        vec![
            SetAssignment::Remove(cs("fo"), cs("t")),
            SetAssignment::Append(cs("fo"), cs("c")),
            SetAssignment::Prepend(cs("com"), cs("b:##")),
            SetAssignment::Append(cs("tw"), cs("4")),
        ]
    );
}

#[test]
fn set_operator_values_may_contain_equals_and_colons() {
    assert_eq!(
        set_args("setlocal com-=fb:- com=s1:/*,mb:*,ex:*/"),
        vec![
            SetAssignment::Remove(cs("com"), cs("fb:-")),
            SetAssignment::Assign(cs("com"), cs("s1:/*,mb:*,ex:*/")),
        ]
    );
    assert_eq!(
        set_args("set cms==%s"),
        vec![SetAssignment::Assign(cs("cms"), cs("=%s"))]
    );
}

#[test]
fn set_empty_operator_value() {
    assert_eq!(
        set_args("set fo-= com+="),
        vec![
            SetAssignment::Remove(cs("fo"), cs("")),
            SetAssignment::Append(cs("com"), cs("")),
        ]
    );
}

#[test]
fn set_colon_assignment() {
    assert_eq!(
        set_args("set tw:7"),
        vec![SetAssignment::Assign(cs("tw"), cs("7"))]
    );
}

#[test]
fn set_arguments_split_and_values_as_before_operators() {
    // The operators did not change how other arguments split or how values
    // read: white space separates arguments and a backslash is kept.
    assert_eq!(
        set_args(r"set sw=\2 cms=a\\ ts=4"),
        vec![
            SetAssignment::Assign(cs("sw"), cs(r"\2")),
            SetAssignment::Assign(cs("cms"), cs(r"a\\")),
            SetAssignment::Assign(cs("ts"), cs("4")),
        ]
    );
    assert_eq!(
        set_args("set sw =2"),
        vec![
            SetAssignment::SetBool(cs("sw")),
            SetAssignment::Assign(cs(""), cs("2")),
        ]
    );
}

#[test]
fn set_backslash_before_white_space_keeps_it_in_the_value() {
    // `:help option-backslash`: Vim 9.1 gives "# %s" and "<!-- %s -->".
    assert_eq!(
        set_args(r"setlocal commentstring=#\ %s"),
        vec![SetAssignment::Assign(cs("commentstring"), cs("# %s"))]
    );
    assert_eq!(
        set_args(r"setlocal comments=fb:* commentstring=<!--\ %s\ -->"),
        vec![
            SetAssignment::Assign(cs("comments"), cs("fb:*")),
            SetAssignment::Assign(cs("commentstring"), cs("<!-- %s -->")),
        ]
    );
}

#[test]
fn set_bool_forms_unchanged() {
    assert_eq!(
        set_args("set ic noai et! ts? all"),
        vec![
            SetAssignment::SetBool(cs("ic")),
            SetAssignment::UnsetBool(cs("ai")),
            SetAssignment::ToggleBool(cs("et")),
            SetAssignment::Query(cs("ts")),
            SetAssignment::ShowAll,
        ]
    );
}

#[test]
fn set_colon_after_a_sign_is_a_plain_assignment() {
    // `tw+:7` is not an operator; it reaches the executor as an assignment
    // to an unknown option `tw+`, as before the operators.
    assert_eq!(
        set_args("set tw+:7"),
        vec![SetAssignment::Assign(cs("tw+"), cs("7"))]
    );
}
