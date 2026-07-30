/// Regex interpretation mode (Vim's magic levels).
///
/// Controls which characters are treated as regex metacharacters vs literals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum MagicMode {
    /// `\v` — very magic: all ASCII punctuation is special (like Perl regex).
    VeryMagic,
    /// Default Vim mode: `.`, `*`, `[`, `]`, `^`, `$` are special.
    #[default]
    Magic,
    /// `\M` — nomagic: only `^` and `$` are special.
    NoMagic,
    /// `\V` — very nomagic: only `\` is special (literal matching).
    VeryNoMagic,
}
