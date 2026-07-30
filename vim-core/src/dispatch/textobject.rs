//! Text object dispatcher.
//!
//! Maps `Grammar::TextObject` to `commands::textobjects` implementations.
//! This is the ONLY place to update when adding text objects.
//!
//! # Design
//!
//! Grammar layer has flat `TextObject` struct for parsing.
//! Commands layer has organized implementations (word.rs, brackets.rs, etc).
//! This dispatcher bridges them via exhaustive match.
//!
//! # Adding New Text Objects
//!
//! 1. Add variant to `grammar::TextObjectKind` enum
//! 2. Create implementation in `commands/textobjects/`
//! 3. Add match arm HERE in `dispatch_textobject()`
//!
//! # Consistency
//!
//! All dispatchers follow the same pattern:
//! - `dispatch_motion(motion, &MotionContext)`
//! - `dispatch_operator(op, &OperatorContext)`
//! - `dispatch_textobject(object, &TextObjectContext)`

use crate::commands::helpers::{next_char_boundary, prev_char_boundary};
use crate::commands::textobjects::{
    aggregate, argument, brackets, entire, indent, paragraph, quotes, sentence, subword, symbol,
    syntax, tag, word,
};
pub use crate::commands::textobjects::{BracketType, TextObjectContext, TextObjectRange};
#[cfg(test)]
use crate::grammar::types::TextObjectScope;
use crate::grammar::types::{TextObject, TextObjectKind};
use crate::primitives::SemanticObject;
use crate::primitives::{Offset, WordKind};

/// Dispatch a `Grammar::TextObject` to the appropriate implementation.
///
/// This is the **exhaustive match** for all text objects.
/// Adding a new `TextObjectKind` variant will cause a compile error here.
///
/// # Arguments
/// * `object` - The text object from Grammar
/// * `ctx` - Text object context with text and cursor
///
/// # Returns
/// * `Some(TextObjectRange)` if the text object was found, `None` otherwise
#[inline]
#[must_use]
pub fn dispatch_textobject(
    object: TextObject,
    ctx: &TextObjectContext<'_>,
) -> Option<TextObjectRange> {
    // targets.vim: if a seek modifier is present, reposition cursor before
    // dispatching the normal text object.
    if let Some(seek) = object.seek {
        return dispatch_textobject_with_seek(object, ctx, seek);
    }

    let scope = object.scope;

    match object.kind {
        TextObjectKind::Word => word::compute_word_object(ctx, scope, WordKind::Word),
        TextObjectKind::WORD => word::compute_word_object(ctx, scope, WordKind::WORD),
        TextObjectKind::Sentence => sentence::compute_sentence_object(ctx, scope),
        TextObjectKind::Paragraph => paragraph::compute_paragraph_object(ctx, scope),
        TextObjectKind::Paren => brackets::compute_bracket_object(ctx, scope, BracketType::Paren),
        TextObjectKind::Brace => brackets::compute_bracket_object(ctx, scope, BracketType::Brace),
        TextObjectKind::Bracket => {
            brackets::compute_bracket_object(ctx, scope, BracketType::Bracket)
        }
        TextObjectKind::Angle => brackets::compute_bracket_object(ctx, scope, BracketType::Angle),
        TextObjectKind::DoubleQuote => quotes::compute_quote_object(ctx, scope, '"'),
        TextObjectKind::SingleQuote => quotes::compute_quote_object(ctx, scope, '\''),
        TextObjectKind::Backtick => quotes::compute_quote_object(ctx, scope, '`'),
        TextObjectKind::Pipe => quotes::compute_quote_object(ctx, scope, '|'),
        TextObjectKind::Tag => {
            syntax::compute_syntax_object(ctx, scope, crate::document::SyntaxNodeKind::Tag)
                .or_else(|| tag::compute_tag_object(ctx, scope))
        }
        TextObjectKind::Function => {
            syntax::compute_syntax_object(ctx, scope, crate::document::SyntaxNodeKind::Function)
        }
        TextObjectKind::Class => {
            syntax::compute_syntax_object(ctx, scope, crate::document::SyntaxNodeKind::Class)
        }
        TextObjectKind::Argument => {
            syntax::compute_syntax_object(ctx, scope, crate::document::SyntaxNodeKind::Argument)
                .or_else(|| argument::compute_argument_object(ctx, scope))
        }
        TextObjectKind::Comment => {
            syntax::compute_syntax_object(ctx, scope, crate::document::SyntaxNodeKind::Comment)
        }
        TextObjectKind::Custom(id) => {
            if let Some(provider) = ctx.providers.custom_textobjects {
                let inner = scope.is_inner();
                provider
                    .compute_textobject_with_info(id, ctx.text, ctx.cursor.get(), inner)
                    .map(|result| {
                        let doc_len = ctx.text.len();
                        let s = result.start.min(doc_len);
                        let e = result.end.min(doc_len);
                        let range = if s <= e {
                            crate::primitives::Range::from_raw(s, e)
                        } else {
                            crate::primitives::Range::from_raw(e, s)
                        };
                        TextObjectRange {
                            range,
                            linewise: result.linewise,
                        }
                    })
            } else {
                None
            }
        }
        TextObjectKind::Entire => entire::compute_entire_object(ctx, scope),
        TextObjectKind::IndentBlock => indent::compute_indent_object(ctx, scope),
        TextObjectKind::IndentBlockNoBelow => indent::compute_indent_object_no_below(ctx, scope),
        TextObjectKind::AnyBracket => aggregate::compute_any_bracket_object(ctx, scope),
        TextObjectKind::AnyQuote => aggregate::compute_any_quote_object(ctx, scope),
        TextObjectKind::Symbol => symbol::compute_symbol_object(ctx, scope),
        TextObjectKind::Subword => subword::compute_subword_object(ctx, scope),
        TextObjectKind::Semantic(obj) => dispatch_semantic_textobject(obj, object, ctx),
    }
}

/// Dispatch a text object with seek modifier (targets.vim `n`/`l`).
///
/// Searches forward (`Next`) or backward (`Last`) from the cursor for the
/// delimiter character, then dispatches the normal text object from that
/// position.
///
/// # Seeking strategy
///
/// **Brackets** (`()`, `{}`, `[]`, `<>`):
/// - `Next`: search forward for the opening bracket. Dispatch from there.
/// - `Last`: search backward for the closing bracket. Dispatch from there.
///
/// **Quotes** (`"`, `'`, `` ` ``):
/// - `Next`: search forward for the quote character. Dispatch from there.
/// - `Last`: search backward for the quote character. Dispatch from there.
///
/// **Tags** (`t`):
/// - `Next`: search forward for `<`. Dispatch from there.
/// - `Last`: search backward for `>`. Dispatch from there.
///
/// **AnyBracket/AnyQuote** (`b`/`q`):
/// - `Next`: search forward for any opening bracket / any quote.
/// - `Last`: search backward for any closing bracket / any quote.
///
/// Returns `None` if no delimiter is found in the seek direction.
fn dispatch_textobject_with_seek(
    object: TextObject,
    ctx: &TextObjectContext<'_>,
    seek: crate::grammar::types::SeekDirection,
) -> Option<TextObjectRange> {
    use crate::grammar::types::SeekDirection;

    let text = ctx.text;
    let cursor = ctx.cursor.get();

    // Build the base text object (without seek) for re-dispatch.
    let base_object = TextObject {
        scope: object.scope,
        kind: object.kind,
        seek: None,
    };

    // Determine the character(s) to search for based on the text object kind
    // and seek direction.
    let seek_chars = seek_chars_for_kind(object.kind, seek);

    // First, determine the range of the current text object at cursor (if any).
    // We need this to skip past the current text object when seeking.
    let current_range = {
        let current_ctx = TextObjectContext::new(text, cursor).with_providers(ctx.providers);
        dispatch_textobject(base_object, &current_ctx)
    };

    // Search for the delimiter in the appropriate direction, skipping past
    // any occurrence that resolves to the same text object we're already in.
    match seek {
        SeekDirection::Next => {
            let mut search_from = next_char_boundary(text, cursor);
            // Safety limit to prevent infinite loops on pathological input.
            let mut attempts = 0;
            while attempts < 100 {
                let found = find_char_forward(text, search_from, &seek_chars)?;
                let seek_ctx = TextObjectContext::new(text, found).with_providers(ctx.providers);
                if let Some(result) = dispatch_textobject(base_object, &seek_ctx) {
                    // Check that this is a DIFFERENT text object from the current one.
                    if current_range
                        .as_ref()
                        .is_none_or(|cur| cur.range != result.range)
                    {
                        return Some(result);
                    }
                }
                // This delimiter resolved to the same text object or none — skip past it.
                search_from = next_char_boundary(text, found);
                attempts += 1;
            }
            None
        }
        SeekDirection::Last => {
            let mut search_until = cursor;
            let mut attempts = 0;
            while attempts < 100 {
                let found = find_char_backward(text, search_until, &seek_chars)?;
                let seek_ctx = TextObjectContext::new(text, found).with_providers(ctx.providers);
                if let Some(result) = dispatch_textobject(base_object, &seek_ctx) {
                    if current_range
                        .as_ref()
                        .is_none_or(|cur| cur.range != result.range)
                    {
                        return Some(result);
                    }
                }
                // Skip past this delimiter backward.
                if found == 0 {
                    return None;
                }
                search_until = found;
                attempts += 1;
            }
            None
        }
    }
}

