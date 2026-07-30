//! Custom extension provider traits.
//!
//! These traits allow hosts to register custom motions and text objects
//! at runtime, making the engine infinitely extensible without forking.
//!
//! # Architecture
//!
//! Provider traits live at the `document` layer (alongside `FoldProvider`,
//! `SearchProvider`, etc.) because they have the same role: shell-provided
//! capabilities injected into the engine at key processing time.

/// Provider for host-registered custom motions.
///
/// Hosts implement this trait and pass it via `Providers::with_custom_motions()`.
/// When the engine encounters `Motion::Custom(id)`, the dispatch layer calls
/// `compute_motion(id, ...)` to determine the target position.
///
/// # Example (host-side)
///
/// ```ignore
/// struct TreeSitterMotions { /* tree-sitter state */ }
///
/// impl CustomMotionProvider for TreeSitterMotions {
///     fn compute_motion(&self, id: u32, text: &str, cursor: usize, count: u32) -> Option<usize> {
///         match id {
///             1 => find_next_function(text, cursor, count),
///             2 => find_next_class(text, cursor, count),
///             _ => None,
///         }
///     }
/// }
/// ```
/// Result of a custom motion computation with type information.
///
/// Allows custom motions to specify their inclusivity, rather than
/// defaulting to exclusive.
#[derive(Debug, Clone, Copy)]
pub struct CustomMotionResult {
    /// Target byte offset.
    pub offset: usize,
    /// How the end position of this motion range is treated
    /// (exclusive, inclusive, or linewise).
    pub inclusivity: crate::primitives::MotionInclusivity,
}

impl CustomMotionResult {
    /// Create an exclusive, charwise motion result (the default).
    #[inline]
    #[must_use]
    pub const fn new(offset: usize) -> Self {
        Self {
            offset,
            inclusivity: crate::primitives::MotionInclusivity::Exclusive,
        }
    }

    /// Create an inclusive charwise motion result.
    #[inline]
    #[must_use]
    pub const fn inclusive(offset: usize) -> Self {
        Self {
            offset,
            inclusivity: crate::primitives::MotionInclusivity::Inclusive,
        }
    }

    /// Create a linewise motion result.
    #[inline]
    #[must_use]
    pub const fn linewise(offset: usize) -> Self {
        Self {
            offset,
            inclusivity: crate::primitives::MotionInclusivity::Linewise,
        }
    }
}

/// Provider for host-registered custom motions.
///
/// Hosts implement this trait and register it via
/// `Providers::with_custom_motions()` or `VimEngine::register_motion_provider()`.
///
/// The basic [`compute_motion`](Self::compute_motion) returns an offset (exclusive, charwise).
/// Override [`compute_motion_with_info`](Self::compute_motion_with_info) to control
/// inclusivity and linewise behavior.
pub trait CustomMotionProvider: Send {
    /// Compute the target cursor position for a custom motion.
    ///
    /// Returns `None` if the motion cannot be performed (e.g., no target found).
    ///
    /// - `id`: the unique ID assigned when registering this motion
    /// - `text`: full document text
    /// - `cursor`: current cursor byte offset
    /// - `count`: repeat count (e.g., `3` in `3<custom-motion>`)
    fn compute_motion(&self, id: u32, text: &str, cursor: usize, count: u32) -> Option<usize>;

    /// Compute the target position with type information (inclusive/linewise).
    ///
    /// Override this to control the motion's inclusivity. The default
    /// delegates to [`compute_motion`](Self::compute_motion) and returns
    /// an exclusive, charwise result — backward compatible.
    fn compute_motion_with_info(
        &self,
        id: u32,
        text: &str,
        cursor: usize,
        count: u32,
    ) -> Option<CustomMotionResult> {
        self.compute_motion(id, text, cursor, count)
            .map(CustomMotionResult::new)
    }
}

/// Provider for host-registered custom text objects.
///
/// Hosts implement this trait and pass it via `Providers::with_custom_textobjects()`.
/// When the engine encounters `TextObjectKind::Custom(id)`, the dispatch layer calls
/// `compute_textobject(id, ...)` to determine the selection range.
///
/// # Example (host-side)
///
/// ```ignore
/// struct TreeSitterTextObjects { /* tree-sitter state */ }
///
/// impl CustomTextObjectProvider for TreeSitterTextObjects {
///     fn compute_textobject(&self, id: u32, text: &str, cursor: usize, inner: bool) -> Option<(usize, usize)> {
///         match id {
///             1 => find_function_range(text, cursor, inner),  // dif = delete inner function
///             2 => find_class_range(text, cursor, inner),     // dic = delete inner class
///             _ => None,
///         }
///     }
/// }
/// ```
/// Result of a custom text object computation with type information.
///
/// Allows custom text objects to specify whether they are linewise,
/// rather than defaulting to charwise.
#[derive(Debug, Clone, Copy)]
pub struct CustomTextObjectResult {
    /// Start byte offset (inclusive).
    pub start: usize,
    /// End byte offset (exclusive).
    pub end: usize,
    /// Whether the text object should be treated as linewise.
    /// Default: `false` (charwise).
    pub linewise: bool,
}

