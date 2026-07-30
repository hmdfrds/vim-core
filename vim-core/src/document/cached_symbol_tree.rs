//! A cached DocumentSymbol tree from the host's language server.
//!
//! Pushed into the engine via [`VimSession::set_document_symbols`] and
//! registered as a persistent [`SemanticTextObjectProvider`]. The tree is
//! walked synchronously during `processKey` to resolve semantic text objects
//! like `dif` (delete inner function) and `cic` (change inner class).
//!
//! # Byte Offsets
//!
//! All offsets are UTF-8 byte offsets into the document text. The host
//! converts the language server's UTF-16 character offsets to byte offsets
//! before pushing the tree into WASM.
//!
//! # Inner vs Around
//!
//! Each `CachedSymbol` carries two ranges from the LSP `DocumentSymbol`:
//!
//! - **`range`** (`range_start..range_end`): the full extent of the symbol,
//!   including decorators, doc-comments, and the closing delimiter.
//! - **`selection_range`** (`selection_start..selection_end`): the "name"
//!   or declaration portion (e.g., the function signature line).
//!
//! For **around** (`a`), we return the full `range`.
//!
//! For **inner** (`i`), we return the body — from after the selection range
//! end to before the range end. This approximates the function/class body
//! without the declaration line or closing brace, trimmed of leading/trailing
//! whitespace lines. When selection_end == range_end (no body), we fall back
//! to returning the full range (same as around).

use super::{SemanticTextObjectProvider, SemanticTextObjectResult};
use crate::primitives::SemanticObject;

// ─────────────────────────────────────────────────────────────────────────────
// CachedSymbol — one node in the LSP DocumentSymbol tree
// ─────────────────────────────────────────────────────────────────────────────

/// A single node in the cached DocumentSymbol tree.
///
/// Mirrors LSP's `DocumentSymbol` structure with byte offsets and a mapped
/// `SemanticObject` kind. Symbols whose `SymbolKind` does not map to any
/// `SemanticObject` are filtered out during conversion.
pub struct CachedSymbol {
    /// Mapped semantic object kind (Function, Class, TypeDef).
    kind: SemanticObject,
    /// Full symbol range start (byte offset, inclusive).
    range_start: usize,
    /// Full symbol range end (byte offset, exclusive).
    range_end: usize,
    /// Selection (name/declaration) range start (byte offset, inclusive).
    /// Reserved for future use (e.g., go-to-definition within semantic objects).
    #[allow(dead_code)]
    selection_start: usize,
    /// Selection (name/declaration) range end (byte offset, exclusive).
    selection_end: usize,
    /// Nested child symbols.
    children: Vec<Self>,
}

impl CachedSymbol {
    /// Create a new `CachedSymbol`.
    #[must_use]
    pub const fn new(
        kind: SemanticObject,
        range_start: usize,
        range_end: usize,
        selection_start: usize,
        selection_end: usize,
        children: Vec<Self>,
    ) -> Self {
        Self {
            kind,
            range_start,
            range_end,
            selection_start,
            selection_end,
            children,
        }
    }

