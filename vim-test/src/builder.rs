//! Fluent builder API for vim-core tests.
//!
//! ```ignore
//! vim("|hello world")
//!     .keys("dw")
//!     .expect_text("|world")
//!     .expect_register('"', "hello ")
//!     .run();
//! ```

use vim_core::primitives::Mode;

use crate::effects::EffectInspector;
use crate::session::TestSession;
use crate::snapshot::{assert_state, ExpectedState};

/// Entry point: create a test builder from single-cursor annotated text.
///
/// ```ignore
/// vim("|hello world").keys("dw").expect_text("|world").run();
/// ```
pub fn vim(annotated: &str) -> VimTestBuilder {
    VimTestBuilder {
        initial: annotated.to_string(),
        multi_cursor: false,
        steps: Vec::new(),
        viewport: None,
        initial_registers: Vec::new(),
        options: Vec::new(),
        auto_pairs: false,
    }
}

/// Entry point: create a test builder from multi-cursor annotated text.
///
/// ```ignore
/// vim_mc("|1hello |2world").keys("dw").expect_text("").run();
/// ```
pub fn vim_mc(annotated: &str) -> VimTestBuilder {
    VimTestBuilder {
        initial: annotated.to_string(),
        multi_cursor: true,
        steps: Vec::new(),
        viewport: None,
        initial_registers: Vec::new(),
        options: Vec::new(),
        auto_pairs: false,
    }
}

/// Entry point for multiline annotated text.
#[macro_export]
macro_rules! vim_block {
    ($($line:expr),+ $(,)?) => {
        $crate::builder::vim(&[$($line),+].join("\n"))
    };
}

/// A step in the test execution queue.
enum TestStep {
    Keys {
        keys: String,
    },
    ExpectText {
        annotated: String,
        label: String,
    },
    ExpectState {
        expected: ExpectedState,
        label: String,
    },
    ExpectMode {
        mode: Mode,
        label: String,
    },
    ExpectRegister {
        name: char,
        content: String,
        label: String,
    },
    ExpectCursorCount {
        count: usize,
        label: String,
    },
    ExpectCursor {
        line: usize,
        col: usize,
        label: String,
    },
    ExpectChangeCount {
        count: usize,
        label: String,
    },
    ExpectEffects {
        kinds: Vec<vim_core::effects::EffectKind>,
        label: String,
    },
    ExpectCursors {
        positions: Vec<(usize, usize)>,
        label: String,
    },
    ExpectNoEffect {
        kind: vim_core::effects::EffectKind,
        label: String,
    },
    AssertAtomic {
        keys: String,
    },
    AssertRoundTrip {
        keys: String,
    },
    Undo {
        count: usize,
    },
    Redo {
        count: usize,
    },
    SetRegister {
        name: char,
        content: String,
    },
    SelectAllOccurrences,
    AddNextMatch,
}

/// Fluent test builder. Accumulates steps, executes on `.run()`.
pub struct VimTestBuilder {
    initial: String,
    multi_cursor: bool,
    steps: Vec<TestStep>,
    viewport: Option<(usize, usize)>,
    initial_registers: Vec<(char, String)>,
    options: Vec<(String, vim_core::primitives::OptionValue)>,
    auto_pairs: bool,
}

impl VimTestBuilder {
    /// Feed a Vim notation key sequence.
    pub fn keys(&mut self, keys: &str) -> &mut Self {
        self.steps.push(TestStep::Keys {
            keys: keys.to_string(),
        });
        self
    }

    /// Assert text and cursor position using annotated text.
    pub fn expect_text(&mut self, annotated: &str) -> &mut Self {
        self.steps.push(TestStep::ExpectText {
            annotated: annotated.to_string(),
            label: String::new(),
        });
        self
    }

    /// Assert current mode.
    pub fn expect_mode(&mut self, mode: Mode) -> &mut Self {
        self.steps.push(TestStep::ExpectMode {
            mode,
            label: String::new(),
        });
        self
    }

