#![allow(dead_code)]
//! Test orchestrator.
//!
//! Main entry point for running fidelity tests.
//! Coordinates golden file loading, VimEngine execution, and result comparison.

use super::{apply_effect, compare_states, parse_keys};
use crate::common::document::TestDocument;
use crate::common::golden::{self, GoldenFile, GoldenState, TestInput};
use crate::common::neovim_oracle::NeovimOracle;

/// Convert a 0-indexed (line, col) pair to a byte offset in `text`.
///
/// Returns `None` if the line/col is out of range.
fn line_col_to_offset(text: &str, line: usize, col: usize) -> Option<usize> {
    let mut current_line = 0;
    let mut line_start = 0;
    for (i, ch) in text.char_indices() {
        if current_line == line {
            let col_offset = line_start + col;
            if col_offset <= text.len() {
                return Some(col_offset);
            }
            return None;
        }
        if ch == '\n' {
            current_line += 1;
            line_start = i + 1;
        }
    }
    // Handle last line (or only line with no trailing newline)
    if current_line == line {
        let col_offset = line_start + col;
        if col_offset <= text.len() {
            return Some(col_offset);
        }
    }
    None
}

/// Insert a `|` cursor marker at `offset` in `text`.
///
/// For selections, uses `#[...|]#` (forward) or `#[|...]#` (backward) notation.
/// Returns `None` if any offset is out of bounds or not on a char boundary.
fn annotate_cursor(text: &str, cursor: usize, anchor: Option<usize>) -> Option<String> {
    match anchor {
        None => {
            // Simple cursor: insert `|` at offset
            if cursor > text.len() || !text.is_char_boundary(cursor) {
                return None;
            }
            let mut s = String::with_capacity(text.len() + 1);
            s.push_str(&text[..cursor]);
            s.push('|');
            s.push_str(&text[cursor..]);
            Some(s)
        }
        Some(a) => {
            // Selection: use #[...|]# or #[|...]# notation
            if cursor > text.len()
                || a > text.len()
                || !text.is_char_boundary(cursor)
                || !text.is_char_boundary(a)
            {
                return None;
            }
            let mut s = String::with_capacity(text.len() + 5);
            if cursor <= a {
                // Backward or collapsed: #[|text]#
                s.push_str(&text[..cursor]);
                s.push_str("#[|");
                s.push_str(&text[cursor..a]);
                s.push_str("]#");
                s.push_str(&text[a..]);
            } else {
                // Forward: #[text|]#
                s.push_str(&text[..a]);
                s.push_str("#[");
                s.push_str(&text[a..cursor]);
                s.push_str("|]#");
                s.push_str(&text[cursor..]);
            }
            Some(s)
        }
    }
}

/// Build annotated text visualization for a `GoldenState`.
///
/// Returns an annotated string like `"hel|lo"` or `"h#[ell|]#o"`.
/// Falls back to `"(annotation unavailable)"` if the cursor is out of bounds.
fn try_annotate_golden(state: &GoldenState) -> String {
    let text = state.text.strip_suffix('\n').unwrap_or(&state.text);
    annotate_cursor(text, state.cursor_offset, state.selection_anchor)
        .unwrap_or_else(|| "(annotation unavailable)".to_string())
}

/// Produce an annotated text visualization block for failure messages.
///
/// Shows input, expected, and actual states with inline cursor markers:
///
/// ```text
///   Input:    "|hello world"
///   Expected: "|world"
///   Actual:   "hello| world"
/// ```
fn annotated_visualization(
    input: &TestInput,
    expected: &GoldenState,
    actual: &GoldenState,
) -> String {
    let input_viz = line_col_to_offset(&input.text, input.cursor.0, input.cursor.1)
        .and_then(|offset| annotate_cursor(&input.text, offset, None))
        .unwrap_or_else(|| "(annotation unavailable)".to_string());

    let expected_viz = try_annotate_golden(expected);
    let actual_viz = try_annotate_golden(actual);

    format!(
        "  Input:    \"{input_viz}\"\n\
         \x20 Expected: \"{expected_viz}\"\n\
         \x20 Actual:   \"{actual_viz}\""
    )
}

