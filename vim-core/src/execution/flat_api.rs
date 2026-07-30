//! [`FlatApi`] — flat function projection layer for external bindings.
//!
//! Wraps [`VimApi`] domain accessors as flat functions with type conversions
//! suitable for Rhai, FFI, and WASM consumption.
//!
//! This is the single source of truth for all external binding layers:
//! - Rhai registers these as global functions
//! - FFI wraps them in `extern "C"` shims
//! - WASM exports them as named functions
//!
//! # Design Principles
//!
//! - **i64 everywhere**: All sizes/offsets use `i64` (Rhai's native integer type).
//! - **-1 sentinel**: "Not found" returns -1 (doc-16 spec convention).
//! - **Scope parsing**: Accepts short (`"g"`) and long (`"global"`) scope strings.
//! - **FlatResult**: Success value or human-readable error string.

#![allow(clippy::redundant_closure_for_method_calls)]

use compact_str::CompactString;

use crate::execution::api::VimApi;
use crate::primitives::byte_delta::{offset_from_i64, to_i64};
use crate::primitives::{MotionType, VarScope, VimValue};

/// Result type for flat API functions.
///
/// External bindings convert this to their native error handling:
/// - Rhai: `Dynamic` / exception
/// - FFI: error code + out-param
/// - WASM: JSON envelope
pub type FlatResult<T> = Result<T, CompactString>;

/// Flat API projection — wraps [`VimApi`] domain accessors as flat functions.
///
/// Zero-cost abstraction layer between the typed Rust API and external bindings.
/// All methods are static, taking `&VimApi` as first argument.
pub struct FlatApi;

impl FlatApi {
    // ── Buffer domain ──────────────────────────────────────────────────

    /// Full document text as an owned `String`.
    #[inline]
    pub fn buf_text(api: &VimApi<'_>) -> String {
        api.buffer().text().to_owned()
    }

    /// Byte length of the document.
    #[inline]
    pub fn buf_len(api: &VimApi<'_>) -> i64 {
        to_i64(api.buffer().len())
    }

    /// Number of lines in the document.
    #[inline]
    pub fn buf_line_count(api: &VimApi<'_>) -> i64 {
        to_i64(api.buffer().line_count())
    }

    /// Content of the nth line (0-indexed).
    ///
    /// # Errors
    ///
    /// Returns `"line out of range"` if `n` does not name a line of the
    /// document — that is, `n >= buf_line_count`. Negative `n` is cast to
    /// `usize` and wraps to a huge index, so it fails the same way.
    #[inline]
    pub fn buf_line(api: &VimApi<'_>, n: i64) -> FlatResult<String> {
        api.buffer()
            .line(offset_from_i64(n))
            .map(str::to_owned)
            .ok_or_else(|| CompactString::from("line out of range"))
    }

    /// Content of the line containing `offset` (without trailing newline).
    #[inline]
    pub fn buf_line_at(api: &VimApi<'_>, offset: i64) -> String {
        api.buffer().line_at(offset_from_i64(offset)).to_owned()
    }

    /// Line number (0-indexed) containing `offset`.
    #[inline]
    pub fn buf_line_number(api: &VimApi<'_>, offset: i64) -> i64 {
        to_i64(api.buffer().line_number(offset_from_i64(offset)))
    }

    /// Byte offset of the start of line `n`.
    #[inline]
    pub fn buf_line_start(api: &VimApi<'_>, n: i64) -> i64 {
        to_i64(api.buffer().line_start(offset_from_i64(n)))
    }

    /// Byte offset of the end of line `n` (before `\n`).
    #[inline]
    pub fn buf_line_end(api: &VimApi<'_>, n: i64) -> i64 {
        to_i64(api.buffer().line_end(offset_from_i64(n)))
    }

    /// Substring from `start` to `end`, clamped to document bounds.
    #[inline]
    pub fn buf_slice(api: &VimApi<'_>, start: i64, end: i64) -> String {
        api.buffer()
            .slice(offset_from_i64(start), offset_from_i64(end))
            .to_owned()
    }

