//! Parser tests: Tasks 6-8 — magic switches, buffer positions, lookaround, VeryMagic mode.

use crate::ir::{
    CollectionItem, ColumnSpec, LineSpec, LookaroundKind, MarkRel, VimPatternNode, VimRegexError,
    VimRegexErrorKind,
};
use crate::parser::{parse_pattern, parse_with_magic};

fn parse(pattern: &str) -> Result<VimPatternNode, VimRegexError> {
    parse_pattern(pattern).map(|r| r.node)
}

fn parse_result(pattern: &str) -> Result<crate::ir::ParseResult, VimRegexError> {
    parse_pattern(pattern)
}

fn parse_vm(pattern: &str) -> Result<VimPatternNode, VimRegexError> {
    parse_with_magic(pattern, crate::MagicMode::VeryMagic).map(|r| r.node)
}

fn parse_nm(pattern: &str) -> Result<VimPatternNode, VimRegexError> {
    parse_with_magic(pattern, crate::MagicMode::NoMagic).map(|r| r.node)
}

fn parse_vnm(pattern: &str) -> Result<VimPatternNode, VimRegexError> {
    parse_with_magic(pattern, crate::MagicMode::VeryNoMagic).map(|r| r.node)
}

// ═══════════════════════════════════════════════════════════════════════
// TASK 6: Magic mode switches
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn magic_switch_verymagic_group() {
    assert_eq!(
        parse("\\v(a|b)+"),
        Ok(VimPatternNode::Quantifier {
            node: Box::new(VimPatternNode::Group {
                inner: Box::new(VimPatternNode::Alternation(vec![
                    VimPatternNode::Literal('a'),
                    VimPatternNode::Literal('b'),
                ])),
                capturing: true,
            }),
            min: 1,
            max: None,
            greedy: true,
        })
    );
}

#[test]
fn magic_switch_verynomagic() {
    assert_eq!(
        parse("\\Va\\+b"),
        Ok(VimPatternNode::Sequence(vec![
            VimPatternNode::Quantifier {
                node: Box::new(VimPatternNode::Literal('a')),
                min: 1,
                max: None,
                greedy: true,
            },
            VimPatternNode::Literal('b'),
        ]))
    );
}

#[test]
fn magic_switch_mid_pattern() {
    assert_eq!(
        parse("abc\\vd.f"),
        Ok(VimPatternNode::Sequence(vec![
            VimPatternNode::Literal('a'),
            VimPatternNode::Literal('b'),
            VimPatternNode::Literal('c'),
            VimPatternNode::Literal('d'),
            VimPatternNode::AnyChar,
            VimPatternNode::Literal('f'),
        ]))
    );
}

// ═══════════════════════════════════════════════════════════════════════
// TASK 6: Buffer positions
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn percent_current_vcol() {
    assert_eq!(
        parse("\\%.v"),
        Ok(VimPatternNode::AtVirtualColumn(ColumnSpec::Current))
    );
}

#[test]
fn percent_mark_at() {
    assert_eq!(
        parse("\\%'m"),
        Ok(VimPatternNode::AtMark {
            mark: 'm',
            rel: MarkRel::At
        })
    );
    assert!(parse_result("\\%'m").unwrap().features.has_buffer_position);
}

#[test]
fn percent_mark_before() {
    assert_eq!(
        parse("\\%<'m"),
        Ok(VimPatternNode::AtMark {
            mark: 'm',
            rel: MarkRel::Before
        })
    );
}

#[test]
fn percent_mark_after() {
    assert_eq!(
        parse("\\%>'m"),
        Ok(VimPatternNode::AtMark {
            mark: 'm',
            rel: MarkRel::After
        })
    );
}

// ═══════════════════════════════════════════════════════════════════════
// TASK 6: Character codes
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn percent_char_code_decimal() {
    assert_eq!(parse("\\%d65"), Ok(VimPatternNode::CharByCode('A')));
}

#[test]
fn percent_char_code_hex() {
    assert_eq!(parse("\\%x41"), Ok(VimPatternNode::CharByCode('A')));
}

#[test]
fn percent_char_code_octal() {
    assert_eq!(parse("\\%o101"), Ok(VimPatternNode::CharByCode('A')));
}

#[test]
fn percent_char_code_unicode_4() {
    assert_eq!(parse("\\%u0041"), Ok(VimPatternNode::CharByCode('A')));
}

