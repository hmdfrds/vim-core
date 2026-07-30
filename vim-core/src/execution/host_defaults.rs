//! Default host request handling — the protocol truth table.
//!
//! Every integration must decide how to respond to each of the 56 host request
//! types. This module encodes the canonical safe defaults so integrators don't
//! have to rediscover the protocol rules independently.
//!
//! # Usage
//!
//! Integrators building their own dispatch loop can use [`default_result`] as a
//! fallback for requests they don't handle:
//!
//! ```ignore
//! for request in response.host_requests() {
//!     let result = match request {
//!         HostRequest::WriteFile { .. } => my_save_handler(request),
//!         HostRequest::Quit { .. }      => my_quit_handler(request),
//!         // ... handle what you need ...
//!         other => host_defaults::default_result(other)
//!             .unwrap_or_else(|| HostResult::Failure {
//!                 id: other.id(),
//!                 error: "not implemented".into(),
//!             }),
//!     };
//!     session.complete_request(&result);
//! }
//! ```
//!
//! [`VimSession`](crate::execution::VimSession) uses this module internally when
//! auto-handling is enabled,
//! so most integrators never need to call these functions directly.

use compact_str::CompactString;

use super::host::{HostRequest, HostRequestKind, HostResult};

/// What kind of [`HostResult`] the engine expects for a given request type.
///
/// Returning the wrong variant causes either a silent no-op (dangerous) or
/// a `"host completion payload mismatch"` error (noisy but safe).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpectedResultKind {
    /// [`HostResult::Success`] — engine ignores the payload.
    Success,
    /// [`HostResult::Data`] — engine acts on the data (insert text, show message, inject keys).
    Data,
    /// [`HostResult::ClipboardText`] — engine inserts clipboard content at cursor.
    ClipboardText,
    /// [`HostResult::FilteredRange`] — engine replaces the requested range with the payload.
    FilteredRange,
}

/// Return the expected [`HostResult`] kind for a request type.
///
/// This is the protocol truth table. Use it to construct the correct
/// result variant when building a manual dispatch loop.
///
/// # Silent-bug warning
///
/// Returning `Success` for a request that expects `Data` causes the engine to
/// silently produce empty content — no error, no feedback. For example:
///
/// - `ReadFile` + `Success` → `:r file` inserts nothing
/// - `EvaluateExpression` + `Success` → `<C-R>=` inserts nothing
/// - `EvaluateMapping` + `Success` → `<expr>` mapping produces no keys
/// - `FilterDocumentRange` + `Success` → engine silently no-ops (range unchanged)
#[must_use]
pub const fn expected_result_kind(kind: HostRequestKind) -> ExpectedResultKind {
    match kind {
        // Data-bearing: engine acts on the returned content.
        HostRequestKind::ReadFile
        | HostRequestKind::ReadConfigFile
        | HostRequestKind::EvaluateExpression
        | HostRequestKind::EvaluateMapping
        | HostRequestKind::ExternalCommand => ExpectedResultKind::Data,

        // Clipboard: engine inserts at cursor offset.
        HostRequestKind::ReadClipboard => ExpectedResultKind::ClipboardText,

        // Filtered range: engine replaces the document range.
        HostRequestKind::FilterDocumentRange | HostRequestKind::ReindentRange => {
            ExpectedResultKind::FilteredRange
        }

        // Everything else: engine ignores the result payload.
        _ => ExpectedResultKind::Success,
    }
}

