//! Host boundary contracts.
//!
//! Core execution stays pure by emitting [`HostRequest`] values for shell-only
//! operations. The host performs I/O and returns [`HostResult`] back to core.

use crate::primitives::{BufferId, CompletionKind, RegisterName};
use crate::state::{CommandLinePrompt, MessageEntry};
use crate::{primitives::MotionType, primitives::Offset, primitives::Range};
use compact_str::CompactString;

/// What category of completion candidates the host should provide
/// for command-line tab completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CmdlineCompletionKind {
    /// File path completion (`:e`, `:w`, `:r`, etc.).
    FilePath,
    /// Buffer name completion (`:b`, `:sb`, etc.).
    Buffer,
    /// Host action name completion (`:action`).
    Action,
}

/// Direction for window splits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SplitDirection {
    /// Horizontal split (`:split`, `:new`).
    Horizontal,
    /// Vertical split (`:vsplit`, `:vnew`).
    Vertical,
}

/// Stable discriminator for [`HostRequest`] variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum HostRequestKind {
    /// [`HostRequest::WriteFile`]
    WriteFile,
    /// [`HostRequest::Quit`]
    Quit,
    /// [`HostRequest::WriteQuit`]
    WriteQuit,
    /// [`HostRequest::EditFile`]
    EditFile,
    /// [`HostRequest::ReadFile`]
    ReadFile,
    /// [`HostRequest::FilterDocumentRange`]
    FilterDocumentRange,
    /// [`HostRequest::ReadClipboard`]
    ReadClipboard,
    /// [`HostRequest::ExternalCommand`]
    ExternalCommand,
    /// [`HostRequest::CustomExCommand`]
    CustomExCommand,
    /// [`HostRequest::SyncCommandLine`]
    SyncCommandLine,
    /// [`HostRequest::ReindentRange`]
    ReindentRange,
    /// [`HostRequest::ListActions`]
    ListActions,
    /// [`HostRequest::ReadConfigFile`]
    ReadConfigFile,
    /// [`HostRequest::SwitchBuffer`]
    SwitchBuffer,
    /// [`HostRequest::BufferNext`]
    BufferNext,
    /// [`HostRequest::BufferPrev`]
    BufferPrev,
    /// [`HostRequest::BufferFirst`]
    BufferFirst,
    /// [`HostRequest::BufferLast`]
    BufferLast,
    /// [`HostRequest::BufferList`]
    BufferList,
    /// [`HostRequest::TabNew`]
    TabNew,
    /// [`HostRequest::TabNext`]
    TabNext,
    /// [`HostRequest::TabPrev`]
    TabPrev,
    /// [`HostRequest::TabClose`]
    TabClose,
    /// [`HostRequest::DiagnosticNext`]
    DiagnosticNext,
    /// [`HostRequest::DiagnosticPrev`]
    DiagnosticPrev,
    /// [`HostRequest::DiagnosticList`]
    DiagnosticList,
    /// [`HostRequest::DiagnosticGoto`]
    DiagnosticGoto,
    /// [`HostRequest::EvaluateExpression`]
    EvaluateExpression,
    /// [`HostRequest::RequestCompletion`]
    RequestCompletion,
    /// [`HostRequest::ShowMessageHistory`]
    ShowMessageHistory,
    /// [`HostRequest::EvaluateMapping`]
    EvaluateMapping,
    /// [`HostRequest::JumpToBuffer`]
    JumpToBuffer,
    /// [`HostRequest::JumpToGlobalMark`]
    JumpToGlobalMark,
    /// [`HostRequest::RequestCmdlineCompletion`]
    RequestCmdlineCompletion,
    /// [`HostRequest::RunAction`]
    RunAction,
    /// [`HostRequest::SplitWindow`]
    SplitWindow,
    /// [`HostRequest::CloseWindow`]
    CloseWindow,
    /// [`HostRequest::CloseOtherWindows`]
    CloseOtherWindows,
    /// [`HostRequest::WriteAll`]
    WriteAll,
    /// [`HostRequest::QuitAll`]
    QuitAll,
    /// [`HostRequest::WriteQuitAll`]
    WriteQuitAll,
    /// [`HostRequest::CloseBuffer`]
    CloseBuffer,
    /// [`HostRequest::WindowNext`]
    WindowNext,
    /// [`HostRequest::WindowPrev`]
    WindowPrev,
    /// [`HostRequest::WindowMoveLeft`]
    WindowMoveLeft,
    /// [`HostRequest::WindowMoveRight`]
    WindowMoveRight,
    /// [`HostRequest::WindowMoveUp`]
    WindowMoveUp,
    /// [`HostRequest::WindowMoveDown`]
    WindowMoveDown,
    /// [`HostRequest::WindowRotateDown`]
    WindowRotateDown,
    /// [`HostRequest::WindowRotateUp`]
    WindowRotateUp,
    /// [`HostRequest::WindowEqualSize`]
    WindowEqualSize,
    /// [`HostRequest::WindowIncreaseHeight`]
    WindowIncreaseHeight,
    /// [`HostRequest::WindowDecreaseHeight`]
    WindowDecreaseHeight,
    /// [`HostRequest::WindowIncreaseWidth`]
    WindowIncreaseWidth,
    /// [`HostRequest::WindowDecreaseWidth`]
    WindowDecreaseWidth,
    /// [`HostRequest::GotoDefinition`]
    GotoDefinition,
    /// [`HostRequest::ShowDocumentation`]
    ShowDocumentation,
    /// [`HostRequest::OpenCommandWindow`]
    OpenCommandWindow,
    /// [`HostRequest::CallOperatorFunc`]
    CallOperatorFunc,
    /// [`HostRequest::ExecuteNorm`]
    ExecuteNorm,
    /// [`HostRequest::CQuit`]
    CQuit,
    /// [`HostRequest::UpdateFile`]
    UpdateFile,
    /// [`HostRequest::FoldRange`]
    FoldRange,
    /// [`HostRequest::FoldOpenRange`]
    FoldOpenRange,
    /// [`HostRequest::FoldCloseRange`]
    FoldCloseRange,
    /// [`HostRequest::ForEachWindow`]
    ForEachWindow,
    /// [`HostRequest::ForEachBuffer`]
    ForEachBuffer,
    /// [`HostRequest::ForEachTab`]
    ForEachTab,
    /// [`HostRequest::MkVimrc`]
    MkVimrc,
}

