//! Corpus data structures and loading for the conformance test suite.

use std::collections::BTreeMap;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

/// A single test case in the conformance corpus.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct TestCase {
    /// The Vim regex pattern to test.
    pub pattern: String,
    /// The magic mode to use for compilation.
    pub magic_mode: String,
    /// The input text to match against.
    pub input: String,
    /// Expected match ranges (all non-overlapping matches).
    pub expected_matches: Vec<MatchRange>,
    /// Expected capture-group strings: `expected_captures[match_idx][group - 1]`.
    /// `None` = group did not participate; `Some(s)` = captured `s` (may be `""`).
    /// Empty outer vec = captures not checked for this case (range-only).
    #[serde(default)]
    pub expected_captures: Vec<Vec<Option<String>>>,
    /// Source of truth that generated this test case.
    pub source: String,
}

/// A match range with start/end byte offsets.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct MatchRange {
    pub start: usize,
    pub end: usize,
}

impl MatchRange {
    #[allow(dead_code)]
    pub fn to_range(&self) -> Range<usize> {
        self.start..self.end
    }
}

/// The full conformance corpus, organized by category.
#[derive(Debug)]
pub struct Corpus {
    pub categories: BTreeMap<String, Vec<TestCase>>,
}

impl Corpus {
    /// Load all corpus JSON files from the given directory.
    pub fn load(corpus_dir: &Path) -> Self {
        let mut categories = BTreeMap::new();

        if !corpus_dir.exists() {
            panic!("Corpus directory not found: {}", corpus_dir.display());
        }

        let mut entries: Vec<PathBuf> = fs::read_dir(corpus_dir)
            .expect("cannot read corpus dir")
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
            .collect();
        entries.sort();

        for path in entries {
            let category = path.file_stem().unwrap().to_string_lossy().into_owned();
            let content = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
            let cases: Vec<TestCase> = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("cannot parse {}: {e}", path.display()));
            categories.insert(category, cases);
        }

        Self { categories }
    }

    /// Total number of test cases across all categories.
    #[allow(dead_code)]
    pub fn total_cases(&self) -> usize {
        self.categories.values().map(|v| v.len()).sum()
    }
}
