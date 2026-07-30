//! Parser tests: Tasks 4-5 — groups, alternation, quantifiers, collections.

use crate::ir::{
    CharClass, CollectionItem, ParseResult, VimPatternNode, VimRegexError, VimRegexErrorKind,
};
use crate::parser::{parse_pattern, parse_with_magic};

fn parse(pattern: &str) -> Result<VimPatternNode, VimRegexError> {
    parse_pattern(pattern).map(|r| r.node)
}

fn parse_result(pattern: &str) -> Result<ParseResult, VimRegexError> {
    parse_pattern(pattern)
}

fn parse_nm(pattern: &str) -> Result<VimPatternNode, VimRegexError> {
    parse_with_magic(pattern, crate::MagicMode::NoMagic).map(|r| r.node)
}

// ═══════════════════════════════════════════════════════════════════════
// TASK 4: Groups
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn group_capture_count() {
    let r = parse_result("\\(a\\)\\(b\\)").unwrap();
    assert_eq!(r.features.capture_count, 2);
}

#[test]
fn nested_groups_capture_count() {
    let r = parse_result("\\(\\(a\\)\\)").unwrap();
    assert_eq!(r.features.capture_count, 2);
    assert_eq!(
        r.node,
        VimPatternNode::Group {
            inner: Box::new(VimPatternNode::Group {
                inner: Box::new(VimPatternNode::Literal('a')),
                capturing: true,
            }),
            capturing: true,
        }
    );
}

#[test]
fn non_capturing_group_no_count_increment() {
    let r = parse_result("\\%(abc\\)").unwrap();
    assert_eq!(r.features.capture_count, 0);
}

#[test]
fn unmatched_group_close() {
    assert!(matches!(
        parse("\\)").unwrap_err().kind,
        VimRegexErrorKind::UnmatchedGroup { .. }
    ));
}

#[test]
fn unmatched_group_open() {
    assert!(parse("\\(abc").is_err());
}

#[test]
fn group_with_single_atom() {
    assert_eq!(
        parse("\\(a\\)"),
        Ok(VimPatternNode::Group {
            inner: Box::new(VimPatternNode::Literal('a')),
            capturing: true,
        })
    );
}

#[test]
fn empty_group() {
    assert_eq!(
        parse("\\(\\)"),
        Ok(VimPatternNode::Group {
            inner: Box::new(VimPatternNode::Sequence(vec![])),
            capturing: true,
        })
    );
}

// ═══════════════════════════════════════════════════════════════════════
// TASK 4: Alternation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn alternation_three_branches() {
    assert_eq!(
        parse("a\\|b\\|c"),
        Ok(VimPatternNode::Alternation(vec![
            VimPatternNode::Literal('a'),
            VimPatternNode::Literal('b'),
            VimPatternNode::Literal('c'),
        ]))
    );
}

#[test]
fn alternation_with_sequences() {
    assert_eq!(
        parse("ab\\|cd"),
        Ok(VimPatternNode::Alternation(vec![
            VimPatternNode::Sequence(vec![
                VimPatternNode::Literal('a'),
                VimPatternNode::Literal('b'),
            ]),
            VimPatternNode::Sequence(vec![
                VimPatternNode::Literal('c'),
                VimPatternNode::Literal('d'),
            ]),
        ]))
    );
}

#[test]
fn alternation_inside_group() {
    assert_eq!(
        parse("\\(a\\|b\\)"),
        Ok(VimPatternNode::Group {
            inner: Box::new(VimPatternNode::Alternation(vec![
                VimPatternNode::Literal('a'),
                VimPatternNode::Literal('b'),
            ])),
            capturing: true,
        })
    );
}

#[test]
fn alternation_empty_branch() {
    assert_eq!(
        parse("a\\|"),
        Ok(VimPatternNode::Alternation(vec![
            VimPatternNode::Literal('a'),
            VimPatternNode::Sequence(vec![]),
        ]))
    );
}

// ═══════════════════════════════════════════════════════════════════════
// TASK 4: Quantifiers
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn quantifier_question() {
    assert_eq!(
        parse("a\\?"),
        Ok(VimPatternNode::Quantifier {
            node: Box::new(VimPatternNode::Literal('a')),
            min: 0,
            max: Some(1),
            greedy: true,
        })
    );
}