impl HostRequestKind {
    /// Returns `true` if this request kind requires a trusted workspace.
    ///
    /// These requests can execute arbitrary code, access the file system, or
    /// otherwise affect the host machine. They must be blocked when the
    /// workspace is not trusted (e.g., a restricted-workspace mode).
    #[must_use]
    pub const fn requires_trust(self) -> bool {
        matches!(
            self,
            Self::ExternalCommand
                | Self::FilterDocumentRange
                | Self::CustomExCommand
                | Self::ReadFile
                | Self::EditFile
                | Self::RunAction
                | Self::EvaluateExpression
        )
    }

    /// Returns `true` if this request kind requires a desktop platform.
    ///
    /// These requests depend on OS-level capabilities (child processes, file I/O)
    /// that are unavailable in browser-based hosts.
    #[must_use]
    pub const fn requires_desktop(self) -> bool {
        matches!(self, Self::ExternalCommand | Self::FilterDocumentRange)
    }

    /// Ordered list of all currently defined host request kinds.
    pub const ALL: [Self; 66] = [
        Self::WriteFile,
        Self::Quit,
        Self::WriteQuit,
        Self::EditFile,
        Self::ReadFile,
        Self::FilterDocumentRange,
        Self::ReadClipboard,
        Self::ExternalCommand,
        Self::CustomExCommand,
        Self::SyncCommandLine,
        Self::ReindentRange,
        Self::ListActions,
        Self::ReadConfigFile,
        Self::SwitchBuffer,
        Self::BufferNext,
        Self::BufferPrev,
        Self::BufferFirst,
        Self::BufferLast,
        Self::BufferList,
        Self::TabNew,
        Self::TabNext,
        Self::TabPrev,
        Self::TabClose,
        Self::DiagnosticNext,
        Self::DiagnosticPrev,
        Self::DiagnosticList,
        Self::DiagnosticGoto,
        Self::EvaluateExpression,
        Self::RequestCompletion,
        Self::ShowMessageHistory,
        Self::EvaluateMapping,
        Self::JumpToBuffer,
        Self::JumpToGlobalMark,
        Self::RequestCmdlineCompletion,
        Self::RunAction,
        Self::SplitWindow,
        Self::CloseWindow,
        Self::CloseOtherWindows,
        Self::WriteAll,
        Self::QuitAll,
        Self::WriteQuitAll,
        Self::CloseBuffer,
        Self::WindowNext,
        Self::WindowPrev,
        Self::WindowMoveLeft,
        Self::WindowMoveRight,
        Self::WindowMoveUp,
        Self::WindowMoveDown,
        Self::WindowRotateDown,
        Self::WindowRotateUp,
        Self::WindowEqualSize,
        Self::WindowIncreaseHeight,
        Self::WindowDecreaseHeight,
        Self::WindowIncreaseWidth,
        Self::WindowDecreaseWidth,
        Self::GotoDefinition,
        Self::ShowDocumentation,
        Self::OpenCommandWindow,
        Self::CallOperatorFunc,
        Self::ExecuteNorm,
        Self::CQuit,
        Self::UpdateFile,
        Self::FoldRange,
        Self::FoldOpenRange,
        Self::FoldCloseRange,
        Self::MkVimrc,
    ];
}

/// Correlation identifier for host requests/results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HostRequestId(u64);

impl HostRequestId {
    /// Maximum request ID value.
    ///
    /// vim-core uses the full `u64` range. Integration layers that cross
    /// boundaries with narrower integer types (e.g. JavaScript's 53-bit safe
    /// integers) are responsible for their own range validation.
    pub const MAX_SAFE: u64 = u64::MAX;

