/// Which mode(s) a `:map`/`:noremap`/`:unmap` command targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MapModePrefix {
    /// `:map` — normal + visual + operator-pending.
    All,
    /// `:nmap` — normal mode only.
    Normal,
    /// `:vmap` — visual + select mode.
    Visual,
    /// `:imap` — insert mode only.
    Insert,
    /// `:omap` — operator-pending mode only.
    Operator,
    /// `:cmap` — command-line mode only.
    Command,
    /// `:xmap` — visual-only mode (not select).
    VisualOnly,
    /// `:smap` — select-only mode (not visual).
    SelectOnly,
}

impl MapModePrefix {
    /// All variants, in declaration order.
    pub const ALL: [Self; 8] = [
        Self::All,
        Self::Normal,
        Self::Visual,
        Self::Insert,
        Self::Operator,
        Self::Command,
        Self::VisualOnly,
        Self::SelectOnly,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn map_mode_prefix_all_no_duplicates() {
        let unique: HashSet<MapModePrefix> = MapModePrefix::ALL.iter().copied().collect();
        assert_eq!(
            unique.len(),
            MapModePrefix::ALL.len(),
            "Duplicate in MapModePrefix::ALL"
        );
    }
}
