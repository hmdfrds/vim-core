//! Tests for `\Z` (ignore composing characters) parser modifier.

use crate::ir::{ComposingMode, VimPatternNode};
use crate::parser::parse_pattern;

#[test]
fn backslash_z_uppercase_sets_composing_mode() {
    let result = parse_pattern("\\Zfoo").unwrap();
    assert_eq!(result.composing_mode, ComposingMode::Ignore);
    // The \Z modifier produces no AST node; only "foo" remains.
    assert_eq!(
        result.node,
        VimPatternNode::Sequence(vec![
            VimPatternNode::Literal('f'),
            VimPatternNode::Literal('o'),
            VimPatternNode::Literal('o'),
        ])
    );
}

#[test]
fn backslash_z_lowercase_is_not_composing_modifier() {
    // \zs is SetMatchStart — only \Z (uppercase) is the composing modifier.
    let result = parse_pattern("\\zs").unwrap();
    assert_eq!(result.node, VimPatternNode::SetMatchStart);
    assert_eq!(result.composing_mode, ComposingMode::Respect);
}

#[test]
fn backslash_z_uppercase_alone() {
    // Pattern consisting only of \Z produces an empty sequence.
    let result = parse_pattern("\\Z").unwrap();
    assert_eq!(result.composing_mode, ComposingMode::Ignore);
    assert_eq!(result.node, VimPatternNode::Sequence(vec![]));
}

#[test]
fn backslash_z_uppercase_mid_pattern() {
    // \Z can appear anywhere in the pattern.
    let result = parse_pattern("a\\Zb").unwrap();
    assert_eq!(result.composing_mode, ComposingMode::Ignore);
    assert_eq!(
        result.node,
        VimPatternNode::Sequence(vec![
            VimPatternNode::Literal('a'),
            VimPatternNode::Literal('b'),
        ])
    );
}

#[test]
fn default_composing_mode_is_respect() {
    let result = parse_pattern("abc").unwrap();
    assert_eq!(result.composing_mode, ComposingMode::Respect);
}

#[test]
fn backslash_z_uppercase_does_not_affect_dollar_anchor() {
    // `$` before `\Z` should still be recognized as end-of-line anchor.
    // Vim skips \Z when checking if $ is at branch end.
    let result = parse_pattern("foo$\\Z").unwrap();
    assert_eq!(result.composing_mode, ComposingMode::Ignore);
    // The $ should be EndOfLine (not literal) because \Z is skipped
    // during the is_at_branch_end check.
    assert_eq!(
        result.node,
        VimPatternNode::Sequence(vec![
            VimPatternNode::Literal('f'),
            VimPatternNode::Literal('o'),
            VimPatternNode::Literal('o'),
            VimPatternNode::EndOfLine,
        ])
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// \%C — ANY COMPOSING CHARACTER ATOM
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn percent_c_parses_without_error() {
    use crate::engine::VimRegex;
    assert!(VimRegex::new(r"\%C").is_ok(), r"\%C should parse");
}

#[test]
fn percent_c_produces_any_composing_node() {
    let result = parse_pattern("\\%C").unwrap();
    assert_eq!(result.node, VimPatternNode::AnyComposing);
}

#[test]
fn percent_c_in_sequence() {
    let result = parse_pattern("a\\%Cb").unwrap();
    assert_eq!(
        result.node,
        VimPatternNode::Sequence(vec![
            VimPatternNode::Literal('a'),
            VimPatternNode::AnyComposing,
            VimPatternNode::Literal('b'),
        ])
    );
}

#[test]
fn percent_c_display_roundtrip() {
    let node = VimPatternNode::AnyComposing;
    assert_eq!(format!("{node}"), "\\%C");
}