    /// Assert register contents.
    pub fn expect_register(&mut self, name: char, content: &str) -> &mut Self {
        self.steps.push(TestStep::ExpectRegister {
            name,
            content: content.to_string(),
            label: String::new(),
        });
        self
    }

    /// Assert cursor count.
    pub fn expect_cursor_count(&mut self, count: usize) -> &mut Self {
        self.steps.push(TestStep::ExpectCursorCount {
            count,
            label: String::new(),
        });
        self
    }

    /// Assert cursor position as `(line, col)`.
    pub fn expect_cursor(&mut self, line: usize, col: usize) -> &mut Self {
        self.steps.push(TestStep::ExpectCursor {
            line,
            col,
            label: String::new(),
        });
        self
    }

    /// Assert number of committed undo groups.
    pub fn expect_change_count(&mut self, count: usize) -> &mut Self {
        self.steps.push(TestStep::ExpectChangeCount {
            count,
            label: String::new(),
        });
        self
    }

    /// Assert with a full `ExpectedState` (for multi-field assertions).
    pub fn expect(&mut self, expected: ExpectedState) -> &mut Self {
        self.steps.push(TestStep::ExpectState {
            expected,
            label: String::new(),
        });
        self
    }

    /// Feed `u` key `count` times.
    pub fn undo(&mut self, count: usize) -> &mut Self {
        self.steps.push(TestStep::Undo { count });
        self
    }

    /// Feed `<C-r>` key `count` times.
    pub fn redo(&mut self, count: usize) -> &mut Self {
        self.steps.push(TestStep::Redo { count });
        self
    }

    /// Assert effect kinds from the last keystroke.
    pub fn expect_effects(&mut self, kinds: &[vim_core::effects::EffectKind]) -> &mut Self {
        self.steps.push(TestStep::ExpectEffects {
            kinds: kinds.to_vec(),
            label: String::new(),
        });
        self
    }

    /// Assert all cursor positions as `(line, col)` pairs.
    pub fn expect_cursors(&mut self, positions: &[(usize, usize)]) -> &mut Self {
        self.steps.push(TestStep::ExpectCursors {
            positions: positions.to_vec(),
            label: String::new(),
        });
        self
    }

    /// Pre-populate a register before execution.
    pub fn with_register(&mut self, name: char, content: &str) -> &mut Self {
        self.initial_registers.push((name, content.to_string()));
        self
    }

    /// Set viewport dimensions before execution.
    pub fn with_viewport(&mut self, height: usize, width: usize) -> &mut Self {
        self.viewport = Some((height, width));
        self
    }

    /// Assert no effects of a specific kind from the last keystroke.
    pub fn expect_no_effect(&mut self, kind: vim_core::effects::EffectKind) -> &mut Self {
        self.steps.push(TestStep::ExpectNoEffect {
            kind,
            label: String::new(),
        });
        self
    }

    /// Assert the next key sequence is atomic (one undo group).
    pub fn assert_atomic(&mut self, keys: &str) -> &mut Self {
        self.steps.push(TestStep::AssertAtomic {
            keys: keys.to_string(),
        });
        self
    }

    /// Assert a full undo/redo round-trip for the next key sequence.
    pub fn assert_round_trip(&mut self, keys: &str) -> &mut Self {
        self.steps.push(TestStep::AssertRoundTrip {
            keys: keys.to_string(),
        });
        self
    }

    /// Select all occurrences of the word under cursor (Ctrl+D all).
    pub fn select_all_occurrences(&mut self) -> &mut Self {
        self.steps.push(TestStep::SelectAllOccurrences);
        self
    }

    /// Add next match of word under cursor (Ctrl+D single).
    pub fn add_next_match(&mut self) -> &mut Self {
        self.steps.push(TestStep::AddNextMatch);
        self
    }

    /// Set a register mid-test.
    pub fn set_register(&mut self, name: char, content: &str) -> &mut Self {
        self.steps.push(TestStep::SetRegister {
            name,
            content: content.to_string(),
        });
        self
    }

