//! The options vim-core knows: one row each, with the names `:set` takes
//! and whether Vim keeps the option out of reach of untrusted text.

use super::OptionId;

/// An option vim-core knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct KnownOption {
    /// The full name.
    pub(crate) name: &'static str,
    /// The short names.
    pub(crate) short: &'static [&'static str],
    /// Its id, for an option kept in the scoped option layers.
    pub(crate) id: Option<OptionId>,
    /// Vim's `P_SECURE` (optiondefs.h): a modeline and the sandbox cannot
    /// set it.
    pub(crate) secure: bool,
}

const fn row(
    name: &'static str,
    short: &'static [&'static str],
    id: Option<OptionId>,
    secure: bool,
) -> KnownOption {
    KnownOption {
        name,
        short,
        id,
        secure,
    }
}

/// Every option `:set` can name in vim-core, by full name.
#[rustfmt::skip]
pub(crate) static KNOWN_OPTIONS: [KnownOption; 37] = {
    use OptionId as I;
    [
        row("autoindent", &["ai"], Some(I::AutoIndent), false),
        row("backspace", &["bs"], Some(I::Backspace), false),
        row("belloff", &["bo"], Some(I::BellOff), false),
        row("clipboard", &["cb"], Some(I::Clipboard), false),
        row("comments", &["com"], Some(I::Comments), false),
        row("commentstring", &["cms"], Some(I::CommentString), false),
        row("expandtab", &["et"], Some(I::ExpandTab), false),
        row("formatoptions", &["fo"], Some(I::FormatOptions), false),
        row("gdefault", &["gd"], Some(I::GDefault), false),
        row("hlsearch", &["hls"], Some(I::HlSearch), false),
        row("ignorecase", &["ic"], Some(I::IgnoreCase), false),
        row("inccommand", &["icm"], Some(I::IncCommand), false),
        row("incsearch", &["is"], Some(I::IncSearch), false),
        row("iskeyword", &["isk"], Some(I::IsKeyword), false),
        row("langmap", &["lmap"], None, true),
        row("langnoremap", &["lnr"], None, false),
        row("langremap", &["lrm"], None, false),
        row("multilinefind", &["mlf"], None, false),
        row("multilinefindrange", &["mlfr"], None, false),
        row("number", &["nu"], Some(I::Number), false),
        row("relativenumber", &["rnu"], Some(I::RelativeNumber), false),
        row("scrolloff", &["so"], Some(I::ScrollOff), false),
        row("selection", &["sel"], Some(I::Selection), false),
        row("shiftwidth", &["sw"], Some(I::ShiftWidth), false),
        row("sidescrolloff", &["siso"], Some(I::SideScrollOff), false),
        row("smartcase", &["scs"], Some(I::SmartCase), false),
        row("smartindent", &["si"], Some(I::SmartIndent), false),
        row("softtabstop", &["sts"], Some(I::SoftTabStop), false),
        row("tabstop", &["ts"], Some(I::TabStop), false),
        row("textwidth", &["tw"], Some(I::TextWidth), false),
        row("timeoutlen", &["tm"], Some(I::TimeoutLen), false),
        row("undoautogroupms", &["uagm"], Some(I::UndoAutoGroupMs), false),
        row("undolevels", &["ul"], Some(I::UndoLevels), false),
        row("virtualedit", &["ve"], Some(I::VirtualEdit), false),
        row("visualstar", &[], Some(I::VisualStar), false),
        row("whichwrap", &["ww"], Some(I::WhichWrap), false),
        row("wrapscan", &["ws"], Some(I::WrapScan), false),
    ]
};

/// The option vim-core knows by the full or short name `name`, as written:
/// `:set` matches option names with case.
pub(crate) fn known_option(name: &str) -> Option<&'static KnownOption> {
    KNOWN_OPTIONS
        .iter()
        .find(|row| row.name == name || row.short.contains(&name))
}

/// What a name in `:set` or `&name` stands for, as a host judging a config
/// sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OptionName {
    /// The full name, for an option of vim-core or of Vim; `None` for a
    /// name neither has.
    pub(crate) full: Option<&'static str>,
    /// vim-core has the option.
    pub(crate) known: bool,
    /// Vim's `P_SECURE`.
    pub(crate) secure: bool,
}