/// Run a single fidelity test.
///
/// This is the main entry point called by vim_test! macro.
///
/// # Flow
///
/// 1. Create test input from parameters
/// 2. Load or generate golden file from Neovim
/// 3. Run same keys through VimEngine
/// 4. Compare results and panic on mismatch
pub fn run_fidelity_test(
    name: &str,
    category: &str,
    text: &str,
    cursor: (usize, usize),
    keys: &str,
) {
    // 1. Create test input
    let input = TestInput::new(text, cursor, keys);

    // 2. Get or generate golden file
    // `env!` expands at this call site, so it resolves to vim-core's manifest
    // dir (where tests/golden/ lives), not vim-test's.
    let golden_path = golden::golden_path(env!("CARGO_MANIFEST_DIR"), category, name);

    let golden = if golden::should_regenerate_test(name) || !golden_path.exists() {
        // Generate from Neovim
        match NeovimOracle::capture(&input) {
            Ok((nvim_version, expected)) => {
                let golden = GoldenFile::new(nvim_version, input.clone(), expected);
                if let Err(e) = golden.save(&golden_path) {
                    panic!("Failed to save golden file: {}", e);
                }
                eprintln!("[REGEN] Generated golden file: {:?}", golden_path);
                golden
            }
            Err(e) => {
                panic!("Failed to capture from Neovim: {}", e);
            }
        }
    } else {
        // Load existing golden file
        match GoldenFile::load(&golden_path) {
            Ok(golden) => golden,
            Err(e) => {
                panic!("Failed to load golden file {:?}: {}", golden_path, e);
            }
        }
    };

    // 3. Run through VimEngine (pass viewport info from golden for H/M/L)
    let actual = run_vim_commands(&input, golden.expected.window.as_ref(), false);

    // 4. Compare results
    if let Some(diff) = compare_states(&actual, &golden.expected) {
        let viz = annotated_visualization(&input, &golden.expected, &actual);
        eprintln!("[DEBUG] actual text: {:?}", actual.text);
        eprintln!("[DEBUG] expect text: {:?}", golden.expected.text);
        panic!(
            "\n\n{bar}\n\
             FIDELITY TEST FAILED: {name}\n\
             {bar}\n\n\
             {viz}\n\n\
             Input:\n\
               Text: {text:?}\n\
               Cursor: ({line}, {col})\n\
               Keys: {keys:?}\n\n\
             {diff}\n",
            bar = "═".repeat(60),
            name = name,
            viz = viz,
            text = input.text,
            line = input.cursor.0,
            col = input.cursor.1,
            keys = input.keys,
            diff = diff,
        );
    }

    // 5. Run again with shadow execution enabled.
    // Shadow is supposed to be transparent — same results, different internal
    // path. Any difference is a bug (e.g., the shadow-undo-merge bug where
    // shadow inside drain_pending_keys consumed macro keys prematurely).
    let shadow_actual = run_vim_commands(&input, golden.expected.window.as_ref(), true);

    if let Some(diff) = compare_states(&shadow_actual, &golden.expected) {
        let viz = annotated_visualization(&input, &golden.expected, &shadow_actual);
        panic!(
            "\n\n{bar}\n\
             FIDELITY TEST FAILED (SHADOW): {name}\n\
             {bar}\n\n\
             {viz}\n\n\
             The test passes without shadow execution but FAILS with it.\n\
             This indicates a shadow execution bug.\n\n\
             Input:\n\
               Text: {text:?}\n\
               Cursor: ({line}, {col})\n\
               Keys: {keys:?}\n\n\
             {diff}\n",
            bar = "═".repeat(60),
            name = name,
            viz = viz,
            text = input.text,
            line = input.cursor.0,
            col = input.cursor.1,
            keys = input.keys,
            diff = diff,
        );
    }
}