    /// Enable auto-pairs with default bracket/quote pairs: `()`, `[]`, `{}`, `''`, `""`, `` `` ``.
    pub fn with_auto_pairs(&mut self) -> &mut Self {
        self.auto_pairs = true;
        self
    }

    /// Set a vim option before execution.
    ///
    /// Accepts any `OptionValue` variant (Bool, Unsigned, Signed, Str).
    /// Uses `VimOptions::set_option()` with the corresponding `OptionId`.
    /// Set a vim option before execution.
    ///
    /// For numeric options: `with_option("scrolloff", OptionValue::Unsigned(5))`
    /// For boolean options: `with_option("ignorecase", OptionValue::Bool(true))`
    pub fn with_option(
        &mut self,
        name: &str,
        value: vim_core::primitives::OptionValue,
    ) -> &mut Self {
        self.options.push((name.to_string(), value));
        self
    }

    /// Label the most recently added step (appears in error messages).
    pub fn labeled(&mut self, label: &str) -> &mut Self {
        if let Some(step) = self.steps.last_mut() {
            match step {
                TestStep::ExpectText { label: l, .. }
                | TestStep::ExpectState { label: l, .. }
                | TestStep::ExpectMode { label: l, .. }
                | TestStep::ExpectRegister { label: l, .. }
                | TestStep::ExpectCursorCount { label: l, .. }
                | TestStep::ExpectCursor { label: l, .. }
                | TestStep::ExpectChangeCount { label: l, .. }
                | TestStep::ExpectEffects { label: l, .. }
                | TestStep::ExpectCursors { label: l, .. }
                | TestStep::ExpectNoEffect { label: l, .. } => {
                    *l = label.to_string();
                }
                TestStep::Keys { .. }
                | TestStep::Undo { .. }
                | TestStep::Redo { .. }
                | TestStep::SetRegister { .. }
                | TestStep::AssertAtomic { .. }
                | TestStep::AssertRoundTrip { .. }
                | TestStep::SelectAllOccurrences
                | TestStep::AddNextMatch => {}
            }
        }
        self
    }

    /// Execute all steps and assert expectations.
    #[track_caller]
    pub fn run(&mut self) {
        self.execute_steps();
    }

    /// Execute all steps and return the `TestSession` for further inspection.
    #[track_caller]
    pub fn run_session(&mut self) -> TestSession {
        self.execute_steps()
    }

