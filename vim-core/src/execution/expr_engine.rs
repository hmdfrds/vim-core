//! Expression engine trait and built-in simple expression evaluator.
//!
//! [`ExprEngine`] is the extension point for expression/script evaluation
//! engines. [`SimpleExprEval`] is the built-in evaluator that handles basic
//! expressions (arithmetic, string ops, variable reads) without external deps.

use compact_str::CompactString;

use crate::primitives::{VarScope, VimOptions, VimValue};
use crate::state::VariableStore;

// ═══════════════════════════════════════════════════════════════════════════════
// RuntimeError
// ═══════════════════════════════════════════════════════════════════════════════

/// Errors from the expression runtime.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum RuntimeError {
    /// Expression failed to compile/parse.
    CompileError(CompactString),
    /// Runtime execution error.
    RuntimeError(CompactString),
    /// Execution exceeded fuel limit.
    FuelExhausted,
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CompileError(msg) => write!(f, "compile error: {msg}"),
            Self::RuntimeError(msg) => write!(f, "runtime error: {msg}"),
            Self::FuelExhausted => write!(f, "fuel exhausted"),
        }
    }
}

impl std::error::Error for RuntimeError {}

// ═══════════════════════════════════════════════════════════════════════════════
// ExprContext
// ═══════════════════════════════════════════════════════════════════════════════

/// Context available during expression evaluation.
///
/// Provides read-only access to variables and options so that expressions
/// can reference `g:varname`, `b:varname`, and option values.
pub struct ExprContext<'a> {
    /// The variable store for `g:` and `b:` lookups.
    pub variables: &'a VariableStore,
    /// Resolved Vim options for option queries.
    pub options: &'a VimOptions,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ExprEngine trait
// ═══════════════════════════════════════════════════════════════════════════════

/// Trait for expression/script evaluation engines.
///
/// Implementors: built-in [`SimpleExprEval`].
pub trait ExprEngine {
    /// Evaluate an expression string and return its value.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError`] if the expression cannot be parsed or evaluated.
    fn eval_expression(
        &self,
        expr: &str,
        context: &ExprContext<'_>,
    ) -> Result<VimValue, RuntimeError>;
}

// ═══════════════════════════════════════════════════════════════════════════════
// SimpleExprEval — built-in expression evaluator
// ═══════════════════════════════════════════════════════════════════════════════

/// Built-in expression evaluator for basic expressions.
///
/// Handles integer/float literals, string literals, boolean values,
/// variable access (`g:name`, `b:name`), basic arithmetic (`+`, `-`, `*`, `/`),
/// string concatenation (`.`), comparison operators, unary minus,
/// and parenthesised grouping.
///
/// Does not support function calls, control flow, or complex scripting.
pub struct SimpleExprEval;

