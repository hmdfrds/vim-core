//! Which-key popup data types.

use crate::VimString;

/// A single key hint for the which-key popup.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct KeyHint {
    /// Display representation of the key (e.g., `"d"`, `"Ctrl-A"`, `"<Space>"`).
    pub key: VimString,
    /// Human-readable description (e.g., "Go to definition").
    pub description: VimString,
}

/// Complete which-key popup data.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct KeyHintsInfo {
    /// Title for the popup (e.g., "Goto / Misc (g)", "Window (Ctrl-W)").
    pub title: VimString,
    /// Available continuation keys with descriptions.
    pub hints: Vec<KeyHint>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_hint_construction() {
        let hint = KeyHint {
            key: VimString::from("d"),
            description: VimString::from("Go to definition"),
        };
        assert_eq!(hint.key, "d");
        assert_eq!(hint.description, "Go to definition");
    }

    #[test]
    fn key_hints_info_construction() {
        let info = KeyHintsInfo {
            title: VimString::from("Goto / Misc (g)"),
            hints: vec![
                KeyHint {
                    key: VimString::from("g"),
                    description: VimString::from("Go to first line"),
                },
                KeyHint {
                    key: VimString::from("d"),
                    description: VimString::from("Go to definition"),
                },
            ],
        };
        assert_eq!(info.title, "Goto / Misc (g)");
        assert_eq!(info.hints.len(), 2);
    }

    #[test]
    fn key_hints_info_empty() {
        let info = KeyHintsInfo {
            title: VimString::from("Empty"),
            hints: vec![],
        };
        assert!(info.hints.is_empty());
    }
}
