//! UTF-8 byte sequence decomposition for DFA transitions.
//!
//! Converts ranges of Unicode scalar values into equivalent ranges of UTF-8 byte
//! sequences. This enables the byte-level lazy DFA to handle multi-byte characters
//! natively by decomposing character-level transitions into sequences of byte-level
//! transitions.
//!
//! Based on the Russ Cox algorithm (via Ken Thompson's grep) as implemented in
//! regex-syntax. Each Unicode scalar value range [start, end] is decomposed into
//! a minimal set of `Utf8Sequence` values, where each sequence represents an
//! alternation of byte ranges that match a non-overlapping subset of the range.

use core::iter::FusedIterator;

const MAX_UTF8_BYTES: usize = 4;

// ═══════════════════════════════════════════════════════════════════════════════
// UTF-8 BYTE RANGE CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Minimum continuation byte value (10xxxxxx).
pub(super) const CONT_MIN: u8 = 0x80;
/// Maximum continuation byte value (10111111).
pub(super) const CONT_MAX: u8 = 0xBF;
/// Minimum 2-byte lead byte (11000010 -- 0xC0/0xC1 are overlong).
pub(super) const LEAD_2B_MIN: u8 = 0xC2;
/// Maximum 2-byte lead byte (11011111).
#[allow(dead_code, reason = "available for future byte-range validation")]
pub(super) const LEAD_2B_MAX: u8 = 0xDF;
/// Minimum 3-byte lead byte (11100000).
pub(super) const LEAD_3B_MIN: u8 = 0xE0;
/// Maximum 3-byte lead byte (11101111).
#[allow(dead_code, reason = "available for future byte-range validation")]
pub(super) const LEAD_3B_MAX: u8 = 0xEF;
/// Minimum 4-byte lead byte (11110000).
pub(super) const LEAD_4B_MIN: u8 = 0xF0;
/// Maximum 4-byte lead byte (11110100 -- 0xF5+ are invalid).
pub(super) const LEAD_4B_MAX: u8 = 0xF4;
/// First invalid lead byte (after LEAD_4B_MAX).
pub(super) const INVALID_MIN: u8 = 0xF5;
/// Overlong lead byte range start (0xC0).
pub(super) const OVERLONG_MIN: u8 = 0xC0;

// ═══════════════════════════════════════════════════════════════════════════════
// DATA STRUCTURES
// ═══════════════════════════════════════════════════════════════════════════════

/// A single inclusive range of UTF-8 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Utf8Range {
    pub start: u8,
    pub end: u8,
}

impl Utf8Range {
    #[inline]
    pub const fn new(start: u8, end: u8) -> Self {
        Self { start, end }
    }

    /// Returns true if the byte falls within this range (inclusive).
    #[inline]
    pub const fn matches(&self, b: u8) -> bool {
        self.start <= b && b <= self.end
    }
}

/// A sequence of 1-4 byte ranges that together match one UTF-8 encoded scalar
/// value. To match, a byte sequence must satisfy each range in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Utf8Sequence {
    /// A single byte range (ASCII).
    One(Utf8Range),
    /// Two successive byte ranges (2-byte UTF-8).
    Two([Utf8Range; 2]),
    /// Three successive byte ranges (3-byte UTF-8).
    Three([Utf8Range; 3]),
    /// Four successive byte ranges (4-byte UTF-8).
    Four([Utf8Range; 4]),
}

impl Utf8Sequence {
    /// Returns the underlying sequence of byte ranges as a slice.
    pub fn as_slice(&self) -> &[Utf8Range] {
        match self {
            Utf8Sequence::One(r) => core::slice::from_ref(r),
            Utf8Sequence::Two(r) => &r[..],
            Utf8Sequence::Three(r) => &r[..],
            Utf8Sequence::Four(r) => &r[..],
        }
    }

    /// Returns the number of byte ranges in this sequence.
    #[inline]
    pub fn len(&self) -> usize {
        self.as_slice().len()
    }

    /// Returns true if a prefix of `bytes` matches this sequence of byte ranges.
    #[cfg(test)]
    pub fn matches(&self, bytes: &[u8]) -> bool {
        if bytes.len() < self.len() {
            return false;
        }
        for (&b, r) in bytes.iter().zip(self.as_slice()) {
            if !r.matches(b) {
                return false;
            }
        }
        true
    }

