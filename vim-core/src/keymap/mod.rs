#![allow(
    clippy::module_inception,
    reason = "keymap::keymap re-export is the canonical pattern"
)]
//! Keymap module - Key events, modifiers, and classification.
//!
//! This module provides:
//! - [`Key`] - Representation of a single key
//! - [`Modifiers`] - Keyboard modifiers (Ctrl, Alt, Shift, Meta)
//! - [`KeyEvent`] - Key + Modifiers combined
//! - [`KeyClass`] - Grammatical classification of keys
//! - [`CoreKeymap`] - Default Vim keybindings
//! - [`Keymap`] - Three-layer keymap (buffer-local + user + core)
//!
//! # Layering
//!
//! Imports `std` and `primitives::Mode` (for mode-aware classification); must
//! not import `commands`, `grammar`, `effects` or `execution`. Keymap is a low
//! layer, consumed by grammar for key classification.

mod core;
mod handler_map;
mod key;
mod key_class;
mod key_event;
mod keymap;
mod langmap;
mod mapping_flags;
mod mapping_kind;
mod mapping_owner;
mod modifiers;
mod name_registry;
mod trie;

pub use core::{CoreKeymap, CORE_KEYMAP};
pub use handler_map::{Handler, HandlerMap};
pub use key::Key;
pub use key_class::KeyClass;
pub use key_event::KeyEvent;
pub use keymap::{
    key_sequence, BufferMappings, KeySequence, Keymap, MapError, MappingMode, ModeMap,
    MAX_KEY_SEQUENCE_LEN,
};
pub use langmap::{LangmapError, LangmapTable};
pub use mapping_flags::MappingFlags;
pub use mapping_kind::MappingKind;
pub use mapping_owner::MappingOwner;
pub use modifiers::Modifiers;
pub use name_registry::NameRegistry;
pub use trie::{MappingEntry, MappingTrie, TrieLookup};