    /// Create a new request id.
    #[inline]
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Get the raw id value.
    #[inline]
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Metadata attached to each host request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HostRequestMeta {
    /// Unique request id (also provides monotonic ordering via `Ord`).
    pub id: HostRequestId,
}

/// Shell-only requests emitted by core.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum HostRequest {
    /// Write current buffer or explicit path.
    WriteFile {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Optional target path.
        path: Option<CompactString>,
        /// Force write.
        force: bool,
    },
    /// Quit editor.
    Quit {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Force quit.
        force: bool,
    },
    /// Write and quit as one host transaction.
    WriteQuit {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Force write/quit.
        force: bool,
    },
    /// Edit/open file in host.
    EditFile {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Path to open.
        path: CompactString,
        /// Force open even if the host detects unsaved changes.
        force: bool,
    },
    /// Read file content and insert into current buffer.
    ReadFile {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Path to read.
        path: CompactString,
        /// Line to insert after (1-indexed), None = current line.
        after_line: Option<u32>,
    },
    /// Apply external filter command over an exact document range.
    FilterDocumentRange {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Exact document range to replace.
        range: Range,
        /// Motion type for register/put semantics.
        motion_type: MotionType,
        /// Input text corresponding to `range`.
        input_text: CompactString,
        /// Filter command.
        command: CompactString,
    },
    /// Read text from system clipboard.
    ReadClipboard {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Cursor offset where clipboard text should be inserted.
        cursor_offset: usize,
    },
    /// Run external command.
    ExternalCommand {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Command to execute.
        command: CompactString,
    },
    /// Forward unsupported ex command to host.
    CustomExCommand {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Raw command text.
        command: CompactString,
    },
    /// Auto-reindent a document range (`={motion}`).
    ReindentRange {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Exact document range to reindent.
        range: Range,
        /// Motion type for register/put semantics.
        motion_type: MotionType,
        /// Input text corresponding to `range`.
        input_text: CompactString,
        /// Column of `oap->start` for mark `[` computation.
        /// See Neovim indent.c:1054.
        start_col: usize,
        /// Column of `oap->end` for mark `]` computation.
        /// See Neovim indent.c:1055.
        end_col: usize,
        /// Number of newlines between `range.start()` and `oap->end` in the
        /// original text.  Tells `reindent_completion` which line of the
        /// replacement text the mark `]` falls on.
        end_line_in_range: usize,
        /// Byte offset of `oap->start` in the original text.
        ///
        /// When the linewise range is extended to include a preceding newline
        /// (EOF case), `range.start()` differs from `oap->start`. This field
        /// carries the true content position for correct mark `[` computation.
        start_byte_offset: usize,
    },
    /// List available host actions with optional filter.
    ListActions {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Optional name filter substring.
        filter: Option<CompactString>,
    },
    /// Read a config file for `:source` command.
    ///
    /// Host reads the file and returns content via `HostResult::Data`.
    /// The host can then call `VimEngine::source_config_text()` to
    /// process each line as an ex command.
    ReadConfigFile {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Path to the config file.
        path: CompactString,
    },
    /// Switch to buffer by number.
    SwitchBuffer {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Buffer number to switch to.
        number: u32,
    },
    /// Switch to next buffer.
    BufferNext {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Count of buffers to advance.
        count: u32,
    },
    /// Switch to previous buffer.
    BufferPrev {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Count of buffers to go back.
        count: u32,
    },
    /// Switch to first buffer.
    BufferFirst {
        /// Correlation metadata.
        meta: HostRequestMeta,
    },
    /// Switch to last buffer.
    BufferLast {
        /// Correlation metadata.
        meta: HostRequestMeta,
    },
    /// List open buffers.
    BufferList {
        /// Correlation metadata.
        meta: HostRequestMeta,
    },
    /// Open a new tab.
    TabNew {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Optional path to open in new tab.
        path: Option<CompactString>,
    },
    /// Switch to next tab.
    TabNext {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Count of tabs to advance.
        count: u32,
    },
    /// Switch to previous tab.
    TabPrev {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Count of tabs to go back.
        count: u32,
    },
    /// Close current tab.
    TabClose {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Force close without saving.
        force: bool,
    },

    // === Diagnostic navigation (host handles) ===
    /// Jump to next diagnostic: `:cn[ext]`
    DiagnosticNext {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Count of diagnostics to advance.
        count: u32,
    },
    /// Jump to previous diagnostic: `:cp[revious]`
    DiagnosticPrev {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Count of diagnostics to go back.
        count: u32,
    },
    /// Show diagnostic list: `:cl[ist]`
    DiagnosticList {
        /// Correlation metadata.
        meta: HostRequestMeta,
    },
    /// Jump to specific diagnostic by index: `:cc [index]`
    DiagnosticGoto {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// 1-based diagnostic index.
        index: u32,
    },

    // === Window management (host handles) ===
    /// Split window: `:sp[lit]`, `:vs[plit]`, `:new`, `:vnew`
    SplitWindow {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Horizontal or vertical split.
        direction: SplitDirection,
        /// Optional path to open in the new split.
        path: Option<CompactString>,
        /// If true, open a new empty buffer (`:new` / `:vnew`).
        new_file: bool,
    },
    /// Close current window: `:close[!]`
    CloseWindow {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Force close without saving.
        force: bool,
    },
    /// Close all other windows: `:only[!]`
    CloseOtherWindows {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Force close without saving.
        force: bool,
    },
    /// Write all buffers: `:wall`
    WriteAll {
        /// Correlation metadata.
        meta: HostRequestMeta,
    },
    /// Quit all windows: `:qall[!]`
    QuitAll {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Force quit without saving.
        force: bool,
    },
    /// Write all and quit all: `:wqall` / `:xall`
    WriteQuitAll {
        /// Correlation metadata.
        meta: HostRequestMeta,
    },
    /// Close/delete/wipeout a buffer: `:bd[elete]`, `:bw[ipeout]`
    CloseBuffer {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Force close without saving.
        force: bool,
        /// If true, wipeout (remove all traces); if false, just delete.
        wipeout: bool,
        /// Buffer target (number or name); host resolves.
        target: Option<CompactString>,
    },

    // === Window navigation/sizing (host handles) ===
    /// Move to next window (`Ctrl-W w`).
    WindowNext {
        /// Correlation metadata.
        meta: HostRequestMeta,
    },
    /// Move to previous window (`Ctrl-W W`).
    WindowPrev {
        /// Correlation metadata.
        meta: HostRequestMeta,
    },
    /// Move cursor to left window (`Ctrl-W h`).
    WindowMoveLeft {
        /// Correlation metadata.
        meta: HostRequestMeta,
    },
    /// Move cursor to right window (`Ctrl-W l`).
    WindowMoveRight {
        /// Correlation metadata.
        meta: HostRequestMeta,
    },
    /// Move cursor to window above (`Ctrl-W k`).
    WindowMoveUp {
        /// Correlation metadata.
        meta: HostRequestMeta,
    },
    /// Move cursor to window below (`Ctrl-W j`).
    WindowMoveDown {
        /// Correlation metadata.
        meta: HostRequestMeta,
    },
    /// Rotate windows downward/rightward (`Ctrl-W r`).
    WindowRotateDown {
        /// Correlation metadata.
        meta: HostRequestMeta,
    },
    /// Rotate windows upward/leftward (`Ctrl-W R`).
    WindowRotateUp {
        /// Correlation metadata.
        meta: HostRequestMeta,
    },
    /// Equalize all window sizes (`Ctrl-W =`).
    WindowEqualSize {
        /// Correlation metadata.
        meta: HostRequestMeta,
    },
    /// Increase window height by count (`Ctrl-W +`).
    WindowIncreaseHeight {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Number of rows to increase by.
        count: u32,
    },
    /// Decrease window height by count (`Ctrl-W -`).
    WindowDecreaseHeight {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Number of rows to decrease by.
        count: u32,
    },
    /// Increase window width by count (`Ctrl-W >`).
    WindowIncreaseWidth {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Number of columns to increase by.
        count: u32,
    },
    /// Decrease window width by count (`Ctrl-W <`).
    WindowDecreaseWidth {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Number of columns to decrease by.
        count: u32,
    },

