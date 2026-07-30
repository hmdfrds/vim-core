//! HIR (High-level IR) lowering pass for Vim regex patterns.
//!
//! Transforms the parser's `VimPatternNode` AST into a normalized `LoweredNode`
//! tree plus `PatternProperties` acceleration hints. The lowered form:
//!
//! - Eliminates redundant representations (`EscapeSequence` → `Literal`,
//!   `CharByCode` → `Literal`, `Class` → `Collection`).
//! - Fuses adjacent `Literal` nodes into `LiteralString`.
//! - Flattens nested `Sequence` nodes.
//! - Removes trivial quantifiers (`{1,1}` → unwrap, `{0,0}` → empty).
//! - Reduces nested quantifiers (Oniguruma-style: `a**` → `a*`, `a++` → `a+`).
//! - Collapses small literal-only `Alternation` into `Collection`.
//!
//! The companion `PatternProperties` struct extends `PatternFeatures` with
//! acceleration hints (anchoring, literal prefix, match length bounds) that
//! downstream NFA/backtracker engines can exploit.

use compact_str::CompactString;
use std::cmp::Ordering;

use super::ir::{
    CharClass, CollectionItem, ColumnSpec, EscapeKind, LineSpec, LookaroundKind, MarkRel,
    VimPatternNode,
};
use super::nfa::CaptureGroup;

pub(crate) mod charset;
pub(crate) mod possessify;
pub(crate) mod start_set;

// `StartSet` is the start-set type used by the accel consumers (start_desc,
// prefilter). `can_match_empty` / `is_zero_width` are accessed directly via
// `hir::start_set::…` by their in-crate callers (e.g. possessify), so they are
// not re-exported here to avoid dead re-export warnings.
pub(crate) use start_set::StartSet;

// ═══════════════════════════════════════════════════════════════════════════════
// LOWERED NODE
// ═══════════════════════════════════════════════════════════════════════════════

/// A normalized IR node consumed by the NFA builder.
///
/// Compared to `VimPatternNode`, this enum has fewer variants — redundant
/// representations are canonicalized during lowering.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LoweredNode {
    /// A single literal character.
    Literal(char),
    /// Fused adjacent literals (≥2 characters).
    LiteralString(CompactString),
    /// `.` — any character except newline.
    AnyChar,
    /// `\_.` — any character including newline.
    AnyCharNl,
    /// `[...]` or `[^...]` — character set (also used for lowered `Class`/`ClassWithNewline`).
    Collection {
        negated: bool,
        items: Vec<CollectionItem>,
        include_newline: bool,
    },
    /// Sequence of atoms matched left-to-right.
    Sequence(Vec<LoweredNode>),
    /// Alternation: matches any one branch.
    Alternation(Vec<LoweredNode>),
    /// `\&` — branch-and: all branches must match at the same position.
    BranchAnd(Vec<LoweredNode>),
    /// Grouped subexpression.
    Group {
        inner: Box<LoweredNode>,
        capturing: bool,
        /// 1-based capture identity, assigned by `assign_capture_groups` during
        /// `lower()`. `Some` iff `capturing` (after the pass); `None` for
        /// non-capturing groups and transiently before the pass runs.
        group: Option<CaptureGroup>,
    },
    /// Repetition quantifier.
    Quantifier {
        node: Box<LoweredNode>,
        min: u32,
        max: Option<u32>,
        greedy: bool,
    },
    /// `\1`..`\9` — back-reference.
    BackReference(CaptureGroup),
    /// Lookaround / atomic group.
    Lookaround {
        inner: Box<LoweredNode>,
        kind: LookaroundKind,
        limit: Option<u32>,
    },
    /// `~` — last substitute string.
    LastSubstitute,
    /// `\%[atoms]` — optionally matched sequence.
    OptionalSequence(Vec<LoweredNode>),
    // Zero-width assertions:
    /// `^`
    StartOfLine,
    /// `$`
    EndOfLine,
    /// `\_^` — start-of-line at any pattern position.
    AnywhereStartOfLine,
    /// `\_$` — end-of-line at any pattern position.
    AnywhereEndOfLine,
    /// `\%^`
    StartOfFile,
    /// `\%$`
    EndOfFile,
    /// `\<`
    WordBoundaryStart,
    /// `\>`
    WordBoundaryEnd,
    /// `\zs`
    SetMatchStart,
    /// `\ze`
    SetMatchEnd,
    /// `\%#`
    CursorPosition,
    /// `\%V`
    VisualArea,
    /// `\%l`
    AtLine(LineSpec),
    /// `\%c`
    AtColumn(ColumnSpec),
    /// `\%v`
    AtVirtualColumn(ColumnSpec),
    /// `\%'m`
    AtMark { mark: char, rel: MarkRel },
}

// ═══════════════════════════════════════════════════════════════════════════════
// PATTERN PROPERTIES — Sub-Structs
// ═══════════════════════════════════════════════════════════════════════════════

/// Feature flags describing what constructs a pattern uses.
/// Superset of the public `PatternFeatures` (those are copied from here).
#[derive(Debug, Clone, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent orthogonal booleans"
)]
pub(crate) struct FeatureFlags {
    pub has_backreferences: bool,
    pub has_lookaround: bool,
    pub has_atomic: bool,
    pub has_buffer_position: bool,
    pub has_match_override: bool,
    pub has_last_substitute: bool,
    pub has_visual_area_prefix: bool,
    /// Pattern contains `\n`, `\_x`, or `\_.` — can match across line boundaries.
    pub has_multiline: bool,
    /// Pattern contains `\&` (branch-and operator).
    pub has_branch_and: bool,
    /// Pattern contains zero-width assertions (^, $, \<, \>).
    #[allow(
        dead_code,
        reason = "informational flag; DFA eligibility uses has_look_ahead_assertions"
    )]
    pub has_zero_width_assertions: bool,
    /// Pattern contains look-ahead assertions ($, \<, \>) that inspect
    /// the character at the current position rather than behind it.
    /// `^` (StartOfLine) is excluded: it only inspects the previous character.
    pub has_look_ahead_assertions: bool,
    /// Pattern contains a non-greedy (lazy) quantifier.
    pub has_lazy_quantifier: bool,
    /// Pattern contains a true alternation (not a literal-collapsed Collection).
    pub has_alternation: bool,
    pub capture_count: u8,
}

