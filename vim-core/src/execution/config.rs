//! Sourcing a config through the host that read it.
//!
//! A host hands [`VimEngine::source_script`](crate::execution::VimEngine::source_script)
//! a [`ConfigText`] and a [`ConfigHost`], which answers what the engine
//! cannot know itself while the config runs.

use crate::execution::Response;

/// A config a host has read: its name, its text and a tag of the host's.
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub struct ConfigText<'a> {
    /// The name its errors are reported under, such as `"init.vim"`; `""`
    /// for none.
    pub name: &'a str,
    /// The text of the config.
    pub text: &'a str,
    /// Opaque to vim-core, kept for the host.
    pub trust: TrustTag,
}

impl<'a> ConfigText<'a> {
    /// The config `text` named `name`, with the default [`TrustTag`].
    #[must_use]
    pub const fn new(name: &'a str, text: &'a str) -> Self {
        Self {
            name,
            text,
            trust: TrustTag(0),
        }
    }

    /// The same config with the tag `trust`.
    #[must_use]
    pub const fn with_trust(self, trust: TrustTag) -> Self {
        Self { trust, ..self }
    }
}

/// A tag a host puts on a config, such as how far it trusts the file.
/// vim-core never reads it.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct TrustTag(pub u32);

impl TrustTag {
    /// The tag `n`.
    #[must_use]
    pub const fn new(n: u32) -> Self {
        Self(n)
    }

    /// The number of the tag.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// What the host answers while a config runs.
pub trait ConfigHost {
    /// Whether `has({feature})` is 1 for a feature vim-core does not
    /// answer itself, such as one the host names after itself.
    fn has_feature(&self, _feature: &str) -> bool {
        false
    }
}

/// A host that answers nothing.
pub(crate) struct NoHost;

impl ConfigHost for NoHost {}

/// What running a config came to.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ConfigOutcome {
    /// The errors it showed; those `:silent!` hid are not counted.
    pub errors: u32,
}

/// What [`VimEngine::source_script`](crate::execution::VimEngine::source_script)
/// returns.
#[non_exhaustive]
#[derive(Debug)]
#[must_use = "ConfigResponse contains effects that must be applied to the editor"]
pub struct ConfigResponse {
    /// Its messages, errors and host requests.
    pub response: Response,
    /// What it came to.
    pub outcome: ConfigOutcome,
}