#[test]
fn quantifier_equals() {
    assert_eq!(
        parse("a\\="),
        Ok(VimPatternNode::Quantifier {
            node: Box::new(VimPatternNode::Literal('a')),
            min: 0,
            max: Some(1),
            greedy: true,
        })
    );
}

#[test]
fn quantifier_brace_range() {
    assert_eq!(
        parse("a\\{2,5}"),
        Ok(VimPatternNode::Quantifier {
            node: Box::new(VimPatternNode::Literal('a')),
            min: 2,
            max: Some(5),
            greedy: true,
        })
    );
}

#[test]
fn quantifier_brace_nongreedy_unbounded() {
    assert_eq!(
        parse("a\\{-}"),
        Ok(VimPatternNode::Quantifier {
            node: Box::new(VimPatternNode::Literal('a')),
            min: 0,
            max: None,
            greedy: false,
        })
    );
}

#[test]
fn quantifier_brace_nongreedy_range() {
    assert_eq!(
        parse("a\\{-2,5}"),
        Ok(VimPatternNode::Quantifier {
            node: Box::new(VimPatternNode::Literal('a')),
            min: 2,
            max: Some(5),
            greedy: false,
        })
    );
}

#[test]
fn quantifier_brace_exact() {
    assert_eq!(
        parse("a\\{3}"),
        Ok(VimPatternNode::Quantifier {
            node: Box::new(VimPatternNode::Literal('a')),
            min: 3,
            max: Some(3),
            greedy: true,
        })
    );
}

#[test]
fn quantifier_brace_unbounded_max() {
    assert_eq!(
        parse("a\\{2,}"),
        Ok(VimPatternNode::Quantifier {
            node: Box::new(VimPatternNode::Literal('a')),
            min: 2,
            max: None,
            greedy: true,
        })
    );
}

#[test]
fn quantifier_brace_zero_to_max() {
    assert_eq!(
        parse("a\\{,3}"),
        Ok(VimPatternNode::Quantifier {
            node: Box::new(VimPatternNode::Literal('a')),
            min: 0,
            max: Some(3),
            greedy: true,
        })
    );
}

#[test]
fn quantifier_brace_empty_greedy() {
    assert_eq!(
        parse("a\\{}"),
        Ok(VimPatternNode::Quantifier {
            node: Box::new(VimPatternNode::Literal('a')),
            min: 0,
            max: None,
            greedy: true,
        })
    );
}

#[test]
fn quantifier_star_at_start_is_literal() {
    assert_eq!(parse("*"), Ok(VimPatternNode::Literal('*')));
}

#[test]
fn quantifier_on_group() {
    assert_eq!(
        parse("\\(ab\\)*"),
        Ok(VimPatternNode::Quantifier {
            node: Box::new(VimPatternNode::Group {
                inner: Box::new(VimPatternNode::Sequence(vec![
                    VimPatternNode::Literal('a'),
                    VimPatternNode::Literal('b'),
                ])),
                capturing: true,
            }),
            min: 0,
            max: None,
            greedy: true,
        })
    );
}

#[test]
fn quantifier_nongreedy_with_limit() {
    assert_eq!(
        parse("a\\{-1,3}"),
        Ok(VimPatternNode::Quantifier {
            node: Box::new(VimPatternNode::Literal('a')),
            min: 1,
            max: Some(3),
            greedy: false,
        })
    );
}

#[test]
fn quantifier_nongreedy_exact() {
    assert_eq!(
        parse("a\\{-2}"),
        Ok(VimPatternNode::Quantifier {
            node: Box::new(VimPatternNode::Literal('a')),
            min: 2,
            max: Some(2),
            greedy: false,
        })
    );
}

// ═══════════════════════════════════════════════════════════════════════
// TASK 4: Backreferences
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn backreference_9() {
    assert_eq!(parse("\\9"), Ok(VimPatternNode::BackReference(9)));
}

#[test]
fn backreference_all_digits() {
    for d in 1..=9u8 {
        let pattern = format!("\\{d}");
        assert_eq!(parse(&pattern), Ok(VimPatternNode::BackReference(d)));
    }
}

#[test]
fn backreference_sets_feature_flag() {
    let r = parse_result("\\(a\\)\\1").unwrap();
    assert!(r.features.has_backreferences);
    assert_eq!(r.features.capture_count, 1);
}

