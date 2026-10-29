//! `VimError` enum — typed error variants with Vim error codes.
//!
//! Every variant maps to a real Vim error code (`:help error-messages`).
//! The `Display` impl produces the canonical `E{N}: {message}` format
//! that Vim displays in the status bar.
//!
//! **Exception**: `HostFailure` passes through unstructured host error
//! strings verbatim — these originate outside the engine and have no
//! Vim error code.
//!
//! # Usage
//!
//! Errors flow through the system as `Effect::ShowError { error: VimError }`.
//! The host reads `error.to_string()` to display in the status bar.
//!
//! # Adding New Errors
//!
//! 1. Add variant here with a doc comment showing the Vim error code
//! 2. Add the `Display` match arm with the `E{N}: {message}` format
//! 3. Add a unit test in the `tests` module verifying the Display output

use compact_str::CompactString;
use std::fmt;

/// Errors that can occur in vim-core.
///
/// Each variant carries a Vim error code in its Display representation,
/// matching real Vim/Neovim error behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum VimError {
    // ═══════════════════════════════════════════════════════════════════
    // Search errors
    // ═══════════════════════════════════════════════════════════════════
    /// E486: Pattern not found.
    PatternNotFound(CompactString),

    /// E35: No previous regular expression.
    NoPreviousPattern,

    /// E33: No previous substitute regular expression.
    NoPreviousSubstitute,

    /// E348: No string under cursor.
    NoStringUnderCursor,

    // ═══════════════════════════════════════════════════════════════════
    // Register errors
    // ═══════════════════════════════════════════════════════════════════
    /// E353: Nothing in register {0}.
    NothingInRegister(char),

    /// E748: No previously used register.
    NoPreviousRegister,

    /// E354: Invalid register name: '{0}'.
    InvalidRegisterName(CompactString),

    // ═══════════════════════════════════════════════════════════════════
    // Mark errors
    // ═══════════════════════════════════════════════════════════════════
    /// E20: Mark not set.
    MarkNotSet(char),

    // ═══════════════════════════════════════════════════════════════════
    // Changelist / jump list errors
    // ═══════════════════════════════════════════════════════════════════
    /// E662: At start of changelist.
    AtStartOfChangelist,

    /// E663: At end of changelist.
    AtEndOfChangelist,

    /// E664: Changelist is empty.
    ChangelistEmpty,

    /// E790: undojoin is not allowed after undo.
    E790,

    // ═══════════════════════════════════════════════════════════════════
    // Ex command errors
    // ═══════════════════════════════════════════════════════════════════
    /// E492: Not an editor command.
    NotEditorCommand(CompactString),

    /// E523: Not allowed here, such as a command that needs a buffer in a
    /// config file. Carries the command.
    NotAllowedHere(CompactString),

    /// E14: Invalid address.
    InvalidAddress(CompactString),

    /// E16: Invalid range.
    InvalidRange,

    /// E471: Argument required.
    ArgumentRequired,

    /// E493: Backwards range given.
    BackwardsRange,

    /// E134: Cannot move a range of lines into itself.
    MoveIntoItself,

    /// E521: Invalid argument (number required, wrong type, etc.).
    InvalidArgument(CompactString),

    /// `:set` rejected an option value (E474, E487, E488, E521, E524,
    /// E525, E539).
    ///
    /// Carries Vim's whole message, code and argument included, because the
    /// code depends on the option: `E539: Illegal character <Z>: fo=Z`.
    OptionValue(CompactString),

    /// E518: `:set` names an option that does not exist. Carries the
    /// argument as written, such as `nosuch=1`.
    UnknownOptionArgument(Box<str>),

    /// E519: `:set` asks for the value of an option this build lacks.
    /// Carries the argument as written, such as `mouse?`.
    OptionNotSupported(Box<str>),

    /// E474: a map or abbreviation command with an argument it does not
    /// take, such as `:mapclear x`.
    InvalidMapArgument,

    /// E225: `<buffer><unique>` for keys mapped in every buffer. Carries
    /// the keys.
    GlobalMappingExists(Box<str>),

    /// E226: `<unique>` for an abbreviation that is defined. Carries its
    /// trigger.
    AbbreviationExists(Box<str>),

    /// E227: `<unique>` for keys that are mapped. Carries the keys.
    MappingExists(Box<str>),

    /// E31: an unmap command found no mapping to remove.
    NoSuchMapping,

    /// E24: an unabbreviate command found no abbreviation to remove.
    NoSuchAbbreviation,

    /// E34: No previous command.
    NoPreviousCommand,

    /// E476: No command after g/v (global/vglobal).
    NoCommandAfterGlobal,

    /// E5100: :global command exceeded maximum line count.
    GlobalLineLimitExceeded {
        /// The configured limit that was exceeded.
        limit: usize,
    },

    /// E5101: :global recursion depth limit exceeded.
    GlobalRecursionLimitExceeded {
        /// The maximum allowed recursion depth.
        limit: u8,
    },

    /// E146: Regular expressions can't be delimited.
    InvalidRegexDelimiter,

    /// E682: Invalid search pattern or delimiter.
    InvalidSearchPattern,

    /// E476: Invalid command (e.g. empty pattern in `:s` with no previous).
    InvalidCommand,

    /// E481: No range allowed.
    NoRangeAllowed,

    /// E319: a form of a Vim command this engine does not implement.
    ///
    /// Carries the form, such as `source!`, where Vim shows the typed
    /// command line.
    NotAvailable(CompactString),

    /// E168: `:finish` used outside of a sourced file.
    FinishOutsideSource,

    /// E488: Trailing characters, such as text after the flags of `:s`.
    /// Carries the text.
    TrailingCharacters(Box<str>),

    // ═══════════════════════════════════════════════════════════════════
    // Sourced file errors
    // ═══════════════════════════════════════════════════════════════════
    /// W15: a line of a file whose first line ends in CR LF ends in LF
    /// alone. Vim gives this warning as an error.
    WrongLineSeparator,

    /// E126: a `:function` body has no `:endfunction` before the end of
    /// the file.
    MissingEndfunction,

    /// E172: a `:let` heredoc has no marker.
    MissingMarker,

    /// E221: the marker of a `:let` heredoc starts with a lowercase
    /// letter.
    LowercaseMarker,

    /// E990: a heredoc has no end marker before the end of the file.
    /// Carries the marker.
    MissingEndMarker(Box<str>),

    /// E1026: a `{` block of `:command` or `:autocmd` has no `}` before
    /// the end of the file.
    MissingBrace,

    /// E1057: a `:def` body has no `:enddef` before the end of the file.
    MissingEnddef,

    /// E1058: functions nest deeper than 50.
    FunctionNestingTooDeep,

    /// E1145: the file ends inside a heredoc or `:append` text of a
    /// function body. Carries the marker.
    MissingHeredocEndMarker(Box<str>),

    /// E1171: the file ends inside a `{` block of a function body.
    MissingEndBlock,

    // ═══════════════════════════════════════════════════════════════════
    // Script block errors
    // ═══════════════════════════════════════════════════════════════════
    /// E170: a `:while` has no `:endwhile`.
    MissingEndwhile,

    /// E170: a `:for` has no `:endfor`.
    MissingEndfor,

    /// E171: an `:if` has no `:endif`.
    MissingEndif,

    /// E477: a `!` after a command that takes none.
    NoBangAllowed,

    /// E579: `:if` inside 50 open blocks.
    IfNestingTooDeep,

    /// E580: `:endif` without `:if`.
    EndifWithoutIf,

    /// E581: `:else` without `:if`.
    ElseWithoutIf,

    /// E582: `:elseif` without `:if`.
    ElseifWithoutIf,

    /// E583: a second `:else` for one `:if`.
    MultipleElse,

    /// E584: `:elseif` after `:else`.
    ElseifAfterElse,

    /// E585: `:while` or `:for` inside 50 open blocks.
    LoopNestingTooDeep,

    /// E586: `:continue` outside a loop.
    ContinueWithoutLoop,

    /// E587: `:break` outside a loop.
    BreakWithoutLoop,

    /// E588: `:endwhile` without `:while`.
    EndwhileWithoutWhile,

    /// E588: `:endfor` without `:for`.
    EndforWithoutFor,

    /// E600: a `:try` has no `:endtry`.
    MissingEndtry,

    /// E601: `:try` inside 50 open blocks.
    TryNestingTooDeep,

    /// E602: `:endtry` without `:try`.
    EndtryWithoutTry,

    /// E603: `:catch` without `:try`.
    CatchWithoutTry,

    /// E606: `:finally` without `:try`.
    FinallyWithoutTry,

    /// E732: `:endfor` ends a `:while`.
    EndforWithWhile,

    /// E733: `:endwhile` ends a `:for`.
    EndwhileWithFor,

    // ═══════════════════════════════════════════════════════════════════
    // Expression errors
    // ═══════════════════════════════════════════════════════════════════
    /// E15: Invalid expression. Carries the expression.
    InvalidExpression(Box<str>),

    /// E109: Missing ':' after '?'.
    MissingColon,

    /// E110: Missing ')'.
    MissingParen,

    /// E112: Option name missing. Carries the text from the `&`.
    OptionNameMissing(Box<str>),

    /// E113: Unknown option. Carries the option name.
    UnknownOption(Box<str>),

    /// E114: Missing double quote. Carries the text from the quote.
    MissingDoubleQuote(Box<str>),

    /// E115: Missing single quote. Carries the text from the quote.
    MissingSingleQuote(Box<str>),

    /// E116: Invalid arguments for function. Carries the function name.
    InvalidArguments(Box<str>),

    /// E117: Unknown function. Carries the function name.
    UnknownFunction(Box<str>),

    /// E118: Too many arguments for function. Carries the function name.
    TooManyArguments(Box<str>),

    /// E119: Not enough arguments for function. Carries the function name.
    NotEnoughArguments(Box<str>),

    /// E121: Undefined variable. Carries the variable name.
    UndefinedVariable(Box<str>),

    /// E362: Using a boolean value as a Float.
    BoolAsFloat,

    /// E740: a function call with more than 20 arguments. Carries the
    /// function name.
    ArgumentLimit(Box<str>),

    /// E804: Cannot use '%' with Float.
    PercentWithFloat,

    /// E805: Using a Float as a Number.
    FloatAsNumber,

    /// E892: Using a String as a Float.
    StringAsFloat,

    /// E1169: Expression too recursive. Carries the rest of the
    /// expression.
    ExpressionTooRecursive(Box<str>),

    /// A pattern that does not compile. Carries the message, which holds
    /// its Vim error number.
    InvalidPattern(Box<str>),

    /// E108: `:unlet` of a variable that does not exist. Carries its name.
    NoSuchVariable(Box<str>),

    /// E355: `:let` of an option that does not exist. Carries its name.
    UnknownOptionToSet(Box<str>),

    /// E734: Wrong variable type for an operator such as `.=`. Carries the
    /// operator before the `=`.
    WrongVariableType(char),

    /// E928: String required.
    StringRequired,

    // ═══════════════════════════════════════════════════════════════════
    // Macro errors
    // ═══════════════════════════════════════════════════════════════════
    /// E223: Recursive mapping.
    RecursiveMacro {
        /// Register that caused the recursion.
        register: char,
        /// Current recursion depth.
        depth: usize,
    },

    /// Macro replay aborted: effect limit exceeded.
    MacroEffectLimitExceeded {
        /// The configured limit that was exceeded.
        limit: usize,
    },

    // ═══════════════════════════════════════════════════════════════════
    // Internal errors
    // ═══════════════════════════════════════════════════════════════════
    /// Internal engine error (should not occur in production).
    ///
    /// Fires when a new `Command` variant is added without a corresponding
    /// executor handler. Caught by `debug_assert!` in test builds;
    /// this variant provides graceful degradation in release.
    InternalError(CompactString),

    // ═══════════════════════════════════════════════════════════════════
    // Host boundary
    // ═══════════════════════════════════════════════════════════════════
    // ═══════════════════════════════════════════════════════════════════
    // Motion errors
    // ═══════════════════════════════════════════════════════════════════
    /// Motion failed (e.g. j at last line, k at first line).
    /// Causes macro playback to abort, matching Vim's beep-and-abort behavior.
    MotionFailed,

    /// All selections were filtered out (e.g. by `:keep` / `:remove`).
    NoSelectionsRemaining,

    /// Host-reported failure (e.g. file I/O error from the shell).
    ///
    /// Used exclusively at the FFI boundary when the host signals an
    /// unstructured error string. All vim-core-internal errors use
    /// typed variants above.
    HostFailure(CompactString),
}