    // === LSP / Navigation (host handles) ===
    /// Navigate to the definition of the symbol under cursor (`gd`).
    GotoDefinition {
        /// Correlation metadata.
        meta: HostRequestMeta,
    },
    /// Display documentation for the symbol under cursor (`K`).
    ShowDocumentation {
        /// Correlation metadata.
        meta: HostRequestMeta,
    },

    // === Command-line / extension (host handles) ===
    /// Open a command-line history window (`q:`, `q/`, `q?`, `Ctrl-F`).
    ///
    /// The host should display an editable history buffer populated with
    /// `history` entries (oldest first). If `prefill` is `Some`, the current
    /// partial input is appended as the last line.
    OpenCommandWindow {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Which command-line history to show.
        prompt: CommandLinePrompt,
        /// History entries for this prompt kind (oldest first, newest last).
        history: Vec<CompactString>,
        /// Partial input from an active command-line session (`Ctrl-F`).
        /// `None` when opened via `q:` / `q/` / `q?` from normal mode.
        prefill: Option<CompactString>,
    },
    /// Call the host's operatorfunc on a range (`g@{motion}`).
    ///
    /// The host should invoke the registered `operatorfunc` with the given
    /// range and motion type.
    CallOperatorFunc {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// The range the operator applies to.
        range: Range,
        /// Whether the motion was charwise or linewise.
        motion_type: MotionType,
    },
    /// Execute normal-mode keystrokes on a line range (`:norm`).
    ///
    /// The engine emits this so the host orchestrator can feed keys through
    /// the engine for each line in the specified range.
    ExecuteNorm {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Start line (0-indexed).
        start_line: u32,
        /// End line (0-indexed, inclusive).
        end_line: u32,
        /// Normal-mode keystrokes to execute.
        keys: CompactString,
        /// Whether mappings should be expanded while replaying keys.
        remap: bool,
    },

    /// Evaluate an expression for the `=` register.
    ///
    /// Host evaluates the expression string and returns the result text
    /// via `HostResult::Data`. The engine stores it as the expression
    /// register content.
    EvaluateExpression {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Expression text to evaluate.
        expression: CompactString,
    },
    /// Request insert-mode completion from the host.
    ///
    /// Emitted when the user presses Ctrl-X followed by a completion
    /// trigger key in insert mode. The host should provide completion
    /// candidates of the specified kind.
    RequestCompletion {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// The kind of completion requested.
        kind: CompletionKind,
    },
    /// Optional host-side command-line UI synchronization.
    SyncCommandLine {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Prompt kind.
        prompt: CommandLinePrompt,
        /// Current command-line input.
        input: CompactString,
        /// Cursor byte offset within `input`.
        cursor: usize,
    },
    /// Display the full message history to the user (`:messages`).
    ///
    /// The host should render all entries in order (oldest first) using
    /// appropriate visual distinction for each [`crate::state::MessageKind`].
    ShowMessageHistory {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Snapshot of all retained message history entries, oldest first.
        entries: Vec<MessageEntry>,
    },
    /// Evaluate an `<expr>` mapping's RHS expression.
    ///
    /// When a mapping defined with the `<expr>` flag is triggered, the engine
    /// emits this request instead of directly expanding the RHS. The host
    /// evaluates the expression string and returns the resulting key sequence
    /// as a string via `HostResult::Data`. The engine then parses those keys
    /// and feeds them into the typeahead buffer.
    ///
    /// The `kind` determines how the returned keys are treated:
    /// - `Recursive` (from `:map <expr>`): returned keys go through mapping expansion
    /// - `NonRecursive` (from `:noremap <expr>`): returned keys bypass mapping expansion
    EvaluateMapping {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// The expression text to evaluate (the mapping's RHS).
        expression: CompactString,
        /// The mapping mode in which the mapping was triggered.
        mode: crate::keymap::MappingMode,
        /// Whether returned keys should be remappable (`Recursive` for `:map <expr>`)
        /// or bypass mapping expansion (`NonRecursive` for `:noremap <expr>`).
        kind: crate::keymap::MappingKind,
        /// Whether the mapping was defined with `<silent>` — when `true`,
        /// `ShowMessage` effects produced by the returned keys are suppressed.
        silent: bool,
    },
    /// Navigate to a position in a different buffer via jump list.
    ///
    /// Emitted when Ctrl-O or Ctrl-I navigates to a jump list entry whose
    /// `buffer_id` differs from the current buffer. The host should:
    /// 1. Switch to the target buffer identified by `buffer_id`
    /// 2. Place the cursor at `offset` within that buffer
    ///
    /// This is a fire-and-forget request — the engine has already updated
    /// its internal jump list cursor. The host handles the actual buffer
    /// switch and cursor positioning.
    JumpToBuffer {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Target buffer to switch to.
        buffer_id: BufferId,
        /// Cursor offset within the target buffer.
        offset: Offset,
    },
    /// Navigate to a global mark (A-Z) in a different buffer.
    ///
    /// Emitted when jumping to a global mark whose `buffer_id` differs from
    /// the current buffer. The host should:
    /// 1. Switch to the target buffer identified by `buffer_id`
    /// 2. Place the cursor at `offset` within that buffer
    ///
    /// The `name` field identifies which mark triggered the jump (e.g. `'A`).
    /// This is a fire-and-forget request — the engine does NOT move the
    /// cursor in the current buffer.
    JumpToGlobalMark {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Target buffer to switch to.
        buffer_id: BufferId,
        /// Cursor offset within the target buffer.
        offset: Offset,
        /// The mark name character (A-Z).
        name: char,
    },
    /// Run a named host action (from `<Action>(name)` mappings).
    ///
    /// Emitted when a mapping triggers `<Action>(name)`. The host should
    /// look up the action by name and execute it. This is fire-and-forget:
    /// the engine does not depend on the response payload.
    RunAction {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// The action name to execute (e.g. `"editor.action.addSelectionToNextFindMatch"`).
        name: CompactString,
        /// Count prefix typed before the action key (e.g. `5` in `5<key>`).
        count: Option<u32>,
        /// Register selected before the action key (e.g. `"a` in `"a<key>`).
        register: Option<RegisterName>,
        /// Mode at the time the action was triggered (e.g. `"NORMAL"`, `"VISUAL"`).
        mode: CompactString,
        /// Anchor offset of the visual selection, if any.
        selection_anchor: Option<usize>,
        /// Head (cursor) offset of the visual selection, if any.
        selection_head: Option<usize>,
    },
    /// Request command-line completion candidates from the host.
    ///
    /// Emitted when the user presses Tab in command-line mode for an argument
    /// that requires host-side data (file paths, buffer names). The host
    /// should return a JSON response with `{ "candidates": [...] }` via
    /// `HostResult::Data`. The engine will then cycle through the candidates
    /// using `complete_cycle`.
    RequestCmdlineCompletion {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// What category of completion the host should provide.
        kind: CmdlineCompletionKind,
        /// The current text prefix to filter candidates against.
        prefix: CompactString,
        /// Start byte offset of the text to replace in the command line.
        replace_range_start: usize,
        /// End byte offset of the text to replace in the command line.
        replace_range_end: usize,
    },
    /// Quit with a non-zero exit code: `:cq[uit] [exit_code]`
    CQuit {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Exit code to return to the shell (default: 1).
        exit_code: i32,
    },
    /// Update file (write only if modified): `:up[date][!] [path]`
    UpdateFile {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Optional target path.
        path: Option<CompactString>,
        /// Force write.
        force: bool,
    },
    /// Create a fold from a line range: `:[range]fold`
    FoldRange {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Start line (0-indexed).
        start_line: u32,
        /// End line (0-indexed, inclusive).
        end_line: u32,
    },
    /// Open folds in a line range: `:[range]foldopen[!]`
    FoldOpenRange {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Start line (0-indexed).
        start_line: u32,
        /// End line (0-indexed, inclusive).
        end_line: u32,
        /// If true, open recursively.
        recursive: bool,
    },
    /// Close folds in a line range: `:[range]foldclose[!]`
    FoldCloseRange {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Start line (0-indexed).
        start_line: u32,
        /// End line (0-indexed, inclusive).
        end_line: u32,
        /// If true, close recursively.
        recursive: bool,
    },

