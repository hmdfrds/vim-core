//! Tests for `MatchContextBuilder` — fluent builder API for `MatchContext`.

use crate::matchers::{MatchContext, MatchContextBuilder, MockLineResolver, MockMarkResolver};

// ═══════════════════════════════════════════════════════════════════════════════
// MATCH CONTEXT BUILDER TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn builder_simple_equivalent() {
    let simple = MatchContext::simple("hello");
    let built = MatchContext::builder("hello").build();
    assert_eq!(simple.text, built.text);
    assert_eq!(simple.cursor, built.cursor);
    assert_eq!(simple.visual_range, built.visual_range);
    assert_eq!(simple.case_sensitive, built.case_sensitive);
}

#[test]
fn builder_with_cursor() {
    let ctx = MatchContext::builder("hello world").cursor(5).build();
    assert_eq!(ctx.cursor, Some(5));
    assert_eq!(ctx.text, "hello world");
}

#[test]
fn builder_with_visual_range() {
    let ctx = MatchContext::builder("hello world")
        .visual_range(2, 7)
        .build();
    assert_eq!(ctx.visual_range, Some((2, 7)));
}

#[test]
fn builder_case_insensitive() {
    let ctx = MatchContext::builder("Hello").case_sensitive(false).build();
    assert!(!ctx.case_sensitive);
}

#[test]
fn builder_last_substitute() {
    let ctx = MatchContext::builder("text").last_substitute("foo").build();
    assert_eq!(ctx.last_substitute, Some("foo"));
}

#[test]
fn with_cursor_convenience() {
    let ctx = MatchContext::with_cursor("hello", 3);
    assert_eq!(ctx.cursor, Some(3));
    assert_eq!(ctx.text, "hello");
    assert!(ctx.case_sensitive);
    assert!(ctx.visual_range.is_none());
}

#[test]
fn builder_with_line_resolver() {
    let resolver = MockLineResolver::new(vec![(0, 1, 1, 1), (5, 2, 1, 1)], 1);
    let ctx = MatchContext::builder("hello\nworld")
        .line_resolver(&resolver)
        .build();
    assert!(ctx.line_resolver.is_some());
}

#[test]
fn builder_with_mark_resolver() {
    let resolver = MockMarkResolver::new(vec![('a', 3)]);
    let ctx = MatchContext::builder("hello")
        .mark_resolver(&resolver)
        .build();
    assert!(ctx.mark_resolver.is_some());
}

#[test]
fn builder_chaining_all_fields() {
    let line_resolver = MockLineResolver::new(vec![(0, 1, 1, 1)], 1);
    let mark_resolver = MockMarkResolver::new(vec![('a', 5)]);
    let ctx = MatchContext::builder("hello world")
        .cursor(3)
        .line_resolver(&line_resolver)
        .mark_resolver(&mark_resolver)
        .visual_range(1, 9)
        .case_sensitive(false)
        .last_substitute("bar")
        .build();

    assert_eq!(ctx.text, "hello world");
    assert_eq!(ctx.cursor, Some(3));
    assert!(ctx.line_resolver.is_some());
    assert!(ctx.mark_resolver.is_some());
    assert_eq!(ctx.visual_range, Some((1, 9)));
    assert!(!ctx.case_sensitive);
    assert_eq!(ctx.last_substitute, Some("bar"));
}

#[test]
fn builder_defaults_match_simple() {
    // Builder with no setters should produce same defaults as simple()
    let built = MatchContext::builder("test").build();
    assert_eq!(built.cursor, None);
    assert_eq!(built.visual_range, None);
    assert!(built.case_sensitive);
    assert!(built.line_resolver.is_none());
    assert!(built.mark_resolver.is_none());
    assert_eq!(built.last_substitute, None);
}

#[test]
fn builder_returns_builder_type() {
    // Verify that builder() returns MatchContextBuilder (type check)
    let _builder: MatchContextBuilder<'_> = MatchContext::builder("text");
}
