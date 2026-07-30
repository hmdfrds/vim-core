//! Grammar module - Command parser state machine.
//!
//! This module provides the grammar parser for Vim commands.
//!
//! # Layering
//!
//! Grammar is a middle layer: it parses keys into `Command` enums. It imports
//! `primitives`, `state`, `keymap` and `errors`; it must not import
//! `commands`, `execution`, `effects`, `dispatch` or `mode`. Running the
//! parsed commands is the execution layer's job.
//!
//! ```text
//! grammar ──► primitives (Mode, RegisterName, MarkName, …)
//!    │
//!    ├──► state (allowed — grammar reads state types)
//!    ├──► keymap (KeyEvent, KeyClass, Keymap)
//!    ├──► errors (VimError for ex_parser)
//!    │
//!    ✗──► commands (grammar doesn't execute commands)
//!    ✗──► execution (grammar is consumed BY execution)
//!    ✗──► effects (grammar produces Commands, not Effects)
//! ```
//!
//! # Architecture
//!
//! The parser follows the **impl-block splitting** pattern where handler
//! methods are organized in separate files under `handlers/`:
//!
//! ```text
//! grammar/
//! ├── mod.rs                (this file)
//! ├── types/                (grammar type definitions)
//! │   ├── mod.rs
//! │   ├── motion.rs         (Motion enum — see types/ for full list)
//! │   ├── textobject.rs     (TextObject, TextObjectKind — see types/ for full list)
//! │   ├── action.rs         (Action enum — see types/ for full list)
//! │   ├── char_command.rs   (CharCommand enum)
//! │   ├── mark.rs           (MarkType enum)
//! │   ├── ex_command.rs     (ExCommand, SortOptions)
//! │   └── ex_range.rs       (ExRange, LineSpec)
//! ├── input_state.rs        (InputState enum — see types/ for full list)
//! ├── command.rs            (Command enum — see types/ for full list)
//! ├── command_line_intent.rs (CommandLineIntent, OperatorSearchIntent)
//! ├── result.rs             (GrammarResult enum)
//! ├── ex_parser.rs          (Ex command parser)
//! ├── parser.rs             (Parser struct — core logic)
//! └── handlers/             (impl Parser { ... } split by state)
//!     ├── mod.rs
//!     ├── helpers.rs        (shared utilities)
//!     ├── ready.rs          (Ready state handler)
//!     ├── operator.rs       (Operator state handler)
//!     ├── awaiting.rs       (AwaitingRegister/Char/Mark handlers)
//!     ├── prefix.rs         (AwaitingPrefix handler)
//!     ├── insert.rs         (Insert mode handler)
//!     └── visual.rs         (Visual mode handler)
//! ```
//!
//! # State Machine Flow
//!
//! ```text
//! Key Press → [Keymap] → KeyClass → [Parser] → GrammarResult
//!                                       │
//!                                       ├── Continue(InputState)
//!                                       ├── Execute(Command)
//!                                       ├── ModeChange(Mode)
//!                                       ├── Invalid
//!                                       └── Cancel
//! ```
//!
//! # Example
//!
//! ```ignore
//! use vim_core::grammar::{Parser, GrammarResult, Command};
//! use vim_core::keymap::{KeyEvent, Keymap};
//! use vim_core::state::Mode;
//!
//! let mut parser = Parser::new();
//! let keymap = Keymap::default();
//!
//! // Parse "dw" (delete word)
//! let _ = parser.process(KeyEvent::char('d'), &keymap, Mode::Normal);
//! let result = parser.process(KeyEvent::char('w'), &keymap, Mode::Normal);
//!
//! match result {
//!     GrammarResult::Execute(Command::OperatorMotion { .. }) => {
//!         // Execute the delete word command
//!     }
//!     _ => {}
//! }
//! ```

pub mod command;
pub mod command_line_intent;
pub mod command_meta;
pub mod ex_parser;
pub mod handlers;
pub mod hints;
pub mod input_state;
pub mod parser;
pub mod result;
pub mod types;

// Re-exports for convenience
pub use command::{
    compute_count, count_or_default, Command, InsertKind, MacroKind, PrefixCommand, VisualKind,
};
pub use command_line_intent::{CommandLineIntent, OperatorSearchIntent};
pub use ex_parser::{parse_ex_command, parse_ex_command_with_modifiers, split_ex_pipeline};
pub use input_state::{InputState, InsertLiteralState, MacroAwaitKind};
pub use parser::Parser;
pub use result::GrammarResult;
pub use types::MapModePrefix;
pub use types::ModifierFlags;
pub use types::{
    Action, CharCommand, CommandLineEdit, MarkType, Motion, Operator, TextObject, TextObjectKind,
};
