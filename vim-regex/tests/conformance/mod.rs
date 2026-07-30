//! Neovim conformance test suite.
//!
//! Runs pre-generated corpus data against the vim-regex engine and asserts
//! match results match Neovim's output. The corpus is static JSON committed
//! to version control — CI runs are fast (no Neovim needed at test time).

pub mod corpus;
pub mod oracle;

use corpus::{Corpus, TestCase};
use std::path::PathBuf;
use vim_regex::{MagicMode, MatchContext, VimRegex};

/// Path to the corpus data directory relative to the crate root.
fn corpus_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .join("tests")
        .join("conformance")
        .join("corpus_data")
}

/// Convert a magic mode string to the `MagicMode` enum.
fn parse_magic_mode(s: &str) -> MagicMode {
    match s {
        "magic" => MagicMode::Magic,
        "nomagic" => MagicMode::NoMagic,
        "very_magic" => MagicMode::VeryMagic,
        "very_nomagic" => MagicMode::VeryNoMagic,
        _ => MagicMode::Magic,
    }
}

/// Run a single test case against the vim-regex engine.
fn run_test_case(tc: &TestCase) -> Result<(), String> {
    let magic = parse_magic_mode(&tc.magic_mode);

    let regex = VimRegex::with_magic(&tc.pattern, magic).map_err(|e| {
        format!(
            "Compilation failed for pattern {:?} (mode {:?}): {e}",
            tc.pattern, tc.magic_mode
        )
    })?;

    let ctx = MatchContext::simple(&tc.input);

    let matches = regex.find_all(&ctx).map_err(|e| {
        format!(
            "Engine error for pattern {:?} on input {:?}: {e}",
            tc.pattern, tc.input
        )
    })?;

    let actual_ranges: Vec<(usize, usize)> = matches
        .iter()
        .map(|m| (m.range.start, m.range.end))
        .collect();
    let expected_ranges: Vec<(usize, usize)> = tc
        .expected_matches
        .iter()
        .map(|m| (m.start, m.end))
        .collect();

    if actual_ranges != expected_ranges {
        return Err(format!(
            "Mismatch for pattern {:?} (mode {:?}) on input {:?}:\n  expected: {:?}\n  actual:   {:?}",
            tc.pattern, tc.magic_mode, tc.input, expected_ranges, actual_ranges
        ));
    }

    // Capture-group comparison (only when the case specifies expected captures).
    if !tc.expected_captures.is_empty() {
        // Consistency guard: one capture-row per match, so no case can be
        // silently under-checked due to a malformed corpus entry.
        if tc.expected_captures.len() != matches.len() {
            return Err(format!(
                "Capture-row count mismatch for pattern {:?} (mode {:?}) on input {:?}:\n  \
                 expected_captures rows: {}\n  actual matches: {}",
                tc.pattern,
                tc.magic_mode,
                tc.input,
                tc.expected_captures.len(),
                matches.len()
            ));
        }
        for (mi, m) in matches.iter().enumerate() {
            let expected = &tc.expected_captures[mi];
            for (gi, exp) in expected.iter().enumerate() {
                let group = gi + 1;
                // Non-panicking lookup: a participating group whose range is out
                // of bounds / not on a char boundary yields an explicit Err
                // rather than an opaque slice-index panic.
                let actual: Option<&str> = match m.capture(group) {
                    Some(r) => match tc.input.get(r.clone()) {
                        Some(s) => Some(s),
                        None => {
                            return Err(format!(
                                "Capture range {r:?} out of bounds / not on a char boundary \
                                 for input {:?} (pattern {:?}, mode {:?}, match {mi}, group {group})",
                                tc.input, tc.pattern, tc.magic_mode
                            ));
                        }
                    },
                    None => None,
                };
                let exp_ref: Option<&str> = exp.as_deref();
                if actual != exp_ref {
                    return Err(format!(
                        "Capture mismatch for pattern {:?} (mode {:?}) on input {:?}, \
                         match {mi}, group {group}:\n  expected: {exp_ref:?}\n  actual:   {actual:?}",
                        tc.pattern, tc.magic_mode, tc.input
                    ));
                }
            }
        }
    }

    Ok(())
}

/// Run all corpus test cases. Returns a summary of pass/fail counts.
pub fn run_all() -> (usize, usize, Vec<String>) {
    let corpus = Corpus::load(&corpus_dir());
    let mut passed = 0;
    let mut failed = 0;
    let mut errors = Vec::new();

    for (category, cases) in &corpus.categories {
        for tc in cases {
            match run_test_case(tc) {
                Ok(()) => passed += 1,
                Err(msg) => {
                    failed += 1;
                    errors.push(format!("[{category}] {msg}"));
                }
            }
        }
    }

    (passed, failed, errors)
}
