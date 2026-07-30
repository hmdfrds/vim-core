//! Compile-time size guards for critical types.
//!
//! These assertions ensure our core types remain cache-efficient
//! and catch accidental size bloat at compile time.
//!
//! # Why This Matters
//!
//! - `SmallVec<[Effect; 4]>` assumes Effect fits in ~64-80 bytes
//! - Primitives must stay small for Copy efficiency
//! - Hot-path types (KeyEvent, Command, GrammarResult) are created every keystroke
//! - Marks and `JumpEntries` are stored in arrays/maps

use static_assertions::const_assert;

#[cfg(target_pointer_width = "64")]
use static_assertions::assert_eq_size;

// Import types to check
use crate::effects::Effect;
use crate::grammar::{Command, GrammarResult};
use crate::keymap::KeyEvent;
use crate::primitives::Mark;
use crate::primitives::Mode;
use crate::primitives::{Column, LineNumber, Offset, Position, Range};

// ============================================================================
// PRIMITIVES - Must be small, Copy, cache-line friendly
// ============================================================================

// Single usize wrappers = 8 bytes on 64-bit, 4 bytes on 32-bit (wasm32)
#[cfg(target_pointer_width = "64")]
assert_eq_size!(Offset, [u8; 8]);
#[cfg(target_pointer_width = "64")]
assert_eq_size!(LineNumber, [u8; 8]);
#[cfg(target_pointer_width = "64")]
assert_eq_size!(Column, [u8; 8]);

// Two usizes = 16 bytes on 64-bit
#[cfg(target_pointer_width = "64")]
assert_eq_size!(Range, [u8; 16]);
#[cfg(target_pointer_width = "64")]
assert_eq_size!(Position, [u8; 16]);

// Mark is an Offset + Option<Offset> (topline for viewport context).
// Thanks to niche-bearing Offset (NonMaxUsize), Option<Offset> is 8 bytes,
// so Mark = 8 + 8 = 16 bytes (down from 24 before the niche optimization).
#[cfg(target_pointer_width = "64")]
assert_eq_size!(Mark, [u8; 16]);

// Niche optimization: Option<Offset> fits in a single machine word.
#[cfg(target_pointer_width = "64")]
assert_eq_size!(Option<Offset>, [u8; 8]);

// ============================================================================
// 32-BIT TARGET SIZE ASSERTIONS (wasm32 parity)
// ============================================================================

#[cfg(target_pointer_width = "32")]
mod wasm32_size_checks {
    use super::*;
    use static_assertions::assert_eq_size;

    // usize is 4 bytes on 32-bit targets
    assert_eq_size!(Offset, [u8; 4]);
    assert_eq_size!(LineNumber, [u8; 4]);
    assert_eq_size!(Column, [u8; 4]);

    // Two usizes = 8 bytes on 32-bit
    assert_eq_size!(Range, [u8; 8]);
    assert_eq_size!(Position, [u8; 8]);

    // Mark: Offset + Option<Offset> = 4 + 4 = 8 bytes (niche optimization on NonMaxUsize)
    assert_eq_size!(Mark, [u8; 8]);

    // Niche optimization: Option<Offset> fits in a single 4-byte word
    assert_eq_size!(Option<Offset>, [u8; 4]);
}

// ============================================================================
// EFFECTS - Must fit in SmallVec inline storage
// ============================================================================

// Effect should be reasonably sized for SmallVec<[Effect; 4]>
// 4 * 80 = 320 bytes max inline, which fits in ~5 cache lines.
// SelectionRange includes `Option<VirtualColumn>` (16 bytes on 64-bit), which
// increases SmallVec<[SelectionRange; 4]> inline storage, pushing Effect to ~144
// bytes. SmallVec will spill to heap only for the SetMultiSelection variant.
const_assert!(std::mem::size_of::<Effect>() <= 160);

// ============================================================================
// HOT-PATH TYPES - Created/returned every keystroke
// ============================================================================

