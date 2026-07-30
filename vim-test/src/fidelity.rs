//! FidelitySession — orchestrator for Neovim fidelity tests.
//!
//! Wraps [`TestSession`] with a fidelity-specific key loop that handles
//! Neovim-compatible host requests, viewport tracking, auto-escape, and
//! persistent error message accumulation.

use compact_str::CompactString;

use vim_core::dispatch::ViewportInfo;
use vim_core::effects::Effect;
use vim_core::execution::HostSession;
use vim_core::execution::{HostRequest, HostResult, ScrollInfo, ScrollPlacement};
use vim_core::primitives::Mode;

use crate::golden::{
    golden_path, should_regenerate_test, GoldenFile, GoldenState, TestInput, WindowState,
};
use crate::neovim_oracle::NeovimOracle;
use crate::session::{run_per_key_invariants, run_per_test_invariants, TestSession};
use crate::state_diff::compare_states;
use vim_core::keymap::KeyEvent;

/// Fidelity test session — wraps TestSession with Neovim-compatible behavior.
///
/// Manages its own key loop instead of using `TestSession::feed()` because
/// fidelity tests need:
/// - Neovim-compatible host request responses (not auto-handle defaults)
/// - Viewport tracking for H/M/L and scroll commands
/// - Auto-escape from Insert/Replace at end of key sequence
/// - Persistent error message accumulation across keys
pub struct FidelitySession {
    session: TestSession,
    viewport: Option<ViewportInfo>,
    errmsg: Option<String>,
}

impl FidelitySession {
    /// Create a new fidelity session from test input.
    ///
    /// Sets `auto_handle_defaults=false` so host requests are returned
    /// to the caller for Neovim-compatible handling.
    pub fn new(input: &TestInput, window: Option<&WindowState>) -> Self {
        let mut host = HostSession::new(&input.text).with_auto_handle_defaults(false);
        let offset = line_col_to_offset(&input.text, input.cursor.0, input.cursor.1)
            .unwrap_or_else(|| {
                panic!(
                    "cursor ({}, {}) out of range for text of length {}",
                    input.cursor.0,
                    input.cursor.1,
                    input.text.len()
                )
            });
        host.set_cursor_offset(offset);

        let viewport = window.map(|w| ViewportInfo {
            first_line: 0,
            height: w.height,
            width: w.width,
        });
        if let Some(vp) = viewport {
            host.set_viewport(vp);
        }

        let mut session = TestSession::from_host_session(host);
        // FidelitySession manages invariants explicitly via run_per_key/test_invariants.
        // Disable TestSession's Drop-based invariant check to avoid double-running.
        session.disable_invariants();

        Self {
            session,
            viewport,
            errmsg: None,
        }
    }

    /// Run the full key sequence with fidelity-specific handling.
    ///
    /// For each key:
    /// 1. Set viewport on HostSession
    /// 2. `process_key_host(key)` — processes through HostSession
    /// 3. Take captured effects, accumulate errmsg from ShowError
    /// 4. Handle outstanding host requests via Neovim-compatible handler
    /// 5. Update viewport tracking
    /// 6. Run per-key invariants
    pub fn run_keys(&mut self, keys: &str) {
        let parsed = parse_fidelity_keys(keys);
        for key in parsed {
            let key_repr = format!("{key:?}");

            if let Some(vp) = self.viewport {
                self.session.session_mut().set_viewport(vp);
            }

            let response = self.session.session_mut().process_key_host(key);

            self.complete_host_requests(&response.host_requests);

            // Take effects AFTER completions so ShowError from Failure results is included.
            let effects = self.session.session_mut().take_captured_effects();

            for effect in &effects {
                if let Effect::ShowError { error, .. } = effect {
                    self.errmsg = Some(error.to_string());
                }
            }

            if let Some(ref mut vp) = self.viewport {
                update_viewport_after_key(
                    vp,
                    self.session.text(),
                    self.session.cursor_offset(),
                    key,
                    response.scroll.as_ref(),
                );
            }

            run_per_key_invariants(self.session.session(), &effects, &key_repr);
        }

        self.auto_escape();

        run_per_test_invariants(self.session.session());
    }

    /// Capture the final state as a GoldenState.
    pub fn capture(&self) -> GoldenState {
        let window = self.viewport.map(|vp| WindowState {
            topline: vp.first_line + 1,
            botline: (vp.first_line + vp.height)
                .min(vim_core::commands::helpers::line_count(self.session.text())),
            height: vp.height,
            width: vp.width,
        });

        self.session.capture_golden(window, self.errmsg.clone())
    }

