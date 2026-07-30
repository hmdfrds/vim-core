/// Semantic text object kinds for the semantic text object protocol.
///
/// Each variant maps to a structural element that a language-aware
/// provider (e.g. tree-sitter, LSP) can resolve to a byte range.
///
/// # Key Bindings (after `i`/`a`)
///
/// | Key | Object           |
/// |-----|-----------------|
/// | `f` | Function         |
/// | `c` | Class            |
/// | `a` | Parameter        |
/// | `C` | Conditional      |
/// | `o` | Loop             |
/// | `K` | Comment          |
/// | `S` | Scope            |
/// | `F` | Call             |
/// | `T` | TypeDef          |
/// | `R` | Return           |
/// | `U` | Import           |
/// | `Z` | StringLiteral    |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u8)]
pub enum SemanticObject {
    /// Function or method body (`f`).
    Function = 0,
    /// Class, struct, or module definition (`c`).
    Class = 1,
    /// Function parameter or argument (`a`).
    Parameter = 2,
    /// Conditional block — if/else/match/switch (`C`).
    Conditional = 3,
    /// Loop block — for/while/loop (`o`).
    Loop = 4,
    /// Comment line or block (`K`).
    Comment = 5,
    /// Lexical scope / block (`S`).
    Scope = 6,
    /// Function call expression (`F`).
    Call = 7,
    /// Type definition / alias (`T`).
    TypeDef = 8,
    /// Return statement (`R`).
    Return = 9,
    /// Import / use statement (`U`).
    Import = 10,
    /// String literal (`Z`).
    StringLiteral = 11,
}
