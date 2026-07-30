//! Tests for pattern-compilation diagnostics.
//!
//! Covers: Span byte-offset correctness (ASCII and multi-byte), split
//! InvalidCollection variants with detail strings, pattern context in Display,
//! and Vim error codes in Display output.

use crate::{Span, VimPatternNode, VimRegex, VimRegexError, VimRegexErrorKind};

// ═══════════════════════════════════════════════════════════════════════════════
// Span correctness — ASCII patterns
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn unmatched_group_span_points_to_open_paren() {
    let err = VimRegex::new(r"ab\(cd").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::UnmatchedGroup { span } => {
            // open_pos is at '(' which is char 3, byte 3 in ASCII "ab\(cd"
            // span_from(3) at end-of-input (pos=6) gives byte 3..6
            assert_eq!(span.start, 3);
        }
        other => panic!("expected UnmatchedGroup, got {other:?}"),
    }
}

#[test]
fn trailing_backslash_span() {
    let err = VimRegex::new(r"abc\").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::TrailingBackslash { span } => {
            assert_eq!(span.start, 3); // byte offset of '\'
        }
        other => panic!("expected TrailingBackslash, got {other:?}"),
    }
}

#[test]
fn unterminated_collection_span() {
    let err = VimRegex::new("[abc").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::UnterminatedCollection { span } => {
            assert_eq!(span.start, 0); // byte offset of '['
        }
        other => panic!("expected UnterminatedCollection, got {other:?}"),
    }
}

#[test]
fn invalid_collection_range_detail() {
    let err = VimRegex::new("[z-a]").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::InvalidCollectionRange { span, detail } => {
            assert_eq!(span.start, 0);
            assert!(detail.contains("less than"), "detail: {detail}");
        }
        other => panic!("expected InvalidCollectionRange, got {other:?}"),
    }
}

#[test]
fn unknown_posix_class_span_and_name() {
    let err = VimRegex::new("[[:bogus:]]").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::UnknownPosixClass { span, name } => {
            assert_eq!(span.start, 0);
            assert_eq!(name.as_str(), "bogus");
        }
        other => panic!("expected UnknownPosixClass, got {other:?}"),
    }
}

#[test]
fn invalid_char_code_no_digits() {
    let err = VimRegex::new(r"\%dz").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::InvalidCharCode { span, detail } => {
            assert_eq!(span.start, 0); // \% starts at byte 0
            assert!(detail.contains("no digits"), "detail: {detail}");
        }
        other => panic!("expected InvalidCharCode, got {other:?}"),
    }
}

#[test]
fn invalid_char_code_bad_codepoint() {
    // \%UFFFFFF00 — too large for Unicode → CharCodeOutOfRange
    let err = VimRegex::new(r"\%UFFFFFF00").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::CharCodeOutOfRange { span, value } => {
            assert_eq!(span.start, 0);
            assert_eq!(*value, 0xFFFF_FF00);
        }
        other => panic!("expected CharCodeOutOfRange, got {other:?}"),
    }
}

