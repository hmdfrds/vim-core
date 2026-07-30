//! Neovim oracle for generating ground-truth match results.
//!
//! Runs `nvim --headless` with a VimScript snippet that uses `matchstrpos()`
//! to find all matches of a pattern in a given input string.
//!
//! This module is used offline for corpus regeneration, not in CI.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use super::corpus::{MatchRange, TestCase};

/// The Neovim version used for oracle generation. Recorded in corpus metadata.
const ORACLE_VERSION: &str = "neovim-oracle-v0.10.0";

/// Monotonic counter ensuring each temp script file gets a unique name within
/// this process (combined with the process id to stay unique across processes).
static SCRIPT_SEQ: AtomicU64 = AtomicU64::new(0);

/// stdout/stderr captured from running a VimScript through Neovim.
struct NvimRun {
    stdout: String,
    stderr: String,
}

/// RAII guard that removes a temp file when dropped, so the script file is
/// cleaned up even if a panic unwinds through the caller.
struct TempScript {
    path: PathBuf,
}

impl Drop for TempScript {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Run a VimScript reliably and return its captured stdout/stderr.
///
/// The script is written to a unique temp file and sourced via
/// `nvim --headless -u NONE -i NONE -S <file>`. Passing a multi-line script as
/// a single `-c` argument does NOT work: Neovim treats it as one ex command
/// line (newlines are not statement separators there), aborting with
/// `E461` and then hanging headless waiting on input. Sourcing a file runs the
/// script as proper line-separated statements. The temp file is removed via an
/// RAII guard, covering the panic path too. Offline only.
fn run_nvim_script(script: &str) -> NvimRun {
    let path = std::env::temp_dir().join(format!(
        "vim_regex_oracle_{}_{}.vim",
        std::process::id(),
        SCRIPT_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, script).expect("failed to write nvim oracle script");
    let _guard = TempScript { path: path.clone() };

    let output = Command::new("nvim")
        .args(["--headless", "-u", "NONE", "-i", "NONE", "-S"])
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("Failed to run nvim. Is nvim installed?");

    NvimRun {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// Result from a single oracle query.
#[derive(Debug)]
pub struct OracleResult {
    pub matches: Vec<MatchRange>,
    pub error: Option<String>,
}

/// Query Neovim for all matches of `pattern` in `input` using `magic_mode`.
///
/// Returns the list of match byte ranges, or an error if the pattern is invalid
/// in Neovim.
///
/// # Panics
///
/// Panics if `nvim` is not found or returns unparseable output.
#[allow(dead_code)]
pub fn query_neovim(pattern: &str, input: &str, magic_mode: &str) -> OracleResult {
    // Build the magic prefix for the pattern.
    let magic_prefix = match magic_mode {
        "magic" => "\\m",
        "nomagic" => "\\M",
        "very_magic" => "\\v",
        "very_nomagic" => "\\V",
        _ => "\\m",
    };

    // Escape single quotes in pattern and input for VimScript.
    let escaped_pattern = pattern.replace('\'', "''");
    let escaped_input = input.replace('\'', "''");

    let full_pattern = format!("{magic_prefix}{escaped_pattern}");

    // VimScript that finds all matches and prints JSON-formatted results.
    let script = format!(
        r#"
let s:input = '{escaped_input}'
let s:pattern = '{full_pattern}'
let s:matches = []
let s:pos = 0
let s:max_iter = 1000
let s:iter = 0

while s:iter < s:max_iter
    let s:iter += 1
    let [s:str, s:start, s:end] = matchstrpos(s:input, s:pattern, s:pos)
    if s:start == -1
        break
    endif
    call add(s:matches, {{'start': s:start, 'end': s:end}})
    if s:end <= s:pos
        let s:pos = s:pos + 1
        if s:pos >= len(s:input)
            break
        endif
    else
        let s:pos = s:end
    endif
endwhile

" Output as JSON
let s:json = json_encode(s:matches)
call writefile([s:json], '/dev/stdout')
qall!
"#
    );

    let run = run_nvim_script(&script);
    let stdout = run.stdout;
    let stderr = run.stderr;

    // Check for Neovim errors.
    if !stderr.is_empty() && stderr.contains("E") {
        return OracleResult {
            matches: vec![],
            error: Some(stderr.to_string()),
        };
    }

    // Parse JSON output.
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return OracleResult {
            matches: vec![],
            error: None,
        };
    }

    let matches: Vec<MatchRange> = match serde_json::from_str(trimmed) {
        Ok(m) => m,
        Err(e) => {
            return OracleResult {
                matches: vec![],
                error: Some(format!("JSON parse error: {e}\nstdout: {stdout}")),
            };
        }
    };

    OracleResult {
        matches,
        error: None,
    }
}

/// Generate a `TestCase` by querying the Neovim oracle.
#[allow(dead_code)]
pub fn generate_test_case(pattern: &str, input: &str, magic_mode: &str) -> Option<TestCase> {
    let result = query_neovim(pattern, input, magic_mode);

    if result.error.is_some() {
        return None;
    }

    Some(TestCase {
        pattern: pattern.to_string(),
        magic_mode: magic_mode.to_string(),
        input: input.to_string(),
        expected_matches: result.matches,
        expected_captures: Vec::new(),
        source: ORACLE_VERSION.to_string(),
    })
}

/// Regenerate an entire corpus category by querying Neovim for each test case.
///
/// Used offline when patterns or inputs change. The resulting corpus is
/// committed to version control.
#[allow(dead_code)]
pub fn regenerate_corpus(cases: &[TestCase]) -> Vec<TestCase> {
    cases
        .iter()
        .filter_map(|tc| {
            let result = query_neovim(&tc.pattern, &tc.input, &tc.magic_mode);
            if result.error.is_some() {
                eprintln!(
                    "WARN: Oracle error for pattern {:?}: {:?}",
                    tc.pattern, result.error
                );
                return None;
            }
            Some(TestCase {
                pattern: tc.pattern.clone(),
                magic_mode: tc.magic_mode.clone(),
                input: tc.input.clone(),
                expected_matches: result.matches,
                expected_captures: Vec::new(),
                source: ORACLE_VERSION.to_string(),
            })
        })
        .collect()
}

/// Query Neovim for the submatch strings of each successive match of `pattern`
/// in `input`, using `matchlist()`. Returns, per match, groups 1..=9 as
/// `Option<String>`.
///
/// NOTE: `matchlist` returns `""` for both an empty match and a non-participating
/// group, so this helper reports `Some("")` in both cases. Participation (`None`)
/// for the curated corpus is set by hand from known Vim semantics; the engine-level
/// capture cross-check (test_builder invariant #18) is the authority for
/// participation across the broader proptest surface. Offline only.
#[allow(dead_code)]
pub fn query_neovim_captures(
    pattern: &str,
    input: &str,
    magic_mode: &str,
) -> Vec<Vec<Option<String>>> {
    let magic_prefix = match magic_mode {
        "magic" => "\\m",
        "nomagic" => "\\M",
        "very_magic" => "\\v",
        "very_nomagic" => "\\V",
        _ => "\\m",
    };
    let escaped_pattern = pattern.replace('\'', "''");
    let escaped_input = input.replace('\'', "''");
    let full_pattern = format!("{magic_prefix}{escaped_pattern}");

    let script = format!(
        r#"
let s:input = '{escaped_input}'
let s:pattern = '{full_pattern}'
let s:out = []
let s:pos = 0
let s:iter = 0
while s:iter < 1000
    let s:iter += 1
    let [s:mstr, s:start, s:end] = matchstrpos(s:input, s:pattern, s:pos)
    if s:start == -1
        break
    endif
    let s:groups = matchlist(s:input, s:pattern, s:pos)
    call add(s:out, s:groups[1:9])
    if s:end == s:start
        let s:pos = s:end + 1
    else
        let s:pos = s:end
    endif
endwhile
call writefile([json_encode(s:out)], '/dev/stdout')
qa!
"#
    );

    let run = run_nvim_script(&script);
    let stdout = run.stdout;
    let line = stdout
        .lines()
        .find(|l| l.trim_start().starts_with('['))
        .unwrap_or("[]");
    let raw: Vec<Vec<String>> = serde_json::from_str(line.trim()).expect("oracle JSON parse");
    raw.into_iter()
        .map(|groups| groups.into_iter().map(Some).collect())
        .collect()
}
