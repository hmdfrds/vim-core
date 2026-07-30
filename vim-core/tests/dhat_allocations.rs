//! DHAT Allocation Tracking for vim-core
//!
//! This benchmark tracks heap allocations to ensure low-allocation hot paths.
//!
//! # Usage
//!
//! ```bash
//! # Run allocation tracking with profiling enabled
//! cargo test -p vim-core --test dhat_allocations --features dhat-heap -- --nocapture
//!
//! # View results
//! # Output: dhat-heap.json (viewable at https://nnethercote.github.io/dh_view/dh_view.html)
//! ```
//!
//! # Goals
//!
//! - Parser `process` call: < 5 allocations per keystroke
//! - Grammar state machine: minimal heap usage
//! - Motion computation: zero-allocation where possible

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

use vim_core::grammar::Parser;
use vim_core::keymap::{KeyEvent, Keymap};
use vim_core::primitives::Mode;

/// Profile basic motion key parsing
#[test]
fn allocation_profile_parser_motions() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    let mut parser = Parser::new();
    let keymap = Keymap::default();

    // Warm up and profile basic motions
    let motions = ['h', 'j', 'k', 'l', 'w', 'b', 'e', '0', '$'];
    for _ in 0..10 {
        for &key in &motions {
            let _ = parser.process(KeyEvent::char(key), &keymap, Mode::Normal);
            parser.reset();
        }
    }

    #[cfg(feature = "dhat-heap")]
    {
        let stats = dhat::HeapStats::get();
        eprintln!("\n=== Basic Motion Parser Allocation Stats ===");
        eprintln!("Total blocks: {}", stats.total_blocks);
        eprintln!("Total bytes: {}", stats.total_bytes);
        eprintln!("Max blocks at once: {}", stats.max_blocks);
        eprintln!("Max bytes at once: {}", stats.max_bytes);
        eprintln!("Allocations per motion: ~{}", stats.total_blocks / 90);
        assert_eq!(
            stats.total_blocks, 0,
            "Parser must be zero-allocation. Got {} blocks ({} bytes)",
            stats.total_blocks, stats.total_bytes
        );
    }
}

/// Profile operator + motion combinations (dw, cw, yw)
#[test]
fn allocation_profile_parser_operators() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    let mut parser = Parser::new();
    let keymap = Keymap::default();

    // Operator + motion pairs
    let sequences = [
        ('d', 'w'),
        ('c', 'w'),
        ('y', 'w'),
        ('d', 'e'),
        ('c', 'e'),
        ('y', 'e'),
    ];

    for _ in 0..10 {
        for (op, motion) in sequences {
            let _ = parser.process(KeyEvent::char(op), &keymap, Mode::Normal);
            let _ = parser.process(KeyEvent::char(motion), &keymap, Mode::Normal);
            parser.reset();
        }
    }

    #[cfg(feature = "dhat-heap")]
    {
        let stats = dhat::HeapStats::get();
        eprintln!("\n=== Operator Parser Allocation Stats ===");
        eprintln!("Total blocks: {}", stats.total_blocks);
        eprintln!("Total bytes: {}", stats.total_bytes);
        eprintln!("Allocations per 'dw': ~{}", stats.total_blocks / 60);
        assert_eq!(
            stats.total_blocks, 0,
            "Parser must be zero-allocation. Got {} blocks ({} bytes)",
            stats.total_blocks, stats.total_bytes
        );
    }
}

/// Profile text object parsing (ciw, dap, etc.)
#[test]
fn allocation_profile_parser_textobjects() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    let mut parser = Parser::new();
    let keymap = Keymap::default();

    // Text object sequences
    let sequences = [
        ['c', 'i', 'w'],
        ['d', 'a', 'w'],
        ['y', 'i', '('],
        ['c', 'a', ')'],
        ['d', 'i', 'p'],
    ];

    for _ in 0..10 {
        for seq in &sequences {
            for &key in seq {
                let _ = parser.process(KeyEvent::char(key), &keymap, Mode::Normal);
            }
            parser.reset();
        }
    }

    #[cfg(feature = "dhat-heap")]
    {
        let stats = dhat::HeapStats::get();
        eprintln!("\n=== Text Object Parser Allocation Stats ===");
        eprintln!("Total blocks: {}", stats.total_blocks);
        eprintln!("Total bytes: {}", stats.total_bytes);
        eprintln!("Allocations per text object: ~{}", stats.total_blocks / 50);
        assert_eq!(
            stats.total_blocks, 0,
            "Parser must be zero-allocation. Got {} blocks ({} bytes)",
            stats.total_blocks, stats.total_bytes
        );
    }
}

