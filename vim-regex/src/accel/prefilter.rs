//! Prefilter trait and 8 concrete variants for fast position skipping:
//! `SingleByte`, `DualByte`, `TriByte`, `SubstringPrefilter`, `ByteSet`,
//! `PackedPrefilter`, `NewlineStartPrefilter`, and `AcPrefilter` (Aho-Corasick).
//!
//! Each variant wraps a different search strategy, from SIMD-accelerated
//! `memchr` for single/dual/tri bytes to linear-scan fallbacks for byte sets.

use crate::common::BACKWARD_SCAN_WINDOW;
use crate::hir::{LoweredNode, PatternProperties, StartSet};
use crate::ir::CaseMode;

// ═══════════════════════════════════════════════════════════════════════════════
// PREFILTER TRAIT
// ═══════════════════════════════════════════════════════════════════════════════

/// A prefilter that quickly finds candidate match positions.
///
/// Implementations may use SIMD-accelerated byte searches (`memchr`),
/// substring finders, or byte-set scans to skip positions that cannot
/// possibly start a match.
pub(crate) trait Prefilter: std::fmt::Debug + Send + Sync {
    /// Find the next candidate position at or after `start` in `text`.
    fn find_next(&self, text: &str, start: usize) -> Option<usize>;
    /// Find the previous candidate position strictly before `end` in `text`.
    fn find_prev(&self, text: &str, end: usize) -> Option<usize>;
    /// Whether this prefilter uses SIMD or other hardware acceleration.
    fn is_fast(&self) -> bool;
}

// ═══════════════════════════════════════════════════════════════════════════════
// SINGLE BYTE
// ═══════════════════════════════════════════════════════════════════════════════

/// Prefilter that searches for a single byte using `memchr`.
#[derive(Debug)]
struct SingleByte(u8);

impl Prefilter for SingleByte {
    fn find_next(&self, text: &str, start: usize) -> Option<usize> {
        let haystack = text.as_bytes().get(start..)?;
        memchr::memchr(self.0, haystack).map(|pos| start + pos)
    }

    fn find_prev(&self, text: &str, end: usize) -> Option<usize> {
        let limit = end.min(text.len());
        let haystack = text.as_bytes().get(..limit)?;
        memchr::memrchr(self.0, haystack)
    }

