//! Parser tests — literals, anchors, character classes, escapes.

use crate::ir::{
    CaseMode, CharClass, CollectionItem, EscapeKind, ParseResult, Span, VimPatternNode,
    VimRegexError, VimRegexErrorKind,
};
use crate::parser::parse_pattern;

fn parse(pattern: &str) -> Result<VimPatternNode, VimRegexError> {
    parse_pattern(pattern).map(|r| r.node)
}

fn parse_result(pattern: &str) -> Result<ParseResult, VimRegexError> {
    parse_pattern(pattern)
}

// ═══════════════════════════════════════════════════════════════════════
// Basic patterns — literals, anchors, sequences
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn empty_pattern_returns_error() {
    assert_eq!(parse(""), Err(VimRegexErrorKind::EmptyPattern.into()));
}

#[test]
fn sequence_abc() {
    assert_eq!(
        parse("abc"),
        Ok(VimPatternNode::Sequence(vec![
            VimPatternNode::Literal('a'),
            VimPatternNode::Literal('b'),
            VimPatternNode::Literal('c'),
        ]))
    );
}

#[test]
fn trailing_backslash_error() {
    assert_eq!(
        parse("a\\"),
        Err(VimRegexErrorKind::TrailingBackslash {
            span: Span::new(1, 2),
        }
        .into())
    );
}

#[test]
fn anchors_in_sequence() {
    assert_eq!(
        parse("^abc$"),
        Ok(VimPatternNode::Sequence(vec![
            VimPatternNode::StartOfLine,
            VimPatternNode::Literal('a'),
            VimPatternNode::Literal('b'),
            VimPatternNode::Literal('c'),
            VimPatternNode::EndOfLine,
        ]))
    );
}

#[test]
fn dot_in_sequence() {
    assert_eq!(
        parse("a.b"),
        Ok(VimPatternNode::Sequence(vec![
            VimPatternNode::Literal('a'),
            VimPatternNode::AnyChar,
            VimPatternNode::Literal('b'),
        ]))
    );
}

#[test]
fn escaped_metachar_becomes_literal() {
    assert_eq!(parse("\\."), Ok(VimPatternNode::Literal('.')));
}

#[test]
fn tilde_sets_feature_flag() {
    assert!(parse_result("~").unwrap().features.has_last_substitute);
}

// ═══════════════════════════════════════════════════════════════════════
// Character classes
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn class_keyword() {
    assert_eq!(parse("\\k"), Ok(VimPatternNode::Class(CharClass::Keyword)));
}

#[test]
fn class_not_keyword() {
    assert_eq!(
        parse("\\K"),
        Ok(VimPatternNode::Class(CharClass::KeywordNoDigit))
    );
}

#[test]
fn class_with_newline_digit() {
    assert_eq!(
        parse("\\_d"),
        Ok(VimPatternNode::ClassWithNewline(CharClass::Digit))
    );
}

#[test]
fn any_char_nl() {
    assert_eq!(parse("\\_."), Ok(VimPatternNode::AnyCharNl));
}

// ═══════════════════════════════════════════════════════════════════════
// Word boundaries, match overrides, case modifiers
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn set_match_start_sets_feature_flag() {
    assert!(parse_result("\\zs").unwrap().features.has_match_override);
}

#[test]
fn set_match_end_sets_feature_flag() {
    assert!(parse_result("\\ze").unwrap().features.has_match_override);
}

#[test]
fn invalid_z_escape() {
    assert!(matches!(
        parse("\\zx").unwrap_err().kind,
        VimRegexErrorKind::InvalidEscape { ch: 'z', .. }
    ));
}

#[test]
fn case_insensitive_modifier() {
    assert_eq!(
        parse_result("\\cfoo").unwrap().case_mode,
        CaseMode::Insensitive
    );
}

#[test]
fn case_sensitive_modifier() {
    assert_eq!(
        parse_result("\\Cfoo").unwrap().case_mode,
        CaseMode::Sensitive
    );
}

#[test]
fn combined_class_and_literal() {
    assert_eq!(
        parse("\\d+"),
        Ok(VimPatternNode::Sequence(vec![
            VimPatternNode::Class(CharClass::Digit),
            VimPatternNode::Literal('+'),
        ]))
    );
}

