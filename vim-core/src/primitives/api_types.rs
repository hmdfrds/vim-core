//! Primitive types shared across the universal API layer.
//!
//! These types are used by the capability system and the scripting API.
//! They live in `primitives` because they carry no logic and have zero
//! internal dependencies.

use std::collections::BTreeMap;

use compact_str::CompactString;

/// A single text edit for cross-buffer operations.
///
/// Represents a byte-range replacement: replaces the content in
/// `start..end` with `text`. An empty `text` with `start < end` is a
/// deletion; `start == end` with non-empty `text` is an insertion.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextEdit {
    /// Byte offset of the range start (inclusive).
    pub start: usize,
    /// Byte offset past the end of the range to replace (exclusive).
    pub end: usize,
    /// Replacement text.
    pub text: CompactString,
}

/// The capability tier required to call an API function.
///
/// `ReadOnly` operations never mutate buffer state; `Mutating` operations may
/// change text, registers, marks, or any other editor state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CapabilityTier {
    /// The operation only reads state and never mutates it.
    ReadOnly,
    /// The operation may mutate editor state.
    Mutating,
}

/// The scope in which a variable or option is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum VarScope {
    /// Variable is global across all buffers.
    Global,
    /// Variable is local to a specific buffer.
    Buffer,
}

/// Opaque identifier for a registered autocommand.
///
/// The inner `u64` is assigned sequentially by the engine when an autocommand
/// is registered and is stable for the lifetime of the editor session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AutocmdId(pub u64);

/// A dynamically-typed value used by the variable store, option queries,
/// and expression evaluation.
///
/// `BTreeMap` is used for maps to ensure deterministic iteration order.
/// `CompactString` is used for strings to avoid heap allocation for short values.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum VimValue {
    /// The absence of a value.
    Nil,
    /// A boolean value.
    Bool(bool),
    /// A 64-bit signed integer.
    Int(i64),
    /// A 64-bit floating-point number.
    Float(f64),
    /// A UTF-8 string.
    String(CompactString),
    /// An ordered list of values.
    List(Vec<VimValue>),
    /// A string-keyed map of values with deterministic iteration order.
    Map(BTreeMap<CompactString, VimValue>),
}

/// Identifies who is issuing an API call.
///
/// Used by the capability checker to decide which permissions apply and to
/// attribute side-effects (e.g. undo history entries) to the correct origin.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CallerId {
    /// Call originates from an autocommand handler.
    Autocmd(AutocmdId),
    /// Call originates from a Vimscript/expression evaluation.
    Expression,
    /// Call originates from the host editor (e.g. LSP-driven edits).
    Host,
    /// Call originates from inside the vim-core engine itself.
    Internal,
}