    /// Construct from encoded start/end byte arrays of equal length.
    fn from_encoded_range(start: &[u8], end: &[u8]) -> Self {
        assert_eq!(start.len(), end.len());
        match start.len() {
            2 => Utf8Sequence::Two([
                Utf8Range::new(start[0], end[0]),
                Utf8Range::new(start[1], end[1]),
            ]),
            3 => Utf8Sequence::Three([
                Utf8Range::new(start[0], end[0]),
                Utf8Range::new(start[1], end[1]),
                Utf8Range::new(start[2], end[2]),
            ]),
            4 => Utf8Sequence::Four([
                Utf8Range::new(start[0], end[0]),
                Utf8Range::new(start[1], end[1]),
                Utf8Range::new(start[2], end[2]),
                Utf8Range::new(start[3], end[3]),
            ]),
            n => unreachable!("invalid encoded length: {n}"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ITERATOR
// ═══════════════════════════════════════════════════════════════════════════════

/// Decomposes a Unicode scalar value range [start, end] into the minimal set
/// of UTF-8 byte-range sequences that match exactly that range.
///
/// This implements the Russ Cox algorithm. Each yielded `Utf8Sequence` matches
/// a unique, non-overlapping subset of the original range.
#[derive(Debug)]
pub(super) struct Utf8Sequences {
    range_stack: Vec<ScalarRange>,
}

#[derive(Debug)]
struct ScalarRange {
    start: u32,
    end: u32,
}

impl Utf8Sequences {
    /// Create a new iterator over UTF-8 byte ranges for the given scalar value range.
    pub fn new(start: char, end: char) -> Self {
        let range = ScalarRange {
            start: u32::from(start),
            end: u32::from(end),
        };
        Utf8Sequences {
            range_stack: vec![range],
        }
    }

    /// Decompose a single character into its byte-level sequence.
    /// Always yields exactly one `Utf8Sequence`.
    pub fn single(ch: char) -> Utf8Sequence {
        Self::new(ch, ch).next().unwrap()
    }

    fn push(&mut self, start: u32, end: u32) {
        self.range_stack.push(ScalarRange { start, end });
    }
}

impl Iterator for Utf8Sequences {
    type Item = Utf8Sequence;

    fn next(&mut self) -> Option<Self::Item> {
        'TOP: while let Some(mut r) = self.range_stack.pop() {
            'INNER: loop {
                // Split around surrogates.
                if let Some((r1, r2)) = r.split() {
                    self.push(r2.start, r2.end);
                    r.start = r1.start;
                    r.end = r1.end;
                    continue 'INNER;
                }
                if !r.is_valid() {
                    continue 'TOP;
                }
                // Split at encoding-length boundaries.
                for i in 1..MAX_UTF8_BYTES {
                    let max = max_scalar_value(i);
                    if r.start <= max && max < r.end {
                        self.push(max + 1, r.end);
                        r.end = max;
                        continue 'INNER;
                    }
                }
                // ASCII range — emit directly.
                if let Some(ascii_range) = r.as_ascii() {
                    return Some(Utf8Sequence::One(ascii_range));
                }
                // Split at continuation-byte boundaries to ensure
                // the encoded range is "aligned" at each byte position.
                for i in 1..MAX_UTF8_BYTES {
                    let m = (1 << (6 * i)) - 1;
                    if (r.start & !m) != (r.end & !m) {
                        if (r.start & m) != 0 {
                            self.push((r.start | m) + 1, r.end);
                            r.end = r.start | m;
                            continue 'INNER;
                        }
                        if (r.end & m) != m {
                            self.push(r.end & !m, r.end);
                            r.end = (r.end & !m) - 1;
                            continue 'INNER;
                        }
                    }
                }
                // Range is now aligned: encode start/end and emit.
                let mut start_buf = [0u8; MAX_UTF8_BYTES];
                let mut end_buf = [0u8; MAX_UTF8_BYTES];
                let n = r.encode(&mut start_buf, &mut end_buf);
                return Some(Utf8Sequence::from_encoded_range(
                    &start_buf[..n],
                    &end_buf[..n],
                ));
            }
        }
        None
    }
}

impl FusedIterator for Utf8Sequences {}

// ═══════════════════════════════════════════════════════════════════════════════
// SCALAR RANGE HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

impl ScalarRange {
    /// Split this range if it overlaps with surrogate codepoints.
    fn split(&self) -> Option<(ScalarRange, ScalarRange)> {
        if self.start < 0xE000 && self.end > 0xD7FF {
            Some((
                ScalarRange {
                    start: self.start,
                    end: 0xD7FF,
                },
                ScalarRange {
                    start: 0xE000,
                    end: self.end,
                },
            ))
        } else {
            None
        }
    }

    /// Returns true if start <= end.
    fn is_valid(&self) -> bool {
        self.start <= self.end
    }

    /// If this range is ASCII-only, return it as a single Utf8Range.
    fn as_ascii(&self) -> Option<Utf8Range> {
        if self.is_ascii() {
            let start = u8::try_from(self.start).unwrap();
            let end = u8::try_from(self.end).unwrap();
            Some(Utf8Range::new(start, end))
        } else {
            None
        }
    }

    /// Returns true if the range is ASCII-only.
    fn is_ascii(&self) -> bool {
        self.is_valid() && self.end <= 0x7F
    }

    /// Encode the start and end of this range as UTF-8, returning the byte length.
    fn encode(&self, start: &mut [u8], end: &mut [u8]) -> usize {
        let cs = char::from_u32(self.start).unwrap();
        let ce = char::from_u32(self.end).unwrap();
        let ss = cs.encode_utf8(start);
        let se = ce.encode_utf8(end);
        assert_eq!(ss.len(), se.len());
        ss.len()
    }
}

fn max_scalar_value(nbytes: usize) -> u32 {
    match nbytes {
        1 => 0x007F,
        2 => 0x07FF,
        3 => 0xFFFF,
        4 => 0x0010_FFFF,
        _ => unreachable!("invalid UTF-8 byte sequence size"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn rutf8(s: u8, e: u8) -> Utf8Range {
        Utf8Range::new(s, e)
    }

    #[test]
    fn utf8_single_ascii_yields_one() {
        let seq = Utf8Sequences::single('a');
        assert_eq!(seq, Utf8Sequence::One(rutf8(0x61, 0x61)));
    }

    #[test]
    fn utf8_two_byte_char() {
        // U+00E9 (e-acute) encodes as [0xC3, 0xA9].
        let seq = Utf8Sequences::single('\u{00E9}');
        assert_eq!(
            seq,
            Utf8Sequence::Two([rutf8(0xC3, 0xC3), rutf8(0xA9, 0xA9)])
        );
    }

    #[test]
    fn utf8_three_byte_char() {
        // U+4E2D (CJK "zhong") encodes as [0xE4, 0xB8, 0xAD].
        let seq = Utf8Sequences::single('\u{4E2D}');
        assert_eq!(
            seq,
            Utf8Sequence::Three([rutf8(0xE4, 0xE4), rutf8(0xB8, 0xB8), rutf8(0xAD, 0xAD)])
        );
    }

    #[test]
    fn utf8_four_byte_char() {
        // U+1F600 (grinning face) encodes as [0xF0, 0x9F, 0x98, 0x80].
        let seq = Utf8Sequences::single('\u{1F600}');
        assert_eq!(
            seq,
            Utf8Sequence::Four([
                rutf8(0xF0, 0xF0),
                rutf8(0x9F, 0x9F),
                rutf8(0x98, 0x98),
                rutf8(0x80, 0x80)
            ])
        );
    }

    #[test]
    fn utf8_bmp_decomposition() {
        // The basic multilingual plane [U+0000, U+FFFF] yields the canonical
        // 6-sequence decomposition.
        let seqs: Vec<_> = Utf8Sequences::new('\u{0}', '\u{FFFF}').collect();
        assert_eq!(
            seqs,
            vec![
                Utf8Sequence::One(rutf8(0x0, 0x7F)),
                Utf8Sequence::Two([rutf8(0xC2, 0xDF), rutf8(0x80, 0xBF)]),
                Utf8Sequence::Three([rutf8(0xE0, 0xE0), rutf8(0xA0, 0xBF), rutf8(0x80, 0xBF)]),
                Utf8Sequence::Three([rutf8(0xE1, 0xEC), rutf8(0x80, 0xBF), rutf8(0x80, 0xBF)]),
                Utf8Sequence::Three([rutf8(0xED, 0xED), rutf8(0x80, 0x9F), rutf8(0x80, 0xBF)]),
                Utf8Sequence::Three([rutf8(0xEE, 0xEF), rutf8(0x80, 0xBF), rutf8(0x80, 0xBF)]),
            ]
        );
    }

    #[test]
    fn utf8_never_accepts_surrogates() {
        // No sequence should match a surrogate codepoint encoding.
        for cp in 0xD800..0xE000u32 {
            let buf = encode_surrogate(cp);
            for r in Utf8Sequences::new('\u{0}', '\u{FFFF}') {
                assert!(
                    !r.matches(&buf),
                    "Sequence matches surrogate codepoint {:X}",
                    cp
                );
            }
        }
    }

    #[test]
    fn utf8_every_char_in_range_matches_one_sequence() {
        // For the Cyrillic range [U+0400, U+04FF], every character must match
        // exactly one yielded sequence.
        let seqs: Vec<_> = Utf8Sequences::new('\u{0400}', '\u{04FF}').collect();
        for cp in 0x0400..=0x04FFu32 {
            let ch = char::from_u32(cp).unwrap();
            let mut buf = [0u8; 4];
            let encoded = ch.encode_utf8(&mut buf);
            let match_count = seqs
                .iter()
                .filter(|s| s.matches(encoded.as_bytes()))
                .count();
            assert_eq!(
                match_count, 1,
                "Codepoint U+{:04X} matches {} sequences (expected 1)",
                cp, match_count
            );
        }
    }

    #[test]
    fn utf8_no_char_outside_range_matches() {
        // For a small range, verify no char outside the range matches.
        let seqs: Vec<_> = Utf8Sequences::new('\u{00E0}', '\u{00FF}').collect();

        // Test chars just outside the range.
        for cp in [0x00DF, 0x0100, 0x0041, 0x007F] {
            let ch = char::from_u32(cp).unwrap();
            let mut buf = [0u8; 4];
            let encoded = ch.encode_utf8(&mut buf);
            let matches_any = seqs.iter().any(|s| s.matches(encoded.as_bytes()));
            assert!(
                !matches_any,
                "Codepoint U+{:04X} should NOT match but does",
                cp
            );
        }
    }

    #[test]
    fn utf8_single_char_yields_one_sequence() {
        // Every single-char range must yield exactly one sequence.
        let test_chars = ['\0', 'A', '\u{00E9}', '\u{4E2D}', '\u{1F600}', '\u{10FFFF}'];
        for &ch in &test_chars {
            let seqs: Vec<_> = Utf8Sequences::new(ch, ch).collect();
            assert_eq!(
                seqs.len(),
                1,
                "Single char U+{:04X} should yield 1 sequence, got {}",
                ch as u32,
                seqs.len()
            );
        }
    }

    #[test]
    fn utf8_sequence_len() {
        assert_eq!(Utf8Sequences::single('a').len(), 1);
        assert_eq!(Utf8Sequences::single('\u{00E9}').len(), 2);
        assert_eq!(Utf8Sequences::single('\u{4E2D}').len(), 3);
        assert_eq!(Utf8Sequences::single('\u{1F600}').len(), 4);
    }

    #[test]
    fn utf8_matches_method() {
        let seq = Utf8Sequences::single('\u{00E9}');
        assert!(seq.matches(&[0xC3, 0xA9]));
        assert!(!seq.matches(&[0xC3, 0xA8])); // e-grave, not e-acute
        assert!(!seq.matches(&[0xC3])); // too short
    }

    #[test]
    fn utf8_cjk_range() {
        // CJK Unified Ideographs [U+4E00, U+9FFF] — should yield a small number of sequences.
        let seqs: Vec<_> = Utf8Sequences::new('\u{4E00}', '\u{9FFF}').collect();
        // This is a large range but should decompose into a bounded number of sequences.
        assert!(
            seqs.len() <= 10,
            "CJK range decomposed into {} sequences",
            seqs.len()
        );

        // Spot-check that U+4E2D matches.
        let mut buf = [0u8; 4];
        let encoded = '\u{4E2D}'.encode_utf8(&mut buf);
        assert!(seqs.iter().any(|s| s.matches(encoded.as_bytes())));

        // Spot-check that U+3FFF (outside range) does NOT match.
        let mut buf2 = [0u8; 4];
        let encoded2 = '\u{3FFF}'.encode_utf8(&mut buf2);
        assert!(!seqs.iter().any(|s| s.matches(encoded2.as_bytes())));
    }

    #[test]
    fn utf8_emoji_range() {
        // Emoji emoticons [U+1F600, U+1F64F] — 4-byte sequences.
        let seqs: Vec<_> = Utf8Sequences::new('\u{1F600}', '\u{1F64F}').collect();
        assert!(!seqs.is_empty());

        // All sequences should be 4 bytes.
        for seq in &seqs {
            assert_eq!(seq.len(), 4);
        }

        // U+1F601 should match.
        let mut buf = [0u8; 4];
        let encoded = '\u{1F601}'.encode_utf8(&mut buf);
        assert!(seqs.iter().any(|s| s.matches(encoded.as_bytes())));

        // U+1F650 (outside range) should NOT match.
        let mut buf2 = [0u8; 4];
        let encoded2 = '\u{1F650}'.encode_utf8(&mut buf2);
        assert!(!seqs.iter().any(|s| s.matches(encoded2.as_bytes())));
    }

    /// Encode a surrogate codepoint as if it were valid 3-byte UTF-8 (it isn't).
    fn encode_surrogate(cp: u32) -> [u8; 3] {
        const TAG_CONT: u8 = 0b1000_0000;
        const TAG_THREE_B: u8 = 0b1110_0000;

        assert!((0xD800..0xE000).contains(&cp));
        let mut dst = [0u8; 3];
        dst[0] = u8::try_from(cp >> 12 & 0x0F).unwrap() | TAG_THREE_B;
        dst[1] = u8::try_from(cp >> 6 & 0x3F).unwrap() | TAG_CONT;
        dst[2] = u8::try_from(cp & 0x3F).unwrap() | TAG_CONT;
        dst
    }
}