/// Run commands through VimEngine.
///
/// # Steps
///
/// 1. Create TestDocument with initial state
/// 2. Create VimEngine
/// 3. Process each key with validated InputContext
/// 4. Capture final state
pub fn run_vim_commands(
    input: &TestInput,
    window: Option<&crate::common::golden::WindowState>,
    shadow: bool,
) -> GoldenState {
    use vim_core::effects::Effect;
    use vim_core::execution::{InputContext, VimEngine};

    // 1. Create TestDocument with initial state
    // The golden input text uses Neovim's table.concat(buf_get_lines, '\n')
    // convention: N lines produce N-1 '\n' separators. We feed this directly
    // to our engine so both sides start with the same number of lines.
    //
    // Historical note: an earlier version stripped one trailing '\n' here,
    // but that caused a 1-line deficit for inputs like "\n" (2 lines in
    // Neovim, 1 in ours), breaking operations like O/cc/yy on empty buffers.
    // The golden files generated with escape_for_lua correctly preserve
    // leading newlines, so no stripping is needed.
    let mut doc = TestDocument::new(&input.text, input.cursor);

    // 2. Create VimEngine
    let mut engine = VimEngine::new();
    engine.set_shadow_execution(shadow);

    #[cfg(feature = "engine-tracing")]
    engine.set_tracing_enabled(true);

    // Macro recording state
    let mut recording_register: Option<char> = None;
    let mut macro_buffer = String::new();

    // Viewport state for H/M/L and scroll commands
    // Use height/width from golden file but start with topline=0 (initial state).
    // The golden file's topline is the *expected* post-scroll state, not the initial.
    // This is mutable because scroll commands (Ctrl-E/Y) update the viewport.
    let mut viewport_info = window.map(|w| {
        vim_core::commands::motions::types::ViewportInfo {
            first_line: 0, // Initial viewport always starts at top
            height: w.height,
            width: w.width,
        }
    });

    // 3. Parse and process each key
    let keys = parse_keys(&input.keys);
    for key in keys {
        // If recording, accumulate keystroke (but skip the `q` that stops recording)
        // We add the keystroke BEFORE processing, then remove the final `q` on stop
        let key_repr = key_to_string(&key);
        let was_recording = recording_register.is_some();

        // Create and validate InputContext with current cursor position
        // Use validate_clamped() - never fails, clamps to bounds
        let mut ctx = InputContext::new(&doc, doc.cursor_offset()).validate_clamped();

        // Add selection if in visual mode
        if let Some(selection) = doc.selection() {
            ctx = ctx.with_selection(selection);
        }

        // Add viewport info for H/M/L motions
        if let Some(vp) = viewport_info {
            ctx = ctx.with_viewport(vp);
        }

        let response = engine.process(key, ctx);

        #[cfg(feature = "engine-tracing")]
        for event in engine.drain_trace_events() {
            eprintln!("[trace] {:?}", event);
        }

        // Snapshot effects for invariant checking before apply_response consumes them
        let effects_for_check: Vec<Effect> = response.effects().to_vec();

        apply_response(
            &mut engine,
            &mut doc,
            response,
            &mut recording_register,
            &mut macro_buffer,
        );

        // Update viewport tracking: ensure cursor is visible and scroll
        // commands advance the viewport for subsequent operations.
        if let Some(ref mut vp) = viewport_info {
            update_viewport_after_key(vp, &doc, key);
        }

        // Per-key invariant checks (after effects applied, cursor updated)
        super::invariants::check_per_key(&engine, &doc, &effects_for_check, &key_repr);

        // '^' mark (INSERT_STOP) is set by the engine's SetMark effect in exit_finalize.
        // No manual override needed — the engine emits the correct pre-backup position.

        // Drain pending entries (mapping expansion + macro replay) via unified API.
        // In Vim, N@a produces a single undo entry. We wrap the entire macro
        // replay in a single undo group on the TestDocument, stripping
        // intermediate BeginUndoGroup/EndUndoGroup effects from individual
        // key responses (the engine also strips them via is_merging check).
        // The engine calls begin_merge() in process_macro_plays,
        // so is_merging() is true when macro frames are on the stack.
        let is_macro_replay = engine.has_pending_keys() && engine.state().undo_tree().is_merging();
        if is_macro_replay {
            doc.begin_undo_group(false);
        }
        while let Some(pending_output) = engine.drain_next_key() {
            use vim_core::execution::MacroOutput;

            match pending_output {
                MacroOutput::Key(pending_key) => {
                    let mut pending_ctx =
                        InputContext::new(&doc, doc.cursor_offset()).validate_clamped();
                    if let Some(selection) = doc.selection() {
                        pending_ctx = pending_ctx.with_selection(selection);
                    }
                    let pending_key_repr = key_to_string(&pending_key);
                    let pending_response = engine.process(pending_key, pending_ctx);

                    #[cfg(feature = "engine-tracing")]
                    for event in engine.drain_trace_events() {
                        eprintln!("[trace] {:?}", event);
                    }

                    // Snapshot effects for invariant checking
                    let pending_effects: Vec<Effect> = pending_response.effects().to_vec();

                    // Vim aborts macro replay on error (e.g., motion past EOF)
                    let has_error = !pending_response.consumed()
                        || pending_effects
                            .iter()
                            .any(|e| matches!(e, Effect::ShowError { .. }));

                    apply_response(
                        &mut engine,
                        &mut doc,
                        pending_response,
                        &mut recording_register,
                        &mut macro_buffer,
                    );

                    // Per-key invariant checks for pending/macro keys
                    super::invariants::check_per_key(
                        &engine,
                        &doc,
                        &pending_effects,
                        &pending_key_repr,
                    );

                    if has_error {
                        engine.abort_replay();
                        break;
                    }
                }
                MacroOutput::TextBlock {
                    text,
                    cursor_offset,
                } => {
                    // Apply text block directly at current cursor position.
                    let insert_pos = doc.cursor_offset();
                    doc.apply_insert(insert_pos, &text);
                    doc.set_cursor_offset(insert_pos + cursor_offset);
                }
            }
        }
        if is_macro_replay {
            doc.end_undo_group();
        }

        // If we were recording (after processing), accumulate keystroke
        // Skip the key that started recording (qa) or stopped it (q)
        if was_recording && recording_register.is_some() {
            // Still recording after this key - add to buffer
            macro_buffer.push_str(&key_repr);
        }
    }

    // Auto-escape: If the engine is in Insert or Replace mode at the end of key processing,
    // send <Esc> to normalize. The Neovim golden file generator captures state after
    // escaping from insert/replace mode. Visual/CommandLine modes are NOT escaped.
    {
        use vim_core::primitives::Mode;
        let mode = engine.mode();
        if matches!(mode, Mode::Insert | Mode::Replace) {
            let esc_key = vim_core::keymap::KeyEvent::escape();
            let ctx = InputContext::new(&doc, doc.cursor_offset()).validate_clamped();
            let response = engine.process(esc_key, ctx);

            #[cfg(feature = "engine-tracing")]
            for event in engine.drain_trace_events() {
                eprintln!("[trace] {:?}", event);
            }

            apply_response(
                &mut engine,
                &mut doc,
                response,
                &mut recording_register,
                &mut macro_buffer,
            );
        }
    }

    // Per-test invariant checks (after auto-escape, before golden capture)
    super::invariants::check_per_test(&engine, &doc);

    // 4. Capture final state
    doc.to_golden_state(&engine)
}

