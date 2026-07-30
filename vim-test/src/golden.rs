//! Golden file management for fidelity testing.
//!
//! Types and I/O for golden files that store expected Neovim output.
//! Used by [`crate::fidelity::FidelitySession`] to compare vim-core
//! behavior against real Neovim.

use serde::{Deserialize, Serialize};
use smart_default::SmartDefault;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Current schema version for golden files.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// A golden file containing expected test results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoldenFile {
    /// Schema version for future migration support.
    #[serde(default)]
    pub schema_version: u32,
    /// Neovim version used to generate this file.
    pub nvim_version: String,
    /// When this file was generated (Unix timestamp).
    pub generated_at: String,
    /// Test input configuration.
    pub input: TestInput,
    /// Expected output from Neovim.
    pub expected: GoldenState,
}

/// Test input configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestInput {
    /// Initial document text.
    pub text: String,
    /// Initial cursor position (line, col) — 0-indexed.
    pub cursor: (usize, usize),
    /// Initial mode (usually "Normal").
    pub mode: String,
    /// Key sequence in Vim notation.
    pub keys: String,
}

/// Complete state snapshot from Neovim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, SmartDefault)]
pub struct GoldenState {
    /// Full document text.
    #[default(String::new())]
    pub text: String,

    /// Cursor byte offset.
    #[default = 0]
    pub cursor_offset: usize,
    /// Cursor 0-indexed line.
    #[default = 0]
    pub cursor_line: usize,
    /// Cursor 0-indexed column.
    #[default = 0]
    pub cursor_col: usize,

    /// Current mode.
    #[default = "Normal"]
    pub mode: String,
    /// Visual mode subtype if in visual mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visual_type: Option<String>,

    /// Selection anchor offset (where visual started).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection_anchor: Option<usize>,

    /// Register contents.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    #[default(HashMap::new())]
    pub registers: HashMap<String, RegisterSnapshot>,

    /// Mark positions (byte offset).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    #[default(HashMap::new())]
    pub marks: HashMap<String, usize>,

    /// Last search pattern.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_pattern: Option<String>,
    /// Search direction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_direction: Option<String>,

    /// Window state (topline, dimensions).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowState>,

    /// Virtual column for j/k movement (curswant).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub curswant: Option<usize>,

    /// Jump list positions (byte offsets), oldest first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jumplist: Option<Vec<usize>>,
    /// Current position in the jump list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jumplist_idx: Option<usize>,

    /// Change list positions (byte offsets), oldest first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changelist: Option<Vec<usize>>,
    /// Current position in the change list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changelist_idx: Option<usize>,

    /// Error message if command failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errmsg: Option<String>,
}

/// Window state for scroll verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowState {
    /// First visible line (1-indexed).
    pub topline: usize,
    /// Last visible line (1-indexed).
    pub botline: usize,
    /// Window height in lines.
    pub height: usize,
    /// Window width in columns.
    pub width: usize,
}

/// Snapshot of register content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterSnapshot {
    /// Text content.
    pub text: String,
    /// Register type: `"v"` (char), `"V"` (line), or `"\x16"` (block).
    pub regtype: String,
}

impl GoldenFile {
    /// Create a new golden file.
    pub fn new(nvim_version: String, input: TestInput, expected: GoldenState) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            nvim_version,
            generated_at: chrono_lite_now(),
            input,
            expected,
        }
    }

    /// Load a golden file from disk.
    pub fn load(path: &Path) -> Result<Self, GoldenError> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let golden: Self = serde_json::from_reader(reader)?;
        if golden.schema_version > CURRENT_SCHEMA_VERSION {
            eprintln!(
                "[WARN] Golden file {:?} has schema_version {} (current: {})",
                path, golden.schema_version, CURRENT_SCHEMA_VERSION
            );
        }
        Ok(golden)
    }

    /// Save this golden file to disk.
    pub fn save(&self, path: &Path) -> Result<(), GoldenError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = File::create(path)?;
        let writer = BufWriter::new(file);
        Ok(serde_json::to_writer_pretty(writer, self)?)
    }
}

impl TestInput {
    /// Create a new test input.
    pub fn new(text: impl Into<String>, cursor: (usize, usize), keys: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            cursor,
            mode: "Normal".to_string(),
            keys: keys.into(),
        }
    }
}

/// Error type for golden file operations.
#[derive(Error, Debug)]
pub enum GoldenError {
    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON parse error.
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    /// Neovim error.
    #[error("Neovim error: {0}")]
    Neovim(String),
}

/// Get the path to a golden file.
///
/// `manifest_dir` is the `CARGO_MANIFEST_DIR` of the crate that owns the
/// golden files (e.g., vim-core). Callers pass `env!("CARGO_MANIFEST_DIR")`
/// which expands at their call site.
pub fn golden_path(manifest_dir: &str, category: &str, test_name: &str) -> PathBuf {
    PathBuf::from(manifest_dir)
        .join("tests")
        .join("golden")
        .join(category)
        .join(format!("{test_name}.json"))
}

/// Check if we should regenerate golden files.
pub fn should_regenerate() -> bool {
    std::env::var("REGEN").is_ok() || std::env::var("REGEN_ALL").is_ok()
}

/// Check if a specific test should be regenerated.
pub fn should_regenerate_test(test_name: &str) -> bool {
    if should_regenerate() {
        return true;
    }
    if let Ok(name) = std::env::var("REGEN_TEST") {
        return name == test_name;
    }
    false
}

fn chrono_lite_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", duration.as_secs())
}
