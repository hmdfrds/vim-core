//! # Annotated Text DSL for Tests
//!
//! Encode cursor positions and selections directly in test strings,
//! replacing raw byte offsets with readable inline markers.
//!
//! # Syntax
//!
//! **Cursor** (no visual selection):
//! ```text
//! "hel|lo"    → text = "hello", cursor at byte 3
//! "|hello"    → cursor at byte 0
//! "hello|"    → cursor at byte 5 (end of text)
//! ```
//!
//! **Selection** (visual mode, with anchor and head):
//! ```text
//! "h#[ell|]#o"  → forward: anchor=1, head=4
//! "h#[|ell]#o"  → backward: anchor=4, head=1
//! "#[|]#hello"  → collapsed selection at 0
//! ```
//!
//! `|` marks the **head** (cursor position). In selection mode, the
//! opposite bracket boundary is the **anchor**.
//!
//! # Round-trip Property
//!
//! `parse(annotate(text, spec)) == (text.to_string(), spec)` — always holds.

use crate::primitives::{CursorMode, Offset, SelectionRange, Selections};

// ═══════════════════════════════════════════════════════════════════════════
// TYPES
// ═══════════════════════════════════════════════════════════════════════════

/// Cursor or selection parsed from annotated text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorSpec {
    /// Simple cursor position (no visual selection).
    Cursor(Offset),
    /// Visual selection with anchor (fixed end) and head (cursor end).
    Selection {
        /// The fixed end of the selection.
        anchor: Offset,
        /// The moving end (cursor position) of the selection.
        head: Offset,
    },
}

impl CursorSpec {
    /// Get the head/cursor byte offset.
    ///
    /// For `Cursor`, returns the cursor offset.
    /// For `Selection`, returns the head (moving end).
    #[inline]
    #[must_use]
    pub const fn offset(&self) -> Offset {
        match self {
            Self::Cursor(o) => *o,
            Self::Selection { head, .. } => *head,
        }
    }

    /// Convert to a `SelectionRange`.
    ///
    /// For `Cursor`, returns a collapsed selection (anchor == head).
    /// For `Selection`, returns the full selection range.
    #[inline]
    #[must_use]
    pub const fn selection_range(&self) -> SelectionRange {
        match self {
            Self::Cursor(o) => SelectionRange::insert_cursor(*o),
            Self::Selection { anchor, head } => SelectionRange::new(*anchor, *head),
        }
    }

    /// True if this represents a visual selection (not just a cursor).
    #[inline]
    #[must_use]
    pub const fn is_selection(&self) -> bool {
        match self {
            Self::Cursor(_) => false,
            Self::Selection { .. } => true,
        }
    }