impl ExprEngine for SimpleExprEval {
    fn eval_expression(
        &self,
        expr: &str,
        context: &ExprContext<'_>,
    ) -> Result<VimValue, RuntimeError> {
        let tokens = tokenize(expr)?;
        let mut parser = Parser::new(&tokens, context);
        let result = parser.parse_expression()?;
        if parser.pos < parser.tokens.len() {
            return Err(RuntimeError::CompileError(CompactString::from(
                "unexpected token after expression",
            )));
        }
        Ok(result)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tokenizer
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Int(i64),
    Float(f64),
    Str(CompactString),
    Bool(bool),
    Variable(VarScope, CompactString),
    Plus,
    Minus,
    Star,
    Slash,
    Dot,
    Eq,
    Neq,
    Lt,
    Gt,
    Le,
    Ge,
    LParen,
    RParen,
}

/// Tokenize an expression string into a list of tokens.
///
/// Uses a character iterator to avoid direct indexing. All bounds are checked.
fn tokenize(input: &str) -> Result<Vec<Token>, RuntimeError> {
    let mut tokens = Vec::new();
    let mut chars = input.char_indices().peekable();

    while let Some(&(_, ch)) = chars.peek() {
        // Skip whitespace.
        if ch.is_ascii_whitespace() {
            chars.next();
            continue;
        }

        // Two-character operators: peek at current + next.
        if let Some(tok) = try_two_char_op(&mut chars) {
            tokens.push(tok);
            continue;
        }

        // Single-character operators and punctuation.
        if let Some(tok) = try_single_char_op(ch) {
            chars.next();
            tokens.push(tok);
            continue;
        }

        // String literals: "..." or '...'
        if ch == '"' || ch == '\'' {
            tokens.push(tokenize_string(&mut chars)?);
            continue;
        }

        // Numbers (integers and floats).
        if ch.is_ascii_digit() {
            tokens.push(tokenize_number(input, &mut chars)?);
            continue;
        }

        // Identifiers: variable references (g:name, b:name) or keywords (true/false).
        if ch.is_ascii_alphabetic() || ch == '_' {
            tokens.push(tokenize_ident(input, &mut chars)?);
            continue;
        }

        return Err(RuntimeError::CompileError(CompactString::from(&format!(
            "unexpected character: {ch:?}"
        ))));
    }

    Ok(tokens)
}

/// Try to match a two-character operator at the current position.
fn try_two_char_op(chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>) -> Option<Token> {
    // We need to look at both current and next chars.
    let mut clone = chars.clone();
    let (_, c1) = clone.next()?;
    let (_, c2) = clone.peek().copied()?;

    let tok = match (c1, c2) {
        ('=', '=') => Token::Eq,
        ('!', '=') => Token::Neq,
        ('<', '=') => Token::Le,
        ('>', '=') => Token::Ge,
        _ => return None,
    };

    // Consume both characters from the real iterator.
    chars.next();
    chars.next();
    Some(tok)
}

/// Try to match a single-character operator.
const fn try_single_char_op(ch: char) -> Option<Token> {
    match ch {
        '+' => Some(Token::Plus),
        '-' => Some(Token::Minus),
        '*' => Some(Token::Star),
        '/' => Some(Token::Slash),
        '.' => Some(Token::Dot),
        '<' => Some(Token::Lt),
        '>' => Some(Token::Gt),
        '(' => Some(Token::LParen),
        ')' => Some(Token::RParen),
        _ => None,
    }
}

/// Tokenize a string literal (double or single quoted).
fn tokenize_string(
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> Result<Token, RuntimeError> {
    let (_, quote) = chars
        .next()
        .ok_or_else(|| RuntimeError::CompileError(CompactString::from("unexpected end")))?;

    let mut content = String::new();
    loop {
        match chars.next() {
            None => {
                return Err(RuntimeError::CompileError(CompactString::from(
                    "unterminated string literal",
                )));
            }
            Some((_, ch)) if ch == quote => break,
            Some((_, ch)) => content.push(ch),
        }
    }
    Ok(Token::Str(CompactString::from(content)))
}

/// Tokenize a number (integer or float).
fn tokenize_number(
    input: &str,
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> Result<Token, RuntimeError> {
    let (start, _) = chars
        .next()
        .ok_or_else(|| RuntimeError::CompileError(CompactString::from("unexpected end")))?;

    // Consume remaining digits.
    let mut end = start + 1;
    while let Some(&(idx, ch)) = chars.peek() {
        if ch.is_ascii_digit() {
            end = idx + 1;
            chars.next();
        } else {
            break;
        }
    }

    // Check for float: `.` followed by a digit.
    if let Some(&(dot_idx, '.')) = chars.peek() {
        let mut clone = chars.clone();
        clone.next(); // skip the dot
        if let Some(&(_, next_ch)) = clone.peek() {
            if next_ch.is_ascii_digit() {
                // Consume the dot.
                chars.next();
                end = dot_idx + 1;
                // Consume fractional digits.
                while let Some(&(idx, ch)) = chars.peek() {
                    if ch.is_ascii_digit() {
                        end = idx + 1;
                        chars.next();
                    } else {
                        break;
                    }
                }
                let f: f64 = input[start..end].parse().map_err(|_| {
                    RuntimeError::CompileError(CompactString::from("invalid float literal"))
                })?;
                return Ok(Token::Float(f));
            }
        }
    }

    let n: i64 = input[start..end]
        .parse()
        .map_err(|_| RuntimeError::CompileError(CompactString::from("invalid integer literal")))?;
    Ok(Token::Int(n))
}

/// Tokenize an identifier (keyword or scoped variable).
fn tokenize_ident(
    input: &str,
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> Result<Token, RuntimeError> {
    let (start, _) = chars
        .next()
        .ok_or_else(|| RuntimeError::CompileError(CompactString::from("unexpected end")))?;

    let mut end = start + 1;
    while let Some(&(idx, ch)) = chars.peek() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            end = idx + 1;
            chars.next();
        } else {
            break;
        }
    }

    let word = &input[start..end];

    // Check for scope prefix: g: or b:
    if (word == "g" || word == "b") && chars.peek().map(|&(_, c)| c) == Some(':') {
        let scope = if word == "g" {
            VarScope::Global
        } else {
            VarScope::Buffer
        };
        chars.next(); // skip ':'
        let name_start = chars.peek().map(|&(idx, _)| idx);
        let Some(ns) = name_start else {
            return Err(RuntimeError::CompileError(CompactString::from(
                "expected variable name after scope prefix",
            )));
        };
        // Must have at least one valid char.
        if !chars
            .peek()
            .is_some_and(|&(_, ch)| ch.is_ascii_alphanumeric() || ch == '_')
        {
            return Err(RuntimeError::CompileError(CompactString::from(
                "expected variable name after scope prefix",
            )));
        }
        let mut name_end = ns;
        while let Some(&(idx, ch)) = chars.peek() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                name_end = idx + 1;
                chars.next();
            } else {
                break;
            }
        }
        return Ok(Token::Variable(
            scope,
            CompactString::from(&input[ns..name_end]),
        ));
    }