#[test]
fn invalid_quantifier_span() {
    // \{a} — the 'a' is not a digit/comma/close-brace, triggers error
    let err = VimRegex::new(r"x\{a}").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::InvalidQuantifier { span, detail } => {
            // brace_pos = self.pos.saturating_sub(1) = position of '{'
            // In "x\{a}", chars: x=0, \=1, {=2, a=3, }=4
            // After consuming \{ we are at pos=3, brace_pos = 2
            // span_from(2) at pos=3 gives byte 2..3
            assert_eq!(span.start, 2);
            assert!(
                detail.contains("expected number or comma"),
                "detail: {detail}"
            );
        }
        other => panic!("expected InvalidQuantifier, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Span correctness — multi-byte patterns
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn span_byte_offset_with_multibyte_chars() {
    // "a\u{00E9}\(cd" — chars: a(1byte), e-acute(2bytes), \(1byte), ((1byte), c(1), d(1)
    // Char indices:  a=0, e=1, \=2, (=3, c=4, d=5
    // Byte offsets:  a=0, e=1, \=3, (=4, c=5, d=6
    // open_pos (the '(' char) is char index 3 -> byte 4
    let pattern = "a\u{00E9}\\(cd";
    let err = VimRegex::new(pattern).unwrap_err();
    match &err.kind {
        VimRegexErrorKind::UnmatchedGroup { span } => {
            // open_pos is char 3 = byte 4 (after 'a'=1byte + e-acute=2bytes + '\'=1byte)
            assert_eq!(span.start, 4);
        }
        other => panic!("expected UnmatchedGroup, got {other:?}"),
    }
}

#[test]
fn span_byte_offset_with_cjk_before_error() {
    // "\u{4E16}" (CJK) is 3 bytes. Pattern: "\u{4E16}\\" -> trailing backslash at byte 3
    let pattern = "\u{4E16}\\";
    let err = VimRegex::new(pattern).unwrap_err();
    match &err.kind {
        VimRegexErrorKind::TrailingBackslash { span } => {
            assert_eq!(span.start, 3); // after 3-byte CJK char
        }
        other => panic!("expected TrailingBackslash, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Pattern context in Display
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn error_display_includes_pattern_context() {
    let err = VimRegex::new(r"ab\(cd").unwrap_err();
    let display = err.to_string();
    assert!(display.contains("in pattern '"), "display: {display}");
    assert!(display.contains(r"ab\(cd"), "display: {display}");
}

#[test]
fn pattern_context_truncated_for_long_patterns() {
    let long = "a".repeat(100) + r"\(";
    let err = VimRegex::new(&long).unwrap_err();
    let display = err.to_string();
    assert!(
        display.contains("..."),
        "long pattern should be truncated: {display}"
    );
}

#[test]
fn pattern_context_not_present_for_parser_direct_errors() {
    // Parser errors without compile_pattern don't have context
    use crate::parser::parse_pattern;
    let err = parse_pattern("").unwrap_err();
    assert!(err.pattern_context().is_none());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Vim error codes in Display
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn display_includes_vim_error_code_e54() {
    let err = VimRegex::new(r"\)").unwrap_err();
    assert!(err.to_string().starts_with("E54:"), "got: {}", err);
}

#[test]
fn display_includes_vim_error_code_e69_unterminated() {
    let err = VimRegex::new("[abc").unwrap_err();
    assert!(err.to_string().starts_with("E69:"), "got: {}", err);
}

#[test]
fn display_includes_vim_error_code_e69_invalid_range() {
    let err = VimRegex::new("[z-a]").unwrap_err();
    assert!(err.to_string().starts_with("E69:"), "got: {}", err);
}

#[test]
fn display_includes_vim_error_code_e69_unknown_posix() {
    let err = VimRegex::new("[[:bogus:]]").unwrap_err();
    assert!(err.to_string().starts_with("E69:"), "got: {}", err);
}

#[test]
fn display_includes_vim_error_code_e476_trailing() {
    let err = VimRegex::new(r"abc\").unwrap_err();
    assert!(err.to_string().starts_with("E476:"), "got: {}", err);
}

#[test]
fn display_includes_vim_error_code_e476_quantifier() {
    let err = VimRegex::new(r"x\{a}").unwrap_err();
    assert!(err.to_string().starts_with("E476:"), "got: {}", err);
}

#[test]
fn display_includes_vim_error_code_e35() {
    let err = VimRegex::new("").unwrap_err();
    assert!(err.to_string().starts_with("E35:"), "got: {}", err);
}

#[test]
fn display_includes_vim_error_code_e678() {
    let err = VimRegex::new(r"\%dz").unwrap_err();
    assert!(err.to_string().starts_with("E678:"), "got: {}", err);
}

#[test]
fn display_includes_vim_error_code_e339() {
    // Deep nesting triggers PatternTooComplex
    let mut pattern = String::new();
    for _ in 0..250 {
        pattern.push_str("\\%(");
    }
    pattern.push('x');
    for _ in 0..250 {
        pattern.push_str("\\)");
    }
    let err = VimRegex::new(&pattern).unwrap_err();
    assert!(err.to_string().starts_with("E339:"), "got: {}", err);
}

#[test]
fn display_includes_vim_error_code_e523() {
    let err = VimRegexError::from(VimRegexErrorKind::ExpressionReplacementNotSupported);
    assert!(err.to_string().starts_with("E523:"), "got: {}", err);
}

// ═══════════════════════════════════════════════════════════════════════════════
// VimRegexError struct methods
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn with_pattern_attaches_context() {
    let err = VimRegexError::from(VimRegexErrorKind::EmptyPattern);
    assert!(err.pattern_context().is_none());

    let err = err.with_pattern("test_pattern");
    assert_eq!(err.pattern_context(), Some("test_pattern"));
}

#[test]
fn span_accessor_returns_correct_span() {
    let err = VimRegexError::from(VimRegexErrorKind::TrailingBackslash {
        span: Span::new(10, 11),
    });
    let span = err.span().unwrap();
    assert_eq!(span.start, 10);
    assert_eq!(span.end, 11);
}

#[test]
fn span_accessor_returns_none_for_empty_pattern() {
    let err = VimRegexError::from(VimRegexErrorKind::EmptyPattern);
    assert!(err.span().is_none());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Pattern-context truncation — multi-byte safety
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn pattern_context_truncation_does_not_panic_on_multibyte() {
    // 30 x 3-byte CJK characters = 90 bytes > 60 limit.
    // Byte 60 lands at the start of the 21st character (bytes 60-62).
    // floor_char_boundary(60) should return 60 (start of 21st char).
    let cjk = "\u{4E16}".repeat(30); // 90 bytes
    let pattern = format!("{}\\(", cjk); // add unmatched group
    let err = VimRegex::new(&pattern).unwrap_err();
    let ctx = err.pattern_context().unwrap();
    assert!(ctx.ends_with("..."), "context should be truncated: {ctx}");
    assert!(
        ctx.len() <= 63,
        "truncated context too long: {} bytes",
        ctx.len()
    );
}

#[test]
fn pattern_context_truncation_on_2byte_boundary() {
    // 31 x 2-byte e-acute = 62 bytes. Byte 60 is middle of 31st char.
    // floor_char_boundary(60) = 58 (end of 29th char, which is 2*29=58).
    let acute = "\u{00E9}".repeat(31); // 62 bytes
    let err = VimRegex::new(&format!("{}\\(", acute)).unwrap_err();
    let ctx = err.pattern_context().unwrap();
    assert!(ctx.ends_with("..."));
}

#[test]
fn pattern_context_truncation_on_4byte_boundary() {
    // 16 x 4-byte emoji = 64 bytes. Byte 60 is in the middle of the 16th char.
    let emoji = "\u{1F600}".repeat(16); // 64 bytes
    let err = VimRegex::new(&format!("{}\\(", emoji)).unwrap_err();
    let ctx = err.pattern_context().unwrap();
    assert!(ctx.ends_with("..."));
    // Verify no partial character at the boundary
    let trimmed = ctx.trim_end_matches("...");
    assert!(
        trimmed.is_char_boundary(trimmed.len()),
        "boundary not valid: {ctx}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// InvalidPercentEscape variant tests
// ═══════════════════════════════════════════════════════════════════════════════

use crate::PercentEscapeContext;

#[test]
fn percent_escape_end_of_input() {
    let err = VimRegex::new(r"\%").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::InvalidPercentEscape { found, .. } => {
            assert_eq!(*found, PercentEscapeContext::EndOfInput);
        }
        other => panic!("expected InvalidPercentEscape, got {other:?}"),
    }
}

#[test]
fn percent_escape_unrecognized_char() {
    let err = VimRegex::new(r"\%z").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::InvalidPercentEscape { found, .. } => {
            assert_eq!(*found, PercentEscapeContext::UnrecognizedChar);
        }
        other => panic!("expected InvalidPercentEscape, got {other:?}"),
    }
}

#[test]
fn percent_escape_missing_mark_char() {
    let err = VimRegex::new(r"\%'").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::InvalidPercentEscape { found, .. } => {
            assert_eq!(*found, PercentEscapeContext::MissingMarkChar);
        }
        other => panic!("expected InvalidPercentEscape, got {other:?}"),
    }
}

#[test]
fn percent_escape_missing_position_suffix() {
    let err = VimRegex::new(r"\%23").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::InvalidPercentEscape { found, .. } => {
            assert_eq!(*found, PercentEscapeContext::MissingPositionSuffix);
        }
        other => panic!("expected InvalidPercentEscape, got {other:?}"),
    }
}

#[test]
fn percent_escape_invalid_position_suffix() {
    let err = VimRegex::new(r"\%23z").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::InvalidPercentEscape { found, .. } => {
            assert_eq!(*found, PercentEscapeContext::InvalidPositionSuffix);
        }
        other => panic!("expected InvalidPercentEscape, got {other:?}"),
    }
}

#[test]
fn percent_escape_missing_position_suffix_dot() {
    // \%. at end of input (no l/c/v suffix)
    let err = VimRegex::new(r"\%.").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::InvalidPercentEscape { found, .. } => {
            assert_eq!(*found, PercentEscapeContext::MissingPositionSuffix);
        }
        other => panic!("expected InvalidPercentEscape, got {other:?}"),
    }
}

#[test]
fn percent_escape_invalid_position_suffix_dot() {
    // \%.z — invalid suffix after dot
    let err = VimRegex::new(r"\%.z").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::InvalidPercentEscape { found, .. } => {
            assert_eq!(*found, PercentEscapeContext::InvalidPositionSuffix);
        }
        other => panic!("expected InvalidPercentEscape, got {other:?}"),
    }
}

#[test]
fn percent_escape_missing_mark_char_relation() {
    // \%<' at end of input — mark char missing
    let err = VimRegex::new(r"\%<'").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::InvalidPercentEscape { found, .. } => {
            assert_eq!(*found, PercentEscapeContext::MissingMarkChar);
        }
        other => panic!("expected InvalidPercentEscape, got {other:?}"),
    }
}

#[test]
fn percent_escape_display_format() {
    let err = VimRegex::new(r"\%z").unwrap_err();
    let display = err.to_string();
    assert!(display.starts_with("E71:"), "got: {display}");
    assert!(display.contains("unrecognized"), "got: {display}");
}

// ═══════════════════════════════════════════════════════════════════════════════
// InternalError variant tests
// ═══════════════════════════════════════════════════════════════════════════════

use compact_str::CompactString;

#[test]
fn internal_error_display() {
    let err = VimRegexError::from(VimRegexErrorKind::InternalError {
        detail: CompactString::from("test internal failure"),
    });
    let display = err.to_string();
    assert!(display.contains("Internal regex error"), "got: {display}");
    assert!(display.contains("test internal failure"), "got: {display}");
}

#[test]
fn internal_error_has_no_span() {
    let err = VimRegexError::from(VimRegexErrorKind::InternalError {
        detail: CompactString::from("test"),
    });
    assert!(err.span().is_none());
}

#[test]
fn internal_error_display_includes_error_code() {
    let err = VimRegexError::from(VimRegexErrorKind::InternalError {
        detail: CompactString::from("cascade exhausted"),
    });
    let display = err.to_string();
    assert!(display.starts_with("E342:"), "got: {display}");
}

// ═══════════════════════════════════════════════════════════════════════════════
// InvalidCollectionCharCode detail tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn invalid_collection_char_code_has_detail() {
    // \d4294967295 (u32::MAX) produces an invalid code point
    let err = VimRegex::new(r"[\d4294967295]").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::InvalidCollectionCharCode { detail, .. } => {
            assert!(detail.contains("not a valid Unicode"), "detail: {detail}");
            assert!(detail.contains("decimal"), "detail: {detail}");
        }
        other => panic!("expected InvalidCollectionCharCode, got {other:?}"),
    }
}