    // === Iterator commands (host handles iteration) ===
    /// Execute an ex command in each window: `:windo {cmd}`
    ///
    /// The host should iterate over all windows and re-execute the given
    /// command text in each one by calling `VimEngine::execute_ex_command`.
    ForEachWindow {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// The ex command text to execute in each window.
        command: CompactString,
    },
    /// Execute an ex command in each buffer: `:bufdo {cmd}`
    ///
    /// The host should iterate over all listed buffers and re-execute the
    /// given command text in each one by calling `VimEngine::execute_ex_command`.
    ForEachBuffer {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// The ex command text to execute in each buffer.
        command: CompactString,
    },
    /// Execute an ex command in each tab: `:tabdo {cmd}`
    ///
    /// The host should iterate over all tabs and re-execute the given
    /// command text in each one by calling `VimEngine::execute_ex_command`.
    ForEachTab {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// The ex command text to execute in each tab.
        command: CompactString,
    },

    /// Generate a `.godot-vimrc` template file: `:mkvimrc[!]`
    ///
    /// The host should write a starter configuration file to the project
    /// or user config path. If `force` is false and the file already exists,
    /// the host should report an error instead of overwriting.
    MkVimrc {
        /// Correlation metadata.
        meta: HostRequestMeta,
        /// Force overwrite if file already exists.
        force: bool,
    },
}