    /// Character at the given byte offset.
    ///
    /// # Errors
    ///
    /// Returns `"offset out of range"` if there is no character at `offset`:
    /// it is at or past the end of the document. Negative `offset` is cast to
    /// `usize` and wraps past the end, so it fails the same way.
    #[inline]
    pub fn buf_char_at(api: &VimApi<'_>, offset: i64) -> FlatResult<String> {
        api.buffer()
            .char_at(offset_from_i64(offset))
            .map(|c| c.to_string())
            .ok_or_else(|| CompactString::from("offset out of range"))
    }

    /// UTF-8 byte length of the character at `offset`.
    #[inline]
    pub fn buf_char_len_at(api: &VimApi<'_>, offset: i64) -> i64 {
        to_i64(api.buffer().char_len_at(offset_from_i64(offset)))
    }

    /// Whether the byte at `offset` is an ASCII word character.
    #[inline]
    pub fn buf_is_word_byte(api: &VimApi<'_>, offset: i64) -> bool {
        api.buffer().is_word_byte(offset_from_i64(offset))
    }

    /// Find the next occurrence of a literal string after `offset`.
    ///
    /// Returns -1 if not found (Rhai convention).
    #[inline]
    pub fn buf_find_forward(api: &VimApi<'_>, offset: i64, pattern: &str) -> i64 {
        api.buffer()
            .find_forward(offset_from_i64(offset), pattern)
            .map_or(-1, to_i64)
    }

    /// Find the previous occurrence of a literal string before `offset`.
    ///
    /// Returns -1 if not found (Rhai convention).
    #[inline]
    pub fn buf_find_backward(api: &VimApi<'_>, offset: i64, pattern: &str) -> i64 {
        api.buffer()
            .find_backward(offset_from_i64(offset), pattern)
            .map_or(-1, to_i64)
    }

    /// Find the word boundaries containing `offset`.
    ///
    /// Returns `(start, end)` as an `i64` pair, where `end` is one past the
    /// last word byte.
    ///
    /// # Errors
    ///
    /// Returns `"no word at offset"` if `offset` is at or past the end of the
    /// document, or if the byte at `offset` is not a word byte (ASCII
    /// alphanumeric or `_`) and so starts no word.
    #[inline]
    pub fn buf_word_at(api: &VimApi<'_>, offset: i64) -> FlatResult<(i64, i64)> {
        api.buffer()
            .word_at(offset_from_i64(offset))
            .map(|(s, e)| (to_i64(s), to_i64(e)))
            .ok_or_else(|| CompactString::from("no word at offset"))
    }

    /// Count of leading whitespace bytes on line `n`.
    #[inline]
    pub fn buf_line_indent(api: &VimApi<'_>, n: i64) -> i64 {
        to_i64(api.buffer().line_indent(offset_from_i64(n)))
    }

    // ── Cursor domain ──────────────────────────────────────────────────

    /// Current cursor byte offset.
    #[inline]
    pub fn cursor_offset(api: &VimApi<'_>) -> i64 {
        to_i64(api.cursor().offset())
    }

    // ── State domain ───────────────────────────────────────────────────

    /// Current editing mode as a debug string (e.g., `"Normal"`, `"Insert"`).
    #[inline]
    pub fn mode(api: &VimApi<'_>) -> String {
        format!("{:?}", api.state().mode())
    }

    /// Current search pattern.
    ///
    /// # Errors
    ///
    /// Returns `"no search pattern"` when no pattern is active — nothing in
    /// this session has set the search register (`/`, `?`, `*`, `#`, `:s`).
    #[inline]
    pub fn search_pattern(api: &VimApi<'_>) -> FlatResult<String> {
        api.state()
            .search_pattern()
            .map(str::to_owned)
            .ok_or_else(|| CompactString::from("no search pattern"))
    }

    /// Whether a macro is currently being recorded.
    #[inline]
    pub const fn is_recording(api: &VimApi<'_>) -> bool {
        api.state().is_recording()
    }

    // ── Options domain ─────────────────────────────────────────────────

    /// Shift width for indent/outdent.
    #[inline]
    pub fn opt_shiftwidth(api: &VimApi<'_>) -> i64 {
        to_i64(api.options().shiftwidth())
    }

    /// Tab stop width in columns.
    #[inline]
    pub fn opt_tabstop(api: &VimApi<'_>) -> i64 {
        to_i64(api.options().tabstop())
    }

    /// Whether tabs are expanded to spaces.
    #[inline]
    pub const fn opt_expandtab(api: &VimApi<'_>) -> bool {
        api.options().expandtab()
    }