    fn execute_steps(&mut self) -> TestSession {
        let mut session = if self.multi_cursor {
            TestSession::new_multi(&self.initial)
        } else {
            TestSession::new(&self.initial)
        };

        if let Some((height, width)) = self.viewport {
            session
                .session_mut()
                .set_viewport(vim_core::dispatch::ViewportInfo {
                    first_line: 0,
                    height,
                    width,
                });
        }

        for (name, value) in &self.options {
            let id = option_name_to_id(name);
            session.session_mut().options_mut().set_option(id, value);
        }

        if self.auto_pairs {
            session
                .session_mut()
                .options_mut()
                .set_auto_pairs(Some(vim_core::primitives::AutoPairs::default()));
        }

        for (name, content) in &self.initial_registers {
            session.session_mut().set_register(
                *name,
                content,
                vim_core::primitives::MotionType::CharWise,
            );
        }

        for (step_num, step) in self.steps.iter().enumerate() {
            let step_num = step_num + 1;
            match step {
                TestStep::Keys { keys } => {
                    session.feed(keys);
                }
                TestStep::ExpectText { annotated, label } => {
                    let l = make_label(step_num, label, "expect_text");
                    assert_state(&session, ExpectedState::EMPTY.text(annotated), &l);
                }
                TestStep::ExpectState { expected, label } => {
                    let l = make_label(step_num, label, "expect");
                    assert_state(&session, expected.clone(), &l);
                }
                TestStep::ExpectMode { mode, label } => {
                    let l = make_label(step_num, label, "expect_mode");
                    assert_state(&session, ExpectedState::EMPTY.mode(*mode), &l);
                }
                TestStep::ExpectRegister {
                    name,
                    content,
                    label,
                } => {
                    let l = make_label(step_num, label, &format!("expect_register('{name}')"));
                    assert_state(
                        &session,
                        ExpectedState::EMPTY.register(*name, content.clone()),
                        &l,
                    );
                }
                TestStep::ExpectCursorCount { count, label } => {
                    let l = make_label(step_num, label, "expect_cursor_count");
                    assert_state(&session, ExpectedState::EMPTY.cursor_count(*count), &l);
                }
                TestStep::ExpectCursor { line, col, label } => {
                    let l = make_label(step_num, label, "expect_cursor");
                    assert_state(&session, ExpectedState::EMPTY.cursor(*line, *col), &l);
                }
                TestStep::ExpectChangeCount { count, label } => {
                    let l = make_label(step_num, label, "expect_change_count");
                    assert_state(&session, ExpectedState::EMPTY.change_count(*count), &l);
                }
                TestStep::ExpectEffects { kinds, label } => {
                    let l = make_label(step_num, label, "expect_effects");
                    assert_state(&session, ExpectedState::EMPTY.effects(kinds.clone()), &l);
                }
                TestStep::ExpectCursors { positions, label } => {
                    let l = make_label(step_num, label, "expect_cursors");
                    assert_state(
                        &session,
                        ExpectedState::EMPTY.cursor_positions(positions.clone()),
                        &l,
                    );
                }
                TestStep::ExpectNoEffect { kind, label } => {
                    let l = make_label(step_num, label, "expect_no_effect");
                    let inspector = EffectInspector::new(session.last_effects());
                    let count = inspector.of_kind(*kind).count();
                    assert!(
                        count == 0,
                        "ASSERTION FAILED: {l}\n\
                         \x20 expected no {kind:?} effects, found {count}"
                    );
                }
                TestStep::AssertAtomic { keys } => {
                    crate::undo::assert_atomic(&mut session, keys);
                }
                TestStep::AssertRoundTrip { keys } => {
                    crate::undo::assert_round_trip(&mut session, keys);
                }
                TestStep::Undo { count } => {
                    for _ in 0..*count {
                        session.feed("u");
                    }
                }
                TestStep::Redo { count } => {
                    for _ in 0..*count {
                        session.feed("<C-r>");
                    }
                }
                TestStep::SetRegister { name, content } => {
                    session.session_mut().set_register(
                        *name,
                        content,
                        vim_core::primitives::MotionType::CharWise,
                    );
                }
                TestStep::SelectAllOccurrences => {
                    session.select_all_occurrences();
                }
                TestStep::AddNextMatch => {
                    session.add_next_match();
                }
            }
        }
        session
    }
}

fn make_label(step_num: usize, label: &str, default: &str) -> String {
    if label.is_empty() {
        format!("step {step_num}: {default}")
    } else {
        format!("step {step_num}: {label}")
    }
}

fn option_name_to_id(name: &str) -> vim_core::primitives::OptionId {
    use vim_core::primitives::OptionId;
    match name {
        "scrolloff" => OptionId::ScrollOff,
        "tabstop" => OptionId::TabStop,
        "shiftwidth" => OptionId::ShiftWidth,
        "expandtab" => OptionId::ExpandTab,
        "ignorecase" => OptionId::IgnoreCase,
        "smartcase" => OptionId::SmartCase,
        "hlsearch" => OptionId::HlSearch,
        "incsearch" => OptionId::IncSearch,
        "wrapscan" => OptionId::WrapScan,
        "number" => OptionId::Number,
        "relativenumber" => OptionId::RelativeNumber,
        "autoindent" => OptionId::AutoIndent,
        "smartindent" => OptionId::SmartIndent,
        "clipboard" => OptionId::Clipboard,
        "undolevels" => OptionId::UndoLevels,
        other => panic!("unknown option: {other}"),
    }
}
