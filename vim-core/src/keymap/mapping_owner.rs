//! Ownership tracking for key mappings.
//!
//! Every mapping installed in the keymap has an owner that identifies
//! who created it. This allows mappings to be removed by owner (e.g.,
//! when an extension is removed) and helps with conflict reporting.

use compact_str::CompactString;

/// Identifies who owns a key mapping.
///
/// Ownership determines the precedence and lifecycle of a mapping:
/// - [`User`](MappingOwner::User) mappings are highest priority and persist
///   until explicitly removed.
/// - [`Core`](MappingOwner::Core) mappings are the built-in Vim defaults.
/// - [`Host`](MappingOwner::Host) mappings are installed by an embedding
///   host application and identified by an arbitrary string tag.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MappingOwner {
    /// Mapping created directly by the user (e.g., via `:nmap`).
    User,
    /// Built-in Vim default mapping installed by the engine core.
    Core,
    /// Mapping installed by an embedding host, identified by a tag string.
    Host(CompactString),
}
