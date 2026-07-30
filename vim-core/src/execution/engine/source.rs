//! `:source` config file processing for [`VimEngine`].
//!
//! Provides a convenience method to process config file content line by line,
//! applying safe ex commands (`:set`, `:let mapleader`, `:map`/`:noremap`/`:unmap`,
//! `:noh`) without requiring document context.

use super::Response;
use crate::commands::ex::effects as ex_effects;
use crate::effects::Effects;
use crate::execution::executor_ex;
use crate::grammar::types::ExCommand;
use crate::keymap::KeyEvent;
use compact_str::CompactString;

impl super::VimEngine {
    /// Process a config file's content line by line as ex commands.
    ///
    /// Called by the host after reading a config file (from `:source`).
    /// Processes only safe commands that do not require document context:
    ///
    /// - `:set` — applies option mutations to engine options
    /// - `:let mapleader` — sets the leader key for subsequent mappings
    /// - `:map`/`:noremap`/`:unmap` — installs or removes key mappings
    /// - `:noh` — clears search highlights
    ///
    /// File operations and commands requiring document context are skipped.
    /// Lines starting with `"` are treated as comments and ignored.
    ///
    /// Returns a [`Response`] with any resulting effects.
    pub fn source_config_text(&mut self, text: &str) -> Response {
        let mut all_effects = Effects::new();

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('"') {
                continue;
            }

            let Ok(cmd) = crate::grammar::parse_ex_command(trimmed) else {
                continue; // Skip unparseable lines silently (like Vim)
            };

            match &cmd {
                // During config sourcing there is no buffer/window context, so
                // :setlocal and :setglobal are treated the same as :set
                // (always SetScope::Effective on the global layer). This matches
                // the spec's documented limitation for sourced config files.
                ExCommand::Set { assignments }
                | ExCommand::SetLocal { assignments }
                | ExCommand::SetGlobal { assignments } => {
                    let effects = executor_ex::apply_set_assignments(
                        executor_ex::SetScope::Effective,
                        &mut self.options,
                        &mut self.buffer_overrides,
                        &mut self.window_overrides,
                        assignments,
                    );
                    self.rebuild_resolved_cache();
                    self.rebuild_langmap_if_needed();
                    all_effects.extend(effects);
                }
                ExCommand::Map {
                    mode_prefix,
                    lhs,
                    rhs: Some(rhs),
                    kind,
                    flags,
                } => {
                    self.source_apply_map(*mode_prefix, lhs, rhs, *kind, *flags);
                }
                ExCommand::Unmap { mode_prefix, lhs } => {
                    self.source_apply_unmap(*mode_prefix, lhs);
                }
                ExCommand::LetMapleader { leader } => {
                    self.set_leader(KeyEvent::char(*leader));
                }
                ExCommand::SetHandler { key, assignments } => {
                    // Call the dedicated sethandler executor (needs only the
                    // parsed fields, not a document context).
                    if let Ok(output) = executor_ex::execute_sethandler(key.as_deref(), assignments)
                    {
                        for change in &output.handler_changes {
                            self.apply_handler_change(change);
                        }
                    }
                }
                ExCommand::NoHighlight => {
                    all_effects.extend(Effects::new().clear_highlights());
                }
                _ => {} // Skip commands requiring document context
            }
        }

