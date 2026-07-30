//! Syntax-aware text objects: function, class, argument, comment.
//!
//! Delegates to [`SyntaxProvider`](crate::document::SyntaxProvider) for
//! structural queries. Returns `None` when no provider is available or
//! the cursor is not inside a node of the requested kind.
//!
//! # Inner vs Around
//!
//! - **Around**: uses the full node range from the provider.
//! - **Inner**: trims the first and last lines of the node range,
//!   selecting only the body content. This heuristic works well for
//!   brace-delimited languages (the signature line and closing brace
//!   are excluded).

use super::types::{TextObjectContext, TextObjectRange};
use crate::document::SyntaxNodeKind;
use crate::grammar::types::TextObjectScope;

/// Compute a syntax-aware text object by querying the [`SyntaxProvider`](crate::document::SyntaxProvider).
///
/// # Resolution Order
///
/// 1. **Query-based**: tries `provider.query_textobject(name, ...)`
///    with a name like `"function.inside"` or `"class.around"`. If the provider
///    implements this (e.g., using tree-sitter `textobjects.scm` queries), the
///    returned range is used directly — the query itself distinguishes inner/around.
///
/// 2. **Enum-based** (fallback): calls `provider.enclosing_node(kind)` and applies
///    engine-side inner/around trimming. This is the original behavior for providers
///    that don't implement query-based lookup.
///
/// # Arguments
/// * `ctx` — text object context (text, cursor, providers)
/// * `scope` — inner or around
/// * `kind` — which syntax node kind to look for
///
/// # Returns
/// `Some(TextObjectRange)` if the provider found an enclosing node,
/// `None` if no provider is registered or the cursor isn't inside
/// a node of the requested kind.
#[must_use]
pub fn compute_syntax_object(
    ctx: &TextObjectContext<'_>,
    scope: TextObjectScope,
    kind: SyntaxNodeKind,
) -> Option<TextObjectRange> {
    let provider = ctx.providers.syntax?;

    // ── Try query-based resolution first ──────────────────────────
    if let Some(query_name) = kind.query_name() {
        let scope_suffix = match scope {
            TextObjectScope::Inner => "inside",
            TextObjectScope::Around => "around",
        };
        let name = format!("{query_name}.{scope_suffix}");
        if let Some((start, end)) = provider.query_textobject(&name, ctx.text, ctx.cursor.get()) {
            if start < end && end <= ctx.text.len() {
                return Some(TextObjectRange::char(start, end));
            }
        }
    }

    // ── Fallback: enum-based with engine-side trimming ─────────────
    let (start, end) = provider.enclosing_node(ctx.text, ctx.cursor.get(), kind)?;

    if start >= end || end > ctx.text.len() {
        return None;
    }

    match scope {
        TextObjectScope::Around => Some(TextObjectRange::char(start, end)),
        TextObjectScope::Inner => {
            let inner = compute_inner_range(ctx.text, start, end);
            if inner.0 >= inner.1 {
                // Degenerate inner range — fall back to full range
                Some(TextObjectRange::char(start, end))
            } else {
                Some(TextObjectRange::char(inner.0, inner.1))
            }
        }
    }
}