    /// Whether case is ignored in searches.
    #[inline]
    pub const fn opt_ignorecase(api: &VimApi<'_>) -> bool {
        api.options().ignorecase()
    }

    /// Whether uppercase chars override ignorecase.
    #[inline]
    pub const fn opt_smartcase(api: &VimApi<'_>) -> bool {
        api.options().smartcase()
    }

    /// Maximum line width for formatting.
    #[inline]
    pub fn opt_textwidth(api: &VimApi<'_>) -> i64 {
        to_i64(api.options().textwidth())
    }

    /// Comment string format (e.g., `"// %s"`).
    #[inline]
    pub fn opt_commentstring(api: &VimApi<'_>) -> String {
        api.options().commentstring().to_owned()
    }

    /// Characters that form keywords (Vim's `iskeyword` option).
    #[inline]
    pub fn opt_iskeyword(api: &VimApi<'_>) -> String {
        api.options().iskeyword().to_owned()
    }

    // ── Register domain ────────────────────────────────────────────────

    /// Get register content by name character string.
    ///
    /// Only the first character of `name` is significant.
    ///
    /// # Errors
    ///
    /// Returns `"empty register name"` if `name` is the empty string, so there
    /// is no character to interpret.
    ///
    /// Returns `"register empty or not found"` if that first character is not
    /// a register name Vim recognises, or names a register that currently
    /// holds nothing.
    #[inline]
    pub fn reg_get(api: &VimApi<'_>, name: &str) -> FlatResult<String> {
        let ch = name
            .chars()
            .next()
            .ok_or_else(|| CompactString::from("empty register name"))?;
        api.registers()
            .get(ch)
            .map(str::to_owned)
            .ok_or_else(|| CompactString::from("register empty or not found"))
    }

    // ── Mark domain ────────────────────────────────────────────────────

    /// Get mark offset by name character string.
    ///
    /// Returns -1 if the mark is not set or name is invalid.
    #[inline]
    pub fn mark_get(api: &VimApi<'_>, name: &str) -> i64 {
        let ch = name.chars().next().unwrap_or('\0');
        api.marks().get(ch).map_or(-1, to_i64)
    }

    // ── Variable domain ────────────────────────────────────────────────

    /// Get a variable by scope string and name.
    ///
    /// Scope accepts: `"g"`, `"global"`, `"b"`, `"buffer"`.
    ///
    /// # Errors
    ///
    /// Returns `"unknown scope: <scope>"` if `scope` is not one of those four
    /// spellings.
    ///
    /// Returns `"variable not found"` if the scope is valid but holds no
    /// variable called `name`.
    #[inline]
    pub fn var_get(api: &VimApi<'_>, scope: &str, name: &str) -> FlatResult<VimValue> {
        let scope = parse_scope(scope)?;
        api.variables()
            .get(scope, name)
            .cloned()
            .ok_or_else(|| CompactString::from("variable not found"))
    }

    /// Check whether a variable exists in the given scope.
    ///
    /// Scope accepts: `"g"`, `"global"`, `"b"`, `"buffer"`.
    ///
    /// # Errors
    ///
    /// Returns `"unknown scope: <scope>"` if `scope` is not one of those four
    /// spellings. This is the only failure: a variable that is simply not set
    /// is reported as `Ok(false)`.
    #[inline]
    pub fn var_exists(api: &VimApi<'_>, scope: &str, name: &str) -> FlatResult<bool> {
        let scope = parse_scope(scope)?;
        Ok(api.variables().exists(scope, name))
    }

    // ── Effect emission ────────────────────────────────────────────────

    /// Insert text at `offset`.
    ///
    /// # Errors
    ///
    /// Returns `"insufficient capability tier: required Mutating, have
    /// ReadOnly"` when the caller holds only the read-only tier. That is the
    /// only way this call fails; the arguments are not validated here.
    #[inline]
    pub fn emit_insert(api: &VimApi<'_>, offset: i64, text: &str) -> FlatResult<()> {
        api.emit()
            .insert(offset_from_i64(offset), text)
            .map_err(|e| CompactString::from(e.to_string()))
    }