#[test]
fn percent_char_code_unicode_8() {
    assert_eq!(parse("\\%U00000041"), Ok(VimPatternNode::CharByCode('A')));
}

#[test]
fn percent_char_code_euro_sign() {
    // U+20AC = Euro sign
    assert_eq!(
        parse("\\%u20AC"),
        Ok(VimPatternNode::CharByCode('\u{20AC}'))
    );
}

// ═══════════════════════════════════════════════════════════════════════
// TASK 8: VeryMagic comprehensive tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn verymagic_group_alternation_quantifier_equivalence() {
    let vm = parse("\\v(a|b)+").unwrap();
    let magic = parse("\\(a\\|b\\)\\+").unwrap();
    assert_eq!(vm, magic);
}

#[test]
fn verymagic_collection_brace_quantifier_equivalence() {
    let vm = parse("\\v[a-z]{2,5}").unwrap();
    let magic = parse("[a-z]\\{2,5}").unwrap();
    assert_eq!(vm, magic);
}

#[test]
fn verymagic_dot_is_meta() {
    assert_eq!(
        parse("\\va.b"),
        Ok(VimPatternNode::Sequence(vec![
            VimPatternNode::Literal('a'),
            VimPatternNode::AnyChar,
            VimPatternNode::Literal('b'),
        ]))
    );
}

#[test]
fn verymagic_escaped_paren_is_literal() {
    assert_eq!(parse("\\v\\("), Ok(VimPatternNode::Literal('(')));
}

#[test]
fn verymagic_escaped_pipe_is_literal() {
    assert_eq!(parse("\\v\\|"), Ok(VimPatternNode::Literal('|')));
}

#[test]
fn verymagic_escaped_close_paren_is_literal() {
    assert_eq!(parse("\\v\\)"), Ok(VimPatternNode::Literal(')')));
}

#[test]
fn verymagic_escaped_dot_is_literal() {
    assert_eq!(parse_vm("\\."), Ok(VimPatternNode::Literal('.')));
}

#[test]
fn verymagic_star_is_quantifier() {
    assert_eq!(
        parse_vm("a*"),
        Ok(VimPatternNode::Quantifier {
            node: Box::new(VimPatternNode::Literal('a')),
            min: 0,
            max: None,
            greedy: true,
        })
    );
}

#[test]
fn verymagic_plus_is_quantifier() {
    assert_eq!(
        parse_vm("a+"),
        Ok(VimPatternNode::Quantifier {
            node: Box::new(VimPatternNode::Literal('a')),
            min: 1,
            max: None,
            greedy: true,
        })
    );
}

#[test]
fn verymagic_question_is_quantifier() {
    assert_eq!(
        parse_vm("a?"),
        Ok(VimPatternNode::Quantifier {
            node: Box::new(VimPatternNode::Literal('a')),
            min: 0,
            max: Some(1),
            greedy: true,
        })
    );
}

#[test]
fn verymagic_brace_quantifier() {
    assert_eq!(
        parse_vm("a{2,5}"),
        Ok(VimPatternNode::Quantifier {
            node: Box::new(VimPatternNode::Literal('a')),
            min: 2,
            max: Some(5),
            greedy: true,
        })
    );
}

#[test]
fn verymagic_bare_paren_is_group() {
    assert_eq!(
        parse_vm("(abc)"),
        Ok(VimPatternNode::Group {
            inner: Box::new(VimPatternNode::Sequence(vec![
                VimPatternNode::Literal('a'),
                VimPatternNode::Literal('b'),
                VimPatternNode::Literal('c'),
            ])),
            capturing: true,
        })
    );
}

#[test]
fn verymagic_bare_pipe_is_alternation() {
    assert_eq!(
        parse_vm("a|b"),
        Ok(VimPatternNode::Alternation(vec![
            VimPatternNode::Literal('a'),
            VimPatternNode::Literal('b'),
        ]))
    );
}

#[test]
fn verymagic_collection() {
    assert_eq!(
        parse_vm("[a-z]"),
        Ok(VimPatternNode::Collection {
            negated: false,
            items: vec![CollectionItem::Range('a', 'z')],
            include_newline: false,
        })
    );
}

// ═══════════════════════════════════════════════════════════════════════
// TASK 8: NoMagic mode tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn nomagic_dot_is_literal() {
    assert_eq!(parse_nm("."), Ok(VimPatternNode::Literal('.')));
}