#[test]
fn invalid_collection_char_code_hex_detail() {
    // \UFFFFFF00 in a collection
    let err = VimRegex::new(r"[\UFFFFFF00]").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::InvalidCollectionCharCode { detail, .. } => {
            assert!(detail.contains("8-digit hex"), "detail: {detail}");
        }
        other => panic!("expected InvalidCollectionCharCode, got {other:?}"),
    }
}

#[test]
fn invalid_collection_char_code_display_includes_detail() {
    let err = VimRegex::new(r"[\d4294967295]").unwrap_err();
    let display = err.to_string();
    assert!(display.starts_with("E678:"), "got: {display}");
    assert!(display.contains("not a valid Unicode"), "got: {display}");
}

// ═══════════════════════════════════════════════════════════════════════════════
// CharCodeOutOfRange / CharCodeSurrogate split
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn char_code_out_of_range_decimal() {
    // \%d4294967295 — u32::MAX = 4294967295, which is > 0x10FFFF
    let err = VimRegex::new(r"\%d4294967295").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::CharCodeOutOfRange { span, value } => {
            assert_eq!(span.start, 0);
            assert!(
                *value > 0x10FFFF,
                "value should exceed Unicode max: {value}"
            );
        }
        other => panic!("expected CharCodeOutOfRange, got {other:?}"),
    }
}

