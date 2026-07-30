//! State backend trait for cross-instance state federation.
//!
//! Defines the [`StateBackend`] trait for loading and storing persisted Vim state
//! (registers, global marks, search/command history, macros) across editor instances.
//!
//! Implementations are intentionally NOT required to be `Send + Sync` — the backend
//! is accessed only from the engine thread via `VimEngine`'s federation API.

use crate::primitives::{MarkName, RegisterContent, RegisterName};
use compact_str::CompactString;

/// A saved cross-buffer mark with file context and position.
///
/// Equivalent to Vim's global mark (A-Z) with full persistence metadata.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GlobalMark {
    /// Absolute path to the file containing this mark.
    pub file_path: CompactString,
    /// Byte offset within the file.
    pub offset: usize,
    /// Line number (1-based) at the marked position.
    pub line: usize,
    /// Column number (0-based byte column) at the marked position.
    pub column: usize,
    /// Unix timestamp (seconds since epoch) when the mark was set.
    pub timestamp: u64,
}

/// Pluggable persistence backend for cross-instance state federation.
///
/// Implementations of this trait allow a `VimEngine` to load shared state
/// (registers, global marks, search/command history, macros) from an external
/// store (e.g., a shared file, SQLite, or in-process IPC channel) and to
/// persist state changes back to that store after processing.
///
/// # Design Notes
///
/// - **`Send` bound** — the backend is called synchronously from
///   the engine thread but must be `Send` because `VimEngine` itself is `Send`.
/// - **Infallible load** — load methods return `Option` / `Vec`; errors should
///   be handled internally by the implementation (e.g., log and return `None`).
/// - **Immutable `&self` for stores** — interior mutability (`RefCell`, `Mutex`,
///   file handles, etc.) is the implementation's responsibility.
pub trait StateBackend: Send {
    // ── Load operations ──────────────────────────────────────────────────────

    /// Load the content of a named register from the backend.
    ///
    /// Returns `None` if the register is not present in the store.
    fn load_register(&self, name: RegisterName) -> Option<RegisterContent>;

    /// Load a global mark (A-Z) from the backend.
    ///
    /// Returns `None` if the mark is not present in the store.
    fn load_global_mark(&self, name: MarkName) -> Option<GlobalMark>;

    /// Load the search history ring from the backend.
    ///
    /// Returns an empty `Vec` if no history is stored.
    fn load_search_history(&self) -> Vec<CompactString>;

    /// Load the command-line (`:`) history ring from the backend.
    ///
    /// Returns an empty `Vec` if no history is stored.
    fn load_command_history(&self) -> Vec<CompactString>;

    /// Load a macro from the backend.
    ///
    /// Returns `None` if the macro register is not present in the store.
    fn load_macro(&self, name: RegisterName) -> Option<CompactString>;

    // ── Store operations ─────────────────────────────────────────────────────

    /// Persist the content of a named register to the backend.
    fn store_register(&self, name: RegisterName, content: &RegisterContent);

    /// Persist a global mark (A-Z) to the backend.
    fn store_global_mark(&self, name: MarkName, mark: &GlobalMark);

    /// Persist the search history ring to the backend.
    fn store_search_history(&self, history: &[CompactString]);

    /// Persist the command-line (`:`) history ring to the backend.
    fn store_command_history(&self, history: &[CompactString]);

    /// Persist a macro to the backend.
    fn store_macro(&self, name: RegisterName, content: &str);

    // ── Jump list operations ─────────────────────────────────────────────────

    /// Load the persisted jump list from the backend.
    ///
    /// Returns an empty `Vec` if no jump list is stored.
    fn load_jump_list(&self) -> Vec<FederatedJumpEntry> {
        Vec::new()
    }

    /// Persist the jump list to the backend.
    fn store_jump_list(&self, _entries: &[FederatedJumpEntry]) {}
}

/// A jump list entry with file context for cross-instance federation.
///
/// Unlike [`crate::state::JumpEntry`] (which uses opaque `Offset` + `BufferId`
/// for in-process navigation), this struct carries absolute file paths and
/// line/column info so that entries can be serialized, restored across sessions,
/// and shared between independent editor instances.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FederatedJumpEntry {
    /// Absolute path to the file containing this jump position.
    pub file_path: CompactString,
    /// Byte offset within the file.
    pub offset: usize,
    /// Line number (1-based) at the jump position.
    pub line: usize,
    /// Column number (0-based byte column) at the jump position.
    pub column: usize,
}