    /// Get the anchor offset, if this is a selection.
    #[inline]
    #[must_use]
    pub const fn anchor(&self) -> Option<Offset> {
        match self {
            Self::Cursor(_) => None,
            Self::Selection { anchor, .. } => Some(*anchor),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// PARSE — annotated string → (clean text, CursorSpec)
// ═══════════════════════════════════════════════════════════════════════════

/// Parse annotated text into clean text and a cursor/selection specification.
///
/// # Syntax
///
/// - `|` — cursor position (when no `#[...]#` brackets present)
/// - `#[text|]#` — forward selection: anchor at `#[`, head at `|`
/// - `#[|text]#` — backward selection: head at `|`, anchor at `]#`
/// - `#[|]#` — collapsed selection (anchor == head)
///
/// # Panics
///
/// - No cursor marker (`|` or `#[|]#`) found
/// - Unclosed `#[` bracket
/// - Multiple `|` markers inside a single `#[...]#`
#[allow(
    clippy::indexing_slicing,
    reason = "byte-level scanning with explicit bounds checks at every access"
)]
#[must_use]
pub fn parse(s: &str) -> (String, CursorSpec) {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut clean = String::with_capacity(len);
    let mut i = 0;

    // Tracking state
    let mut cursor_pos: Option<usize> = None;
    let mut head_pos: Option<usize> = None;
    let mut in_selection = false;
    let mut selection_start: usize = 0;
    let mut selection_end: Option<usize> = None;

    while i < len {
        // ── Selection start: #[ ──────────────────────────────────────
        if !in_selection && i + 1 < len && bytes[i] == b'#' && bytes[i + 1] == b'[' {
            in_selection = true;
            selection_start = clean.len();
            i += 2;
            continue;
        }

        // ── Selection end: ]# ────────────────────────────────────────
        if in_selection && i + 1 < len && bytes[i] == b']' && bytes[i + 1] == b'#' {
            in_selection = false;
            selection_end = Some(clean.len());
            i += 2;
            continue;
        }

        // ── Head/cursor marker: | ────────────────────────────────────
        if bytes[i] == b'|' {
            if in_selection {
                assert!(
                    head_pos.is_none(),
                    "multiple '|' markers inside #[...]# in: {s:?}"
                );
                head_pos = Some(clean.len());
            } else if cursor_pos.is_none() && selection_end.is_none() {
                cursor_pos = Some(clean.len());
            }
            i += 1;
            continue;
        }

        // ── Regular character (may be multi-byte UTF-8) ──────────────
        let ch_start = i;
        i += 1;
        while i < len && (bytes[i] & 0xC0) == 0x80 {
            i += 1;
        }
        clean.push_str(&s[ch_start..i]);
    }

    assert!(!in_selection, "unclosed '#[' bracket in: {s:?}");

    // ── Build CursorSpec ─────────────────────────────────────────────
    if let (Some(h), Some(sel_end)) = (head_pos, selection_end) {
        // Selection mode: determine anchor from head position
        let anchor = if h == selection_start {
            // #[|text]# → head at start, anchor at end (backward)
            sel_end
        } else {
            // #[text|]# → head at end, anchor at start (forward)
            selection_start
        };
        (
            clean,
            CursorSpec::Selection {
                anchor: Offset::new(anchor),
                head: Offset::new(h),
            },
        )
    } else if let Some(c) = cursor_pos {
        (clean, CursorSpec::Cursor(Offset::new(c)))
    } else {
        panic!("no cursor marker ('|' or '#[|]#') found in: {s:?}");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ANNOTATE — (clean text, CursorSpec) → annotated string
// ═══════════════════════════════════════════════════════════════════════════

/// Generate an annotated string from clean text and a cursor/selection spec.
///
/// This is the inverse of [`parse`]: `parse(annotate(text, &spec)) == (text.to_string(), spec)`.
///
/// # Panics
///
/// - Offset out of bounds for the given text
/// - Offset not on a UTF-8 char boundary
#[allow(
    clippy::indexing_slicing,
    reason = "offsets validated by assertions before slicing"
)]
#[must_use]
pub fn annotate(text: &str, spec: &CursorSpec) -> String {
    match spec {
        CursorSpec::Cursor(offset) => {
            let pos = offset.get();
            assert!(
                pos <= text.len() && text.is_char_boundary(pos),
                "cursor offset {pos} is out of bounds or not on a char boundary \
                 (text length: {})",
                text.len()
            );
            let mut result = String::with_capacity(text.len() + 1);
            result.push_str(&text[..pos]);
            result.push('|');
            result.push_str(&text[pos..]);
            result
        }
        CursorSpec::Selection { anchor, head } => {
            let a = anchor.get();
            let h = head.get();
            assert!(
                a <= text.len() && text.is_char_boundary(a),
                "anchor offset {a} is out of bounds or not on a char boundary \
                 (text length: {})",
                text.len()
            );
            assert!(
                h <= text.len() && text.is_char_boundary(h),
                "head offset {h} is out of bounds or not on a char boundary \
                 (text length: {})",
                text.len()
            );

            // Markers add 5 bytes: "#[" (2) + "|" (1) + "]#" (2)
            let mut result = String::with_capacity(text.len() + 5);

            if h <= a {
                // Backward or collapsed: #[|text]#
                result.push_str(&text[..h]);
                result.push_str("#[|");
                result.push_str(&text[h..a]);
                result.push_str("]#");
                result.push_str(&text[a..]);
            } else {
                // Forward: #[text|]#
                result.push_str(&text[..a]);
                result.push_str("#[");
                result.push_str(&text[a..h]);
                result.push_str("|]#");
                result.push_str(&text[h..]);
            }

            result
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// PARSE BLOCK — multiline annotated string with auto-dedent
// ═══════════════════════════════════════════════════════════════════════════

/// Parse a multiline annotated block, stripping leading/trailing blank lines
/// and removing shared indentation before delegating to [`parse`].
///
/// This lets tests embed annotated text inline with natural indentation:
///
/// ```rust,ignore
/// let (text, spec) = parse_block("
///     first line
///     second |line
///     third line
/// ");
/// assert_eq!(text, "first line\nsecond line\nthird line");
/// ```
#[must_use]
pub fn parse_block(block: &str) -> (String, CursorSpec) {
    let lines: Vec<&str> = block.lines().collect();
    let start = lines.iter().position(|l| !l.trim().is_empty()).unwrap_or(0);
    let end = lines
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .map_or(start, |i| i + 1);
    let body = &lines[start..end];

    if body.is_empty() {
        return parse(block);
    }

    let shared = body
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);

    let dedented: Vec<&str> = body
        .iter()
        .map(|l| {
            if l.trim().is_empty() {
                ""
            } else {
                &l[shared..]
            }
        })
        .collect();

    parse(&dedented.join("\n"))
}

// ═══════════════════════════════════════════════════════════════════════════
// MULTI-CURSOR TYPES
// ═══════════════════════════════════════════════════════════════════════════

use std::collections::BTreeMap;

/// Multi-cursor specification parsed from annotated text with numbered markers.
///
/// Cursor 1 is always the primary cursor. Numbering must be contiguous
/// starting from 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiCursorSpec {
    cursors: BTreeMap<u32, CursorSpec>,
}

impl MultiCursorSpec {
    /// Number of cursors.
    #[must_use]
    pub fn len(&self) -> usize {
        self.cursors.len()
    }

    /// True if empty (should never happen after successful parse).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cursors.is_empty()
    }

    /// Get the primary cursor spec (cursor 1).
    ///
    /// # Panics
    ///
    /// Panics if there is no cursor numbered 1. Every constructor in this
    /// module ([`parse_multi`], [`parse_multi_block`], [`Self::from_pairs`])
    /// rejects specs whose numbering does not start at 1, so this can only
    /// trigger on a `MultiCursorSpec` built by other means.
    #[must_use]
    pub fn primary(&self) -> &CursorSpec {
        self.cursors.get(&1).expect("cursor 1 (primary) must exist")
    }

    /// Get a cursor by number (1-based).
    #[must_use]
    pub fn get(&self, n: u32) -> Option<&CursorSpec> {
        self.cursors.get(&n)
    }

    /// Iterate in cursor-number order (1, 2, 3, ...).
    pub fn iter(&self) -> impl Iterator<Item = (u32, &CursorSpec)> {
        self.cursors.iter().map(|(&k, v)| (k, v))
    }

    /// True if this has exactly one cursor.
    #[must_use]
    pub fn is_single(&self) -> bool {
        self.cursors.len() == 1
    }

    /// If single cursor, return its spec.
    #[must_use]
    pub fn as_single(&self) -> Option<&CursorSpec> {
        if self.is_single() {
            Some(self.primary())
        } else {
            None
        }
    }

    /// Convert to runtime `Selections` type.
    ///
    /// Cursor 1 is primary. Ranges are sorted by position via `normalize()`.
    #[must_use]
    pub fn to_selections(&self) -> Selections {
        let ranges: Vec<SelectionRange> = self
            .cursors
            .values()
            .map(CursorSpec::selection_range)
            .collect();
        let mut selections = Selections::from_vec(ranges, 0).normalize();
        if self.cursors.len() > 1 {
            selections.set_cursor_mode(CursorMode::Multi);
        }
        selections
    }

    /// Build from a list of `(cursor_number, CursorSpec)` pairs.
    ///
    /// # Panics
    ///
    /// Panics if `pairs` repeats a cursor number, if `pairs` is empty, if the
    /// lowest cursor number is not 1, or if the highest cursor number is not
    /// equal to the number of cursors (i.e. the numbering has a gap). Cursor
    /// numbers must be exactly `1..=n`.
    #[must_use]
    pub fn from_pairs(pairs: impl IntoIterator<Item = (u32, CursorSpec)>) -> Self {
        let mut cursors = BTreeMap::new();
        for (id, spec) in pairs {
            assert!(
                cursors.insert(id, spec).is_none(),
                "duplicate cursor number {id} in from_pairs"
            );
        }
        assert!(
            !cursors.is_empty(),
            "MultiCursorSpec must have at least one cursor"
        );
        assert!(
            *cursors.keys().next().unwrap() == 1,
            "cursor numbers must start from 1, got {:?}",
            cursors.keys().collect::<Vec<_>>()
        );
        let max_id = *cursors.keys().last().unwrap();
        assert!(
            u32::try_from(cursors.len()).is_ok_and(|count| max_id == count),
            "cursor numbers must be contiguous: found {:?}",
            cursors.keys().collect::<Vec<_>>()
        );
        Self { cursors }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// PARSE MULTI — annotated string → (clean text, MultiCursorSpec)
// ═══════════════════════════════════════════════════════════════════════════

/// Intermediate builder used during parse_multi.
enum CursorBuilder {
    Cursor(usize),
    Selection { anchor: usize, head: usize },
}

/// Parse a cursor number starting at `start`. Returns `(number, digit_count)`.
/// If no digit found, returns `(1, 0)` (bare `|` = cursor 1).
fn parse_cursor_number(bytes: &[u8], start: usize) -> (u32, usize) {
    let mut i = start;
    let mut num: u32 = 0;
    let mut digits = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        num = num * 10 + u32::from(bytes[i] - b'0');
        digits += 1;
        i += 1;
    }
    if digits == 0 {
        (1, 0)
    } else {
        (num, digits)
    }
}

/// Try to parse `#N[` starting at position `i` (where `bytes[i] == b'#'`).
/// Returns `Some((cursor_number, total_bytes_consumed))` on success.
fn try_parse_selection_open(bytes: &[u8], i: usize) -> Option<(u32, usize)> {
    let mut j = i + 1;
    let mut num: u32 = 0;
    let mut digits = 0;
    while j < bytes.len() && bytes[j].is_ascii_digit() {
        num = num * 10 + u32::from(bytes[j] - b'0');
        digits += 1;
        j += 1;
    }
    if digits > 0 && j < bytes.len() && bytes[j] == b'[' {
        Some((num, 1 + digits + 1))
    } else {
        None
    }
}

/// Parse multi-cursor annotated text into clean text and a
/// [`MultiCursorSpec`].
///
/// # Syntax
///
/// - `|N` — cursor N at this position (`|1` is primary)
/// - `|` (bare, no digit) — shorthand for `|1`
/// - `#N[text|N]#` — forward selection for cursor N
/// - `#N[|Ntext]#` — backward selection for cursor N
/// - `||` — literal `|` character in text
/// - `##` — literal `#` when followed by digit+`[`
///
/// # Rules
///
/// - Cursor 1 is always primary.
/// - Numbers must be contiguous starting from 1.
/// - `|N` inside a selection must be at a boundary (immediately after
///   `#N[` or immediately before `]#`).
///
/// # Panics
///
/// On malformed input (no markers, duplicate cursor, gap in numbering,
/// unclosed selection, head not at boundary).
#[allow(
    clippy::indexing_slicing,
    reason = "byte-level scanning with explicit bounds checks"
)]
#[must_use]
pub fn parse_multi(s: &str) -> (String, MultiCursorSpec) {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut clean = String::with_capacity(len);
    let mut i = 0;

    let mut cursors: BTreeMap<u32, CursorBuilder> = BTreeMap::new();
    let mut in_selection: Option<u32> = None;
    let mut selection_start: usize = 0;
    let mut selection_head: Option<usize> = None;

    while i < len {
        // ── Escaped pipe: || ─────────────────────────────────────
        if bytes[i] == b'|' && i + 1 < len && bytes[i + 1] == b'|' {
            clean.push('|');
            i += 2;
            continue;
        }

        // ── Cursor/head marker: |N or bare | ─────────────────────
        if bytes[i] == b'|' {
            let (num, digits) = parse_cursor_number(bytes, i + 1);
            if let Some(sel_id) = in_selection {
                assert!(
                    num == sel_id,
                    "head marker |{num} inside selection #{sel_id}[...] \
                     (expected |{sel_id}) in: {s:?}"
                );
                assert!(
                    selection_head.is_none(),
                    "multiple |{sel_id} markers inside #{sel_id}[...] in: {s:?}"
                );
                selection_head = Some(clean.len());
            } else {
                assert!(
                    !cursors.contains_key(&num),
                    "cursor {num} defined twice in: {s:?}"
                );
                cursors.insert(num, CursorBuilder::Cursor(clean.len()));
            }
            i += 1 + digits;
            continue;
        }

        // ── Escaped hash: ## (only when followed by digit+[) ─────
        if bytes[i] == b'#' && i + 1 < len && bytes[i + 1] == b'#' {
            let mut k = i + 2;
            let mut has_digit = false;
            while k < len && bytes[k].is_ascii_digit() {
                has_digit = true;
                k += 1;
            }
            if has_digit && k < len && bytes[k] == b'[' {
                clean.push('#');
                i += 2;
                continue;
            }
        }

        // ── Selection start: #N[ ─────────────────────────────────
        if bytes[i] == b'#' && in_selection.is_none() {
            if let Some((num, consumed)) = try_parse_selection_open(bytes, i) {
                assert!(
                    !cursors.contains_key(&num),
                    "cursor {num} defined twice in: {s:?}"
                );
                in_selection = Some(num);
                selection_start = clean.len();
                selection_head = None;
                i += consumed;
                continue;
            }
        }

        // ── Selection end: ]# ────────────────────────────────────
        if in_selection.is_some() && i + 1 < len && bytes[i] == b']' && bytes[i + 1] == b'#' {
            let sel_id = in_selection.unwrap();
            let sel_end = clean.len();
            let head = selection_head.unwrap_or_else(|| {
                panic!("selection #{sel_id}[...] has no |{sel_id} head marker in: {s:?}");
            });

            assert!(
                head == selection_start || head == sel_end,
                "|{sel_id} must be immediately after #{sel_id}[ or before ]# \
                 (head={head}, start={selection_start}, end={sel_end}) in: {s:?}"
            );

            let (anchor, head_offset) = if head == selection_start {
                (sel_end, selection_start)
            } else {
                (selection_start, sel_end)
            };

            cursors.insert(
                sel_id,
                CursorBuilder::Selection {
                    anchor,
                    head: head_offset,
                },
            );

            in_selection = None;
            i += 2;
            continue;
        }

        // ── Regular character (may be multi-byte UTF-8) ──────────
        let ch_start = i;
        i += 1;
        while i < len && (bytes[i] & 0xC0) == 0x80 {
            i += 1;
        }
        clean.push_str(&s[ch_start..i]);
    }

    assert!(
        in_selection.is_none(),
        "unclosed selection #{}[ in: {s:?}",
        in_selection.unwrap()
    );
    assert!(!cursors.is_empty(), "no cursor markers found in: {s:?}");
    assert!(
        !cursors.contains_key(&0),
        "cursor number 0 is invalid (numbering starts from 1) in: {s:?}"
    );

    let max_id = *cursors.keys().last().unwrap();
    assert!(
        u32::try_from(cursors.len()).is_ok_and(|count| max_id == count),
        "cursor numbers must be contiguous starting from 1: \
         found {:?} in: {s:?}",
        cursors.keys().collect::<Vec<_>>()
    );

    let spec_map: BTreeMap<u32, CursorSpec> = cursors
        .into_iter()
        .map(|(id, builder)| {
            let spec = match builder {
                CursorBuilder::Cursor(offset) => CursorSpec::Cursor(Offset::new(offset)),
                CursorBuilder::Selection { anchor, head } => CursorSpec::Selection {
                    anchor: Offset::new(anchor),
                    head: Offset::new(head),
                },
            };
            (id, spec)
        })
        .collect();

    (clean, MultiCursorSpec { cursors: spec_map })
}