#[test]
fn char_code_out_of_range_hex() {
    // \%UFFFFFF00 — 0xFFFFFF00 > 0x10FFFF
    let err = VimRegex::new(r"\%UFFFFFF00").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::CharCodeOutOfRange { span, value } => {
            assert_eq!(span.start, 0);
            assert_eq!(*value, 0xFFFF_FF00);
        }
        other => panic!("expected CharCodeOutOfRange, got {other:?}"),
    }
}

#[test]
fn char_code_out_of_range_display_e678() {
    let err = VimRegex::new(r"\%UFFFFFF00").unwrap_err();
    let display = err.to_string();
    assert!(display.starts_with("E678:"), "got: {display}");
    assert!(display.contains("out of range"), "got: {display}");
    assert!(display.contains("U+10FFFF"), "got: {display}");
}

#[test]
fn char_code_surrogate_low() {
    // \%uD800 — first surrogate code point
    let err = VimRegex::new(r"\%uD800").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::CharCodeSurrogate { span, value } => {
            assert_eq!(span.start, 0);
            assert_eq!(*value, 0xD800);
        }
        other => panic!("expected CharCodeSurrogate, got {other:?}"),
    }
}

#[test]
fn char_code_surrogate_high() {
    // \%uDFFF — last surrogate code point
    let err = VimRegex::new(r"\%uDFFF").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::CharCodeSurrogate { value, .. } => {
            assert_eq!(*value, 0xDFFF);
        }
        other => panic!("expected CharCodeSurrogate, got {other:?}"),
    }
}