impl HostRequest {
    /// Return the stable [`HostRequestKind`] discriminator for this value.
    #[must_use]
    pub const fn kind(&self) -> HostRequestKind {
        match self {
            Self::WriteFile { .. } => HostRequestKind::WriteFile,
            Self::Quit { .. } => HostRequestKind::Quit,
            Self::WriteQuit { .. } => HostRequestKind::WriteQuit,
            Self::EditFile { .. } => HostRequestKind::EditFile,
            Self::ReadFile { .. } => HostRequestKind::ReadFile,
            Self::FilterDocumentRange { .. } => HostRequestKind::FilterDocumentRange,
            Self::ReadClipboard { .. } => HostRequestKind::ReadClipboard,
            Self::ExternalCommand { .. } => HostRequestKind::ExternalCommand,
            Self::CustomExCommand { .. } => HostRequestKind::CustomExCommand,
            Self::ReindentRange { .. } => HostRequestKind::ReindentRange,
            Self::ListActions { .. } => HostRequestKind::ListActions,
            Self::ReadConfigFile { .. } => HostRequestKind::ReadConfigFile,
            Self::SwitchBuffer { .. } => HostRequestKind::SwitchBuffer,
            Self::BufferNext { .. } => HostRequestKind::BufferNext,
            Self::BufferPrev { .. } => HostRequestKind::BufferPrev,
            Self::BufferFirst { .. } => HostRequestKind::BufferFirst,
            Self::BufferLast { .. } => HostRequestKind::BufferLast,
            Self::BufferList { .. } => HostRequestKind::BufferList,
            Self::TabNew { .. } => HostRequestKind::TabNew,
            Self::TabNext { .. } => HostRequestKind::TabNext,
            Self::TabPrev { .. } => HostRequestKind::TabPrev,
            Self::TabClose { .. } => HostRequestKind::TabClose,
            Self::DiagnosticNext { .. } => HostRequestKind::DiagnosticNext,
            Self::DiagnosticPrev { .. } => HostRequestKind::DiagnosticPrev,
            Self::DiagnosticList { .. } => HostRequestKind::DiagnosticList,
            Self::DiagnosticGoto { .. } => HostRequestKind::DiagnosticGoto,
            Self::EvaluateExpression { .. } => HostRequestKind::EvaluateExpression,
            Self::RequestCompletion { .. } => HostRequestKind::RequestCompletion,
            Self::SyncCommandLine { .. } => HostRequestKind::SyncCommandLine,
            Self::ShowMessageHistory { .. } => HostRequestKind::ShowMessageHistory,
            Self::EvaluateMapping { .. } => HostRequestKind::EvaluateMapping,
            Self::JumpToBuffer { .. } => HostRequestKind::JumpToBuffer,
            Self::JumpToGlobalMark { .. } => HostRequestKind::JumpToGlobalMark,
            Self::RunAction { .. } => HostRequestKind::RunAction,
            Self::RequestCmdlineCompletion { .. } => HostRequestKind::RequestCmdlineCompletion,
            Self::SplitWindow { .. } => HostRequestKind::SplitWindow,
            Self::CloseWindow { .. } => HostRequestKind::CloseWindow,
            Self::CloseOtherWindows { .. } => HostRequestKind::CloseOtherWindows,
            Self::WriteAll { .. } => HostRequestKind::WriteAll,
            Self::QuitAll { .. } => HostRequestKind::QuitAll,
            Self::WriteQuitAll { .. } => HostRequestKind::WriteQuitAll,
            Self::CloseBuffer { .. } => HostRequestKind::CloseBuffer,
            Self::WindowNext { .. } => HostRequestKind::WindowNext,
            Self::WindowPrev { .. } => HostRequestKind::WindowPrev,
            Self::WindowMoveLeft { .. } => HostRequestKind::WindowMoveLeft,
            Self::WindowMoveRight { .. } => HostRequestKind::WindowMoveRight,
            Self::WindowMoveUp { .. } => HostRequestKind::WindowMoveUp,
            Self::WindowMoveDown { .. } => HostRequestKind::WindowMoveDown,
            Self::WindowRotateDown { .. } => HostRequestKind::WindowRotateDown,
            Self::WindowRotateUp { .. } => HostRequestKind::WindowRotateUp,
            Self::WindowEqualSize { .. } => HostRequestKind::WindowEqualSize,
            Self::WindowIncreaseHeight { .. } => HostRequestKind::WindowIncreaseHeight,
            Self::WindowDecreaseHeight { .. } => HostRequestKind::WindowDecreaseHeight,
            Self::WindowIncreaseWidth { .. } => HostRequestKind::WindowIncreaseWidth,
            Self::WindowDecreaseWidth { .. } => HostRequestKind::WindowDecreaseWidth,
            Self::GotoDefinition { .. } => HostRequestKind::GotoDefinition,
            Self::ShowDocumentation { .. } => HostRequestKind::ShowDocumentation,
            Self::OpenCommandWindow { .. } => HostRequestKind::OpenCommandWindow,
            Self::CallOperatorFunc { .. } => HostRequestKind::CallOperatorFunc,
            Self::ExecuteNorm { .. } => HostRequestKind::ExecuteNorm,
            Self::CQuit { .. } => HostRequestKind::CQuit,
            Self::UpdateFile { .. } => HostRequestKind::UpdateFile,
            Self::FoldRange { .. } => HostRequestKind::FoldRange,
            Self::FoldOpenRange { .. } => HostRequestKind::FoldOpenRange,
            Self::FoldCloseRange { .. } => HostRequestKind::FoldCloseRange,
            Self::ForEachWindow { .. } => HostRequestKind::ForEachWindow,
            Self::ForEachBuffer { .. } => HostRequestKind::ForEachBuffer,
            Self::ForEachTab { .. } => HostRequestKind::ForEachTab,
            Self::MkVimrc { .. } => HostRequestKind::MkVimrc,
        }
    }

    /// Get request metadata.
    #[must_use]
    pub const fn meta(&self) -> HostRequestMeta {
        match self {
            Self::WriteFile { meta, .. }
            | Self::Quit { meta, .. }
            | Self::WriteQuit { meta, .. }
            | Self::EditFile { meta, .. }
            | Self::ReadFile { meta, .. }
            | Self::FilterDocumentRange { meta, .. }
            | Self::ReadClipboard { meta, .. }
            | Self::ExternalCommand { meta, .. }
            | Self::CustomExCommand { meta, .. }
            | Self::ReindentRange { meta, .. }
            | Self::ListActions { meta, .. }
            | Self::ReadConfigFile { meta, .. }
            | Self::SwitchBuffer { meta, .. }
            | Self::BufferNext { meta, .. }
            | Self::BufferPrev { meta, .. }
            | Self::BufferFirst { meta, .. }
            | Self::BufferLast { meta, .. }
            | Self::BufferList { meta, .. }
            | Self::TabNew { meta, .. }
            | Self::TabNext { meta, .. }
            | Self::TabPrev { meta, .. }
            | Self::TabClose { meta, .. }
            | Self::DiagnosticNext { meta, .. }
            | Self::DiagnosticPrev { meta, .. }
            | Self::DiagnosticList { meta, .. }
            | Self::DiagnosticGoto { meta, .. }
            | Self::EvaluateExpression { meta, .. }
            | Self::RequestCompletion { meta, .. }
            | Self::SyncCommandLine { meta, .. }
            | Self::ShowMessageHistory { meta, .. }
            | Self::EvaluateMapping { meta, .. }
            | Self::JumpToBuffer { meta, .. }
            | Self::JumpToGlobalMark { meta, .. }
            | Self::RunAction { meta, .. }
            | Self::RequestCmdlineCompletion { meta, .. }
            | Self::SplitWindow { meta, .. }
            | Self::CloseWindow { meta, .. }
            | Self::CloseOtherWindows { meta, .. }
            | Self::WriteAll { meta, .. }
            | Self::QuitAll { meta, .. }
            | Self::WriteQuitAll { meta, .. }
            | Self::CloseBuffer { meta, .. }
            | Self::WindowNext { meta, .. }
            | Self::WindowPrev { meta, .. }
            | Self::WindowMoveLeft { meta, .. }
            | Self::WindowMoveRight { meta, .. }
            | Self::WindowMoveUp { meta, .. }
            | Self::WindowMoveDown { meta, .. }
            | Self::WindowRotateDown { meta, .. }
            | Self::WindowRotateUp { meta, .. }
            | Self::WindowEqualSize { meta, .. }
            | Self::WindowIncreaseHeight { meta, .. }
            | Self::WindowDecreaseHeight { meta, .. }
            | Self::WindowIncreaseWidth { meta, .. }
            | Self::WindowDecreaseWidth { meta, .. }
            | Self::GotoDefinition { meta, .. }
            | Self::ShowDocumentation { meta, .. }
            | Self::OpenCommandWindow { meta, .. }
            | Self::CallOperatorFunc { meta, .. }
            | Self::ExecuteNorm { meta, .. }
            | Self::CQuit { meta, .. }
            | Self::UpdateFile { meta, .. }
            | Self::FoldRange { meta, .. }
            | Self::FoldOpenRange { meta, .. }
            | Self::FoldCloseRange { meta, .. }
            | Self::ForEachWindow { meta, .. }
            | Self::ForEachBuffer { meta, .. }
            | Self::ForEachTab { meta, .. }
            | Self::MkVimrc { meta, .. } => *meta,
        }
    }

