/// Maximum characters to scan when searching for a matching bracket.
///
/// Neovim uses a similar internal limit. 100K characters covers ~2,500
/// lines of 40-char code — any reasonable source file. Pathological
/// inputs bail out in ~1ms.
pub const MAX_BRACKET_TRAVEL: usize = 100_000;