// ═══════════════════════════════════════════════════════════════════════════
// ANNOTATE MULTI — (clean text, MultiCursorSpec) → annotated string
// ═══════════════════════════════════════════════════════════════════════════

/// Generate a multi-cursor annotated string from clean text and a
/// [`MultiCursorSpec`].
///
/// Inverse of [`parse_multi`]:
/// `parse_multi(annotate_multi(text, &spec)) == (text.to_string(), spec)`.
///
/// Escapes literal `|` as `||` and `#` as `##` (when followed by digit+`[`)
/// to prevent ambiguity.
///
/// # Panics
///
/// Panics if any offset in `spec` does not address a valid position in `text`:
/// a cursor offset, or a selection's anchor or head, that is greater than
/// `text.len()` or that falls inside a multi-byte UTF-8 sequence rather than on
/// a character boundary.
#[allow(
    clippy::indexing_slicing,
    reason = "byte positions validated by assertion and char-boundary checks"
)]
#[must_use]
pub fn annotate_multi(text: &str, spec: &MultiCursorSpec) -> String {
    // Collect markers: (byte_position, marker_text, order)
    // order: 0 = close/head+close, 1 = bare cursor, 2 = open/open+head
    let mut markers: Vec<(usize, String, u8)> = Vec::new();

    for (&num, cursor_spec) in &spec.cursors {
        match cursor_spec {
            CursorSpec::Cursor(offset) => {
                let pos = offset.get();
                assert!(
                    pos <= text.len() && text.is_char_boundary(pos),
                    "cursor {num} offset {pos} out of bounds or not on char boundary \
                     (text length: {})",
                    text.len()
                );
                markers.push((pos, format!("|{num}"), 1));
            }
            CursorSpec::Selection { anchor, head } => {
                let a = anchor.get();
                let h = head.get();
                assert!(
                    a <= text.len() && text.is_char_boundary(a),
                    "cursor {num} anchor {a} out of bounds (text length: {})",
                    text.len()
                );
                assert!(
                    h <= text.len() && text.is_char_boundary(h),
                    "cursor {num} head {h} out of bounds (text length: {})",
                    text.len()
                );

                if h <= a {
                    // Backward or collapsed: #N[|N at head, ]# at anchor
                    markers.push((h, format!("#{num}[|{num}"), 2));
                    markers.push((a, "]#".to_owned(), 0));
                } else {
                    // Forward: #N[ at anchor, |N]# at head
                    markers.push((a, format!("#{num}["), 2));
                    markers.push((h, format!("|{num}]#"), 0));
                }
            }
        }
    }

    markers.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.cmp(&b.2)));

    let bytes = text.as_bytes();
    let mut result = String::with_capacity(text.len() + markers.len() * 4);
    let mut text_pos = 0;
    let mut marker_idx = 0;

    while text_pos <= text.len() {
        while marker_idx < markers.len() && markers[marker_idx].0 == text_pos {
            result.push_str(&markers[marker_idx].1);
            marker_idx += 1;
        }

        if text_pos >= text.len() {
            break;
        }

        // Escape and copy next character
        if bytes[text_pos] == b'|' {
            result.push_str("||");
            text_pos += 1;
        } else if bytes[text_pos] == b'#' {
            let mut j = text_pos + 1;
            let mut has_digits = false;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                has_digits = true;
                j += 1;
            }
            if has_digits && j < bytes.len() && bytes[j] == b'[' {
                result.push_str("##");
            } else {
                result.push('#');
            }
            text_pos += 1;
        } else {
            let ch_start = text_pos;
            text_pos += 1;
            while text_pos < bytes.len() && (bytes[text_pos] & 0xC0) == 0x80 {
                text_pos += 1;
            }
            result.push_str(&text[ch_start..text_pos]);
        }
    }

    result
}

