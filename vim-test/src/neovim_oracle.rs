//! Neovim Oracle for generating expected test results.
//!
//! Runs Neovim headless and captures complete state for comparison
//! with vim-core's output. Used by [`crate::fidelity::FidelitySession`]
//! to generate golden files on first run.

use crate::golden::{GoldenError, GoldenState, TestInput};
use std::process::{Command, Stdio};

/// Neovim Oracle — generates expected results by running in Neovim.
pub struct NeovimOracle;

impl NeovimOracle {
    /// Capture the expected state from Neovim.
    ///
    /// Spawns a headless Neovim process, sets initial text/cursor,
    /// feeds the key sequence, and captures the resulting state as JSON.
    /// Returns `(nvim_version, state)`.
    pub fn capture(input: &TestInput) -> Result<(String, GoldenState), GoldenError> {
        let lua_script = Self::generate_lua_script(input);
        let (nvim_version, state) = Self::run_neovim(&lua_script)?;
        Ok((nvim_version, state))
    }

    fn generate_lua_script(input: &TestInput) -> String {
        let escaped_text = escape_for_lua(&input.text);
        let escaped_keys = escape_for_lua(&input.keys);

        format!(
            r#"
-- vim-core Neovim Oracle State Capture Script
-- This script sets up initial state, feeds keys, and captures final state.

-- Disable plugins and set clean state
vim.o.undolevels = -1
vim.cmd('set expandtab shiftwidth=4 tabstop=4')
vim.cmd('set noswapfile')

-- Set initial text via setline() instead of nvim_buf_set_lines().
-- setline() uses u_savesub() which bypasses u_saveline(), keeping the
-- U (undo-line) state clean. nvim_buf_set_lines() triggers u_saveline()
-- which saves the empty pre-setup buffer as the U baseline, causing U
-- to restore to empty instead of the user-visible initial text.
local initial_text = [=[{escaped_text}]=]
local lines = vim.split(initial_text, '\n', {{plain = true}})
vim.fn.setline(1, lines)

-- Re-enable undo
vim.o.undolevels = 1000

-- Set initial cursor (1-based line, 0-based byte col)
-- Convert grapheme/column index to byte offset using Neovim API
local target_line = {cursor_line}
local target_col = {cursor_col}  -- This is character/column index
local line_text = lines[target_line] or ''
-- vim.str_byteindex converts character index to byte index
-- It returns the byte index for the given character index (0-based)
local byte_offset = 0
if target_col > 0 and #line_text > 0 then
    -- vim.str_byteindex(str, char_index) returns byte index
    local ok, result = pcall(vim.str_byteindex, line_text, target_col)
    if ok and result then
        byte_offset = result
    else
        -- Fallback: assume ASCII
        byte_offset = math.min(target_col, #line_text)
    end
end
vim.api.nvim_win_set_cursor(0, {{target_line, byte_offset}})

-- Clear jump list populated by cursor movement during setup
-- This ensures only user-initiated jumps are recorded
vim.cmd('clearjumps')

-- Feed keys
local keys_raw = [=[{escaped_keys}]=]
if keys_raw ~= '' then
    local keys = vim.api.nvim_replace_termcodes(keys_raw, true, true, true)
    vim.api.nvim_feedkeys(keys, 'mtx', false)
    -- Execute pending keys
    vim.cmd('redraw')
end

-- Capture state
local function capture_state()
    local result = {{}}

    -- Text as single string
    local buf_lines = vim.api.nvim_buf_get_lines(0, 0, -1, false)
    result.text = table.concat(buf_lines, '\n')

    -- Cursor
    local cursor = vim.api.nvim_win_get_cursor(0)
    result.cursor_line = cursor[1] - 1  -- 0-indexed
    result.cursor_col = cursor[2]

    -- Calculate byte offset
    local byte_offset = 0
    for i = 1, cursor[1] - 1 do
        byte_offset = byte_offset + #(buf_lines[i] or '') + 1  -- +1 for newline
    end
    byte_offset = byte_offset + cursor[2]
    result.cursor_offset = byte_offset

    -- Mode (complete mapping)
    local mode = vim.api.nvim_get_mode().mode
    local mode_map = {{
        ['n'] = 'Normal',
        ['no'] = 'OperatorPending',
        ['nov'] = 'OperatorPending',
        ['noV'] = 'OperatorPending',
        ['no\22'] = 'OperatorPending',
        ['niI'] = 'Normal',
        ['niR'] = 'Normal',
        ['niV'] = 'Normal',
        ['i'] = 'Insert',
        ['ic'] = 'Insert',
        ['ix'] = 'Insert',
        ['R'] = 'Replace',
        ['Rc'] = 'Replace',
        ['Rv'] = 'Replace',
        ['Rx'] = 'Replace',
        ['v'] = 'Visual',
        ['V'] = 'VisualLine',
        ['\22'] = 'VisualBlock',
        ['s'] = 'Select',
        ['S'] = 'SelectLine',
        ['\19'] = 'SelectBlock',
        ['c'] = 'CommandLine',
        ['cv'] = 'VimEx',
        ['ce'] = 'NormalEx',
        ['r'] = 'HitEnter',
        ['rm'] = 'More',
        ['r?'] = 'Confirm',
        ['!'] = 'Shell',
        ['t'] = 'Terminal',
    }}
    result.mode = mode_map[mode] or mode

    -- Visual type (if in visual mode)
    if mode == 'v' then
        result.visual_type = 'Char'
    elseif mode == 'V' then
        result.visual_type = 'Line'
    elseif mode == '\22' then
        result.visual_type = 'Block'
    end

    -- Selection anchor (if visual)
    if mode:match('[vV\22]') then
        local vis_start = vim.fn.getpos('v')
        local anchor_offset = 0
        for i = 1, vis_start[2] - 1 do
            anchor_offset = anchor_offset + #(buf_lines[i] or '') + 1
        end
        anchor_offset = anchor_offset + vis_start[3] - 1
        result.selection_anchor = anchor_offset
    end

    -- Registers (important ones)
    local registers = vim.empty_dict()
    local reg_names = {{'"', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
                       'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j',
                       '-', '+', '*', '/', ':'}}
    for _, reg in ipairs(reg_names) do
        local content = vim.fn.getreg(reg)
        if content ~= '' then
            registers[reg] = {{
                text = content,
                regtype = vim.fn.getregtype(reg)
            }}
        end
    end
    result.registers = registers

    -- Marks
    local marks = vim.empty_dict()
    -- a-z marks
    for i = 97, 122 do
        local mark = string.char(i)
        local pos = vim.fn.getpos("'" .. mark)
        if pos[2] > 0 then
            local mark_offset = 0
            for j = 1, pos[2] - 1 do
                mark_offset = mark_offset + #(buf_lines[j] or '') + 1
            end
            mark_offset = mark_offset + pos[3] - 1
            marks[mark] = mark_offset
        end
    end
    -- Special marks
    for _, mark in ipairs({{'[', ']', '<', '>', '.', '^'}}) do
        local pos = vim.fn.getpos("'" .. mark)
        if pos[2] > 0 then
            local mark_offset = 0
            for j = 1, pos[2] - 1 do
                mark_offset = mark_offset + #(buf_lines[j] or '') + 1
            end
            mark_offset = mark_offset + pos[3] - 1
            marks[mark] = mark_offset
        end
    end
    result.marks = marks

    -- Search
    local search_reg = vim.fn.getreg('/')
    if search_reg ~= '' then
        result.search_pattern = search_reg
        result.search_direction = vim.v.searchforward == 1 and 'Forward' or 'Backward'
    end

    -- Window state (for scroll verification)
    result.window = {{
        topline = vim.fn.line('w0'),
        botline = vim.fn.line('w$'),
        height = vim.fn.winheight(0),
        width = vim.fn.winwidth(0),
    }}

    -- Error/message state
    local errmsg = vim.v.errmsg
    if errmsg ~= '' then
        result.errmsg = errmsg
    end

    -- Curswant (virtual column for j/k movement)
    local view = vim.fn.winsaveview()
    result.curswant = view.curswant

    -- Jump list
    local jl_result = vim.fn.getjumplist()
    local jl_entries = jl_result[1]
    local jl_idx = jl_result[2]
    if #jl_entries > 0 then
        local jl_offsets = {{}}
        for _, entry in ipairs(jl_entries) do
            local jl_offset = 0
            for i = 1, entry.lnum - 1 do
                jl_offset = jl_offset + #(buf_lines[i] or '') + 1
            end
            jl_offset = jl_offset + entry.col
            table.insert(jl_offsets, jl_offset)
        end
        result.jumplist = jl_offsets
        result.jumplist_idx = jl_idx
    end

    -- Change list
    local cl_result = vim.fn.getchangelist()
    local cl_entries = cl_result[1]
    local cl_idx = cl_result[2]
    if #cl_entries > 0 then
        local cl_offsets = {{}}
        for _, entry in ipairs(cl_entries) do
            local cl_offset = 0
            for i = 1, entry.lnum - 1 do
                cl_offset = cl_offset + #(buf_lines[i] or '') + 1
            end
            cl_offset = cl_offset + entry.col
            table.insert(cl_offsets, cl_offset)
        end
        result.changelist = cl_offsets
        result.changelist_idx = cl_idx
    end

    return result
end

-- Output result as JSON
local result = capture_state()
local nvim_version = vim.version()
local version_str = string.format('%d.%d.%d', nvim_version.major, nvim_version.minor, nvim_version.patch)

io.stdout:write('NVIM_VERSION:' .. version_str .. '\n')
io.stdout:write('STATE_JSON:' .. vim.json.encode(result) .. '\n')
"#,
            escaped_text = escaped_text,
            escaped_keys = escaped_keys,
            cursor_line = input.cursor.0 + 1, // Lua is 1-indexed
            cursor_col = input.cursor.1,
        )
    }

    fn run_neovim(lua_script: &str) -> Result<(String, GoldenState), GoldenError> {
        let temp_dir = std::env::temp_dir();
        let thread_id = std::thread::current().id();
        let script_path = temp_dir.join(format!(
            "vim_core_oracle_{:?}_{}.lua",
            thread_id,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        std::fs::write(&script_path, lua_script)?;

        let mut child = Command::new("nvim")
            .args([
                "--headless",
                "-u",
                "NONE",
                "-n",
                "-l",
                script_path.to_str().unwrap(),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| GoldenError::Neovim(format!("Failed to spawn nvim: {e}")))?;

        let timeout = std::time::Duration::from_secs(5);
        let start = std::time::Instant::now();

        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let output = child
                        .wait_with_output()
                        .map_err(|e| GoldenError::Neovim(format!("Failed to get output: {e}")))?;

                    let _ = std::fs::remove_file(&script_path);

                    if !status.success() {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        return Err(GoldenError::Neovim(format!(
                            "Neovim exited with error: {stderr}"
                        )));
                    }

                    let stdout = String::from_utf8_lossy(&output.stdout);
                    return Self::parse_output(&stdout);
                }
                Ok(None) => {
                    if start.elapsed() > timeout {
                        let _ = child.kill();
                        let _ = child.wait();
                        let _ = std::fs::remove_file(&script_path);
                        return Err(GoldenError::Neovim(
                            "Neovim timed out after 5 seconds (possible infinite loop)".to_string(),
                        ));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&script_path);
                    return Err(GoldenError::Neovim(format!("Error waiting for nvim: {e}")));
                }
            }
        }
    }

    fn parse_output(output: &str) -> Result<(String, GoldenState), GoldenError> {
        let mut nvim_version = String::new();
        let mut state_json = String::new();

        for line in output.lines() {
            if let Some(version) = line.strip_prefix("NVIM_VERSION:") {
                nvim_version = version.to_string();
            } else if let Some(json) = line.strip_prefix("STATE_JSON:") {
                state_json = json.to_string();
            }
        }

        if state_json.is_empty() {
            return Err(GoldenError::Neovim(format!(
                "No state JSON in output. Full output:\n{output}"
            )));
        }

        let state: GoldenState = serde_json::from_str(&state_json)?;

        Ok((nvim_version, state))
    }
}

fn escape_for_lua(s: &str) -> String {
    let mut result = if s.contains("]=]") {
        if s.contains("]==]") {
            s.replace("]==]", "]==\"] .. [[]=]")
        } else {
            s.replace("]=]", "]=\"] .. [[]=]")
        }
    } else {
        s.to_string()
    };

    // Lua long strings [=[ ]=] strip the first newline.
    if result.starts_with('\n') {
        result.insert(0, '\n');
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_for_lua() {
        assert_eq!(escape_for_lua("hello"), "hello");
        assert_eq!(escape_for_lua("hello\nworld"), "hello\nworld");
    }

    #[test]
    #[ignore] // Requires nvim to be installed
    fn test_neovim_oracle_basic() {
        let input = TestInput::new("hello world", (0, 0), "l");
        let result = NeovimOracle::capture(&input);

        match result {
            Ok((version, state)) => {
                println!("Neovim version: {version}");
                println!("State: {state:?}");
                assert_eq!(state.cursor_col, 1);
            }
            Err(e) => {
                println!("Oracle error (nvim may not be installed): {e}");
            }
        }
    }
}