    fn is_fast(&self) -> bool {
        true
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// DUAL BYTE
// ═══════════════════════════════════════════════════════════════════════════════

/// Prefilter that searches for either of two bytes using `memchr2`.
#[derive(Debug)]
struct DualByte(u8, u8);

impl Prefilter for DualByte {
    fn find_next(&self, text: &str, start: usize) -> Option<usize> {
        let haystack = text.as_bytes().get(start..)?;
        memchr::memchr2(self.0, self.1, haystack).map(|pos| start + pos)
    }

    fn find_prev(&self, text: &str, end: usize) -> Option<usize> {
        let limit = end.min(text.len());
        let haystack = text.as_bytes().get(..limit)?;
        memchr::memrchr2(self.0, self.1, haystack)
    }

    fn is_fast(&self) -> bool {
        true
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TRI BYTE
// ═══════════════════════════════════════════════════════════════════════════════

/// Prefilter that searches for any of three bytes using `memchr3`.
#[derive(Debug)]
struct TriByte(u8, u8, u8);

impl Prefilter for TriByte {
    fn find_next(&self, text: &str, start: usize) -> Option<usize> {
        let haystack = text.as_bytes().get(start..)?;
        memchr::memchr3(self.0, self.1, self.2, haystack).map(|pos| start + pos)
    }

    fn find_prev(&self, text: &str, end: usize) -> Option<usize> {
        let limit = end.min(text.len());
        let haystack = text.as_bytes().get(..limit)?;
        memchr::memrchr3(self.0, self.1, self.2, haystack)
    }

    fn is_fast(&self) -> bool {
        true
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SUBSTRING PREFILTER
// ═══════════════════════════════════════════════════════════════════════════════

/// Prefilter that searches for a multi-byte substring using `memchr::memmem`.
#[derive(Debug)]
struct SubstringPrefilter {
    forward: memchr::memmem::Finder<'static>,
    backward: memchr::memmem::FinderRev<'static>,
}

impl SubstringPrefilter {
    fn new(needle: &[u8]) -> Self {
        Self {
            forward: memchr::memmem::Finder::new(needle).into_owned(),
            backward: memchr::memmem::FinderRev::new(needle).into_owned(),
        }
    }
}

impl Prefilter for SubstringPrefilter {
    fn find_next(&self, text: &str, start: usize) -> Option<usize> {
        let haystack = text.as_bytes().get(start..)?;
        self.forward.find(haystack).map(|pos| start + pos)
    }

    fn find_prev(&self, text: &str, end: usize) -> Option<usize> {
        let limit = end.min(text.len());
        let haystack = text.as_bytes().get(..limit)?;
        self.backward.rfind(haystack)
    }

    fn is_fast(&self) -> bool {
        true
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// BYTE SET
// ═══════════════════════════════════════════════════════════════════════════════

/// Prefilter using a 256-entry boolean table for linear scan.
#[derive(Debug)]
struct ByteSet([bool; 256]);

impl Prefilter for ByteSet {
    fn find_next(&self, text: &str, start: usize) -> Option<usize> {
        let bytes = text.as_bytes();
        (start..bytes.len()).find(|&i| self.0[bytes[i] as usize])
    }

    fn find_prev(&self, text: &str, end: usize) -> Option<usize> {
        let bytes = text.as_bytes();
        let limit = end.min(bytes.len());
        (0..limit).rev().find(|&i| self.0[bytes[i] as usize])
    }

    fn is_fast(&self) -> bool {
        false
    }
}

#[allow(
    dead_code,
    reason = "covers set_bytes and try_promote_to_packed: bytes_to_prefilter already tries PackedPrefilter::new before falling back to ByteSet, so an existing ByteSet is never promoted after the fact; both are exercised by this file's tests only"
)]
impl ByteSet {
    /// Iterate over all byte values that are set in this table.
    fn set_bytes(&self) -> impl Iterator<Item = u8> + '_ {
        self.0
            .iter()
            .enumerate()
            .filter(|(_, &set)| set)
            .map(|(i, _)| i as u8)
    }

    /// Try to promote this ByteSet to a SIMD-accelerated PackedPrefilter.
    ///
    /// Succeeds when:
    /// - 1-64 bytes are set (packed searcher limit is 128, but we cap at 64
    ///   to avoid heuristic pattern-limit rejections)
    /// - The platform supports SIMD (x86_64/aarch64 with SSE/NEON)
    ///
    /// Each set byte becomes a single-byte needle in the packed searcher,
    /// which internally uses the Teddy SIMD algorithm.
    fn try_promote_to_packed(&self) -> Option<PackedPrefilter> {
        let needles: Vec<Vec<u8>> = self.set_bytes().map(|b| vec![b]).collect();
        if needles.is_empty() || needles.len() > 64 {
            return None;
        }
        PackedPrefilter::new(&needles)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PACKED PREFILTER (SIMD-accelerated via aho_corasick::packed)
// ═══════════════════════════════════════════════════════════════════════════════

/// SIMD-accelerated multi-pattern prefilter via Teddy/Rabin-Karp.
///
/// Wraps `aho_corasick::packed::Searcher` which internally uses the Teddy
/// SIMD algorithm on x86_64/aarch64, falling back to Rabin-Karp for short
/// haystacks. Returns `None` on construction for platforms without SIMD
/// support or when too many patterns are provided (>128).
#[derive(Debug, Clone)]
struct PackedPrefilter(aho_corasick::packed::Searcher);

impl PackedPrefilter {
    /// Build a packed prefilter from a set of byte-string needles.
    ///
    /// Returns `None` if:
    /// - `needles` is empty
    /// - The platform lacks SIMD support
    /// - Too many patterns (>128, aho-corasick internal limit)
    fn new(needles: &[Vec<u8>]) -> Option<Self> {
        if needles.is_empty() {
            return None;
        }
        let searcher = aho_corasick::packed::Searcher::new(needles.iter())?;
        Some(Self(searcher))
    }
}

impl Prefilter for PackedPrefilter {
    fn find_next(&self, text: &str, start: usize) -> Option<usize> {
        let haystack = text.as_bytes().get(start..)?;
        self.0.find(haystack).map(|m| start + m.start())
    }

    fn find_prev(&self, text: &str, end: usize) -> Option<usize> {
        let limit = end.min(text.len());
        if limit == 0 {
            return None;
        }
        // Limit backward scan to avoid quadratic behavior on large texts.
        let window_start = limit.saturating_sub(BACKWARD_SCAN_WINDOW);
        let haystack = &text.as_bytes()[window_start..limit];
        let mut last_pos: Option<usize> = None;
        for mat in self.0.find_iter(haystack) {
            last_pos = Some(window_start + mat.start());
        }
        last_pos
    }

    fn is_fast(&self) -> bool {
        true
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// BYTE FREQUENCY TABLE (for rare-byte selection)
// ═══════════════════════════════════════════════════════════════════════════════

/// Approximate byte frequency in English text. 0 = rarest, 255 = most common.
///
/// Derived from letter frequency analysis of English prose, extended to all
/// 256 byte values. Rare bytes (control chars, high non-ASCII) get low scores.
/// Common letters (e, t, a, o, i, n, s) and space get high scores.
///
/// This is used to select the rarest bytes from a candidate set for memchr
/// prefilters, maximizing skip distance in typical text.
#[rustfmt::skip]
const BYTE_FREQUENCY: [u8; 256] = [
    //  0x0_  0x1_  0x2_  0x3_  0x4_  0x5_  0x6_  0x7_  0x8_  0x9_  0xA_  0xB_  0xC_  0xD_  0xE_  0xF_
        0,    0,    0,    0,    0,    0,    0,    0,    0,   35,   90,    0,    0,   35,    0,    0, // 0x0_
        0,    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,    0, // 0x1_
      255,   40,   50,   20,   15,   18,   20,   45,   45,   45,   25,   22,   75,   55,   80,   35, // 0x2_ (space ! " # $ % & ' ( ) * + , - . /)
      100,  100,   95,   90,   85,   85,   80,   75,   75,   70,   55,   40,   25,   30,   25,   30, // 0x3_ (0-9 : ; < = > ?)
       20,  120,   85,  100,  105,  130,   80,   75,   95,  120,   30,   40,   95,   90,  110,  115, // 0x4_ (@ A-O)
       80,   15,  105,  120,  130,   75,   40,   65,   25,   60,   15,   30,   20,   30,   15,   55, // 0x5_ (P-Z [ \ ] ^ _)
       15,  200,  100,  130,  150,  220,   85,   85,  155,  185,   15,   30,  145,  110,  175,  190, // 0x6_ (` a-o)
       95,   12,  170,  180,  195,  120,   40,   85,   25,   75,   12,   20,   15,   20,   20,    0, // 0x7_ (p-z { | } ~ DEL)
        5,    5,    5,    5,    5,    5,    5,    5,    5,    5,    5,    5,    5,    5,    5,    5, // 0x8_
        5,    5,    5,    5,    5,    5,    5,    5,    5,    5,    5,    5,    5,    5,    5,    5, // 0x9_
        5,    5,    5,    5,    5,    5,    5,    5,    5,    5,    5,    5,    5,    5,    5,    5, // 0xA_
        5,    5,    5,    5,    5,    5,    5,    5,    5,    5,    5,    5,    5,    5,    5,    5, // 0xB_
        8,    8,    8,    8,    8,    8,    8,    8,    8,    8,    8,    8,    8,    8,    8,    8, // 0xC_ (UTF-8 lead bytes)
        8,    8,    8,    8,    8,    8,    8,    8,    8,    8,    8,    8,    8,    8,    8,    8, // 0xD_
       10,   10,   10,   10,   10,   10,   10,   10,   10,   10,   10,   10,   10,   10,   10,   10, // 0xE_
       10,   10,   10,   10,   10,    3,    3,    3,    3,    3,    3,    3,    3,    3,    3,    3, // 0xF_
];

/// Select the `count` rarest bytes from `candidates` by frequency.
///
/// Returns a `SmallVec` with at most `count` elements, sorted by ascending
/// frequency (rarest first). If `candidates` has fewer elements than `count`,
/// returns all of them.
fn select_rarest(candidates: &[u8], count: usize) -> smallvec::SmallVec<[u8; 3]> {
    let mut sorted: smallvec::SmallVec<[u8; 64]> = candidates.iter().copied().collect();
    sorted.sort_by_key(|&b| BYTE_FREQUENCY[b as usize]);
    sorted.truncate(count);
    sorted.into_iter().collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// NEWLINE START PREFILTER
// ═══════════════════════════════════════════════════════════════════════════════

/// Prefilter that jumps to line starts using memchr for '\n'.
///
/// For `^`-anchored patterns, match positions can only occur at:
/// - Position 0 (start of text)
/// - Position immediately after a '\n'
///
/// This prefilter uses SIMD-accelerated newline search to skip entire
/// lines that cannot contain a match start.
#[derive(Debug)]
struct NewlineStartPrefilter;

impl Prefilter for NewlineStartPrefilter {
    fn find_next(&self, text: &str, start: usize) -> Option<usize> {
        // Position 0 is always a valid start-of-line
        if start == 0 {
            return Some(0);
        }
        // Check if `start` is already at a line start (prev char was '\n')
        if start <= text.len() && text.as_bytes().get(start.wrapping_sub(1)).copied() == Some(b'\n')
        {
            return Some(start);
        }
        // Find next '\n' and return position after it
        let haystack = text.as_bytes().get(start..)?;
        memchr::memchr(b'\n', haystack)
            .map(|pos| start + pos + 1)
            .filter(|&p| p <= text.len())
    }

    fn find_prev(&self, text: &str, end: usize) -> Option<usize> {
        let limit = end.min(text.len());
        if limit == 0 {
            return Some(0);
        }
        // Search for '\n' in [0..limit-1], return pos+1
        let haystack = text.as_bytes().get(..limit.saturating_sub(1))?;
        match memchr::memrchr(b'\n', haystack) {
            Some(pos) => Some(pos + 1),
            None => Some(0), // Start of text is a valid line start
        }
    }

    fn is_fast(&self) -> bool {
        true
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// BUILD PREFILTER
// ═══════════════════════════════════════════════════════════════════════════════

/// Build a case-insensitive prefilter for an ASCII prefix.
///
/// For single-byte prefixes: `DualByte(lower, upper)` for letters, `SingleByte` otherwise.
/// For multi-byte prefixes (up to 4 bytes, max 64 case variants): generates all case
/// variants and uses Aho-Corasick multi-pattern search.
/// Returns `None` for non-ASCII prefixes or when variant explosion is too large.
pub(crate) fn build_case_insensitive_prefilter(
    properties: &crate::hir::PatternProperties,
) -> Option<Box<dyn Prefilter>> {
    let prefix = properties.literal_prefix()?;
    let bytes = prefix.as_bytes();

    if bytes.is_empty() || !bytes.iter().all(|b| b.is_ascii()) {
        return None;
    }

    // Single byte: existing fast path.
    if bytes.len() == 1 {
        let b = bytes[0];
        if b.is_ascii_alphabetic() {
            return Some(Box::new(DualByte(
                b.to_ascii_lowercase(),
                b.to_ascii_uppercase(),
            )));
        } else {
            return Some(Box::new(SingleByte(b)));
        }
    }

    // Multi-byte: generate case variants and use Aho-Corasick.
    // Limit: prefix up to 4 bytes with max 64 variants (2^6 = 64).
    let effective_len = bytes.len().min(4);
    let effective_prefix = &bytes[..effective_len];

    if let Some(variants) = generate_ascii_case_variants(effective_prefix, 64) {
        let compact_variants: Vec<compact_str::CompactString> = variants
            .iter()
            .map(|v| compact_str::CompactString::from(String::from_utf8_lossy(v).as_ref()))
            .collect();
        if let Some(ac_pf) = super::aho_corasick::AcPrefilter::new(&compact_variants, false) {
            return Some(Box::new(ac_pf));
        }
    }

    // Fallback: use just the first byte.
    let b = bytes[0];
    if b.is_ascii_alphabetic() {
        Some(Box::new(DualByte(
            b.to_ascii_lowercase(),
            b.to_ascii_uppercase(),
        )))
    } else {
        Some(Box::new(SingleByte(b)))
    }
}

/// Generate all case variants of an ASCII byte prefix.
///
/// For a prefix of length N with K alphabetic characters, generates 2^K variants.
/// Returns None if the variant count exceeds `max_variants`.
fn generate_ascii_case_variants(prefix: &[u8], max_variants: usize) -> Option<Vec<Vec<u8>>> {
    let alpha_count = prefix.iter().filter(|b| b.is_ascii_alphabetic()).count();
    let variant_count = 1usize.checked_shl(alpha_count as u32)?;
    if variant_count > max_variants {
        return None;
    }

    let mut variants = Vec::with_capacity(variant_count);
    let mut current = prefix.to_vec();

    for mask in 0..variant_count {
        let mut alpha_idx = 0;
        for (i, &b) in prefix.iter().enumerate() {
            if b.is_ascii_alphabetic() {
                current[i] = if (mask >> alpha_idx) & 1 == 0 {
                    b.to_ascii_lowercase()
                } else {
                    b.to_ascii_uppercase()
                };
                alpha_idx += 1;
            } else {
                current[i] = b;
            }
        }
        variants.push(current.clone());
    }

    Some(variants)
}

/// Build the best available prefilter for a pattern.
///
/// Priority cascade:
/// 1. Literal prefix from `PatternProperties` → SingleByte or SubstringPrefilter
/// 2. Alternation first-bytes from root node → memchr variants or ByteSet
/// 3. `StartSet`-derived start-byte set (ASCII-only) → memchr variants or ByteSet
/// 4. Newline skip for `^`-anchored patterns
/// 5. None if nothing works
pub(crate) fn build_prefilter(
    properties: &PatternProperties,
    root: &LoweredNode,
    case_mode: CaseMode,
) -> Option<Box<dyn Prefilter>> {
    // 1. Try literal prefix.
    if let Some(prefix) = properties.literal_prefix() {
        let bytes = prefix.as_bytes();
        if bytes.len() == 1 {
            return Some(Box::new(SingleByte(bytes[0])));
        }
        if bytes.len() > 1 {
            return Some(Box::new(SubstringPrefilter::new(bytes)));
        }
    }

    // Try AC multi-pattern prefilter for literal alternations.
    if let Some(literals) = super::aho_corasick::extract_ac_literals(root) {
        if super::aho_corasick::should_use_ac(&literals) {
            if let Some(ac_pf) = super::aho_corasick::AcPrefilter::new(&literals, false) {
                return Some(Box::new(ac_pf));
            }
        }
    }

    // 2. Try alternation first-bytes.
    if let Some(first_bytes) = extract_alternation_first_bytes(root) {
        return Some(bytes_to_prefilter(&first_bytes));
    }

    // 3. Start-byte set prefilter, derived from the sound `StartSet`.
    //    `StartSet::Anywhere` (nullable / unknown-first / universal start) means
    //    no sound prefilter is possible, so we skip — this subsumes the old
    //    `node_always_consumes` guard. Only build a byte prefilter from a
    //    pure-ASCII set: `to_byte_vec` emits ASCII bytes only, so a set with any
    //    non-ASCII char would be under-approximated (multibyte soundness, §5) —
    //    those are left to the sound start bitmap, which sets UTF-8 lead bytes.
    if let StartSet::Constrained(char_set) = StartSet::of(root, case_mode) {
        if !char_set.is_universal() && !char_set.has_non_ascii() {
            let bytes = char_set.to_byte_vec();
            if !bytes.is_empty() && bytes.len() <= 64 {
                return Some(bytes_to_prefilter(&bytes));
            }
        }
    }

    // 4. For ^ patterns with no other prefilter, use newline skip.
    if properties.accel_hints.is_anchored_start {
        return Some(Box::new(NewlineStartPrefilter));
    }

    // 5. Nothing useful.
    None
}

/// Convert a set of bytes into the best available prefilter variant.
///
/// Priority:
/// 1. 1 byte -> SingleByte (memchr)
/// 2. 2 bytes -> DualByte (memchr2), with rarest-byte selection
/// 3. 3 bytes -> TriByte (memchr3), with rarest-byte selection
/// 4. 4-64 bytes -> try PackedPrefilter (SIMD Teddy), fall back to ByteSet
/// 5. >64 bytes -> ByteSet
///
/// For 2-3 byte cases, rare-byte selection ensures the rarest bytes from the
/// candidate set are used for memchr, maximizing average skip distance.
fn bytes_to_prefilter(bytes: &[u8]) -> Box<dyn Prefilter> {
    match bytes.len() {
        0 => unreachable!("bytes_to_prefilter called with empty slice"),
        1 => Box::new(SingleByte(bytes[0])),
        2 => {
            let rarest = select_rarest(bytes, 2);
            Box::new(DualByte(rarest[0], rarest[1]))
        }
        3 => {
            let rarest = select_rarest(bytes, 3);
            Box::new(TriByte(rarest[0], rarest[1], rarest[2]))
        }
        n if n <= 64 => {
            // Try SIMD-accelerated packed search first.
            let needles: Vec<Vec<u8>> = bytes.iter().map(|&b| vec![b]).collect();
            if let Some(packed) = PackedPrefilter::new(&needles) {
                return Box::new(packed);
            }
            // Fallback: ByteSet linear scan.
            let mut table = [false; 256];
            for &b in bytes {
                table[b as usize] = true;
            }
            Box::new(ByteSet(table))
        }
        _ => {
            let mut table = [false; 256];
            for &b in bytes {
                table[b as usize] = true;
            }
            Box::new(ByteSet(table))
        }
    }
}

/// Extract the first byte of each branch when the root is an `Alternation`.
///
/// Returns `None` if the root is not an `Alternation`, or if any branch
/// cannot determine a definite first byte.
fn extract_alternation_first_bytes(node: &LoweredNode) -> Option<Vec<u8>> {
    let branches = match node {
        LoweredNode::Alternation(branches) => branches,
        _ => return None,
    };

    let mut bytes = Vec::with_capacity(branches.len());
    for branch in branches {
        let byte = first_byte_of_node(branch)?;
        if !bytes.contains(&byte) {
            bytes.push(byte);
        }
    }

    if bytes.is_empty() {
        None
    } else {
        Some(bytes)
    }
}

/// Determine the first byte that a node must match.
///
/// Returns `None` for nodes that can match multiple first bytes or
/// are too complex to analyze.
fn first_byte_of_node(node: &LoweredNode) -> Option<u8> {
    match node {
        LoweredNode::Literal(ch) => {
            let mut buf = [0u8; 4];
            ch.encode_utf8(&mut buf);
            Some(buf[0])
        }
        LoweredNode::LiteralString(s) => s.as_bytes().first().copied(),
        LoweredNode::Sequence(children) => {
            for child in children {
                if is_zero_width_prefilter(child) {
                    continue;
                }
                return first_byte_of_node(child);
            }
            None
        }
        LoweredNode::Group { inner, .. } => first_byte_of_node(inner),
        LoweredNode::Quantifier { min, node, .. } if *min > 0 => first_byte_of_node(node),
        _ => None,
    }
}

/// Check whether a `LoweredNode` is a zero-width assertion.
fn is_zero_width_prefilter(node: &LoweredNode) -> bool {
    matches!(
        node,
        LoweredNode::StartOfLine
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
            | LoweredNode::AtMark { .. }
    )
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use compact_str::CompactString;

    use super::*;
    use crate::ir::CollectionItem;

    // ── SingleByte ─────────────────────────────────────────────────────

    #[test]
    fn single_byte_find_next() {
        let pf = SingleByte(b'x');
        assert_eq!(pf.find_next("abcxyz", 0), Some(3));
        assert_eq!(pf.find_next("abcxyz", 3), Some(3));
        assert_eq!(pf.find_next("abcxyz", 4), None);
    }

    #[test]
    fn single_byte_find_prev() {
        let pf = SingleByte(b'a');
        assert_eq!(pf.find_prev("abcabc", 6), Some(3));
        assert_eq!(pf.find_prev("abcabc", 3), Some(0));
        assert_eq!(pf.find_prev("xyz", 3), None);
    }

    #[test]
    fn single_byte_empty_text() {
        let pf = SingleByte(b'a');
        assert_eq!(pf.find_next("", 0), None);
        assert_eq!(pf.find_prev("", 0), None);
    }

    #[test]
    fn single_byte_is_fast() {
        assert!(SingleByte(b'x').is_fast());
    }

    // ── DualByte ───────────────────────────────────────────────────────

    #[test]
    fn dual_byte_find_next() {
        let pf = DualByte(b'x', b'y');
        assert_eq!(pf.find_next("abcxyz", 0), Some(3));
        assert_eq!(pf.find_next("abcyz", 0), Some(3));
    }

    #[test]
    fn dual_byte_find_prev() {
        let pf = DualByte(b'a', b'c');
        assert_eq!(pf.find_prev("abcabc", 6), Some(5));
        assert_eq!(pf.find_prev("abcabc", 4), Some(3));
    }

    #[test]
    fn dual_byte_empty_text() {
        let pf = DualByte(b'a', b'b');
        assert_eq!(pf.find_next("", 0), None);
        assert_eq!(pf.find_prev("", 0), None);
    }

    #[test]
    fn dual_byte_is_fast() {
        assert!(DualByte(b'a', b'b').is_fast());
    }

    // ── TriByte ────────────────────────────────────────────────────────

    #[test]
    fn tri_byte_find_next() {
        let pf = TriByte(b'x', b'y', b'z');
        assert_eq!(pf.find_next("abcxyz", 0), Some(3));
        assert_eq!(pf.find_next("abcyz", 0), Some(3));
        assert_eq!(pf.find_next("abcz", 0), Some(3));
    }

    #[test]
    fn tri_byte_find_prev() {
        let pf = TriByte(b'a', b'b', b'c');
        assert_eq!(pf.find_prev("abcdef", 6), Some(2));
    }

    #[test]
    fn tri_byte_empty_text() {
        let pf = TriByte(b'a', b'b', b'c');
        assert_eq!(pf.find_next("", 0), None);
        assert_eq!(pf.find_prev("", 0), None);
    }

    #[test]
    fn tri_byte_is_fast() {
        assert!(TriByte(b'a', b'b', b'c').is_fast());
    }

    // ── SubstringPrefilter ─────────────────────────────────────────────

    #[test]
    fn substring_find_next() {
        let pf = SubstringPrefilter::new(b"foo");
        assert_eq!(pf.find_next("barfoobar", 0), Some(3));
        assert_eq!(pf.find_next("barfoobar", 4), None);
    }

    #[test]
    fn substring_find_prev() {
        let pf = SubstringPrefilter::new(b"bar");
        assert_eq!(pf.find_prev("barfoobar", 9), Some(6));
        assert_eq!(pf.find_prev("barfoobar", 6), Some(0));
    }

    #[test]
    fn substring_empty_text() {
        let pf = SubstringPrefilter::new(b"abc");
        assert_eq!(pf.find_next("", 0), None);
        assert_eq!(pf.find_prev("", 0), None);
    }

    #[test]
    fn substring_no_match() {
        let pf = SubstringPrefilter::new(b"xyz");
        assert_eq!(pf.find_next("abcdef", 0), None);
        assert_eq!(pf.find_prev("abcdef", 6), None);
    }

    #[test]
    fn substring_is_fast() {
        assert!(SubstringPrefilter::new(b"foo").is_fast());
    }

    // ── ByteSet ────────────────────────────────────────────────────────

    #[test]
    fn byte_set_find_next() {
        let mut table = [false; 256];
        table[b'x' as usize] = true;
        table[b'y' as usize] = true;
        let pf = ByteSet(table);
        assert_eq!(pf.find_next("abcxyz", 0), Some(3));
    }

    #[test]
    fn byte_set_find_prev() {
        let mut table = [false; 256];
        table[b'a' as usize] = true;
        let pf = ByteSet(table);
        assert_eq!(pf.find_prev("abcabc", 6), Some(3));
    }

    #[test]
    fn byte_set_empty_text() {
        let mut table = [false; 256];
        table[b'a' as usize] = true;
        let pf = ByteSet(table);
        assert_eq!(pf.find_next("", 0), None);
        assert_eq!(pf.find_prev("", 0), None);
    }

    #[test]
    fn byte_set_no_match() {
        let mut table = [false; 256];
        table[b'z' as usize] = true;
        let pf = ByteSet(table);
        assert_eq!(pf.find_next("abcdef", 0), None);
    }

    #[test]
    fn byte_set_is_not_fast() {
        assert!(!ByteSet([false; 256]).is_fast());
    }

    // ── build_prefilter ────────────────────────────────────────────────

    #[test]
    fn build_prefilter_single_byte_prefix() {
        let props = PatternProperties {
            accel_hints: crate::hir::AccelHints {
                literal_prefix: Some(CompactString::from("f")),
                ..Default::default()
            },
            ..Default::default()
        };
        let root = LoweredNode::Literal('f');
        let pf = build_prefilter(&props, &root, CaseMode::Sensitive).unwrap();
        assert!(pf.is_fast());
        assert_eq!(pf.find_next("abcfoo", 0), Some(3));
    }

    #[test]
    fn build_prefilter_multi_byte_prefix() {
        let props = PatternProperties {
            accel_hints: crate::hir::AccelHints {
                literal_prefix: Some(CompactString::from("foo")),
                ..Default::default()
            },
            ..Default::default()
        };
        let root = LoweredNode::LiteralString("foo".into());
        let pf = build_prefilter(&props, &root, CaseMode::Sensitive).unwrap();
        assert!(pf.is_fast());
        assert_eq!(pf.find_next("barfoobar", 0), Some(3));
    }

    #[test]
    fn build_prefilter_alternation_first_bytes() {
        let props = PatternProperties::default();
        let root =
            LoweredNode::Alternation(vec![LoweredNode::Literal('x'), LoweredNode::Literal('y')]);
        let pf = build_prefilter(&props, &root, CaseMode::Sensitive).unwrap();
        assert!(pf.is_fast());
        assert_eq!(pf.find_next("abcxyz", 0), Some(3));
    }

    #[test]
    fn build_prefilter_start_byte_set_fallback() {
        let props = PatternProperties::default();
        // A Collection with a few items — no literal prefix, not an alternation
        // at the top level, so the StartSet-derived start-byte set is the fallback.
        let root = LoweredNode::Collection {
            negated: false,
            items: vec![CollectionItem::Single('m'), CollectionItem::Single('n')],
            include_newline: false,
        };
        let pf = build_prefilter(&props, &root, CaseMode::Sensitive).unwrap();
        assert_eq!(pf.find_next("abcmno", 0), Some(3));
    }

    #[test]
    fn build_prefilter_returns_none_for_any_char() {
        let props = PatternProperties::default();
        let root = LoweredNode::AnyChar;
        assert!(build_prefilter(&props, &root, CaseMode::Sensitive).is_none());
    }

    // ── extract_alternation_first_bytes ────────────────────────────────

    #[test]
    fn alternation_extracts_first_bytes() {
        let node = LoweredNode::Alternation(vec![
            LoweredNode::Literal('a'),
            LoweredNode::Literal('b'),
            LoweredNode::Literal('c'),
        ]);
        let bytes = extract_alternation_first_bytes(&node).unwrap();
        assert_eq!(bytes, vec![b'a', b'b', b'c']);
    }

    #[test]
    fn alternation_deduplicates_bytes() {
        let node =
            LoweredNode::Alternation(vec![LoweredNode::Literal('a'), LoweredNode::Literal('a')]);
        let bytes = extract_alternation_first_bytes(&node).unwrap();
        assert_eq!(bytes, vec![b'a']);
    }

    #[test]
    fn alternation_with_sequence_branches() {
        let node = LoweredNode::Alternation(vec![
            LoweredNode::Sequence(vec![LoweredNode::StartOfLine, LoweredNode::Literal('f')]),
            LoweredNode::Literal('g'),
        ]);
        let bytes = extract_alternation_first_bytes(&node).unwrap();
        assert_eq!(bytes, vec![b'f', b'g']);
    }

    #[test]
    fn alternation_with_any_char_branch_returns_none() {
        let node = LoweredNode::Alternation(vec![LoweredNode::Literal('a'), LoweredNode::AnyChar]);
        assert!(extract_alternation_first_bytes(&node).is_none());
    }

    #[test]
    fn non_alternation_returns_none() {
        assert!(extract_alternation_first_bytes(&LoweredNode::Literal('a')).is_none());
    }

    // ── NewlineStartPrefilter ─────────────────────────────────────────

    #[test]
    fn newline_prefilter_find_next_at_start() {
        let pf = NewlineStartPrefilter;
        assert_eq!(pf.find_next("hello\nworld", 0), Some(0));
    }

    #[test]
    fn newline_prefilter_find_next_after_newline() {
        let pf = NewlineStartPrefilter;
        assert_eq!(pf.find_next("hello\nworld", 1), Some(6));
    }

    #[test]
    fn newline_prefilter_find_next_at_line_start() {
        let pf = NewlineStartPrefilter;
        assert_eq!(pf.find_next("hello\nworld", 6), Some(6));
    }

    #[test]
    fn newline_prefilter_find_next_no_more_lines() {
        let pf = NewlineStartPrefilter;
        assert_eq!(pf.find_next("hello", 1), None);
    }

    #[test]
    fn newline_prefilter_find_prev() {
        let pf = NewlineStartPrefilter;
        assert_eq!(pf.find_prev("aaa\nbbb\nccc", 11), Some(8));
        assert_eq!(pf.find_prev("aaa\nbbb\nccc", 8), Some(4));
        assert_eq!(pf.find_prev("aaa\nbbb\nccc", 4), Some(0));
    }

    #[test]
    fn newline_prefilter_is_fast() {
        assert!(NewlineStartPrefilter.is_fast());
    }

    #[test]
    fn anchored_start_pattern_gets_newline_prefilter() {
        let props = PatternProperties {
            accel_hints: crate::hir::AccelHints {
                is_anchored_start: true,
                ..Default::default()
            },
            ..Default::default()
        };
        // No literal prefix, no alternation, no start desc — should fallback to newline
        let root = LoweredNode::Sequence(vec![LoweredNode::StartOfLine, LoweredNode::AnyChar]);
        let pf = build_prefilter(&props, &root, CaseMode::Sensitive);
        assert!(pf.is_some(), "anchored-start should produce a prefilter");
        assert!(pf.unwrap().is_fast());
    }

    // ── CI Multi-Byte Prefilter ───────────────────────────────────────

    #[test]
    fn ci_prefilter_multi_byte_generates_variants() {
        let variants = generate_ascii_case_variants(b"Fo", 64).unwrap();
        assert_eq!(variants.len(), 4); // fo, Fo, fO, FO
        assert!(variants.contains(&b"fo".to_vec()));
        assert!(variants.contains(&b"Fo".to_vec()));
        assert!(variants.contains(&b"fO".to_vec()));
        assert!(variants.contains(&b"FO".to_vec()));
    }

    #[test]
    fn ci_prefilter_multi_byte_non_alpha_unchanged() {
        let variants = generate_ascii_case_variants(b"1a", 64).unwrap();
        assert_eq!(variants.len(), 2); // 1a, 1A
        assert!(variants.contains(&b"1a".to_vec()));
        assert!(variants.contains(&b"1A".to_vec()));
    }

    #[test]
    fn ci_prefilter_multi_byte_too_many_variants() {
        // 7 alpha chars -> 128 variants > 64 limit
        assert!(generate_ascii_case_variants(b"abcdefg", 64).is_none());
    }

    #[test]
    fn ci_prefilter_multi_byte_finds_match() {
        let props = PatternProperties {
            accel_hints: crate::hir::AccelHints {
                literal_prefix: Some(CompactString::from("foo")),
                ..Default::default()
            },
            ..Default::default()
        };
        let pf = build_case_insensitive_prefilter(&props).unwrap();
        assert!(pf.is_fast());
        assert_eq!(pf.find_next("xxxFOObar", 0), Some(3));
        assert_eq!(pf.find_next("xxxfOobar", 0), Some(3));
    }

    #[test]
    fn ci_prefilter_single_byte_unchanged() {
        // Existing behavior preserved
        let props = PatternProperties {
            accel_hints: crate::hir::AccelHints {
                literal_prefix: Some(CompactString::from("A")),
                ..Default::default()
            },
            ..Default::default()
        };
        let pf = build_case_insensitive_prefilter(&props).unwrap();
        assert!(pf.is_fast());
        assert_eq!(pf.find_next("xxxabc", 0), Some(3));
        assert_eq!(pf.find_next("xxxAbc", 0), Some(3));
    }

    // ── PackedPrefilter ──────────────────────────────────────────────

    #[test]
    fn packed_prefilter_find_next() {
        let needles: Vec<Vec<u8>> = vec![vec![b'x'], vec![b'y'], vec![b'z']];
        let pf = PackedPrefilter::new(&needles).unwrap();
        assert_eq!(pf.find_next("abcxyz", 0), Some(3));
        assert_eq!(pf.find_next("abcxyz", 4), Some(4));
        assert_eq!(pf.find_next("abcxyz", 6), None);
    }

    #[test]
    fn packed_prefilter_find_prev() {
        let needles: Vec<Vec<u8>> = vec![vec![b'a'], vec![b'b']];
        let pf = PackedPrefilter::new(&needles).unwrap();
        assert_eq!(pf.find_prev("abcabc", 6), Some(4));
        assert_eq!(pf.find_prev("abcabc", 4), Some(3));
        assert_eq!(pf.find_prev("abcabc", 3), Some(1));
    }

    #[test]
    fn packed_prefilter_empty_text() {
        let needles: Vec<Vec<u8>> = vec![vec![b'a']];
        let pf = PackedPrefilter::new(&needles).unwrap();
        assert_eq!(pf.find_next("", 0), None);
        assert_eq!(pf.find_prev("", 0), None);
    }

    #[test]
    fn packed_prefilter_no_match() {
        let needles: Vec<Vec<u8>> = vec![vec![b'x'], vec![b'y']];
        let pf = PackedPrefilter::new(&needles).unwrap();
        assert_eq!(pf.find_next("abcdef", 0), None);
        assert_eq!(pf.find_prev("abcdef", 6), None);
    }

    #[test]
    fn packed_prefilter_is_fast() {
        let needles: Vec<Vec<u8>> = vec![vec![b'a'], vec![b'b']];
        if let Some(pf) = PackedPrefilter::new(&needles) {
            assert!(pf.is_fast());
        }
        // On WASM/non-SIMD platforms, PackedPrefilter::new returns None -- that's OK.
    }

    #[test]
    fn packed_prefilter_multi_byte_needles() {
        let needles: Vec<Vec<u8>> = vec![b"foo".to_vec(), b"bar".to_vec(), b"baz".to_vec()];
        let pf = PackedPrefilter::new(&needles).unwrap();
        assert_eq!(pf.find_next("xxxfooyyy", 0), Some(3));
        assert_eq!(pf.find_next("xxxbaryyy", 0), Some(3));
        assert_eq!(pf.find_next("xxxbazyyy", 0), Some(3));
        assert_eq!(pf.find_next("xxxquxyyy", 0), None);
    }

    #[test]
    fn packed_prefilter_returns_none_for_empty_needles() {
        let needles: Vec<Vec<u8>> = vec![];
        assert!(PackedPrefilter::new(&needles).is_none());
    }

    // ── Rare-byte selection ──────────────────────────────────────────

    #[test]
    fn select_rarest_picks_least_frequent() {
        // Space (0x20) is very common, 'z' and 'q' are rare
        let candidates = *b" zqe";
        let result = select_rarest(&candidates, 2);
        // 'q' and 'z' should be selected (rarest in English text)
        assert_eq!(result.len(), 2);
        assert!(result.contains(&b'q'));
        assert!(result.contains(&b'z'));
    }

    #[test]
    fn select_rarest_with_fewer_candidates_than_count() {
        let candidates = *b"ab";
        let result = select_rarest(&candidates, 3);
        assert_eq!(result.len(), 2);
        assert!(result.contains(&b'a'));
        assert!(result.contains(&b'b'));
    }

    #[test]
    fn select_rarest_empty_input() {
        let candidates: [u8; 0] = [];
        let result = select_rarest(&candidates, 3);
        assert!(result.is_empty());
    }

    #[test]
    fn select_rarest_prefers_non_ascii_control() {
        // Control characters (0x00-0x1F except \t, \n, \r) are very rare
        let candidates = [b'e', b'a', 0x01, 0x02];
        let result = select_rarest(&candidates, 2);
        assert!(result.contains(&0x01));
        assert!(result.contains(&0x02));
    }

    #[test]
    fn select_rarest_single() {
        let candidates = *b" q";
        let result = select_rarest(&candidates, 1);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], b'q'); // 'q' is rarer than space
    }

    // ── ByteSet promotion ────────────────────────────────────────────

    #[test]
    fn byte_set_set_bytes_returns_set_entries() {
        let mut table = [false; 256];
        table[b'x' as usize] = true;
        table[b'a' as usize] = true;
        table[b'm' as usize] = true;
        let bs = ByteSet(table);
        let bytes: Vec<u8> = bs.set_bytes().collect();
        assert_eq!(bytes, vec![b'a', b'm', b'x']);
    }

    #[test]
    fn byte_set_try_promote_few_bytes() {
        let mut table = [false; 256];
        table[b'x' as usize] = true;
        table[b'y' as usize] = true;
        table[b'z' as usize] = true;
        let bs = ByteSet(table);
        if let Some(pf) = bs.try_promote_to_packed() {
            assert!(pf.is_fast());
            assert_eq!(pf.find_next("abcxyz", 0), Some(3));
            assert_eq!(pf.find_next("abcdef", 0), None);
        }
        // On non-SIMD platforms, promotion may return None -- acceptable.
    }

    #[test]
    fn byte_set_try_promote_too_many_bytes_returns_none() {
        // 65+ bytes should fail (exceeds the 64-byte promotion limit)
        let mut table = [false; 256];
        for i in 0u8..65 {
            table[i as usize] = true;
        }
        let bs = ByteSet(table);
        // Limit is 64, so 65 should return None
        assert!(bs.try_promote_to_packed().is_none());
    }

    #[test]
    fn byte_set_try_promote_exactly_64_bytes() {
        let mut table = [false; 256];
        for i in 0u8..64 {
            table[i as usize] = true;
        }
        let bs = ByteSet(table);
        // 64 is within the limit -- may succeed on SIMD platforms.
        // On non-SIMD it returns None, which is fine.
        if let Some(pf) = bs.try_promote_to_packed() {
            assert!(pf.is_fast());
        }
    }

    #[test]
    fn byte_set_try_promote_empty_returns_none() {
        let bs = ByteSet([false; 256]);
        assert!(bs.try_promote_to_packed().is_none());
    }

    // ── Packed promotion in build_prefilter ──────────────────────────

    #[test]
    fn bytes_to_prefilter_4_bytes_tries_packed_promotion() {
        // 4 bytes goes to ByteSet path, which should try packed promotion.
        let pf = bytes_to_prefilter(b"wxyz");
        // On SIMD platforms: PackedPrefilter (is_fast = true)
        // On non-SIMD: ByteSet (is_fast = false)
        // Either way, it must find the correct position.
        assert_eq!(pf.find_next("abcwxyz", 0), Some(3));
    }

    #[test]
    fn bytes_to_prefilter_rarest_bytes_selected_for_3() {
        // When we have >3 candidates and packed promotion fails, we should
        // select the 3 rarest bytes for TriByte (not just the first 3).
        // This test verifies correctness of the result.
        let pf = bytes_to_prefilter(b" eqz");
        // Must still find all 4 bytes
        assert!(pf.find_next("q", 0).is_some());
        assert!(pf.find_next("z", 0).is_some());
        assert!(pf.find_next(" ", 0).is_some());
        assert!(pf.find_next("e", 0).is_some());
    }

    // ── End-to-end packed promotion ──────────────────────────────────

    #[test]
    fn build_prefilter_alternation_4_branches_promotes_to_packed_or_byteset() {
        let props = PatternProperties::default();
        let root = LoweredNode::Alternation(vec![
            LoweredNode::Literal('w'),
            LoweredNode::Literal('x'),
            LoweredNode::Literal('y'),
            LoweredNode::Literal('z'),
        ]);
        let pf = build_prefilter(&props, &root, CaseMode::Sensitive).unwrap();
        // On SIMD: PackedPrefilter (fast), on non-SIMD: ByteSet (not fast).
        // Either way, correctness is preserved.
        assert_eq!(pf.find_next("abcwxyz", 0), Some(3));
        assert_eq!(pf.find_next("abcxyz", 0), Some(3));
        assert_eq!(pf.find_next("abcyz", 0), Some(3));
        assert_eq!(pf.find_next("abcz", 0), Some(3));
        assert_eq!(pf.find_next("abcdef", 0), None);
    }

    #[test]
    fn build_prefilter_start_byte_set_many_bytes_promotes_or_falls_back() {
        let props = PatternProperties::default();
        // Collection with 10 items -> start-byte set with 10 bytes -> try packed
        let items: Vec<CollectionItem> = (b'a'..=b'j')
            .map(|b| CollectionItem::Single(b as char))
            .collect();
        let root = LoweredNode::Collection {
            negated: false,
            items,
            include_newline: false,
        };
        let pf = build_prefilter(&props, &root, CaseMode::Sensitive).unwrap();
        assert_eq!(pf.find_next("xxxayyy", 0), Some(3));
        assert_eq!(pf.find_next("xxxjyyy", 0), Some(3));
        assert_eq!(pf.find_next("xxxkyyy", 0), None);
    }

    #[test]
    fn rare_byte_selection_correctness_in_dual_byte() {
        // When alternation has 2 branches, we still use DualByte but with
        // rarest bytes. Both bytes must match.
        let props = PatternProperties::default();
        let root = LoweredNode::Alternation(vec![
            LoweredNode::Literal('e'), // common
            LoweredNode::Literal('q'), // rare
        ]);
        let pf = build_prefilter(&props, &root, CaseMode::Sensitive).unwrap();
        assert!(pf.is_fast()); // DualByte is always fast
        assert_eq!(pf.find_next("xxxeyyy", 0), Some(3));
        assert_eq!(pf.find_next("xxxqyyy", 0), Some(3));
    }
}