// ═══════════════════════════════════════════════════════════════════════════
// PARSE MULTI BLOCK — multiline multi-cursor with auto-dedent
// ═══════════════════════════════════════════════════════════════════════════

/// Parse a multiline multi-cursor annotated block, stripping
/// leading/trailing blank lines and removing shared indentation before
/// delegating to [`parse_multi`].
#[must_use]
pub fn parse_multi_block(block: &str) -> (String, MultiCursorSpec) {
    let lines: Vec<&str> = block.lines().collect();
    let start = lines.iter().position(|l| !l.trim().is_empty()).unwrap_or(0);
    let end = lines
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .map_or(start, |i| i + 1);
    let body = &lines[start..end];

    if body.is_empty() {
        return parse_multi(block);
    }

    let shared = body
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);

    let dedented: Vec<&str> = body
        .iter()
        .map(|l| {
            if l.trim().is_empty() {
                ""
            } else {
                &l[shared..]
            }
        })
        .collect();

    parse_multi(&dedented.join("\n"))
}

// ═══════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── Parse: cursor ────────────────────────────────────────────────

    #[test]
    fn parse_cursor_middle() {
        let (text, spec) = parse("hel|lo");
        assert_eq!(text, "hello");
        assert_eq!(spec, CursorSpec::Cursor(Offset::new(3)));
    }

    #[test]
    fn parse_cursor_start() {
        let (text, spec) = parse("|hello");
        assert_eq!(text, "hello");
        assert_eq!(spec, CursorSpec::Cursor(Offset::new(0)));
    }

    #[test]
    fn parse_cursor_end() {
        let (text, spec) = parse("hello|");
        assert_eq!(text, "hello");
        assert_eq!(spec, CursorSpec::Cursor(Offset::new(5)));
    }

    #[test]
    fn parse_cursor_empty_text() {
        let (text, spec) = parse("|");
        assert_eq!(text, "");
        assert_eq!(spec, CursorSpec::Cursor(Offset::new(0)));
    }

    #[test]
    fn parse_cursor_multiline() {
        let (text, spec) = parse("hello\n|world");
        assert_eq!(text, "hello\nworld");
        assert_eq!(spec, CursorSpec::Cursor(Offset::new(6)));
    }

    // ── Parse: selection ─────────────────────────────────────────────

    #[test]
    fn parse_selection_forward() {
        let (text, spec) = parse("h#[ell|]#o");
        assert_eq!(text, "hello");
        assert_eq!(
            spec,
            CursorSpec::Selection {
                anchor: Offset::new(1),
                head: Offset::new(4),
            }
        );
    }

    #[test]
    fn parse_selection_backward() {
        let (text, spec) = parse("h#[|ell]#o");
        assert_eq!(text, "hello");
        assert_eq!(
            spec,
            CursorSpec::Selection {
                anchor: Offset::new(4),
                head: Offset::new(1),
            }
        );
    }

    #[test]
    fn parse_selection_collapsed() {
        let (text, spec) = parse("#[|]#hello");
        assert_eq!(text, "hello");
        assert_eq!(
            spec,
            CursorSpec::Selection {
                anchor: Offset::new(0),
                head: Offset::new(0),
            }
        );
    }

    #[test]
    fn parse_selection_whole_text() {
        let (text, spec) = parse("#[hello|]#");
        assert_eq!(text, "hello");
        assert_eq!(
            spec,
            CursorSpec::Selection {
                anchor: Offset::new(0),
                head: Offset::new(5),
            }
        );
    }

    #[test]
    fn parse_selection_at_end() {
        let (text, spec) = parse("hello#[|]#");
        assert_eq!(text, "hello");
        assert_eq!(
            spec,
            CursorSpec::Selection {
                anchor: Offset::new(5),
                head: Offset::new(5),
            }
        );
    }

    #[test]
    fn parse_selection_multiline() {
        let (text, spec) = parse("hel#[lo\nwor|]#ld");
        assert_eq!(text, "hello\nworld");
        assert_eq!(
            spec,
            CursorSpec::Selection {
                anchor: Offset::new(3),
                head: Offset::new(9),
            }
        );
    }

    // ── Parse: UTF-8 ─────────────────────────────────────────────────

    #[test]
    fn parse_cursor_multibyte() {
        // 'é' is 2 bytes, so cursor after 'é' is at byte 3
        let (text, spec) = parse("hé|llo");
        assert_eq!(text, "héllo");
        assert_eq!(spec, CursorSpec::Cursor(Offset::new(3)));
    }

    #[test]
    fn parse_cursor_cjk() {
        // '世' is 3 bytes, '界' is 3 bytes
        let (text, spec) = parse("世|界");
        assert_eq!(text, "世界");
        assert_eq!(spec, CursorSpec::Cursor(Offset::new(3)));
    }

    #[test]
    fn parse_selection_multibyte() {
        let (text, spec) = parse("#[hé|]#llo");
        assert_eq!(text, "héllo");
        assert_eq!(
            spec,
            CursorSpec::Selection {
                anchor: Offset::new(0),
                head: Offset::new(3),
            }
        );
    }

    // ── Parse: error cases ───────────────────────────────────────────

    #[test]
    #[should_panic(expected = "no cursor marker")]
    fn parse_no_marker() {
        parse("hello");
    }

    #[test]
    #[should_panic(expected = "unclosed")]
    fn parse_unclosed_bracket() {
        parse("hel#[lo");
    }

    #[test]
    #[should_panic(expected = "multiple '|'")]
    fn parse_double_pipe_in_selection() {
        parse("#[h|el|lo]#");
    }

    // ── Annotate: cursor ─────────────────────────────────────────────

    #[test]
    fn annotate_cursor_middle() {
        let result = annotate("hello", &CursorSpec::Cursor(Offset::new(3)));
        assert_eq!(result, "hel|lo");
    }

    #[test]
    fn annotate_cursor_start() {
        let result = annotate("hello", &CursorSpec::Cursor(Offset::new(0)));
        assert_eq!(result, "|hello");
    }

    #[test]
    fn annotate_cursor_end() {
        let result = annotate("hello", &CursorSpec::Cursor(Offset::new(5)));
        assert_eq!(result, "hello|");
    }

    #[test]
    fn annotate_cursor_empty() {
        let result = annotate("", &CursorSpec::Cursor(Offset::new(0)));
        assert_eq!(result, "|");
    }

    // ── Annotate: selection ──────────────────────────────────────────

    #[test]
    fn annotate_selection_forward() {
        let result = annotate(
            "hello",
            &CursorSpec::Selection {
                anchor: Offset::new(1),
                head: Offset::new(4),
            },
        );
        assert_eq!(result, "h#[ell|]#o");
    }

    #[test]
    fn annotate_selection_backward() {
        let result = annotate(
            "hello",
            &CursorSpec::Selection {
                anchor: Offset::new(4),
                head: Offset::new(1),
            },
        );
        assert_eq!(result, "h#[|ell]#o");
    }

    #[test]
    fn annotate_selection_collapsed() {
        let result = annotate(
            "hello",
            &CursorSpec::Selection {
                anchor: Offset::new(0),
                head: Offset::new(0),
            },
        );
        assert_eq!(result, "#[|]#hello");
    }

    #[test]
    fn annotate_selection_whole_text() {
        let result = annotate(
            "hello",
            &CursorSpec::Selection {
                anchor: Offset::new(0),
                head: Offset::new(5),
            },
        );
        assert_eq!(result, "#[hello|]#");
    }

    // ── Annotate: UTF-8 ──────────────────────────────────────────────

    #[test]
    fn annotate_cursor_multibyte() {
        let result = annotate("héllo", &CursorSpec::Cursor(Offset::new(3)));
        assert_eq!(result, "hé|llo");
    }

    #[test]
    fn annotate_selection_multibyte() {
        let result = annotate(
            "héllo",
            &CursorSpec::Selection {
                anchor: Offset::new(0),
                head: Offset::new(3),
            },
        );
        assert_eq!(result, "#[hé|]#llo");
    }

    // ── Annotate: error cases ────────────────────────────────────────

    #[test]
    #[should_panic(expected = "out of bounds")]
    fn annotate_cursor_out_of_bounds() {
        annotate("hello", &CursorSpec::Cursor(Offset::new(100)));
    }

    #[test]
    #[should_panic(expected = "char boundary")]
    fn annotate_cursor_not_char_boundary() {
        // 'é' is 2 bytes at offset 1-2, so offset 2 is mid-codepoint
        annotate("héllo", &CursorSpec::Cursor(Offset::new(2)));
    }

    // ── Round-trip: parse ∘ annotate = identity ──────────────────────

    #[test]
    fn roundtrip_cursor() {
        let spec = CursorSpec::Cursor(Offset::new(3));
        let annotated = annotate("hello", &spec);
        let (text, parsed) = parse(&annotated);
        assert_eq!(text, "hello");
        assert_eq!(parsed, spec);
    }

    #[test]
    fn roundtrip_selection_forward() {
        let spec = CursorSpec::Selection {
            anchor: Offset::new(1),
            head: Offset::new(4),
        };
        let annotated = annotate("hello", &spec);
        let (text, parsed) = parse(&annotated);
        assert_eq!(text, "hello");
        assert_eq!(parsed, spec);
    }

    #[test]
    fn roundtrip_selection_backward() {
        let spec = CursorSpec::Selection {
            anchor: Offset::new(4),
            head: Offset::new(1),
        };
        let annotated = annotate("hello", &spec);
        let (text, parsed) = parse(&annotated);
        assert_eq!(text, "hello");
        assert_eq!(parsed, spec);
    }

    #[test]
    fn roundtrip_selection_collapsed() {
        let spec = CursorSpec::Selection {
            anchor: Offset::new(3),
            head: Offset::new(3),
        };
        let annotated = annotate("hello", &spec);
        let (text, parsed) = parse(&annotated);
        assert_eq!(text, "hello");
        assert_eq!(parsed, spec);
    }

    #[test]
    fn roundtrip_multibyte() {
        let spec = CursorSpec::Cursor(Offset::new(3));
        let annotated = annotate("héllo", &spec);
        let (text, parsed) = parse(&annotated);
        assert_eq!(text, "héllo");
        assert_eq!(parsed, spec);
    }

    #[test]
    fn roundtrip_annotate_parse_canonical_cursor() {
        // annotate(parse(s)) should reproduce canonical form
        let input = "hel|lo";
        let (text, spec) = parse(input);
        let regenerated = annotate(&text, &spec);
        assert_eq!(regenerated, input);
    }

    #[test]
    fn roundtrip_annotate_parse_canonical_selection() {
        let input = "h#[ell|]#o";
        let (text, spec) = parse(input);
        let regenerated = annotate(&text, &spec);
        assert_eq!(regenerated, input);
    }

    #[test]
    fn roundtrip_annotate_parse_canonical_backward() {
        let input = "h#[|ell]#o";
        let (text, spec) = parse(input);
        let regenerated = annotate(&text, &spec);
        assert_eq!(regenerated, input);
    }

    // ── CursorSpec methods ───────────────────────────────────────────

    #[test]
    fn cursor_spec_offset() {
        assert_eq!(CursorSpec::Cursor(Offset::new(5)).offset(), Offset::new(5));
        assert_eq!(
            CursorSpec::Selection {
                anchor: Offset::new(1),
                head: Offset::new(4),
            }
            .offset(),
            Offset::new(4) // head
        );
    }

    #[test]
    fn cursor_spec_selection_range() {
        let cursor = CursorSpec::Cursor(Offset::new(5));
        let sr = cursor.selection_range();
        assert!(sr.is_collapsed());
        assert_eq!(sr.anchor(), Offset::new(5));
        assert_eq!(sr.head(), Offset::new(5));

        let sel = CursorSpec::Selection {
            anchor: Offset::new(1),
            head: Offset::new(4),
        };
        let sr = sel.selection_range();
        assert!(!sr.is_collapsed());
        assert_eq!(sr.anchor(), Offset::new(1));
        assert_eq!(sr.head(), Offset::new(4));
    }

    #[test]
    fn cursor_spec_is_selection() {
        assert!(!CursorSpec::Cursor(Offset::new(0)).is_selection());
        assert!(CursorSpec::Selection {
            anchor: Offset::new(0),
            head: Offset::new(5),
        }
        .is_selection());
    }

    #[test]
    fn cursor_spec_anchor() {
        assert_eq!(CursorSpec::Cursor(Offset::new(0)).anchor(), None);
        assert_eq!(
            CursorSpec::Selection {
                anchor: Offset::new(3),
                head: Offset::new(7),
            }
            .anchor(),
            Some(Offset::new(3))
        );
    }

    // ── Realistic test scenarios ─────────────────────────────────────

    #[test]
    fn scenario_word_motion() {
        // Before: cursor on 'w' in "hello world"
        let (before_text, before_spec) = parse("hello |world");
        assert_eq!(before_text, "hello world");
        assert_eq!(before_spec.offset(), Offset::new(6));

        // After: cursor at end of "world" (e motion)
        let (after_text, after_spec) = parse("hello worl|d");
        assert_eq!(after_text, "hello world");
        assert_eq!(after_spec.offset(), Offset::new(10));
    }

    #[test]
    fn scenario_visual_select_word() {
        // Visual selection of "world" (forward)
        let (text, spec) = parse("hello #[world|]#");
        assert_eq!(text, "hello world");
        assert_eq!(
            spec,
            CursorSpec::Selection {
                anchor: Offset::new(6),
                head: Offset::new(11),
            }
        );
    }

    #[test]
    fn scenario_visual_select_backward() {
        // Backward visual selection of "hello"
        let (text, spec) = parse("#[|hello]# world");
        assert_eq!(text, "hello world");
        assert_eq!(
            spec,
            CursorSpec::Selection {
                anchor: Offset::new(5),
                head: Offset::new(0),
            }
        );
    }

    #[test]
    fn scenario_text_object_inner_word() {
        // "iw" text object on "world" — selected "world"
        let (text, spec) = parse("hello #[world|]#!");
        assert_eq!(text, "hello world!");
        let sr = spec.selection_range();
        assert_eq!(sr.start(), Offset::new(6));
        assert_eq!(sr.end(), Offset::new(11));
    }

    // ── parse_block ─────────────────────────────────────────────────

    #[test]
    fn parse_block_basic_multiline() {
        let (text, spec) = parse_block(
            "
            first line
            second |line
            third line
        ",
        );
        assert_eq!(text, "first line\nsecond line\nthird line");
        assert_eq!(spec, CursorSpec::Cursor(Offset::new(18)));
    }

    #[test]
    fn parse_block_single_line() {
        let (text, spec) = parse_block(
            "
            hel|lo
        ",
        );
        assert_eq!(text, "hello");
        assert_eq!(spec, CursorSpec::Cursor(Offset::new(3)));
    }

    #[test]
    fn parse_block_empty_lines_in_middle() {
        let (text, spec) = parse_block(
            "
            first

            |third
        ",
        );
        assert_eq!(text, "first\n\nthird");
        assert_eq!(spec, CursorSpec::Cursor(Offset::new(7)));
    }

    #[test]
    fn parse_block_selection() {
        let (text, spec) = parse_block(
            "
            hello
            #[wor|]#ld
        ",
        );
        assert_eq!(text, "hello\nworld");
        assert_eq!(
            spec,
            CursorSpec::Selection {
                anchor: Offset::new(6),
                head: Offset::new(9),
            }
        );
    }

    #[test]
    fn parse_block_cursor_at_start() {
        let (text, spec) = parse_block(
            "
            |hello
            world
        ",
        );
        assert_eq!(text, "hello\nworld");
        assert_eq!(spec, CursorSpec::Cursor(Offset::new(0)));
    }

    // ── parse_multi: basic ──────────────────────────────────────

    #[test]
    fn parse_multi_single_cursor_numbered() {
        let (text, spec) = parse_multi("he|1llo");
        assert_eq!(text, "hello");
        assert_eq!(spec.len(), 1);
        assert_eq!(*spec.primary(), CursorSpec::Cursor(Offset::new(2)));
    }

    #[test]
    fn parse_multi_single_cursor_bare() {
        let (text, spec) = parse_multi("he|llo");
        assert_eq!(text, "hello");
        assert_eq!(spec.len(), 1);
        assert_eq!(*spec.primary(), CursorSpec::Cursor(Offset::new(2)));
    }

    #[test]
    fn parse_multi_two_cursors() {
        let (text, spec) = parse_multi("he|1llo wo|2rld");
        assert_eq!(text, "hello world");
        assert_eq!(spec.len(), 2);
        assert_eq!(*spec.get(1).unwrap(), CursorSpec::Cursor(Offset::new(2)));
        assert_eq!(*spec.get(2).unwrap(), CursorSpec::Cursor(Offset::new(8)));
    }

    #[test]
    fn parse_multi_three_cursors() {
        let (text, spec) = parse_multi("|1a|2b|3c");
        assert_eq!(text, "abc");
        assert_eq!(spec.len(), 3);
        assert_eq!(*spec.get(1).unwrap(), CursorSpec::Cursor(Offset::new(0)));
        assert_eq!(*spec.get(2).unwrap(), CursorSpec::Cursor(Offset::new(1)));
        assert_eq!(*spec.get(3).unwrap(), CursorSpec::Cursor(Offset::new(2)));
    }

    #[test]
    fn parse_multi_cursors_out_of_document_order() {
        let (text, spec) = parse_multi("|2world |1hello");
        assert_eq!(text, "world hello");
        assert_eq!(*spec.get(1).unwrap(), CursorSpec::Cursor(Offset::new(6)));
        assert_eq!(*spec.get(2).unwrap(), CursorSpec::Cursor(Offset::new(0)));
    }

    // ── parse_multi: selections ─────────────────────────────────

    #[test]
    fn parse_multi_forward_selection() {
        let (text, spec) = parse_multi("#1[hello|1]# world");
        assert_eq!(text, "hello world");
        assert_eq!(
            *spec.get(1).unwrap(),
            CursorSpec::Selection {
                anchor: Offset::new(0),
                head: Offset::new(5),
            }
        );
    }

    #[test]
    fn parse_multi_backward_selection() {
        let (text, spec) = parse_multi("#1[|1hello]# world");
        assert_eq!(text, "hello world");
        assert_eq!(
            *spec.get(1).unwrap(),
            CursorSpec::Selection {
                anchor: Offset::new(5),
                head: Offset::new(0),
            }
        );
    }

    #[test]
    fn parse_multi_collapsed_selection() {
        let (text, spec) = parse_multi("#1[|1]#hello");
        assert_eq!(text, "hello");
        assert_eq!(
            *spec.get(1).unwrap(),
            CursorSpec::Selection {
                anchor: Offset::new(0),
                head: Offset::new(0),
            }
        );
    }

    #[test]
    fn parse_multi_selection_and_cursor() {
        let (text, spec) = parse_multi("#1[hello|1]# |2world");
        assert_eq!(text, "hello world");
        assert_eq!(spec.len(), 2);
        assert_eq!(
            *spec.get(1).unwrap(),
            CursorSpec::Selection {
                anchor: Offset::new(0),
                head: Offset::new(5),
            }
        );
        assert_eq!(*spec.get(2).unwrap(), CursorSpec::Cursor(Offset::new(6)));
    }

    // ── parse_multi: escaping ───────────────────────────────────

    #[test]
    fn parse_multi_escaped_pipe() {
        let (text, spec) = parse_multi("hello || world |1here");
        assert_eq!(text, "hello | world here");
        assert_eq!(*spec.primary(), CursorSpec::Cursor(Offset::new(14)));
    }

    #[test]
    fn parse_multi_escaped_hash() {
        let (text, spec) = parse_multi("##1[text |1here");
        assert_eq!(text, "#1[text here");
        assert_eq!(*spec.primary(), CursorSpec::Cursor(Offset::new(8)));
    }

    // ── parse_multi: edge cases ─────────────────────────────────

    #[test]
    fn parse_multi_cursor_at_start_and_end() {
        let (text, spec) = parse_multi("|1hello|2");
        assert_eq!(text, "hello");
        assert_eq!(*spec.get(1).unwrap(), CursorSpec::Cursor(Offset::new(0)));
        assert_eq!(*spec.get(2).unwrap(), CursorSpec::Cursor(Offset::new(5)));
    }

    #[test]
    fn parse_multi_empty_text() {
        let (text, spec) = parse_multi("|1");
        assert_eq!(text, "");
        assert_eq!(*spec.primary(), CursorSpec::Cursor(Offset::new(0)));
    }

    #[test]
    fn parse_multi_utf8() {
        let (text, spec) = parse_multi("日|1本|2語");
        assert_eq!(text, "日本語");
        assert_eq!(*spec.get(1).unwrap(), CursorSpec::Cursor(Offset::new(3)));
        assert_eq!(*spec.get(2).unwrap(), CursorSpec::Cursor(Offset::new(6)));
    }

    // ── parse_multi: error cases ────────────────────────────────

    #[test]
    #[should_panic(expected = "no cursor markers")]
    fn parse_multi_no_markers() {
        parse_multi("hello");
    }

    #[test]
    #[should_panic(expected = "defined twice")]
    fn parse_multi_duplicate_cursor() {
        parse_multi("|1hello|1");
    }

    #[test]
    #[should_panic(expected = "contiguous")]
    fn parse_multi_non_contiguous() {
        parse_multi("|1hello|3world");
    }

    #[test]
    #[should_panic(expected = "unclosed selection")]
    fn parse_multi_unclosed_selection() {
        parse_multi("#1[hello");
    }

    #[test]
    #[should_panic(expected = "no |1 head marker")]
    fn parse_multi_selection_no_head() {
        parse_multi("#1[hello]#");
    }

    #[test]
    #[should_panic(expected = "head marker |2")]
    fn parse_multi_selection_wrong_head_number() {
        parse_multi("#1[hello|2]#");
    }

    #[test]
    #[should_panic(expected = "must be immediately")]
    fn parse_multi_head_in_middle() {
        parse_multi("#1[he|1llo]#");
    }

    #[test]
    #[should_panic(expected = "cursor number 0")]
    fn parse_multi_cursor_zero() {
        parse_multi("|0hello");
    }

    #[test]
    fn parse_multi_consecutive_hashes_preserved() {
        let (text, spec) = parse_multi("a##b |1here");
        assert_eq!(text, "a##b here");
        assert_eq!(*spec.primary(), CursorSpec::Cursor(Offset::new(5)));
    }

    #[test]
    fn parse_multi_close_bracket_outside_selection() {
        let (text, spec) = parse_multi("]# |1here");
        assert_eq!(text, "]# here");
        assert_eq!(*spec.primary(), CursorSpec::Cursor(Offset::new(3)));
    }

    #[test]
    fn roundtrip_multi_text_with_consecutive_hashes() {
        let spec = MultiCursorSpec::from_pairs(vec![(1, CursorSpec::Cursor(Offset::new(0)))]);
        let annotated = annotate_multi("a##b", &spec);
        let (text, parsed) = parse_multi(&annotated);
        assert_eq!(text, "a##b");
        assert_eq!(parsed, spec);
    }

    #[test]
    fn roundtrip_multi_text_with_standalone_hash() {
        let spec = MultiCursorSpec::from_pairs(vec![(1, CursorSpec::Cursor(Offset::new(0)))]);
        let annotated = annotate_multi("a#b", &spec);
        let (text, parsed) = parse_multi(&annotated);
        assert_eq!(text, "a#b");
        assert_eq!(parsed, spec);
    }

    // ── annotate_multi ──────────────────────────────────────────

    #[test]
    fn annotate_multi_single_cursor() {
        let spec = MultiCursorSpec::from_pairs(vec![(1, CursorSpec::Cursor(Offset::new(2)))]);
        assert_eq!(annotate_multi("hello", &spec), "he|1llo");
    }

    #[test]
    fn annotate_multi_two_cursors() {
        let spec = MultiCursorSpec::from_pairs(vec![
            (1, CursorSpec::Cursor(Offset::new(2))),
            (2, CursorSpec::Cursor(Offset::new(8))),
        ]);
        assert_eq!(annotate_multi("hello world", &spec), "he|1llo wo|2rld");
    }

    #[test]
    fn annotate_multi_forward_selection() {
        let spec = MultiCursorSpec::from_pairs(vec![(
            1,
            CursorSpec::Selection {
                anchor: Offset::new(0),
                head: Offset::new(5),
            },
        )]);
        assert_eq!(annotate_multi("hello world", &spec), "#1[hello|1]# world");
    }

    #[test]
    fn annotate_multi_backward_selection() {
        let spec = MultiCursorSpec::from_pairs(vec![(
            1,
            CursorSpec::Selection {
                anchor: Offset::new(5),
                head: Offset::new(0),
            },
        )]);
        assert_eq!(annotate_multi("hello world", &spec), "#1[|1hello]# world");
    }

    #[test]
    fn annotate_multi_escapes_literal_pipe() {
        let spec = MultiCursorSpec::from_pairs(vec![(1, CursorSpec::Cursor(Offset::new(0)))]);
        assert_eq!(annotate_multi("a|b", &spec), "|1a||b");
    }

    // ── round-trip: parse_multi ∘ annotate_multi ────────────────

    #[test]
    fn roundtrip_multi_single() {
        let spec = MultiCursorSpec::from_pairs(vec![(1, CursorSpec::Cursor(Offset::new(3)))]);
        let annotated = annotate_multi("hello", &spec);
        let (text, parsed) = parse_multi(&annotated);
        assert_eq!(text, "hello");
        assert_eq!(parsed, spec);
    }

    #[test]
    fn roundtrip_multi_two_cursors() {
        let spec = MultiCursorSpec::from_pairs(vec![
            (1, CursorSpec::Cursor(Offset::new(0))),
            (2, CursorSpec::Cursor(Offset::new(5))),
        ]);
        let annotated = annotate_multi("hello world", &spec);
        let (text, parsed) = parse_multi(&annotated);
        assert_eq!(text, "hello world");
        assert_eq!(parsed, spec);
    }

    #[test]
    fn roundtrip_multi_selection_and_cursor() {
        let spec = MultiCursorSpec::from_pairs(vec![
            (
                1,
                CursorSpec::Selection {
                    anchor: Offset::new(0),
                    head: Offset::new(5),
                },
            ),
            (2, CursorSpec::Cursor(Offset::new(6))),
        ]);
        let annotated = annotate_multi("hello world", &spec);
        let (text, parsed) = parse_multi(&annotated);
        assert_eq!(text, "hello world");
        assert_eq!(parsed, spec);
    }

    #[test]
    fn roundtrip_multi_backward_selection() {
        let spec = MultiCursorSpec::from_pairs(vec![(
            1,
            CursorSpec::Selection {
                anchor: Offset::new(5),
                head: Offset::new(0),
            },
        )]);
        let annotated = annotate_multi("hello", &spec);
        let (text, parsed) = parse_multi(&annotated);
        assert_eq!(text, "hello");
        assert_eq!(parsed, spec);
    }

    #[test]
    fn roundtrip_multi_with_literal_pipe() {
        let spec = MultiCursorSpec::from_pairs(vec![(1, CursorSpec::Cursor(Offset::new(0)))]);
        let annotated = annotate_multi("a|b", &spec);
        let (text, parsed) = parse_multi(&annotated);
        assert_eq!(text, "a|b");
        assert_eq!(parsed, spec);
    }

    #[test]
    fn roundtrip_multi_utf8() {
        let spec = MultiCursorSpec::from_pairs(vec![
            (1, CursorSpec::Cursor(Offset::new(3))),
            (2, CursorSpec::Cursor(Offset::new(6))),
        ]);
        let annotated = annotate_multi("日本語", &spec);
        let (text, parsed) = parse_multi(&annotated);
        assert_eq!(text, "日本語");
        assert_eq!(parsed, spec);
    }

    // ── parse_multi_block ───────────────────────────────────────

    #[test]
    fn parse_multi_block_basic() {
        let (text, spec) = parse_multi_block(
            "
            |1first line
            |2second line
        ",
        );
        assert_eq!(text, "first line\nsecond line");
        assert_eq!(spec.len(), 2);
        assert_eq!(*spec.get(1).unwrap(), CursorSpec::Cursor(Offset::new(0)));
        assert_eq!(*spec.get(2).unwrap(), CursorSpec::Cursor(Offset::new(11)));
    }
}