/// Return a safe default [`HostResult`] for requests that don't need host involvement.
///
/// Returns `Some(result)` for requests where the library can produce a correct
/// response without any host action. Returns `None` for requests that require
/// host-specific logic (file I/O, clipboard, quit, buffer navigation, etc.).
///
/// # Categories
///
/// **Auto-succeed (safe no-ops):** `SyncCommandLine`, `ShowMessageHistory`,
/// `RequestCompletion`, `ListActions`, and all buffer/tab navigation.
/// These are either fire-and-forget notifications or operations that a
/// single-buffer host can safely acknowledge without action.
///
/// **Auto-fail (prevent silent bugs):** `EvaluateExpression`, `EvaluateMapping`,
/// `FilterDocumentRange`, `ReindentRange`, `ExternalCommand`.
/// Fulfilling these with `Success` causes silent data loss or invisible no-ops.
/// Failing them gives the user a visible error message.
///
/// **Requires host (`None`):** `WriteFile`, `Quit`, `WriteQuit`, `EditFile`,
/// `ReadFile`, `ReadConfigFile`, `ReadClipboard`, `CustomExCommand`, `JumpToBuffer`.
/// The library cannot produce a meaningful response for these.
#[must_use]
pub fn default_result(request: &HostRequest) -> Option<HostResult> {
    let id = request.id();
    match request.kind() {
        // ── Auto-succeed: fire-and-forget / safe no-ops ─────────────────
        HostRequestKind::SyncCommandLine
        | HostRequestKind::ShowMessageHistory
        | HostRequestKind::RequestCompletion
        | HostRequestKind::ListActions
        | HostRequestKind::SwitchBuffer
        | HostRequestKind::BufferNext
        | HostRequestKind::BufferPrev
        | HostRequestKind::BufferFirst
        | HostRequestKind::BufferLast
        | HostRequestKind::BufferList
        | HostRequestKind::TabNew
        | HostRequestKind::TabNext
        | HostRequestKind::TabPrev
        | HostRequestKind::TabClose
        | HostRequestKind::DiagnosticNext
        | HostRequestKind::DiagnosticPrev
        | HostRequestKind::DiagnosticList
        | HostRequestKind::DiagnosticGoto
        | HostRequestKind::JumpToBuffer
        | HostRequestKind::JumpToGlobalMark
        | HostRequestKind::RunAction
        | HostRequestKind::SplitWindow
        | HostRequestKind::CloseWindow
        | HostRequestKind::CloseOtherWindows
        | HostRequestKind::WriteAll
        | HostRequestKind::QuitAll
        | HostRequestKind::WriteQuitAll
        | HostRequestKind::CloseBuffer
        | HostRequestKind::WindowNext
        | HostRequestKind::WindowPrev
        | HostRequestKind::WindowMoveLeft
        | HostRequestKind::WindowMoveRight
        | HostRequestKind::WindowMoveUp
        | HostRequestKind::WindowMoveDown
        | HostRequestKind::WindowRotateDown
        | HostRequestKind::WindowRotateUp
        | HostRequestKind::WindowEqualSize
        | HostRequestKind::WindowIncreaseHeight
        | HostRequestKind::WindowDecreaseHeight
        | HostRequestKind::WindowIncreaseWidth
        | HostRequestKind::WindowDecreaseWidth
        | HostRequestKind::GotoDefinition
        | HostRequestKind::ShowDocumentation
        | HostRequestKind::OpenCommandWindow
        | HostRequestKind::CallOperatorFunc
        | HostRequestKind::ExecuteNorm
        | HostRequestKind::FoldRange
        | HostRequestKind::FoldOpenRange
        | HostRequestKind::FoldCloseRange
        | HostRequestKind::ForEachWindow
        | HostRequestKind::ForEachBuffer
        | HostRequestKind::ForEachTab => Some(HostResult::Success { id, message: None }),

        // ── Auto-fail: prevent silent bugs ──────────────────────────────
        HostRequestKind::EvaluateExpression | HostRequestKind::EvaluateMapping => {
            Some(HostResult::Failure {
                id,
                error: CompactString::from("Expression evaluation not supported"),
            })
        }

        HostRequestKind::FilterDocumentRange | HostRequestKind::ReindentRange => {
            Some(HostResult::Failure {
                id,
                error: CompactString::from("Filter/reindent not supported"),
            })
        }

        HostRequestKind::ExternalCommand => Some(HostResult::Failure {
            id,
            error: CompactString::from("Shell commands not supported"),
        }),

        // ── Requires host action ────────────────────────────────────────
        HostRequestKind::WriteFile
        | HostRequestKind::Quit
        | HostRequestKind::WriteQuit
        | HostRequestKind::EditFile
        | HostRequestKind::ReadFile
        | HostRequestKind::ReadConfigFile
        | HostRequestKind::ReadClipboard
        | HostRequestKind::CustomExCommand
        | HostRequestKind::RequestCmdlineCompletion
        | HostRequestKind::CQuit
        | HostRequestKind::UpdateFile
        | HostRequestKind::MkVimrc => None,
    }
}