    /// Auto-escape: if in Insert or Replace mode, send Esc to normalize.
    ///
    /// The Neovim golden file captures state after escaping from insert/replace.
    /// Visual and CommandLine modes are NOT escaped.
    fn auto_escape(&mut self) {
        let mode = self.session.mode();
        if matches!(mode, Mode::Insert | Mode::Replace) {
            let esc_key = vim_core::keymap::KeyEvent::escape();

            if let Some(vp) = self.viewport {
                self.session.session_mut().set_viewport(vp);
            }

            let response = self.session.session_mut().process_key_host(esc_key);

            self.complete_host_requests(&response.host_requests);

            let effects = self.session.session_mut().take_captured_effects();

            for effect in &effects {
                if let Effect::ShowError { error, .. } = effect {
                    self.errmsg = Some(error.to_string());
                }
            }

            if let Some(ref mut vp) = self.viewport {
                update_viewport_after_key(
                    vp,
                    self.session.text(),
                    self.session.cursor_offset(),
                    esc_key,
                    response.scroll.as_ref(),
                );
            }
        }
    }

    /// Complete host requests, handling cascading requests from completions.
    fn complete_host_requests(&mut self, requests: &[HostRequest]) {
        let mut pending: Vec<HostRequest> = requests.to_vec();
        while !pending.is_empty() {
            let batch = std::mem::take(&mut pending);
            for request in &batch {
                let result = handle_neovim_request(self.session.session(), request);
                let completion = self.session.session_mut().complete_request_host(&result);
                pending.extend(completion.host_requests);
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// NEOVIM-COMPATIBLE HOST REQUEST HANDLER
// ═══════════════════════════════════════════════════════════════════════════

/// Handle a host request with Neovim-compatible behavior.
///
/// Maps requests to responses matching what Neovim would do in a headless
/// test environment (no file backing, expandtab/shiftwidth=4).
fn handle_neovim_request(session: &HostSession, request: &HostRequest) -> HostResult {
    match request {
        HostRequest::ReadFile {
            meta,
            path,
            after_line,
        } => match std::fs::read_to_string(path.as_str()) {
            Ok(data) => HostResult::Data {
                id: meta.id,
                data: data.into(),
                offset: Some(read_insert_offset(session, *after_line)),
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
            let result = cindent_simulate(session.text(), input_text, range.start().get());
            let leading_newlines = result.bytes().take_while(|&b| b == b'\n').count();
            let has_content_after = leading_newlines < result.len();
            let skip = if has_content_after {
                leading_newlines
            } else {
                0
            };
            let cursor = range.start().get() + skip;

            let mark_dot = {
                let old_lines: Vec<&str> = input_text.lines().collect();
                let new_lines: Vec<&str> = result.lines().collect();
                let mut offset_in_result = 0usize;
                let mut found = None;
                for (old, new) in old_lines.iter().zip(new_lines.iter()) {
                    if old != new {
                        found = Some(range.start().get() + offset_in_result);
                        break;
                    }
                    offset_in_result += new.len() + 1;
                }
                found
            };

            HostResult::FilteredRange {
                id: meta.id,
                replacement: CompactString::from(result),
                cursor_offset: Some(cursor),
                stderr: None,
                mark_dot_offset: mark_dot,
            }
        }
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

// ═══════════════════════════════════════════════════════════════════════════
// CINDENT SIMULATION
// ═══════════════════════════════════════════════════════════════════════════

/// Simulate Neovim's cindent with `expandtab shiftwidth=4 tabstop=4`.
///
/// Matches the oracle settings. Algorithm:
/// - Determine initial brace nesting by scanning document text before range.
/// - Strip existing indentation from every line.
/// - Track brace/paren nesting.
/// - Lines inside braces whose predecessor does not end with `{`, `}`, or `;`
///   receive continuation indent.
fn cindent_simulate(doc_text: &str, input_text: &str, range_start: usize) -> String {
    const SW: usize = 4;
    let has_trailing_newline = input_text.ends_with('\n');
    let lines: Vec<&str> = input_text.lines().collect();
    let mut result = String::with_capacity(input_text.len());

    let prefix = &doc_text[..range_start.min(doc_text.len())];
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

        if trimmed.starts_with('}') {
            brace_nesting = (brace_nesting - 1).max(0);
            prev_is_continuation = false;
        }

        let brace_indent = (brace_nesting as usize) * SW;
        let paren_indent = paren_nesting as usize;
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

        if trimmed.starts_with(')') {
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

    result
}

// ═══════════════════════════════════════════════════════════════════════════
// VIEWPORT TRACKING
// ═══════════════════════════════════════════════════════════════════════════

/// Update viewport tracking after processing a key.
///
/// Three sources of viewport updates (applied in order):
/// 1. **Scroll placement** from `HostResponse.scroll` (zz/zt/zb, z-CR/z./z-)
/// 2. **Key-based scroll deltas** for Ctrl-E/Y/F/B/D/U (pure cursor motions
///    that don't produce scroll effects)
/// 3. **Cursor-visible clamping** ensures cursor stays within viewport
fn update_viewport_after_key(
    vp: &mut ViewportInfo,
    text: &str,
    cursor_offset: usize,
    key: vim_core::keymap::KeyEvent,
    scroll: Option<&ScrollInfo>,
) {
    use vim_core::keymap::{Key, Modifiers};

    let cursor_line = vim_core::commands::helpers::line_of(text, cursor_offset);
    let total_lines = vim_core::commands::helpers::line_count(text);
    let max_first = total_lines.saturating_sub(1);

    // 1. Apply scroll info from HostResponse.
    // ScrollTo (from Ctrl-E/Y/F/B/D/U) uses Visible placement with
    // target_line = new topline. zz/zt/zb use Center/Top/Bottom.
    if let Some(info) = scroll {
        match info.placement {
            ScrollPlacement::Visible => {
                vp.first_line = info.target_line;
            }
            ScrollPlacement::Top => {
                vp.first_line = info.target_line;
            }
            ScrollPlacement::Center => {
                vp.first_line = info.target_line.saturating_sub(vp.height / 2);
            }
            ScrollPlacement::Bottom => {
                vp.first_line = info.target_line.saturating_sub(vp.height.saturating_sub(1));
            }
            _ => {}
        }
    }

    // 3. Ensure cursor is visible within viewport (simple clamping).
    if cursor_line < vp.first_line {
        vp.first_line = cursor_line;
    }
    let last_visible = vp.first_line + vp.height.saturating_sub(1);
    if cursor_line > last_visible {
        vp.first_line = cursor_line.saturating_sub(vp.height.saturating_sub(1));
    }
    if vp.first_line > max_first {
        vp.first_line = max_first;
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// KEY PARSING
// ═══════════════════════════════════════════════════════════════════════════

/// Parse Vim notation keys for fidelity tests.
///
/// Unlike `vim_core::execution::parse_keys_from_string`, this parser treats
/// unrecognized single-character `<X>` notations (like `<T>`) as literal
/// `<`, `X`, `>` characters — matching Neovim's `nvim_replace_termcodes`
/// behavior which does not recognize `<T>` as a key code.
fn parse_fidelity_keys(keys: &str) -> Vec<KeyEvent> {
    use vim_core::keymap::Key;

    let mut result = Vec::new();
    let mut chars = keys.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '<' {
            let mut special = String::new();
            let mut found_close = false;
            let mut lookahead = chars.clone();

            while let Some(&nc) = lookahead.peek() {
                if nc == '>' {
                    lookahead.next();
                    found_close = true;
                    break;
                }
                if special.len() >= 12 {
                    break;
                }
                special.push(lookahead.next().unwrap());
            }

            if found_close {
                let notation = format!("<{special}>");
                if let Some(key) = KeyEvent::from_vim_notation(&notation) {
                    // Reject bare single ASCII letter: <T> → literal <, T, >
                    // Accept everything else: <Esc>, <CR>, <C-a>, <lt>, <F1>, etc.
                    let is_bare_letter =
                        special.len() == 1 && special.as_bytes()[0].is_ascii_alphabetic();
                    if !is_bare_letter {
                        for _ in 0..special.len() + 1 {
                            chars.next();
                        }
                        result.push(key);
                        continue;
                    }
                }
                // Not a recognized special key — treat '<' as literal
                result.push(KeyEvent::char('<'));
            } else {
                result.push(KeyEvent::char('<'));
            }
        } else {
            let key = match c {
                '\x1b' => KeyEvent::escape(),
                '\r' | '\n' => KeyEvent::enter(),
                '\t' => KeyEvent::tab(),
                '\x08' | '\x7f' => KeyEvent::backspace(),
                '\x01'..='\x1a' => {
                    let letter = (b'a' + (c as u8) - 1) as char;
                    KeyEvent::ctrl(letter)
                }
                c => KeyEvent::char(c),
            };
            result.push(key);
        }
    }

    result
}

// ═══════════════════════════════════════════════════════════════════════════
// HELPERS
// ═══════════════════════════════════════════════════════════════════════════

/// Convert a 0-indexed (line, col) pair to a byte offset.
///
/// `col` is treated as a CHARACTER INDEX (matching the Neovim oracle's
/// `vim.str_byteindex()`), not a raw byte offset. For ASCII text these
/// are identical; for multi-byte text the character-to-byte conversion
/// is essential.
fn line_col_to_offset(text: &str, line: usize, col: usize) -> Option<usize> {
    let mut current_line = 0;
    let mut line_start = 0;
    for (i, ch) in text.char_indices() {
        if ch == '\n' {
            if current_line == line {
                break;
            }
            current_line += 1;
            line_start = i + 1;
        }
    }
    if current_line != line && line > 0 {
        return None;
    }

    let line_end = text[line_start..]
        .find('\n')
        .map_or(text.len(), |nl| line_start + nl);
    let line_content = &text[line_start..line_end];
    let char_count = line_content.chars().count();

    let col_byte = if col < char_count {
        line_content
            .char_indices()
            .nth(col)
            .map(|(byte_idx, _)| byte_idx)
            .unwrap_or(0)
    } else if col == char_count && char_count > 0 {
        line_content
            .char_indices()
            .last()
            .map(|(byte_idx, _)| byte_idx)
            .unwrap_or(0)
    } else {
        let max_byte = line_content.len().saturating_sub(1);
        let mut b = col.min(max_byte);
        while b > 0 && !line_content.is_char_boundary(b) {
            b -= 1;
        }
        b
    };

    Some(line_start + col_byte)
}

/// Compute insertion offset for `:r` (ReadFile) requests.
fn read_insert_offset(session: &HostSession, after_line: Option<u32>) -> usize {
    let text = session.text();
    let (cursor_line, _) = {
        let offset = session.cursor_offset().min(text.len());
        let before = &text.as_bytes()[..offset];
        let line = before.iter().filter(|&&b| b == b'\n').count();
        (line, 0)
    };
    let current_line_1 = cursor_line + 1;
    let after = after_line.unwrap_or(current_line_1 as u32) as usize;
    let target = after.saturating_add(1);
    line_start_offset_1_indexed(text, target).unwrap_or(text.len())
}

/// Get byte offset of the start of a 1-indexed line.
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

/// Build annotated text visualization for failure messages.
fn annotate_cursor(text: &str, cursor: usize, anchor: Option<usize>) -> Option<String> {
    match anchor {
        None => {
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
            if cursor > text.len()
                || a > text.len()
                || !text.is_char_boundary(cursor)
                || !text.is_char_boundary(a)
            {
                return None;
            }
            let mut s = String::with_capacity(text.len() + 5);
            if cursor <= a {
                s.push_str(&text[..cursor]);
                s.push_str("#[|");
                s.push_str(&text[cursor..a]);
                s.push_str("]#");
                s.push_str(&text[a..]);
            } else {
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

/// Annotate a GoldenState with cursor/selection markers for error display.
fn try_annotate_golden(state: &GoldenState) -> String {
    let text = state.text.strip_suffix('\n').unwrap_or(&state.text);
    annotate_cursor(text, state.cursor_offset, state.selection_anchor)
        .unwrap_or_else(|| "(annotation unavailable)".to_string())
}

/// Produce an annotated text visualization block for failure messages.
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

// ═══════════════════════════════════════════════════════════════════════════
// PUBLIC ENTRY POINT
// ═══════════════════════════════════════════════════════════════════════════

/// Run a single Neovim fidelity test.
///
/// This is the backing function for the `neovim_test!` macro.
///
/// 1. Creates test input from parameters
/// 2. Loads or generates golden file from Neovim
/// 3. Runs same keys through vim-core via FidelitySession
/// 4. Compares results and panics on mismatch
pub fn run_neovim_test(
    name: &str,
    category: &str,
    text: &str,
    cursor: (usize, usize),
    keys: &str,
    manifest_dir: &str,
) {
    let input = TestInput::new(text, cursor, keys);

    let path = golden_path(manifest_dir, category, name);

    let golden = if should_regenerate_test(name) || !path.exists() {
        match NeovimOracle::capture(&input) {
            Ok((nvim_version, expected)) => {
                let golden = GoldenFile::new(nvim_version, input.clone(), expected);
                if let Err(e) = golden.save(&path) {
                    panic!("Failed to save golden file: {e}");
                }
                eprintln!("[REGEN] Generated golden file: {path:?}");
                golden
            }
            Err(e) => {
                panic!("Failed to capture from Neovim: {e}");
            }
        }
    } else {
        match GoldenFile::load(&path) {
            Ok(golden) => golden,
            Err(e) => {
                panic!("Failed to load golden file {path:?}: {e}");
            }
        }
    };

    let mut session = FidelitySession::new(&input, golden.expected.window.as_ref());
    session.run_keys(&input.keys);
    let actual = session.capture();

    if let Some(diff) = compare_states(&actual, &golden.expected) {
        let viz = annotated_visualization(&input, &golden.expected, &actual);
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
            text = input.text,
            line = input.cursor.0,
            col = input.cursor.1,
            keys = input.keys,
        );
    }
}