#[test]
fn nomagic_star_is_literal() {
    assert_eq!(parse_nm("*"), Ok(VimPatternNode::Literal('*')));
}

#[test]
fn nomagic_caret_dollar_still_meta() {
    assert_eq!(
        parse_nm("^$"),
        Ok(VimPatternNode::Sequence(vec![
            VimPatternNode::StartOfLine,
            VimPatternNode::EndOfLine,
        ]))
    );
}

#[test]
fn nomagic_escaped_dot_plus() {
    assert_eq!(
        parse("\\M\\.\\+"),
        Ok(VimPatternNode::Quantifier {
            node: Box::new(VimPatternNode::AnyChar),
            min: 1,
            max: None,
            greedy: true,
        })
    );
}

#[test]
fn nomagic_escaped_star_is_quantifier() {
    assert_eq!(
        parse_nm("a\\*"),
        Ok(VimPatternNode::Quantifier {
            node: Box::new(VimPatternNode::Literal('a')),
            min: 0,
            max: None,
            greedy: true,
        })
    );
}

#[test]
fn nomagic_bracket_is_literal() {
    assert_eq!(parse_nm("["), Ok(VimPatternNode::Literal('[')));
}

#[test]
fn nomagic_escaped_bracket_is_collection() {
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

#[test]
fn nomagic_group_via_backslash() {
    assert_eq!(
        parse_nm("\\(a\\)"),
        Ok(VimPatternNode::Group {
            inner: Box::new(VimPatternNode::Literal('a')),
            capturing: true,
        })
    );
}

#[test]
fn nomagic_alternation_via_backslash() {
    assert_eq!(
        parse_nm("a\\|b"),
        Ok(VimPatternNode::Alternation(vec![
            VimPatternNode::Literal('a'),
            VimPatternNode::Literal('b'),
        ]))
    );
}

// ═══════════════════════════════════════════════════════════════════════
// TASK 8: VeryNoMagic mode tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn verynomagic_everything_literal() {
    assert_eq!(
        parse_vnm("..."),
        Ok(VimPatternNode::Sequence(vec![
            VimPatternNode::Literal('.'),
            VimPatternNode::Literal('.'),
            VimPatternNode::Literal('.'),
        ]))
    );
}

#[test]
fn verynomagic_caret_is_literal() {
    assert_eq!(parse_vnm("^"), Ok(VimPatternNode::Literal('^')));
}

#[test]
fn verynomagic_dollar_is_literal() {
    assert_eq!(parse_vnm("$"), Ok(VimPatternNode::Literal('$')));
}

#[test]
fn verynomagic_backslash_still_escapes() {
    assert_eq!(parse_vnm("\\."), Ok(VimPatternNode::AnyChar));
}

#[test]
fn verynomagic_group_via_backslash() {
    assert_eq!(
        parse_vnm("\\(a\\)"),
        Ok(VimPatternNode::Group {
            inner: Box::new(VimPatternNode::Literal('a')),
            capturing: true,
        })
    );
}

#[test]
fn verynomagic_alternation_via_backslash() {
    assert_eq!(
        parse_vnm("a\\|b"),
        Ok(VimPatternNode::Alternation(vec![
            VimPatternNode::Literal('a'),
            VimPatternNode::Literal('b'),
        ]))
    );
}

// ═══════════════════════════════════════════════════════════════════════
// TASK 7: Lookaround and remaining atoms
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn lookaround_positive_ahead() {
    assert_eq!(
        parse("f\\@="),
        Ok(VimPatternNode::Lookaround {
            inner: Box::new(VimPatternNode::Literal('f')),
            kind: LookaroundKind::PositiveAhead,
            limit: None,
        })
    );
}

#[test]
fn lookaround_negative_ahead() {
    assert_eq!(
        parse("f\\@!"),
        Ok(VimPatternNode::Lookaround {
            inner: Box::new(VimPatternNode::Literal('f')),
            kind: LookaroundKind::NegativeAhead,
            limit: None,
        })
    );
}

#[test]
fn lookaround_positive_behind() {
    assert_eq!(
        parse("f\\@<="),
        Ok(VimPatternNode::Lookaround {
            inner: Box::new(VimPatternNode::Literal('f')),
            kind: LookaroundKind::PositiveBehind,
            limit: None,
        })
    );
}

