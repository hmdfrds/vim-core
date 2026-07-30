use std::fmt;

use compact_str::CompactString;

/// Semantic category for informational messages.
///
/// Hosts use this to route messages to different UI elements (e.g.,
/// `Echo` to the command line, `SearchCount` to a search indicator,
/// `FileInfo` to the title bar).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum MessageKind {
    /// Generic echo (`:echo`, `:echomsg`).
    #[default]
    Echo,
    /// `:echoerr`-style (non-error info that looks like one).
    EchoMsg,
    /// Search match count display ("3/17 matches").
    SearchCount,
    /// File identification (Ctrl-G, `:file`).
    FileInfo,
    /// Mode indicator text ("-- INSERT --").
    ModeInfo,
    /// Partial-command indicator ("2d" in status bar).
    ShowCmd,
    /// Quickfix/location list summary (":cn", ":lopen").
    QuickfixInfo,
    /// Prompt / press-enter display.
    Return,
}

/// Structured informational message for the host.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum InfoMessage {
    /// Line modification report. Structured so hosts can localize or style.
    LineReport(LineModCounts),
    /// Verbose-only: shown only when the host's verbosity is high.
    Verbose(CompactString),
    /// Free-text informational message.
    Text(CompactString),
    /// Categorized text message with a semantic kind for host routing.
    Categorized {
        /// The message text.
        text: CompactString,
        /// Semantic routing hint.
        kind: MessageKind,
    },
}

/// Structured line modification counts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LineModCounts {
    /// Lines added.
    pub added: u32,
    /// Lines deleted.
    pub deleted: u32,
    /// Lines changed (substituted, filtered, etc.).
    pub changed: u32,
    /// Lines yanked.
    pub yanked: u32,
    /// Lines joined.
    pub joined: u32,
    /// Lines moved (`:move`).
    pub moved: u32,
    /// Lines shifted (`>`, `<`).
    pub shifted: u32,
}

impl LineModCounts {
    /// Sum of all modification counts.
    #[inline]
    #[must_use]
    pub const fn total(&self) -> u32 {
        self.added
            + self.deleted
            + self.changed
            + self.yanked
            + self.joined
            + self.moved
            + self.shifted
    }

    /// Returns `true` if all counts are zero.
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.total() == 0
    }

    /// Add another set of counts into this one.
    pub const fn merge(&mut self, other: &Self) {
        self.added += other.added;
        self.deleted += other.deleted;
        self.changed += other.changed;
        self.yanked += other.yanked;
        self.joined += other.joined;
        self.moved += other.moved;
        self.shifted += other.shifted;
    }
}

impl fmt::Display for LineModCounts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;

        // Helper: writes a semicolon separator before every segment except the first.
        let mut sep = |f: &mut fmt::Formatter<'_>| -> fmt::Result {
            if first {
                first = false;
                Ok(())
            } else {
                f.write_str("; ")
            }
        };

        let plural = |n: u32| if n == 1 { "line" } else { "lines" };

        // "added" → "N more line(s)"
        if self.added > 0 {
            sep(f)?;
            write!(f, "{} more {}", self.added, plural(self.added))?;
        }
        // "deleted" → "N fewer line(s)"
        if self.deleted > 0 {
            sep(f)?;
            write!(f, "{} fewer {}", self.deleted, plural(self.deleted))?;
        }
        // The remaining fields all use "N line(s) VERB" form.
        for (count, verb) in [
            (self.changed, "changed"),
            (self.yanked, "yanked"),
            (self.joined, "joined"),
            (self.moved, "moved"),
            (self.shifted, "shifted"),
        ] {
            if count > 0 {
                sep(f)?;
                write!(f, "{} {} {}", count, plural(count), verb)?;
            }
        }

        Ok(())
    }
}

