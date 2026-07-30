//! Composable, invertible changeset for text transformations.
//!
//! A [`ChangeSet`] describes how to transform one document into another
//! via a sequence of Retain/Insert/Delete operations. This is the
//! foundational data structure for:
//!
//! - **Undo**: store edit + inverse instead of full document snapshots
//! - **Composition**: chain edits (`A→B` + `B→C` = `A→C`)
//! - **Position mapping**: track cursor/mark positions through edits
//!
//! # Architecture
//!
//! This module lives in `primitives/` — zero internal dependencies.
//! Only imports `std` and `compact_str`.

mod apply;
mod builder;
mod change_set;
mod compose;
mod diff;
mod error;
mod invert;
mod iter;
mod position_map;
mod recorder;
mod text_op;
mod transform;

pub use change_set::ChangeSet;
pub use error::ChangeSetError;
pub use iter::ChangeIter;
pub use position_map::Assoc;
pub use recorder::ChangeSetRecorder;
pub use text_op::TextOp;
