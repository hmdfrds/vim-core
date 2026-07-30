//! Completion kind for Ctrl-X insert-mode sub-mode.
//!
//! Models Vim's 13 Ctrl-X completion types that are triggered
//! by pressing Ctrl-X followed by a second key in insert mode.

/// The kind of completion requested by Ctrl-X in insert mode.
///
/// Each variant corresponds to a specific Vim Ctrl-X completion type.
/// See `:help ins-completion` for full documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum CompletionKind {
    /// Ctrl-X Ctrl-L — whole line completion.
    Line,
    /// Ctrl-X Ctrl-N — keyword completion (search forward).
    KeywordNext,
    /// Ctrl-X Ctrl-P — keyword completion (search backward).
    KeywordPrev,
    /// Ctrl-X Ctrl-K — dictionary completion.
    Dictionary,
    /// Ctrl-X Ctrl-T — thesaurus completion.
    Thesaurus,
    /// Ctrl-X Ctrl-I — include/path completion.
    IncludePath,
    /// Ctrl-X Ctrl-] — tag completion.
    Tag,
    /// Ctrl-X Ctrl-F — file name completion.
    FileName,
    /// Ctrl-X Ctrl-D — definition/macro completion.
    DefinitionMacro,
    /// Ctrl-X Ctrl-V — Vim command-line completion.
    VimCommand,
    /// Ctrl-X Ctrl-U — user-defined function completion.
    UserDefined,
    /// Ctrl-X Ctrl-O — omni completion.
    Omni,
    /// Ctrl-X Ctrl-S or Ctrl-X s — spelling completion.
    Spelling,
}

impl CompletionKind {
    /// Short display name for this completion kind.
    ///
    /// Returns a human-readable label suitable for status-line display.
    #[must_use]
    pub const fn short_name(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::KeywordNext => "keyword (next)",
            Self::KeywordPrev => "keyword (prev)",
            Self::Dictionary => "dictionary",
            Self::Thesaurus => "thesaurus",
            Self::IncludePath => "include",
            Self::Tag => "tag",
            Self::FileName => "file",
            Self::DefinitionMacro => "definition",
            Self::VimCommand => "vim command",
            Self::UserDefined => "user",
            Self::Omni => "omni",
            Self::Spelling => "spelling",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_variants_have_short_name() {
        let variants = [
            CompletionKind::Line,
            CompletionKind::KeywordNext,
            CompletionKind::KeywordPrev,
            CompletionKind::Dictionary,
            CompletionKind::Thesaurus,
            CompletionKind::IncludePath,
            CompletionKind::Tag,
            CompletionKind::FileName,
            CompletionKind::DefinitionMacro,
            CompletionKind::VimCommand,
            CompletionKind::UserDefined,
            CompletionKind::Omni,
            CompletionKind::Spelling,
        ];
        assert_eq!(
            variants.len(),
            13,
            "expected exactly 13 CompletionKind variants"
        );

        for v in &variants {
            let name = v.short_name();
            assert!(!name.is_empty(), "short_name() must not be empty for {v:?}");
        }
    }

    #[test]
    fn short_name_values() {
        assert_eq!(CompletionKind::Line.short_name(), "line");
        assert_eq!(CompletionKind::KeywordNext.short_name(), "keyword (next)");
        assert_eq!(CompletionKind::KeywordPrev.short_name(), "keyword (prev)");
        assert_eq!(CompletionKind::Dictionary.short_name(), "dictionary");
        assert_eq!(CompletionKind::Thesaurus.short_name(), "thesaurus");
        assert_eq!(CompletionKind::IncludePath.short_name(), "include");
        assert_eq!(CompletionKind::Tag.short_name(), "tag");
        assert_eq!(CompletionKind::FileName.short_name(), "file");
        assert_eq!(CompletionKind::DefinitionMacro.short_name(), "definition");
        assert_eq!(CompletionKind::VimCommand.short_name(), "vim command");
        assert_eq!(CompletionKind::UserDefined.short_name(), "user");
        assert_eq!(CompletionKind::Omni.short_name(), "omni");
        assert_eq!(CompletionKind::Spelling.short_name(), "spelling");
    }

    #[test]
    fn clone_copy_eq_hash() {
        let a = CompletionKind::Omni;
        let b = a; // Copy
        let c = a.clone(); // Clone
        assert_eq!(a, b);
        assert_eq!(a, c);

        // Hash: use in a HashSet
        let mut set = std::collections::HashSet::new();
        set.insert(a);
        assert!(set.contains(&CompletionKind::Omni));
        assert!(!set.contains(&CompletionKind::Line));
    }

    #[test]
    fn debug_format() {
        let dbg = format!("{:?}", CompletionKind::FileName);
        assert_eq!(dbg, "FileName");
    }
}
