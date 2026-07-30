//! Cross-instance state federation.
//!
//! This module provides the infrastructure for sharing Vim state (registers,
//! global marks, search/command history, macros) across multiple editor
//! instances — e.g., multiple tabs in the same IDE session, or separate
//! processes sharing a persistent store.
//!
//! # Architecture
//!
//! ```text
//! VimEngine
//!    │
//!    ├── process(key) → Effects
//!    │       │
//!    │       └── execution::federation_extractor::extract_events(effects, state) → [StateEvent]
//!    │                                                   │
//!    │                                    StateBackend::store_*(...)
//!    │
//!    └── load_persisted_state()
//!            │
//!            └── StateBackend::load_*(...)
//! ```
//!
//! This module is always compiled (formerly feature-gated as `federation`).

pub mod backend;
pub mod events;

pub use backend::{FederatedJumpEntry, GlobalMark, StateBackend};
pub use events::{EventSource, StateEvent};