/// Return the delimiter characters to seek for a given text object kind and direction.
///
/// For brackets: `Next` seeks the opening delimiter, `Last` seeks the closing one.
/// For quotes: both directions seek the quote character itself.
/// For tags: `Next` seeks `<`, `Last` seeks `>`.
const fn seek_chars_for_kind(
    kind: TextObjectKind,
    seek: crate::grammar::types::SeekDirection,
) -> SeekChars {
    use crate::grammar::types::SeekDirection;

    match kind {
        TextObjectKind::Paren => match seek {
            SeekDirection::Next => SeekChars::One('('),
            SeekDirection::Last => SeekChars::One(')'),
        },
        TextObjectKind::Brace => match seek {
            SeekDirection::Next => SeekChars::One('{'),
            SeekDirection::Last => SeekChars::One('}'),
        },
        TextObjectKind::Bracket => match seek {
            SeekDirection::Next => SeekChars::One('['),
            SeekDirection::Last => SeekChars::One(']'),
        },
        TextObjectKind::Angle => match seek {
            SeekDirection::Next => SeekChars::One('<'),
            SeekDirection::Last => SeekChars::One('>'),
        },
        TextObjectKind::DoubleQuote => SeekChars::One('"'),
        TextObjectKind::SingleQuote => SeekChars::One('\''),
        TextObjectKind::Backtick => SeekChars::One('`'),
        TextObjectKind::Pipe => SeekChars::One('|'),
        TextObjectKind::Tag => match seek {
            SeekDirection::Next => SeekChars::One('<'),
            SeekDirection::Last => SeekChars::One('>'),
        },
        TextObjectKind::AnyBracket => match seek {
            SeekDirection::Next => SeekChars::Many(&['(', '{', '[', '<']),
            SeekDirection::Last => SeekChars::Many(&[')', '}', ']', '>']),
        },
        TextObjectKind::AnyQuote => SeekChars::Many(&['"', '\'', '`']),
        // Non-seekable kinds should not reach here (grammar rejects them),
        // but return empty as a safety net.
        _ => SeekChars::Many(&[]),
    }
}