    /// Delete the range `start..end`.
    ///
    /// # Errors
    ///
    /// Returns `"insufficient capability tier: required Mutating, have
    /// ReadOnly"` when the caller holds only the read-only tier. That is the
    /// only way this call fails; the arguments are not validated here.
    #[inline]
    pub fn emit_delete(api: &VimApi<'_>, start: i64, end: i64) -> FlatResult<()> {
        api.emit()
            .delete(offset_from_i64(start), offset_from_i64(end))
            .map_err(|e| CompactString::from(e.to_string()))
    }

    /// Replace the range `start..end` with `text`.
    ///
    /// # Errors
    ///
    /// Returns `"insufficient capability tier: required Mutating, have
    /// ReadOnly"` when the caller holds only the read-only tier. That is the
    /// only way this call fails; the arguments are not validated here.
    #[inline]
    pub fn emit_replace(api: &VimApi<'_>, start: i64, end: i64, text: &str) -> FlatResult<()> {
        api.emit()
            .replace(offset_from_i64(start), offset_from_i64(end), text)
            .map_err(|e| CompactString::from(e.to_string()))
    }

    /// Set cursor to `offset`.
    ///
    /// # Errors
    ///
    /// Never returns an error: moving the cursor is permitted at every capability tier.
    /// The `FlatResult` is kept so that all `emit_*` projections share one
    /// signature across the Rhai, FFI and WASM bindings.
    #[inline]
    pub fn emit_cursor(api: &VimApi<'_>, offset: i64) -> FlatResult<()> {
        api.emit()
            .set_cursor(offset_from_i64(offset))
            .map_err(|e| CompactString::from(e.to_string()))
    }

    /// Show an informational message.
    ///
    /// # Errors
    ///
    /// Never returns an error: showing a message is permitted at every capability tier.
    /// The `FlatResult` is kept so that all `emit_*` projections share one
    /// signature across the Rhai, FFI and WASM bindings.
    #[inline]
    pub fn emit_message(api: &VimApi<'_>, text: &str) -> FlatResult<()> {
        api.emit()
            .message(text)
            .map_err(|e| CompactString::from(e.to_string()))
    }

    /// Show an error message.
    ///
    /// # Errors
    ///
    /// Never returns an error: showing an error is permitted at every capability tier.
    /// The `FlatResult` is kept so that all `emit_*` projections share one
    /// signature across the Rhai, FFI and WASM bindings.
    #[inline]
    pub fn emit_error(api: &VimApi<'_>, text: &str) -> FlatResult<()> {
        api.emit()
            .error(text)
            .map_err(|e| CompactString::from(e.to_string()))
    }

    /// Set register content.
    ///
    /// `linewise` controls whether the content is treated as line-wise or char-wise.
    /// Only the first character of `name` is significant.
    ///
    /// # Errors
    ///
    /// Returns `"empty register name"` if `name` is the empty string.
    ///
    /// Returns `"insufficient capability tier: required Mutating, have
    /// ReadOnly"` if the caller holds only the read-only tier, or
    /// `"register not found: <c>"` if the first character of `name` is not a
    /// register name Vim recognises.
    #[inline]
    pub fn emit_set_register(
        api: &VimApi<'_>,
        name: &str,
        text: &str,
        linewise: bool,
    ) -> FlatResult<()> {
        let ch = name
            .chars()
            .next()
            .ok_or_else(|| CompactString::from("empty register name"))?;
        let motion_type = if linewise {
            MotionType::LineWise
        } else {
            MotionType::CharWise
        };
        api.emit()
            .set_register(ch, text, motion_type)
            .map_err(|e| CompactString::from(e.to_string()))
    }

    /// Set a mark to the given offset.
    ///
    /// Only the first character of `name` is significant.
    ///
    /// # Errors
    ///
    /// Returns `"empty mark name"` if `name` is the empty string.
    ///
    /// Returns `"insufficient capability tier: required Mutating, have
    /// ReadOnly"` if the caller holds only the read-only tier, or
    /// `"mark not found: <c>"` if the first character of `name` is not a mark
    /// name Vim recognises. `offset` itself is not range-checked here.
    #[inline]
    pub fn emit_set_mark(api: &VimApi<'_>, name: &str, offset: i64) -> FlatResult<()> {
        let ch = name
            .chars()
            .next()
            .ok_or_else(|| CompactString::from("empty mark name"))?;
        api.emit()
            .set_mark(ch, offset_from_i64(offset))
            .map_err(|e| CompactString::from(e.to_string()))
    }

