//! Configurable separators and boundary detection for subword motions.

use compact_str::CompactString;

/// Configuration for subword motion boundary detection.
///
/// Controls which characters act as separators between subwords,
/// and whether case transitions (camelCase) and acronym boundaries
/// (XMLParser) are recognized.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SubwordConfig {
    /// Characters that act as subword separators (e.g., `_`, `.`, `-`).
    pub separators: CompactString,
    /// Whether lowercase-to-uppercase transitions are boundaries (camelCase).
    pub detect_case_boundaries: bool,
    /// Whether uppercase-run-to-lowercase transitions are boundaries (XMLParser).
    pub detect_acronyms: bool,
}

impl Default for SubwordConfig {
    fn default() -> Self {
        Self {
            separators: CompactString::new("._-"),
            detect_case_boundaries: true,
            detect_acronyms: true,
        }
    }
}

impl SubwordConfig {
    /// Check if a character is a configured separator.
    #[must_use]
    pub fn is_separator(&self, c: char) -> bool {
        self.separators.contains(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_separators() {
        let cfg = SubwordConfig::default();
        assert_eq!(cfg.separators.as_str(), "._-");
        assert!(cfg.detect_case_boundaries);
        assert!(cfg.detect_acronyms);
    }

    #[test]
    fn is_separator_default() {
        let cfg = SubwordConfig::default();
        assert!(cfg.is_separator('.'));
        assert!(cfg.is_separator('_'));
        assert!(cfg.is_separator('-'));
        assert!(!cfg.is_separator(' '));
        assert!(!cfg.is_separator('a'));
        assert!(!cfg.is_separator('/'));
    }

    #[test]
    fn custom_separators() {
        let cfg = SubwordConfig {
            separators: CompactString::new("_/"),
            detect_case_boundaries: true,
            detect_acronyms: false,
        };
        assert!(cfg.is_separator('_'));
        assert!(cfg.is_separator('/'));
        assert!(!cfg.is_separator('.'));
        assert!(!cfg.is_separator('-'));
    }

    #[test]
    fn empty_separators() {
        let cfg = SubwordConfig {
            separators: CompactString::new(""),
            detect_case_boundaries: true,
            detect_acronyms: true,
        };
        assert!(!cfg.is_separator('_'));
        assert!(!cfg.is_separator('.'));
    }
}