impl FeatureFlags {
    /// Merge another set of flags into self (logical OR for booleans,
    /// saturating add for capture_count).
    pub(crate) fn merge_from(&mut self, other: &FeatureFlags) {
        self.has_backreferences |= other.has_backreferences;
        self.has_lookaround |= other.has_lookaround;
        self.has_atomic |= other.has_atomic;
        self.has_buffer_position |= other.has_buffer_position;
        self.has_match_override |= other.has_match_override;
        self.has_last_substitute |= other.has_last_substitute;
        self.has_visual_area_prefix |= other.has_visual_area_prefix;
        self.has_multiline |= other.has_multiline;
        self.has_branch_and |= other.has_branch_and;
        self.has_zero_width_assertions |= other.has_zero_width_assertions;
        self.has_look_ahead_assertions |= other.has_look_ahead_assertions;
        self.has_lazy_quantifier |= other.has_lazy_quantifier;
        self.has_alternation |= other.has_alternation;
        self.capture_count = self.capture_count.saturating_add(other.capture_count);
    }

    /// Returns true if any feature requiring the backtracker engine is set.
    pub(crate) const fn needs_backtracker(&self) -> bool {
        self.has_backreferences || self.has_last_substitute || self.has_atomic
    }

    /// Returns true if any feature disqualifying the lazy DFA is set.
    ///
    /// Note: `has_look_ahead_assertions` is intentionally excluded here because
    /// its DFA-eligibility is gated by `DFA_LOOK_AHEAD_ENABLED` in the caller.
    pub(crate) const fn disqualifies_dfa(&self) -> bool {
        self.has_backreferences
            || self.has_last_substitute
            || self.has_atomic
            || self.has_lookaround
            || self.has_branch_and
            || self.has_buffer_position
            || self.has_lazy_quantifier
    }
}

/// Acceleration hints computed during HIR lowering.
#[derive(Debug, Clone, Default)]
pub(crate) struct AccelHints {
    #[allow(
        dead_code,
        reason = "will be used by anchored fast-path (needs line vs file anchor distinction)"
    )]
    pub is_anchored_start: bool,
    pub is_anchored_start_of_file: bool,
    #[allow(dead_code, reason = "will be used by backward search optimization")]
    pub is_anchored_end: bool,
    pub is_anchored_end_of_file: bool,
    pub is_literal: bool,
    pub literal_prefix: Option<CompactString>,
    pub literal_suffix: Option<CompactString>,
    pub required_line: Option<u32>,
    pub required_line_range: Option<(Ordering, u32)>,
}