/// Trim the first and last lines from a range to produce the "inner" content.
///
/// For a function like:
/// ```text
/// fn foo() {    ← first line (trimmed for inner)
///     body      ← inner content
/// }             ← last line (trimmed for inner)
/// ```
///
/// If the range is single-line, returns the full range unchanged.
fn compute_inner_range(text: &str, start: usize, end: usize) -> (usize, usize) {
    let slice = &text[start..end];

    // Find the end of the first line within the slice
    let first_newline = slice.find('\n');
    let Some(nl_offset) = first_newline else {
        // Single line — inner is the same as around
        return (start, end);
    };

    // Inner starts after the first newline
    let inner_start = start + nl_offset + 1;

    // Find the start of the last line within the slice
    let last_newline = slice.rfind('\n');
    let inner_end = match last_newline {
        Some(rn_offset) if start + rn_offset > inner_start => start + rn_offset,
        _ => end,
    };

    (inner_start, inner_end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Providers, SyntaxProvider};

    struct MockSyntaxProvider {
        range: Option<(usize, usize)>,
    }

    impl SyntaxProvider for MockSyntaxProvider {
        fn enclosing_node(
            &self,
            _text: &str,
            _cursor: usize,
            _kind: SyntaxNodeKind,
        ) -> Option<(usize, usize)> {
            self.range
        }

        fn next_node(
            &self,
            _text: &str,
            _cursor: usize,
            _kind: SyntaxNodeKind,
            _count: u32,
        ) -> Option<usize> {
            None
        }

        fn prev_node(
            &self,
            _text: &str,
            _cursor: usize,
            _kind: SyntaxNodeKind,
            _count: u32,
        ) -> Option<usize> {
            None
        }
    }

    fn make_ctx<'a>(
        text: &'a str,
        cursor: usize,
        provider: &'a MockSyntaxProvider,
    ) -> TextObjectContext<'a> {
        let providers = Providers::new().with_syntax(provider);
        TextObjectContext::new(text, cursor).with_providers(providers)
    }

    #[test]
    fn around_returns_full_node_range() {
        let text = "fn foo() {\n    body\n}";
        let provider = MockSyntaxProvider {
            range: Some((0, text.len())),
        };
        let ctx = make_ctx(text, 12, &provider);

        let result = compute_syntax_object(&ctx, TextObjectScope::Around, SyntaxNodeKind::Function);
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.start(), 0);
        assert_eq!(r.end(), text.len());
        assert!(!r.linewise);
    }

    #[test]
    fn inner_trims_first_and_last_lines() {
        let text = "fn foo() {\n    body\n}";
        let provider = MockSyntaxProvider {
            range: Some((0, text.len())),
        };
        let ctx = make_ctx(text, 12, &provider);

        let result = compute_syntax_object(&ctx, TextObjectScope::Inner, SyntaxNodeKind::Function);
        assert!(result.is_some());
        let r = result.unwrap();
        // Should skip "fn foo() {\n" and trim from "\n}"
        assert_eq!(r.start(), 11); // after first \n
        assert_eq!(r.end(), 19); // before last \n
    }

    #[test]
    fn single_line_inner_equals_around() {
        let text = "fn foo() { body }";
        let provider = MockSyntaxProvider {
            range: Some((0, text.len())),
        };
        let ctx = make_ctx(text, 5, &provider);

        let around =
            compute_syntax_object(&ctx, TextObjectScope::Around, SyntaxNodeKind::Function).unwrap();
        let inner =
            compute_syntax_object(&ctx, TextObjectScope::Inner, SyntaxNodeKind::Function).unwrap();
        assert_eq!(around.start(), inner.start());
        assert_eq!(around.end(), inner.end());
    }

    #[test]
    fn no_provider_returns_none() {
        let text = "fn foo() {}";
        let ctx = TextObjectContext::new(text, 5);
        let result = compute_syntax_object(&ctx, TextObjectScope::Around, SyntaxNodeKind::Function);
        assert!(result.is_none());
    }

    #[test]
    fn provider_returns_none_for_no_match() {
        let text = "let x = 5;";
        let provider = MockSyntaxProvider { range: None };
        let ctx = make_ctx(text, 5, &provider);

        let result = compute_syntax_object(&ctx, TextObjectScope::Around, SyntaxNodeKind::Function);
        assert!(result.is_none());
    }

    #[test]
    fn invalid_range_returns_none() {
        let text = "hello";
        let provider = MockSyntaxProvider {
            range: Some((5, 3)),
        };
        let ctx = make_ctx(text, 2, &provider);

        let result = compute_syntax_object(&ctx, TextObjectScope::Around, SyntaxNodeKind::Function);
        assert!(result.is_none());
    }

    #[test]
    fn class_text_object_works() {
        let text = "class Foo {\n    field\n}";
        let provider = MockSyntaxProvider {
            range: Some((0, text.len())),
        };
        let ctx = make_ctx(text, 14, &provider);

        let result = compute_syntax_object(&ctx, TextObjectScope::Around, SyntaxNodeKind::Class);
        assert!(result.is_some());
        assert_eq!(result.unwrap().start(), 0);
    }

    #[test]
    fn argument_text_object_works() {
        let text = "foo(arg1, arg2)";
        let provider = MockSyntaxProvider {
            range: Some((4, 8)),
        };
        let ctx = make_ctx(text, 5, &provider);

        let result = compute_syntax_object(&ctx, TextObjectScope::Around, SyntaxNodeKind::Argument);
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.start(), 4);
        assert_eq!(r.end(), 8);
    }

    #[test]
    fn comment_text_object_works() {
        let text = "// this is a comment\ncode";
        let provider = MockSyntaxProvider {
            range: Some((0, 20)),
        };
        let ctx = make_ctx(text, 5, &provider);

        let result = compute_syntax_object(&ctx, TextObjectScope::Around, SyntaxNodeKind::Comment);
        assert!(result.is_some());
    }

    // ── Query-based resolution ──────────────────────────────────────

    /// A provider that supports query-based text objects.
    struct QuerySyntaxProvider {
        /// Maps query names to ranges.
        queries: std::collections::HashMap<String, (usize, usize)>,
        /// Fallback for enclosing_node.
        enclosing: Option<(usize, usize)>,
    }

    impl QuerySyntaxProvider {
        fn new() -> Self {
            Self {
                queries: std::collections::HashMap::new(),
                enclosing: None,
            }
        }

        fn with_query(mut self, name: &str, start: usize, end: usize) -> Self {
            self.queries.insert(name.to_string(), (start, end));
            self
        }

        fn with_enclosing(mut self, start: usize, end: usize) -> Self {
            self.enclosing = Some((start, end));
            self
        }
    }

    impl SyntaxProvider for QuerySyntaxProvider {
        fn enclosing_node(
            &self,
            _text: &str,
            _cursor: usize,
            _kind: SyntaxNodeKind,
        ) -> Option<(usize, usize)> {
            self.enclosing
        }

        fn next_node(
            &self,
            _text: &str,
            _cursor: usize,
            _kind: SyntaxNodeKind,
            _count: u32,
        ) -> Option<usize> {
            None
        }

        fn prev_node(
            &self,
            _text: &str,
            _cursor: usize,
            _kind: SyntaxNodeKind,
            _count: u32,
        ) -> Option<usize> {
            None
        }

        fn query_textobject(
            &self,
            name: &str,
            _text: &str,
            _cursor: usize,
        ) -> Option<(usize, usize)> {
            self.queries.get(name).copied()
        }
    }

    fn make_query_ctx<'a>(
        text: &'a str,
        cursor: usize,
        provider: &'a QuerySyntaxProvider,
    ) -> TextObjectContext<'a> {
        let providers = Providers::new().with_syntax(provider);
        TextObjectContext::new(text, cursor).with_providers(providers)
    }

    #[test]
    fn query_based_function_inside() {
        let text = "fn foo() {\n    body\n}";
        // Query returns precise inner range (just "    body")
        let provider = QuerySyntaxProvider::new().with_query("function.inside", 11, 19);
        let ctx = make_query_ctx(text, 12, &provider);

        let result = compute_syntax_object(&ctx, TextObjectScope::Inner, SyntaxNodeKind::Function);
        let r = result.unwrap();
        assert_eq!(r.start(), 11);
        assert_eq!(r.end(), 19);
    }

    #[test]
    fn query_based_function_around() {
        let text = "fn foo() {\n    body\n}";
        let provider = QuerySyntaxProvider::new().with_query("function.around", 0, text.len());
        let ctx = make_query_ctx(text, 12, &provider);

        let result = compute_syntax_object(&ctx, TextObjectScope::Around, SyntaxNodeKind::Function);
        let r = result.unwrap();
        assert_eq!(r.start(), 0);
        assert_eq!(r.end(), text.len());
    }

    #[test]
    fn query_based_class_around() {
        let text = "class Foo {\n    field\n}";
        let provider = QuerySyntaxProvider::new().with_query("class.around", 0, text.len());
        let ctx = make_query_ctx(text, 14, &provider);

        let result = compute_syntax_object(&ctx, TextObjectScope::Around, SyntaxNodeKind::Class);
        let r = result.unwrap();
        assert_eq!(r.start(), 0);
        assert_eq!(r.end(), text.len());
    }

    #[test]
    fn query_based_parameter_inside() {
        let text = "fn foo(x: i32, y: &str) {}";
        // Tree-sitter captures just "x: i32" for parameter.inside
        let provider = QuerySyntaxProvider::new().with_query("parameter.inside", 7, 13);
        let ctx = make_query_ctx(text, 9, &provider);

        let result = compute_syntax_object(&ctx, TextObjectScope::Inner, SyntaxNodeKind::Argument);
        let r = result.unwrap();
        assert_eq!(r.start(), 7);
        assert_eq!(r.end(), 13);
        assert_eq!(&text[7..13], "x: i32");
    }

    #[test]
    fn query_falls_back_to_enclosing_node() {
        let text = "fn foo() {\n    body\n}";
        // No query registered, but enclosing_node returns the full range
        let provider = QuerySyntaxProvider::new().with_enclosing(0, text.len());
        let ctx = make_query_ctx(text, 12, &provider);

        // Should fall back to enclosing_node + engine-side trimming
        let result = compute_syntax_object(&ctx, TextObjectScope::Inner, SyntaxNodeKind::Function);
        let r = result.unwrap();
        // Engine-side inner trimming: skip first line, trim last line
        assert_eq!(r.start(), 11); // after "fn foo() {\n"
        assert_eq!(r.end(), 19); // before "\n}"
    }

    #[test]
    fn query_takes_precedence_over_enclosing() {
        let text = "fn foo() {\n    body\n}";
        // Both query and enclosing are available — query should win
        let provider = QuerySyntaxProvider::new()
            .with_query("function.inside", 11, 15) // just "    " (different from engine trimming)
            .with_enclosing(0, text.len());
        let ctx = make_query_ctx(text, 12, &provider);

        let result = compute_syntax_object(&ctx, TextObjectScope::Inner, SyntaxNodeKind::Function);
        let r = result.unwrap();
        // Query result, not engine-trimmed result
        assert_eq!(r.start(), 11);
        assert_eq!(r.end(), 15);
    }

    #[test]
    fn query_with_invalid_range_falls_back() {
        let text = "fn foo() {}";
        // Query returns invalid range (start >= end)
        let provider = QuerySyntaxProvider::new()
            .with_query("function.around", 5, 3)
            .with_enclosing(0, text.len());
        let ctx = make_query_ctx(text, 5, &provider);

        // Should skip invalid query result and use enclosing_node
        let result = compute_syntax_object(&ctx, TextObjectScope::Around, SyntaxNodeKind::Function);
        let r = result.unwrap();
        assert_eq!(r.start(), 0);
        assert_eq!(r.end(), text.len());
    }

    #[test]
    fn custom_kind_skips_query() {
        let text = "hello";
        // Custom kinds don't have query names
        let provider = QuerySyntaxProvider::new().with_enclosing(0, 5);
        let ctx = make_query_ctx(text, 2, &provider);

        let result =
            compute_syntax_object(&ctx, TextObjectScope::Around, SyntaxNodeKind::Custom(42));
        let r = result.unwrap();
        assert_eq!(r.start(), 0);
        assert_eq!(r.end(), 5);
    }

    #[test]
    fn query_name_mapping() {
        assert_eq!(SyntaxNodeKind::Function.query_name(), Some("function"));
        assert_eq!(SyntaxNodeKind::Class.query_name(), Some("class"));
        assert_eq!(SyntaxNodeKind::Argument.query_name(), Some("parameter"));
        assert_eq!(SyntaxNodeKind::Comment.query_name(), Some("comment"));
        assert_eq!(
            SyntaxNodeKind::Conditional.query_name(),
            Some("conditional")
        );
        assert_eq!(SyntaxNodeKind::Loop.query_name(), Some("loop"));
        assert_eq!(SyntaxNodeKind::Block.query_name(), Some("block"));
        assert_eq!(SyntaxNodeKind::Tag.query_name(), Some("tag"));
        assert_eq!(SyntaxNodeKind::Custom(0).query_name(), None);
    }
}