fn apply_response(
    engine: &mut vim_core::execution::VimEngine,
    doc: &mut TestDocument,
    mut response: vim_core::execution::Response,
    recording_register: &mut Option<char>,
    macro_buffer: &mut String,
) {
    use std::collections::VecDeque;
    use vim_core::effects::Effect;
    use vim_core::execution::HostRequest;

    let effects = response.take_effects();
    let host_requests = response.take_host_requests();

    for effect in effects {
        match effect {
            Effect::StartRecording { register } => {
                *recording_register = Some(register.char());
                macro_buffer.clear();
            }
            Effect::StopRecording => {
                if let Some(reg) = recording_register.take() {
                    let content = macro_buffer.clone();
                    doc.set_register(reg, content.clone(), "v".to_string());
                    // Sync to engine registers so drain_next_key() can read it
                    engine.apply_effect(&Effect::SetRegister {
                        name: vim_core::primitives::RegisterName::new(reg).unwrap(),
                        text: compact_str::CompactString::from(content),
                        motion_type: vim_core::primitives::MotionType::CharWise,
                    });
                    macro_buffer.clear();
                }
            }
            Effect::OperatorToMark {
                operator,
                mark,
                linewise,
                register,
                cursor,
            } => {
                super::operator_to_mark::apply_operator_to_mark(
                    doc,
                    Some(engine),
                    operator,
                    mark,
                    linewise,
                    register,
                    cursor,
                );
            }
            Effect::SetSearchPattern { pattern, direction } => {
                let search_dir = if direction.is_forward() {
                    vim_core::primitives::SearchDirection::Forward
                } else {
                    vim_core::primitives::SearchDirection::Backward
                };
                engine.set_search_pattern(pattern.to_string(), search_dir);
                doc.set_register('/', pattern.to_string(), "v".to_string());
            }
            Effect::NormCommand {
                start_line,
                end_line,
                keys,
                ..
            } => {
                // Execute normal-mode keys on each line in the range.
                use vim_core::document::Document as _;
                use vim_core::execution::InputContext;
                use vim_core::keymap::{Key, KeyEvent, Modifiers};

                let total_lines = doc.text().lines().count();
                let end = end_line.get().min(total_lines.saturating_sub(1));
                let norm_keys = parse_keys(&keys);

                for line in start_line.get()..=end {
                    // Find line start offset by counting newlines in the text
                    let text = doc.text();
                    let line_offset = text
                        .as_bytes()
                        .iter()
                        .enumerate()
                        .filter(|(_, &b)| b == b'\n')
                        .map(|(i, _)| i + 1)
                        .nth(line.wrapping_sub(1))
                        .unwrap_or(if line == 0 { 0 } else { text.len() });
                    let line_offset = if line == 0 { 0 } else { line_offset };
                    if line_offset > text.len() {
                        break;
                    }

                    doc.set_cursor_offset(line_offset);

                    for key in &norm_keys {
                        let ctx = InputContext::new(&*doc, doc.cursor_offset()).validate_clamped();
                        let response = engine.process(*key, ctx);
                        apply_response(engine, doc, response, recording_register, macro_buffer);
                    }

                    // Vim's :norm implicitly returns to normal mode after keys are exhausted.
                    if engine.mode() != vim_core::primitives::Mode::Normal {
                        let esc_key = KeyEvent::new(Key::Escape, Modifiers::NONE);
                        let ctx = InputContext::new(&*doc, doc.cursor_offset()).validate_clamped();
                        let response = engine.process(esc_key, ctx);
                        apply_response(engine, doc, response, recording_register, macro_buffer);
                    }
                }
            }
            other => apply_effect(doc, other),
        }
    }

    let mut queue: VecDeque<HostRequest> = host_requests.into_iter().collect();
    while let Some(request) = queue.pop_front() {
        if let HostRequest::ExecuteNorm {
            meta,
            start_line,
            end_line,
            keys,
            ..
        } = &request
        {
            use vim_core::document::Document as _;
            use vim_core::execution::InputContext;
            use vim_core::keymap::{Key, KeyEvent, Modifiers};

            let total_lines = doc.text().lines().count();
            let end = (*end_line as usize).min(total_lines.saturating_sub(1));
            let norm_keys = parse_keys(keys);

            for line in (*start_line as usize)..=end {
                let text = doc.text();
                let line_offset = text
                    .as_bytes()
                    .iter()
                    .enumerate()
                    .filter(|(_, &b)| b == b'\n')
                    .map(|(i, _)| i + 1)
                    .nth(line.wrapping_sub(1))
                    .unwrap_or(if line == 0 { 0 } else { text.len() });
                let line_offset = if line == 0 { 0 } else { line_offset };
                if line_offset > text.len() {
                    break;
                }

                doc.set_cursor_offset(line_offset);

                for key in &norm_keys {
                    let ctx = InputContext::new(&*doc, doc.cursor_offset()).validate_clamped();
                    let response = engine.process(*key, ctx);
                    apply_response(engine, doc, response, recording_register, macro_buffer);
                }

                if engine.mode() != vim_core::primitives::Mode::Normal {
                    let esc_key = KeyEvent::new(Key::Escape, Modifiers::NONE);
                    let ctx = InputContext::new(&*doc, doc.cursor_offset()).validate_clamped();
                    let response = engine.process(esc_key, ctx);
                    apply_response(engine, doc, response, recording_register, macro_buffer);
                }
            }

            let result = vim_core::execution::HostResult::Success {
                id: meta.id,
                message: None,
            };
            let completion = engine.complete_host_request(&result);
            apply_response(engine, doc, completion, recording_register, macro_buffer);
            continue;
        }

        let result = execute_host_request(doc, &request);
        let completion = engine.complete_host_request(&result);
        apply_response(engine, doc, completion, recording_register, macro_buffer);
    }
}