#[test]
fn lookaround_negative_behind() {
    assert_eq!(
        parse("f\\@<!"),
        Ok(VimPatternNode::Lookaround {
            inner: Box::new(VimPatternNode::Literal('f')),
            kind: LookaroundKind::NegativeBehind,
            limit: None,
        })
    );
}

#[test]
fn lookaround_atomic() {
    assert_eq!(
        parse("f\\@>"),
        Ok(VimPatternNode::Lookaround {
            inner: Box::new(VimPatternNode::Literal('f')),
            kind: LookaroundKind::Atomic,
            limit: None,
        })
    );
}

#[test]
fn lookaround_with_limit() {
    assert_eq!(
        parse("f\\@123<="),
        Ok(VimPatternNode::Lookaround {
            inner: Box::new(VimPatternNode::Literal('f')),
            kind: LookaroundKind::PositiveBehind,
            limit: Some(123),
        })
    );
}

#[test]
fn lookaround_negative_behind_with_limit() {
    assert_eq!(
        parse("f\\@50<!"),
        Ok(VimPatternNode::Lookaround {
            inner: Box::new(VimPatternNode::Literal('f')),
            kind: LookaroundKind::NegativeBehind,
            limit: Some(50),
        })
    );
}

#[test]
fn lookaround_sets_feature_flag() {
    assert!(parse_result("f\\@=").unwrap().features.has_lookaround);
}

#[test]
fn lookaround_on_group() {
    assert_eq!(
        parse("\\(foo\\)\\@="),
        Ok(VimPatternNode::Lookaround {
            inner: Box::new(VimPatternNode::Group {
                inner: Box::new(VimPatternNode::Sequence(vec![
                    VimPatternNode::Literal('f'),
                    VimPatternNode::Literal('o'),
                    VimPatternNode::Literal('o'),
                ])),
                capturing: true,
            }),
            kind: LookaroundKind::PositiveAhead,
            limit: None,
        })
    );
}

#[test]
fn optional_sequence() {
    assert_eq!(
        parse("\\%[abc]"),
        Ok(VimPatternNode::OptionalSequence(vec![
            VimPatternNode::Literal('a'),
            VimPatternNode::Literal('b'),
            VimPatternNode::Literal('c'),
        ]))
    );
}