    /// Whether the cursor (byte offset) falls within this symbol's full range.
    const fn contains(&self, cursor: usize) -> bool {
        cursor >= self.range_start && cursor < self.range_end
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// CachedSymbolTree — the top-level container
// ─────────────────────────────────────────────────────────────────────────────

/// A cached DocumentSymbol tree from the host's language server.
///
/// Implements [`SemanticTextObjectProvider`] by walking the tree to find the
/// innermost symbol of the requested kind that contains the cursor.
pub struct CachedSymbolTree {
    symbols: Vec<CachedSymbol>,
}

impl CachedSymbolTree {
    /// Create a new `CachedSymbolTree` from pre-converted symbols.
    #[must_use]
    pub const fn new(symbols: Vec<CachedSymbol>) -> Self {
        Self { symbols }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SymbolKind → SemanticObject mapping
// ─────────────────────────────────────────────────────────────────────────────

/// Map an LSP `SymbolKind` numeric value to a `SemanticObject`.
///
/// Only maps kinds that have a direct semantic equivalent. Returns `None`
/// for all other kinds, allowing them to fall through to the syntax provider
/// or heuristic fallback.
///
/// SymbolKind values (from the LSP spec):
/// - 5  = Method
/// - 8  = Constructor
/// - 11 = Function
/// - 4  = Class
/// - 9  = Enum
/// - 10 = Interface
/// - 22 = Struct
/// - 25 = TypeParameter
#[must_use]
pub const fn symbol_kind_to_semantic_object(kind: u32) -> Option<SemanticObject> {
    match kind {
        5 | 8 | 11 => Some(SemanticObject::Function), // Method, Constructor, Function
        4 | 9 | 10 | 22 => Some(SemanticObject::Class), // Class, Enum, Interface, Struct
        25 => Some(SemanticObject::TypeDef),          // TypeParameter
        _ => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tree walking — find innermost match
// ─────────────────────────────────────────────────────────────────────────────

/// Walk the symbol tree to find the innermost symbol of `kind` containing `cursor`.
fn find_innermost(
    symbols: &[CachedSymbol],
    kind: SemanticObject,
    cursor: usize,
) -> Option<&CachedSymbol> {
    for sym in symbols {
        if !sym.contains(cursor) {
            continue;
        }
        // Try children first (depth-first -- innermost wins).
        if let Some(result) = find_innermost(&sym.children, kind, cursor) {
            return Some(result);
        }
        // This symbol matches if it's the right kind.
        if sym.kind == kind {
            return Some(sym);
        }
    }
    None
}

/// Collect all symbols of `kind` on the path from root to cursor, innermost last.
fn collect_ancestor_matches<'a>(
    symbols: &'a [CachedSymbol],
    kind: SemanticObject,
    cursor: usize,
    out: &mut Vec<&'a CachedSymbol>,
) {
    for sym in symbols {
        if !sym.contains(cursor) {
            continue;
        }
        // Recurse into children first to maintain innermost-last order
        // after reversing.
        if sym.kind == kind {
            out.push(sym);
        }
        collect_ancestor_matches(&sym.children, kind, cursor, out);
        // Only one child can contain the cursor; stop after finding it.
        return;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Inner range computation
// ─────────────────────────────────────────────────────────────────────────────

/// Compute the "inner" byte range for a symbol.
///
/// The inner range is the body of the symbol — from after the selection range
/// (declaration/signature) to before the symbol range end (closing delimiter).
///
/// For a function like:
/// ```text
/// fn foo() {    ← selection_end points after "fn foo() {"
///     body      ← inner range
/// }             ← range_end points after "}"
/// ```
///
/// The inner range is `selection_end..body_end` where `body_end` is `range_end`
/// minus the closing delimiter (e.g., `}`).
///
/// Heuristic: scans forward from `selection_end` to skip whitespace/newlines
/// at the start of the body, and backward from `range_end` to skip whitespace
/// and the closing delimiter.
fn compute_inner_range(sym: &CachedSymbol, text: &str) -> (usize, usize) {
    let body_start = sym.selection_end;
    let body_end = sym.range_end;

    // Degenerate case: selection covers the entire range (no body).
    if body_start >= body_end {
        return (sym.range_start, sym.range_end);
    }

    // Trim leading whitespace/newlines from body_start.
    let trimmed_start = text[body_start..body_end]
        .find(|c: char| !c.is_whitespace())
        .map_or(body_end, |offset| body_start + offset);

    // Trim trailing whitespace and closing delimiter from body_end.
    // Walk backward from body_end, skipping whitespace, then skip one
    // closing delimiter if found.
    let mut trimmed_end = body_end;
    // Skip trailing whitespace (newlines, spaces, tabs).
    while trimmed_end > trimmed_start {
        let prev = prev_char_boundary(text, trimmed_end);
        let ch = text[prev..].chars().next().unwrap_or(' ');
        if ch.is_whitespace() {
            trimmed_end = prev;
        } else {
            break;
        }
    }
    // Skip one closing delimiter (}, ), ]).
    if trimmed_end > trimmed_start {
        let prev = prev_char_boundary(text, trimmed_end);
        let ch = text[prev..].chars().next().unwrap_or(' ');
        if matches!(ch, '}' | ')' | ']') {
            trimmed_end = prev;
            // Skip whitespace before the closing delimiter too.
            while trimmed_end > trimmed_start {
                let p = prev_char_boundary(text, trimmed_end);
                let c = text[p..].chars().next().unwrap_or(' ');
                if c.is_whitespace() {
                    trimmed_end = p;
                } else {
                    break;
                }
            }
        }
    }

    // Safety: if trimming collapsed the range, return the un-trimmed body.
    if trimmed_start >= trimmed_end {
        return (body_start, body_end);
    }

    (trimmed_start, trimmed_end)
}

/// Find the previous UTF-8 character boundary at or before `pos`.
const fn prev_char_boundary(text: &str, pos: usize) -> usize {
    if pos == 0 {
        return 0;
    }
    let mut p = pos - 1;
    while p > 0 && !text.is_char_boundary(p) {
        p -= 1;
    }
    p
}

// ─────────────────────────────────────────────────────────────────────────────
// SemanticTextObjectProvider implementation
// ─────────────────────────────────────────────────────────────────────────────

impl SemanticTextObjectProvider for CachedSymbolTree {
    fn resolve(
        &self,
        object: SemanticObject,
        text: &str,
        cursor: usize,
        inner: bool,
        count: u32,
    ) -> Option<SemanticTextObjectResult> {
        let count = count.max(1);

        if count > 1 {
            // For count > 1, walk up to the Nth enclosing symbol.
            // Ancestor list is outermost-first; reverse to get innermost-first.
            let mut ancestors = Vec::new();
            collect_ancestor_matches(&self.symbols, object, cursor, &mut ancestors);
            ancestors.reverse();
            let idx = (count as usize).saturating_sub(1);
            let sym = ancestors.get(idx)?;
            return Some(build_result(sym, text, inner));
        }

        // count == 1: find the innermost matching symbol.
        let sym = find_innermost(&self.symbols, object, cursor)?;
        Some(build_result(sym, text, inner))
    }

    fn supported_objects(&self) -> &[SemanticObject] {
        // The cached tree only contains symbols mapped to these kinds.
        &[
            SemanticObject::Function,
            SemanticObject::Class,
            SemanticObject::TypeDef,
        ]
    }
}

/// Build a `SemanticTextObjectResult` from a matched symbol.
fn build_result(sym: &CachedSymbol, text: &str, inner: bool) -> SemanticTextObjectResult {
    if inner {
        let (start, end) = compute_inner_range(sym, text);
        SemanticTextObjectResult {
            start,
            end,
            linewise: false,
            parent: None,
        }
    } else {
        SemanticTextObjectResult {
            start: sym.range_start,
            end: sym.range_end,
            linewise: true,
            parent: None,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_function(
        range_start: usize,
        range_end: usize,
        selection_start: usize,
        selection_end: usize,
        children: Vec<CachedSymbol>,
    ) -> CachedSymbol {
        CachedSymbol::new(
            SemanticObject::Function,
            range_start,
            range_end,
            selection_start,
            selection_end,
            children,
        )
    }

    fn make_class(
        range_start: usize,
        range_end: usize,
        selection_start: usize,
        selection_end: usize,
        children: Vec<CachedSymbol>,
    ) -> CachedSymbol {
        CachedSymbol::new(
            SemanticObject::Class,
            range_start,
            range_end,
            selection_start,
            selection_end,
            children,
        )
    }

    #[test]
    fn test_symbol_kind_mapping() {
        // Function kinds
        assert_eq!(
            symbol_kind_to_semantic_object(11),
            Some(SemanticObject::Function)
        );
        assert_eq!(
            symbol_kind_to_semantic_object(5),
            Some(SemanticObject::Function)
        );
        assert_eq!(
            symbol_kind_to_semantic_object(8),
            Some(SemanticObject::Function)
        );
        // Class kinds
        assert_eq!(
            symbol_kind_to_semantic_object(4),
            Some(SemanticObject::Class)
        );
        assert_eq!(
            symbol_kind_to_semantic_object(22),
            Some(SemanticObject::Class)
        );
        assert_eq!(
            symbol_kind_to_semantic_object(10),
            Some(SemanticObject::Class)
        );
        assert_eq!(
            symbol_kind_to_semantic_object(9),
            Some(SemanticObject::Class)
        );
        // TypeDef
        assert_eq!(
            symbol_kind_to_semantic_object(25),
            Some(SemanticObject::TypeDef)
        );
        // Unknown
        assert_eq!(symbol_kind_to_semantic_object(1), None);
        assert_eq!(symbol_kind_to_semantic_object(99), None);
    }

    #[test]
    fn test_innermost_function() {
        // fn outer() {
        //     fn inner() {
        //         body
        //     }
        // }
        let text = "fn outer() {\n    fn inner() {\n        body\n    }\n}";
        let inner_fn = make_function(
            17,
            48, // range: "fn inner() {\n        body\n    }"
            17,
            30, // selection: "fn inner() {"
            vec![],
        );
        let outer_fn = make_function(
            0,
            49, // range: entire text
            0,
            13, // selection: "fn outer() {"
            vec![inner_fn],
        );
        let tree = CachedSymbolTree::new(vec![outer_fn]);

        // Cursor in inner body → resolves to inner function
        let result = tree.resolve(SemanticObject::Function, text, 35, false, 1);
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.start, 17);
        assert_eq!(r.end, 48);
    }

    #[test]
    fn test_around_returns_full_range() {
        let text = "fn foo() {\n    body\n}";
        let sym = make_function(0, 20, 0, 11, vec![]);
        let tree = CachedSymbolTree::new(vec![sym]);

        let result = tree
            .resolve(SemanticObject::Function, text, 5, false, 1)
            .unwrap();
        assert_eq!(result.start, 0);
        assert_eq!(result.end, 20);
        assert!(result.linewise);
    }

    #[test]
    fn test_inner_trims_body() {
        let text = "fn foo() {\n    body\n}";
        // range: 0..20, selection: 0..11 ("fn foo() {")
        let sym = make_function(0, 20, 0, 11, vec![]);
        let tree = CachedSymbolTree::new(vec![sym]);

        let result = tree
            .resolve(SemanticObject::Function, text, 5, true, 1)
            .unwrap();
        // Inner should be "body" — trimmed of whitespace and closing brace
        assert_eq!(&text[result.start..result.end], "body");
    }

    #[test]
    fn test_count_walks_to_parent() {
        let text = "class Foo {\n    fn bar() {\n        body\n    }\n}";
        let inner_fn = make_function(16, 44, 16, 27, vec![]);
        let outer_class = make_class(0, 46, 0, 12, vec![inner_fn]);
        let tree = CachedSymbolTree::new(vec![outer_class]);

        // count=1 for Function → inner function
        let r1 = tree
            .resolve(SemanticObject::Function, text, 35, false, 1)
            .unwrap();
        assert_eq!(r1.start, 16);

        // count=1 for Class → the class
        let r2 = tree
            .resolve(SemanticObject::Class, text, 35, false, 1)
            .unwrap();
        assert_eq!(r2.start, 0);
    }

    #[test]
    fn test_no_match_returns_none() {
        let text = "let x = 42;";
        let tree = CachedSymbolTree::new(vec![]);

        let result = tree.resolve(SemanticObject::Function, text, 5, false, 1);
        assert!(result.is_none());
    }

    #[test]
    fn test_cursor_outside_symbol_returns_none() {
        let text = "before\nfn foo() { body }\nafter";
        let sym = make_function(7, 24, 7, 17, vec![]);
        let tree = CachedSymbolTree::new(vec![sym]);

        // Cursor on "before" — outside the function
        let result = tree.resolve(SemanticObject::Function, text, 3, false, 1);
        assert!(result.is_none());
    }

    #[test]
    fn test_supported_objects() {
        let tree = CachedSymbolTree::new(vec![]);
        let supported = tree.supported_objects();
        assert!(supported.contains(&SemanticObject::Function));
        assert!(supported.contains(&SemanticObject::Class));
        assert!(supported.contains(&SemanticObject::TypeDef));
        assert!(!supported.contains(&SemanticObject::Parameter));
    }
}