/// Severity level of a [`VimError`], used by hosts to choose how to display it.
///
/// | Severity | Host behavior |
/// |----------|---------------|
/// | `Brief`  | Bell only — no status-bar text (boundary navigation noise) |
/// | `Normal` | Always shown in the status bar |
/// | `System` | Shown with host/system context (file I/O, FFI errors) |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ErrorSeverity {
    /// Bell-only errors: the cursor hit a boundary, nothing dramatic happened.
    Brief,
    /// Standard Vim errors displayed in the status bar.
    Normal,
    /// Host-originated or system-level errors that warrant extra context.
    System,
}

impl VimError {
    /// Returns the [`ErrorSeverity`] for this error.
    ///
    /// Hosts use this to decide display strategy: bell-only for `Brief`,
    /// status-bar for `Normal`, and system context for `System`.
    #[must_use]
    #[inline]
    pub const fn severity(&self) -> ErrorSeverity {
        match self {
            // Brief: boundary navigation — the cursor simply hit an edge.
            Self::NothingInRegister(_) | Self::MotionFailed => ErrorSeverity::Brief,

            // System: host-originated errors with no Vim error code.
            Self::HostFailure(_) => ErrorSeverity::System,

            // Normal: all other errors go to the status bar.
            _ => ErrorSeverity::Normal,
        }
    }
}