/// The option `name` names, a full or short name with no `no` or `inv`
/// before it: one of vim-core's, or else one of Vim's table that vim-core
/// lacks.
pub(crate) fn option_name(name: &str) -> OptionName {
    if let Some(row) = known_option(name) {
        return OptionName {
            full: Some(row.name),
            known: true,
            secure: row.secure,
        };
    }
    let vim = super::vim_option_names::OPTIONS
        .iter()
        .find(|row| row.name == name || (!row.short.is_empty() && row.short == name));
    OptionName {
        full: vim.map(|row| row.name),
        known: false,
        secure: vim.is_some_and(|row| row.secure),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::vim_option_names::OPTIONS;

    /// Every [`OptionId`]; [`listed`] makes a new one fail to compile.
    const IDS: [OptionId; 32] = [
        OptionId::IgnoreCase,
        OptionId::SmartCase,
        OptionId::HlSearch,
        OptionId::IncSearch,
        OptionId::WrapScan,
        OptionId::GDefault,
        OptionId::Clipboard,
        OptionId::IncCommand,
        OptionId::TimeoutLen,
        OptionId::UndoLevels,
        OptionId::TabStop,
        OptionId::ShiftWidth,
        OptionId::ExpandTab,
        OptionId::AutoIndent,
        OptionId::SmartIndent,
        OptionId::CommentString,
        OptionId::IsKeyword,
        OptionId::TextWidth,
        OptionId::ScrollOff,
        OptionId::Number,
        OptionId::RelativeNumber,
        OptionId::SideScrollOff,
        OptionId::VirtualEdit,
        OptionId::Selection,
        OptionId::Backspace,
        OptionId::WhichWrap,
        OptionId::VisualStar,
        OptionId::UndoAutoGroupMs,
        OptionId::BellOff,
        OptionId::SoftTabStop,
        OptionId::FormatOptions,
        OptionId::Comments,
    ];

    /// The place of `id` in [`IDS`].
    const fn listed(id: OptionId) -> usize {
        match id {
            OptionId::IgnoreCase => 0,
            OptionId::SmartCase => 1,
            OptionId::HlSearch => 2,
            OptionId::IncSearch => 3,
            OptionId::WrapScan => 4,
            OptionId::GDefault => 5,
            OptionId::Clipboard => 6,
            OptionId::IncCommand => 7,
            OptionId::TimeoutLen => 8,
            OptionId::UndoLevels => 9,
            OptionId::TabStop => 10,
            OptionId::ShiftWidth => 11,
            OptionId::ExpandTab => 12,
            OptionId::AutoIndent => 13,
            OptionId::SmartIndent => 14,
            OptionId::CommentString => 15,
            OptionId::IsKeyword => 16,
            OptionId::TextWidth => 17,
            OptionId::ScrollOff => 18,
            OptionId::Number => 19,
            OptionId::RelativeNumber => 20,
            OptionId::SideScrollOff => 21,
            OptionId::VirtualEdit => 22,
            OptionId::Selection => 23,
            OptionId::Backspace => 24,
            OptionId::WhichWrap => 25,
            OptionId::VisualStar => 26,
            OptionId::UndoAutoGroupMs => 27,
            OptionId::BellOff => 28,
            OptionId::SoftTabStop => 29,
            OptionId::FormatOptions => 30,
            OptionId::Comments => 31,
        }
    }

    #[test]
    fn every_option_id_has_one_row() {
        for id in IDS {
            assert_eq!(IDS[listed(id)], id);
            let rows = KNOWN_OPTIONS
                .iter()
                .filter(|row| row.id == Some(id))
                .count();
            assert_eq!(rows, 1, "{id:?}");
        }
        let with_id = KNOWN_OPTIONS.iter().filter(|row| row.id.is_some()).count();
        assert_eq!(with_id, IDS.len());
    }

    /// The names and `P_SECURE` of Vim's options as optiondefs.h has them.
    #[test]
    fn rows_agree_with_vim_s_table() {
        for row in &KNOWN_OPTIONS {
            match OPTIONS.iter().find(|vim| vim.name == row.name) {
                Some(vim) => {
                    assert_eq!(row.secure, vim.secure, "{}", row.name);
                    assert_eq!(row.short, [vim.short], "{}", row.name);
                }
                None => assert!(!row.secure, "{}", row.name),
            }
        }
    }

    #[test]
    fn names_are_unique_and_in_order() {
        let mut names: Vec<&str> = KNOWN_OPTIONS
            .iter()
            .flat_map(|row| std::iter::once(row.name).chain(row.short.iter().copied()))
            .collect();
        assert!(KNOWN_OPTIONS.windows(2).all(|w| w[0].name < w[1].name));
        let all = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), all);
    }

    #[test]
    fn option_name_reads_vim_s_options_vim_core_lacks() {
        let tw = option_name("tw");
        assert_eq!(
            (tw.full, tw.known, tw.secure),
            (Some("textwidth"), true, false)
        );
        let lmap = option_name("lmap");
        assert_eq!(
            (lmap.full, lmap.known, lmap.secure),
            (Some("langmap"), true, true)
        );
        let sh = option_name("sh");
        assert_eq!((sh.full, sh.known, sh.secure), (Some("shell"), false, true));
        let mouse = option_name("mouse");
        assert_eq!(
            (mouse.full, mouse.known, mouse.secure),
            (Some("mouse"), false, false)
        );
        let nosuch = option_name("nosuch");
        assert_eq!(
            (nosuch.full, nosuch.known, nosuch.secure),
            (None, false, false)
        );
    }
}