    /// Set a variable in the given scope.
    ///
    /// Scope accepts: `"g"`, `"global"`, `"b"`, `"buffer"`.
    ///
    /// # Errors
    ///
    /// Returns `"unknown scope: <scope>"` if `scope` is not one of those four
    /// spellings.
    ///
    /// Returns `"insufficient capability tier: required Mutating, have
    /// ReadOnly"` if the caller holds only the read-only tier, or
    /// `variable not found: "" in scope <scope>` if `name` is empty — the
    /// empty string is not a legal variable name.
    #[inline]
    pub fn emit_set_variable(
        api: &VimApi<'_>,
        scope: &str,
        name: &str,
        value: VimValue,
    ) -> FlatResult<()> {
        let scope = parse_scope(scope)?;
        api.emit()
            .set_variable(scope, name, value)
            .map_err(|e| CompactString::from(e.to_string()))
    }

    /// Delete a variable in the given scope.
    ///
    /// Scope accepts: `"g"`, `"global"`, `"b"`, `"buffer"`.
    ///
    /// # Errors
    ///
    /// Returns `"unknown scope: <scope>"` if `scope` is not one of those four
    /// spellings.
    ///
    /// Returns `"insufficient capability tier: required Mutating, have
    /// ReadOnly"` if the caller holds only the read-only tier, or
    /// `variable not found: "" in scope <scope>` if `name` is empty.
    /// Deleting a variable that was never set is *not* an error.
    #[inline]
    pub fn emit_delete_variable(api: &VimApi<'_>, scope: &str, name: &str) -> FlatResult<()> {
        let scope = parse_scope(scope)?;
        api.emit()
            .delete_variable(scope, name)
            .map_err(|e| CompactString::from(e.to_string()))
    }

    /// Set a highlight over a byte range.
    ///
    /// Emits [`Effect::SetHighlightRange`](crate::effects::Effect::SetHighlightRange) with the
    /// given range and group name.
    /// Allowed at [`CapabilityTier::ReadOnly`](crate::primitives::CapabilityTier::ReadOnly).
    ///
    /// # Errors
    ///
    /// Never returns an error: highlighting a range is permitted at every capability tier.
    /// The `FlatResult` is kept so that all `emit_*` projections share one
    /// signature across the Rhai, FFI and WASM bindings.
    /// `start`, `end` and `group` are not validated here; an inverted or
    /// out-of-range span is resolved when the effect is applied.
    #[inline]
    pub fn emit_set_highlight(
        api: &VimApi<'_>,
        start: i64,
        end: i64,
        group: &str,
    ) -> FlatResult<()> {
        api.emit()
            .set_highlight(offset_from_i64(start), offset_from_i64(end), group)
            .map_err(|e| CompactString::from(e.to_string()))
    }

    /// Begin an undo group.
    ///
    /// # Errors
    ///
    /// Returns `"insufficient capability tier: required Mutating, have
    /// ReadOnly"` when the caller holds only the read-only tier. That is the
    /// only way this call fails; the arguments are not validated here.
    #[inline]
    pub fn emit_begin_undo(api: &VimApi<'_>) -> FlatResult<()> {
        api.emit()
            .begin_undo_group()
            .map_err(|e| CompactString::from(e.to_string()))
    }

    /// End an undo group.
    ///
    /// # Errors
    ///
    /// Returns `"insufficient capability tier: required Mutating, have
    /// ReadOnly"` when the caller holds only the read-only tier. That is the
    /// only way this call fails; the arguments are not validated here.
    #[inline]
    pub fn emit_end_undo(api: &VimApi<'_>) -> FlatResult<()> {
        api.emit()
            .end_undo_group()
            .map_err(|e| CompactString::from(e.to_string()))
    }
}