#[test]
fn char_code_surrogate_mid() {
    // \%uDA00 — middle of surrogate range
    let err = VimRegex::new(r"\%uDA00").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::CharCodeSurrogate { .. } => {}
        other => panic!("expected CharCodeSurrogate, got {other:?}"),
    }
}

#[test]
fn char_code_surrogate_display_e679() {
    let err = VimRegex::new(r"\%uD800").unwrap_err();
    let display = err.to_string();
    assert!(display.starts_with("E679:"), "got: {display}");
    assert!(display.contains("surrogate"), "got: {display}");
}

#[test]
fn char_code_just_below_surrogate_succeeds() {
    // \%uD7FF — one below surrogate range, should succeed
    let result = VimRegex::new(r"\%uD7FF");
    assert!(result.is_ok(), "U+D7FF should be valid: {:?}", result.err());
}

#[test]
fn char_code_just_above_surrogate_succeeds() {
    // \%uE000 — one above surrogate range, should succeed
    let result = VimRegex::new(r"\%uE000");
    assert!(result.is_ok(), "U+E000 should be valid: {:?}", result.err());
}

#[test]
fn char_code_max_unicode_succeeds() {
    // \%U0010FFFF — exactly the maximum, should succeed
    let result = VimRegex::new(r"\%U0010FFFF");
    assert!(
        result.is_ok(),
        "U+10FFFF should be valid: {:?}",
        result.err()
    );
}

#[test]
fn char_code_one_above_max_fails() {
    // \%U00110000 — one above max Unicode
    let err = VimRegex::new(r"\%U00110000").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::CharCodeOutOfRange { value, .. } => {
            assert_eq!(*value, 0x0011_0000);
        }
        other => panic!("expected CharCodeOutOfRange, got {other:?}"),
    }
}

