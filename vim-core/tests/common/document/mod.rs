//! Test document infrastructure.
//!
//! In-memory Document implementation for testing.
//!
//! Split into:
//! - `test_document.rs` - Core Document trait implementation
//! - `cursor.rs` - Cursor and selection tracking  
//! - `edits.rs` - Text editing operations

mod cursor;
mod edits;
mod test_document;

pub use test_document::TestDocument;