impl CustomTextObjectResult {
    /// Create a charwise text object result (the default).
    #[inline]
    #[must_use]
    pub const fn new(start: usize, end: usize) -> Self {
        Self {
            start,
            end,
            linewise: false,
        }
    }

    /// Create a linewise text object result.
    #[inline]
    #[must_use]
    pub const fn linewise(start: usize, end: usize) -> Self {
        Self {
            start,
            end,
            linewise: true,
        }
    }
}

/// Provider for host-registered custom text objects.
///
/// Hosts implement this trait and register it via
/// `Providers::with_custom_textobjects()` or `VimEngine::register_textobject_provider()`.
///
/// The basic [`compute_textobject`](Self::compute_textobject) returns a `(start, end)` pair (charwise).
/// Override [`compute_textobject_with_info`](Self::compute_textobject_with_info) to control
/// the linewise flag.
pub trait CustomTextObjectProvider: Send {
    /// Compute the range for a custom text object.
    ///
    /// Returns `Some((start, end))` byte offsets for the text object range,
    /// or `None` if no text object is found at the cursor position.
    ///
    /// - `id`: the unique ID assigned when registering this text object
    /// - `text`: full document text
    /// - `cursor`: current cursor byte offset
    /// - `inner`: `true` for inner scope (i), `false` for around scope (a)
    fn compute_textobject(
        &self,
        id: u32,
        text: &str,
        cursor: usize,
        inner: bool,
    ) -> Option<(usize, usize)>;

    /// Compute the range with type information (linewise flag).
    ///
    /// Override this to control whether the text object is linewise.
    /// The default delegates to [`compute_textobject`](Self::compute_textobject)
    /// and returns a charwise result — backward compatible.
    fn compute_textobject_with_info(
        &self,
        id: u32,
        text: &str,
        cursor: usize,
        inner: bool,
    ) -> Option<CustomTextObjectResult> {
        self.compute_textobject(id, text, cursor, inner)
            .map(|(start, end)| CustomTextObjectResult::new(start, end))
    }
}

/// Result of a custom operator computation.
///
/// Returned by [`CustomOperatorProvider::compute_operator`]. Lives at the
/// `document` layer (no `effects` import) so dispatch can convert it to
/// the appropriate `Effect` values.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum CustomOperatorResult {
    /// Replace the target range with the given text.
    ///
    /// Dispatch converts this to `Effect::Replace { range, text }`.
    Replace(compact_str::CompactString),
    /// Delete the target range entirely.
    ///
    /// Dispatch converts this to `Effect::Delete { range }`.
    Delete,
    /// No text modification; fall through to `Effect::CallOperatorFunc`
    /// for host-side handling via the async host request path.
    Defer,
    /// Operator handled successfully but no text change needed.
    ///
    /// Unlike `Defer`, this does NOT fall through to `CallOperatorFunc`.
    /// Use this when the provider fully handled the operator (e.g., side
    /// effects like showing info) but no document edit is required.
    NoOp,
}

/// Provider for host-registered custom operators.
///
/// Hosts implement this trait and pass it via `Providers::with_custom_operators()`.
/// When the engine encounters `Operator::Custom(id)`, the dispatch layer calls
/// `compute_operator(id, ...)` to determine how to transform the text.
///
/// Unlike `CallOperatorFunc` (which defers to a single host callback), custom
/// operators allow hosts to register multiple named operators with distinct IDs,
/// each with their own in-process computation.
///
/// Returns [`CustomOperatorResult`] — a primitives-level type that the dispatch
/// layer converts to the appropriate `Effect` values. For operations too complex
/// for `Replace`/`Delete`, return `Defer` to fall through to `CallOperatorFunc`.
///
/// # Example (host-side)
///
/// ```ignore
/// struct SortOperator;
///
/// impl CustomOperatorProvider for SortOperator {
///     fn compute_operator(
///         &self,
///         id: u32,
///         text: &str,
///         range: (usize, usize),
///         count: u32,
///     ) -> Option<CustomOperatorResult> {
///         match id {
///             1 => {
///                 let slice = &text[range.0..range.1];
///                 let mut lines: Vec<&str> = slice.lines().collect();
///                 lines.sort_unstable();
///                 Some(CustomOperatorResult::Replace(lines.join("\n").into()))
///             }
///             _ => None,
///         }
///     }
/// }
/// ```
pub trait CustomOperatorProvider: Send {
    /// Compute the result of a custom operator on the given range.
    ///
    /// Returns `Some(result)` to handle the operator, or `None` to fall back
    /// to the default `CallOperatorFunc` behavior.
    ///
    /// - `id`: the unique ID assigned when registering this operator
    /// - `text`: full document text
    /// - `range`: `(start, end)` byte offsets of the operator's target range
    /// - `count`: repeat count (e.g., `3` in `3<custom-op>iw`)
    fn compute_operator(
        &self,
        id: u32,
        text: &str,
        range: (usize, usize),
        count: u32,
    ) -> Option<CustomOperatorResult>;
}