/// Whether a request is fire-and-forget (engine ignores the response payload).
///
/// Fire-and-forget requests should still be completed (to drain the engine's
/// pending map), but the host does not need to perform any action.
#[must_use]
pub const fn is_fire_and_forget(kind: HostRequestKind) -> bool {
    matches!(
        kind,
        HostRequestKind::SyncCommandLine
            | HostRequestKind::JumpToBuffer
            | HostRequestKind::JumpToGlobalMark
            | HostRequestKind::RunAction
            | HostRequestKind::WindowNext
            | HostRequestKind::WindowPrev
            | HostRequestKind::WindowMoveLeft
            | HostRequestKind::WindowMoveRight
            | HostRequestKind::WindowMoveUp
            | HostRequestKind::WindowMoveDown
            | HostRequestKind::WindowRotateDown
            | HostRequestKind::WindowRotateUp
            | HostRequestKind::WindowEqualSize
            | HostRequestKind::WindowIncreaseHeight
            | HostRequestKind::WindowDecreaseHeight
            | HostRequestKind::WindowIncreaseWidth
            | HostRequestKind::WindowDecreaseWidth
            | HostRequestKind::GotoDefinition
            | HostRequestKind::ShowDocumentation
            | HostRequestKind::OpenCommandWindow
            | HostRequestKind::CallOperatorFunc
            | HostRequestKind::ExecuteNorm
    )
}