// KeyEvent: created per keystroke, must be small and Copy.
// The optional latin_key field (transport metadata for non-Latin layouts)
// adds one Key-sized slot, raising the ceiling from 16 to 32 bytes.
const_assert!(std::mem::size_of::<KeyEvent>() <= 32);

// Mode: copied on every keystroke check. Operator::Composed(ComposedPair)
// adds two u64 fields, so Mode::OperatorPending(Operator) is up to 32 bytes.
const_assert!(std::mem::size_of::<Mode>() <= 32);

// Command: parser output, stored in GrammarResult
const_assert!(std::mem::size_of::<Command>() <= 72);

// GrammarResult: returned from Parser::process() every keystroke
const_assert!(std::mem::size_of::<GrammarResult>() <= 80);

// ============================================================================
// DISPATCHER SIGNATURE ASSERTIONS
// ============================================================================

// These ensure each dispatch function has the exact signature
// (GrammarType, &CommandsContext) → CommandsResult.
// If a dispatcher's signature drifts, this block fails to compile.
//
// Migrated from the deleted dispatch/traits.rs — this was the only
// genuinely useful part of the sealed trait system.
#[allow(
    dead_code,
    reason = "compile-time assertion block; no runtime usage intended"
)]
const _DISPATCHER_SIGNATURE_ASSERTIONS: () = {
    use crate::commands::motions::MotionResult;
    use crate::commands::textobjects::TextObjectRange;
    use crate::commands::CommandResult;
    use crate::dispatch::{
        dispatch_action, dispatch_find, dispatch_insert, dispatch_mark, dispatch_motion,
        dispatch_operator, dispatch_textobject, dispatch_visual, ActionContext, MarkContext,
        MotionContext, OperatorContext, TextObjectContext, VisualContext,
    };

    use crate::commands::insert::InsertContext;
    use crate::grammar::types::{Action, CharCommand, MarkType, Motion, Operator, TextObject};
    use crate::grammar::Command as Cmd;

    let _: fn(Motion, &MotionContext<'_>) -> MotionResult = dispatch_motion;
    let _: fn(Operator, &OperatorContext<'_>) -> CommandResult = dispatch_operator;
    let _: fn(TextObject, &TextObjectContext<'_>) -> Option<TextObjectRange> = dispatch_textobject;
    let _: fn(Action, &ActionContext<'_>) -> CommandResult = dispatch_action;
    let _: fn(&Cmd, &InsertContext<'_>) -> CommandResult = dispatch_insert;
    let _: fn(&Cmd, &VisualContext<'_>) -> CommandResult = dispatch_visual;
    let _: fn(CharCommand, &MotionContext<'_>) -> Option<MotionResult> = dispatch_find;
    let _: fn(MarkType, &MarkContext, Option<crate::primitives::Mark>, &str) -> CommandResult =
        dispatch_mark;
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::print_stdout)]
    fn print_type_sizes() {
        use crate::execution::{HostRequest, Response};
        // Primitives
        println!("Offset: {} bytes", std::mem::size_of::<Offset>());
        println!("Range: {} bytes", std::mem::size_of::<Range>());
        println!("Position: {} bytes", std::mem::size_of::<Position>());
        println!("Mark: {} bytes", std::mem::size_of::<Mark>());
        // Effects
        println!("Effect: {} bytes", std::mem::size_of::<Effect>());
        // Hot-path
        println!("KeyEvent: {} bytes", std::mem::size_of::<KeyEvent>());
        println!("Mode: {} bytes", std::mem::size_of::<Mode>());
        println!("Command: {} bytes", std::mem::size_of::<Command>());
        println!(
            "GrammarResult: {} bytes",
            std::mem::size_of::<GrammarResult>()
        );
        // Engine output
        println!("Response: {} bytes", std::mem::size_of::<Response>());
        println!("HostRequest: {} bytes", std::mem::size_of::<HostRequest>());
    }
}