    match word {
        "true" => Ok(Token::Bool(true)),
        "false" => Ok(Token::Bool(false)),
        _ => Err(RuntimeError::CompileError(CompactString::from(&format!(
            "unknown identifier: {word}"
        )))),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Recursive descent parser
// ─────────────────────────────────────────────────────────────────────────────

struct Parser<'a> {
    tokens: &'a [Token],
    context: &'a ExprContext<'a>,
    pos: usize,
}

impl<'a> Parser<'a> {
    const fn new(tokens: &'a [Token], context: &'a ExprContext<'a>) -> Self {
        Self {
            tokens,
            context,
            pos: 0,
        }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<&Token> {
        let tok = self.tokens.get(self.pos);
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    /// Advance and clone the token. Only call when `peek()` has confirmed a token exists.
    #[allow(
        clippy::expect_used,
        reason = "callers always confirm via peek() / matches! before calling"
    )]
    fn advance_clone(&mut self) -> Token {
        let tok = self
            .tokens
            .get(self.pos)
            .expect("peek confirmed token exists");
        self.pos += 1;
        tok.clone()
    }

    /// expression = comparison
    fn parse_expression(&mut self) -> Result<VimValue, RuntimeError> {
        self.parse_comparison()
    }

    /// comparison = addition (( "==" | "!=" | "<" | ">" | "<=" | ">=" ) addition)?
    fn parse_comparison(&mut self) -> Result<VimValue, RuntimeError> {
        let left = self.parse_addition()?;

        match self.peek() {
            Some(Token::Eq | Token::Neq | Token::Lt | Token::Gt | Token::Le | Token::Ge) => {}
            _ => return Ok(left),
        }

        let op = self.advance_clone();
        let right = self.parse_addition()?;
        eval_comparison(&left, &op, &right)
    }

    /// addition = multiplication (( "+" | "-" | "." ) multiplication)*
    fn parse_addition(&mut self) -> Result<VimValue, RuntimeError> {
        let mut result = self.parse_multiplication()?;

        while matches!(self.peek(), Some(Token::Plus | Token::Minus | Token::Dot)) {
            let op = self.advance_clone();
            let right = self.parse_multiplication()?;
            result = eval_additive(&result, &op, &right)?;
        }

        Ok(result)
    }

    /// multiplication = unary (( "*" | "/" ) unary)*
    fn parse_multiplication(&mut self) -> Result<VimValue, RuntimeError> {
        let mut result = self.parse_unary()?;

        while matches!(self.peek(), Some(Token::Star | Token::Slash)) {
            let op = self.advance_clone();
            let right = self.parse_unary()?;
            result = eval_multiplicative(&result, &op, &right)?;
        }

        Ok(result)
    }