        all_effects.extend(ex_effects::show_message(CompactString::from(
            "Config file sourced",
        )));
        Response::with_effects(all_effects)
    }

    /// Apply a `:map`/`:noremap` command from config, resolving `<Action>(name)`
    /// and `<Plug>(name)` notation against the keymap's registries.
    fn source_apply_map(
        &mut self,
        mode_prefix: crate::grammar::types::MapModePrefix,
        lhs: &str,
        rhs: &str,
        kind: crate::keymap::MappingKind,
        flags: crate::keymap::MappingFlags,
    ) {
        if lhs.is_empty() {
            return;
        }
        let parse =
            |s, km: &mut _| executor_ex::parse_key_notation_sequence_with_keymap(s, Some(km));
        let lhs_seq = parse(lhs, &mut self.keymap);
        if flags.expr {
            let expr_text = Some(compact_str::CompactString::from(rhs));
            for &mode in &executor_ex::modes_for_prefix(mode_prefix) {
                self.map_with_expr(
                    mode,
                    lhs_seq.as_slice(),
                    Vec::new(),
                    kind,
                    flags,
                    expr_text.clone(),
                );
            }
        } else {
            let rhs_seq = parse(rhs, &mut self.keymap);
            for &mode in &executor_ex::modes_for_prefix(mode_prefix) {
                self.map(mode, lhs_seq.as_slice(), rhs_seq.clone(), kind, flags);
            }
        }
    }

    /// Apply a `:unmap` command from config, resolving `<Action>(name)` and
    /// `<Plug>(name)` notation against the keymap's registries.
    fn source_apply_unmap(&mut self, mode_prefix: crate::grammar::types::MapModePrefix, lhs: &str) {
        let lhs_seq =
            executor_ex::parse_key_notation_sequence_with_keymap(lhs, Some(&mut self.keymap));
        for &mode in &executor_ex::modes_for_prefix(mode_prefix) {
            self.unmap(mode, lhs_seq.as_slice());
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::execution::VimEngine;

    #[test]
    fn source_config_applies_set_options() {
        let mut engine = VimEngine::new();
        let config = "set tabstop=8\nset expandtab\nset noautoindent\n";
        let response = engine.source_config_text(config);

        assert_eq!(engine.options().tabstop(), 8);
        assert!(engine.options().expandtab());
        assert!(!engine.options().autoindent());
        assert!(!response.effects.is_empty());
    }

    #[test]
    fn source_config_skips_comments_and_empty_lines() {
        let mut engine = VimEngine::new();
        let config = "\" This is a comment\n\n  \" Another comment\nset tabstop=2\n";
        engine.source_config_text(config);

        assert_eq!(engine.options().tabstop(), 2);
    }

    #[test]
    fn source_config_skips_unparseable_lines() {
        let mut engine = VimEngine::new();
        let config = "not_a_valid_command\nset tabstop=4\ngibberish!!!!\n";
        engine.source_config_text(config);

        assert_eq!(engine.options().tabstop(), 4);
    }

    #[test]
    fn source_config_applies_mappings() {
        let mut engine = VimEngine::new();
        let config = "nnoremap jk <Esc>\n";
        let response = engine.source_config_text(config);

        // Mapping was applied (we can verify it indirectly by checking
        // that could_start_mapping returns true for 'j')
        let j_key = crate::keymap::KeyEvent::char('j');
        assert!(engine.could_start_mapping(j_key));
        assert!(!response.effects.is_empty());
    }

    #[test]
    fn source_config_applies_let_mapleader() {
        let mut engine = VimEngine::new();
        // Default leader is backslash
        let default_leader = engine.leader();

        let config = "let mapleader = \" \"\n";
        engine.source_config_text(config);

        let new_leader = engine.leader();
        assert_ne!(default_leader, new_leader);
        assert_eq!(new_leader, crate::keymap::KeyEvent::char(' '));
    }

    #[test]
    fn source_config_leader_before_mapping() {
        let mut engine = VimEngine::new();
        let config = "let mapleader = \",\"\nnnoremap <Leader>w :save<CR>\n";
        engine.source_config_text(config);

        // The mapping should be registered under comma (the leader)
        let comma = crate::keymap::KeyEvent::char(',');
        assert!(engine.could_start_mapping(comma));
    }

    #[test]
    fn source_config_shows_sourced_message() {
        let mut engine = VimEngine::new();
        let response = engine.source_config_text("");

        // Even with empty config, should show "Config file sourced"
        let has_message = response.effects.iter().any(|e| {
            matches!(e, crate::effects::Effect::ShowInfo { info: crate::effects::InfoMessage::Text(text) } if text.contains("sourced"))
        });
        assert!(has_message);
    }

    // ── :sethandler via source ──────────────────────────────────────

    #[test]
    fn source_config_applies_sethandler_single_key_mode() {
        use crate::keymap::{Handler, KeyEvent, MappingMode};

        let mut engine = VimEngine::new();
        let config = "sethandler <C-a> i:ide\n";
        engine.source_config_text(config);

        assert!(
            engine
                .handler_map()
                .is_host_handled(KeyEvent::ctrl('a'), MappingMode::Insert,),
            "Ctrl-A should be host-handled in Insert after sethandler <C-a> i:ide"
        );
        assert_eq!(
            engine
                .handler_map()
                .get(KeyEvent::ctrl('a'), MappingMode::Normal),
            Handler::Vim,
            "Ctrl-A should still be Vim-handled in Normal (not configured)"
        );
    }

    #[test]
    fn source_config_applies_sethandler_multiple_assignments() {
        use crate::keymap::{KeyEvent, MappingMode};

        let mut engine = VimEngine::new();
        let config = "sethandler <C-a> n:vim i:ide\n";
        engine.source_config_text(config);

        assert!(
            !engine
                .handler_map()
                .is_host_handled(KeyEvent::ctrl('a'), MappingMode::Normal,),
            "Ctrl-A Normal should be Vim"
        );
        assert!(
            engine
                .handler_map()
                .is_host_handled(KeyEvent::ctrl('a'), MappingMode::Insert,),
            "Ctrl-A Insert should be Host"
        );
    }

    #[test]
    fn source_config_applies_sethandler_dash_modes() {
        use crate::keymap::{KeyEvent, MappingMode};

        let mut engine = VimEngine::new();
        let config = "sethandler <C-c> n-v:ide i:vim\n";
        engine.source_config_text(config);

        assert!(
            engine
                .handler_map()
                .is_host_handled(KeyEvent::ctrl('c'), MappingMode::Normal,),
            "Ctrl-C Normal should be Host (ide)"
        );
        assert!(
            engine
                .handler_map()
                .is_host_handled(KeyEvent::ctrl('c'), MappingMode::Visual,),
            "Ctrl-C Visual should be Host (ide)"
        );
        assert!(
            !engine
                .handler_map()
                .is_host_handled(KeyEvent::ctrl('c'), MappingMode::Insert,),
            "Ctrl-C Insert should be Vim"
        );
    }

    #[test]
    fn source_config_applies_sethandler_all_modes() {
        use crate::keymap::{KeyEvent, MappingMode};

        let mut engine = VimEngine::new();
        let config = "sethandler <C-v> a:host\n";
        engine.source_config_text(config);

        for mm in MappingMode::ALL {
            assert!(
                engine
                    .handler_map()
                    .is_host_handled(KeyEvent::ctrl('v'), mm),
                "Ctrl-V should be host-handled in {mm:?}"
            );
        }
    }

    #[test]
    fn source_config_sethandler_then_key_returns_ignored() {
        use crate::execution::InputContext;
        use crate::keymap::KeyEvent;
        use crate::test_utils::SimpleDocument;

        let mut engine = VimEngine::new();
        let config = "sethandler <C-a> n:ide\n";
        engine.source_config_text(config);

        // Process Ctrl-A in Normal mode — should be ignored (host-handled)
        let doc = SimpleDocument::new("hello");
        let ctx = InputContext::new(&doc, 0).validate().unwrap();
        let response = engine.process(KeyEvent::ctrl('a'), ctx);

        assert!(
            !response.consumed(),
            "Ctrl-A in Normal should return Ignored after sethandler n:ide"
        );
    }

    #[test]
    fn source_config_sethandler_vim_still_processes() {
        use crate::execution::InputContext;
        use crate::keymap::KeyEvent;
        use crate::test_utils::SimpleDocument;

        let mut engine = VimEngine::new();
        // Set Ctrl-A to Vim in Normal, Host in Insert
        let config = "sethandler <C-a> n:vim i:ide\n";
        engine.source_config_text(config);

        // Process Ctrl-A in Normal mode — should be consumed (Vim handles it)
        let doc = SimpleDocument::new("hello");
        let ctx = InputContext::new(&doc, 0).validate().unwrap();
        let response = engine.process(KeyEvent::ctrl('a'), ctx);

        assert!(
            response.consumed(),
            "Ctrl-A in Normal should be consumed after sethandler n:vim"
        );
    }

    #[test]
    fn source_config_sethandler_abbreviated() {
        use crate::keymap::{KeyEvent, MappingMode};

        let mut engine = VimEngine::new();
        let config = "seth <C-a> i:ide\n";
        engine.source_config_text(config);

        assert!(
            engine
                .handler_map()
                .is_host_handled(KeyEvent::ctrl('a'), MappingMode::Insert,),
            "seth abbreviation should work for sethandler"
        );
    }

    #[test]
    fn source_config_sethandler_invalid_handler_silently_skipped() {
        let mut engine = VimEngine::new();
        // Invalid handler name "banana" — source_config_text silently skips errors
        let config = "sethandler <C-a> n:banana\n";
        engine.source_config_text(config);

        // Handler map should be unchanged (still empty)
        assert!(engine.handler_map().is_empty());
    }

    #[test]
    fn source_config_sethandler_no_key_is_noop() {
        let mut engine = VimEngine::new();
        // No key specified — global default, currently a no-op
        let config = "sethandler n:vim\n";
        engine.source_config_text(config);

        // Should not error, handler map stays empty
        assert!(engine.handler_map().is_empty());
    }

    // ── :set langmap / langremap via source ────────────────────────────

    #[test]
    fn source_config_langmap_remaps_key_in_normal_mode() {
        use crate::effects::Effect;
        use crate::execution::InputContext;
        use crate::keymap::KeyEvent;
        use crate::test_utils::SimpleDocument;

        let mut engine = VimEngine::new();
        // Semicolon format: й maps to j, ц maps to k
        let config = "set langmap=йц;jk\n";
        engine.source_config_text(config);

        // Verify the langmap table was populated
        assert!(
            !engine.langmap_table.is_empty(),
            "langmap table should be non-empty after :set langmap=йц;jk"
        );

        // Process й in Normal mode — should be remapped to j (cursor down)
        let doc: &'static SimpleDocument =
            Box::leak(Box::new(SimpleDocument::new("hello\nworld\nfoo\n")));
        let ctx = InputContext::new(doc, 0).validate_clamped();
        let response = engine.process(KeyEvent::char('й'), ctx);

        assert!(response.consumed(), "й should be consumed (remapped to j)");
        assert!(
            response
                .effects()
                .iter()
                .any(|e| matches!(e, Effect::SetCursor { .. })),
            "langmap й→j should produce a SetCursor effect (cursor down), got: {:?}",
            response.effects(),
        );
    }

    #[test]
    fn source_config_langmap_direct_table_set() {
        use crate::effects::Effect;
        use crate::execution::InputContext;
        use crate::keymap::KeyEvent;
        use crate::test_utils::SimpleDocument;

        let mut engine = VimEngine::new();
        // Directly set the langmap table for testing (bypassing :set)
        engine.langmap_table = crate::keymap::LangmapTable::parse("йц;jk").unwrap();

        let doc: &'static SimpleDocument =
            Box::leak(Box::new(SimpleDocument::new("hello\nworld\nfoo\n")));
        let ctx = InputContext::new(doc, 0).validate_clamped();
        let response = engine.process(KeyEvent::char('й'), ctx);

        assert!(
            response.consumed(),
            "й should be consumed (remapped to j via direct table)"
        );
        assert!(
            response
                .effects()
                .iter()
                .any(|e| matches!(e, Effect::SetCursor { .. })),
            "langmap й→j should produce SetCursor, got: {:?}",
            response.effects(),
        );
    }

    #[test]
    fn source_config_set_langremap() {
        let mut engine = VimEngine::new();
        // Default is false (Neovim convention)
        assert!(!engine.options().langremap());

        let config = "set langremap\n";
        engine.source_config_text(config);
        assert!(
            engine.options().langremap(),
            "langremap should be true after :set langremap"
        );

        let config2 = "set nolangremap\n";
        engine.source_config_text(config2);
        assert!(
            !engine.options().langremap(),
            "langremap should be false after :set nolangremap"
        );
    }

    #[test]
    fn source_config_langmap_sets_key_interest_dirty() {
        let mut engine = VimEngine::new();
        // Clear the dirty flag (it starts true)
        engine.key_interest_dirty = false;

        let config = "set langmap=ab\n";
        engine.source_config_text(config);

        assert!(
            engine.key_interest_dirty,
            "key_interest_dirty should be set after langmap change"
        );
    }

    #[test]
    fn source_config_langmap_empty_clears_table() {
        let mut engine = VimEngine::new();
        // First set a non-empty langmap
        let config = "set langmap=йц;jk\n";
        engine.source_config_text(config);
        assert!(!engine.langmap_table.is_empty());

        // Now clear it
        let config2 = "set langmap=\n";
        engine.source_config_text(config2);
        assert!(
            engine.langmap_table.is_empty(),
            "langmap table should be empty after :set langmap="
        );
    }
}