#[test]
fn word_boundary_pattern() {
    assert_eq!(
        parse("\\<word\\>"),
        Ok(VimPatternNode::Sequence(vec![
            VimPatternNode::WordBoundaryStart,
            VimPatternNode::Literal('w'),
            VimPatternNode::Literal('o'),
            VimPatternNode::Literal('r'),
            VimPatternNode::Literal('d'),
            VimPatternNode::WordBoundaryEnd,
        ]))
    );
}

#[test]
fn invalid_underscore_escape() {
    assert!(matches!(
        parse("\\_q").unwrap_err().kind,
        VimRegexErrorKind::InvalidEscape { ch: '_', .. }
    ));
}

#[test]
fn underscore_at_end_of_pattern() {
    assert!(matches!(
        parse("\\_").unwrap_err().kind,
        VimRegexErrorKind::InvalidEscape { ch: '_', .. }
    ));
}

#[test]
fn all_class_with_newline_variants() {
    assert_eq!(
        parse("\\_w"),
        Ok(VimPatternNode::ClassWithNewline(CharClass::Word))
    );
    assert_eq!(
        parse("\\_s"),
        Ok(VimPatternNode::ClassWithNewline(CharClass::Whitespace))
    );
    assert_eq!(
        parse("\\_a"),
        Ok(VimPatternNode::ClassWithNewline(CharClass::Alpha))
    );
}

#[test]
fn case_modifier_last_wins_sensitive() {
    assert_eq!(
        parse_result("\\c\\Cfoo").unwrap().case_mode,
        CaseMode::Sensitive
    );
}

#[test]
fn case_modifier_last_wins_insensitive() {
    assert_eq!(
        parse_result("\\C\\cfoo").unwrap().case_mode,
        CaseMode::Insensitive
    );
}

#[test]
fn case_modifier_at_end_of_pattern() {
    let r = parse_result("foo\\c").unwrap();
    assert_eq!(r.case_mode, CaseMode::Insensitive);
    assert_eq!(
        r.node,
        VimPatternNode::Sequence(vec![
            VimPatternNode::Literal('f'),
            VimPatternNode::Literal('o'),
            VimPatternNode::Literal('o'),
        ])
    );
}

#[test]
fn case_modifier_standalone() {
    let r = parse_result("\\c").unwrap();
    assert_eq!(r.case_mode, CaseMode::Insensitive);
    assert_eq!(r.node, VimPatternNode::Sequence(vec![]));
}

// ═══════════════════════════════════════════════════════════════════════
// Collection escape sequences
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn collection_escape_tab() {
    let result = parse_pattern(r"[\t]").unwrap();
    match &result.node {
        VimPatternNode::Collection { items, .. } => {
            assert_eq!(items, &[CollectionItem::Single('\t')]);
        }
        _ => panic!("expected Collection"),
    }
}

#[test]
fn collection_escape_return() {
    let result = parse_pattern(r"[\r]").unwrap();
    match &result.node {
        VimPatternNode::Collection { items, .. } => {
            assert_eq!(items, &[CollectionItem::Single('\r')]);
        }
        _ => panic!("expected Collection"),
    }
}

#[test]
fn collection_escape_escape() {
    let result = parse_pattern(r"[\e]").unwrap();
    match &result.node {
        VimPatternNode::Collection { items, .. } => {
            assert_eq!(items, &[CollectionItem::Single('\x1B')]);
        }
        _ => panic!("expected Collection"),
    }
}