    /// unary = "-" unary | atom
    fn parse_unary(&mut self) -> Result<VimValue, RuntimeError> {
        if self.peek() == Some(&Token::Minus) {
            self.advance();
            let val = self.parse_unary()?;
            return match val {
                VimValue::Int(n) => Ok(VimValue::Int(-n)),
                VimValue::Float(f) => Ok(VimValue::Float(-f)),
                _ => Err(RuntimeError::RuntimeError(CompactString::from(
                    "unary minus requires a number",
                ))),
            };
        }
        self.parse_atom()
    }

    /// atom = INT | FLOAT | STRING | BOOL | VARIABLE | "(" expression ")"
    fn parse_atom(&mut self) -> Result<VimValue, RuntimeError> {
        let tok = self
            .advance()
            .ok_or_else(|| {
                RuntimeError::CompileError(CompactString::from("unexpected end of expression"))
            })?
            .clone();

        match &tok {
            Token::Int(n) => Ok(VimValue::Int(*n)),
            Token::Float(f) => Ok(VimValue::Float(*f)),
            Token::Str(s) => Ok(VimValue::String(s.clone())),
            Token::Bool(b) => Ok(VimValue::Bool(*b)),
            Token::Variable(scope, name) => self
                .context
                .variables
                .get(*scope, name)
                .cloned()
                .ok_or_else(|| {
                    let prefix = match scope {
                        VarScope::Global => "g",
                        VarScope::Buffer => "b",
                    };
                    RuntimeError::RuntimeError(CompactString::from(format!(
                        "undefined variable: {prefix}:{name}"
                    )))
                }),
            Token::LParen => {
                let val = self.parse_expression()?;
                if self.advance() != Some(&Token::RParen) {
                    return Err(RuntimeError::CompileError(CompactString::from(
                        "expected closing parenthesis",
                    )));
                }
                Ok(val)
            }
            other => Err(RuntimeError::CompileError(CompactString::from(&format!(
                "unexpected token: {other:?}"
            )))),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Evaluation helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Coerce a `VimValue` to `i64` for arithmetic.
fn to_int(v: &VimValue) -> Result<i64, RuntimeError> {
    match v {
        VimValue::Int(n) => Ok(*n),
        VimValue::Bool(b) => Ok(i64::from(*b)),
        _ => Err(RuntimeError::RuntimeError(CompactString::from(
            "expected integer operand",
        ))),
    }
}

/// Coerce a `VimValue` to `f64` for arithmetic.
fn to_float(v: &VimValue) -> Result<f64, RuntimeError> {
    match v {
        VimValue::Float(f) => Ok(*f),
        VimValue::Int(n) => {
            #[allow(
                clippy::cast_precision_loss,
                reason = "i64 -> f64 precision loss acceptable for expression eval"
            )]
            let result = *n as f64;
            Ok(result)
        }
        VimValue::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        _ => Err(RuntimeError::RuntimeError(CompactString::from(
            "expected numeric operand",
        ))),
    }
}

/// Evaluate additive operators: `+`, `-`, `.`
fn eval_additive(left: &VimValue, op: &Token, right: &VimValue) -> Result<VimValue, RuntimeError> {
    // String concatenation with `.`
    if *op == Token::Dot {
        let ls = vim_value_to_display(left);
        let rs = vim_value_to_display(right);
        return Ok(VimValue::String(CompactString::from(format!("{ls}{rs}"))));
    }

    // Float promotion: if either operand is float, compute in float.
    if matches!(left, VimValue::Float(_)) || matches!(right, VimValue::Float(_)) {
        let lf = to_float(left)?;
        let rf = to_float(right)?;
        return match op {
            Token::Plus => Ok(VimValue::Float(lf + rf)),
            Token::Minus => Ok(VimValue::Float(lf - rf)),
            _ => unreachable!(),
        };
    }

    let li = to_int(left)?;
    let ri = to_int(right)?;
    match op {
        Token::Plus => Ok(VimValue::Int(li.wrapping_add(ri))),
        Token::Minus => Ok(VimValue::Int(li.wrapping_sub(ri))),
        _ => unreachable!(),
    }
}

/// Evaluate multiplicative operators: `*`, `/`
fn eval_multiplicative(
    left: &VimValue,
    op: &Token,
    right: &VimValue,
) -> Result<VimValue, RuntimeError> {
    // Float promotion.
    if matches!(left, VimValue::Float(_)) || matches!(right, VimValue::Float(_)) {
        let lf = to_float(left)?;
        let rf = to_float(right)?;
        return match op {
            Token::Star => Ok(VimValue::Float(lf * rf)),
            Token::Slash => {
                if rf == 0.0 {
                    return Err(RuntimeError::RuntimeError(CompactString::from(
                        "division by zero",
                    )));
                }
                Ok(VimValue::Float(lf / rf))
            }
            _ => unreachable!(),
        };
    }

    let li = to_int(left)?;
    let ri = to_int(right)?;
    match op {
        Token::Star => Ok(VimValue::Int(li.wrapping_mul(ri))),
        Token::Slash => {
            if ri == 0 {
                return Err(RuntimeError::RuntimeError(CompactString::from(
                    "division by zero",
                )));
            }
            Ok(VimValue::Int(li / ri))
        }
        _ => unreachable!(),
    }
}

/// Evaluate comparison operators.
fn eval_comparison(
    left: &VimValue,
    op: &Token,
    right: &VimValue,
) -> Result<VimValue, RuntimeError> {
    // String comparison: if both are strings, compare lexicographically.
    if let (VimValue::String(ls), VimValue::String(rs)) = (left, right) {
        let result = match op {
            Token::Eq => ls == rs,
            Token::Neq => ls != rs,
            Token::Lt => ls < rs,
            Token::Gt => ls > rs,
            Token::Le => ls <= rs,
            Token::Ge => ls >= rs,
            _ => unreachable!(),
        };
        return Ok(VimValue::Bool(result));
    }

    // Float promotion for numeric comparisons.
    if matches!(left, VimValue::Float(_)) || matches!(right, VimValue::Float(_)) {
        let lf = to_float(left)?;
        let rf = to_float(right)?;
        let result = match op {
            Token::Eq => (lf - rf).abs() < f64::EPSILON,
            Token::Neq => (lf - rf).abs() >= f64::EPSILON,
            Token::Lt => lf < rf,
            Token::Gt => lf > rf,
            Token::Le => lf <= rf,
            Token::Ge => lf >= rf,
            _ => unreachable!(),
        };
        return Ok(VimValue::Bool(result));
    }

    // Integer comparison.
    let li = to_int(left)?;
    let ri = to_int(right)?;
    let result = match op {
        Token::Eq => li == ri,
        Token::Neq => li != ri,
        Token::Lt => li < ri,
        Token::Gt => li > ri,
        Token::Le => li <= ri,
        Token::Ge => li >= ri,
        _ => unreachable!(),
    };
    Ok(VimValue::Bool(result))
}

/// Convert a `VimValue` to a display string (for concatenation).
fn vim_value_to_display(value: &VimValue) -> CompactString {
    match value {
        VimValue::Nil => CompactString::default(),
        VimValue::Bool(b) => {
            if *b {
                CompactString::from("1")
            } else {
                CompactString::from("0")
            }
        }
        VimValue::Int(n) => CompactString::from(n.to_string()),
        VimValue::Float(f) => CompactString::from(format!("{f}")),
        VimValue::String(s) => s.clone(),
        VimValue::List(items) => CompactString::from(
            items
                .iter()
                .map(|v| vim_value_to_display(v).to_string())
                .collect::<Vec<_>>()
                .join(" "),
        ),
        VimValue::Map(_) => CompactString::from("{...}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Unit tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_context() -> ExprContext<'static> {
        let variables = Box::leak(Box::new(VariableStore::default()));
        let options = Box::leak(Box::new(VimOptions::default()));
        ExprContext { variables, options }
    }

    #[test]
    fn tokenize_integer() {
        let tokens = tokenize("42").unwrap();
        assert_eq!(tokens, vec![Token::Int(42)]);
    }

    #[test]
    fn tokenize_float() {
        let tokens = tokenize("3.14").unwrap();
        assert_eq!(tokens, vec![Token::Float(3.14)]);
    }

    #[test]
    fn tokenize_string_double() {
        let tokens = tokenize("\"hello\"").unwrap();
        assert_eq!(tokens, vec![Token::Str(CompactString::from("hello"))]);
    }

    #[test]
    fn tokenize_string_single() {
        let tokens = tokenize("'world'").unwrap();
        assert_eq!(tokens, vec![Token::Str(CompactString::from("world"))]);
    }

    #[test]
    fn tokenize_variable() {
        let tokens = tokenize("g:myvar").unwrap();
        assert_eq!(
            tokens,
            vec![Token::Variable(
                VarScope::Global,
                CompactString::from("myvar")
            )]
        );
    }

    #[test]
    fn tokenize_comparison_operators() {
        let tokens = tokenize("1 == 2 != 3 <= 4 >= 5 < 6 > 7").unwrap();
        assert_eq!(tokens.len(), 13);
        assert_eq!(tokens[1], Token::Eq);
        assert_eq!(tokens[3], Token::Neq);
        assert_eq!(tokens[5], Token::Le);
        assert_eq!(tokens[7], Token::Ge);
        assert_eq!(tokens[9], Token::Lt);
        assert_eq!(tokens[11], Token::Gt);
    }

    #[test]
    fn expression_integer_literal() {
        let runtime = SimpleExprEval;
        let ctx = empty_context();
        assert_eq!(
            runtime.eval_expression("42", &ctx).unwrap(),
            VimValue::Int(42)
        );
    }

    #[test]
    fn expression_float_literal() {
        let runtime = SimpleExprEval;
        let ctx = empty_context();
        assert_eq!(
            runtime.eval_expression("3.14", &ctx).unwrap(),
            VimValue::Float(3.14)
        );
    }

    #[test]
    fn expression_string_literal() {
        let runtime = SimpleExprEval;
        let ctx = empty_context();
        assert_eq!(
            runtime.eval_expression("\"hello\"", &ctx).unwrap(),
            VimValue::String(CompactString::from("hello"))
        );
    }

    #[test]
    fn expression_bool_true() {
        let runtime = SimpleExprEval;
        let ctx = empty_context();
        assert_eq!(
            runtime.eval_expression("true", &ctx).unwrap(),
            VimValue::Bool(true)
        );
    }

    #[test]
    fn expression_bool_false() {
        let runtime = SimpleExprEval;
        let ctx = empty_context();
        assert_eq!(
            runtime.eval_expression("false", &ctx).unwrap(),
            VimValue::Bool(false)
        );
    }

    #[test]
    fn expression_addition() {
        let runtime = SimpleExprEval;
        let ctx = empty_context();
        assert_eq!(
            runtime.eval_expression("2 + 3", &ctx).unwrap(),
            VimValue::Int(5)
        );
    }

    #[test]
    fn expression_subtraction() {
        let runtime = SimpleExprEval;
        let ctx = empty_context();
        assert_eq!(
            runtime.eval_expression("10 - 4", &ctx).unwrap(),
            VimValue::Int(6)
        );
    }

    #[test]
    fn expression_multiplication() {
        let runtime = SimpleExprEval;
        let ctx = empty_context();
        assert_eq!(
            runtime.eval_expression("3 * 7", &ctx).unwrap(),
            VimValue::Int(21)
        );
    }

    #[test]
    fn expression_integer_division() {
        let runtime = SimpleExprEval;
        let ctx = empty_context();
        assert_eq!(
            runtime.eval_expression("10 / 3", &ctx).unwrap(),
            VimValue::Int(3)
        );
    }

    #[test]
    fn expression_unary_minus() {
        let runtime = SimpleExprEval;
        let ctx = empty_context();
        assert_eq!(
            runtime.eval_expression("-5", &ctx).unwrap(),
            VimValue::Int(-5)
        );
    }

    #[test]
    fn expression_parentheses() {
        let runtime = SimpleExprEval;
        let ctx = empty_context();
        assert_eq!(
            runtime.eval_expression("(2 + 3) * 4", &ctx).unwrap(),
            VimValue::Int(20)
        );
    }

    #[test]
    fn expression_string_concat() {
        let runtime = SimpleExprEval;
        let ctx = empty_context();
        assert_eq!(
            runtime
                .eval_expression("\"hello\" . \" world\"", &ctx)
                .unwrap(),
            VimValue::String(CompactString::from("hello world"))
        );
    }

    #[test]
    fn expression_comparison_eq() {
        let runtime = SimpleExprEval;
        let ctx = empty_context();
        assert_eq!(
            runtime.eval_expression("1 == 1", &ctx).unwrap(),
            VimValue::Bool(true)
        );
        assert_eq!(
            runtime.eval_expression("1 == 2", &ctx).unwrap(),
            VimValue::Bool(false)
        );
    }

    #[test]
    fn expression_comparison_gt() {
        let runtime = SimpleExprEval;
        let ctx = empty_context();
        assert_eq!(
            runtime.eval_expression("2 > 3", &ctx).unwrap(),
            VimValue::Bool(false)
        );
        assert_eq!(
            runtime.eval_expression("3 > 2", &ctx).unwrap(),
            VimValue::Bool(true)
        );
    }

    #[test]
    fn expression_division_by_zero() {
        let runtime = SimpleExprEval;
        let ctx = empty_context();
        let err = runtime.eval_expression("1 / 0", &ctx).unwrap_err();
        assert!(matches!(err, RuntimeError::RuntimeError(_)));
    }

    #[test]
    fn expression_unterminated_string() {
        let runtime = SimpleExprEval;
        let ctx = empty_context();
        let err = runtime.eval_expression("\"oops", &ctx).unwrap_err();
        assert!(matches!(err, RuntimeError::CompileError(_)));
    }

    #[test]
    fn expression_undefined_variable() {
        let runtime = SimpleExprEval;
        let ctx = empty_context();
        let err = runtime.eval_expression("g:nope", &ctx).unwrap_err();
        assert!(matches!(err, RuntimeError::RuntimeError(_)));
    }

    #[test]
    fn expression_syntax_error() {
        let runtime = SimpleExprEval;
        let ctx = empty_context();
        let err = runtime.eval_expression("+ +", &ctx).unwrap_err();
        assert!(matches!(err, RuntimeError::CompileError(_)));
    }

    #[test]
    fn expression_operator_precedence() {
        let runtime = SimpleExprEval;
        let ctx = empty_context();
        // 2 + 3 * 4 = 2 + 12 = 14
        assert_eq!(
            runtime.eval_expression("2 + 3 * 4", &ctx).unwrap(),
            VimValue::Int(14)
        );
    }

    #[test]
    fn expression_float_arithmetic() {
        let runtime = SimpleExprEval;
        let ctx = empty_context();
        assert_eq!(
            runtime.eval_expression("1.5 + 2.5", &ctx).unwrap(),
            VimValue::Float(4.0)
        );
    }

    #[test]
    fn expression_float_promotion() {
        let runtime = SimpleExprEval;
        let ctx = empty_context();
        assert_eq!(
            runtime.eval_expression("1 + 2.5", &ctx).unwrap(),
            VimValue::Float(3.5)
        );
    }

    #[test]
    fn expression_variable_read() {
        let mut vars = VariableStore::default();
        vars.set(
            VarScope::Global,
            "name",
            VimValue::String(CompactString::from("vim")),
        );
        let options = Box::leak(Box::new(VimOptions::default()));
        let ctx = ExprContext {
            variables: Box::leak(Box::new(vars)),
            options,
        };
        let runtime = SimpleExprEval;
        assert_eq!(
            runtime.eval_expression("g:name", &ctx).unwrap(),
            VimValue::String(CompactString::from("vim"))
        );
    }

    #[test]
    fn expression_buffer_variable() {
        let mut vars = VariableStore::default();
        vars.set(VarScope::Buffer, "count", VimValue::Int(7));
        let options = Box::leak(Box::new(VimOptions::default()));
        let ctx = ExprContext {
            variables: Box::leak(Box::new(vars)),
            options,
        };
        let runtime = SimpleExprEval;
        assert_eq!(
            runtime.eval_expression("b:count", &ctx).unwrap(),
            VimValue::Int(7)
        );
    }
}
