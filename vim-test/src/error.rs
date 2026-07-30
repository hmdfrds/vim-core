//! Terminal rendering with optional ANSI color support.

use std::io::IsTerminal;

/// Terminal renderer with optional ANSI color.
pub struct TerminalRenderer {
    use_color: bool,
}

impl TerminalRenderer {
    /// Detect color support from environment.
    #[must_use]
    pub fn new() -> Self {
        Self {
            use_color: detect_color_support(),
        }
    }

    /// Force color on or off.
    #[must_use]
    pub fn with_color(use_color: bool) -> Self {
        Self { use_color }
    }

    /// Red text (for actual/wrong values).
    #[must_use]
    pub fn red(&self, s: &str) -> String {
        if self.use_color {
            format!("\x1b[31m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }

    /// Green text (for expected/correct values).
    #[must_use]
    pub fn green(&self, s: &str) -> String {
        if self.use_color {
            format!("\x1b[32m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }

    /// Bold text.
    #[must_use]
    pub fn bold(&self, s: &str) -> String {
        if self.use_color {
            format!("\x1b[1m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }

    /// Dim text (for context/explanatory).
    #[must_use]
    pub fn dim(&self, s: &str) -> String {
        if self.use_color {
            format!("\x1b[2m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }

    /// Bold white on red background (for banner).
    #[must_use]
    pub fn banner(&self, s: &str) -> String {
        if self.use_color {
            format!("\x1b[1;37;41m {s} \x1b[0m")
        } else {
            format!("[{s}]")
        }
    }

    /// Whether color is enabled.
    #[must_use]
    pub fn has_color(&self) -> bool {
        self.use_color
    }

    /// Format a line-by-line text diff.
    #[must_use]
    pub fn format_text_diff(&self, expected: &str, actual: &str) -> String {
        let exp_lines: Vec<&str> = expected.lines().collect();
        let act_lines: Vec<&str> = actual.lines().collect();
        let max_lines = exp_lines.len().max(act_lines.len());
        let mut out = String::new();

        for i in 0..max_lines {
            let exp = exp_lines.get(i).copied();
            let act = act_lines.get(i).copied();
            match (exp, act) {
                (Some(e), Some(a)) if e == a => {
                    out.push_str(&format!("    {}: {e:?}\n", i + 1));
                }
                (Some(e), Some(a)) => {
                    out.push_str(&format!(
                        "  {} {}: {e:?}\n  {} {}: {a:?}\n",
                        if self.use_color {
                            "\x1b[32m+\x1b[0m"
                        } else {
                            "+"
                        },
                        i + 1,
                        if self.use_color {
                            "\x1b[31m-\x1b[0m"
                        } else {
                            "-"
                        },
                        i + 1,
                    ));
                }
                (Some(e), None) => {
                    out.push_str(&format!(
                        "  {} {}: {e:?}\n",
                        if self.use_color {
                            "\x1b[32m+\x1b[0m"
                        } else {
                            "+"
                        },
                        i + 1,
                    ));
                }
                (None, Some(a)) => {
                    out.push_str(&format!(
                        "  {} {}: {a:?}\n",
                        if self.use_color {
                            "\x1b[31m-\x1b[0m"
                        } else {
                            "-"
                        },
                        i + 1,
                    ));
                }
                (None, None) => {}
            }
        }
        out
    }
}

fn detect_color_support() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if let Ok(term) = std::env::var("TERM") {
        if term == "dumb" {
            return false;
        }
    }
    std::io::stderr().is_terminal()
}