/// Characters to seek for a text object.
enum SeekChars {
    /// Single character to find.
    One(char),
    /// Any of these characters.
    Many(&'static [char]),
}

impl SeekChars {
    fn matches(&self, c: char) -> bool {
        match self {
            Self::One(target) => c == *target,
            Self::Many(targets) => targets.contains(&c),
        }
    }
}

/// Search forward from `start` (inclusive) for any of the given characters.
/// Returns the byte offset of the first match, or `None`.
fn find_char_forward(text: &str, start: usize, chars: &SeekChars) -> Option<usize> {
    for (offset, c) in text[start..].char_indices() {
        if chars.matches(c) {
            return Some(start + offset);
        }
    }
    None
}

/// Search backward from `end` (exclusive) for any of the given characters.
/// Returns the byte offset of the first match, or `None`.
fn find_char_backward(text: &str, end: usize, chars: &SeekChars) -> Option<usize> {
    // Iterate characters backward using byte indices.
    let slice = &text[..end];
    for (offset, c) in slice.char_indices().rev() {
        if chars.matches(c) {
            return Some(offset);
        }
    }
    None
}

/// Dispatch a semantic text object, trying the semantic provider first, then
/// the syntax provider, and finally a pure-text heuristic fallback.
///
/// # Fallback chain (in order)
///
/// 1. `SemanticTextObjectProvider` — rich LSP / tree-sitter result.
/// 2. `SyntaxProvider` — structural syntax result (existing path).
/// 3. Pure-text heuristic — works with no provider at all:
///
/// | [`SemanticObject`]       | Heuristic fallback                    |
/// |--------------------------|---------------------------------------|
/// | `Scope`                  | indent block object                   |
/// | all others               | `None` (requires a provider)          |
fn dispatch_semantic_textobject(
    obj: SemanticObject,
    object: TextObject,
    ctx: &TextObjectContext<'_>,
) -> Option<TextObjectRange> {
    let scope = object.scope;
    let inner = scope.is_inner();

    // 1. Try the semantic provider first.
    if let Some(provider) = ctx.providers.semantic_textobjects {
        if let Some(result) = provider.resolve(obj, ctx.text, ctx.cursor.get(), inner, 1) {
            let doc_len = ctx.text.len();
            let s = result.start.min(doc_len);
            let e = result.end.min(doc_len);
            let range = if s <= e {
                crate::primitives::Range::from_raw(s, e)
            } else {
                crate::primitives::Range::from_raw(e, s)
            };
            return Some(TextObjectRange {
                range,
                linewise: result.linewise,
            });
        }
    }

    // 2. Try the syntax provider (maps SemanticObject to the closest SyntaxNodeKind).
    let syntax_kind = semantic_object_to_syntax_kind(obj);
    if let Some(kind) = syntax_kind {
        let result = syntax::compute_syntax_object(ctx, scope, kind);
        if result.is_some() {
            return result;
        }
    }

    // 3. Pure-text heuristic fallback (no provider required).
    //
    // Only `Scope` has a safe heuristic (indent blocks); all others
    // require structural knowledge and return `None` to avoid incorrect
    // selections (e.g. `dif` should not silently delete a brace pair
    // when no syntax/semantic provider is configured — see the
    // `syntax_provider_no_provider_returns_no_delete` test).
    match obj {
        // Scope → indent-block approximation.
        SemanticObject::Scope => indent::compute_indent_object(ctx, scope),
        // Parameter → pure-text argument fallback.
        SemanticObject::Parameter => argument::compute_argument_object(ctx, scope),
        // All other objects — no reliable heuristic without a parser.
        SemanticObject::Function
        | SemanticObject::Class
        | SemanticObject::Conditional
        | SemanticObject::Loop
        | SemanticObject::Comment
        | SemanticObject::Call
        | SemanticObject::TypeDef
        | SemanticObject::Return
        | SemanticObject::Import
        | SemanticObject::StringLiteral => None,
    }
}

/// Map a [`SemanticObject`] to the nearest [`crate::document::SyntaxNodeKind`].
///
/// Returns `None` for objects that have no direct structural equivalent
/// in the `SyntaxNodeKind` vocabulary.
const fn semantic_object_to_syntax_kind(
    obj: SemanticObject,
) -> Option<crate::document::SyntaxNodeKind> {
    use crate::document::SyntaxNodeKind;
    match obj {
        SemanticObject::Function => Some(SyntaxNodeKind::Function),
        SemanticObject::Class => Some(SyntaxNodeKind::Class),
        SemanticObject::Parameter => Some(SyntaxNodeKind::Argument),
        SemanticObject::Comment => Some(SyntaxNodeKind::Comment),
        SemanticObject::Conditional => Some(SyntaxNodeKind::Conditional),
        SemanticObject::Loop => Some(SyntaxNodeKind::Loop),
        SemanticObject::Scope => Some(SyntaxNodeKind::Block),
        SemanticObject::Call
        | SemanticObject::TypeDef
        | SemanticObject::Return
        | SemanticObject::Import
        | SemanticObject::StringLiteral => None,
    }
}

/// Dispatch a text object in visual mode, producing the full `CommandResult`.
///
/// Owns the complete visual text object lifecycle:
/// 1. **Initial dispatch** with overlap retry (re-dispatches from head+1 if
///    the text object overlaps the existing selection)
/// 2. **Selection update** — compute new anchor/cursor from text object range
/// 3. **Extension** — if selection already covers the text object, extend to
///    the next one by re-dispatching
/// 4. **Effect building** — produces mode/selection/cursor effects via commands layer
///
/// Returns `CommandResult::none()` if no text object is found.
pub fn dispatch_visual_textobject(
    textobject: TextObject,
    text: &str,
    cursor: usize,
    selection: Option<&crate::primitives::SelectionRange>,
    providers: &crate::document::Providers<'_>,
    count: u32,
    current_mode_linewise: bool,
) -> crate::commands::CommandResult {
    use crate::commands::visual::textobject as vt;

    // 1. Dispatch with count expansion, then overlap retry
    let text_ctx = TextObjectContext::new(text, cursor).with_providers(*providers);
    let base_range = if count > 1 {
        dispatch_textobject_with_count(textobject, &text_ctx, count)
    } else {
        None
    };
    let Some(text_obj_range) = base_range
        .or_else(|| resolve_with_overlap_retry(text, textobject, cursor, selection, providers))
    else {
        return crate::commands::CommandResult::none();
    };

    let range_start = text_obj_range.range.start().get();
    let range_end = text_obj_range.range.end().get();

    // 2. Compute selection update (pure — delegated to commands)
    let mut result = vt::compute_selection_update(
        text,
        range_start,
        range_end,
        text_obj_range.linewise,
        selection,
        current_mode_linewise,
    );

    // 3. If selection already covers this text object, extend to next
    if result.needs_extend {
        if let Some(sel) = selection {
            let anchor = sel.anchor().get();
            let head = sel.head().get();
            // For inner bracket text objects, range_end points AT the closing
            // bracket, so dispatching from there finds the same pair.  Advance
            // one character past range_end (and past head) to escape the
            // current text object and find the enclosing/next one.
            // For sequential text objects (words, paragraphs, sentences),
            // dispatch from range_end directly — the next text object starts
            // right at the boundary.  For nesting text objects (brackets,
            // quotes, tags), advance one char past to escape the delimiters.
            let is_sequential = matches!(
                textobject.kind,
                TextObjectKind::Word
                    | TextObjectKind::WORD
                    | TextObjectKind::Sentence
                    | TextObjectKind::Paragraph
            );
            let next_cursor = if sel.is_forward() {
                let past = if is_sequential {
                    range_end
                } else {
                    next_char_boundary(text, range_end)
                };
                past.max(next_char_boundary(text, head))
            } else {
                let past = if is_sequential && range_start > 0 {
                    range_start
                } else if range_start > 0 {
                    range_start - 1
                } else {
                    0
                };
                past.min(prev_char_boundary(text, head))
            };

            if next_cursor < text.len() {
                let next_ctx = TextObjectContext::new(text, next_cursor).with_providers(*providers);
                if let Some(next_range) = dispatch_textobject(textobject, &next_ctx) {
                    let (new_anchor, mut new_cursor) = vt::merge_extended_range(
                        anchor,
                        head,
                        next_range.range.start().get(),
                        next_range.range.end().get(),
                    );
                    // merge_extended_range returns the exclusive range end as
                    // cursor; convert to the inclusive position (last char).
                    if head >= anchor && new_cursor > new_anchor {
                        new_cursor = prev_char_boundary(text, new_cursor);
                    }
                    result.anchor = Offset::new(new_anchor);
                    result.cursor = Offset::new(new_cursor);
                    result.needs_extend = false;
                }
            }
        }
    }

    // 4. Build effects (delegated to commands layer)
    vt::build_textobject_effects(&result)
}

/// Resolve text object, retrying from head+1 if it overlaps existing selection.
///
/// On first dispatch, if the text object range contains the selection head
/// (meaning we're already "inside" the object), re-dispatch from head+1
/// to find the next occurrence.
fn resolve_with_overlap_retry(
    text: &str,
    textobject: TextObject,
    cursor: usize,
    selection: Option<&crate::primitives::SelectionRange>,
    providers: &crate::document::Providers<'_>,
) -> Option<TextObjectRange> {
    let text_ctx = TextObjectContext::new(text, cursor).with_providers(*providers);
    let text_obj_range = dispatch_textobject(textobject, &text_ctx)?;

    if let Some(sel) = selection {
        let anchor = sel.anchor().get();
        let head = sel.head().get();
        if anchor != head
            && text_obj_range.range.start().get() <= head
            && head < text_obj_range.range.end().get()
        {
            // Dispatch from one past the forward-most selected position,
            // not from range_end.  Using range_end skips over adjacent text
            // objects and causes `vawaw` to jump too far.
            let sel_end = anchor.max(head);
            let next_cursor = next_char_boundary(text, sel_end);
            if next_cursor < text.len() {
                let next_ctx = TextObjectContext::new(text, next_cursor).with_providers(*providers);
                return Some(dispatch_textobject(textobject, &next_ctx).unwrap_or(text_obj_range));
            }
        }
    }
    Some(text_obj_range)
}

/// Dispatch a text object with count expansion.
///
/// Handles the two expansion strategies:
/// - **Nesting** (brackets, quotes, tags): Step back from the opening bracket
///   and re-dispatch to find the parent pair. `inner` text objects step back 2
///   (past bracket + first char), `outer` step back 1.
/// - **Sequential** (words, sentences, paragraphs): Extend forward from the
///   end of the current range to find the next occurrence.
///
/// Returns `None` if the base text object is not found.
#[must_use]
pub fn dispatch_textobject_with_count(
    object: TextObject,
    ctx: &TextObjectContext<'_>,
    count: u32,
) -> Option<TextObjectRange> {
    let mut range = dispatch_textobject(object, ctx)?;

    if count <= 1 {
        return Some(range);
    }

    // Entire-buffer text objects are not expandable — count is meaningless.
    if matches!(object.kind, TextObjectKind::Entire) {
        return Some(range);
    }

    let uses_nesting = matches!(
        object.kind,
        TextObjectKind::Paren
            | TextObjectKind::Brace
            | TextObjectKind::Bracket
            | TextObjectKind::Angle
            | TextObjectKind::DoubleQuote
            | TextObjectKind::SingleQuote
            | TextObjectKind::Backtick
            | TextObjectKind::Pipe
            | TextObjectKind::Tag
            | TextObjectKind::AnyBracket
            | TextObjectKind::AnyQuote
    );

    if uses_nesting {
        for i in 1..count {
            // Step back from the range start to find the parent pair.
            // Use char-boundary-safe helpers to avoid landing mid-UTF-8.
            let start = range.range.start().get();
            let one_back = prev_char_boundary(ctx.text, start);
            let outer_cursor = if object.scope.is_inner() {
                prev_char_boundary(ctx.text, one_back)
            } else {
                one_back
            };
            if outer_cursor == start {
                // Can't step further back. For inner quotes/brackets with
                // count >= 2, Vim promotes inner to around (includes delimiters).
                if i == 1 && object.scope.is_inner() {
                    let around = TextObject {
                        scope: crate::grammar::types::TextObjectScope::Around,
                        kind: object.kind,
                        seek: None,
                    };
                    let around_ctx = TextObjectContext::new(ctx.text, ctx.cursor.get())
                        .with_providers(ctx.providers);
                    if let Some(around_range) = dispatch_textobject(around, &around_ctx) {
                        range = around_range;
                    }
                }
                break;
            }
            let outer_ctx =
                TextObjectContext::new(ctx.text, outer_cursor).with_providers(ctx.providers);
            if let Some(outer_range) = dispatch_textobject(object, &outer_ctx) {
                if outer_range.range == range.range {
                    // Same range — can't expand further. Promote inner to around.
                    if object.scope.is_inner() {
                        let around = TextObject {
                            scope: crate::grammar::types::TextObjectScope::Around,
                            kind: object.kind,
                            seek: None,
                        };
                        let around_ctx = TextObjectContext::new(ctx.text, ctx.cursor.get())
                            .with_providers(ctx.providers);
                        if let Some(around_range) = dispatch_textobject(around, &around_ctx) {
                            range = around_range;
                        }
                    }
                    break;
                }
                range = outer_range;
            } else {
                break;
            }
        }
    } else {
        // Vim's inner-sentence count: `Nis` = (N-1) around-sentences + 1 inner.
        // The first N-1 sentences are selected with trailing whitespace (around),
        // and the last sentence is selected without trailing whitespace (inner).
        let is_inner_sentence =
            object.scope.is_inner() && matches!(object.kind, TextObjectKind::Sentence);

        if is_inner_sentence {
            // Replace initial `is` with `as` for the first sentence.
            let around = TextObject {
                scope: crate::grammar::types::TextObjectScope::Around,
                kind: object.kind,
                seek: None,
            };
            let around_ctx =
                TextObjectContext::new(ctx.text, ctx.cursor.get()).with_providers(ctx.providers);
            if let Some(ar) = dispatch_textobject(around, &around_ctx) {
                range = ar;
            }
            // Extend with additional `as` for counts 2..N-1.
            for _ in 2..count {
                let raw = range.range.end().get();
                let nc = if raw >= ctx.text.len() && !ctx.text.is_empty() {
                    crate::primitives::text_util::prev_char_boundary(ctx.text, ctx.text.len())
                } else {
                    raw
                };
                let nctx = TextObjectContext::new(ctx.text, nc).with_providers(ctx.providers);
                if let Some(nr) = dispatch_textobject(around, &nctx) {
                    if nr.range.end() <= range.range.end() {
                        break;
                    }
                    range.range = range.range.with_end(nr.range.end());
                    range.linewise = range.linewise || nr.linewise;
                } else {
                    break;
                }
            }
            // For counts > 2, the last `as` may have included trailing whitespace
            // that should be trimmed (to match the `is` behavior on the last sentence).
            if count > 2 {
                let end = range.range.end().get();
                let mut tr = end;
                while tr > range.range.start().get() {
                    let p = prev_char_boundary(ctx.text, tr);
                    match ctx.text[p..].chars().next() {
                        Some(c) if c == ' ' || c == '\t' => tr = p,
                        _ => break,
                    }
                }
                if tr < end {
                    range.range = range.range.with_end(Offset::new(tr));
                }
            }
        } else {
            // Sequential expansion for words, paragraphs, etc.
            // If any step fails to advance, the entire counted operation fails
            // (Vim beeps and does nothing when count exceeds available objects).
            let mut expanded = 0u32;
            for _ in 1..count {
                let next_cursor = {
                    let raw = range.range.end().get();
                    if raw >= ctx.text.len() && !ctx.text.is_empty() {
                        crate::primitives::text_util::prev_char_boundary(ctx.text, ctx.text.len())
                    } else {
                        raw
                    }
                };
                let next_ctx =
                    TextObjectContext::new(ctx.text, next_cursor).with_providers(ctx.providers);
                if let Some(next_range) = dispatch_textobject(object, &next_ctx) {
                    if next_range.range.end() <= range.range.end() {
                        break;
                    }
                    range.range = range.range.with_end(next_range.range.end());
                    range.linewise = range.linewise || next_range.linewise;
                    expanded += 1;
                } else {
                    break;
                }
            }
            // If we couldn't expand enough, cancel the operation.
            if expanded < count - 1 {
                return None;
            }
        }
    }

    Some(range)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dispatch_inner_word() {
        let text = "hello world";
        let ctx = TextObjectContext::new(text, 0);
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Word,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx);
        assert!(result.is_some());
        let range = result.unwrap();
        assert_eq!(range.range.start().get(), 0);
        assert_eq!(range.range.end().get(), 5); // "hello"
    }