fn execute_host_request(
    doc: &TestDocument,
    request: &vim_core::execution::HostRequest,
) -> vim_core::execution::HostResult {
    use vim_core::execution::{HostRequest, HostResult};

    match request {
        HostRequest::ReadFile {
            meta,
            path,
            after_line,
        } => match std::fs::read_to_string(path.as_str()) {
            Ok(data) => HostResult::Data {
                id: meta.id,
                data: data.into(),
                offset: Some(read_insert_offset(doc, *after_line)),
            },
            Err(err) => HostResult::Failure {
                id: meta.id,
                error: err.to_string().into(),
            },
        },
        HostRequest::ReindentRange {
            meta,
            input_text,
            range,
            ..
        } => {
            // Simulate Neovim's cindent with expandtab shiftwidth=4 tabstop=4
            // (matching the oracle settings).
            //
            // Algorithm:
            //  - Determine initial brace nesting by scanning the document
            //    text *before* the range (so `=i{` knows it is inside `{`).
            //  - Strip existing indentation from every line.
            //  - Track brace nesting: `{` at EOL increments, `}` at BOL
            //    decrements.
            //  - Lines inside braces whose predecessor does not end with
            //    `{`, `}`, or `;` receive an extra shiftwidth of
            //    "continuation" indent (matching Neovim default cinoptions).
            //  - Outside braces (nesting == 0) everything goes to indent 0.
            //
            // Cursor is placed at `range.start()` (beginning of the first
            // operated line), matching Neovim's post-`=` cursor placement.
            use vim_core::document::Document as _;
            const SW: usize = 4;
            let has_trailing_newline = input_text.ends_with('\n');
            let lines: Vec<&str> = input_text.lines().collect();
            let mut result = String::with_capacity(input_text.len());

            // Compute initial nesting from document context before range.
            let doc_text = doc.text();
            let prefix = &doc_text[..range.start().get().min(doc_text.len())];
            let mut initial_brace: i32 = 0;
            let mut initial_paren: i32 = 0;
            for c in prefix.chars() {
                match c {
                    '{' => initial_brace += 1,
                    '}' => initial_brace = (initial_brace - 1).max(0),
                    '(' => initial_paren += 1,
                    ')' => initial_paren = (initial_paren - 1).max(0),
                    _ => {}
                }
            }
            let mut brace_nesting: i32 = initial_brace;
            let mut paren_nesting: i32 = initial_paren;
            let mut prev_is_continuation = false;

            for (i, line) in lines.iter().enumerate() {
                let trimmed = line.trim_start();

                // Closing braces decrement before computing indent (so `}` aligns
                // with the opening `{` line). Closing parens do NOT decrement
                // before indent — Neovim's cindent places `)` at the content
                // indent level (same as lines inside the parens).
                if trimmed.starts_with('}') {
                    brace_nesting = (brace_nesting - 1).max(0);
                    prev_is_continuation = false;
                }
                let paren_closing = trimmed.starts_with(')');

                // Brace nesting uses shiftwidth, paren nesting uses 1 space
                let brace_indent = (brace_nesting as usize) * SW;
                let paren_indent = paren_nesting as usize; // cindent (0 default
                let cont = if prev_is_continuation { SW } else { 0 };
                let indent = brace_indent + paren_indent + cont;

                if i > 0 {
                    result.push('\n');
                }
                if !trimmed.is_empty() {
                    for _ in 0..indent {
                        result.push(' ');
                    }
                    result.push_str(trimmed);
                }

                // Decrement paren nesting AFTER computing indent for closing paren
                if paren_closing {
                    paren_nesting = (paren_nesting - 1).max(0);
                }

                if trimmed.ends_with('{') {
                    brace_nesting += 1;
                    prev_is_continuation = false;
                } else if trimmed.ends_with('(') {
                    paren_nesting += 1;
                    prev_is_continuation = false;
                } else {
                    prev_is_continuation = brace_nesting > 0
                        && !trimmed.is_empty()
                        && !trimmed.ends_with('}')
                        && !trimmed.ends_with(';')
                        && !trimmed.starts_with('}');
                }
            }
            if has_trailing_newline {
                result.push('\n');
            }

            // Neovim's `=` operator places cursor at column 0 of the first
            // content line in the reindented range.  When the replacement
            // starts with newlines (text-object ranges that begin at the
            // newline terminating the previous line), skip past them so the
            // cursor lands on the first actual content line.  However, if the
            // replacement consists entirely of newlines (blank lines), keep
            // the cursor at range.start() — the line is blank, and the cursor
            // stays at its beginning.
            let leading_newlines = result.bytes().take_while(|&b| b == b'\n').count();
            let has_content_after = leading_newlines < result.len();
            let skip = if has_content_after {
                leading_newlines
            } else {
                0
            };
            let cursor = range.start().get() + skip;

            // Compute mark `.` offset: Neovim's op_reindent sets mark `.`
            // to the start of the first line whose indentation actually changed.
            // Compare old and new lines to find it.
            let mark_dot = {
                let old_lines: Vec<&str> = input_text.lines().collect();
                let new_lines: Vec<&str> = result.lines().collect();
                let mut offset_in_result = 0usize;
                let mut found = None;
                for (i, (old, new)) in old_lines.iter().zip(new_lines.iter()).enumerate() {
                    if old != new {
                        found = Some(range.start().get() + offset_in_result);
                        break;
                    }
                    offset_in_result += new.len() + 1; // +1 for '\n'
                }
                found
            };

            HostResult::FilteredRange {
                id: meta.id,
                replacement: result.into(),
                cursor_offset: Some(cursor),
                stderr: None,
                mark_dot_offset: mark_dot,
            }
        }
        // In the test harness there is no actual file backing the buffer.
        // Neovim returns "E32: No file name" for :w/:wq/:x on unnamed buffers
        // and "E162: No write since last change" for :q on a modified buffer.
        HostRequest::WriteFile { meta, path, .. } if path.is_none() => HostResult::Failure {
            id: meta.id,
            error: "E32: No file name".into(),
        },
        HostRequest::WriteQuit { meta, .. } => HostResult::Failure {
            id: meta.id,
            error: "E32: No file name".into(),
        },
        HostRequest::Quit { meta, force, .. } if !force => HostResult::Failure {
            id: meta.id,
            error: "E162: No write since last change for buffer \"[No Name]\"".into(),
        },
        HostRequest::CustomExCommand { meta, command } => HostResult::Failure {
            id: meta.id,
            error: format!("E492: Not an editor command: {}", command.as_str()).into(),
        },
        _ => HostResult::Success {
            id: request.id(),
            message: None,
        },
    }
}