    /// Get request id.
    #[must_use]
    pub const fn id(&self) -> HostRequestId {
        self.meta().id
    }
}

/// A single candidate entry returned by the host for command-line completion.
///
/// Carries the completion text plus optional display metadata. Created by
/// the wire/boundary layer after deserializing the host's
/// JSON response, so vim-core never needs a JSON parser.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CmdlineCompletionEntry {
    /// The completion text to insert.
    pub text: CompactString,
    /// Optional short description (e.g. "directory", "file").
    pub description: Option<CompactString>,
    /// Optional detail text (e.g. full path).
    pub detail: Option<CompactString>,
}

/// Host completion result.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum HostResult {
    /// Generic successful completion.
    Success {
        /// Completed request id.
        id: HostRequestId,
        /// Optional host message.
        message: Option<CompactString>,
    },
    /// Failure completion.
    Failure {
        /// Failed request id.
        id: HostRequestId,
        /// Error message.
        error: CompactString,
    },
    /// Data-bearing completion (e.g. `:r`).
    Data {
        /// Completed request id.
        id: HostRequestId,
        /// Returned payload.
        data: CompactString,
        /// Host-computed insertion offset if applicable.
        offset: Option<usize>,
    },
    /// Clipboard text completion payload.
    ClipboardText {
        /// Completed request id.
        id: HostRequestId,
        /// Clipboard contents.
        text: CompactString,
    },
    /// Filter completion payload for range replacement.
    FilteredRange {
        /// Completed request id.
        id: HostRequestId,
        /// Replacement text for the requested range.
        replacement: CompactString,
        /// Optional cursor offset after replacement.
        cursor_offset: Option<usize>,
        /// Optional stderr text from filter execution.
        stderr: Option<CompactString>,
        /// Optional mark `.` offset (first byte of first changed line).
        ///
        /// Used by `=` (reindent) where mark `.` differs from cursor position.
        /// When `None`, the engine uses `cursor_offset` or range start.
        #[cfg_attr(feature = "serde", serde(default))]
        mark_dot_offset: Option<usize>,
    },
    /// Command-line completion candidates returned by the host.
    ///
    /// Created by the boundary layer after deserializing the host's JSON
    /// response to `RequestCmdlineCompletion`. The engine feeds these into
    /// `CommandLineState::complete_cycle()`.
    CmdlineCompletionCandidates {
        /// Completed request id.
        id: HostRequestId,
        /// The parsed completion candidates.
        candidates: Vec<CmdlineCompletionEntry>,
    },
}

impl HostResult {
    /// Get completed request id.
    #[must_use]
    pub const fn id(&self) -> HostRequestId {
        match self {
            Self::Success { id, .. }
            | Self::Failure { id, .. }
            | Self::Data { id, .. }
            | Self::ClipboardText { id, .. }
            | Self::FilteredRange { id, .. }
            | Self::CmdlineCompletionCandidates { id, .. } => *id,
        }
    }
}

/// Three-valued response to a HostRequest.
///
/// Replaces `Option<HostResult>` to distinguish between async deferral
/// and unsupported operations.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RequestDisposition {
    /// Host completed the request synchronously.
    Completed(HostResult),
    /// Host will complete later via `VimSession::complete_request()`.
    Deferred,
    /// Host cannot handle this request. Engine applies default fallback.
    Unsupported,
}

/// Deterministic request id generator.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct HostRequestSequencer {
    next: u64,
}