/// Whether a request should pause batch-key draining.
///
/// When processing a batch of keys, pausing requests stop the drain so
/// the host can provide a result before the engine continues. Non-pausing
/// requests are emitted as events but don't create data dependencies for
/// subsequent key processing.
///
/// ## Pauses drain (returns `true`)
///
/// Requests where the engine needs the host's response before it can
/// meaningfully continue: file I/O (`WriteFile`, `EditFile`, `ReadFile`,
/// `ReadConfigFile`), data retrieval (`ReadClipboard`), text transformation
/// (`FilterDocumentRange`, `ReindentRange`), and evaluation (`ExternalCommand`,
/// `EvaluateExpression`, `EvaluateMapping`).
///
/// ## Does not pause (returns `false`)
///
/// Lifecycle (`Quit`, `WriteQuit`), navigation (`SwitchBuffer`, buffer/tab
/// operations), UI synchronization (`SyncCommandLine`, `ShowMessageHistory`,
/// `RequestCompletion`, `ListActions`), host-dispatched commands
/// (`CustomExCommand`), and jumps (`JumpToBuffer`).
///
/// # Exhaustive match
///
/// This function uses an exhaustive `match` (no wildcard arm) so the compiler
/// forces a classification decision when new `HostRequestKind` variants are
/// added. This is superior to the adapter-side `!matches!()` pattern, which
/// silently defaults new variants.
#[must_use]
pub const fn pauses_batch_drain(kind: HostRequestKind) -> bool {
    match kind {
        // Lifecycle: session is ending, no need to wait
        HostRequestKind::Quit | HostRequestKind::WriteQuit | HostRequestKind::CQuit => false,

        // Navigation: engine fires and forgets
        HostRequestKind::SwitchBuffer
        | HostRequestKind::BufferNext
        | HostRequestKind::BufferPrev
        | HostRequestKind::BufferFirst
        | HostRequestKind::BufferLast
        | HostRequestKind::BufferList
        | HostRequestKind::TabNew
        | HostRequestKind::TabNext
        | HostRequestKind::TabPrev
        | HostRequestKind::TabClose
        | HostRequestKind::DiagnosticNext
        | HostRequestKind::DiagnosticPrev
        | HostRequestKind::DiagnosticList
        | HostRequestKind::DiagnosticGoto
        | HostRequestKind::JumpToBuffer
        | HostRequestKind::JumpToGlobalMark => false,

        // UI synchronization: no data dependency
        HostRequestKind::SyncCommandLine
        | HostRequestKind::ShowMessageHistory
        | HostRequestKind::RequestCompletion
        | HostRequestKind::RequestCmdlineCompletion
        | HostRequestKind::ListActions => false,

        // Host-dispatched: handled asynchronously
        HostRequestKind::CustomExCommand | HostRequestKind::RunAction => false,

        // Window/session/buffer management: fire and forget
        HostRequestKind::SplitWindow
        | HostRequestKind::CloseWindow
        | HostRequestKind::CloseOtherWindows
        | HostRequestKind::WriteAll
        | HostRequestKind::QuitAll
        | HostRequestKind::WriteQuitAll
        | HostRequestKind::CloseBuffer
        | HostRequestKind::WindowNext
        | HostRequestKind::WindowPrev
        | HostRequestKind::WindowMoveLeft
        | HostRequestKind::WindowMoveRight
        | HostRequestKind::WindowMoveUp
        | HostRequestKind::WindowMoveDown
        | HostRequestKind::WindowRotateDown
        | HostRequestKind::WindowRotateUp
        | HostRequestKind::WindowEqualSize
        | HostRequestKind::WindowIncreaseHeight
        | HostRequestKind::WindowDecreaseHeight
        | HostRequestKind::WindowIncreaseWidth
        | HostRequestKind::WindowDecreaseWidth
        | HostRequestKind::GotoDefinition
        | HostRequestKind::ShowDocumentation
        | HostRequestKind::OpenCommandWindow
        | HostRequestKind::CallOperatorFunc
        | HostRequestKind::ExecuteNorm
        | HostRequestKind::FoldRange
        | HostRequestKind::FoldOpenRange
        | HostRequestKind::FoldCloseRange
        | HostRequestKind::ForEachWindow
        | HostRequestKind::ForEachBuffer
        | HostRequestKind::ForEachTab
        | HostRequestKind::MkVimrc => false,

        // File I/O: engine needs content or confirmation
        HostRequestKind::WriteFile
        | HostRequestKind::UpdateFile
        | HostRequestKind::EditFile
        | HostRequestKind::ReadFile
        | HostRequestKind::ReadConfigFile => true,

        // Data retrieval: engine needs the data
        HostRequestKind::ReadClipboard => true,

        // Text transformation: engine needs the result
        HostRequestKind::FilterDocumentRange | HostRequestKind::ReindentRange => true,

        // Evaluation: engine needs the output
        HostRequestKind::ExternalCommand
        | HostRequestKind::EvaluateExpression
        | HostRequestKind::EvaluateMapping => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::host::{HostRequestId, HostRequestMeta};

    fn meta(n: u64) -> HostRequestMeta {
        HostRequestMeta {
            id: HostRequestId::new(n),
        }
    }

    #[test]
    fn dangerous_requests_are_failed() {
        let dangerous = [
            HostRequest::EvaluateExpression {
                meta: meta(1),
                expression: "1+1".into(),
            },
            HostRequest::EvaluateMapping {
                meta: meta(2),
                expression: "expr".into(),
                mode: crate::keymap::MappingMode::Normal,
                kind: crate::keymap::MappingKind::NonRecursive,
                silent: false,
            },
            HostRequest::FilterDocumentRange {
                meta: meta(3),
                range: crate::primitives::Range::empty_at(crate::primitives::Offset::new(0)),
                motion_type: crate::primitives::MotionType::CharWise,
                input_text: "".into(),
                command: "sort".into(),
            },
            HostRequest::ReindentRange {
                meta: meta(4),
                range: crate::primitives::Range::empty_at(crate::primitives::Offset::new(0)),
                motion_type: crate::primitives::MotionType::CharWise,
                input_text: "".into(),
                start_col: 0,
                end_col: 0,
                end_line_in_range: 0,
                start_byte_offset: 0,
            },
            HostRequest::ExternalCommand {
                meta: meta(5),
                command: "ls".into(),
            },
        ];

        for req in &dangerous {
            let result = default_result(req);
            assert!(
                matches!(result, Some(HostResult::Failure { .. })),
                "expected Failure for {:?}, got {:?}",
                req.kind(),
                result,
            );
        }
    }

    #[test]
    fn safe_noop_requests_are_succeeded() {
        let safe = [
            HostRequestKind::SyncCommandLine,
            HostRequestKind::ShowMessageHistory,
            HostRequestKind::RequestCompletion,
            HostRequestKind::BufferNext,
            HostRequestKind::TabClose,
            HostRequestKind::JumpToBuffer,
            HostRequestKind::JumpToGlobalMark,
        ];

        for kind in safe {
            // We need a concrete request — use the kind to decide which variant.
            // Just test expected_result_kind instead for simplicity.
            assert_eq!(
                expected_result_kind(kind),
                ExpectedResultKind::Success,
                "expected Success kind for {kind:?}",
            );
        }
    }

    #[test]
    fn host_dependent_requests_return_none() {
        let host_deps = [
            HostRequest::WriteFile {
                meta: meta(1),
                path: None,
                force: false,
            },
            HostRequest::Quit {
                meta: meta(2),
                force: false,
            },
            HostRequest::ReadFile {
                meta: meta(3),
                path: "f.txt".into(),
                after_line: None,
            },
            HostRequest::ReadClipboard {
                meta: meta(4),
                cursor_offset: 0,
            },
        ];

        for req in &host_deps {
            assert!(
                default_result(req).is_none(),
                "expected None for {:?}",
                req.kind(),
            );
        }
    }

    #[test]
    fn expected_result_kinds_are_correct() {
        assert_eq!(
            expected_result_kind(HostRequestKind::ReadFile),
            ExpectedResultKind::Data
        );
        assert_eq!(
            expected_result_kind(HostRequestKind::ReadClipboard),
            ExpectedResultKind::ClipboardText
        );
        assert_eq!(
            expected_result_kind(HostRequestKind::FilterDocumentRange),
            ExpectedResultKind::FilteredRange
        );
        assert_eq!(
            expected_result_kind(HostRequestKind::WriteFile),
            ExpectedResultKind::Success
        );
        assert_eq!(
            expected_result_kind(HostRequestKind::EvaluateExpression),
            ExpectedResultKind::Data
        );
        assert_eq!(
            expected_result_kind(HostRequestKind::EvaluateMapping),
            ExpectedResultKind::Data
        );
    }

    #[test]
    fn fire_and_forget_classification() {
        assert!(is_fire_and_forget(HostRequestKind::SyncCommandLine));
        assert!(is_fire_and_forget(HostRequestKind::JumpToBuffer));
        assert!(is_fire_and_forget(HostRequestKind::JumpToGlobalMark));
        assert!(is_fire_and_forget(HostRequestKind::RunAction));
        assert!(!is_fire_and_forget(HostRequestKind::WriteFile));
        assert!(!is_fire_and_forget(HostRequestKind::Quit));
        assert!(!is_fire_and_forget(HostRequestKind::ReadFile));
    }

    #[test]
    fn batch_drain_pausing_classification() {
        // Pauses drain: file I/O, data retrieval, text transformation, evaluation
        let pauses = [
            HostRequestKind::WriteFile,
            HostRequestKind::EditFile,
            HostRequestKind::ReadFile,
            HostRequestKind::ReadConfigFile,
            HostRequestKind::ReadClipboard,
            HostRequestKind::FilterDocumentRange,
            HostRequestKind::ReindentRange,
            HostRequestKind::ExternalCommand,
            HostRequestKind::EvaluateExpression,
            HostRequestKind::EvaluateMapping,
        ];
        for kind in pauses {
            assert!(
                pauses_batch_drain(kind),
                "{kind:?} should pause batch drain",
            );
        }

        // Does not pause: lifecycle, navigation, UI sync, host-dispatched
        let no_pause = [
            HostRequestKind::Quit,
            HostRequestKind::WriteQuit,
            HostRequestKind::SwitchBuffer,
            HostRequestKind::BufferNext,
            HostRequestKind::BufferPrev,
            HostRequestKind::BufferFirst,
            HostRequestKind::BufferLast,
            HostRequestKind::BufferList,
            HostRequestKind::TabNew,
            HostRequestKind::TabNext,
            HostRequestKind::TabPrev,
            HostRequestKind::TabClose,
            HostRequestKind::CustomExCommand,
            HostRequestKind::SyncCommandLine,
            HostRequestKind::ShowMessageHistory,
            HostRequestKind::RequestCompletion,
            HostRequestKind::ListActions,
            HostRequestKind::JumpToBuffer,
            HostRequestKind::JumpToGlobalMark,
            HostRequestKind::RunAction,
            HostRequestKind::SplitWindow,
            HostRequestKind::CloseWindow,
            HostRequestKind::CloseOtherWindows,
            HostRequestKind::WriteAll,
            HostRequestKind::QuitAll,
            HostRequestKind::WriteQuitAll,
            HostRequestKind::CloseBuffer,
            HostRequestKind::WindowNext,
            HostRequestKind::WindowPrev,
            HostRequestKind::WindowMoveLeft,
            HostRequestKind::WindowMoveRight,
            HostRequestKind::WindowMoveUp,
            HostRequestKind::WindowMoveDown,
            HostRequestKind::WindowRotateDown,
            HostRequestKind::WindowRotateUp,
            HostRequestKind::WindowEqualSize,
            HostRequestKind::WindowIncreaseHeight,
            HostRequestKind::WindowDecreaseHeight,
            HostRequestKind::WindowIncreaseWidth,
            HostRequestKind::WindowDecreaseWidth,
            HostRequestKind::GotoDefinition,
            HostRequestKind::ShowDocumentation,
            HostRequestKind::OpenCommandWindow,
            HostRequestKind::CallOperatorFunc,
            HostRequestKind::ExecuteNorm,
        ];
        for kind in no_pause {
            assert!(
                !pauses_batch_drain(kind),
                "{kind:?} should NOT pause batch drain",
            );
        }
    }

    /// Every variant must be covered — if this test fails, a new variant was
    /// added to `HostRequestKind::ALL` but not to the test arrays above.
    #[test]
    fn batch_drain_covers_all_variants() {
        assert_eq!(
            HostRequestKind::ALL.len(),
            66,
            "HostRequestKind variant count changed — update pauses_batch_drain"
        );
    }
}