fn read_insert_offset(doc: &TestDocument, after_line: Option<u32>) -> usize {
    use vim_core::document::Document as _;

    let text = doc.text();
    let current_line = doc.cursor_position().0 + 1;
    let after = after_line.unwrap_or(current_line as u32) as usize;
    let target = after.saturating_add(1);
    line_start_offset_1_indexed(text, target).unwrap_or(text.len())
}

fn line_start_offset_1_indexed(text: &str, line: usize) -> Option<usize> {
    if line <= 1 {
        return Some(0);
    }
    let mut current = 1usize;
    for (idx, ch) in text.char_indices() {
        if ch == '\n' {
            current += 1;
            if current == line {
                return Some(idx + 1);
            }
        }
    }
    None
}

/// Convert a KeyEvent to its string representation for macro storage.
fn key_to_string(key: &vim_core::keymap::KeyEvent) -> String {
    use vim_core::keymap::{Key, Modifiers};

    let has_ctrl = key.modifiers().contains(Modifiers::CTRL);

    // If Ctrl modifier is present, produce the raw control character
    if has_ctrl {
        if let Key::Char(c) = key.key() {
            // Ctrl-A = 0x01, Ctrl-B = 0x02, ..., Ctrl-Z = 0x1A
            let ctrl_byte = (c.to_ascii_lowercase() as u8).wrapping_sub(b'a' - 1);
            if ctrl_byte <= 26 {
                return String::from(ctrl_byte as char);
            }
        }
        // Ctrl with special keys: fall through to special key handling
    }

    match key.key() {
        // Raw control characters for special keys (matching Neovim internal format)
        Key::Escape => String::from('\x1b'),    // ESC = 0x1b
        Key::Enter => String::from('\r'),       // CR = 0x0d
        Key::Tab => String::from('\t'),         // TAB = 0x09
        Key::Backspace => String::from('\x08'), // BS = 0x08
        Key::Char(c) => c.to_string(),          // Regular characters (including space)
        // Arrow/navigation keys: use Neovim's internal 3-byte encoding
        // These are \x80 followed by two bytes
        Key::Up => "\u{80}ku".to_string(),
        Key::Down => "\u{80}kd".to_string(),
        Key::Left => "\u{80}kl".to_string(),
        Key::Right => "\u{80}kr".to_string(),
        Key::Home => "\u{80}kh".to_string(),
        Key::End => "\u{80}@7".to_string(),
        Key::PageUp => "\u{80}kP".to_string(),
        Key::PageDown => "\u{80}kN".to_string(),
        Key::Delete => "\u{80}kD".to_string(),
        Key::Insert => "\u{80}kI".to_string(),
        Key::F(n) => format!("\u{80}k{}", (n + b'0') as char),
        _ => format!("{:?}", key),
    }
}