/// Profile count prefix parsing (10j, 5dw)
#[test]
fn allocation_profile_parser_counts() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    let mut parser = Parser::new();
    let keymap = Keymap::default();

    // Count + motion
    for _ in 0..10 {
        // 10j
        let _ = parser.process(KeyEvent::char('1'), &keymap, Mode::Normal);
        let _ = parser.process(KeyEvent::char('0'), &keymap, Mode::Normal);
        let _ = parser.process(KeyEvent::char('j'), &keymap, Mode::Normal);
        parser.reset();

        // 5dw
        let _ = parser.process(KeyEvent::char('5'), &keymap, Mode::Normal);
        let _ = parser.process(KeyEvent::char('d'), &keymap, Mode::Normal);
        let _ = parser.process(KeyEvent::char('w'), &keymap, Mode::Normal);
        parser.reset();
    }

    #[cfg(feature = "dhat-heap")]
    {
        let stats = dhat::HeapStats::get();
        eprintln!("\n=== Count Parser Allocation Stats ===");
        eprintln!("Total blocks: {}", stats.total_blocks);
        eprintln!("Total bytes: {}", stats.total_bytes);
        eprintln!(
            "Allocations per counted command: ~{}",
            stats.total_blocks / 20
        );
        assert_eq!(
            stats.total_blocks, 0,
            "Parser must be zero-allocation. Got {} blocks ({} bytes)",
            stats.total_blocks, stats.total_bytes
        );
    }
}

/// Profile find motion parsing (fa, Fb, etc.)
#[test]
fn allocation_profile_parser_find() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    let mut parser = Parser::new();
    let keymap = Keymap::default();

    for _ in 0..10 {
        // fa
        let _ = parser.process(KeyEvent::char('f'), &keymap, Mode::Normal);
        let _ = parser.process(KeyEvent::char('a'), &keymap, Mode::Normal);
        parser.reset();

        // Fb
        let _ = parser.process(KeyEvent::char('F'), &keymap, Mode::Normal);
        let _ = parser.process(KeyEvent::char('b'), &keymap, Mode::Normal);
        parser.reset();

        // ta
        let _ = parser.process(KeyEvent::char('t'), &keymap, Mode::Normal);
        let _ = parser.process(KeyEvent::char('a'), &keymap, Mode::Normal);
        parser.reset();
    }

    #[cfg(feature = "dhat-heap")]
    {
        let stats = dhat::HeapStats::get();
        eprintln!("\n=== Find Motion Parser Allocation Stats ===");
        eprintln!("Total blocks: {}", stats.total_blocks);
        eprintln!("Total bytes: {}", stats.total_bytes);
        eprintln!("Allocations per find: ~{}", stats.total_blocks / 30);
        assert_eq!(
            stats.total_blocks, 0,
            "Parser must be zero-allocation. Got {} blocks ({} bytes)",
            stats.total_blocks, stats.total_bytes
        );
    }
}

/// Profile mark operations (ma, 'a, `a)
#[test]
fn allocation_profile_parser_marks() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    let mut parser = Parser::new();
    let keymap = Keymap::default();

    for _ in 0..10 {
        // ma - set mark
        let _ = parser.process(KeyEvent::char('m'), &keymap, Mode::Normal);
        let _ = parser.process(KeyEvent::char('a'), &keymap, Mode::Normal);
        parser.reset();

        // 'a - go to mark line
        let _ = parser.process(KeyEvent::char('\''), &keymap, Mode::Normal);
        let _ = parser.process(KeyEvent::char('a'), &keymap, Mode::Normal);
        parser.reset();

        // `a - go to mark position
        let _ = parser.process(KeyEvent::char('`'), &keymap, Mode::Normal);
        let _ = parser.process(KeyEvent::char('a'), &keymap, Mode::Normal);
        parser.reset();
    }

    #[cfg(feature = "dhat-heap")]
    {
        let stats = dhat::HeapStats::get();
        eprintln!("\n=== Mark Parser Allocation Stats ===");
        eprintln!("Total blocks: {}", stats.total_blocks);
        eprintln!("Total bytes: {}", stats.total_bytes);
        eprintln!("Allocations per mark op: ~{}", stats.total_blocks / 30);
        assert_eq!(
            stats.total_blocks, 0,
            "Parser must be zero-allocation. Got {} blocks ({} bytes)",
            stats.total_blocks, stats.total_bytes
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Engine-Level Allocation Profiles
// ─────────────────────────────────────────────────────────────────────────────

use vim_core::document::Document;
use vim_core::execution::{InputContext, VimEngine};
use vim_core::primitives::{Offset, Position};

/// Minimal Document for dhat tests (same pattern as bench BenchDoc).
struct DhatDoc {
    text: &'static str,
    line_offsets: Vec<usize>,
}

impl DhatDoc {
    fn new(text: &'static str) -> Self {
        let mut offsets = vec![0];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' && i + 1 < text.len() {
                offsets.push(i + 1);
            }
        }
        Self {
            text,
            line_offsets: offsets,
        }
    }
}

