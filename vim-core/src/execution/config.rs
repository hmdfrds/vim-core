//! Sourcing a config through the host that read it.
//!
//! A host hands [`VimEngine::source_script`](crate::execution::VimEngine::source_script)
//! a [`ConfigText`] and a [`ConfigHost`], which answers what the engine
//! cannot know itself while the config runs.

use crate::execution::{HostResponse, Response};

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
    /// answer itself, such as one the host names after itself. The name
    /// comes in lowercase, as Vim compares feature names without case.
    fn has_feature(&self, _feature: &str) -> bool {
        false
    }

    /// What to do with a `:source` in the config. By default it becomes a
    /// [`HostRequest::ReadConfigFile`](crate::execution::HostRequest::ReadConfigFile)
    /// request, as for a host that reads no file while a config runs.
    fn source(&mut self, _request: &SourceRequest<'_>) -> SourceReply {
        SourceReply::Defer
    }
}

/// A `:source` in a config, for [`ConfigHost::source`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceRequest<'a> {
    /// The file as typed, unexpanded; `None` for a bare `:source`. The host
    /// expands `~`, `$VAR`, `%` and `<sfile>`, which names `from_file`.
    pub path: Option<&'a str>,
    /// The name of the config the `:source` is in.
    pub from_file: &'a str,
    /// The tag of that config.
    pub from_trust: TrustTag,
    /// The line of the `:source`.
    pub line: u32,
    /// How many files would be running with this one: the config the host
    /// passed is 1, a file it sources 2. The file is refused with E169
    /// before it would be 201, so the host is never asked for more than
    /// 200.
    pub depth: u32,
}

/// What [`ConfigHost::source`] answers.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceReply {
    /// The text of the file, which runs now, before the commands after the
    /// `:source`, named `name` in its errors and tagged `trust`.
    Text {
        /// The name its errors are reported under.
        name: String,
        /// The text of the file.
        text: String,
        /// Opaque to vim-core, handed back for the `:source` commands in it.
        trust: TrustTag,
    },
    /// The host sourced the file itself; the message, if any, is shown.
    Done(Option<String>),
    /// The file is not sourced, and the text is shown as the error of the
    /// `:source`, as written, such as `E484: Can't open file b.vim`.
    Refuse(String),
    /// The `:source` becomes a
    /// [`HostRequest::ReadConfigFile`](crate::execution::HostRequest::ReadConfigFile)
    /// request, answered once the config has run.
    Defer,
}

/// What [`HostSession::source_script`](crate::execution::HostSession::source_script)
/// returns.
#[non_exhaustive]
#[derive(Debug)]
pub struct HostConfigResponse {
    /// The session after the config ran, with its messages delivered.
    pub response: HostResponse,
    /// What it came to.
    pub outcome: ConfigOutcome,
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