/// Update viewport tracking after processing a key.
///
/// Scroll commands (Ctrl-E/Y/F/B/D/U) move the viewport, and subsequent
/// scroll commands need the updated first_line to compute correct positions.
/// For regular motions, just ensure the cursor stays visible.
fn update_viewport_after_key(
    vp: &mut vim_core::commands::motions::types::ViewportInfo,
    doc: &TestDocument,
    key: vim_core::keymap::KeyEvent,
) {
    use vim_core::document::Document as _;
    use vim_core::keymap::{Key, Modifiers};

    let cursor_line = vim_core::commands::helpers::line_of(doc.text(), doc.cursor_offset());
    let total_lines = vim_core::commands::helpers::line_count(doc.text());

    // Detect line-scroll commands (Ctrl-E/Y) which scroll the viewport by a fixed
    // amount regardless of cursor position. For page-scroll commands (Ctrl-F/B/D/U),
    // the viewport follows the cursor naturally via the "cursor visible" logic below.
    let is_ctrl = key.modifiers() == Modifiers::CTRL;
    let line_scroll_delta: Option<isize> = if is_ctrl {
        match key.key() {
            Key::Char('e') => Some(1),  // Ctrl-E: scroll down 1 line
            Key::Char('y') => Some(-1), // Ctrl-Y: scroll up 1 line
            _ => None,
        }
    } else {
        None
    };

    if let Some(delta) = line_scroll_delta {
        // Line-scroll: advance viewport by the scroll amount.
        let max_first = total_lines.saturating_sub(1);
        let new_first = if delta >= 0 {
            (vp.first_line + delta as usize).min(max_first)
        } else {
            vp.first_line.saturating_sub((-delta) as usize)
        };
        vp.first_line = new_first;
    }

    // Ensure cursor is visible within viewport (for all keys)
    if cursor_line < vp.first_line {
        vp.first_line = cursor_line;
    }
    let last_visible = vp.first_line + vp.height.saturating_sub(1);
    if cursor_line > last_visible {
        vp.first_line = cursor_line.saturating_sub(vp.height.saturating_sub(1));
    }
    // Clamp to valid range
    let max_first = total_lines.saturating_sub(1);
    if vp.first_line > max_first {
        vp.first_line = max_first;
    }
}
