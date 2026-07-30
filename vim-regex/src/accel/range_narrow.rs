//! Buffer-position range narrowing.
//!
//! Restricts a search range using acceleration hints from `PatternProperties`,
//! such as `\%23l` (required line), `\%<23l` / `\%>23l` (line ranges), and
//! `\%V` (visual area prefix).

use std::cmp::{max, min, Ordering};
use std::ops::Range;

use crate::hir::PatternProperties;
use crate::matchers::LineResolver;

// ═══════════════════════════════════════════════════════════════════════════════
// RANGE NARROWING
// ═══════════════════════════════════════════════════════════════════════════════

/// Narrow a search range using pattern properties.
///
/// Modifies `range` in-place to restrict the search to the smallest
/// region that can possibly contain a match, based on:
///
/// - `required_line` — `\%23l` restricts to line 23
/// - `required_line_range` — `\%<23l` or `\%>23l` restricts to before/after line 23
/// - `has_visual_area_prefix` — `\%V` at pattern start restricts to visual selection
///
/// Ensures the range remains valid (start <= end) after narrowing.
pub(crate) fn narrow_search_range(
    range: &mut Range<usize>,
    properties: &PatternProperties,
    line_resolver: Option<&dyn LineResolver>,
    visual_range: Option<(usize, usize)>,
) {
    // \%23l -> restrict to line 23
    if let Some(line) = properties.required_line() {
        if let Some(resolver) = line_resolver {
            if let Some((start, end)) = resolver.line_byte_range(line) {
                range.start = max(range.start, start);
                range.end = min(range.end, end);
            }
        }
    }

    // \%<23l or \%>23l
    if let Some((ordering, line)) = properties.required_line_range() {
        if let Some(resolver) = line_resolver {
            if let Some((line_start, line_end)) = resolver.line_byte_range(line) {
                match ordering {
                    Ordering::Less => range.end = min(range.end, line_start),
                    Ordering::Greater => range.start = max(range.start, line_end),
                    Ordering::Equal => {} // handled by required_line
                }
            }
        }
    }

    // \%V at pattern start
    if properties.has_visual_area_prefix() {
        if let Some((vis_start, vis_end)) = visual_range {
            range.start = max(range.start, vis_start);
            range.end = min(range.end, vis_end.saturating_add(1));
        }
    }

    // Ensure valid range
    if range.start > range.end {
        range.start = range.end;
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// A simple mock line resolver for testing.
    struct MockLineResolver {
        lines: Vec<(u32, usize, usize)>, // (line_number, start, end)
    }

    impl MockLineResolver {
        fn new(lines: Vec<(u32, usize, usize)>) -> Self {
            Self { lines }
        }
    }

    impl LineResolver for MockLineResolver {
        fn byte_to_line(&self, _offset: usize) -> u32 {
            1
        }
        fn byte_to_col(&self, _offset: usize) -> u32 {
            1
        }
        fn byte_to_vcol(&self, _offset: usize) -> u32 {
            1
        }
        fn cursor_line(&self) -> u32 {
            1
        }
        fn line_byte_range(&self, line: u32) -> Option<(usize, usize)> {
            self.lines
                .iter()
                .find(|(l, _, _)| *l == line)
                .map(|(_, s, e)| (*s, *e))
        }
    }

    #[test]
    fn no_narrowing_when_no_properties_set() {
        let mut range = 0..100;
        let props = PatternProperties::default();
        narrow_search_range(&mut range, &props, None, None);
        assert_eq!(range, 0..100);
    }

    #[test]
    fn required_line_narrows_to_line() {
        use crate::hir::AccelHints;
        let mut range = 0..100;
        let props = PatternProperties {
            accel_hints: AccelHints {
                required_line: Some(2),
                ..Default::default()
            },
            ..Default::default()
        };
        let resolver = MockLineResolver::new(vec![(1, 0, 10), (2, 10, 25), (3, 25, 40)]);
        narrow_search_range(&mut range, &props, Some(&resolver), None);
        assert_eq!(range, 10..25);
    }

    #[test]
    fn required_line_intersects_with_existing_range() {
        use crate::hir::AccelHints;
        let mut range = 15..100;
        let props = PatternProperties {
            accel_hints: AccelHints {
                required_line: Some(2),
                ..Default::default()
            },
            ..Default::default()
        };
        let resolver = MockLineResolver::new(vec![(2, 10, 25)]);
        narrow_search_range(&mut range, &props, Some(&resolver), None);
        assert_eq!(range, 15..25);
    }

    #[test]
    fn required_line_range_less_narrows_end() {
        use crate::hir::AccelHints;
        let mut range = 0..100;
        let props = PatternProperties {
            accel_hints: AccelHints {
                required_line_range: Some((Ordering::Less, 3)),
                ..Default::default()
            },
            ..Default::default()
        };
        let resolver = MockLineResolver::new(vec![(3, 25, 40)]);
        narrow_search_range(&mut range, &props, Some(&resolver), None);
        // \%<3l means match before line 3, so end at start of line 3
        assert_eq!(range, 0..25);
    }

    #[test]
    fn required_line_range_greater_narrows_start() {
        use crate::hir::AccelHints;
        let mut range = 0..100;
        let props = PatternProperties {
            accel_hints: AccelHints {
                required_line_range: Some((Ordering::Greater, 2)),
                ..Default::default()
            },
            ..Default::default()
        };
        let resolver = MockLineResolver::new(vec![(2, 10, 25)]);
        narrow_search_range(&mut range, &props, Some(&resolver), None);
        // \%>2l means match after line 2, so start at end of line 2
        assert_eq!(range, 25..100);
    }

    #[test]
    fn visual_area_prefix_narrows_to_selection() {
        use crate::hir::FeatureFlags;
        let mut range = 0..100;
        let props = PatternProperties {
            features: FeatureFlags {
                has_visual_area_prefix: true,
                ..Default::default()
            },
            ..Default::default()
        };
        narrow_search_range(&mut range, &props, None, Some((20, 50)));
        assert_eq!(range, 20..51);
    }

    #[test]
    fn visual_area_intersects_with_existing_range() {
        use crate::hir::FeatureFlags;
        let mut range = 30..80;
        let props = PatternProperties {
            features: FeatureFlags {
                has_visual_area_prefix: true,
                ..Default::default()
            },
            ..Default::default()
        };
        narrow_search_range(&mut range, &props, None, Some((20, 50)));
        assert_eq!(range, 30..51);
    }

    #[test]
    fn invalid_range_clamped() {
        use crate::hir::{AccelHints, FeatureFlags};
        let mut range = 0..100;
        let props = PatternProperties {
            features: FeatureFlags {
                has_visual_area_prefix: true,
                ..Default::default()
            },
            accel_hints: AccelHints {
                required_line: Some(5),
                ..Default::default()
            },
            ..Default::default()
        };
        // Line 5 at bytes 50-60, but visual range only goes to 40
        let resolver = MockLineResolver::new(vec![(5, 50, 60)]);
        narrow_search_range(&mut range, &props, Some(&resolver), Some((0, 40)));
        // Line narrows to 50..60, visual narrows to max(50,0)..min(60,41) = 50..41
        // Then clamp: start > end, so start = end
        assert_eq!(range, 41..41);
    }

    #[test]
    fn no_line_resolver_skips_line_narrowing() {
        use crate::hir::AccelHints;
        let mut range = 0..100;
        let props = PatternProperties {
            accel_hints: AccelHints {
                required_line: Some(2),
                ..Default::default()
            },
            ..Default::default()
        };
        narrow_search_range(&mut range, &props, None, None);
        assert_eq!(range, 0..100);
    }

    #[test]
    fn unknown_line_number_no_narrowing() {
        use crate::hir::AccelHints;
        let mut range = 0..100;
        let props = PatternProperties {
            accel_hints: AccelHints {
                required_line: Some(99),
                ..Default::default()
            },
            ..Default::default()
        };
        let resolver = MockLineResolver::new(vec![(1, 0, 10)]);
        narrow_search_range(&mut range, &props, Some(&resolver), None);
        // Line 99 not found in resolver, no narrowing
        assert_eq!(range, 0..100);
    }

    #[test]
    fn required_line_range_equal_no_effect() {
        use crate::hir::AccelHints;
        let mut range = 0..100;
        let props = PatternProperties {
            accel_hints: AccelHints {
                required_line_range: Some((Ordering::Equal, 2)),
                ..Default::default()
            },
            ..Default::default()
        };
        let resolver = MockLineResolver::new(vec![(2, 10, 25)]);
        narrow_search_range(&mut range, &props, Some(&resolver), None);
        // Equal ordering is handled by required_line, not required_line_range
        assert_eq!(range, 0..100);
    }
}