    #[test]
    fn test_dispatch_around_paren() {
        let text = "foo(bar)";
        let ctx = TextObjectContext::new(text, 4); // cursor on 'b'
        let object = TextObject {
            scope: TextObjectScope::Around,
            kind: TextObjectKind::Paren,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx);
        assert!(result.is_some());
        let range = result.unwrap();
        assert_eq!(range.range.start().get(), 3); // '('
        assert_eq!(range.range.end().get(), 8); // after ')'
    }

    // ── dispatch_textobject_with_count ──────────────────────────────────

    #[test]
    fn test_with_count_1_same_as_base() {
        let text = "hello world";
        let ctx = TextObjectContext::new(text, 0);
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Word,
            seek: None,
        };

        let base = dispatch_textobject(object, &ctx).unwrap();
        let counted = dispatch_textobject_with_count(object, &ctx, 1).unwrap();

        assert_eq!(base.range.start(), counted.range.start());
        assert_eq!(base.range.end(), counted.range.end());
    }

    #[test]
    fn test_with_count_sequential_word() {
        // "hello world bar" — count=2 from cursor 0 should span "hello world"
        let text = "hello world bar";
        let ctx = TextObjectContext::new(text, 0);
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Word,
            seek: None,
        };

        let result = dispatch_textobject_with_count(object, &ctx, 2).unwrap();
        assert_eq!(result.range.start().get(), 0); // starts at 'h'
        assert!(result.range.end().get() > 5); // extends past "hello"
    }

    #[test]
    fn test_with_count_nested_paren() {
        // ((inner)) — count=2 from inside should find outer pair
        let text = "((inner))";
        let ctx = TextObjectContext::new(text, 3); // inside 'n'
        let object = TextObject {
            scope: TextObjectScope::Around,
            kind: TextObjectKind::Paren,
            seek: None,
        };

        let count1 = dispatch_textobject_with_count(object, &ctx, 1).unwrap();
        let count2 = dispatch_textobject_with_count(object, &ctx, 2).unwrap();

        // count=2 should find a wider range than count=1
        assert!(count2.range.start() <= count1.range.start());
        assert!(count2.range.end() >= count1.range.end());
    }

    #[test]
    fn test_with_count_returns_none_on_no_match() {
        let text = "hello";
        let ctx = TextObjectContext::new(text, 0);
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Paren,
            seek: None,
        };

        assert!(dispatch_textobject_with_count(object, &ctx, 1).is_none());
    }

    // ── Entire (ie/ae) dispatch ───────────────────────────────────────

    #[test]
    fn test_dispatch_ae_selects_entire_buffer() {
        let text = "\n\nhello\nworld\n\n";
        let ctx = TextObjectContext::new(text, 5);
        let object = TextObject {
            scope: TextObjectScope::Around,
            kind: TextObjectKind::Entire,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx).unwrap();
        assert_eq!(result.start(), 0);
        assert_eq!(result.end(), text.len());
        assert!(result.linewise);
    }

    #[test]
    fn test_dispatch_ie_trims_blank_lines() {
        let text = "\n\nhello\nworld\n\n";
        let ctx = TextObjectContext::new(text, 5);
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Entire,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx).unwrap();
        assert_eq!(result.start(), 2); // skips leading blank lines
        assert!(result.end() < text.len()); // trims trailing blank lines
        assert!(result.linewise);
    }

    #[test]
    fn test_dispatch_entire_empty() {
        let ctx = TextObjectContext::new("", 0);
        let object = TextObject {
            scope: TextObjectScope::Around,
            kind: TextObjectKind::Entire,
            seek: None,
        };
        assert!(dispatch_textobject(object, &ctx).is_none());
    }

    #[test]
    fn test_dispatch_entire_count_ignored() {
        let text = "hello\nworld";
        let ctx = TextObjectContext::new(text, 0);
        let object = TextObject {
            scope: TextObjectScope::Around,
            kind: TextObjectKind::Entire,
            seek: None,
        };
        // Count > 1 on Entire should still return the whole buffer.
        let result = dispatch_textobject_with_count(object, &ctx, 5).unwrap();
        assert_eq!(result.start(), 0);
        assert_eq!(result.end(), text.len());
    }

    // ── IndentBlock (ii/ai) dispatch ──────────────────────────────────

    #[test]
    fn test_dispatch_ii_indented_block() {
        let text = "top\n    a\n    b\nbottom";
        let ctx = TextObjectContext::new(text, 4); // cursor on "    a"
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::IndentBlock,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx).unwrap();
        assert!(result.linewise);
        assert_eq!(result.start(), 4);
    }

    #[test]
    fn test_dispatch_ai_includes_surrounding() {
        let text = "top\n    a\n    b\nbottom";
        let ctx = TextObjectContext::new(text, 4); // cursor on "    a"
        let object = TextObject {
            scope: TextObjectScope::Around,
            kind: TextObjectKind::IndentBlock,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx).unwrap();
        assert!(result.linewise);
        assert_eq!(result.start(), 0); // includes "top" line
        assert_eq!(result.end(), text.len()); // includes "bottom" line
    }

    #[test]
    fn test_dispatch_indent_empty() {
        let ctx = TextObjectContext::new("", 0);
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::IndentBlock,
            seek: None,
        };
        assert!(dispatch_textobject(object, &ctx).is_none());
    }

    // ── IndentBlockNoBelow (iI/aI) dispatch ─────────────────────────────

    #[test]
    fn test_dispatch_aI_excludes_below() {
        // "header\n    a\n    b\nfooter" — aI from "    a" includes header but NOT footer
        let text = "header\n    a\n    b\nfooter";
        let ctx = TextObjectContext::new(text, 7); // cursor on "    a"
        let object = TextObject {
            scope: TextObjectScope::Around,
            kind: TextObjectKind::IndentBlockNoBelow,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx).unwrap();
        assert!(result.linewise);
        assert_eq!(result.start(), 0); // includes "header" line
        assert_eq!(result.end(), 19); // does NOT include "footer" (end = start of line 3)
                                      // Contrast with ai which DOES include footer:
        let ai_object = TextObject {
            scope: TextObjectScope::Around,
            kind: TextObjectKind::IndentBlock,
            seek: None,
        };
        let ai_result = dispatch_textobject(ai_object, &ctx).unwrap();
        assert_eq!(ai_result.end(), text.len()); // includes "footer"
    }

    #[test]
    fn test_dispatch_iI_same_as_ii() {
        // iI must produce the same result as ii (inner doesn't use include_below)
        let text = "header\n    a\n    b\nfooter";
        let ctx = TextObjectContext::new(text, 7); // cursor on "    a"
        let ii_obj = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::IndentBlock,
            seek: None,
        };
        let iI_obj = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::IndentBlockNoBelow,
            seek: None,
        };
        let ii_result = dispatch_textobject(ii_obj, &ctx).unwrap();
        let iI_result = dispatch_textobject(iI_obj, &ctx).unwrap();
        assert_eq!(ii_result.start(), iI_result.start());
        assert_eq!(ii_result.end(), iI_result.end());
    }

    #[test]
    fn test_dispatch_indent_no_below_empty() {
        let ctx = TextObjectContext::new("", 0);
        let object = TextObject {
            scope: TextObjectScope::Around,
            kind: TextObjectKind::IndentBlockNoBelow,
            seek: None,
        };
        assert!(dispatch_textobject(object, &ctx).is_none());
    }

    #[test]
    fn test_dispatch_aI_with_count() {
        // Count on indent text objects uses sequential expansion.
        // "header\n    a\n    b\nfooter\n    c\n    d\nend"
        // From "    a" (indent 4, block = lines 1-2), count=2 should extend to next block.
        let text = "header\n    a\n    b\nfooter\n    c\n    d\nend";
        let ctx = TextObjectContext::new(text, 7); // cursor on "    a"
        let object = TextObject {
            scope: TextObjectScope::Around,
            kind: TextObjectKind::IndentBlockNoBelow,
            seek: None,
        };
        let count1 = dispatch_textobject_with_count(object, &ctx, 1).unwrap();
        // count=1: aI selects header + block (lines 0-2)
        assert_eq!(count1.start(), 0);
        assert_eq!(count1.end(), 19); // "header\n    a\n    b\n" (end = start of line 3)
    }

    // ── AnyBracket (ib/ab) dispatch ─────────────────────────────────────

    #[test]
    fn test_dispatch_inner_any_bracket_picks_tightest() {
        // "[{hello}]" — cursor at "hello", inner of {} is "hello" (tighter than [])
        let text = "[{hello}]";
        let ctx = TextObjectContext::new(text, 3); // on 'l'
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::AnyBracket,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx).unwrap();
        assert_eq!(&text[result.start()..result.end()], "hello");
    }

    #[test]
    fn test_dispatch_around_any_bracket_picks_tightest() {
        // "[{hello}]" — cursor at "hello", around {} is "{hello}"
        let text = "[{hello}]";
        let ctx = TextObjectContext::new(text, 3); // on 'l'
        let object = TextObject {
            scope: TextObjectScope::Around,
            kind: TextObjectKind::AnyBracket,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx).unwrap();
        assert_eq!(&text[result.start()..result.end()], "{hello}");
    }

    #[test]
    fn test_dispatch_any_bracket_no_match() {
        let text = "no brackets";
        let ctx = TextObjectContext::new(text, 3);
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::AnyBracket,
            seek: None,
        };
        assert!(dispatch_textobject(object, &ctx).is_none());
    }

    // ── AnyQuote (iq/aq) dispatch ───────────────────────────────────────

    #[test]
    fn test_dispatch_around_any_quote_picks_tightest() {
        // 'he said "hi"' — cursor inside "hi", around picks "hi" (tighter than outer '')
        // Note: Vim's around-quote includes leading whitespace when no trailing ws exists
        let text = r#"'he said "hi"'"#;
        let ctx = TextObjectContext::new(text, 11); // on 'i' in "hi"
        let object = TextObject {
            scope: TextObjectScope::Around,
            kind: TextObjectKind::AnyQuote,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx).unwrap();
        // Around picks the "" pair (tighter), and includes leading space per Vim's a" behavior
        assert_eq!(&text[result.start()..result.end()], " \"hi\"");
    }

    #[test]
    fn test_dispatch_inner_any_quote() {
        let text = r#""hello""#;
        let ctx = TextObjectContext::new(text, 3);
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::AnyQuote,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx).unwrap();
        assert_eq!(&text[result.start()..result.end()], "hello");
    }

    #[test]
    fn test_dispatch_any_quote_no_match() {
        let text = "no quotes";
        let ctx = TextObjectContext::new(text, 3);
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::AnyQuote,
            seek: None,
        };
        assert!(dispatch_textobject(object, &ctx).is_none());
    }

    // ── AnyBracket/AnyQuote with count ──────────────────────────────────

    #[test]
    fn test_dispatch_any_bracket_with_count() {
        // "([{hello}])" — count=2 from 'hello' should step out from {} to []
        let text = "([{hello}])";
        let ctx = TextObjectContext::new(text, 4); // on 'l'
        let object = TextObject {
            scope: TextObjectScope::Around,
            kind: TextObjectKind::AnyBracket,
            seek: None,
        };

        let count1 = dispatch_textobject_with_count(object, &ctx, 1).unwrap();
        let count2 = dispatch_textobject_with_count(object, &ctx, 2).unwrap();

        // count=2 should find a wider range than count=1
        assert!(count2.start() <= count1.start());
        assert!(count2.end() >= count1.end());
    }

    // ═══════════════════════════════════════════════════════════════════
    // targets.vim seek dispatch tests
    // ═══════════════════════════════════════════════════════════════════

    use crate::grammar::types::SeekDirection;

    /// `in"` — seek forward to next double quote, then resolve inner.
    /// Text: `hello "world" "next"`  cursor on 'h' (0)
    /// Should find "next" (the NEXT quote pair forward).
    #[test]
    fn seek_next_inner_double_quote() {
        let text = r#"hello "world" "next""#;
        // Cursor at position 7 = inside "world" → seeking next should find "next"
        let ctx = TextObjectContext::new(text, 7);
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::DoubleQuote,
            seek: Some(SeekDirection::Next),
        };
        let result = dispatch_textobject(object, &ctx);
        assert!(result.is_some(), "should find next quote pair");
        let range = result.unwrap();
        // The next " after cursor(7) is at position 12, dispatching from there
        // should find the "next" pair: inner range is 15..19
        assert_eq!(&text[range.start()..range.end()], "next");
    }

    /// `il"` — seek backward to last double quote.
    /// Text: `"first" hello "second"`  cursor on 'h' (8)
    /// Should find "first" (the previous quote pair).
    #[test]
    fn seek_last_inner_double_quote() {
        let text = r#""first" hello "second""#;
        let ctx = TextObjectContext::new(text, 8); // 'h' in 'hello'
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::DoubleQuote,
            seek: Some(SeekDirection::Last),
        };
        let result = dispatch_textobject(object, &ctx);
        assert!(result.is_some(), "should find last quote pair");
        let range = result.unwrap();
        assert_eq!(&text[range.start()..range.end()], "first");
    }

    /// `in(` — seek forward to next paren pair.
    /// Text: `foo (bar) baz (qux)`  cursor on 'f' (0)
    /// Normal `di(` from position 0 already finds `(bar)` (bracket search forward),
    /// so `din(` finds the NEXT pair after that → `(qux)`.
    #[test]
    fn seek_next_inner_paren() {
        let text = "foo (bar) baz (qux)";
        let ctx = TextObjectContext::new(text, 0); // cursor on 'f'
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Paren,
            seek: Some(SeekDirection::Next),
        };
        let result = dispatch_textobject(object, &ctx);
        assert!(result.is_some());
        let range = result.unwrap();
        assert_eq!(&text[range.start()..range.end()], "qux");
    }

    /// `in(` from inside a paren pair seeks to the next one.
    /// Text: `(bar) (qux)`  cursor on 'b' (1, inside first pair)
    #[test]
    fn seek_next_inner_paren_from_inside() {
        let text = "(bar) (qux)";
        let ctx = TextObjectContext::new(text, 1); // cursor on 'b' inside (bar)
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Paren,
            seek: Some(SeekDirection::Next),
        };
        let result = dispatch_textobject(object, &ctx);
        assert!(result.is_some());
        let range = result.unwrap();
        assert_eq!(&text[range.start()..range.end()], "qux");
    }

    /// `il(` — seek backward to last paren pair.
    /// Text: `(foo) bar (baz)`  cursor on last 'r' (8)
    #[test]
    fn seek_last_inner_paren() {
        let text = "(foo) bar (baz)";
        let ctx = TextObjectContext::new(text, 8); // 'r' in 'bar'
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Paren,
            seek: Some(SeekDirection::Last),
        };
        let result = dispatch_textobject(object, &ctx);
        assert!(result.is_some());
        let range = result.unwrap();
        assert_eq!(&text[range.start()..range.end()], "foo");
    }

    /// `an"` — around next double quote.
    #[test]
    fn seek_next_around_double_quote() {
        let text = r#"hello "world" "next""#;
        let ctx = TextObjectContext::new(text, 7); // inside "world"
        let object = TextObject {
            scope: TextObjectScope::Around,
            kind: TextObjectKind::DoubleQuote,
            seek: Some(SeekDirection::Next),
        };
        let result = dispatch_textobject(object, &ctx);
        assert!(result.is_some());
        let range = result.unwrap();
        // Around includes the quotes themselves
        let selected = &text[range.start()..range.end()];
        assert!(
            selected.contains("next"),
            "around-next should include 'next', got '{selected}'"
        );
    }

    /// `in{` — seek next brace pair.
    /// Normal `di{` from 0 already finds `{ a }` (brace search forward),
    /// so `din{` finds the NEXT pair → `{ b }`.
    #[test]
    fn seek_next_inner_brace() {
        let text = "let x = { a }; let y = { b };";
        let ctx = TextObjectContext::new(text, 0);
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Brace,
            seek: Some(SeekDirection::Next),
        };
        let result = dispatch_textobject(object, &ctx);
        assert!(result.is_some());
        let range = result.unwrap();
        let inner = text[range.start()..range.end()].trim();
        assert_eq!(inner, "b");
    }

    /// No delimiter found forward → None.
    #[test]
    fn seek_next_no_delimiter_returns_none() {
        let text = "hello world";
        let ctx = TextObjectContext::new(text, 0);
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::DoubleQuote,
            seek: Some(SeekDirection::Next),
        };
        let result = dispatch_textobject(object, &ctx);
        assert!(result.is_none());
    }

    /// No delimiter found backward → None.
    #[test]
    fn seek_last_no_delimiter_returns_none() {
        let text = "hello world";
        let ctx = TextObjectContext::new(text, 5);
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Paren,
            seek: Some(SeekDirection::Last),
        };
        let result = dispatch_textobject(object, &ctx);
        assert!(result.is_none());
    }

    /// Seek next with backtick delimiter.
    #[test]
    fn seek_next_backtick() {
        let text = "foo `bar` `baz`";
        let ctx = TextObjectContext::new(text, 5); // inside `bar`
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Backtick,
            seek: Some(SeekDirection::Next),
        };
        let result = dispatch_textobject(object, &ctx);
        assert!(result.is_some());
        let range = result.unwrap();
        assert_eq!(&text[range.start()..range.end()], "baz");
    }

    /// Seek last with single quote.
    #[test]
    fn seek_last_single_quote() {
        let text = "'first' gap 'second'";
        let ctx = TextObjectContext::new(text, 10); // in gap
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::SingleQuote,
            seek: Some(SeekDirection::Last),
        };
        let result = dispatch_textobject(object, &ctx);
        assert!(result.is_some());
        let range = result.unwrap();
        assert_eq!(&text[range.start()..range.end()], "first");
    }

    /// `in[` — seek next bracket from position 0.
    /// Normal `di[` from 0 finds `[one]` → `din[` finds `[two]`.
    #[test]
    fn seek_next_bracket() {
        let text = "a [one] b [two]";
        let ctx = TextObjectContext::new(text, 0);
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Bracket,
            seek: Some(SeekDirection::Next),
        };
        let result = dispatch_textobject(object, &ctx);
        assert!(result.is_some());
        let range = result.unwrap();
        assert_eq!(&text[range.start()..range.end()], "two");
    }

    /// `in[` from inside first bracket finds second.
    #[test]
    fn seek_next_bracket_from_inside() {
        let text = "a [one] b [two]";
        let ctx = TextObjectContext::new(text, 3); // inside [one]
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Bracket,
            seek: Some(SeekDirection::Next),
        };
        let result = dispatch_textobject(object, &ctx);
        assert!(result.is_some());
        let range = result.unwrap();
        assert_eq!(&text[range.start()..range.end()], "two");
    }

    /// `in<` — seek next angle bracket from position 0.
    /// Normal `di<` from 0 finds `<one>` → `din<` finds `<two>`.
    #[test]
    fn seek_next_angle() {
        let text = "a <one> b <two>";
        let ctx = TextObjectContext::new(text, 0);
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Angle,
            seek: Some(SeekDirection::Next),
        };
        let result = dispatch_textobject(object, &ctx);
        assert!(result.is_some());
        let range = result.unwrap();
        assert_eq!(&text[range.start()..range.end()], "two");
    }

    /// Seek without modifier (None) still works normally.
    #[test]
    fn no_seek_still_works() {
        let text = "foo (bar) baz";
        let ctx = TextObjectContext::new(text, 5); // inside parens
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Paren,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx);
        assert!(result.is_some());
        let range = result.unwrap();
        assert_eq!(&text[range.start()..range.end()], "bar");
    }

    // ── Pipe text object (i|/a|) dispatch ─────────────────────────────

    /// `di|` inside `|x, y|` — deletes inner content "x, y"
    #[test]
    fn test_dispatch_inner_pipe_closure_params() {
        let text = "|x, y|";
        let ctx = TextObjectContext::new(text, 1); // cursor on 'x'
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Pipe,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx).unwrap();
        assert_eq!(&text[result.start()..result.end()], "x, y");
    }

    /// `da|` inside `|x, y|` — deletes including pipes
    #[test]
    fn test_dispatch_around_pipe_closure_params() {
        let text = "|x, y|";
        let ctx = TextObjectContext::new(text, 1); // cursor on 'x'
        let object = TextObject {
            scope: TextObjectScope::Around,
            kind: TextObjectKind::Pipe,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx).unwrap();
        assert_eq!(&text[result.start()..result.end()], "|x, y|");
    }

    /// `di|` with cursor in Rust closure: `let f = |a, b| a + b;`
    #[test]
    fn test_dispatch_inner_pipe_rust_closure() {
        let text = "let f = |a, b| a + b;";
        let ctx = TextObjectContext::new(text, 9); // cursor on 'a' inside pipes
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Pipe,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx).unwrap();
        assert_eq!(&text[result.start()..result.end()], "a, b");
    }

    /// `da|` with cursor in Rust closure: `let f = |a, b| a + b;`
    #[test]
    fn test_dispatch_around_pipe_rust_closure() {
        let text = "let f = |a, b| a + b;";
        let ctx = TextObjectContext::new(text, 9); // cursor on 'a' inside pipes
        let object = TextObject {
            scope: TextObjectScope::Around,
            kind: TextObjectKind::Pipe,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx).unwrap();
        // Around includes the pipes and trailing whitespace
        let selected = &text[result.start()..result.end()];
        assert!(selected.starts_with('|'));
        assert!(selected.contains("a, b"));
    }

    /// No pipe on line → returns None
    #[test]
    fn test_dispatch_pipe_no_match() {
        let text = "hello world";
        let ctx = TextObjectContext::new(text, 3);
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Pipe,
            seek: None,
        };
        assert!(dispatch_textobject(object, &ctx).is_none());
    }

    /// Cursor ON the | character — should find the pair
    #[test]
    fn test_dispatch_pipe_cursor_on_delimiter() {
        let text = "|hello|";
        let ctx = TextObjectContext::new(text, 0); // cursor on first |
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Pipe,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx).unwrap();
        assert_eq!(&text[result.start()..result.end()], "hello");
    }

    /// `a|` with cursor on delimiter includes the pipes
    #[test]
    fn test_dispatch_around_pipe_cursor_on_delimiter() {
        let text = "|hello|";
        let ctx = TextObjectContext::new(text, 0); // cursor on first |
        let object = TextObject {
            scope: TextObjectScope::Around,
            kind: TextObjectKind::Pipe,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx).unwrap();
        assert_eq!(&text[result.start()..result.end()], "|hello|");
    }

    // ── Pipe text object: Rust closure use cases ─────────────────────────

    /// `di|` with typed closure params: `|x: i32, y: i32|`
    /// Cursor at start of params (on 'x')
    #[test]
    fn test_dispatch_inner_pipe_typed_closure_at_start() {
        let text = "|x: i32, y: i32| x + y";
        let ctx = TextObjectContext::new(text, 1); // cursor on 'x'
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Pipe,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx).unwrap();
        assert_eq!(&text[result.start()..result.end()], "x: i32, y: i32");
    }

    /// `di|` with typed closure params: cursor in middle of type annotation
    #[test]
    fn test_dispatch_inner_pipe_typed_closure_mid_type() {
        let text = "|x: i32, y: i32| x + y";
        let ctx = TextObjectContext::new(text, 5); // cursor on '3' in i32
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Pipe,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx).unwrap();
        assert_eq!(&text[result.start()..result.end()], "x: i32, y: i32");
    }

    /// `di|` with typed closure: cursor on second param 'y'
    #[test]
    fn test_dispatch_inner_pipe_typed_closure_second_param() {
        let text = "|x: i32, y: i32| x + y";
        let ctx = TextObjectContext::new(text, 9); // cursor on 'y'
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Pipe,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx).unwrap();
        assert_eq!(&text[result.start()..result.end()], "x: i32, y: i32");
    }

    /// `di|` with typed closure: cursor on last char before closing pipe
    #[test]
    fn test_dispatch_inner_pipe_typed_closure_last_char() {
        let text = "|x: i32, y: i32| x + y";
        let ctx = TextObjectContext::new(text, 14); // cursor on '2' (last digit)
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Pipe,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx).unwrap();
        assert_eq!(&text[result.start()..result.end()], "x: i32, y: i32");
    }

    /// `di|` with typed closure: cursor on opening pipe delimiter
    #[test]
    fn test_dispatch_inner_pipe_typed_closure_on_open() {
        let text = "|x: i32, y: i32| x + y";
        let ctx = TextObjectContext::new(text, 0); // cursor on first |
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Pipe,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx).unwrap();
        assert_eq!(&text[result.start()..result.end()], "x: i32, y: i32");
    }

    /// `di|` with typed closure: cursor on closing pipe delimiter
    #[test]
    fn test_dispatch_inner_pipe_typed_closure_on_close() {
        let text = "|x: i32, y: i32| x + y";
        let ctx = TextObjectContext::new(text, 15); // cursor on closing |
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Pipe,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx).unwrap();
        assert_eq!(&text[result.start()..result.end()], "x: i32, y: i32");
    }

    /// `da|` with typed closure: includes delimiters and trailing whitespace
    #[test]
    fn test_dispatch_around_pipe_typed_closure() {
        let text = "|x: i32, y: i32| x + y";
        let ctx = TextObjectContext::new(text, 5); // cursor mid-type
        let object = TextObject {
            scope: TextObjectScope::Around,
            kind: TextObjectKind::Pipe,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx).unwrap();
        let selected = &text[result.start()..result.end()];
        // Around includes pipes and trailing whitespace
        assert!(selected.starts_with('|'));
        assert!(selected.contains("x: i32, y: i32"));
        assert!(selected.contains('|'));
    }

    /// Full Rust closure with let binding: `let f = |x: i32, y: i32| x + y;`
    /// Cursor within params should select params only
    #[test]
    fn test_dispatch_inner_pipe_full_let_closure() {
        let text = "let f = |x: i32, y: i32| x + y;";
        let ctx = TextObjectContext::new(text, 12); // cursor on ':' after x
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Pipe,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx).unwrap();
        assert_eq!(&text[result.start()..result.end()], "x: i32, y: i32");
    }

    /// `yi|` equivalent: verify the range for yank covers exact inner text
    #[test]
    fn test_dispatch_inner_pipe_yank_range() {
        let text = "let f = |x: i32, y: i32| x + y;";
        let ctx = TextObjectContext::new(text, 18); // cursor on 'y' in second param
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Pipe,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx).unwrap();
        // Verify exact start/end offsets for operator composition
        assert_eq!(result.start(), 9); // byte after opening |
        assert_eq!(result.end(), 23); // byte of closing |
        assert_eq!(&text[result.start()..result.end()], "x: i32, y: i32");
        assert!(!result.linewise); // char-wise, not linewise
    }

    /// `ci|` equivalent: verify the range enables correct replacement
    #[test]
    fn test_dispatch_inner_pipe_change_range() {
        let text = "let f = |x: i32, y: i32| x + y;";
        let ctx = TextObjectContext::new(text, 9); // cursor on 'x'
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Pipe,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx).unwrap();
        // After ci| + typing "a", result would be "|a| x + y;"
        // Verify range covers exactly the inner content
        assert_eq!(result.start(), 9);
        assert_eq!(result.end(), 23);
        // Cursor should land at range start after delete (for ci|)
        assert!(!result.linewise);
    }

    /// Pipe outside closure (no pair on same line) returns None
    #[test]
    fn test_dispatch_pipe_single_pipe_no_pair() {
        let text = "a | b";
        // Only one pipe on line — no pair to match
        // Quote algorithm: cursor between the sole pipe — search backward finds
        // it, forward from there finds nothing. Then try forward from cursor: nothing.
        let ctx = TextObjectContext::new(text, 4); // cursor on 'b', after the |
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Pipe,
            seek: None,
        };
        assert!(dispatch_textobject(object, &ctx).is_none());
    }

    // ── Pipe text object: visual mode (`vi|`) ────────────────────────────

    /// `vi|` — visual inner pipe selects content between pipes.
    /// Uses `dispatch_visual_textobject` which is the full visual text object path.
    #[test]
    fn test_visual_inner_pipe() {
        let text = "|hello|";
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Pipe,
            seek: None,
        };
        let providers = crate::document::Providers::new();
        let result = dispatch_visual_textobject(object, text, 3, None, &providers, 1, false);
        // Should produce effects (selection update) — non-empty means vi| succeeded
        assert!(
            !result.effects.is_empty(),
            "vi| should produce selection effects"
        );
    }

    /// `va|` — visual around pipe selects pipes and content.
    #[test]
    fn test_visual_around_pipe() {
        let text = "|hello|";
        let object = TextObject {
            scope: TextObjectScope::Around,
            kind: TextObjectKind::Pipe,
            seek: None,
        };
        let providers = crate::document::Providers::new();
        let result = dispatch_visual_textobject(object, text, 3, None, &providers, 1, false);
        assert!(
            !result.effects.is_empty(),
            "va| should produce selection effects"
        );
    }

    // ── Pipe text object: seek modifier ─────────────────────────────────

    /// `din|` — seek next: from inside first pipe pair, find the second.
    /// Text: `|a| |b|`  cursor inside first pair (on 'a', offset 1)
    #[test]
    fn test_seek_next_inner_pipe() {
        let text = "|a| |b|";
        let ctx = TextObjectContext::new(text, 1); // on 'a' inside |a|
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Pipe,
            seek: Some(SeekDirection::Next),
        };
        let result = dispatch_textobject(object, &ctx);
        assert!(result.is_some(), "din| should find next pipe pair");
        let range = result.unwrap();
        assert_eq!(&text[range.start()..range.end()], "b");
    }

    /// `dil|` — seek last: from inside second pipe pair, find the first.
    /// Text: `|a| |b|`  cursor inside second pair (on 'b', offset 5)
    #[test]
    fn test_seek_last_inner_pipe() {
        let text = "|a| |b|";
        let ctx = TextObjectContext::new(text, 5); // on 'b' inside |b|
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Pipe,
            seek: Some(SeekDirection::Last),
        };
        let result = dispatch_textobject(object, &ctx);
        assert!(result.is_some(), "dil| should find last pipe pair");
        let range = result.unwrap();
        assert_eq!(&text[range.start()..range.end()], "a");
    }

    /// `2i|` — count=2 on inner pipe promotes to around (nesting expansion).
    /// Text: `|hello|`  cursor inside (offset 3)
    /// Count=2 inner tries to step out; since no enclosing pipe pair exists,
    /// it promotes to around (includes delimiters).
    #[test]
    fn test_count_2_inner_pipe() {
        let text = "|hello|";
        let ctx = TextObjectContext::new(text, 3); // on 'l' inside |hello|
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Pipe,
            seek: None,
        };
        let count1 = dispatch_textobject_with_count(object, &ctx, 1).unwrap();
        assert_eq!(&text[count1.start()..count1.end()], "hello");

        let count2 = dispatch_textobject_with_count(object, &ctx, 2).unwrap();
        // count=2 promotes inner to around: includes the pipe delimiters
        assert!(
            count2.start() <= count1.start(),
            "count=2 should expand start: {} <= {}",
            count2.start(),
            count1.start()
        );
        assert!(
            count2.end() >= count1.end(),
            "count=2 should expand end: {} >= {}",
            count2.end(),
            count1.end()
        );
    }

    /// `2a|` — count=2 on around pipe when no enclosing pair exists.
    /// Text: `|hello|`  cursor on 'l' (offset 3)
    /// count=1 finds `|hello|` (around), count=2 can't expand further
    /// so returns the same range (no panic, no crash).
    #[test]
    fn test_count_2_around_pipe_no_expansion() {
        let text = "|hello|";
        let ctx = TextObjectContext::new(text, 3); // on 'l' in hello
        let object = TextObject {
            scope: TextObjectScope::Around,
            kind: TextObjectKind::Pipe,
            seek: None,
        };
        let count1 = dispatch_textobject_with_count(object, &ctx, 1).unwrap();
        assert_eq!(&text[count1.start()..count1.end()], "|hello|");
        // count=2: can't step further out, should still return a valid range
        // (may be same as count=1 since no outer pair exists)
        let count2 = dispatch_textobject_with_count(object, &ctx, 2);
        // Either returns same range or None — both are valid
        if let Some(r2) = count2 {
            assert!(r2.end() - r2.start() >= count1.end() - count1.start());
        }
    }

    /// A custom text object provider returning offset=999999 for a 10-byte
    /// document must NOT panic. The engine must clamp to document bounds.
    #[test]
    fn custom_textobject_out_of_bounds_no_panic() {
        use crate::document::{CustomTextObjectProvider, Providers};

        struct OutOfBoundsProvider;
        impl CustomTextObjectProvider for OutOfBoundsProvider {
            fn compute_textobject(
                &self,
                _id: u32,
                _text: &str,
                _cursor: usize,
                _inner: bool,
            ) -> Option<(usize, usize)> {
                // Return wildly out-of-bounds offsets
                Some((999_999, 999_999))
            }
        }

        let text = "0123456789"; // 10 bytes
        let provider = OutOfBoundsProvider;
        let providers = Providers::new().with_custom_textobjects(&provider);
        let ctx = TextObjectContext::new(text, 0).with_providers(providers);
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Custom(42),
            seek: None,
        };

        // Must not panic — offsets clamped to text.len()
        let result = dispatch_textobject(object, &ctx);
        assert!(result.is_some());
        let range = result.unwrap();
        // Both start and end clamped to 10
        assert!(range.range.start().get() <= text.len());
        assert!(range.range.end().get() <= text.len());
    }

    // ── Argument text object (ia/aa) dispatch — pure-text fallback ─────

    fn ia(text: &str, cur: usize) -> Option<TextObjectRange> {
        dispatch_textobject(
            TextObject {
                scope: TextObjectScope::Inner,
                kind: TextObjectKind::Argument,
                seek: None,
            },
            &TextObjectContext::new(text, cur),
        )
    }
    fn aa(text: &str, cur: usize) -> Option<TextObjectRange> {
        dispatch_textobject(
            TextObject {
                scope: TextObjectScope::Around,
                kind: TextObjectKind::Argument,
                seek: None,
            },
            &TextObjectContext::new(text, cur),
        )
    }
    fn s<'a>(t: &'a str, r: &TextObjectRange) -> &'a str {
        &t[r.start()..r.end()]
    }
    fn d(t: &str, r: &TextObjectRange) -> String {
        format!("{}{}", &t[..r.start()], &t[r.end()..])
    }

    /// dia/daa: selection, cia replacement, daa deletion across all positions.
    #[test]
    fn dispatch_argument_positions_and_effects() {
        let t = "foo(bar, baz, qux)";
        assert_eq!(s(t, &ia(t, 4).unwrap()), "bar");
        assert_eq!(s(t, &ia(t, 9).unwrap()), "baz");
        assert_eq!(s(t, &ia(t, 14).unwrap()), "qux");
        assert_eq!(s(t, &aa(t, 4).unwrap()), "bar, ");
        assert_eq!(s(t, &aa(t, 9).unwrap()), "baz, ");
        assert_eq!(s(t, &aa(t, 14).unwrap()), ", qux");
        // cia middle: baz -> NEW
        let r = ia(t, 9).unwrap();
        assert_eq!(
            format!("{}NEW{}", &t[..r.start()], &t[r.end()..]),
            "foo(bar, NEW, qux)"
        );
        // daa: middle/first/last
        assert_eq!(d(t, &aa(t, 9).unwrap()), "foo(bar, qux)");
        assert_eq!(d(t, &aa(t, 4).unwrap()), "foo(baz, qux)");
        assert_eq!(d(t, &aa(t, 14).unwrap()), "foo(bar, baz)");
        assert!(!ia(t, 4).unwrap().linewise);
    }

    /// Edge cases, bracket types, nested, whitespace, count, visual.
    #[test]
    fn dispatch_argument_edge_nested_count_visual() {
        assert_eq!(s("foo(bar)", &ia("foo(bar)", 4).unwrap()), "bar");
        assert_eq!(s("foo(bar)", &aa("foo(bar)", 4).unwrap()), "bar");
        assert!(ia("foo()", 4).is_none());
        assert!(ia("bare text", 5).is_none());
        assert_eq!(s("[a, b, c]", &ia("[a, b, c]", 4).unwrap()), "b");
        assert_eq!(s("{x, y}", &ia("{x, y}", 1).unwrap()), "x");
        assert_eq!(s("<A, B>", &ia("<A, B>", 1).unwrap()), "A");
        let n = "foo(bar(1, 2), baz)";
        assert_eq!(s(n, &ia(n, 8).unwrap()), "1");
        assert_eq!(s(n, &ia(n, 15).unwrap()), "baz");
        assert_eq!(s(n, &aa(n, 4).unwrap()), "bar(1, 2), ");
        assert_eq!(
            s(
                "foo(  bar  ,  baz  )",
                &ia("foo(  bar  ,  baz  )", 6).unwrap()
            ),
            "bar"
        );
        // Count=2
        let obj = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Argument,
            seek: None,
        };
        let ctx = TextObjectContext::new("foo(bar, baz, qux)", 4);
        if let Some(r) = dispatch_textobject_with_count(obj, &ctx, 2) {
            assert!(r.end() - r.start() >= ia("foo(bar, baz, qux)", 4).unwrap().len());
        }
        // Visual dispatch
        let providers = crate::document::Providers::new();
        let vr = dispatch_visual_textobject(obj, "foo(bar, baz)", 4, None, &providers, 1, false);
        assert!(!vr.effects.is_empty());
    }

    // ── Tag text object: tree-sitter fallback ─────────────────────────

    /// Without a syntax provider, `it`/`at` still works via text-based fallback.
    #[test]
    fn tag_fallback_to_text_when_no_syntax_provider() {
        let text = "<div>hello</div>";
        let ctx = TextObjectContext::new(text, 6); // cursor on 'e' in "hello"
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Tag,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx);
        assert!(
            result.is_some(),
            "it should fall back to text-based tag matching"
        );
        let range = result.unwrap();
        assert_eq!(&text[range.start()..range.end()], "hello");
    }

    /// With a syntax provider that returns a tag range, syntax takes priority.
    #[test]
    fn tag_prefers_syntax_provider_when_available() {
        use crate::document::{Providers, SyntaxNodeKind, SyntaxProvider};

        struct TagSyntaxProvider;
        impl SyntaxProvider for TagSyntaxProvider {
            fn enclosing_node(
                &self,
                _text: &str,
                _cursor: usize,
                kind: SyntaxNodeKind,
            ) -> Option<(usize, usize)> {
                if kind == SyntaxNodeKind::Tag {
                    // Return a range that differs from text-based parse
                    // to prove syntax was used.
                    Some((0, 16))
                } else {
                    None
                }
            }

            fn next_node(&self, _: &str, _: usize, _: SyntaxNodeKind, _: u32) -> Option<usize> {
                None
            }

            fn prev_node(&self, _: &str, _: usize, _: SyntaxNodeKind, _: u32) -> Option<usize> {
                None
            }
        }

        let text = "<div>hello</div>";
        let provider = TagSyntaxProvider;
        let providers = Providers::new().with_syntax(&provider);
        let ctx = TextObjectContext::new(text, 6).with_providers(providers);
        let object = TextObject {
            scope: TextObjectScope::Around,
            kind: TextObjectKind::Tag,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx);
        assert!(result.is_some(), "at should use syntax provider");
        let range = result.unwrap();
        // Syntax provider returns (0, 16) for around scope, so we get the whole tag.
        assert_eq!(range.start(), 0);
        assert_eq!(range.end(), 16);
    }

    /// When syntax provider returns None for Tag, text-based fallback is used.
    #[test]
    fn tag_falls_back_when_syntax_returns_none() {
        use crate::document::{Providers, SyntaxNodeKind, SyntaxProvider};

        struct NoTagSyntaxProvider;
        impl SyntaxProvider for NoTagSyntaxProvider {
            fn enclosing_node(
                &self,
                _text: &str,
                _cursor: usize,
                _kind: SyntaxNodeKind,
            ) -> Option<(usize, usize)> {
                None
            }

            fn next_node(&self, _: &str, _: usize, _: SyntaxNodeKind, _: u32) -> Option<usize> {
                None
            }

            fn prev_node(&self, _: &str, _: usize, _: SyntaxNodeKind, _: u32) -> Option<usize> {
                None
            }
        }

        let text = "<div>hello</div>";
        let provider = NoTagSyntaxProvider;
        let providers = Providers::new().with_syntax(&provider);
        let ctx = TextObjectContext::new(text, 6).with_providers(providers);
        let object = TextObject {
            scope: TextObjectScope::Inner,
            kind: TextObjectKind::Tag,
            seek: None,
        };
        let result = dispatch_textobject(object, &ctx);
        assert!(result.is_some(), "it should fall back to text parsing");
        let range = result.unwrap();
        assert_eq!(&text[range.start()..range.end()], "hello");
    }
}