impl HostRequestSequencer {
    /// Allocate the next request metadata.
    ///
    /// # Panics
    ///
    /// Panics if the sequence counter exceeds [`HostRequestId::MAX_SAFE`].
    #[must_use]
    pub const fn next_meta(&mut self) -> HostRequestMeta {
        let sequence = self.next;
        assert!(
            sequence <= HostRequestId::MAX_SAFE,
            "host request ID overflow: exceeded u64::MAX"
        );
        self.next = self.next.saturating_add(1);
        HostRequestMeta {
            id: HostRequestId::new(sequence),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::CompletionKind;

    // ── HostRequestKind::ALL ───────────────────────────────────────────

    #[test]
    fn all_array_count_matches_variant_count() {
        // When a new variant is added, both the enum and ALL must be updated.
        // This test ensures the ALL array length is correct (66 with :mkvimrc).
        assert_eq!(HostRequestKind::ALL.len(), 66);
    }

    #[test]
    fn all_array_contains_request_completion() {
        assert!(
            HostRequestKind::ALL.contains(&HostRequestKind::RequestCompletion),
            "ALL array must include RequestCompletion"
        );
    }

    #[test]
    fn all_array_has_no_duplicates() {
        let all = &HostRequestKind::ALL;
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "ALL[{i}] == ALL[{j}] (duplicate)");
                }
            }
        }
    }

    // ── HostRequest::RequestCompletion ─────────────────────────────────

    #[test]
    fn request_completion_kind_discriminator() {
        let meta = HostRequestMeta {
            id: HostRequestId::new(42),
        };
        let req = HostRequest::RequestCompletion {
            meta,
            kind: CompletionKind::Omni,
        };
        assert_eq!(req.kind(), HostRequestKind::RequestCompletion);
    }

    #[test]
    fn request_completion_meta_accessor() {
        let meta = HostRequestMeta {
            id: HostRequestId::new(99),
        };
        let req = HostRequest::RequestCompletion {
            meta,
            kind: CompletionKind::FileName,
        };
        assert_eq!(req.meta(), meta);
        assert_eq!(req.id(), HostRequestId::new(99));
    }

    #[test]
    fn request_completion_preserves_kind_field() {
        let meta = HostRequestMeta {
            id: HostRequestId::new(0),
        };
        let req = HostRequest::RequestCompletion {
            meta,
            kind: CompletionKind::Tag,
        };
        if let HostRequest::RequestCompletion { kind, .. } = req {
            assert_eq!(kind, CompletionKind::Tag);
        } else {
            panic!("expected RequestCompletion variant");
        }
    }

    #[test]
    fn request_completion_all_completion_kinds() {
        // Verify RequestCompletion works with every CompletionKind variant.
        let meta = HostRequestMeta {
            id: HostRequestId::new(0),
        };
        let kinds = [
            CompletionKind::Line,
            CompletionKind::KeywordNext,
            CompletionKind::KeywordPrev,
            CompletionKind::Dictionary,
            CompletionKind::Thesaurus,
            CompletionKind::IncludePath,
            CompletionKind::Tag,
            CompletionKind::FileName,
            CompletionKind::DefinitionMacro,
            CompletionKind::VimCommand,
            CompletionKind::UserDefined,
            CompletionKind::Omni,
            CompletionKind::Spelling,
        ];
        for kind in kinds {
            let req = HostRequest::RequestCompletion { meta, kind };
            assert_eq!(req.kind(), HostRequestKind::RequestCompletion);
            assert_eq!(req.meta(), meta);
        }
    }

    // ── Sequencer ─────────────────────────────────────────────────────

    #[test]
    fn sequencer_produces_monotonic_ids() {
        let mut seq = HostRequestSequencer::default();
        let m1 = seq.next_meta();
        let m2 = seq.next_meta();
        assert!(
            m2.id > m1.id,
            "sequencer ids must be monotonically increasing"
        );
    }

    #[test]
    fn sequencer_allows_max_id() {
        // MAX_SAFE is u64::MAX — vim-core uses the full u64 range.
        // Integration layers impose their own limits.
        let mut seq = HostRequestSequencer {
            next: HostRequestId::MAX_SAFE,
        };
        let meta = seq.next_meta();
        assert_eq!(meta.id.get(), HostRequestId::MAX_SAFE);
    }

    // ── HostRequest::EvaluateMapping ────────────────────────────────────

    #[test]
    fn evaluate_mapping_kind_discriminator() {
        let meta = HostRequestMeta {
            id: HostRequestId::new(50),
        };
        let req = HostRequest::EvaluateMapping {
            meta,
            expression: CompactString::from("v:count ? 'j' : 'gj'"),
            mode: crate::keymap::MappingMode::Normal,
            kind: crate::keymap::MappingKind::Recursive,
            silent: false,
        };
        assert_eq!(req.kind(), HostRequestKind::EvaluateMapping);
    }

    #[test]
    fn evaluate_mapping_meta_accessor() {
        let meta = HostRequestMeta {
            id: HostRequestId::new(77),
        };
        let req = HostRequest::EvaluateMapping {
            meta,
            expression: CompactString::from("MyExpr()"),
            mode: crate::keymap::MappingMode::Insert,
            kind: crate::keymap::MappingKind::NonRecursive,
            silent: false,
        };
        assert_eq!(req.meta(), meta);
        assert_eq!(req.id(), HostRequestId::new(77));
    }

    #[test]
    fn evaluate_mapping_preserves_expression_and_mode() {
        let meta = HostRequestMeta {
            id: HostRequestId::new(0),
        };
        let req = HostRequest::EvaluateMapping {
            meta,
            expression: CompactString::from("pumvisible() ? '<C-Y>' : '<CR>'"),
            mode: crate::keymap::MappingMode::Visual,
            kind: crate::keymap::MappingKind::Recursive,
            silent: false,
        };
        if let HostRequest::EvaluateMapping {
            expression,
            mode,
            kind,
            ..
        } = req
        {
            assert!(kind.is_recursive(), "recursive flag should be preserved");
            assert_eq!(expression.as_str(), "pumvisible() ? '<C-Y>' : '<CR>'");
            assert_eq!(mode, crate::keymap::MappingMode::Visual);
        } else {
            panic!("expected EvaluateMapping variant");
        }
    }

    #[test]
    fn all_array_contains_evaluate_mapping() {
        assert!(
            HostRequestKind::ALL.contains(&HostRequestKind::EvaluateMapping),
            "ALL array must include EvaluateMapping"
        );
    }

    #[test]
    fn evaluate_mapping_all_mapping_modes() {
        let meta = HostRequestMeta {
            id: HostRequestId::new(0),
        };
        let modes = [
            crate::keymap::MappingMode::Normal,
            crate::keymap::MappingMode::Visual,
            crate::keymap::MappingMode::Insert,
            crate::keymap::MappingMode::Operator,
        ];
        for mode in modes {
            let req = HostRequest::EvaluateMapping {
                meta,
                expression: CompactString::from("test()"),
                mode,
                kind: crate::keymap::MappingKind::NonRecursive,
                silent: false,
            };
            assert_eq!(req.kind(), HostRequestKind::EvaluateMapping);
            assert_eq!(req.meta(), meta);
        }
    }
}