#[test]
fn lookahead_followed_by_literal() {
    assert_eq!(
        parse("f\\@=g"),
        Ok(VimPatternNode::Sequence(vec![
            VimPatternNode::Lookaround {
                inner: Box::new(VimPatternNode::Literal('f')),
                kind: LookaroundKind::PositiveAhead,
                limit: None,
            },
            VimPatternNode::Literal('g'),
        ]))
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Fix 2: \v) at top level produces E55 (UnmatchedGroup)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn verymagic_unmatched_close_paren_at_top_level() {
    // \v) at group_depth 0 -> E55 UnmatchedGroup
    assert!(matches!(
        parse_vm(")abc"),
        Err(ref e) if matches!(e.kind, VimRegexErrorKind::UnmatchedGroup { .. })
    ));
}

#[test]
fn verymagic_unmatched_close_paren_after_atoms() {
    // \v abc) -- the ) is at top level after some atoms
    assert!(matches!(
        parse_vm("abc)"),
        Err(ref e) if matches!(e.kind, VimRegexErrorKind::UnmatchedGroup { .. })
    ));
}

#[test]
fn verymagic_matched_close_paren_inside_group_ok() {
    // \v (a) -- matched parens are fine
    assert_eq!(
        parse_vm("(a)b"),
        Ok(VimPatternNode::Sequence(vec![
            VimPatternNode::Group {
                inner: Box::new(VimPatternNode::Literal('a')),
                capturing: true,
            },
            VimPatternNode::Literal('b'),
        ]))
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Fix 3: \%[...] atom validation -- reject disallowed node types
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn optional_sequence_valid_atoms() {
    // \%[abc] -- all literals, should parse fine
    assert_eq!(
        parse("\\%[abc]"),
        Ok(VimPatternNode::OptionalSequence(vec![
            VimPatternNode::Literal('a'),
            VimPatternNode::Literal('b'),
            VimPatternNode::Literal('c'),
        ]))
    );
}

#[test]
fn optional_sequence_rejects_group() {
    // \%[\(a\)] -- group inside \%[] is invalid
    assert!(matches!(
        parse("\\%[\\(a\\)]"),
        Err(ref e) if matches!(e.kind, VimRegexErrorKind::InvalidOptionalSequenceAtom { .. })
    ));
}

#[test]
fn optional_sequence_rejects_backreference() {
    // \%[\1] -- backreference inside \%[] is invalid
    assert!(matches!(
        parse("\\%[\\1]"),
        Err(ref e) if matches!(e.kind, VimRegexErrorKind::InvalidOptionalSequenceAtom { .. })
    ));
}

#[test]
fn optional_sequence_allows_char_class() {
    use crate::ir::CharClass;
    // \%[\d.] -- class and dot are valid
    assert_eq!(
        parse("\\%[\\d.]"),
        Ok(VimPatternNode::OptionalSequence(vec![
            VimPatternNode::Class(CharClass::Digit),
            VimPatternNode::AnyChar,
        ]))
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Before/After Current Line/Column (\%<.l, \%>.l, \%<.c, \%>.c, etc.)
// ═══��═════════════════════════════════════════���═════════════════════════

#[test]
fn percent_before_current_line() {
    assert_eq!(
        parse("\\%<.l"),
        Ok(VimPatternNode::AtLine(LineSpec::BeforeCurrent))
    );
}

#[test]
fn percent_after_current_line() {
    assert_eq!(
        parse("\\%>.l"),
        Ok(VimPatternNode::AtLine(LineSpec::AfterCurrent))
    );
}

#[test]
fn percent_before_current_col() {
    assert_eq!(
        parse("\\%<.c"),
        Ok(VimPatternNode::AtColumn(ColumnSpec::BeforeCurrent))
    );
}

#[test]
fn percent_after_current_col() {
    assert_eq!(
        parse("\\%>.c"),
        Ok(VimPatternNode::AtColumn(ColumnSpec::AfterCurrent))
    );
}

#[test]
fn percent_before_current_vcol() {
    assert_eq!(
        parse("\\%<.v"),
        Ok(VimPatternNode::AtVirtualColumn(ColumnSpec::BeforeCurrent))
    );
}

#[test]
fn percent_after_current_vcol() {
    assert_eq!(
        parse("\\%>.v"),
        Ok(VimPatternNode::AtVirtualColumn(ColumnSpec::AfterCurrent))
    );
}

#[test]
fn percent_before_current_line_sets_buffer_position() {
    assert!(parse_result("\\%<.l").unwrap().features.has_buffer_position);
}

#[test]
fn percent_after_current_col_sets_buffer_position() {
    assert!(parse_result("\\%>.c").unwrap().features.has_buffer_position);
}

#[test]
fn percent_before_current_line_in_sequence() {
    assert_eq!(
        parse("\\%<.lfoo"),
        Ok(VimPatternNode::Sequence(vec![
            VimPatternNode::AtLine(LineSpec::BeforeCurrent),
            VimPatternNode::Literal('f'),
            VimPatternNode::Literal('o'),
            VimPatternNode::Literal('o'),
        ]))
    );
}

#[test]
fn percent_relation_current_invalid_suffix() {
    // \%<.x is not a valid suffix — should error
    assert!(parse("\\%<.x").is_err());
}

#[test]
fn percent_relation_without_number_errors() {
    // \%<l without a number should error (not silently parse 0)
    assert!(parse("\\%<l").is_err(), r"\%<l without number should error");
    assert!(parse("\\%>c").is_err(), r"\%>c without number should error");
}

#[test]
fn empty_optional_sequence_errors() {
    // \%[] should error (empty optional sequence)
    assert!(
        parse("\\%[]").is_err(),
        r"\%[] should error (empty optional sequence)"
    );
}

#[test]
fn octal_escape_max_three_digits() {
    use crate::engine::VimRegex;
    use crate::matchers::MatchContext;

    // \%o141 = 'a' (octal 141 = decimal 97)
    let re = VimRegex::new(r"\%o141").unwrap();
    assert!(re.is_match(&MatchContext::simple("a")).unwrap());

    // \%o1414 — should parse as \%o141 followed by literal '4'
    // (only 3 octal digits consumed)
    let re2 = VimRegex::new(r"\%o1414").unwrap();
    assert!(re2.is_match(&MatchContext::simple("a4")).unwrap());
}