impl fmt::Display for InfoMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InfoMessage::LineReport(counts) => counts.fmt(f),
            InfoMessage::Verbose(text) | InfoMessage::Text(text) => f.write_str(text),
            InfoMessage::Categorized { text, .. } => f.write_str(text),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_mod_counts_total() {
        let counts = LineModCounts {
            added: 1,
            deleted: 2,
            changed: 3,
            yanked: 4,
            joined: 5,
            moved: 6,
            shifted: 7,
        };
        assert_eq!(counts.total(), 28);
    }

    #[test]
    fn line_mod_counts_empty() {
        let counts = LineModCounts::default();
        assert!(counts.is_empty());
        assert_eq!(counts.total(), 0);
    }

    #[test]
    fn message_kind_default_is_echo() {
        assert_eq!(MessageKind::default(), MessageKind::Echo);
    }

    #[test]
    fn message_kind_all_variants_constructable() {
        let kinds = [
            MessageKind::Echo,
            MessageKind::EchoMsg,
            MessageKind::SearchCount,
            MessageKind::FileInfo,
            MessageKind::ModeInfo,
            MessageKind::ShowCmd,
            MessageKind::QuickfixInfo,
            MessageKind::Return,
        ];
        // All distinct.
        for (i, a) in kinds.iter().enumerate() {
            for (j, b) in kinds.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }

    #[test]
    fn message_kind_debug() {
        let debug = format!("{:?}", MessageKind::SearchCount);
        assert!(debug.contains("SearchCount"));
    }

    #[test]
    fn categorized_info_message() {
        let msg = InfoMessage::Categorized {
            text: CompactString::from("3/17"),
            kind: MessageKind::SearchCount,
        };
        if let InfoMessage::Categorized { text, kind } = &msg {
            assert_eq!(text.as_str(), "3/17");
            assert_eq!(*kind, MessageKind::SearchCount);
        } else {
            panic!("expected Categorized variant");
        }
    }

    #[test]
    fn line_mod_counts_merge() {
        let mut a = LineModCounts {
            added: 1,
            deleted: 2,
            ..Default::default()
        };
        let b = LineModCounts {
            added: 3,
            yanked: 5,
            ..Default::default()
        };
        a.merge(&b);
        assert_eq!(a.added, 4);
        assert_eq!(a.deleted, 2);
        assert_eq!(a.yanked, 5);
        assert_eq!(a.total(), 11);
    }

    // ── Display impl tests ──────────────────────────────────────────

    #[test]
    fn display_line_mod_counts_single_yanked_plural() {
        let c = LineModCounts {
            yanked: 3,
            ..Default::default()
        };
        assert_eq!(c.to_string(), "3 lines yanked");
    }

    #[test]
    fn display_line_mod_counts_single_yanked_singular() {
        let c = LineModCounts {
            yanked: 1,
            ..Default::default()
        };
        assert_eq!(c.to_string(), "1 line yanked");
    }

    #[test]
    fn display_line_mod_counts_single_added_plural() {
        let c = LineModCounts {
            added: 5,
            ..Default::default()
        };
        assert_eq!(c.to_string(), "5 more lines");
    }

    #[test]
    fn display_line_mod_counts_single_deleted_plural() {
        let c = LineModCounts {
            deleted: 2,
            ..Default::default()
        };
        assert_eq!(c.to_string(), "2 fewer lines");
    }

    #[test]
    fn display_line_mod_counts_single_deleted_singular() {
        let c = LineModCounts {
            deleted: 1,
            ..Default::default()
        };
        assert_eq!(c.to_string(), "1 fewer line");
    }

    #[test]
    fn display_line_mod_counts_single_changed_plural() {
        let c = LineModCounts {
            changed: 4,
            ..Default::default()
        };
        assert_eq!(c.to_string(), "4 lines changed");
    }

    #[test]
    fn display_line_mod_counts_single_joined() {
        let c = LineModCounts {
            joined: 2,
            ..Default::default()
        };
        assert_eq!(c.to_string(), "2 lines joined");
    }

    #[test]
    fn display_line_mod_counts_single_moved() {
        let c = LineModCounts {
            moved: 1,
            ..Default::default()
        };
        assert_eq!(c.to_string(), "1 line moved");
    }

    #[test]
    fn display_line_mod_counts_single_shifted() {
        let c = LineModCounts {
            shifted: 3,
            ..Default::default()
        };
        assert_eq!(c.to_string(), "3 lines shifted");
    }

    #[test]
    fn display_line_mod_counts_multiple_fields() {
        let c = LineModCounts {
            changed: 3,
            yanked: 1,
            ..Default::default()
        };
        assert_eq!(c.to_string(), "3 lines changed; 1 line yanked");
    }

    #[test]
    fn display_line_mod_counts_all_fields() {
        let c = LineModCounts {
            added: 1,
            deleted: 2,
            changed: 3,
            yanked: 4,
            joined: 5,
            moved: 6,
            shifted: 7,
        };
        assert_eq!(
            c.to_string(),
            "1 more line; 2 fewer lines; 3 lines changed; 4 lines yanked; 5 lines joined; 6 lines moved; 7 lines shifted"
        );
    }

    #[test]
    fn display_line_mod_counts_empty() {
        let c = LineModCounts::default();
        assert_eq!(c.to_string(), "");
    }

    #[test]
    fn display_info_message_text() {
        let msg = InfoMessage::Text(CompactString::from("hello world"));
        assert_eq!(msg.to_string(), "hello world");
    }

    #[test]
    fn display_info_message_verbose() {
        let msg = InfoMessage::Verbose(CompactString::from("verbose info"));
        assert_eq!(msg.to_string(), "verbose info");
    }

    #[test]
    fn display_info_message_categorized() {
        let msg = InfoMessage::Categorized {
            text: CompactString::from("3/17"),
            kind: MessageKind::SearchCount,
        };
        assert_eq!(msg.to_string(), "3/17");
    }

    #[test]
    fn display_info_message_line_report() {
        let msg = InfoMessage::LineReport(LineModCounts {
            yanked: 3,
            ..Default::default()
        });
        assert_eq!(msg.to_string(), "3 lines yanked");
    }
}