// ═══════════════════════════════════════════════════════════════════════
// TASK 5: Collections
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn collection_simple() {
    assert_eq!(
        parse("[abc]"),
        Ok(VimPatternNode::Collection {
            negated: false,
            items: vec![
                CollectionItem::Single('a'),
                CollectionItem::Single('b'),
                CollectionItem::Single('c'),
            ],
            include_newline: false,
        })
    );
}

#[test]
fn collection_negated() {
    assert_eq!(
        parse("[^abc]"),
        Ok(VimPatternNode::Collection {
            negated: true,
            items: vec![
                CollectionItem::Single('a'),
                CollectionItem::Single('b'),
                CollectionItem::Single('c'),
            ],
            include_newline: false,
        })
    );
}

#[test]
fn collection_range() {
    assert_eq!(
        parse("[a-z]"),
        Ok(VimPatternNode::Collection {
            negated: false,
            items: vec![CollectionItem::Range('a', 'z')],
            include_newline: false,
        })
    );
}

#[test]
fn collection_multiple_ranges() {
    assert_eq!(
        parse("[a-zA-Z0-9]"),
        Ok(VimPatternNode::Collection {
            negated: false,
            items: vec![
                CollectionItem::Range('a', 'z'),
                CollectionItem::Range('A', 'Z'),
                CollectionItem::Range('0', '9'),
            ],
            include_newline: false,
        })
    );
}

#[test]
fn collection_class_inside() {
    assert_eq!(
        parse("[\\d]"),
        Ok(VimPatternNode::Collection {
            negated: false,
            items: vec![CollectionItem::Class(CharClass::Digit)],
            include_newline: false,
        })
    );
}

#[test]
fn collection_closing_bracket_first() {
    assert_eq!(
        parse("[]]"),
        Ok(VimPatternNode::Collection {
            negated: false,
            items: vec![CollectionItem::Single(']')],
            include_newline: false,
        })
    );
}

#[test]
fn collection_dash_first() {
    assert_eq!(
        parse("[-abc]"),
        Ok(VimPatternNode::Collection {
            negated: false,
            items: vec![
                CollectionItem::Single('-'),
                CollectionItem::Single('a'),
                CollectionItem::Single('b'),
                CollectionItem::Single('c'),
            ],
            include_newline: false,
        })
    );
}

#[test]
fn collection_dash_last() {
    assert_eq!(
        parse("[abc-]"),
        Ok(VimPatternNode::Collection {
            negated: false,
            items: vec![
                CollectionItem::Single('a'),
                CollectionItem::Single('b'),
                CollectionItem::Single('c'),
                CollectionItem::Single('-'),
            ],
            include_newline: false,
        })
    );
}

#[test]
fn collection_with_newline() {
    assert_eq!(
        parse("\\_[abc]"),
        Ok(VimPatternNode::Collection {
            negated: false,
            items: vec![
                CollectionItem::Single('a'),
                CollectionItem::Single('b'),
                CollectionItem::Single('c'),
            ],
            include_newline: true,
        })
    );
}

#[test]
fn collection_unclosed() {
    assert!(parse("[abc").is_err());
}

#[test]
fn collection_newline_escape() {
    assert_eq!(
        parse("[\\n]"),
        Ok(VimPatternNode::Collection {
            negated: false,
            items: vec![CollectionItem::Newline],
            include_newline: false,
        })
    );
}

#[test]
fn collection_nomagic_escaped_bracket() {
    assert_eq!(
        parse_nm("\\[abc]"),
        Ok(VimPatternNode::Collection {
            negated: false,
            items: vec![
                CollectionItem::Single('a'),
                CollectionItem::Single('b'),
                CollectionItem::Single('c'),
            ],
            include_newline: false,
        })
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Complex patterns (Tasks 4-5 combined)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn complex_word_search() {
    assert_eq!(
        parse("\\<\\w\\+\\>"),
        Ok(VimPatternNode::Sequence(vec![
            VimPatternNode::WordBoundaryStart,
            VimPatternNode::Quantifier {
                node: Box::new(VimPatternNode::Class(CharClass::Word)),
                min: 1,
                max: None,
                greedy: true,
            },
            VimPatternNode::WordBoundaryEnd,
        ]))
    );
}
