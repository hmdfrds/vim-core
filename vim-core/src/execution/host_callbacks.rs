//! Callback-based host request handling for `VimSession`.
//!
//! Instead of manually inspecting `HostResponse::host_requests()` and
//! constructing the correct `HostResult` variant for each request type,
//! integrators implement [`HostCallbacks`] and call
//! [`VimSession::process_key_with_callbacks()`].
//!
//! # Minimal integration
//!
//! Only 3 methods are required — everything else has safe defaults:
//!
//! ```ignore
//! struct MyHost { filepath: String, should_quit: bool }
//!
//! impl HostCallbacks for MyHost {
//!     fn write_file(&mut self, path: &str, text: &str) -> Result<(), String> {
//!         std::fs::write(path, text).map_err(|e| e.to_string())
//!     }
//!     fn read_file(&self, path: &str) -> Result<String, String> {
//!         std::fs::read_to_string(path).map_err(|e| e.to_string())
//!     }
//!     fn quit(&mut self, _force: bool) {
//!         self.should_quit = true;
//!     }
//! }
//! ```

/// Callback trait for handling host requests from `VimSession`.
///
/// Required methods represent operations that every host must support.
/// Optional methods have safe defaults (typically failing with "not supported").
///
/// Return types are plain Rust types — the library constructs the correct
/// `HostResult` variant internally.
pub trait HostCallbacks {
    // ── Required ─────────────────────────────────────────────────────────

    /// Write buffer text to the given path.
    ///
    /// Called on `:w [path]`. The `path` is the engine-supplied path
    /// (or the host's default if none was given). The `text` is the full
    /// document content.
    ///
    /// # Errors
    ///
    /// Returns a host-defined error message string when the write fails
    /// (permission denied, disk full, invalid path, etc.).
    fn write_file(&mut self, path: &str, text: &str) -> Result<(), String>;

    /// Read a file and return its contents.
    ///
    /// Called on `:r path` and `:source path`.
    ///
    /// # Errors
    ///
    /// Returns a host-defined error message string when the read fails
    /// (file not found, permission denied, decode error, etc.).
    fn read_file(&self, path: &str) -> Result<String, String>;

    /// Handle quit request.
    ///
    /// Called on `:q`, `:q!`, `:wq`, etc. The `force` flag indicates
    /// whether `!` was used.
    fn quit(&mut self, force: bool);

    // ── Optional (safe defaults) ─────────────────────────────────────────

    /// Read the system clipboard. Returns empty string by default.
    fn read_clipboard(&self) -> String {
        String::new()
    }

    /// Write to the system clipboard. No-op by default.
    fn write_clipboard(&mut self, _text: &str) {}

    /// Evaluate a Vim expression (for `<C-R>=` and `<expr>` mappings).
    /// Default: fails with "not supported".
    ///
    /// # Errors
    ///
    /// Returns the host's evaluation error message; the default
    /// implementation always returns `"Expression evaluation not supported"`.
    fn eval_expression(&self, _expr: &str) -> Result<String, String> {
        Err("Expression evaluation not supported".into())
    }

    /// Run a shell command and return its output.
    /// Default: fails with "not supported".
    ///
    /// # Errors
    ///
    /// Returns the host's shell error message; the default implementation
    /// always returns `"Shell commands not supported"`.
    fn run_shell_command(&self, _cmd: &str) -> Result<String, String> {
        Err("Shell commands not supported".into())
    }

    /// Filter text through a shell command.
    /// Default: fails with "not supported".
    ///
    /// # Errors
    ///
    /// Returns the host's filter error message; the default implementation
    /// always returns `"Filter not supported"`.
    fn filter_text(&self, _cmd: &str, _input: &str) -> Result<String, String> {
        Err("Filter not supported".into())
    }

    /// Reindent text. Default: fails with "not supported".
    ///
    /// # Errors
    ///
    /// Returns the host's reindent error message; the default implementation
    /// always returns `"Reindent not supported"`.
    fn reindent(&self, _input: &str) -> Result<String, String> {
        Err("Reindent not supported".into())
    }

    /// Handle a custom ex command. Default: fails with standard Vim error.
    ///
    /// # Errors
    ///
    /// Returns a host-defined error message; the default implementation
    /// returns the standard `E492: Not an editor command` Vim error.
    fn custom_ex_command(&mut self, cmd: &str) -> Result<Option<String>, String> {
        Err(format!("E492: Not an editor command: {cmd}"))
    }

    /// Open/edit a file (`:e path`). Default: fails with "not supported".
    ///
    /// # Errors
    ///
    /// Returns a host-defined error message; the default implementation
    /// always returns `":edit not supported in this editor"`.
    fn edit_file(&mut self, _path: &str, _force: bool) -> Result<(), String> {
        Err(":edit not supported in this editor".into())
    }
}