#[test]
fn char_code_no_digits_still_invalid_char_code() {
    // \%dz — no digits, should still be InvalidCharCode (not a range/surrogate issue)
    let err = VimRegex::new(r"\%dz").unwrap_err();
    match &err.kind {
        VimRegexErrorKind::InvalidCharCode { detail, .. } => {
            assert!(detail.contains("no digits"), "detail: {detail}");
        }
        other => panic!("expected InvalidCharCode for no-digits case, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AuxiliarySpan and DiagnosticRenderer
// ═══════════════════════════════════════════════════════════════════════════════

use crate::ir::AuxiliarySpan;
use crate::DiagnosticRenderer;

#[test]
fn auxiliary_span_construction() {
    let aux = AuxiliarySpan::new(Span::new(10, 12), Span::new(3, 5), "first defined here");
    assert_eq!(aux.primary, Span::new(10, 12));
    assert_eq!(aux.auxiliary, Span::new(3, 5));
    assert_eq!(aux.auxiliary_label, "first defined here");
}

#[test]
fn auxiliary_span_equality() {
    let a = AuxiliarySpan::new(Span::new(0, 2), Span::new(5, 7), "here");
    let b = AuxiliarySpan::new(Span::new(0, 2), Span::new(5, 7), "here");
    assert_eq!(a, b);
}

#[test]
fn auxiliary_span_inequality_different_primary() {
    let a = AuxiliarySpan::new(Span::new(0, 2), Span::new(5, 7), "here");
    let b = AuxiliarySpan::new(Span::new(1, 3), Span::new(5, 7), "here");
    assert_ne!(a, b);
}

#[test]
fn render_with_auxiliary_includes_both_annotations() {
    let pattern = r"foo\(bar\(baz";
    let err = VimRegex::new(pattern).unwrap_err();
    let renderer = DiagnosticRenderer::new(pattern);

    let aux = AuxiliarySpan::new(
        Span::new(8, 10), // second \( at "bar\("
        Span::new(3, 5),  // first \( at "foo\("
        "first opened here",
    );

    let output = renderer.render_with_auxiliary(&err, &aux);

    // Should contain the error message
    assert!(output.contains("E54:"), "output:\n{output}");
    // Should contain carets for primary span
    assert!(output.contains('^'), "output:\n{output}");
    // Should contain the auxiliary label
    assert!(output.contains("first opened here"), "output:\n{output}");
    // Should contain "note:" prefix
    assert!(output.contains("note:"), "output:\n{output}");
}

#[test]
fn render_with_auxiliary_char_code_out_of_range_suggestion() {
    let pattern = r"\%UFFFFFF00";
    let err = VimRegex::new(pattern).unwrap_err();
    let renderer = DiagnosticRenderer::new(pattern);
    let output = renderer.render(&err);
    assert!(
        output.contains("suggestion:"),
        "should have suggestion for out-of-range: {output}"
    );
    assert!(
        output.contains("U+10FFFF"),
        "suggestion should mention max: {output}"
    );
}

#[test]
fn render_with_auxiliary_char_code_surrogate_suggestion() {
    let pattern = r"\%uD800";
    let err = VimRegex::new(pattern).unwrap_err();
    let renderer = DiagnosticRenderer::new(pattern);
    let output = renderer.render(&err);
    assert!(
        output.contains("suggestion:"),
        "should have suggestion for surrogate: {output}"
    );
    assert!(
        output.contains("surrogate"),
        "suggestion should mention surrogate: {output}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// MAX_COLLECTION_DEPTH limit
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn collection_depth_1_succeeds() {
    // Simple collection: [abc]
    let result = VimRegex::new("[abc]");
    assert!(result.is_ok());
}

#[test]
fn collection_depth_within_group_succeeds() {
    // Collection inside a group: \([abc]\)
    let result = VimRegex::new(r"\([abc]\)");
    assert!(result.is_ok());
}

#[test]
fn collection_depth_8_at_limit() {
    // A pattern with 8 independent collections should succeed (each starts and
    // finishes, so depth never exceeds 1):
    let result = VimRegex::new("[a][b][c][d][e][f][g][h]");
    assert!(result.is_ok());
}

#[test]
fn collection_depth_exceeded_via_posix_nesting() {
    // POSIX classes like [[:alpha:]] are a form of nesting in our parser.
    // But they're handled specially and don't bump collection_depth.
    // This test just verifies that POSIX classes inside collections work.
    let result = VimRegex::new("[[:alpha:][:digit:]]");
    assert!(result.is_ok());
}

#[test]
fn collection_depth_limit_error_message() {
    // Directly test the error variant for collection depth.
    let err = VimRegexError::from(VimRegexErrorKind::PatternTooComplex {
        span: Some(Span::at(0)),
        detail: "collection nesting too deep (maximum depth is 8)".into(),
    });
    let display = err.to_string();
    assert!(display.starts_with("E339:"), "got: {display}");
    assert!(display.contains("collection nesting"), "got: {display}");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Multi-error recovery infrastructure
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn error_placeholder_node_exists() {
    // Verify the ErrorPlaceholder variant exists and is distinct
    let node = VimPatternNode::ErrorPlaceholder;
    assert_ne!(node, VimPatternNode::AnyChar);
    assert_eq!(node.clone(), VimPatternNode::ErrorPlaceholder);
}

#[test]
fn vim_regex_all_errors_empty_on_success() {
    let regex = VimRegex::new(r"\d\+").unwrap();
    assert!(
        regex.all_errors().is_empty(),
        "successful parse should have no errors"
    );
}

#[test]
fn parse_result_additional_errors_empty_by_default() {
    use crate::parser::parse_pattern;
    let result = parse_pattern(r"\d\+").unwrap();
    assert!(
        result.additional_errors.is_empty(),
        "successful parse should have no additional errors"
    );
}
