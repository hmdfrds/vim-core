//! Vim key notation sequence parser.
//!
//! Parses strings like `<C-w>j`, `<Leader>r<Action>(Rename)`, and
//! `<Plug>(surround-word)` into `Vec<KeyEvent>`.
//!
//! The compound `<Action>(name)` and `<Plug>(name)` notation requires
//! access to the keymap's name registries to register the name and
//! produce the correct `u32` id. When no registries are available,
//! these pseudo-keys produce sentinel id `u32::MAX`.

use crate::keymap::{Key, KeyEvent, NameRegistry};

/// Mutable name registries for resolving `<Action>(name)` and `<Plug>(name)`.
///
/// Passed to the key notation parser so compound pseudo-key notation can
/// register names and produce correct `u32` ids. When no registries are
/// available, the parser falls back to sentinel id `u32::MAX`.
struct Registries<'a> {
    plug: &'a mut NameRegistry,
    action: &'a mut NameRegistry,
}

/// Parse a Vim key notation string into a `KeySequence`, resolving
/// `<Action>(name)` and `<Plug>(name)` against the keymap's registries.
///
/// When `keymap` is `Some`, compound pseudo-key notation like
/// `<Action>(Rename)` or `<Plug>(surround-word)` is recognized:
/// the name is registered in the keymap's action/plug registry and
/// the resulting `KeyEvent` carries the correct `u32` id.
///
/// When `keymap` is `None`, falls back to the non-registering behavior
/// where `<Action>` and `<Plug>` produce sentinel id `u32::MAX`.
pub(crate) fn parse_key_notation_sequence(
    input: &str,
    keymap: Option<&mut crate::keymap::Keymap>,
) -> Vec<KeyEvent> {
    match keymap {
        Some(km) => {
            let (plug_reg, action_reg) = km.registries_mut();
            parse_impl(
                input,
                Some(Registries {
                    plug: plug_reg,
                    action: action_reg,
                }),
            )
        }
        None => parse_impl(input, None),
    }
}

/// Core implementation: parse Vim key notation with optional name registries.
fn parse_impl(input: &str, mut registries: Option<Registries<'_>>) -> Vec<KeyEvent> {
    let mut seq = Vec::new();
    let mut remaining = input;

    while !remaining.is_empty() {
        if remaining.starts_with('<') {
            if let Some(end) = remaining.find('>') {
                let notation = &remaining[..=end];
                if let Some(key) = KeyEvent::from_vim_notation(notation) {
                    // Check for compound <Action>(name) or <Plug>(name)
                    let after_bracket = &remaining[end + 1..];

                    if matches!(key.key(), Key::Action(_) | Key::Plug(_)) {
                        if let Some((key_event, consumed)) =
                            try_parse_pseudo_key_name(key.key(), after_bracket, &mut registries)
                        {
                            seq.push(key_event);
                            remaining = &after_bracket[consumed..];
                            continue;
                        }
                    }

                    seq.push(key);
                    remaining = &remaining[end + 1..];
                    continue;
                }
            }
        }
        // Plain character — remaining is non-empty (loop guard above)
        let Some(ch) = remaining.chars().next() else {
            break;
        };
        seq.push(KeyEvent::char(ch));
        remaining = &remaining[ch.len_utf8()..];
    }
    seq
}

/// Try to parse `(name)` suffix after `<Action>` or `<Plug>`.
///
/// Returns `Some((key_event, bytes_consumed))` on success, `None` if
/// the suffix doesn't match `(name)` pattern.
fn try_parse_pseudo_key_name(
    base_key: Key,
    after_bracket: &str,
    registries: &mut Option<Registries<'_>>,
) -> Option<(KeyEvent, usize)> {
    // Must start with '('
    if !after_bracket.starts_with('(') {
        return None;
    }

    // Find matching ')'
    let close_paren = after_bracket.find(')')?;
    let name = &after_bracket[1..close_paren];

    if name.is_empty() {
        return None;
    }

    let consumed = close_paren + 1; // includes the ')'

    let key_event = match (&base_key, registries.as_mut()) {
        (Key::Action(_), Some(regs)) => {
            let id = regs.action.register(name);
            KeyEvent::action(id)
        }
        (Key::Plug(_), Some(regs)) => {
            let id = regs.plug.register(name);
            KeyEvent::plug(id)
        }
        (Key::Action(_), None) => KeyEvent::action(u32::MAX),
        (Key::Plug(_), None) => KeyEvent::plug(u32::MAX),
        _ => return None,
    };

    Some((key_event, consumed))
}

#[cfg(test)]
#[path = "key_notation_tests.rs"]
mod tests;