/// Match length bounds (computed from quantifier analysis).
#[derive(Debug, Clone, Default)]
pub(crate) struct MatchBounds {
    pub minimum_match_len: usize,
    pub maximum_match_len: Option<usize>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// PATTERN PROPERTIES — Composite
// ═══════════════════════════════════════════════════════════════════════════════

/// Expanded pattern analysis — feature flags plus acceleration hints.
#[derive(Debug, Clone, Default)]
pub(crate) struct PatternProperties {
    pub features: FeatureFlags,
    pub accel_hints: AccelHints,
    pub match_bounds: MatchBounds,
}

/// Forwarding accessor methods on `PatternProperties` to avoid massive
/// mechanical diffs at every call site. These delegate to sub-structs.
impl PatternProperties {
    // Feature flag accessors
    #[inline]
    pub fn has_backreferences(&self) -> bool {
        self.features.has_backreferences
    }
    #[inline]
    pub fn has_lookaround(&self) -> bool {
        self.features.has_lookaround
    }
    #[inline]
    pub fn has_atomic(&self) -> bool {
        self.features.has_atomic
    }
    #[inline]
    #[allow(
        dead_code,
        reason = "forwarding accessor over FeatureFlags; read by onepass::build::is_onepass_eligible to reject patterns that pin buffer positions"
    )]
    pub fn has_buffer_position(&self) -> bool {
        self.features.has_buffer_position
    }
    #[inline]
    pub fn has_match_override(&self) -> bool {
        self.features.has_match_override
    }
    #[inline]
    pub fn has_last_substitute(&self) -> bool {
        self.features.has_last_substitute
    }
    #[inline]
    #[allow(
        dead_code,
        reason = "forwarding accessor over FeatureFlags; read by onepass::build::is_onepass_eligible to reject \\& branch-concat patterns"
    )]
    pub fn has_branch_and(&self) -> bool {
        self.features.has_branch_and
    }
    #[inline]
    pub fn has_look_ahead_assertions(&self) -> bool {
        self.features.has_look_ahead_assertions
    }
    #[inline]
    #[allow(
        dead_code,
        reason = "forwarding accessor over FeatureFlags; read by engine::compile to exclude lazy quantifiers from the reverse-search strategies"
    )]
    pub fn has_lazy_quantifier(&self) -> bool {
        self.features.has_lazy_quantifier
    }
    #[inline]
    pub fn has_alternation(&self) -> bool {
        self.features.has_alternation
    }
    #[inline]
    pub fn capture_count(&self) -> u8 {
        self.features.capture_count
    }
    #[inline]
    #[allow(
        dead_code,
        reason = "public API accessor — used by vim-core's pattern_might_span_lines"
    )]
    pub fn has_multiline(&self) -> bool {
        self.features.has_multiline
    }
    #[inline]
    pub fn has_visual_area_prefix(&self) -> bool {
        self.features.has_visual_area_prefix
    }

    // Accel hint accessors
    #[inline]
    pub fn is_literal(&self) -> bool {
        self.accel_hints.is_literal
    }
    #[inline]
    pub fn is_anchored_start_of_file(&self) -> bool {
        self.accel_hints.is_anchored_start_of_file
    }
    #[inline]
    pub fn is_anchored_end_of_file(&self) -> bool {
        self.accel_hints.is_anchored_end_of_file
    }
    #[inline]
    pub fn literal_prefix(&self) -> Option<&CompactString> {
        self.accel_hints.literal_prefix.as_ref()
    }
    #[inline]
    #[allow(dead_code, reason = "accessor used by strategy implementations")]
    pub fn literal_suffix(&self) -> Option<&CompactString> {
        self.accel_hints.literal_suffix.as_ref()
    }
    #[inline]
    pub fn required_line(&self) -> Option<u32> {
        self.accel_hints.required_line
    }
    #[inline]
    pub fn required_line_range(&self) -> Option<(Ordering, u32)> {
        self.accel_hints.required_line_range
    }

    // Match bounds accessors
    #[inline]
    pub fn minimum_match_len(&self) -> usize {
        self.match_bounds.minimum_match_len
    }
    #[inline]
    pub fn maximum_match_len(&self) -> Option<usize> {
        self.match_bounds.maximum_match_len
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PER-NODE PROPERTIES (internal)
// ═══════════════════════════════════════════════════════════════════════════════

/// Properties computed bottom-up for a single node.
#[derive(Debug, Clone)]
struct NodeProps {
    min_len: usize,
    max_len: Option<usize>,
    prefix: Option<CompactString>,
    is_literal: bool,
    flags: FeatureFlags,
}

impl Default for NodeProps {
    fn default() -> Self {
        Self {
            min_len: 0,
            max_len: Some(0),
            prefix: None,
            is_literal: false,
            flags: FeatureFlags::default(),
        }
    }
}

impl NodeProps {
    /// Merge child feature flags into self.
    fn merge_flags(&mut self, child: &NodeProps) {
        self.flags.merge_from(&child.flags);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ESCAPE HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Convert an `EscapeKind` to its corresponding character.
const fn escape_to_char(kind: EscapeKind) -> char {
    match kind {
        EscapeKind::Newline => '\n',
        EscapeKind::Tab => '\t',
        EscapeKind::Return => '\r',
        EscapeKind::Escape => '\x1B',
        EscapeKind::Backspace => '\x08',
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// HAS_MULTILINE DERIVATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Derive `has_multiline` by walking the lowered node tree.
///
/// Returns `true` if any node can match a newline character.
/// Single source of truth — replaces fragile per-arm flag setting.
fn derive_has_multiline(node: &LoweredNode) -> bool {
    match node {
        LoweredNode::Literal('\n') => true,
        LoweredNode::LiteralString(s) => s.contains('\n'),
        LoweredNode::AnyCharNl => true,
        LoweredNode::Collection {
            items,
            include_newline,
            ..
        } => {
            *include_newline
                || items.iter().any(|item| {
                    matches!(item, CollectionItem::Newline | CollectionItem::Single('\n'))
                })
        }
        LoweredNode::Sequence(children)
        | LoweredNode::Alternation(children)
        | LoweredNode::BranchAnd(children)
        | LoweredNode::OptionalSequence(children) => children.iter().any(derive_has_multiline),
        LoweredNode::Quantifier { node: inner, .. }
        | LoweredNode::Group { inner, .. }
        | LoweredNode::Lookaround { inner, .. } => derive_has_multiline(inner),
        _ => false,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PUBLIC ENTRY POINT
// ═══════════════════════════════════════════════════════════════════════════════

/// Lower a `VimPatternNode` tree into `(LoweredNode, PatternProperties)`.
pub(crate) fn lower(node: &VimPatternNode) -> (LoweredNode, PatternProperties) {
    let (mut lowered, props) = lower_node(node);
    let mut next_group: u8 = 1;
    assign_capture_groups(&mut lowered, &mut next_group);
    debug_assert_eq!(
        u32::from(next_group.saturating_sub(1)),
        u32::from(props.flags.capture_count),
        "capture numbering disagrees with lowering capture_count"
    );
    let has_multiline = derive_has_multiline(&lowered);

    // Compute top-level anchoring and visual-area prefix.
    let (
        is_anchored_start,
        is_anchored_start_of_file,
        is_anchored_end,
        is_anchored_end_of_file,
        has_visual_area_prefix,
    ) = compute_anchoring(&lowered);

    let props = PatternProperties {
        features: FeatureFlags {
            has_multiline,
            has_visual_area_prefix,
            ..props.flags
        },
        accel_hints: AccelHints {
            is_anchored_start,
            is_anchored_start_of_file,
            is_anchored_end,
            is_anchored_end_of_file,
            is_literal: props.is_literal,
            literal_prefix: props.prefix,
            literal_suffix: compute_literal_suffix(&lowered),
            required_line: compute_required_line(&lowered),
            required_line_range: compute_required_line_range(&lowered),
        },
        match_bounds: MatchBounds {
            minimum_match_len: props.min_len,
            maximum_match_len: props.max_len,
        },
    };

    (lowered, props)
}

/// Assign 1-based capture identities to capturing groups in opening-paren
/// (pre-order) order. Single source of truth for capture numbering: the NFA
/// builder reads `Group.group` and never derives numbers itself, so a group
/// rebuilt N times by a quantifier keeps one stable identity.
fn assign_capture_groups(node: &mut LoweredNode, next: &mut u8) {
    match node {
        LoweredNode::Group {
            inner,
            capturing,
            group,
        } => {
            if *capturing {
                // Pre-order: number THIS group before descending so that
                // `\(\(a\)\)` numbers outer=1, inner=2, matching Vim.
                *group = Some(CaptureGroup::from_one_based(*next));
                *next = next.saturating_add(1);
            }
            assign_capture_groups(inner, next);
        }
        LoweredNode::Quantifier { node: inner, .. } => assign_capture_groups(inner, next),
        LoweredNode::Lookaround { inner, .. } => assign_capture_groups(inner, next),
        LoweredNode::Sequence(children)
        | LoweredNode::Alternation(children)
        | LoweredNode::BranchAnd(children)
        | LoweredNode::OptionalSequence(children) => {
            for child in children.iter_mut() {
                assign_capture_groups(child, next);
            }
        }
        // Leaves: no nested capturing groups.
        LoweredNode::Literal(_)
        | LoweredNode::LiteralString(_)
        | LoweredNode::AnyChar
        | LoweredNode::AnyCharNl
        | LoweredNode::Collection { .. }
        | LoweredNode::BackReference(_)
        | LoweredNode::LastSubstitute
        | LoweredNode::StartOfLine
        | LoweredNode::EndOfLine
        | LoweredNode::AnywhereStartOfLine
        | LoweredNode::AnywhereEndOfLine
        | LoweredNode::StartOfFile
        | LoweredNode::EndOfFile
        | LoweredNode::WordBoundaryStart
        | LoweredNode::WordBoundaryEnd
        | LoweredNode::SetMatchStart
        | LoweredNode::SetMatchEnd
        | LoweredNode::CursorPosition
        | LoweredNode::VisualArea
        | LoweredNode::AtLine(_)
        | LoweredNode::AtColumn(_)
        | LoweredNode::AtVirtualColumn(_)
        | LoweredNode::AtMark { .. } => {}
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// RECURSIVE LOWERING
// ═══════════════════════════════════════════════════════════════════════════════

/// Lower a single node, returning the lowered form and per-node properties.
#[allow(clippy::too_many_lines)]
fn lower_node(node: &VimPatternNode) -> (LoweredNode, NodeProps) {
    match node {
        // ─── Literals ────────────────────────────────────────────────
        VimPatternNode::Literal(ch) => {
            let len = ch.len_utf8();
            let props = NodeProps {
                min_len: len,
                max_len: Some(len),
                prefix: Some(CompactString::from(ch.to_string())),
                is_literal: true,
                ..NodeProps::default()
            };
            (LoweredNode::Literal(*ch), props)
        }

        // ─── Normalize: EscapeSequence → Literal ────────────────────
        VimPatternNode::EscapeSequence(kind) => {
            let ch = escape_to_char(*kind);
            let len = ch.len_utf8();
            let props = NodeProps {
                min_len: len,
                max_len: Some(len),
                prefix: Some(CompactString::from(ch.to_string())),
                is_literal: true,
                ..NodeProps::default()
            };
            (LoweredNode::Literal(ch), props)
        }

        // ─── Normalize: CharByCode → Literal ────────────────────────
        VimPatternNode::CharByCode(ch) => {
            let len = ch.len_utf8();
            let props = NodeProps {
                min_len: len,
                max_len: Some(len),
                prefix: Some(CompactString::from(ch.to_string())),
                is_literal: true,
                ..NodeProps::default()
            };
            (LoweredNode::Literal(*ch), props)
        }

        // ─── Wildcards ──────────────────────────────────────────────
        VimPatternNode::AnyChar => {
            let props = NodeProps {
                min_len: 1,
                max_len: Some(4),
                ..NodeProps::default()
            };
            (LoweredNode::AnyChar, props)
        }
        VimPatternNode::AnyCharNl => {
            let props = NodeProps {
                min_len: 1,
                max_len: Some(4),
                ..NodeProps::default()
            };
            (LoweredNode::AnyCharNl, props)
        }

        // ─── Normalize: Class → Collection ──────────────────────────
        VimPatternNode::Class(class) => {
            let props = NodeProps {
                min_len: 1,
                max_len: Some(4),
                ..NodeProps::default()
            };
            (
                LoweredNode::Collection {
                    negated: false,
                    items: vec![CollectionItem::Class(*class)],
                    include_newline: false,
                },
                props,
            )
        }
        VimPatternNode::ClassWithNewline(class) => {
            let props = NodeProps {
                min_len: 1,
                max_len: Some(4),
                ..NodeProps::default()
            };
            (
                LoweredNode::Collection {
                    negated: false,
                    items: vec![CollectionItem::Class(*class)],
                    include_newline: true,
                },
                props,
            )
        }

        // ─── Collection (pass through) ──────────────────────────────
        VimPatternNode::Collection {
            negated,
            items,
            include_newline,
        } => {
            let props = NodeProps {
                min_len: 1,
                max_len: Some(4),
                ..NodeProps::default()
            };
            (
                LoweredNode::Collection {
                    negated: *negated,
                    items: items.clone(),
                    include_newline: *include_newline,
                },
                props,
            )
        }

        // ─── Sequence (flatten + fuse literals) ─────────────────────
        VimPatternNode::Sequence(children) => lower_sequence(children),

        // ─── Alternation (optimize small literal-only) ──────────────
        VimPatternNode::Alternation(branches) => lower_alternation(branches),

        // ─── BranchAnd ─────────────────────────────────────────────
        VimPatternNode::BranchAnd(branches) => lower_branch_and(branches),

        // ─── Group ──────────────────────────────────────────────────
        VimPatternNode::Group { inner, capturing } => {
            let (lowered_inner, mut props) = lower_node(inner);
            if *capturing {
                props.flags.capture_count = props.flags.capture_count.saturating_add(1);
            }
            (
                LoweredNode::Group {
                    inner: Box::new(lowered_inner),
                    capturing: *capturing,
                    group: None,
                },
                props,
            )
        }

        // ─── Quantifier (trivial elimination) ───────────────────────
        VimPatternNode::Quantifier {
            node: inner,
            min,
            max,
            greedy,
        } => lower_quantifier(inner, *min, *max, *greedy),

        // ─── BackReference ──────────────────────────────────────────
        VimPatternNode::BackReference(n) => {
            let props = NodeProps {
                min_len: 0,
                max_len: None,
                flags: FeatureFlags {
                    has_backreferences: true,
                    ..FeatureFlags::default()
                },
                ..NodeProps::default()
            };
            (
                LoweredNode::BackReference(CaptureGroup::from_one_based(*n)),
                props,
            )
        }

        // ─── Lookaround / Atomic ────────────────────────────────────
        VimPatternNode::Lookaround { inner, kind, limit } => {
            let (lowered_inner, child_props) = lower_node(inner);
            let mut flags = child_props.flags.clone();
            flags.has_lookaround = true;
            if *kind == LookaroundKind::Atomic {
                flags.has_atomic = true;
            }
            let props = NodeProps {
                min_len: 0,
                max_len: Some(0),
                flags,
                ..NodeProps::default()
            };
            (
                LoweredNode::Lookaround {
                    inner: Box::new(lowered_inner),
                    kind: *kind,
                    limit: *limit,
                },
                props,
            )
        }

        // ─── LastSubstitute ─────────────────────────────────────────
        VimPatternNode::LastSubstitute => {
            let props = NodeProps {
                min_len: 0,
                max_len: None,
                flags: FeatureFlags {
                    has_last_substitute: true,
                    ..FeatureFlags::default()
                },
                ..NodeProps::default()
            };
            (LoweredNode::LastSubstitute, props)
        }

        // ─── OptionalSequence ───────────────────────────────────────
        VimPatternNode::OptionalSequence(children) => {
            let mut lowered_children = Vec::with_capacity(children.len());
            let mut merged_props = NodeProps::default();
            let mut total_max: Option<usize> = Some(0);
            for child in children {
                let (lc, cp) = lower_node(child);
                merged_props.merge_flags(&cp);
                total_max = match (total_max, cp.max_len) {
                    (Some(sum), Some(m)) => Some(sum.saturating_add(m)),
                    _ => None,
                };
                lowered_children.push(lc);
            }
            // OptionalSequence can match empty (min=0) or up to all atoms
            merged_props.min_len = 0;
            merged_props.max_len = total_max;
            (
                LoweredNode::OptionalSequence(lowered_children),
                merged_props,
            )
        }

        // ─── Zero-width assertions ──────────────────────────────────
        VimPatternNode::StartOfLine => (
            LoweredNode::StartOfLine,
            NodeProps {
                flags: FeatureFlags {
                    has_zero_width_assertions: true,
                    ..FeatureFlags::default()
                },
                ..NodeProps::default()
            },
        ),
        VimPatternNode::EndOfLine => (
            LoweredNode::EndOfLine,
            NodeProps {
                flags: FeatureFlags {
                    has_zero_width_assertions: true,
                    has_look_ahead_assertions: true,
                    ..FeatureFlags::default()
                },
                ..NodeProps::default()
            },
        ),
        VimPatternNode::AnywhereStartOfLine => (
            LoweredNode::AnywhereStartOfLine,
            NodeProps {
                flags: FeatureFlags {
                    has_zero_width_assertions: true,
                    ..FeatureFlags::default()
                },
                ..NodeProps::default()
            },
        ),
        VimPatternNode::AnywhereEndOfLine => (
            LoweredNode::AnywhereEndOfLine,
            NodeProps {
                flags: FeatureFlags {
                    has_zero_width_assertions: true,
                    has_look_ahead_assertions: true,
                    ..FeatureFlags::default()
                },
                ..NodeProps::default()
            },
        ),
        VimPatternNode::StartOfFile => (
            LoweredNode::StartOfFile,
            NodeProps {
                flags: FeatureFlags {
                    has_buffer_position: true,
                    ..FeatureFlags::default()
                },
                ..NodeProps::default()
            },
        ),
        VimPatternNode::EndOfFile => (
            LoweredNode::EndOfFile,
            NodeProps {
                flags: FeatureFlags {
                    has_buffer_position: true,
                    ..FeatureFlags::default()
                },
                ..NodeProps::default()
            },
        ),
        VimPatternNode::WordBoundaryStart => (
            LoweredNode::WordBoundaryStart,
            NodeProps {
                flags: FeatureFlags {
                    has_zero_width_assertions: true,
                    has_look_ahead_assertions: true,
                    ..FeatureFlags::default()
                },
                ..NodeProps::default()
            },
        ),
        VimPatternNode::WordBoundaryEnd => (
            LoweredNode::WordBoundaryEnd,
            NodeProps {
                flags: FeatureFlags {
                    has_zero_width_assertions: true,
                    has_look_ahead_assertions: true,
                    ..FeatureFlags::default()
                },
                ..NodeProps::default()
            },
        ),
        VimPatternNode::SetMatchStart => {
            let props = NodeProps {
                flags: FeatureFlags {
                    has_match_override: true,
                    ..FeatureFlags::default()
                },
                ..NodeProps::default()
            };
            (LoweredNode::SetMatchStart, props)
        }
        VimPatternNode::SetMatchEnd => {
            let props = NodeProps {
                flags: FeatureFlags {
                    has_match_override: true,
                    ..FeatureFlags::default()
                },
                ..NodeProps::default()
            };
            (LoweredNode::SetMatchEnd, props)
        }
        VimPatternNode::CursorPosition => {
            let props = NodeProps {
                flags: FeatureFlags {
                    has_buffer_position: true,
                    ..FeatureFlags::default()
                },
                ..NodeProps::default()
            };
            (LoweredNode::CursorPosition, props)
        }
        VimPatternNode::VisualArea => {
            let props = NodeProps {
                flags: FeatureFlags {
                    has_buffer_position: true,
                    ..FeatureFlags::default()
                },
                ..NodeProps::default()
            };
            (LoweredNode::VisualArea, props)
        }
        VimPatternNode::AtLine(spec) => {
            let props = NodeProps {
                flags: FeatureFlags {
                    has_buffer_position: true,
                    ..FeatureFlags::default()
                },
                ..NodeProps::default()
            };
            (LoweredNode::AtLine(*spec), props)
        }
        VimPatternNode::AtColumn(spec) => {
            let props = NodeProps {
                flags: FeatureFlags {
                    has_buffer_position: true,
                    ..FeatureFlags::default()
                },
                ..NodeProps::default()
            };
            (LoweredNode::AtColumn(*spec), props)
        }
        VimPatternNode::AtVirtualColumn(spec) => {
            let props = NodeProps {
                flags: FeatureFlags {
                    has_buffer_position: true,
                    ..FeatureFlags::default()
                },
                ..NodeProps::default()
            };
            (LoweredNode::AtVirtualColumn(*spec), props)
        }
        VimPatternNode::AtMark { mark, rel } => {
            let props = NodeProps {
                flags: FeatureFlags {
                    has_buffer_position: true,
                    ..FeatureFlags::default()
                },
                ..NodeProps::default()
            };
            (
                LoweredNode::AtMark {
                    mark: *mark,
                    rel: *rel,
                },
                props,
            )
        }

        // ─── AnyComposing (\\%C) ──────────────────────────────────────
        VimPatternNode::AnyComposing => {
            let props = NodeProps {
                min_len: 1,
                max_len: Some(4),
                ..NodeProps::default()
            };
            (
                LoweredNode::Collection {
                    negated: false,
                    items: vec![CollectionItem::Class(CharClass::Composing)],
                    include_newline: false,
                },
                props,
            )
        }

        // ErrorPlaceholder: zero-width always-match, used during multi-error
        // recovery. Lowered as an empty sequence (matches empty string at any
        // position). This node should never appear in a successful parse.
        VimPatternNode::ErrorPlaceholder => {
            (LoweredNode::Sequence(Vec::new()), NodeProps::default())
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SEQUENCE LOWERING
// ═══════════════════════════════════════════════════════════════════════════════

/// Lower a sequence of children into a flat list of `(LoweredNode, NodeProps)` pairs,
/// recursively flattening nested `VimPatternNode::Sequence` nodes so that per-element
/// props are preserved without needing re-computation.
fn lower_sequence_elements(children: &[VimPatternNode]) -> Vec<(LoweredNode, NodeProps)> {
    let mut flat: Vec<(LoweredNode, NodeProps)> = Vec::with_capacity(children.len());
    for child in children {
        match child {
            VimPatternNode::Sequence(grandchildren) => {
                // Recursively flatten nested sequences — this preserves
                // per-element props from `lower_node` without re-computation.
                flat.extend(lower_sequence_elements(grandchildren));
            }
            _ => {
                flat.push(lower_node(child));
            }
        }
    }
    flat
}

/// Lower a sequence: flatten nested sequences, fuse adjacent literals.
fn lower_sequence(children: &[VimPatternNode]) -> (LoweredNode, NodeProps) {
    // Phase 1: lower all children, flatten nested Sequences.
    let flat = lower_sequence_elements(children);

    // Phase 2: fuse adjacent Literal/LiteralString nodes.
    let mut fused: Vec<(LoweredNode, NodeProps)> = Vec::with_capacity(flat.len());
    let mut i = 0;
    while i < flat.len() {
        if matches!(
            &flat[i].0,
            LoweredNode::Literal(_) | LoweredNode::LiteralString(_)
        ) {
            let mut chars = CompactString::new("");
            let start_i = i;
            while i < flat.len() {
                match &flat[i].0 {
                    LoweredNode::Literal(ch) => {
                        chars.push(*ch);
                        i += 1;
                    }
                    LoweredNode::LiteralString(s) => {
                        chars.push_str(s);
                        i += 1;
                    }
                    _ => break,
                }
            }
            if i - start_i == 1 && matches!(&flat[start_i].0, LoweredNode::Literal(_)) {
                // Single Literal — keep as-is.
                fused.push(flat[start_i].clone());
            } else {
                // Multiple adjacent literals or a LiteralString involved — fuse.
                // Merge feature flags from all constituent elements so flags
                // like has_multiline (from \n) are not lost during fusion.
                let len = chars.len();
                let mut flags = FeatureFlags::default();
                for (_, ref elem_props) in &flat[start_i..i] {
                    flags.merge_from(&elem_props.flags);
                }
                let props = NodeProps {
                    min_len: len,
                    max_len: Some(len),
                    prefix: Some(chars.clone()),
                    is_literal: true,
                    flags,
                };
                fused.push((LoweredNode::LiteralString(chars), props));
            }
        } else {
            fused.push(flat[i].clone());
            i += 1;
        }
    }

    // Phase 3: combine properties.
    if fused.is_empty() {
        return (
            LoweredNode::Sequence(vec![]),
            NodeProps {
                is_literal: true,
                ..NodeProps::default()
            },
        );
    }

    if fused.len() == 1 {
        return fused.into_iter().next().expect("checked non-empty");
    }

    let mut merged_props = NodeProps {
        min_len: 0,
        max_len: Some(0),
        is_literal: true,
        ..NodeProps::default()
    };

    let mut prefix = CompactString::new("");
    let mut prefix_complete = false;

    for (_, cp) in &fused {
        merged_props.merge_flags(cp);
        merged_props.min_len = merged_props.min_len.saturating_add(cp.min_len);
        merged_props.max_len = match (merged_props.max_len, cp.max_len) {
            (Some(a), Some(b)) => Some(a.saturating_add(b)),
            _ => None,
        };
        merged_props.is_literal &= cp.is_literal;

        if !prefix_complete {
            if let Some(ref child_prefix) = cp.prefix {
                if cp.is_literal {
                    prefix.push_str(child_prefix);
                } else {
                    prefix.push_str(child_prefix);
                    prefix_complete = true;
                }
            } else {
                prefix_complete = true;
            }
        }
    }

    merged_props.prefix = if prefix.is_empty() {
        None
    } else {
        Some(prefix)
    };

    let nodes: Vec<LoweredNode> = fused.into_iter().map(|(n, _)| n).collect();
    (LoweredNode::Sequence(nodes), merged_props)
}

// ═══════════════════════════════════════════════════════════════════════════════
// ALTERNATION LOWERING
// ═══════════════════════════════════════════════════════════════════════════════

/// Lower an alternation: optimize small literal-only alternations to Collection.
fn lower_alternation(branches: &[VimPatternNode]) -> (LoweredNode, NodeProps) {
    let lowered: Vec<(LoweredNode, NodeProps)> = branches.iter().map(lower_node).collect();

    // Optimization: if all branches are single Literal nodes, collapse to Collection.
    let all_literals: Option<Vec<char>> = lowered
        .iter()
        .map(|(n, _)| match n {
            LoweredNode::Literal(ch) => Some(*ch),
            _ => None,
        })
        .collect();

    if let Some(chars) = all_literals {
        if !chars.is_empty() {
            let items: Vec<CollectionItem> =
                chars.iter().map(|ch| CollectionItem::Single(*ch)).collect();
            let mut merged_flags = FeatureFlags::default();
            for (_, child_props) in &lowered {
                merged_flags.merge_from(&child_props.flags);
            }
            let props = NodeProps {
                min_len: 1,
                max_len: Some(4),
                flags: merged_flags,
                ..NodeProps::default()
            };
            return (
                LoweredNode::Collection {
                    negated: false,
                    items,
                    include_newline: false,
                },
                props,
            );
        }
    }

    // General case: keep as Alternation.
    let mut merged_props = NodeProps {
        min_len: usize::MAX,
        max_len: Some(0),
        is_literal: true,
        flags: FeatureFlags {
            has_alternation: true,
            ..FeatureFlags::default()
        },
        ..NodeProps::default()
    };

    // Collect prefixes for common prefix detection.
    let mut prefixes: Vec<Option<&CompactString>> = Vec::with_capacity(lowered.len());

    for (_, cp) in &lowered {
        merged_props.merge_flags(cp);
        merged_props.min_len = merged_props.min_len.min(cp.min_len);
        merged_props.max_len = match (merged_props.max_len, cp.max_len) {
            (Some(a), Some(b)) => Some(a.max(b)),
            _ => None,
        };
        merged_props.is_literal = false; // alternation is never purely literal
        prefixes.push(cp.prefix.as_ref());
    }

    // If no branches, min should be 0.
    if lowered.is_empty() {
        merged_props.min_len = 0;
    }

    // Common prefix extraction.
    merged_props.prefix = compute_common_prefix(&prefixes);

    let nodes: Vec<LoweredNode> = lowered.into_iter().map(|(n, _)| n).collect();
    (LoweredNode::Alternation(nodes), merged_props)
}

// ═══════════════════════════════════════════════════════════════════════════════
// BRANCH-AND LOWERING
// ═══════════════════════════════════════════════════════════════════════════════

/// Lower a branch-and: each branch is lowered independently.
/// Properties: min/max come from the LAST branch (determines extent).
/// Feature flags are merged from all branches.
/// Note: `is_literal` is always false — the pattern has lookahead semantics.
fn lower_branch_and(branches: &[VimPatternNode]) -> (LoweredNode, NodeProps) {
    let lowered: Vec<(LoweredNode, NodeProps)> = branches.iter().map(lower_node).collect();

    let mut merged_props = NodeProps {
        flags: FeatureFlags {
            has_branch_and: true,
            ..FeatureFlags::default()
        },
        ..NodeProps::default()
    };

    for (i, (_, cp)) in lowered.iter().enumerate() {
        merged_props.merge_flags(cp);
        if i == lowered.len() - 1 {
            // Last branch determines match extent
            merged_props.min_len = cp.min_len;
            merged_props.max_len = cp.max_len;
            merged_props.prefix = cp.prefix.clone();
        }
    }

    // BranchAnd is NEVER a pure literal — it has lookahead semantics
    // that require engine evaluation, not just substring search.
    merged_props.is_literal = false;

    let nodes: Vec<LoweredNode> = lowered.into_iter().map(|(n, _)| n).collect();
    (LoweredNode::BranchAnd(nodes), merged_props)
}

/// Find the longest common prefix among a set of optional prefixes.
fn compute_common_prefix(prefixes: &[Option<&CompactString>]) -> Option<CompactString> {
    if prefixes.is_empty() {
        return None;
    }

    // All branches must have a prefix for a common prefix to exist.
    let strings: Vec<&str> = prefixes
        .iter()
        .map(|p| p.map(|s| s.as_str()))
        .collect::<Option<Vec<_>>>()?;

    if strings.is_empty() {
        return None;
    }

    let first = strings[0];
    let mut common_len = first.len();

    for s in &strings[1..] {
        common_len = common_len.min(s.len());
        for (i, (a, b)) in first.bytes().zip(s.bytes()).enumerate() {
            if a != b {
                common_len = common_len.min(i);
                break;
            }
        }
    }

    // Round down to a char boundary to avoid slicing mid-codepoint.
    while common_len > 0 && !first.is_char_boundary(common_len) {
        common_len -= 1;
    }

    if common_len == 0 {
        None
    } else {
        Some(CompactString::from(&first[..common_len]))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// QUANTIFIER LOWERING
// ═══════════════════════════════════════════════════════════════════════════════

/// Try to reduce a nested quantifier pair into a single quantifier.
///
/// This implements Oniguruma-style nested quantifier reduction. Given an
/// outer quantifier `(outer_min, outer_max)` wrapping an inner quantifier
/// `(inner_min, inner_max)`, we reduce to a single flat quantifier when
/// both are the same greediness and both are "simple" (i.e. `?`, `*`, or `+`).
///
/// Reduction table (all same-greediness):
///
/// | Inner \ Outer |  `?`(0,1)  |  `*`(0,N)  |  `+`(1,N)  |
/// |:-------------:|:----------:|:----------:|:----------:|
/// | `?`(0,1)      | `?`(0,1)   | `*`(0,N)   | `*`(0,N)   |
/// | `*`(0,N)      | `*`(0,N)   | `*`(0,N)   | `*`(0,N)   |
/// | `+`(1,N)      | `+`(1,N)   | `*`(0,N)   | `+`(1,N)   |
///
/// Additionally, exact×exact: `{n}{m}` → `{n*m}` when both have the same
/// greediness.
///
/// Returns `Some((new_min, new_max))` if reduction applies, `None` otherwise.
fn try_reduce_nested_quantifier(
    inner_min: u32,
    inner_max: Option<u32>,
    outer_min: u32,
    outer_max: Option<u32>,
) -> Option<(u32, Option<u32>)> {
    // Classify each quantifier as a simple kind.
    enum Kind {
        Optional,   // (0, Some(1)) — `?`
        Star,       // (0, None)    — `*`
        Plus,       // (1, None)    — `+`
        Exact(u32), // (n, Some(n)) — `{n}` where n >= 2
    }

    let classify = |min: u32, max: Option<u32>| -> Option<Kind> {
        match (min, max) {
            (0, Some(1)) => Some(Kind::Optional),
            (0, None) => Some(Kind::Star),
            (1, None) => Some(Kind::Plus),
            (n, Some(m)) if n == m && n >= 2 => Some(Kind::Exact(n)),
            _ => None,
        }
    };

    let inner_kind = classify(inner_min, inner_max)?;
    let outer_kind = classify(outer_min, outer_max)?;

    match (&inner_kind, &outer_kind) {
        // ── Simple × Simple (3×3 table) ──────────────────────────────
        //
        // ??: (0,1)(0,1) → can match 0 or 1 of (0 or 1) = 0 or 1 → ?
        (Kind::Optional, Kind::Optional) => Some((0, Some(1))),
        // ?*: (0,1)(0,N) → can match 0..N of (0 or 1) = 0..N → *
        (Kind::Optional, Kind::Star) => Some((0, None)),
        // ?+: (0,1)(1,N) → must match 1..N of (0 or 1) = 0..N → *
        //     (inner can be empty, so effective min is 0)
        (Kind::Optional, Kind::Plus) => Some((0, None)),
        // *?: (0,N)(0,1) → can match 0 or 1 of (0..N) = 0..N → *
        (Kind::Star, Kind::Optional) => Some((0, None)),
        // **: (0,N)(0,N) → can match 0..N of (0..N) = 0..N → *
        (Kind::Star, Kind::Star) => Some((0, None)),
        // *+: (0,N)(1,N) → must match 1..N of (0..N) = 0..N → *
        //     (inner can be empty, so effective min is 0)
        (Kind::Star, Kind::Plus) => Some((0, None)),
        // +?: (1,N)(0,1) → can match 0 or 1 of (1..N) = 0..N → *
        (Kind::Plus, Kind::Optional) => Some((0, None)),
        // +*: (1,N)(0,N) → can match 0..N of (1..N) = 0..N → *
        (Kind::Plus, Kind::Star) => Some((0, None)),
        // ++: (1,N)(1,N) → must match 1..N of (1..N) = 1..N → +
        (Kind::Plus, Kind::Plus) => Some((1, None)),

        // ── Exact × Exact: {n}{m} → {n*m} ───────────────────────────
        (Kind::Exact(n), Kind::Exact(m)) => {
            let product = n.checked_mul(*m)?;
            Some((product, Some(product)))
        }

        // ── Everything else: no reduction ────────────────────────────
        _ => None,
    }
}

/// Lower a quantifier: eliminate trivial `{1,1}` and `{0,0}`.
fn lower_quantifier(
    inner: &VimPatternNode,
    min: u32,
    max: Option<u32>,
    greedy: bool,
) -> (LoweredNode, NodeProps) {
    // {0,0} → empty sequence
    if min == 0 && max == Some(0) {
        return (
            LoweredNode::Sequence(vec![]),
            NodeProps {
                is_literal: true,
                ..NodeProps::default()
            },
        );
    }

    let (lowered_inner, child_props) = lower_node(inner);

    // {1,1} → unwrap to inner node
    if min == 1 && max == Some(1) {
        return (lowered_inner, child_props);
    }

    // Nested quantifier reduction: if the inner node is itself a quantifier
    // with the same greediness, try to reduce the pair into a single flat
    // quantifier (Oniguruma-style 6×6 table).
    if let LoweredNode::Quantifier {
        node: innermost_body,
        min: inner_min,
        max: inner_max,
        greedy: inner_greedy,
    } = &lowered_inner
    {
        if *inner_greedy == greedy {
            if let Some((new_min, new_max)) =
                try_reduce_nested_quantifier(*inner_min, *inner_max, min, max)
            {
                // Recurse through lower_quantifier to handle further trivial
                // cases (e.g. if the reduction produces {1,1} or {0,0}).
                //
                // child_props.min_len already includes the inner quantifier factor
                // (inner_min * body_min). Divide out the inner factor to avoid
                // double-counting.
                let body_min_len = if *inner_min > 0 {
                    child_props.min_len / (*inner_min as usize)
                } else {
                    0
                };
                let body_max_len = match (*inner_max, child_props.max_len) {
                    (Some(m), Some(cm)) if m > 0 => Some(cm / (m as usize)),
                    _ => child_props.max_len,
                };
                let reduced_min_len = (new_min as usize).saturating_mul(body_min_len);
                let reduced_max_len = match (new_max, body_max_len) {
                    (Some(m), Some(cm)) => Some((m as usize).saturating_mul(cm)),
                    _ => None,
                };
                let reduced_prefix = if new_min > 0 {
                    child_props.prefix.clone()
                } else {
                    None
                };

                let mut reduced_props = NodeProps {
                    min_len: reduced_min_len,
                    max_len: reduced_max_len,
                    prefix: reduced_prefix,
                    is_literal: false,
                    flags: FeatureFlags {
                        has_lazy_quantifier: !greedy,
                        ..FeatureFlags::default()
                    },
                };
                reduced_props.merge_flags(&child_props);

                return (
                    LoweredNode::Quantifier {
                        node: innermost_body.clone(),
                        min: new_min,
                        max: new_max,
                        greedy,
                    },
                    reduced_props,
                );
            }
        }
    }

    // General case
    let min_len = (min as usize).saturating_mul(child_props.min_len);
    let max_len = match (max, child_props.max_len) {
        (Some(m), Some(cm)) => Some((m as usize).saturating_mul(cm)),
        _ => None,
    };
    let prefix = if min > 0 {
        child_props.prefix.clone()
    } else {
        None
    };

    let mut props = NodeProps {
        min_len,
        max_len,
        prefix,
        is_literal: false,
        flags: FeatureFlags {
            has_lazy_quantifier: !greedy,
            ..FeatureFlags::default()
        },
    };
    props.merge_flags(&child_props);

    (
        LoweredNode::Quantifier {
            node: Box::new(lowered_inner),
            min,
            max,
            greedy,
        },
        props,
    )
}

// ═══════════════════════════════════════════════════════════════════════════════
// TOP-LEVEL PROPERTY HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Compute anchoring and visual-area prefix flags from the top-level node.
fn compute_anchoring(node: &LoweredNode) -> (bool, bool, bool, bool, bool) {
    let elements = match node {
        LoweredNode::Sequence(children) => children.as_slice(),
        _ => std::slice::from_ref(node),
    };

    if elements.is_empty() {
        return (false, false, false, false, false);
    }

    let is_anchored_start = matches!(
        elements.first(),
        Some(LoweredNode::StartOfLine | LoweredNode::StartOfFile)
    );
    let is_anchored_start_of_file = matches!(elements.first(), Some(LoweredNode::StartOfFile));
    let is_anchored_end = matches!(
        elements.last(),
        Some(LoweredNode::EndOfLine | LoweredNode::EndOfFile)
    );
    let is_anchored_end_of_file = matches!(elements.last(), Some(LoweredNode::EndOfFile));
    let has_visual_area_prefix = matches!(elements.first(), Some(LoweredNode::VisualArea));

    (
        is_anchored_start,
        is_anchored_start_of_file,
        is_anchored_end,
        is_anchored_end_of_file,
        has_visual_area_prefix,
    )
}

/// Extract `required_line` from top-level `AtLine(Exact(n))`.
fn compute_required_line(node: &LoweredNode) -> Option<u32> {
    let elements = match node {
        LoweredNode::Sequence(children) => children.as_slice(),
        _ => std::slice::from_ref(node),
    };
    for elem in elements {
        if let LoweredNode::AtLine(LineSpec::Exact(n)) = elem {
            return Some(*n);
        }
    }
    None
}

/// Extract `required_line_range` from top-level `AtLine(Before|After)`.
fn compute_required_line_range(node: &LoweredNode) -> Option<(Ordering, u32)> {
    let elements = match node {
        LoweredNode::Sequence(children) => children.as_slice(),
        _ => std::slice::from_ref(node),
    };
    for elem in elements {
        match elem {
            LoweredNode::AtLine(LineSpec::Before(n)) => return Some((Ordering::Less, *n)),
            LoweredNode::AtLine(LineSpec::After(n)) => return Some((Ordering::Greater, *n)),
            _ => {}
        }
    }
    None
}

/// Extract a literal suffix from trailing literal nodes, skipping zero-width
/// end-assertions (`$`, `\%$`, `\>`, `\zs`, `\ze`).
fn compute_literal_suffix(node: &LoweredNode) -> Option<CompactString> {
    match node {
        LoweredNode::Literal(ch) => Some(CompactString::from(ch.to_string())),
        LoweredNode::LiteralString(s) => Some(s.clone()),
        LoweredNode::Group { inner, .. } => compute_literal_suffix(inner),
        LoweredNode::Sequence(children) if !children.is_empty() => {
            let mut suffix = CompactString::new("");
            for child in children.iter().rev() {
                match child {
                    LoweredNode::EndOfLine
                    | LoweredNode::AnywhereEndOfLine
                    | LoweredNode::EndOfFile
                    | LoweredNode::WordBoundaryEnd
                    | LoweredNode::SetMatchEnd
                    | LoweredNode::SetMatchStart => continue,
                    LoweredNode::Literal(ch) => {
                        let mut new = CompactString::from(ch.to_string());
                        new.push_str(&suffix);
                        suffix = new;
                    }
                    LoweredNode::LiteralString(s) => {
                        let mut new = s.clone();
                        new.push_str(&suffix);
                        suffix = new;
                    }
                    _ => break,
                }
            }
            if suffix.is_empty() {
                None
            } else {
                Some(suffix)
            }
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "../tests/hir.rs"]
mod tests;