#[test]
fn collection_escape_backspace() {
    let result = parse_pattern(r"[\b]").unwrap();
    match &result.node {
        VimPatternNode::Collection { items, .. } => {
            assert_eq!(items, &[CollectionItem::Single('\x08')]);
        }
        _ => panic!("expected Collection"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Collection character codes
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn collection_char_code_decimal() {
    let result = parse_pattern(r"[\d97]").unwrap();
    match &result.node {
        VimPatternNode::Collection { items, .. } => {
            assert_eq!(items, &[CollectionItem::Single('a')]);
        }
        _ => panic!("expected Collection"),
    }
}

#[test]
fn collection_char_code_hex() {
    let result = parse_pattern(r"[\x61]").unwrap();
    match &result.node {
        VimPatternNode::Collection { items, .. } => {
            assert_eq!(items, &[CollectionItem::Single('a')]);
        }
        _ => panic!("expected Collection"),
    }
}

#[test]
fn collection_char_code_octal() {
    let result = parse_pattern(r"[\o141]").unwrap();
    match &result.node {
        VimPatternNode::Collection { items, .. } => {
            assert_eq!(items, &[CollectionItem::Single('a')]);
        }
        _ => panic!("expected Collection"),
    }
}

#[test]
fn collection_char_code_unicode4() {
    let result = parse_pattern(r"[a]").unwrap();
    match &result.node {
        VimPatternNode::Collection { items, .. } => {
            assert_eq!(items, &[CollectionItem::Single('a')]);
        }
        _ => panic!("expected Collection"),
    }
}

#[test]
fn collection_char_code_unicode8() {
    let result = parse_pattern(r"[\U0001F600]").unwrap();
    match &result.node {
        VimPatternNode::Collection { items, .. } => {
            assert_eq!(items, &[CollectionItem::Single('\u{1F600}')]);
        }
        _ => panic!("expected Collection"),
    }
}

/// `\d` alone inside a collection (not followed by digits) should remain
/// a character class (CharClass::Digit), not a char code.
#[test]
fn collection_bare_d_remains_digit_class() {
    let result = parse_pattern(r"[\d]").unwrap();
    match &result.node {
        VimPatternNode::Collection { items, .. } => {
            assert_eq!(items, &[CollectionItem::Class(CharClass::Digit)]);
        }
        _ => panic!("expected Collection"),
    }
}

/// `\d` followed by a non-digit should also remain a class.
#[test]
fn collection_d_followed_by_nondigit_remains_class() {
    let result = parse_pattern(r"[\da]").unwrap();
    match &result.node {
        VimPatternNode::Collection { items, .. } => {
            assert_eq!(
                items,
                &[
                    CollectionItem::Class(CharClass::Digit),
                    CollectionItem::Single('a'),
                ]
            );
        }
        _ => panic!("expected Collection"),
    }
}

/// `\o` alone inside a collection (not followed by octal digits) should remain
/// a character class (CharClass::Octal).
#[test]
fn collection_bare_o_remains_octal_class() {
    let result = parse_pattern(r"[\o]").unwrap();
    match &result.node {
        VimPatternNode::Collection { items, .. } => {
            assert_eq!(items, &[CollectionItem::Class(CharClass::Octal)]);
        }
        _ => panic!("expected Collection"),
    }
}

/// `\x` alone inside a collection (not followed by hex digits) should remain
/// a character class (CharClass::Hex).
#[test]
fn collection_bare_x_remains_hex_class() {
    let result = parse_pattern(r"[\x]").unwrap();
    match &result.node {
        VimPatternNode::Collection { items, .. } => {
            assert_eq!(items, &[CollectionItem::Class(CharClass::Hex)]);
        }
        _ => panic!("expected Collection"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// End-to-end matching: collection escapes
// ═══════════════════════════════════════════════════════════════════════

use crate::engine::VimRegex;
use crate::matchers::MatchContext;

#[test]
fn collection_escape_tab_matches() {
    let re = VimRegex::new(r"[\t]").unwrap();
    assert!(re.is_match(&MatchContext::simple("\t")).unwrap());
    assert!(!re.is_match(&MatchContext::simple("t")).unwrap());
    assert!(!re.is_match(&MatchContext::simple("x")).unwrap());
}

#[test]
fn collection_escape_return_matches() {
    let re = VimRegex::new(r"[\r]").unwrap();
    assert!(re.is_match(&MatchContext::simple("\r")).unwrap());
    assert!(!re.is_match(&MatchContext::simple("r")).unwrap());
}

#[test]
fn collection_escape_backspace_matches() {
    let re = VimRegex::new(r"[\b]").unwrap();
    assert!(re.is_match(&MatchContext::simple("\x08")).unwrap());
    assert!(!re.is_match(&MatchContext::simple("b")).unwrap());
}

#[test]
fn collection_escape_esc_matches() {
    let re = VimRegex::new(r"[\e]").unwrap();
    assert!(re.is_match(&MatchContext::simple("\x1B")).unwrap());
    assert!(!re.is_match(&MatchContext::simple("e")).unwrap());
}

#[test]
fn collection_char_code_decimal_matches() {
    // \d97 = 'a'
    let re = VimRegex::new(r"[\d97]").unwrap();
    assert!(re.is_match(&MatchContext::simple("a")).unwrap());
    assert!(!re.is_match(&MatchContext::simple("b")).unwrap());
    assert!(!re.is_match(&MatchContext::simple("9")).unwrap());
}

#[test]
fn collection_char_code_hex_matches() {
    // \x61 = 'a'
    let re = VimRegex::new(r"[\x61]").unwrap();
    assert!(re.is_match(&MatchContext::simple("a")).unwrap());
    assert!(!re.is_match(&MatchContext::simple("x")).unwrap());
}

#[test]
fn collection_char_code_octal_matches() {
    // \o141 = 'a'
    let re = VimRegex::new(r"[\o141]").unwrap();
    assert!(re.is_match(&MatchContext::simple("a")).unwrap());
    assert!(!re.is_match(&MatchContext::simple("o")).unwrap());
}

#[test]
fn collection_char_code_unicode8_matches() {
    // \U0001F600 = '😀'
    let re = VimRegex::new(r"[\U0001F600]").unwrap();
    assert!(re.is_match(&MatchContext::simple("\u{1F600}")).unwrap());
    assert!(!re.is_match(&MatchContext::simple("U")).unwrap());
}

#[test]
fn collection_digit_class_still_matches_digits() {
    // Bare \d in collection should still match digits
    let re = VimRegex::new(r"[\d]").unwrap();
    assert!(re.is_match(&MatchContext::simple("5")).unwrap());
    assert!(re.is_match(&MatchContext::simple("0")).unwrap());
    assert!(!re.is_match(&MatchContext::simple("a")).unwrap());
}

// ═══════════════════════════════════════════════════════════════════════
// `^`/`$` context sensitivity: anchors only at branch start/end
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn caret_at_start_is_anchor() {
    let result = parse_pattern("^foo").unwrap();
    match &result.node {
        VimPatternNode::Sequence(nodes) => {
            assert!(matches!(nodes[0], VimPatternNode::StartOfLine));
        }
        _ => panic!("expected Sequence"),
    }
}

#[test]
fn caret_mid_pattern_is_literal() {
    let result = parse_pattern("foo^bar").unwrap();
    match &result.node {
        VimPatternNode::Sequence(nodes) => {
            // 'f', 'o', 'o', '^', 'b', 'a', 'r'
            assert_eq!(nodes[3], VimPatternNode::Literal('^'));
        }
        _ => panic!("expected Sequence"),
    }
}

#[test]
fn caret_after_alternation_is_anchor() {
    // "foo\|^bar" — ^ after \| is anchor
    let result = parse_pattern(r"foo\|^bar").unwrap();
    match &result.node {
        VimPatternNode::Alternation(branches) => match &branches[1] {
            VimPatternNode::Sequence(nodes) => {
                assert!(matches!(nodes[0], VimPatternNode::StartOfLine));
            }
            _ => panic!("expected Sequence in second branch"),
        },
        _ => panic!("expected Alternation"),
    }
}

#[test]
fn caret_after_group_open_is_anchor() {
    // "\(^foo\)" — ^ after \( is anchor
    let result = parse_pattern(r"\(^foo\)").unwrap();
    match &result.node {
        VimPatternNode::Group { inner, .. } => match inner.as_ref() {
            VimPatternNode::Sequence(nodes) => {
                assert!(matches!(nodes[0], VimPatternNode::StartOfLine));
            }
            _ => panic!("expected Sequence inside group"),
        },
        _ => panic!("expected Group"),
    }
}

#[test]
fn dollar_at_end_is_anchor() {
    let result = parse_pattern("foo$").unwrap();
    match &result.node {
        VimPatternNode::Sequence(nodes) => {
            assert!(matches!(nodes.last().unwrap(), VimPatternNode::EndOfLine));
        }
        _ => panic!("expected Sequence"),
    }
}

#[test]
fn dollar_mid_pattern_is_literal() {
    let result = parse_pattern("foo$bar").unwrap();
    match &result.node {
        VimPatternNode::Sequence(nodes) => {
            // 'f', 'o', 'o', '$', 'b', 'a', 'r'
            assert_eq!(nodes[3], VimPatternNode::Literal('$'));
        }
        _ => panic!("expected Sequence"),
    }
}

#[test]
fn dollar_before_alternation_is_anchor() {
    // "foo$\|bar" — $ before \| is anchor
    let result = parse_pattern(r"foo$\|bar").unwrap();
    match &result.node {
        VimPatternNode::Alternation(branches) => match &branches[0] {
            VimPatternNode::Sequence(nodes) => {
                assert!(matches!(nodes.last().unwrap(), VimPatternNode::EndOfLine));
            }
            _ => panic!("expected Sequence in first branch"),
        },
        _ => panic!("expected Alternation"),
    }
}

#[test]
fn dollar_before_group_close_is_anchor() {
    // "\(foo$\)" — $ before \) is anchor
    let result = parse_pattern(r"\(foo$\)").unwrap();
    match &result.node {
        VimPatternNode::Group { inner, .. } => match inner.as_ref() {
            VimPatternNode::Sequence(nodes) => {
                assert!(matches!(nodes.last().unwrap(), VimPatternNode::EndOfLine));
            }
            _ => panic!("expected Sequence inside group"),
        },
        _ => panic!("expected Group"),
    }
}

#[test]
fn caret_alone_is_anchor() {
    // "^" alone — at start of pattern, so it's an anchor
    assert_eq!(parse("^"), Ok(VimPatternNode::StartOfLine));
}

#[test]
fn dollar_alone_is_anchor() {
    // "$" alone — at end of pattern, so it's an anchor
    assert_eq!(parse("$"), Ok(VimPatternNode::EndOfLine));
}

#[test]
fn caret_dollar_both_anchors() {
    // "^$" — ^ at start is anchor, $ at end is anchor
    assert_eq!(
        parse("^$"),
        Ok(VimPatternNode::Sequence(vec![
            VimPatternNode::StartOfLine,
            VimPatternNode::EndOfLine,
        ]))
    );
}

#[test]
fn caret_after_non_capturing_group_open_is_anchor() {
    // "\%(^foo\)" — ^ after \%( is anchor
    let result = parse_pattern(r"\%(^foo\)").unwrap();
    match &result.node {
        VimPatternNode::Group { inner, .. } => match inner.as_ref() {
            VimPatternNode::Sequence(nodes) => {
                assert!(matches!(nodes[0], VimPatternNode::StartOfLine));
            }
            _ => panic!("expected Sequence inside group"),
        },
        _ => panic!("expected Group"),
    }
}

#[test]
fn dollar_mid_not_before_alternation_is_literal() {
    // "a$b" — $ followed by 'b', not at end, not before \|, so literal
    assert_eq!(
        parse("a$b"),
        Ok(VimPatternNode::Sequence(vec![
            VimPatternNode::Literal('a'),
            VimPatternNode::Literal('$'),
            VimPatternNode::Literal('b'),
        ]))
    );
}

#[test]
fn caret_after_literal_is_literal() {
    // "a^" — ^ not at start, so literal
    assert_eq!(
        parse("a^"),
        Ok(VimPatternNode::Sequence(vec![
            VimPatternNode::Literal('a'),
            VimPatternNode::Literal('^'),
        ]))
    );
}

// ═══════════════════════════════════════════════════════════════════════
// A quantifier following an unquantifiable atom is a literal
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn caret_star_is_literal_star() {
    // Vim: ^* means StartOfLine followed by literal '*'
    assert_eq!(
        parse("^*ptr"),
        Ok(VimPatternNode::Sequence(vec![
            VimPatternNode::StartOfLine,
            VimPatternNode::Literal('*'),
            VimPatternNode::Literal('p'),
            VimPatternNode::Literal('t'),
            VimPatternNode::Literal('r'),
        ]))
    );
}

#[test]
fn dollar_star_verymagic_is_literal_star() {
    // In VeryMagic mode, $ is always an anchor. * after anchor is literal.
    use crate::parser::parse_with_magic;
    use crate::MagicMode;
    let result = parse_with_magic("$*", MagicMode::VeryMagic).unwrap();
    assert_eq!(
        result.node,
        VimPatternNode::Sequence(vec![
            VimPatternNode::EndOfLine,
            VimPatternNode::Literal('*'),
        ])
    );
}

#[test]
fn word_boundary_start_not_quantifiable() {
    assert_eq!(
        parse("\\<*word"),
        Ok(VimPatternNode::Sequence(vec![
            VimPatternNode::WordBoundaryStart,
            VimPatternNode::Literal('*'),
            VimPatternNode::Literal('w'),
            VimPatternNode::Literal('o'),
            VimPatternNode::Literal('r'),
            VimPatternNode::Literal('d'),
        ]))
    );
}

// ═══════════════════════════════════════════════════════════════════════
// ^ after \n is treated as an anchor
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn caret_after_newline_escape_is_anchor() {
    // \n^ -- the ^ after \n is StartOfLine, not literal
    assert_eq!(
        parse("\\n^foo"),
        Ok(VimPatternNode::Sequence(vec![
            VimPatternNode::EscapeSequence(EscapeKind::Newline),
            VimPatternNode::StartOfLine,
            VimPatternNode::Literal('f'),
            VimPatternNode::Literal('o'),
            VimPatternNode::Literal('o'),
        ]))
    );
}

#[test]
fn caret_after_literal_is_literal_mid_pattern() {
    // a^ -- ^ after non-newline atom is literal (not at branch start)
    assert_eq!(
        parse("a^b"),
        Ok(VimPatternNode::Sequence(vec![
            VimPatternNode::Literal('a'),
            VimPatternNode::Literal('^'),
            VimPatternNode::Literal('b'),
        ]))
    );
}

#[test]
fn caret_after_newline_escape_at_end_is_anchor() {
    // \n^ at end of pattern
    assert_eq!(
        parse("\\n^"),
        Ok(VimPatternNode::Sequence(vec![
            VimPatternNode::EscapeSequence(EscapeKind::Newline),
            VimPatternNode::StartOfLine,
        ]))
    );
}

// ═══════════════════════════════════════════════════════════════════════
// $ lookahead across mode switches -- track effective magic
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dollar_before_mode_switch_to_verymagic_alternation() {
    // $\v| -- $ is followed by \v then bare |
    // \v changes effective magic, so | is an alternation separator.
    // $ should be an anchor (EndOfLine), not a literal.
    assert_eq!(
        parse("foo$\\v|bar"),
        Ok(VimPatternNode::Alternation(vec![
            VimPatternNode::Sequence(vec![
                VimPatternNode::Literal('f'),
                VimPatternNode::Literal('o'),
                VimPatternNode::Literal('o'),
                VimPatternNode::EndOfLine,
            ]),
            VimPatternNode::Sequence(vec![
                VimPatternNode::Literal('b'),
                VimPatternNode::Literal('a'),
                VimPatternNode::Literal('r'),
            ]),
        ]))
    );
}

#[test]
fn dollar_before_mode_switch_to_nomagic_no_anchor() {
    // $\M| -- \M switches to NoMagic where | is literal, not alternation.
    // So $ is NOT at branch end and should be literal.
    assert_eq!(
        parse("a$\\M|b"),
        Ok(VimPatternNode::Sequence(vec![
            VimPatternNode::Literal('a'),
            VimPatternNode::Literal('$'),
            VimPatternNode::Literal('|'),
            VimPatternNode::Literal('b'),
        ]))
    );
}

#[test]
fn dollar_before_verymagic_close_paren() {
    // Inside a group: $\v) -- the ) after \v is a group close in VeryMagic
    // $ should be anchor
    assert_eq!(
        parse("\\(foo$\\v)bar"),
        Ok(VimPatternNode::Sequence(vec![
            VimPatternNode::Group {
                inner: Box::new(VimPatternNode::Sequence(vec![
                    VimPatternNode::Literal('f'),
                    VimPatternNode::Literal('o'),
                    VimPatternNode::Literal('o'),
                    VimPatternNode::EndOfLine,
                ])),
                capturing: true,
            },
            VimPatternNode::Literal('b'),
            VimPatternNode::Literal('a'),
            VimPatternNode::Literal('r'),
        ]))
    );
}