impl fmt::Display for VimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Search
            Self::PatternNotFound(pat) => write!(f, "E486: Pattern not found: {pat}"),
            Self::NoPreviousPattern => write!(f, "E35: No previous regular expression"),
            Self::NoPreviousSubstitute => {
                write!(f, "E33: No previous substitute regular expression")
            }
            Self::NoStringUnderCursor => write!(f, "E348: No string under cursor"),

            // Registers
            Self::NothingInRegister(c) => write!(f, "E353: Nothing in register {c}"),
            Self::NoPreviousRegister => write!(f, "E748: No previously used register"),
            Self::InvalidRegisterName(name) => write!(f, "E354: Invalid register name: '{name}'"),

            // Marks
            Self::MarkNotSet(_mark) => write!(f, "E20: Mark not set"),

            // Changelist
            Self::AtStartOfChangelist => write!(f, "E662: At start of changelist"),
            Self::AtEndOfChangelist => write!(f, "E663: At end of changelist"),
            Self::ChangelistEmpty => write!(f, "E664: Changelist is empty"),
            Self::E790 => write!(f, "E790: undojoin is not allowed after undo"),

            // Ex commands
            Self::NotEditorCommand(cmd) => write!(f, "E492: Not an editor command: {cmd}"),
            Self::NotAllowedHere(cmd) => write!(f, "E523: Not allowed here: {cmd}"),
            Self::InvalidAddress(addr) => write!(f, "E14: Invalid address: {addr}"),
            Self::InvalidRange => write!(f, "E16: Invalid range"),
            Self::ArgumentRequired => write!(f, "E471: Argument required"),
            Self::BackwardsRange => write!(f, "E493: Backwards range given"),
            Self::MoveIntoItself => {
                write!(f, "E134: Cannot move a range of lines into itself")
            }
            Self::InvalidArgument(msg) => write!(f, "E521: {msg}"),
            Self::OptionValue(msg) => write!(f, "{msg}"),
            Self::UnknownOptionArgument(arg) => write!(f, "E518: Unknown option: {arg}"),
            Self::OptionNotSupported(arg) => write!(f, "E519: Option not supported: {arg}"),
            Self::InvalidMapArgument => write!(f, "E474: Invalid argument"),
            Self::GlobalMappingExists(keys) => {
                write!(f, "E225: Global mapping already exists for {keys}")
            }
            Self::AbbreviationExists(trigger) => {
                write!(f, "E226: Abbreviation already exists for {trigger}")
            }
            Self::MappingExists(keys) => write!(f, "E227: Mapping already exists for {keys}"),
            Self::NoSuchMapping => write!(f, "E31: No such mapping"),
            Self::NoSuchAbbreviation => write!(f, "E24: No such abbreviation"),
            Self::NoPreviousCommand => write!(f, "E34: No previous command"),
            Self::NoCommandAfterGlobal => write!(f, "E476: No command after g/v"),
            Self::GlobalLineLimitExceeded { limit } => write!(
                f,
                "E5100: :global command exceeded maximum line count ({limit})"
            ),
            Self::GlobalRecursionLimitExceeded { limit } => {
                write!(f, "E5101: :global recursion depth limit exceeded ({limit})")
            }
            Self::InvalidRegexDelimiter => {
                write!(f, "E146: Regular expressions can't be delimited")
            }
            Self::InvalidSearchPattern => {
                write!(f, "E682: Invalid search pattern or delimiter")
            }
            Self::InvalidCommand => write!(f, "E476: Invalid command"),
            Self::NoRangeAllowed => write!(f, "E481: No range allowed"),
            Self::NotAvailable(form) => write!(
                f,
                "E319: Sorry, the command is not available in this version: {form}"
            ),
            Self::FinishOutsideSource => {
                write!(f, "E168: :finish used outside of a sourced file")
            }
            Self::TrailingCharacters(text) => write!(f, "E488: Trailing characters: {text}"),

            // Sourced files
            Self::WrongLineSeparator => {
                write!(f, "W15: Warning: Wrong line separator, ^M may be missing")
            }
            Self::MissingEndfunction => write!(f, "E126: Missing :endfunction"),
            Self::MissingMarker => write!(f, "E172: Missing marker"),
            Self::LowercaseMarker => {
                write!(f, "E221: Marker cannot start with lower case letter")
            }
            Self::MissingEndMarker(marker) => write!(f, "E990: Missing end marker '{marker}'"),
            Self::MissingBrace => write!(f, "E1026: Missing }}"),
            Self::MissingEnddef => write!(f, "E1057: Missing :enddef"),
            Self::FunctionNestingTooDeep => write!(f, "E1058: Function nesting too deep"),
            Self::MissingHeredocEndMarker(marker) => {
                write!(f, "E1145: Missing heredoc end marker: {marker}")
            }
            Self::MissingEndBlock => write!(f, "E1171: Missing }} after inline function"),

            // Script blocks
            Self::MissingEndwhile => write!(f, "E170: Missing :endwhile"),
            Self::MissingEndfor => write!(f, "E170: Missing :endfor"),
            Self::MissingEndif => write!(f, "E171: Missing :endif"),
            Self::NoBangAllowed => write!(f, "E477: No ! allowed"),
            Self::IfNestingTooDeep => write!(f, "E579: :if nesting too deep"),
            Self::EndifWithoutIf => write!(f, "E580: :endif without :if"),
            Self::ElseWithoutIf => write!(f, "E581: :else without :if"),
            Self::ElseifWithoutIf => write!(f, "E582: :elseif without :if"),
            Self::MultipleElse => write!(f, "E583: Multiple :else"),
            Self::ElseifAfterElse => write!(f, "E584: :elseif after :else"),
            Self::LoopNestingTooDeep => write!(f, "E585: :while/:for nesting too deep"),
            Self::ContinueWithoutLoop => {
                write!(f, "E586: :continue without :while or :for")
            }
            Self::BreakWithoutLoop => write!(f, "E587: :break without :while or :for"),
            Self::EndwhileWithoutWhile => write!(f, "E588: :endwhile without :while"),
            Self::EndforWithoutFor => write!(f, "E588: :endfor without :for"),
            Self::MissingEndtry => write!(f, "E600: Missing :endtry"),
            Self::TryNestingTooDeep => write!(f, "E601: :try nesting too deep"),
            Self::EndtryWithoutTry => write!(f, "E602: :endtry without :try"),
            Self::CatchWithoutTry => write!(f, "E603: :catch without :try"),
            Self::FinallyWithoutTry => write!(f, "E606: :finally without :try"),
            Self::EndforWithWhile => write!(f, "E732: Using :endfor with :while"),
            Self::EndwhileWithFor => write!(f, "E733: Using :endwhile with :for"),

            // Expressions
            Self::InvalidExpression(expr) => write!(f, "E15: Invalid expression: \"{expr}\""),
            Self::MissingColon => write!(f, "E109: Missing ':' after '?'"),
            Self::MissingParen => write!(f, "E110: Missing ')'"),
            Self::OptionNameMissing(text) => write!(f, "E112: Option name missing: {text}"),
            Self::UnknownOption(name) => write!(f, "E113: Unknown option: {name}"),
            Self::MissingDoubleQuote(text) => write!(f, "E114: Missing double quote: {text}"),
            Self::MissingSingleQuote(text) => write!(f, "E115: Missing single quote: {text}"),
            Self::InvalidArguments(name) => {
                write!(f, "E116: Invalid arguments for function {name}")
            }
            Self::UnknownFunction(name) => write!(f, "E117: Unknown function: {name}"),
            Self::TooManyArguments(name) => {
                write!(f, "E118: Too many arguments for function: {name}")
            }
            Self::NotEnoughArguments(name) => {
                write!(f, "E119: Not enough arguments for function: {name}")
            }
            Self::UndefinedVariable(name) => write!(f, "E121: Undefined variable: {name}"),
            Self::BoolAsFloat => write!(f, "E362: Using a boolean value as a Float"),
            Self::ArgumentLimit(name) => write!(f, "E740: Too many arguments for function {name}"),
            Self::PercentWithFloat => write!(f, "E804: Cannot use '%' with Float"),
            Self::FloatAsNumber => write!(f, "E805: Using a Float as a Number"),
            Self::StringAsFloat => write!(f, "E892: Using a String as a Float"),
            Self::ExpressionTooRecursive(rest) => {
                write!(f, "E1169: Expression too recursive: {rest}")
            }
            Self::InvalidPattern(message) => write!(f, "{message}"),
            Self::NoSuchVariable(name) => write!(f, "E108: No such variable: \"{name}\""),
            Self::UnknownOptionToSet(name) => write!(f, "E355: Unknown option: {name}"),
            Self::WrongVariableType(op) => write!(f, "E734: Wrong variable type for {op}="),
            Self::StringRequired => write!(f, "E928: String required"),

            // Macros
            Self::RecursiveMacro { register, depth } => {
                write!(f, "E223: recursive mapping for @{register} (depth {depth})")
            }
            Self::MacroEffectLimitExceeded { limit } => {
                write!(
                    f,
                    "E5765: macro replay aborted: effect limit ({limit}) exceeded"
                )
            }

            // Motions
            Self::MotionFailed => write!(f, "motion failed"),

            // Selections
            Self::NoSelectionsRemaining => write!(f, "No selections remaining"),

            // Internal
            Self::InternalError(msg) => write!(f, "E0: internal error - {msg}"),

            // Host boundary
            Self::HostFailure(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for VimError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_option_argument_keeps_the_argument() {
        assert_eq!(
            VimError::UnknownOptionArgument("nosuch=1".into()).to_string(),
            "E518: Unknown option: nosuch=1"
        );
    }

    #[test]
    fn option_not_supported_keeps_the_argument() {
        assert_eq!(
            VimError::OptionNotSupported("mouse?".into()).to_string(),
            "E519: Option not supported: mouse?"
        );
    }

    #[test]
    fn display_mapping_errors() {
        for (error, text) in [
            (VimError::InvalidMapArgument, "E474: Invalid argument"),
            (
                VimError::GlobalMappingExists("x".into()),
                "E225: Global mapping already exists for x",
            ),
            (
                VimError::AbbreviationExists("teh".into()),
                "E226: Abbreviation already exists for teh",
            ),
            (
                VimError::MappingExists("x".into()),
                "E227: Mapping already exists for x",
            ),
            (VimError::NoSuchMapping, "E31: No such mapping"),
            (VimError::NoSuchAbbreviation, "E24: No such abbreviation"),
        ] {
            assert_eq!(error.to_string(), text);
        }
    }

    #[test]
    fn option_value_displays_message_verbatim() {
        assert_eq!(
            VimError::OptionValue("E539: Illegal character <Z>: fo=Z".into()).to_string(),
            "E539: Illegal character <Z>: fo=Z"
        );
    }

    // ── ErrorSeverity tests ───────────────────────────────────────────────

    #[test]
    fn severity_brief_for_boundary_errors() {
        assert_eq!(
            VimError::NothingInRegister('"').severity(),
            ErrorSeverity::Brief
        );
        assert_eq!(VimError::MotionFailed.severity(), ErrorSeverity::Brief);
    }

    #[test]
    fn severity_system_for_host_errors() {
        assert_eq!(
            VimError::HostFailure("disk full".into()).severity(),
            ErrorSeverity::System
        );
    }

    #[test]
    fn severity_normal_for_standard_errors() {
        assert_eq!(
            VimError::NoPreviousPattern.severity(),
            ErrorSeverity::Normal
        );
        assert_eq!(VimError::InvalidRange.severity(), ErrorSeverity::Normal);
        assert_eq!(
            VimError::PatternNotFound("foo".into()).severity(),
            ErrorSeverity::Normal
        );
    }

    #[test]
    fn all_variants_have_severity() {
        // Exhaustively verify every currently-known variant returns a severity
        // (ensures the match compiles and is exhaustive at the type level).
        let variants: &[VimError] = &[
            VimError::PatternNotFound("x".into()),
            VimError::NoPreviousPattern,
            VimError::NoPreviousSubstitute,
            VimError::NoStringUnderCursor,
            VimError::NothingInRegister('a'),
            VimError::NoPreviousRegister,
            VimError::InvalidRegisterName("bad".into()),
            VimError::MarkNotSet('a'),
            VimError::AtStartOfChangelist,
            VimError::AtEndOfChangelist,
            VimError::ChangelistEmpty,
            VimError::NotEditorCommand("foo".into()),
            VimError::NotAllowedHere("normal dd".into()),
            VimError::InvalidAddress("999".into()),
            VimError::InvalidRange,
            VimError::ArgumentRequired,
            VimError::BackwardsRange,
            VimError::MoveIntoItself,
            VimError::InvalidArgument("bad".into()),
            VimError::OptionValue("E539: Illegal character <Z>: fo=Z".into()),
            VimError::UnknownOptionArgument("nosuch".into()),
            VimError::OptionNotSupported("mouse?".into()),
            VimError::InvalidMapArgument,
            VimError::GlobalMappingExists("x".into()),
            VimError::AbbreviationExists("teh".into()),
            VimError::MappingExists("x".into()),
            VimError::NoSuchMapping,
            VimError::NoSuchAbbreviation,
            VimError::NoPreviousCommand,
            VimError::NoCommandAfterGlobal,
            VimError::GlobalLineLimitExceeded { limit: 100 },
            VimError::GlobalRecursionLimitExceeded { limit: 10 },
            VimError::InvalidRegexDelimiter,
            VimError::InvalidSearchPattern,
            VimError::InvalidCommand,
            VimError::NoRangeAllowed,
            VimError::NotAvailable("source!".into()),
            VimError::FinishOutsideSource,
            VimError::TrailingCharacters("x".into()),
            VimError::WrongLineSeparator,
            VimError::MissingEndfunction,
            VimError::MissingMarker,
            VimError::LowercaseMarker,
            VimError::MissingEndMarker("END".into()),
            VimError::MissingBrace,
            VimError::MissingEnddef,
            VimError::FunctionNestingTooDeep,
            VimError::MissingHeredocEndMarker("END".into()),
            VimError::MissingEndBlock,
            VimError::MissingEndif,
            VimError::EndifWithoutIf,
            VimError::InvalidExpression("x".into()),
            VimError::MissingColon,
            VimError::MissingParen,
            VimError::OptionNameMissing("&".into()),
            VimError::UnknownOption("x".into()),
            VimError::MissingDoubleQuote("\"x".into()),
            VimError::MissingSingleQuote("'x".into()),
            VimError::InvalidArguments("has".into()),
            VimError::UnknownFunction("Foo".into()),
            VimError::TooManyArguments("has".into()),
            VimError::NotEnoughArguments("has".into()),
            VimError::UndefinedVariable("x".into()),
            VimError::BoolAsFloat,
            VimError::ArgumentLimit("Foo".into()),
            VimError::PercentWithFloat,
            VimError::FloatAsNumber,
            VimError::StringAsFloat,
            VimError::ExpressionTooRecursive("1)".into()),
            VimError::InvalidPattern("E54: Unmatched \\(".into()),
            VimError::NoSuchVariable("x".into()),
            VimError::UnknownOptionToSet("x".into()),
            VimError::WrongVariableType('.'),
            VimError::StringRequired,
            VimError::RecursiveMacro {
                register: 'a',
                depth: 1,
            },
            VimError::MacroEffectLimitExceeded { limit: 100 },
            VimError::InternalError("oops".into()),
            VimError::MotionFailed,
            VimError::NoSelectionsRemaining,
            VimError::HostFailure("err".into()),
        ];
        for v in variants {
            // Just calling severity() is enough — the compiler ensures all
            // arms are covered; this loop ensures no panic at runtime.
            let _ = v.severity();
        }
    }

    #[test]
    fn display_pattern_not_found() {
        let e = VimError::PatternNotFound("foo".into());
        assert_eq!(e.to_string(), "E486: Pattern not found: foo");
    }

    #[test]
    fn display_no_previous_pattern() {
        assert_eq!(
            VimError::NoPreviousPattern.to_string(),
            "E35: No previous regular expression"
        );
    }

    #[test]
    fn display_nothing_in_register() {
        let e = VimError::NothingInRegister('"');
        assert_eq!(e.to_string(), "E353: Nothing in register \"");
    }

    #[test]
    fn display_mark_not_set() {
        let e = VimError::MarkNotSet('a');
        assert_eq!(e.to_string(), "E20: Mark not set");
    }

    #[test]
    fn display_not_editor_command() {
        let e = VimError::NotEditorCommand("foo".into());
        assert_eq!(e.to_string(), "E492: Not an editor command: foo");
    }

    #[test]
    fn display_not_allowed_here() {
        let e = VimError::NotAllowedHere("normal dd".into());
        assert_eq!(e.to_string(), "E523: Not allowed here: normal dd");
    }

    #[test]
    fn display_recursive_macro() {
        let e = VimError::RecursiveMacro {
            register: 'a',
            depth: 200,
        };
        assert_eq!(e.to_string(), "E223: recursive mapping for @a (depth 200)");
    }

    #[test]
    fn display_changelist_errors() {
        assert_eq!(
            VimError::AtStartOfChangelist.to_string(),
            "E662: At start of changelist"
        );
        assert_eq!(
            VimError::AtEndOfChangelist.to_string(),
            "E663: At end of changelist"
        );
        assert_eq!(
            VimError::ChangelistEmpty.to_string(),
            "E664: Changelist is empty"
        );
    }

    #[test]
    fn display_host_failure() {
        let e = VimError::HostFailure("disk full".into());
        assert_eq!(e.to_string(), "disk full");
    }

    #[test]
    fn display_no_previous_register() {
        assert_eq!(
            VimError::NoPreviousRegister.to_string(),
            "E748: No previously used register"
        );
    }

    #[test]
    fn display_invalid_address() {
        let e = VimError::InvalidAddress("999".into());
        assert_eq!(e.to_string(), "E14: Invalid address: 999");
    }

    #[test]
    fn display_invalid_range() {
        assert_eq!(VimError::InvalidRange.to_string(), "E16: Invalid range");
    }

    #[test]
    fn display_argument_required() {
        assert_eq!(
            VimError::ArgumentRequired.to_string(),
            "E471: Argument required"
        );
    }

    #[test]
    fn display_no_range_allowed() {
        assert_eq!(
            VimError::NoRangeAllowed.to_string(),
            "E481: No range allowed"
        );
    }

    #[test]
    fn display_not_available() {
        assert_eq!(
            VimError::NotAvailable("source!".into()).to_string(),
            "E319: Sorry, the command is not available in this version: source!"
        );
    }

    #[test]
    fn display_trailing_characters() {
        assert_eq!(
            VimError::TrailingCharacters("x | set ic".into()).to_string(),
            "E488: Trailing characters: x | set ic"
        );
    }

    #[test]
    fn display_wrong_line_separator() {
        assert_eq!(
            VimError::WrongLineSeparator.to_string(),
            "W15: Warning: Wrong line separator, ^M may be missing"
        );
    }

    #[test]
    fn display_sourced_file_errors() {
        for (error, text) in [
            (VimError::MissingEndfunction, "E126: Missing :endfunction"),
            (VimError::MissingMarker, "E172: Missing marker"),
            (
                VimError::LowercaseMarker,
                "E221: Marker cannot start with lower case letter",
            ),
            (
                VimError::MissingEndMarker("END".into()),
                "E990: Missing end marker 'END'",
            ),
            (VimError::MissingBrace, "E1026: Missing }"),
            (VimError::MissingEnddef, "E1057: Missing :enddef"),
            (
                VimError::FunctionNestingTooDeep,
                "E1058: Function nesting too deep",
            ),
            (
                VimError::MissingHeredocEndMarker(".".into()),
                "E1145: Missing heredoc end marker: .",
            ),
            (
                VimError::MissingEndBlock,
                "E1171: Missing } after inline function",
            ),
        ] {
            assert_eq!(error.to_string(), text);
        }
    }

    #[test]
    fn display_script_block_errors() {
        for (error, text) in [
            (VimError::MissingEndwhile, "E170: Missing :endwhile"),
            (VimError::MissingEndfor, "E170: Missing :endfor"),
            (VimError::MissingEndif, "E171: Missing :endif"),
            (VimError::NoBangAllowed, "E477: No ! allowed"),
            (VimError::IfNestingTooDeep, "E579: :if nesting too deep"),
            (VimError::EndifWithoutIf, "E580: :endif without :if"),
            (VimError::ElseWithoutIf, "E581: :else without :if"),
            (VimError::ElseifWithoutIf, "E582: :elseif without :if"),
            (VimError::MultipleElse, "E583: Multiple :else"),
            (VimError::ElseifAfterElse, "E584: :elseif after :else"),
            (
                VimError::LoopNestingTooDeep,
                "E585: :while/:for nesting too deep",
            ),
            (
                VimError::ContinueWithoutLoop,
                "E586: :continue without :while or :for",
            ),
            (
                VimError::BreakWithoutLoop,
                "E587: :break without :while or :for",
            ),
            (
                VimError::EndwhileWithoutWhile,
                "E588: :endwhile without :while",
            ),
            (VimError::EndforWithoutFor, "E588: :endfor without :for"),
            (VimError::MissingEndtry, "E600: Missing :endtry"),
            (VimError::TryNestingTooDeep, "E601: :try nesting too deep"),
            (VimError::EndtryWithoutTry, "E602: :endtry without :try"),
            (VimError::CatchWithoutTry, "E603: :catch without :try"),
            (VimError::FinallyWithoutTry, "E606: :finally without :try"),
            (VimError::EndforWithWhile, "E732: Using :endfor with :while"),
            (VimError::EndwhileWithFor, "E733: Using :endwhile with :for"),
        ] {
            assert_eq!(error.to_string(), text);
        }
    }

    #[test]
    fn display_expression_errors() {
        for (error, text) in [
            (
                VimError::InvalidExpression("1 +".into()),
                "E15: Invalid expression: \"1 +\"",
            ),
            (VimError::MissingColon, "E109: Missing ':' after '?'"),
            (VimError::MissingParen, "E110: Missing ')'"),
            (
                VimError::OptionNameMissing("&".into()),
                "E112: Option name missing: &",
            ),
            (
                VimError::UnknownOption("nosuch".into()),
                "E113: Unknown option: nosuch",
            ),
            (
                VimError::MissingDoubleQuote("\"abc".into()),
                "E114: Missing double quote: \"abc",
            ),
            (
                VimError::MissingSingleQuote("'abc".into()),
                "E115: Missing single quote: 'abc",
            ),
            (
                VimError::InvalidArguments("has".into()),
                "E116: Invalid arguments for function has",
            ),
            (
                VimError::UnknownFunction("Foo".into()),
                "E117: Unknown function: Foo",
            ),
            (
                VimError::TooManyArguments("has".into()),
                "E118: Too many arguments for function: has",
            ),
            (
                VimError::NotEnoughArguments("has".into()),
                "E119: Not enough arguments for function: has",
            ),
            (
                VimError::UndefinedVariable("g:x".into()),
                "E121: Undefined variable: g:x",
            ),
            (
                VimError::BoolAsFloat,
                "E362: Using a boolean value as a Float",
            ),
            (
                VimError::ArgumentLimit("Foo".into()),
                "E740: Too many arguments for function Foo",
            ),
            (
                VimError::PercentWithFloat,
                "E804: Cannot use '%' with Float",
            ),
            (VimError::FloatAsNumber, "E805: Using a Float as a Number"),
            (VimError::StringAsFloat, "E892: Using a String as a Float"),
            (
                VimError::ExpressionTooRecursive("1)".into()),
                "E1169: Expression too recursive: 1)",
            ),
            (
                VimError::InvalidPattern("E54: Unmatched \\(".into()),
                "E54: Unmatched \\(",
            ),
        ] {
            assert_eq!(error.to_string(), text);
        }
    }

    #[test]
    fn display_assignment_errors() {
        for (error, text) in [
            (
                VimError::NoSuchVariable("g:x".into()),
                "E108: No such variable: \"g:x\"",
            ),
            (
                VimError::UnknownOptionToSet("nosuch".into()),
                "E355: Unknown option: nosuch",
            ),
            (
                VimError::WrongVariableType('.'),
                "E734: Wrong variable type for .=",
            ),
            (VimError::StringRequired, "E928: String required"),
        ] {
            assert_eq!(error.to_string(), text);
        }
    }

    #[test]
    fn display_finish_outside_source() {
        assert_eq!(
            VimError::FinishOutsideSource.to_string(),
            "E168: :finish used outside of a sourced file"
        );
    }

    #[test]
    fn display_backwards_range() {
        assert_eq!(
            VimError::BackwardsRange.to_string(),
            "E493: Backwards range given"
        );
    }

    #[test]
    fn display_no_previous_command() {
        assert_eq!(
            VimError::NoPreviousCommand.to_string(),
            "E34: No previous command"
        );
    }

    #[test]
    fn display_no_command_after_global() {
        assert_eq!(
            VimError::NoCommandAfterGlobal.to_string(),
            "E476: No command after g/v"
        );
    }

    #[test]
    fn display_invalid_regex_delimiter() {
        assert_eq!(
            VimError::InvalidRegexDelimiter.to_string(),
            "E146: Regular expressions can't be delimited"
        );
    }

    #[test]
    fn display_invalid_search_pattern() {
        assert_eq!(
            VimError::InvalidSearchPattern.to_string(),
            "E682: Invalid search pattern or delimiter"
        );
    }

    #[test]
    fn display_internal_error() {
        let e = VimError::InternalError("unhandled command".into());
        assert_eq!(e.to_string(), "E0: internal error - unhandled command");
    }

    #[test]
    fn display_macro_effect_limit_exceeded() {
        let e = VimError::MacroEffectLimitExceeded { limit: 100_000 };
        assert_eq!(
            e.to_string(),
            "E5765: macro replay aborted: effect limit (100000) exceeded"
        );
    }

    #[test]
    fn display_global_line_limit_exceeded() {
        let e = VimError::GlobalLineLimitExceeded { limit: 100_000 };
        assert_eq!(
            e.to_string(),
            "E5100: :global command exceeded maximum line count (100000)"
        );
    }

    #[test]
    fn display_global_recursion_limit_exceeded() {
        let e = VimError::GlobalRecursionLimitExceeded { limit: 10 };
        assert_eq!(
            e.to_string(),
            "E5101: :global recursion depth limit exceeded (10)"
        );
    }

    #[test]
    fn display_no_selections_remaining() {
        let e = VimError::NoSelectionsRemaining;
        assert_eq!(e.to_string(), "No selections remaining");
    }
}