/// Parse a scope string into [`VarScope`].
///
/// Accepts both short (`"g"`, `"b"`) and long (`"global"`, `"buffer"`) forms.
fn parse_scope(s: &str) -> FlatResult<VarScope> {
    match s {
        "g" | "global" => Ok(VarScope::Global),
        "b" | "buffer" => Ok(VarScope::Buffer),
        _ => Err(CompactString::from(format!("unknown scope: {s}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::api::InvocationContext;
    use crate::execution::HostSession;
    use crate::primitives::{CallerId, CapabilityTier};

    /// Helper: build a mutating invocation context for tests.
    fn host_ctx() -> InvocationContext {
        InvocationContext::new(CallerId::Host, CapabilityTier::Mutating)
    }

    // ── Buffer domain tests ────────────────────────────────────────────

    #[test]
    fn flat_buf_text() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert_eq!(FlatApi::buf_text(&api), "hello");
    }

    #[test]
    fn flat_buf_len() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert_eq!(FlatApi::buf_len(&api), 5);
    }

    #[test]
    fn flat_buf_line_count() {
        let session = HostSession::new("a\nb\nc");
        let api = VimApi::from_session(&session, host_ctx());
        assert_eq!(FlatApi::buf_line_count(&api), 3);
    }

    #[test]
    fn flat_buf_line_valid() {
        let session = HostSession::new("alpha\nbeta");
        let api = VimApi::from_session(&session, host_ctx());
        assert_eq!(FlatApi::buf_line(&api, 0).unwrap(), "alpha\n");
        assert_eq!(FlatApi::buf_line(&api, 1).unwrap(), "beta");
    }

    #[test]
    fn flat_buf_line_out_of_range() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert!(FlatApi::buf_line(&api, 5).is_err());
    }

    #[test]
    fn flat_buf_line_at() {
        let session = HostSession::new("alpha\nbeta");
        let api = VimApi::from_session(&session, host_ctx());
        assert_eq!(FlatApi::buf_line_at(&api, 6), "beta");
    }

    #[test]
    fn flat_buf_line_number() {
        let session = HostSession::new("alpha\nbeta\ngamma");
        let api = VimApi::from_session(&session, host_ctx());
        assert_eq!(FlatApi::buf_line_number(&api, 0), 0);
        assert_eq!(FlatApi::buf_line_number(&api, 6), 1);
        assert_eq!(FlatApi::buf_line_number(&api, 11), 2);
    }

    #[test]
    fn flat_buf_line_start_end() {
        let session = HostSession::new("abc\ndef");
        let api = VimApi::from_session(&session, host_ctx());
        assert_eq!(FlatApi::buf_line_start(&api, 0), 0);
        assert_eq!(FlatApi::buf_line_start(&api, 1), 4);
        assert_eq!(FlatApi::buf_line_end(&api, 0), 3);
        assert_eq!(FlatApi::buf_line_end(&api, 1), 7);
    }

    #[test]
    fn flat_buf_slice() {
        let session = HostSession::new("hello world");
        let api = VimApi::from_session(&session, host_ctx());
        assert_eq!(FlatApi::buf_slice(&api, 0, 5), "hello");
        assert_eq!(FlatApi::buf_slice(&api, 6, 11), "world");
    }

    #[test]
    fn flat_buf_char_at() {
        let session = HostSession::new("abc");
        let api = VimApi::from_session(&session, host_ctx());
        assert_eq!(FlatApi::buf_char_at(&api, 0).unwrap(), "a");
        assert_eq!(FlatApi::buf_char_at(&api, 2).unwrap(), "c");
        assert!(FlatApi::buf_char_at(&api, 10).is_err());
    }

    #[test]
    fn flat_buf_char_len_at() {
        let session = HostSession::new("café");
        let api = VimApi::from_session(&session, host_ctx());
        assert_eq!(FlatApi::buf_char_len_at(&api, 0), 1); // 'c'
        assert_eq!(FlatApi::buf_char_len_at(&api, 3), 2); // 'é' is 2 bytes
    }

    #[test]
    fn flat_buf_is_word_byte() {
        let session = HostSession::new("a b");
        let api = VimApi::from_session(&session, host_ctx());
        assert!(FlatApi::buf_is_word_byte(&api, 0));
        assert!(!FlatApi::buf_is_word_byte(&api, 1)); // space
        assert!(FlatApi::buf_is_word_byte(&api, 2));
    }

    #[test]
    fn flat_buf_find_forward() {
        let session = HostSession::new("hello world");
        let api = VimApi::from_session(&session, host_ctx());
        assert_eq!(FlatApi::buf_find_forward(&api, 0, "world"), 6);
    }

    #[test]
    fn flat_buf_find_forward_not_found() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert_eq!(FlatApi::buf_find_forward(&api, 0, "xyz"), -1);
    }

    #[test]
    fn flat_buf_find_backward() {
        let session = HostSession::new("hello hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert_eq!(FlatApi::buf_find_backward(&api, 11, "hello"), 6);
    }

    #[test]
    fn flat_buf_find_backward_not_found() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert_eq!(FlatApi::buf_find_backward(&api, 5, "xyz"), -1);
    }

    #[test]
    fn flat_buf_word_at() {
        let session = HostSession::new("foo bar");
        let api = VimApi::from_session(&session, host_ctx());
        let (start, end) = FlatApi::buf_word_at(&api, 0).unwrap();
        assert_eq!(start, 0);
        assert_eq!(end, 3);
    }

    #[test]
    fn flat_buf_word_at_no_word() {
        let session = HostSession::new("foo bar");
        let api = VimApi::from_session(&session, host_ctx());
        assert!(FlatApi::buf_word_at(&api, 3).is_err()); // space
    }

    #[test]
    fn flat_buf_line_indent() {
        let session = HostSession::new("    indented");
        let api = VimApi::from_session(&session, host_ctx());
        assert_eq!(FlatApi::buf_line_indent(&api, 0), 4);
    }

    // ── Cursor domain tests ────────────────────────────────────────────

    #[test]
    fn flat_cursor_offset() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert_eq!(FlatApi::cursor_offset(&api), 0);
    }

    // ── State domain tests ─────────────────────────────────────────────

    #[test]
    fn flat_mode() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert_eq!(FlatApi::mode(&api), "Normal");
    }

    #[test]
    fn flat_search_pattern_none() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert!(FlatApi::search_pattern(&api).is_err());
    }

    #[test]
    fn flat_is_recording() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert!(!FlatApi::is_recording(&api));
    }

    // ── Options domain tests ───────────────────────────────────────────

    #[test]
    fn flat_options() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert!(FlatApi::opt_shiftwidth(&api) > 0);
        assert!(FlatApi::opt_tabstop(&api) > 0);
        assert!(FlatApi::opt_textwidth(&api) >= 0);
    }

    #[test]
    fn flat_opt_booleans() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        // Just verify they return without panicking and are boolean.
        let _ = FlatApi::opt_expandtab(&api);
        let _ = FlatApi::opt_ignorecase(&api);
        let _ = FlatApi::opt_smartcase(&api);
    }

    #[test]
    fn flat_opt_strings() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        // Verify string options return non-empty defaults.
        let cms = FlatApi::opt_commentstring(&api);
        assert!(!cms.is_empty());
        let isk = FlatApi::opt_iskeyword(&api);
        assert!(!isk.is_empty());
    }

    // ── Register domain tests ──────────────────────────────────────────

    #[test]
    fn flat_reg_get_empty() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        // No register has been set, so get should fail.
        assert!(FlatApi::reg_get(&api, "a").is_err());
    }

    #[test]
    fn flat_reg_get_empty_name() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert!(FlatApi::reg_get(&api, "").is_err());
    }

    // ── Mark domain tests ──────────────────────────────────────────────

    #[test]
    fn flat_mark_get_unset() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert_eq!(FlatApi::mark_get(&api, "a"), -1);
    }

    // ── Variable domain tests ──────────────────────────────────────────

    #[test]
    fn flat_var_scope_parsing_invalid() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert!(FlatApi::var_get(&api, "x", "foo").is_err());
    }

    #[test]
    fn flat_var_get_missing() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert!(FlatApi::var_get(&api, "g", "nonexistent").is_err());
    }

    #[test]
    fn flat_var_exists_missing() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert_eq!(
            FlatApi::var_exists(&api, "g", "nonexistent").unwrap(),
            false
        );
    }

    #[test]
    fn flat_var_exists_invalid_scope() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert!(FlatApi::var_exists(&api, "z", "foo").is_err());
    }

    // ── Effect emission tests ──────────────────────────────────────────

    #[test]
    fn flat_emit_insert() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert!(FlatApi::emit_insert(&api, 5, " world").is_ok());
        let effects = api.drain_effects();
        assert_eq!(effects.len(), 1);
    }

    #[test]
    fn flat_emit_delete() {
        let session = HostSession::new("hello world");
        let api = VimApi::from_session(&session, host_ctx());
        assert!(FlatApi::emit_delete(&api, 5, 11).is_ok());
        let effects = api.drain_effects();
        assert_eq!(effects.len(), 1);
    }

    #[test]
    fn flat_emit_replace() {
        let session = HostSession::new("hello world");
        let api = VimApi::from_session(&session, host_ctx());
        assert!(FlatApi::emit_replace(&api, 6, 11, "there").is_ok());
        let effects = api.drain_effects();
        assert_eq!(effects.len(), 1);
    }

    #[test]
    fn flat_emit_cursor() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert!(FlatApi::emit_cursor(&api, 3).is_ok());
        let effects = api.drain_effects();
        assert_eq!(effects.len(), 1);
    }

    #[test]
    fn flat_emit_message() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert!(FlatApi::emit_message(&api, "info").is_ok());
        let effects = api.drain_effects();
        assert_eq!(effects.len(), 1);
    }

    #[test]
    fn flat_emit_error() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert!(FlatApi::emit_error(&api, "oops").is_ok());
        let effects = api.drain_effects();
        assert_eq!(effects.len(), 1);
    }

    #[test]
    fn flat_emit_set_register() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert!(FlatApi::emit_set_register(&api, "a", "text", false).is_ok());
        let effects = api.drain_effects();
        assert_eq!(effects.len(), 1);
    }

    #[test]
    fn flat_emit_set_register_empty_name() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert!(FlatApi::emit_set_register(&api, "", "text", false).is_err());
    }

    #[test]
    fn flat_emit_set_mark() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert!(FlatApi::emit_set_mark(&api, "a", 2).is_ok());
        let effects = api.drain_effects();
        assert_eq!(effects.len(), 1);
    }

    #[test]
    fn flat_emit_set_mark_empty_name() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert!(FlatApi::emit_set_mark(&api, "", 0).is_err());
    }

    #[test]
    fn flat_emit_set_variable() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert!(FlatApi::emit_set_variable(&api, "g", "foo", VimValue::Int(42)).is_ok());
        let effects = api.drain_effects();
        assert_eq!(effects.len(), 1);
    }

    #[test]
    fn flat_emit_set_variable_invalid_scope() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert!(FlatApi::emit_set_variable(&api, "z", "foo", VimValue::Int(1)).is_err());
    }

    #[test]
    fn flat_emit_delete_variable() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert!(FlatApi::emit_delete_variable(&api, "b", "foo").is_ok());
        let effects = api.drain_effects();
        assert_eq!(effects.len(), 1);
    }

    #[test]
    fn flat_emit_begin_end_undo() {
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, host_ctx());
        assert!(FlatApi::emit_begin_undo(&api).is_ok());
        assert!(FlatApi::emit_end_undo(&api).is_ok());
        let effects = api.drain_effects();
        assert_eq!(effects.len(), 2);
    }

    #[test]
    fn flat_emit_readonly_blocked() {
        let ctx = InvocationContext::new(CallerId::Expression, CapabilityTier::ReadOnly);
        let session = HostSession::new("hello");
        let api = VimApi::from_session(&session, ctx);
        // Mutating operations should fail.
        assert!(FlatApi::emit_insert(&api, 0, "x").is_err());
        assert!(FlatApi::emit_delete(&api, 0, 1).is_err());
        assert!(FlatApi::emit_replace(&api, 0, 1, "y").is_err());
        // ReadOnly operations should succeed.
        assert!(FlatApi::emit_cursor(&api, 0).is_ok());
        assert!(FlatApi::emit_message(&api, "hi").is_ok());
    }

    // ── parse_scope unit tests ─────────────────────────────────────────

    #[test]
    fn parse_scope_short_forms() {
        assert_eq!(parse_scope("g").unwrap(), VarScope::Global);
        assert_eq!(parse_scope("b").unwrap(), VarScope::Buffer);
    }

    #[test]
    fn parse_scope_long_forms() {
        assert_eq!(parse_scope("global").unwrap(), VarScope::Global);
        assert_eq!(parse_scope("buffer").unwrap(), VarScope::Buffer);
    }

    #[test]
    fn parse_scope_invalid() {
        assert!(parse_scope("x").is_err());
        assert!(parse_scope("").is_err());
        assert!(parse_scope("Global").is_err()); // case-sensitive
    }
}
