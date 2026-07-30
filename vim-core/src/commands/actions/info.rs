//! Informational commands: ga, g8, Ctrl-G.
//!
//! These commands display character or file information in the status bar.

use crate::commands::helpers::{line_count, line_of};
use crate::effects::Effects;
use crate::primitives::Offset;

/// `ga` — Show ASCII/Unicode value of character under cursor.
///
/// Displays: `<char>  dec, Hex hex, Oct oct`
/// For multi-byte: also shows Unicode codepoint.
pub fn show_ascii(text: &str, cursor: usize) -> Effects {
    let Some(ch) = text.get(cursor..).and_then(|s| s.chars().next()) else {
        return Effects::new().show_message("NUL");
    };

    let codepoint = ch as u32;
    let msg = if codepoint <= 0x7F {
        format!("<{ch}>  {codepoint}, Hex {codepoint:02x}, Oct {codepoint:03o}")
    } else {
        format!("<{ch}> {codepoint}, Hex {codepoint:04x}, Oct {codepoint:o}")
    };
    Effects::new().show_message(msg)
}

/// `g8` — Show UTF-8 byte sequence of character under cursor.
///
/// Displays hex bytes separated by spaces: `e2 80 99` etc.
pub fn show_utf8(text: &str, cursor: usize) -> Effects {
    let Some(ch) = text.get(cursor..).and_then(|s| s.chars().next()) else {
        return Effects::new().show_message("NUL");
    };

    let mut buf = [0u8; 4];
    let bytes = ch.encode_utf8(&mut buf);
    let hex: Vec<String> = bytes
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Effects::new().show_message(hex.join(" "))
}

/// `Ctrl-G` — Show file info (line count, cursor position, percentage).
///
/// Displays: `line N of M  --N%--  col C`
pub fn show_file_info(text: &str, cursor: Offset) -> Effects {
    let total = line_count(text);
    let current = line_of(text, cursor.get()) + 1; // 1-indexed
    let percent = (current * 100).checked_div(total).unwrap_or(0);
    let msg = format!("line {current} of {total}  --{percent}%--");
    Effects::new().show_message(msg)
}