impl Document for DhatDoc {
    fn text(&self) -> &str {
        self.text
    }
    fn line_count(&self) -> usize {
        self.line_offsets.len()
    }
    fn offset_to_pos(&self, offset: Offset) -> Option<Position> {
        let off = offset.get();
        if off > self.text.len() {
            return None;
        }
        let line = self
            .line_offsets
            .partition_point(|&o| o <= off)
            .saturating_sub(1);
        let col = off - self.line_offsets[line];
        Some(Position::from_raw(line, col))
    }
    fn pos_to_offset(&self, pos: Position) -> Option<Offset> {
        let start = *self.line_offsets.get(pos.line().get())?;
        Some(Offset::new(start + pos.col().get()))
    }
}

const DHAT_TEXT: &str = "\
fn main() {\n\
    let x = 42;\n\
    let y = (x + 10) * 2;\n\
    println!(\"result: {}\", y);\n\
}\n";

/// Profile full engine pipeline for simple motions (j, w, $).
#[test]
fn allocation_profile_engine_motions() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    let doc = DhatDoc::new(DHAT_TEXT);
    let motions = ['j', 'w', 'b', '$', '0'];

    for _ in 0..10 {
        for &key in &motions {
            let mut engine = VimEngine::new();
            let ctx = InputContext::new(&doc, 5).validate_clamped();
            let _ = engine.process(KeyEvent::char(key), ctx);
        }
    }

    #[cfg(feature = "dhat-heap")]
    {
        const ITERATIONS: u64 = 50; // 10 outer * 5 motions
        let stats = dhat::HeapStats::get();
        eprintln!("\n=== Engine Motion Allocation Stats ===");
        eprintln!("Total blocks: {}", stats.total_blocks);
        eprintln!("Total bytes: {}", stats.total_bytes);
        eprintln!("Max blocks at once: {}", stats.max_blocks);
        eprintln!("Max bytes at once: {}", stats.max_bytes);
        eprintln!(
            "Allocations per engine.process(motion): ~{}",
            stats.total_blocks / ITERATIONS
        );
        let per_key = stats.total_blocks as f64 / ITERATIONS as f64;
        assert!(
            per_key <= 5.0,
            "Engine motion budget exceeded: {:.1} allocs/key (budget: 5.0)",
            per_key
        );
    }
}

/// Profile full engine pipeline for operator+motion combos (dd, dw).
#[test]
fn allocation_profile_engine_operators() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    let doc = DhatDoc::new(DHAT_TEXT);

    for _ in 0..10 {
        // dd
        let mut engine = VimEngine::new();
        let ctx1 = InputContext::new(&doc, 0).validate_clamped();
        let _ = engine.process(KeyEvent::char('d'), ctx1);
        let ctx2 = InputContext::new(&doc, 0).validate_clamped();
        let _ = engine.process(KeyEvent::char('d'), ctx2);

        // dw
        let mut engine = VimEngine::new();
        let ctx1 = InputContext::new(&doc, 0).validate_clamped();
        let _ = engine.process(KeyEvent::char('d'), ctx1);
        let ctx2 = InputContext::new(&doc, 0).validate_clamped();
        let _ = engine.process(KeyEvent::char('w'), ctx2);

        // yy
        let mut engine = VimEngine::new();
        let ctx1 = InputContext::new(&doc, 0).validate_clamped();
        let _ = engine.process(KeyEvent::char('y'), ctx1);
        let ctx2 = InputContext::new(&doc, 0).validate_clamped();
        let _ = engine.process(KeyEvent::char('y'), ctx2);
    }

    #[cfg(feature = "dhat-heap")]
    {
        const ITERATIONS: u64 = 30; // 10 outer * 3 operators (dd, dw, yy)
        let stats = dhat::HeapStats::get();
        eprintln!("\n=== Engine Operator Allocation Stats ===");
        eprintln!("Total blocks: {}", stats.total_blocks);
        eprintln!("Total bytes: {}", stats.total_bytes);
        eprintln!("Max blocks at once: {}", stats.max_blocks);
        eprintln!("Max bytes at once: {}", stats.max_bytes);
        eprintln!(
            "Allocations per operator combo: ~{}",
            stats.total_blocks / ITERATIONS
        );
        let per_op = stats.total_blocks as f64 / ITERATIONS as f64;
        assert!(
            per_op <= 12.0,
            "Engine operator budget exceeded: {:.1} allocs/op (budget: 12.0)",
            per_op
        );
    }
}
