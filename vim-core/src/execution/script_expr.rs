//! Vim script expressions, as a config evaluates them.
//!
//! The legacy script expressions of Vim's `eval1()` to `eval9()`
//! (`eval.c`) on Numbers, Floats, Strings and `v:true`/`v:false`: `?:`,
//! `||`, `&&`, the comparisons, `+ - . ..`, `* / %`, the prefix `! - +`,
//! number and string literals, `&option`, `$NAME`, variables, and calls of
//! `has()` and `exists()`.
//!
//! Vim also has Lists, Dictionaries, lambdas, registers, `??`, `<<`, `>>`,
//! `->` and subscripts. They give E319 here, with the expression.
//!
//! The text comes from files the user may not have written, so the parser
//! keeps what waits for its right part on a stack of its own instead of
//! recursing, and nothing in it can panic.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::ops::Range;

use compact_str::CompactString;

use crate::errors::VimError;
use crate::grammar::command_meta::cmd_exists;
use crate::keymap::{Key, KeyEvent, Modifiers};
use crate::primitives::{OptionValue, VarScope, VimOptions, VimValue};
use crate::state::VariableStore;

use super::executor_ex::{hidden_option, option_value, SetKind};

/// `v:version`: Vim's `VIM_VERSION_100` for 9.1.
const VERSION: i64 = 901;

/// `v:versionlong` at 9.1.0.
const VERSION_LONG: i64 = 9_010_000;

/// How deep groups and calls nest before E1169: the limit of Vim built
/// with MSVC, as the Windows editors are. Other builds allow 1000.
const MAX_DEPTH: usize = 300;

/// Vim's `MAX_FUNC_ARGS`.
const MAX_ARGS: usize = 20;

/// The scope letters of a variable name (`NAMESPACE_CHAR`).
const NAMESPACES: &[u8] = b"abglstvw";

/// A value of an expression.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Number(i64),
    Float(f64),
    String(String),
    /// `v:true` or `v:false`.
    Bool(bool),
}

impl Value {
    /// The Number of a value, as `tv_get_number_chk()` gives it: a String
    /// by the number it starts with, `v:true` 1. A Float is E805.
    pub(crate) fn number(&self) -> Result<i64, VimError> {
        match self {
            Self::Number(n) => Ok(*n),
            Self::String(s) => Ok(string_number(s)),
            Self::Bool(b) => Ok(i64::from(*b)),
            Self::Float(_) => Err(VimError::FloatAsNumber),
        }
    }

    /// The String of a value, as `tv_get_string()` gives it in a legacy
    /// script.
    pub(crate) fn text(&self) -> Cow<'_, str> {
        match self {
            Self::String(s) => Cow::Borrowed(s),
            Self::Number(n) => Cow::Owned(n.to_string()),
            Self::Float(f) => Cow::Owned(float_text(*f)),
            Self::Bool(true) => Cow::Borrowed("v:true"),
            Self::Bool(false) => Cow::Borrowed("v:false"),
        }
    }

    fn into_text(self) -> String {
        match self {
            Self::String(s) => s,
            other => other.text().into_owned(),
        }
    }

    /// The Float of a value, as `tv_get_float_chk()` gives it.
    const fn float(&self) -> Result<f64, VimError> {
        match self {
            Self::Float(f) => Ok(*f),
            Self::Number(n) => Ok(number_float(*n)),
            Self::String(_) => Err(VimError::StringAsFloat),
            Self::Bool(_) => Err(VimError::BoolAsFloat),
        }
    }

    /// The value as the variable store holds it.
    pub(crate) fn into_variable(self) -> VimValue {
        match self {
            Self::Number(n) => VimValue::Int(n),
            Self::Float(f) => VimValue::Float(f),
            Self::String(s) => VimValue::String(s.into()),
            Self::Bool(b) => VimValue::Bool(b),
        }
    }

    /// A value the variable store holds, or `None` for a List, a
    /// Dictionary or no value, which the evaluator does not have.
    pub(crate) fn from_variable(value: &VimValue) -> Option<Self> {
        match value {
            VimValue::Int(n) => Some(Self::Number(*n)),
            VimValue::Float(f) => Some(Self::Float(*f)),
            VimValue::String(s) => Some(Self::String(s.to_string())),
            VimValue::Bool(b) => Some(Self::Bool(*b)),
            _ => None,
        }
    }

    const fn is_float(&self) -> bool {
        matches!(self, Self::Float(_))
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "Vim converts a Number to a Float the same way"
)]
const fn number_float(n: i64) -> f64 {
    n as f64
}

/// What an expression reads.
pub(crate) struct Env<'a> {
    /// The `g:` and `b:` variables.
    pub(crate) variables: &'a VariableStore,
    /// The options in effect, for `&name` and `&l:name`.
    pub(crate) options: &'a VimOptions,
    /// The global values, for `&g:name`.
    pub(crate) global_options: &'a VimOptions,
}

/// A whole expression evaluated: its value, unless it failed, and the
/// errors it gave, which an expression that does not fail can give too.
pub(crate) struct Evaluated {
    pub(crate) value: Option<Value>,
    pub(crate) errors: Vec<VimError>,
}

/// Evaluate `text` as one whole expression, as `eval0()` does: E15 when it
/// is none and nothing more specific was said, E488 for text after it.
pub(crate) fn evaluate(text: &str, env: &Env<'_>) -> Evaluated {
    let mut parser = Parser::new(text, env);
    parser.skip_blanks();
    parser.start = parser.pos;
    let result = parser.expr1(true);
    parser.skip_blanks();
    let quiet = parser.errors.is_empty();
    let value = match result {
        Ok(value) if matches!(parser.peek(), None | Some(b'|' | b'\n' | b'"')) => Some(value),
        Ok(_) => {
            if quiet {
                let rest = parser.rest();
                parser
                    .errors
                    .push(VimError::TrailingCharacters(rest.into()));
            }
            None
        }
        Err(Fail) => {
            if quiet {
                let expr = parser.text.get(parser.start..).unwrap_or_default();
                parser.errors.push(VimError::InvalidExpression(expr.into()));
            }
            None
        }
    };
    Evaluated {
        value,
        errors: parser.errors,
    }
}

/// The value of an option an expression or `:let` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OptionLayer {
    /// `&name`: the value in effect, and both values for `:let`.
    Both,
    /// `&l:name`.
    Local,
    /// `&g:name`.
    Global,
}

/// What `:let` assigns to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target<'a> {
    Variable(Variable<'a>),
    Option(OptionLayer, &'a str),
}

/// A `g:`, `b:` or plain variable that `:let` and `:unlet` can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Variable<'a> {
    pub(crate) scope: VarScope,
    /// The name in its scope.
    pub(crate) name: &'a str,
    /// The name as written, scope included.
    pub(crate) written: &'a str,
}

/// `:let {target} {op}= {expr}`, as `ex_let()` reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Assignment<'a> {
    pub(crate) target: Target<'a>,
    /// `+ - * / %` or `.` (for `.=` and `..=`) before the `=`, `None`
    /// for `=` alone.
    pub(crate) op: Option<char>,
    pub(crate) expr: &'a str,
}

/// Read the argument of `:let`. `None` for a form the evaluator lacks: a
/// List of variables, a heredoc, a listing of variables, a register, an
/// environment variable, an `s:`, `w:`, `t:`, `l:`, `a:` or `v:`
/// variable, or a subscript.
pub(crate) fn parse_let(arg: &str) -> Option<Assignment<'_>> {
    let (target, end) = if arg.starts_with('&') {
        let (layer, name, end) = option_name(arg, 0)?;
        (Target::Option(layer, arg.get(name)?), end)
    } else {
        let (variable, end) = variable_name(arg)?;
        // A List item or a Dictionary member.
        let bytes = arg.as_bytes();
        let member = bytes.get(end) == Some(&b'.')
            && bytes
                .get(end + 1)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_');
        if bytes.get(end) == Some(&b'[') || member {
            return None;
        }
        (Target::Variable(variable), end)
    };
    let rest = arg.get(skip_blanks(arg, end)..)?;
    let (op, len) = match rest.as_bytes() {
        [b'=', b'<', b'<', ..] => return None,
        [b'=', ..] => (None, 1),
        [b'.', b'.', b'=', ..] => (Some('.'), 3),
        [c @ (b'+' | b'-' | b'*' | b'/' | b'%' | b'.'), b'=', ..] => (Some(char::from(*c)), 2),
        _ => return None,
    };
    let expr = rest.get(len..)?;
    Some(Assignment {
        target,
        op,
        expr: expr.get(skip_blanks(expr, 0)..)?,
    })
}

/// The variable named at the start of `text` and the end of its name,
/// for `:let` and `:unlet`: `g:`, `b:` or plain; not another scope, nor
/// `version`, which is `v:version`.
pub(crate) fn variable_name(text: &str) -> Option<(Variable<'_>, usize)> {
    let (end, curly) = name_end(text, 0);
    let written = text.get(..end)?;
    if curly || end == 0 || written.starts_with('<') || written == "version" {
        return None;
    }
    let (scope, name) = match written.as_bytes() {
        [b'g', b':', ..] => (VarScope::Global, written.get(2..)?),
        [b'b', b':', ..] => (VarScope::Buffer, written.get(2..)?),
        [_, b':', ..] => return None,
        _ => (VarScope::Global, written),
    };
    // A `#` or `:` belongs only in a global name.
    if name.is_empty() || (scope == VarScope::Buffer && name.contains([':', '#'])) {
        return None;
    }
    Some((
        Variable {
            scope,
            name,
            written,
        },
        end,
    ))
}

/// `{var} {op}= {value}`, as `tv_op()` computes it: `+ - * / %` on
/// Numbers and Floats, `.` on Strings. Any other mix is E734.
pub(crate) fn assign_op(op: char, current: &Value, value: &Value) -> Result<Value, VimError> {
    let wrong = || VimError::WrongVariableType(op);
    let number = |v: &Value| match v {
        Value::Float(_) => 0,
        other => other.number().unwrap_or(0),
    };
    match (current, value) {
        (Value::Bool(_), _) => Err(wrong()),
        (_, Value::Bool(_)) if op != '.' => Err(wrong()),
        (Value::Float(_), _) | (_, Value::Float(_)) if matches!(op, '%' | '.') => Err(wrong()),
        (Value::Float(f), rhs) => {
            let g = match rhs {
                Value::Float(g) => *g,
                other => number_float(number(other)),
            };
            Ok(Value::Float(float_op(op, *f, g)))
        }
        (lhs, Value::Float(g)) => Ok(Value::Float(float_op(op, number_float(number(lhs)), *g))),
        (lhs, rhs) if op == '.' => Ok(Value::String(format!("{}{}", lhs.text(), rhs.text()))),
        (lhs, rhs) => Ok(Value::Number(number_op(op, number(lhs), number(rhs)))),
    }
}

/// `{a} {op} {b}` on Numbers, wrapping, with Vim's division by zero.
pub(crate) const fn number_op(op: char, a: i64, b: i64) -> i64 {
    match op {
        '+' => a.wrapping_add(b),
        '-' => a.wrapping_sub(b),
        '*' => a.wrapping_mul(b),
        '/' => num_divide(a, b),
        _ => num_modulus(a, b),
    }
}

fn float_op(op: char, a: f64, b: f64) -> f64 {
    match op {
        '+' => a + b,
        '-' => a - b,
        '*' => a * b,
        _ => a / b,
    }
}

/// What `:echo` shows, in order.
#[derive(Debug, PartialEq)]
pub(crate) enum Echoed {
    /// The values so far, joined by a space.
    Text(String),
    Error(VimError),
}

/// Run the argument of `:echo` as `ex_echo()` does: each expression in
/// turn, the values joined by a space. An expression that fails ends it,
/// after the values before it.
pub(crate) fn echo(text: &str, env: &Env<'_>) -> Vec<Echoed> {
    let mut parser = Parser::new(text, env);
    let mut shown = Vec::new();
    let mut line = String::new();
    let mut values = 0usize;
    parser.skip_blanks();
    while !matches!(parser.peek(), None | Some(b'|' | b'\n')) {
        parser.start = parser.pos;
        let result = parser.expr1(true);
        if result.is_err() && parser.errors.is_empty() {
            let expr = parser.text.get(parser.start..).unwrap_or_default();
            parser.errors.push(VimError::InvalidExpression(expr.into()));
        }
        if !parser.errors.is_empty() {
            if !line.is_empty() {
                shown.push(Echoed::Text(std::mem::take(&mut line)));
            }
            shown.extend(parser.errors.drain(..).map(Echoed::Error));
        }
        let Ok(value) = result else {
            return shown;
        };
        if values > 0 {
            line.push(' ');
        }
        line.push_str(&value.text());
        values += 1;
        parser.skip_blanks();
    }
    if !line.is_empty() || shown.is_empty() {
        shown.push(Echoed::Text(line));
    }
    shown
}

/// An expression failed; its errors are in [`Parser::errors`].
#[derive(Debug)]
struct Fail;

/// A comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Compare {
    Equal,
    NotEqual,
    Greater,
    GreaterEqual,
    Smaller,
    SmallerEqual,
    Match,
    NoMatch,
    Is,
    IsNot,
}

/// A binary operator other than `&&` and `||`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BinOp {
    /// With whether it ignores case.
    Compare(Compare, bool),
    Add,
    Sub,
    Concat,
    Mul,
    Div,
    Mod,
}

impl BinOp {
    /// Its precedence, from `eval4()` (4) to `eval7()` (7).
    const fn level(self) -> u8 {
        match self {
            Self::Compare(..) => 4,
            Self::Add | Self::Sub | Self::Concat => 6,
            Self::Mul | Self::Div | Self::Mod => 7,
        }
    }
}

/// What can follow an operand.
enum Token {
    Binary(BinOp, usize),
    /// `&&` (`true`) or `||`.
    Logic(bool),
    Question,
    Colon,
    Close,
    Comma,
    /// Text Vim reads as an operator this evaluator does not have.
    Unsupported,
    /// Anything else, which ends the expression.
    End,
}

/// A part of an expression that waits for what follows it.
enum Frame {
    /// A binary operator and its left operand.
    Binary { op: BinOp, lhs: Value, ev: bool },
    /// `&&` (`and`) or `||`, and whether its left operand is true.
    Logic { and: bool, lhs: bool, ev: bool },
    /// `(`, after the prefix operators at `leaders`.
    Group { leaders: Range<usize>, ev: bool },
    /// A call of the function named at `name`, and its arguments so far.
    Call {
        name: Range<usize>,
        leaders: Range<usize>,
        args: Vec<Value>,
        ev: bool,
    },
    /// `?` and its condition, waiting for `:`.
    Then { cond: bool, ev: bool },
    /// `:`, the condition and the value before it.
    Else { cond: bool, then: Value, ev: bool },
}

impl Frame {
    /// Whether what comes next in this frame is evaluated. Only the side
    /// that `&&`, `||` and `?:` take is; the other is only read, as Vim
    /// reads it without `EVAL_EVALUATE`.
    const fn inner_ev(&self) -> bool {
        match self {
            Self::Binary { ev, .. } | Self::Group { ev, .. } | Self::Call { ev, .. } => *ev,
            Self::Logic { and, lhs, ev } => *ev && *lhs == *and,
            Self::Then { cond, ev } => *ev && *cond,
            Self::Else { cond, ev, .. } => *ev && !*cond,
        }
    }
}

/// Reads and evaluates expressions in `text`.
struct Parser<'a> {
    text: &'a str,
    pos: usize,
    /// Where the current expression starts, for E319.
    start: usize,
    env: &'a Env<'a>,
    errors: Vec<VimError>,
    /// The groups and calls open around the next operand.
    depth: usize,
}

impl<'a> Parser<'a> {
    const fn new(text: &'a str, env: &'a Env<'a>) -> Self {
        Self {
            text,
            pos: 0,
            start: 0,
            env,
            errors: Vec::new(),
            depth: 0,
        }
    }

    fn byte(&self, at: usize) -> Option<u8> {
        self.text.as_bytes().get(at).copied()
    }

    fn peek(&self) -> Option<u8> {
        self.byte(self.pos)
    }

    fn rest(&self) -> &'a str {
        self.text.get(self.pos..).unwrap_or_default()
    }

    fn slice(&self, range: Range<usize>) -> &'a str {
        self.text.get(range).unwrap_or_default()
    }

    fn skip_blanks(&mut self) {
        self.pos = skip_blanks(self.text, self.pos);
    }

    fn fail<T>(&mut self, error: VimError) -> Result<T, Fail> {
        self.errors.push(error);
        Err(Fail)
    }

    /// E319 for a form Vim has and this evaluator does not.
    fn unsupported<T>(&mut self) -> Result<T, Fail> {
        let expr = self
            .text
            .get(self.start..)
            .unwrap_or_default()
            .trim_end_matches([' ', '\t']);
        self.fail(VimError::NotAvailable(CompactString::from(expr)))
    }

    /// Read one expression from `pos`, as `eval1()` does, and leave `pos`
    /// after it. Without `ev` it is only read.
    fn expr1(&mut self, ev: bool) -> Result<Value, Fail> {
        let mut stack = Vec::new();
        let result = self.expr1_with(&mut stack, ev);
        if result.is_err() {
            // `get_func_tv()`: each call around the failure gives E116.
            for frame in stack.into_iter().rev() {
                if let Frame::Call { name, ev: true, .. } = frame {
                    let name = self.slice(name);
                    self.errors.push(VimError::InvalidArguments(name.into()));
                }
            }
            self.depth = 0;
        }
        result
    }

    fn expr1_with(&mut self, stack: &mut Vec<Frame>, base: bool) -> Result<Value, Fail> {
        'operand: loop {
            let ev = stack.last().map_or(base, Frame::inner_ev);
            let want_string = matches!(
                stack.last(),
                Some(Frame::Binary {
                    op: BinOp::Concat,
                    ..
                })
            );
            let Some(mut value) = self.operand(stack, ev, want_string)? else {
                continue 'operand;
            };
            loop {
                self.after_operand()?;
                self.skip_blanks();
                match self.operator() {
                    Token::Binary(op, len) => {
                        let compare = matches!(op, BinOp::Compare(..));
                        // `eval4()` reads one comparison: a second one ends
                        // the expression.
                        let floor = if compare { 5 } else { op.level() };
                        value = self.reduce(stack, floor, value)?;
                        if compare
                            && matches!(
                                stack.last(),
                                Some(Frame::Binary {
                                    op: BinOp::Compare(..),
                                    ..
                                })
                            )
                        {
                            return self.end(stack, value);
                        }
                        self.pos += len;
                        self.skip_blanks();
                        let ev = stack.last().map_or(base, Frame::inner_ev);
                        stack.push(Frame::Binary { op, lhs: value, ev });
                        continue 'operand;
                    }
                    Token::Logic(and) => {
                        value = self.reduce(stack, if and { 3 } else { 2 }, value)?;
                        let ev = stack.last().map_or(base, Frame::inner_ev);
                        let lhs = ev && self.truth(&value)?;
                        self.pos += 2;
                        self.skip_blanks();
                        stack.push(Frame::Logic { and, lhs, ev });
                        continue 'operand;
                    }
                    Token::Question => {
                        value = self.reduce(stack, 0, value)?;
                        let ev = stack.last().map_or(base, Frame::inner_ev);
                        let cond = ev && self.truth(&value)?;
                        self.pos += 1;
                        self.skip_blanks();
                        stack.push(Frame::Then { cond, ev });
                        continue 'operand;
                    }
                    Token::Colon => {
                        value = self.reduce(stack, 0, value)?;
                        match stack.pop() {
                            Some(Frame::Then { cond, ev }) => {
                                self.pos += 1;
                                self.skip_blanks();
                                stack.push(Frame::Else {
                                    cond,
                                    then: value,
                                    ev,
                                });
                                continue 'operand;
                            }
                            Some(Frame::Else { cond, then, .. }) => {
                                if cond {
                                    value = then;
                                }
                            }
                            Some(frame) => {
                                stack.push(frame);
                                return self.end(stack, value);
                            }
                            None => return Ok(value),
                        }
                    }
                    Token::Close => {
                        value = self.close_branches(stack, value)?;
                        match stack.pop() {
                            Some(Frame::Group { leaders, ev }) => {
                                self.depth = self.depth.saturating_sub(1);
                                self.pos += 1;
                                value = self.apply_leaders(leaders, value, ev)?;
                            }
                            Some(Frame::Call {
                                name,
                                leaders,
                                mut args,
                                ev,
                            }) => {
                                args.push(value);
                                self.pos += 1;
                                value = self.finish_call(name, leaders, &args, ev)?;
                            }
                            Some(frame) => {
                                stack.push(frame);
                                return Err(Fail);
                            }
                            None => return Ok(value),
                        }
                    }
                    Token::Comma => {
                        value = self.close_branches(stack, value)?;
                        let Some(Frame::Call { args, .. }) = stack.last_mut() else {
                            return self.end(stack, value);
                        };
                        args.push(value);
                        if args.len() >= MAX_ARGS {
                            return self.too_many_arguments(stack);
                        }
                        self.pos += 1;
                        match self.argument_start(stack)? {
                            Some(done) => value = done,
                            None => continue 'operand,
                        }
                    }
                    Token::Unsupported => return self.unsupported(),
                    Token::End => return self.end(stack, value),
                }
            }
        }
    }

    /// Read an operand with its prefix operators, as `eval9()` does.
    /// `None` when it opened a group or a call, whose contents come next.
    fn operand(
        &mut self,
        stack: &mut Vec<Frame>,
        ev: bool,
        want_string: bool,
    ) -> Result<Option<Value>, Fail> {
        let lead = self.pos;
        while matches!(self.peek(), Some(b'!' | b'-' | b'+')) {
            self.pos += 1;
            self.skip_blanks();
        }
        let leaders = lead..self.pos;
        if self.peek() == Some(b'.') {
            let rest = self.rest();
            return self.fail(VimError::InvalidExpression(rest.into()));
        }
        if self.depth >= MAX_DEPTH {
            let rest = self.rest();
            return self.fail(VimError::ExpressionTooRecursive(rest.into()));
        }
        let value = match self.peek() {
            Some(b'0'..=b'9') => self.number(ev, want_string)?,
            Some(b'"') => self.double_quoted(ev)?,
            Some(b'\'') => self.single_quoted(ev)?,
            Some(b'&') => self.option(ev)?,
            // An interpolated string.
            Some(b'$') if matches!(self.byte(self.pos + 1), Some(b'"' | b'\'')) => {
                return self.unsupported();
            }
            Some(b'$') => self.environment(ev)?,
            Some(b'(') => {
                self.pos += 1;
                self.skip_blanks();
                self.depth += 1;
                stack.push(Frame::Group { leaders, ev });
                return Ok(None);
            }
            Some(b'[' | b'{' | b'@') => return self.unsupported(),
            Some(b'#') if self.byte(self.pos + 1) == Some(b'{') => return self.unsupported(),
            _ => return self.name(stack, leaders, ev),
        };
        self.apply_leaders(leaders, value, ev).map(Some)
    }

    /// A subscript or a method after an operand, which Vim's
    /// `handle_subscript()` reads.
    fn after_operand(&mut self) -> Result<(), Fail> {
        if self.peek() == Some(b'[') {
            return self.unsupported();
        }
        let at = skip_blanks(self.text, self.pos);
        if self.text.get(at..).is_some_and(|s| s.starts_with("->")) {
            return self.unsupported();
        }
        Ok(())
    }

    /// The operator at `pos`, as `eval1()` to `eval7()` look for one.
    fn operator(&self) -> Token {
        let at = |i: usize| self.byte(self.pos + i);
        let assign = at(1) == Some(b'=');
        match (at(0), at(1)) {
            (Some(b'|'), Some(b'|')) => Token::Logic(false),
            (Some(b'&'), Some(b'&')) => Token::Logic(true),
            (Some(b'<'), Some(b'<')) | (Some(b'>'), Some(b'>')) | (Some(b'?'), Some(b'?')) => {
                Token::Unsupported
            }
            (Some(b'?'), _) => Token::Question,
            (Some(b':'), _) => Token::Colon,
            (Some(b')'), _) => Token::Close,
            (Some(b','), _) => Token::Comma,
            // `+=`, `.=`, `..=` and the like end the expression.
            (Some(b'+'), _) if !assign => Token::Binary(BinOp::Add, 1),
            (Some(b'-'), _) if !assign => Token::Binary(BinOp::Sub, 1),
            (Some(b'*'), _) if !assign => Token::Binary(BinOp::Mul, 1),
            (Some(b'/'), _) if !assign => Token::Binary(BinOp::Div, 1),
            (Some(b'%'), _) if !assign => Token::Binary(BinOp::Mod, 1),
            (Some(b'.'), Some(b'.')) if at(2) != Some(b'=') => Token::Binary(BinOp::Concat, 2),
            (Some(b'.'), _) if !assign && at(1) != Some(b'.') => Token::Binary(BinOp::Concat, 1),
            _ => self.comparison(),
        }
    }

    /// The comparison at `pos`, as `get_compare_type()` reads it, with its
    /// `?` or `#`.
    fn comparison(&self) -> Token {
        let at = |i: usize| self.byte(self.pos + i);
        let (compare, mut len) = match (at(0), at(1)) {
            (Some(b'='), Some(b'=')) => (Compare::Equal, 2),
            (Some(b'='), Some(b'~')) => (Compare::Match, 2),
            (Some(b'!'), Some(b'=')) => (Compare::NotEqual, 2),
            (Some(b'!'), Some(b'~')) => (Compare::NoMatch, 2),
            (Some(b'>'), Some(b'=')) => (Compare::GreaterEqual, 2),
            (Some(b'>'), _) => (Compare::Greater, 1),
            (Some(b'<'), Some(b'=')) => (Compare::SmallerEqual, 2),
            (Some(b'<'), _) => (Compare::Smaller, 1),
            (Some(b'i'), Some(b's')) => {
                let isnot = self.rest().get(2..5) == Some("not");
                let len = if isnot { 5 } else { 2 };
                // Not the start of a name.
                if at(len).is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_') {
                    return Token::End;
                }
                (if isnot { Compare::IsNot } else { Compare::Is }, len)
            }
            _ => return Token::End,
        };
        let ic = match at(len) {
            Some(b'?') => true,
            Some(b'#') => false,
            _ => return Token::Binary(BinOp::Compare(compare, self.env.options.ignorecase()), len),
        };
        len += 1;
        Token::Binary(BinOp::Compare(compare, ic), len)
    }

    /// Apply the binary operators on the stack whose precedence is at
    /// least `floor`, the latest first.
    fn reduce(
        &mut self,
        stack: &mut Vec<Frame>,
        floor: u8,
        mut value: Value,
    ) -> Result<Value, Fail> {
        loop {
            match stack.pop() {
                Some(Frame::Binary { op, lhs, ev }) if op.level() >= floor => {
                    value = if ev {
                        self.binary(op, lhs, &value)?
                    } else {
                        Value::Number(0)
                    };
                }
                Some(Frame::Logic { and, lhs, ev }) if (if and { 3 } else { 2 }) >= floor => {
                    // `eval2()` and `eval3()`: a Number, the right operand
                    // read only when the left one does not decide.
                    value = if !ev {
                        Value::Number(0)
                    } else if lhs == and {
                        Value::Number(i64::from(self.truth(&value)?))
                    } else {
                        Value::Number(i64::from(lhs))
                    };
                }
                Some(frame) => {
                    stack.push(frame);
                    return Ok(value);
                }
                None => return Ok(value),
            }
        }
    }

    /// Apply every binary operator and finish every `?:` back to the
    /// innermost group or call. A `?` without its `:` is E109.
    fn close_branches(&mut self, stack: &mut Vec<Frame>, value: Value) -> Result<Value, Fail> {
        let mut value = self.reduce(stack, 0, value)?;
        loop {
            match stack.pop() {
                Some(Frame::Else { cond, then, .. }) => {
                    if cond {
                        value = then;
                    }
                    value = self.reduce(stack, 0, value)?;
                }
                Some(Frame::Then { .. }) => return self.fail(VimError::MissingColon),
                Some(frame) => {
                    stack.push(frame);
                    return Ok(value);
                }
                None => return Ok(value),
            }
        }
    }

    /// The expression ends before `pos`: E110 inside a group, E116 inside
    /// a call (when the calls around unwind).
    fn end(&mut self, stack: &mut Vec<Frame>, value: Value) -> Result<Value, Fail> {
        let value = self.close_branches(stack, value)?;
        match stack.last() {
            None => Ok(value),
            Some(Frame::Group { .. }) => self.fail(VimError::MissingParen),
            Some(_) => Err(Fail),
        }
    }

    /// After the `(` of a call or a `,` between its arguments, as
    /// `get_func_arguments()` reads them: the value of the call when a `)`
    /// ends it there.
    fn argument_start(&mut self, stack: &mut Vec<Frame>) -> Result<Option<Value>, Fail> {
        self.skip_blanks();
        match self.peek() {
            Some(b')') => {
                let Some(Frame::Call {
                    name,
                    leaders,
                    args,
                    ev,
                }) = stack.pop()
                else {
                    return Err(Fail);
                };
                self.pos += 1;
                self.finish_call(name, leaders, &args, ev).map(Some)
            }
            None | Some(b',') => Err(Fail),
            Some(_) => Ok(None),
        }
    }

    /// A call with more than `MAX_ARGS` arguments: E740 for it, E116 for
    /// the calls around it.
    fn too_many_arguments(&mut self, stack: &mut Vec<Frame>) -> Result<Value, Fail> {
        if let Some(Frame::Call { name, ev, .. }) = stack.pop() {
            self.depth = self.depth.saturating_sub(1);
            if ev {
                let name = self.slice(name);
                self.errors.push(VimError::ArgumentLimit(name.into()));
            }
        }
        Err(Fail)
    }

    fn finish_call(
        &mut self,
        name: Range<usize>,
        leaders: Range<usize>,
        args: &[Value],
        ev: bool,
    ) -> Result<Value, Fail> {
        self.depth = self.depth.saturating_sub(1);
        let value = if ev {
            self.call(self.slice(name), args)?
        } else {
            Value::Number(0)
        };
        self.apply_leaders(leaders, value, ev)
    }

    /// Call a function, as `call_func()` calls a builtin one. Only `has()`
    /// and `exists()` exist; any other name is E117.
    fn call(&mut self, name: &str, args: &[Value]) -> Result<Value, Fail> {
        let (min, max) = match name {
            "has" => (1, 2),
            "exists" => (1, 1),
            _ => return self.fail(VimError::UnknownFunction(name.into())),
        };
        if args.len() < min {
            return self.fail(VimError::NotEnoughArguments(name.into()));
        }
        if args.len() > max {
            return self.fail(VimError::TooManyArguments(name.into()));
        }
        let arg = args.first().map(Value::text).unwrap_or_default();
        let n = if name == "has" {
            self.has(&arg, args.get(1))
        } else {
            self.exists(&arg)
        };
        Ok(Value::Number(n))
    }

    /// `has({feature} [, {check}])`, as `f_has()` answers: with a {check}
    /// that is not zero, whether the name is a feature at all.
    fn has(&mut self, name: &str, check: Option<&Value>) -> i64 {
        let (known, present) = feature(name);
        if let Some(check) = check {
            // `tv_get_bool()`: an error, and then -1.
            let check = check.number().unwrap_or_else(|error| {
                self.errors.push(error);
                -1
            });
            if check != 0 {
                return i64::from(known);
            }
        }
        i64::from(present)
    }

    /// `exists({expr})`, as `f_exists()` answers.
    fn exists(&self, arg: &str) -> i64 {
        let rest_is_blank = |end: usize| skip_blanks(arg, end) == arg.len();
        match arg.as_bytes().first() {
            Some(b'&' | b'+') => {
                let Some((layer, name, end)) = option_name(arg, 0) else {
                    return 0;
                };
                let name = arg.get(name).unwrap_or_default();
                // eval_option(): `+name` is not "working" for an option this
                // build lacks, `&name` exists.
                let found = self.option_value(layer, name).is_some()
                    || (arg.starts_with('&') && hidden_option(self.env.options, name).is_some());
                i64::from(found && rest_is_blank(end))
            }
            Some(b'*') => {
                let (end, curly) = name_end(arg, 1);
                let after = skip_blanks(arg, end);
                let ok = !curly && (after == arg.len() || arg.as_bytes().get(after) == Some(&b'('));
                i64::from(ok && is_function(arg.get(1..end).unwrap_or_default()))
            }
            Some(b'?') => i64::from(is_function(arg.get(1..).unwrap_or_default())),
            Some(b':') => cmd_exists(arg.get(1..).unwrap_or_default()),
            // No environment and no autocommands.
            Some(b'$' | b'#') | None => 0,
            Some(_) => {
                let (end, curly) = name_end(arg, 0);
                let name = arg.get(..end).unwrap_or_default();
                let found = !curly && end > 0 && !matches!(self.variable(name), Lookup::Missing);
                i64::from(found && rest_is_blank(end))
            }
        }
    }

    /// A number constant, as `eval_number()` reads one. After `.` (with
    /// `want_string`) a float is not read, so `1.2.3` is a String.
    fn number(&mut self, ev: bool, want_string: bool) -> Result<Value, Fail> {
        let bytes = self.text.as_bytes();
        let start = self.pos;
        if !want_string {
            if let Some(end) = float_end(bytes, start) {
                self.pos = end;
                let f = self.slice(start..end).parse().unwrap_or_default();
                return Ok(Value::Float(f));
            }
        }
        if matches!(bytes.get(start + 1), Some(b'z' | b'Z')) && bytes.get(start) == Some(&b'0') {
            return self.unsupported();
        }
        match str2nr(bytes.get(start..).unwrap_or_default(), true) {
            Some((n, len)) => {
                self.pos = start + len;
                Ok(Value::Number(n))
            }
            None if ev => {
                let rest = self.rest();
                self.fail(VimError::InvalidExpression(rest.into()))
            }
            None => Err(Fail),
        }
    }

    /// A `"string"`, as `eval_string()` reads it.
    fn double_quoted(&mut self, ev: bool) -> Result<Value, Fail> {
        let bytes = self.text.as_bytes();
        let open = self.pos;
        let mut p = open + 1;
        loop {
            match bytes.get(p) {
                None => {
                    let rest = self.rest();
                    return self.fail(VimError::MissingDoubleQuote(rest.into()));
                }
                Some(b'"') => break,
                Some(b'\\') if p + 1 < bytes.len() => p += 2,
                Some(_) => p += 1,
            }
        }
        self.pos = p + 1;
        if !ev {
            return Ok(Value::Number(0));
        }
        Ok(Value::String(unescape(self.slice(open + 1..p))))
    }

    /// A `'string'`, as `eval_lit_string()` reads it: `''` is one quote.
    fn single_quoted(&mut self, ev: bool) -> Result<Value, Fail> {
        let bytes = self.text.as_bytes();
        let open = self.pos;
        let mut p = open + 1;
        loop {
            match bytes.get(p) {
                None => {
                    let rest = self.rest();
                    return self.fail(VimError::MissingSingleQuote(rest.into()));
                }
                Some(b'\'') if bytes.get(p + 1) == Some(&b'\'') => p += 2,
                Some(b'\'') => break,
                Some(_) => p += 1,
            }
        }
        self.pos = p + 1;
        if !ev {
            return Ok(Value::Number(0));
        }
        Ok(Value::String(self.slice(open + 1..p).replace("''", "'")))
    }

    /// `&name`, `&g:name` or `&l:name`, as `eval_option()` reads it.
    fn option(&mut self, ev: bool) -> Result<Value, Fail> {
        let Some((layer, name, end)) = option_name(self.text, self.pos) else {
            let rest = self.rest();
            return self.fail(VimError::OptionNameMissing(rest.into()));
        };
        self.pos = end;
        if !ev {
            return Ok(Value::Number(0));
        }
        let name = self.slice(name);
        match self.option_value(layer, name) {
            Some(OptionValue::Bool(on)) => Ok(Value::Number(i64::from(on))),
            Some(OptionValue::Unsigned(n)) => {
                Ok(Value::Number(i64::try_from(n).unwrap_or(i64::MAX)))
            }
            Some(OptionValue::Signed(n)) => Ok(Value::Number(n)),
            Some(OptionValue::Str(s)) => Ok(Value::String(s.to_string())),
            // An option this build lacks reads as 0 or an empty String.
            None => match hidden_option(self.env.options, name) {
                Some(SetKind::Text) => Ok(Value::String(String::new())),
                Some(_) => Ok(Value::Number(0)),
                None => self.fail(VimError::UnknownOption(name.into())),
            },
        }
    }

    fn option_value(&self, layer: OptionLayer, name: &str) -> Option<OptionValue> {
        let options = if layer == OptionLayer::Global {
            self.env.global_options
        } else {
            self.env.options
        };
        option_value(options, name).map(|(_, value)| value)
    }

    /// `$NAME`. Deviation: always empty; Vim reads the environment of the
    /// editor's process, which a project's file should not probe.
    fn environment(&mut self, ev: bool) -> Result<Value, Fail> {
        self.pos += 1;
        let start = self.pos;
        while self
            .peek()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            self.pos += 1;
        }
        if ev && self.pos == start {
            return Err(Fail);
        }
        Ok(if ev {
            Value::String(String::new())
        } else {
            Value::Number(0)
        })
    }

    /// A variable, or a function name followed by `(`.
    fn name(
        &mut self,
        stack: &mut Vec<Frame>,
        leaders: Range<usize>,
        ev: bool,
    ) -> Result<Option<Value>, Fail> {
        let start = self.pos;
        let (end, curly) = name_end(self.text, start);
        if curly {
            return self.unsupported();
        }
        if end == start {
            // `get_name_len()`: an error only where there is text.
            if self.pos < self.text.len() {
                let rest = self.rest();
                return self.fail(VimError::InvalidExpression(rest.into()));
            }
            return Err(Fail);
        }
        let after = skip_blanks(self.text, end);
        if self.byte(after) == Some(b'(') {
            self.pos = after + 1;
            self.depth += 1;
            stack.push(Frame::Call {
                name: start..end,
                leaders,
                args: Vec::new(),
                ev,
            });
            return self.argument_start(stack);
        }
        self.pos = end;
        let value = if ev {
            let name = self.slice(start..end);
            match self.variable(name) {
                Lookup::Found(value) => value,
                Lookup::Missing => {
                    return self.fail(VimError::UndefinedVariable(name.into()));
                }
                Lookup::Unsupported => return self.unsupported(),
            }
        } else {
            Value::Number(0)
        };
        self.apply_leaders(leaders, value, ev).map(Some)
    }

    /// A variable by its name as written, as `eval_variable()` finds it at
    /// the script level: a plain name is global, `version` is
    /// `v:version`.
    fn variable(&self, name: &str) -> Lookup {
        let bytes = name.as_bytes();
        let (scope, rest) = match (bytes.first(), bytes.get(1)) {
            (Some(&scope), Some(b':')) => (Some(scope), name.get(2..).unwrap_or_default()),
            _ => (None, name),
        };
        let store = |scope: VarScope, rest: &str| match self.env.variables.get(scope, rest) {
            Some(value) => Value::from_variable(value).map_or(Lookup::Unsupported, Lookup::Found),
            None => Lookup::Missing,
        };
        match scope {
            // The dictionary of a scope.
            Some(_) if rest.is_empty() => Lookup::Unsupported,
            Some(b'g') => store(VarScope::Global, rest),
            Some(b'b') if !rest.contains([':', '#']) => store(VarScope::Buffer, rest),
            Some(b'v') => match rest {
                "version" => Lookup::Found(Value::Number(VERSION)),
                "versionlong" => Lookup::Found(Value::Number(VERSION_LONG)),
                "true" => Lookup::Found(Value::Bool(true)),
                "false" => Lookup::Found(Value::Bool(false)),
                _ => Lookup::Missing,
            },
            Some(_) => Lookup::Missing,
            None if name == "version" => Lookup::Found(Value::Number(VERSION)),
            None => store(VarScope::Global, name),
        }
    }

    /// Whether a value is true, for `&&`, `||` and `?:`.
    fn truth(&mut self, value: &Value) -> Result<bool, Fail> {
        match value.number() {
            Ok(n) => Ok(n != 0),
            Err(error) => self.fail(error),
        }
    }

    /// Apply the prefix `!`, `-` and `+` at `leaders`, the last first, as
    /// `eval9_leader()` does. On a Float `!` gives 0.0 or 1.0.
    fn apply_leaders(
        &mut self,
        leaders: Range<usize>,
        value: Value,
        ev: bool,
    ) -> Result<Value, Fail> {
        let leaders = self.slice(leaders).as_bytes();
        if !ev || leaders.is_empty() {
            return Ok(value);
        }
        if let Value::Float(mut f) = value {
            for &b in leaders.iter().rev() {
                match b {
                    b'!' => f = if f == 0.0 { 1.0 } else { 0.0 },
                    b'-' => f = -f,
                    _ => {}
                }
            }
            return Ok(Value::Float(f));
        }
        let mut n = match value.number() {
            Ok(n) => n,
            Err(error) => return self.fail(error),
        };
        for &b in leaders.iter().rev() {
            match b {
                b'!' => n = i64::from(n == 0),
                b'-' => n = n.wrapping_neg(),
                _ => {}
            }
        }
        Ok(Value::Number(n))
    }

    /// A binary operator on two values, as `eval4()` to `eval7()` compute
    /// it.
    fn binary(&mut self, op: BinOp, lhs: Value, rhs: &Value) -> Result<Value, Fail> {
        let lhs = match op {
            BinOp::Compare(compare, ic) => {
                return self
                    .compare(compare, ic, &lhs, rhs)
                    .map(|b| Value::Number(i64::from(b)));
            }
            BinOp::Concat => {
                let mut text = lhs.into_text();
                text.push_str(&rhs.text());
                return Ok(Value::String(text));
            }
            _ => &lhs,
        };
        let result = match op {
            BinOp::Add | BinOp::Sub if lhs.is_float() || rhs.is_float() => float_operands(lhs, rhs)
                .map(|(a, b)| Value::Float(if op == BinOp::Add { a + b } else { a - b })),
            BinOp::Mul | BinOp::Div | BinOp::Mod if lhs.is_float() || rhs.is_float() => match op {
                BinOp::Mod => Err(VimError::PercentWithFloat),
                BinOp::Mul => float_operands(lhs, rhs).map(|(a, b)| Value::Float(a * b)),
                _ => float_operands(lhs, rhs).map(|(a, b)| Value::Float(a / b)),
            },
            _ => lhs.number().and_then(|a| {
                rhs.number().map(|b| {
                    Value::Number(match op {
                        BinOp::Add => a.wrapping_add(b),
                        BinOp::Sub => a.wrapping_sub(b),
                        BinOp::Mul => a.wrapping_mul(b),
                        BinOp::Div => num_divide(a, b),
                        _ => num_modulus(a, b),
                    })
                })
            }),
        };
        result.or_else(|error| self.fail(error))
    }

    /// A comparison, as `typval_compare()` makes it: values of different
    /// types are never `is`, a Float compares as a Float and a Number as a
    /// Number, except for `=~` and `!~`, and anything else as a String.
    fn compare(
        &mut self,
        compare: Compare,
        ic: bool,
        lhs: &Value,
        rhs: &Value,
    ) -> Result<bool, Fail> {
        let is_number = |v: &Value| matches!(v, Value::Number(_));
        if matches!(compare, Compare::Is | Compare::IsNot)
            && std::mem::discriminant(lhs) != std::mem::discriminant(rhs)
        {
            return Ok(compare == Compare::IsNot);
        }
        let order = match compare {
            Compare::Match | Compare::NoMatch => {
                let found = self.pattern_match(&rhs.text(), &lhs.text(), ic);
                return Ok(found == (compare == Compare::Match));
            }
            _ if lhs.is_float() || rhs.is_float() => {
                let pair = lhs.float().and_then(|a| rhs.float().map(|b| (a, b)));
                match pair {
                    Ok((a, b)) => match a.partial_cmp(&b) {
                        Some(order) => order,
                        // NaN: only "not equal" holds.
                        None => return Ok(matches!(compare, Compare::NotEqual | Compare::IsNot)),
                    },
                    Err(error) => return self.fail(error),
                }
            }
            _ if is_number(lhs) || is_number(rhs) => {
                match lhs.number().and_then(|a| rhs.number().map(|b| a.cmp(&b))) {
                    Ok(order) => order,
                    Err(error) => return self.fail(error),
                }
            }
            _ => compare_text(&lhs.text(), &rhs.text(), ic),
        };
        Ok(match compare {
            Compare::Equal | Compare::Is => order == Ordering::Equal,
            Compare::NotEqual | Compare::IsNot => order != Ordering::Equal,
            Compare::Greater => order == Ordering::Greater,
            Compare::GreaterEqual => order != Ordering::Less,
            Compare::Smaller => order == Ordering::Less,
            Compare::SmallerEqual | Compare::Match | Compare::NoMatch => order != Ordering::Greater,
        })
    }

    /// Whether `pattern` matches in `text`, as `pattern_match()` tries it:
    /// 'magic' on, 'cpoptions' ignored. A pattern that does not compile
    /// gives its error and no match.
    fn pattern_match(&mut self, pattern: &str, text: &str, ic: bool) -> bool {
        match crate::regex::VimRegex::new(pattern) {
            Ok(regex) => {
                let ctx = crate::regex::MatchContext::builder(text)
                    .case_sensitive(!ic)
                    .build();
                regex.is_match(&ctx).unwrap_or(false)
            }
            Err(error) => {
                self.errors
                    .push(VimError::InvalidPattern(error.to_string().into()));
                false
            }
        }
    }
}

/// What a variable name finds.
enum Lookup {
    Found(Value),
    Missing,
    /// A value the evaluator does not have, such as a List.
    Unsupported,
}

/// Past the spaces and tabs at `at`.
fn skip_blanks(text: &str, mut at: usize) -> usize {
    while matches!(text.as_bytes().get(at), Some(b' ' | b'\t')) {
        at += 1;
    }
    at
}

/// The end of the variable or function name at `start`, as
/// `get_name_len()` and `find_name_end()` find it, and whether it has a
/// `{}` part. A `:` belongs to it only after one scope letter.
fn name_end(text: &str, start: usize) -> (usize, bool) {
    let bytes = text.as_bytes();
    let mut p = start;
    let script = text
        .get(start..start + 5)
        .is_some_and(|s| s.eq_ignore_ascii_case("<SID>") || s.eq_ignore_ascii_case("<SNR>"));
    if script {
        p += 5;
    } else if !bytes
        .get(start)
        .is_some_and(|&b| b.is_ascii_alphabetic() || b == b'_' || b == b'{')
    {
        return (start, false);
    }
    while let Some(&b) = bytes.get(p) {
        match b {
            b'{' => return (p, true),
            b':' if p == start + 1 && bytes.get(start).is_some_and(|s| NAMESPACES.contains(s)) => {}
            b if b.is_ascii_alphanumeric() || b == b'_' || b == b'#' => {}
            _ => break,
        }
        p += 1;
    }
    (p, false)
}

/// The option name after the `&` or `+` at `at`, as `find_option_end()`
/// finds it: the value it names, the name, and the end.
fn option_name(text: &str, at: usize) -> Option<(OptionLayer, Range<usize>, usize)> {
    let bytes = text.as_bytes();
    let mut p = at + 1;
    let layer = match bytes.get(p..p + 2) {
        Some(b"g:") => OptionLayer::Global,
        Some(b"l:") => OptionLayer::Local,
        _ => OptionLayer::Both,
    };
    if layer != OptionLayer::Both {
        p += 2;
    }
    if !bytes.get(p).is_some_and(u8::is_ascii_alphabetic) {
        return None;
    }
    let start = p;
    if text.get(p..).is_some_and(|s| s.starts_with("t_")) {
        // A terminal option: `t_` and two characters.
        let mut chars = text.get(p + 2..)?.char_indices();
        let (_, _) = chars.next()?;
        let (last, c) = chars.next()?;
        p += 2 + last + c.len_utf8();
    } else {
        while bytes.get(p).is_some_and(u8::is_ascii_alphabetic) {
            p += 1;
        }
    }
    Some((layer, start..p, p))
}

/// Whether `name` is a function that can be called.
fn is_function(name: &str) -> bool {
    matches!(name, "has" | "exists")
}

/// Whether the evaluator knows `name` as a feature of `has()`, and has it.
/// Other names are no feature, as Vim answers a name it does not know.
fn feature(name: &str) -> (bool, bool) {
    if name.eq_ignore_ascii_case("eval")
        || name.eq_ignore_ascii_case("float")
        || name.eq_ignore_ascii_case("num64")
        || name.eq_ignore_ascii_case("vimscript-1")
    {
        return (true, true);
    }
    if name
        .get(..5)
        .is_some_and(|p| p.eq_ignore_ascii_case("patch"))
    {
        return (true, has_patch_name(name));
    }
    let later_script = ["vim9script", "vimscript-2", "vimscript-3", "vimscript-4"];
    (
        later_script.iter().any(|s| name.eq_ignore_ascii_case(s)),
        false,
    )
}

/// `has('patch-9.1.0')` and `has('patch123')`, as `f_has()` reads them
/// for Vim 9.1.0, which has only patch 0.
fn has_patch_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.get(5) == Some(&b'-') && bytes.len() >= 11 && matches!(bytes.get(6), Some(b'1'..=b'9'))
    {
        // "patch-9.9.01234"; not "patch-9.10.5".
        let (major, end) = leading_number(bytes, 6);
        let digit = |i: usize| bytes.get(i).is_some_and(u8::is_ascii_digit);
        if bytes.get(end) != Some(&b'.')
            || !digit(end + 1)
            || bytes.get(end + 2) != Some(&b'.')
            || !digit(end + 3)
        {
            return false;
        }
        let minor = bytes.get(end + 1).map_or(0, |b| u64::from(b - b'0'));
        let (patch, _) = leading_number(bytes, end + 3);
        major < 9 || (major == 9 && (minor < 1 || (minor == 1 && patch == 0)))
    } else if bytes.get(5).is_some_and(u8::is_ascii_digit) {
        leading_number(bytes, 5).0 == 0
    } else {
        false
    }
}

/// The decimal number at `at`, saturated, and its end.
fn leading_number(bytes: &[u8], mut at: usize) -> (u64, usize) {
    let mut n: u64 = 0;
    while let Some(&b) = bytes.get(at).filter(|b| b.is_ascii_digit()) {
        n = n.saturating_mul(10).saturating_add(u64::from(b - b'0'));
        at += 1;
    }
    (n, at)
}

/// The end of the Float constant at `start`, as `eval_number()` takes one:
/// digits, `.`, digits and an exponent, with no letter or `.` after it.
fn float_end(bytes: &[u8], start: usize) -> Option<usize> {
    let digit = |i: usize| bytes.get(i).is_some_and(u8::is_ascii_digit);
    let skip_digits = |mut i: usize| {
        while digit(i) {
            i += 1;
        }
        i
    };
    let p = skip_digits(start + 1);
    if bytes.get(p) != Some(&b'.') || !digit(p + 1) {
        return None;
    }
    let mut p = skip_digits(p + 2);
    if matches!(bytes.get(p), Some(b'e' | b'E')) {
        p += 1;
        if matches!(bytes.get(p), Some(b'+' | b'-')) {
            p += 1;
        }
        if !digit(p) {
            return None;
        }
        p = skip_digits(p + 1);
    }
    if bytes
        .get(p)
        .is_some_and(|&b| b.is_ascii_alphabetic() || b == b'.')
    {
        return None;
    }
    Some(p)
}

/// The number at the start of `bytes` and its length, as `vim_str2nr()`
/// reads it with `STR2NR_ALL`: `-`, then `0x`, `0X`, `0b`, `0B`, `0o`,
/// `0O`, a leading `0` with only octal digits after it, or decimal; a
/// number too big saturates. `strict` refuses a letter or digit after it.
fn str2nr(bytes: &[u8], strict: bool) -> Option<(i64, usize)> {
    let at = |i: usize| bytes.get(i).copied();
    let negative = at(0) == Some(b'-');
    let mut p = usize::from(negative);
    let mut base = 10u64;
    if at(p) == Some(b'0') && !matches!(at(p + 1), Some(b'8' | b'9')) {
        let next = at(p + 2);
        match at(p + 1) {
            Some(b'x' | b'X') if next.is_some_and(|b| b.is_ascii_hexdigit()) => {
                base = 16;
                p += 2;
            }
            Some(b'b' | b'B') if matches!(next, Some(b'0' | b'1')) => {
                base = 2;
                p += 2;
            }
            Some(b'o' | b'O') if matches!(next, Some(b'0'..=b'7')) => {
                base = 8;
                p += 2;
            }
            // "0", "08" and "0129" are decimal.
            _ => {
                let digits = bytes.get(p + 1..).unwrap_or_default();
                let run = digits.iter().take_while(|b| b.is_ascii_digit()).count();
                if run > 0 && digits.iter().take(run).all(|&b| b <= b'7') {
                    base = 8;
                }
            }
        }
    }
    let mut n: u64 = 0;
    while let Some(digit) = at(p).and_then(|b| char::from(b).to_digit(32)) {
        let digit = u64::from(digit);
        if digit >= base {
            break;
        }
        n = n
            .checked_mul(base)
            .and_then(|n| n.checked_add(digit))
            .unwrap_or(u64::MAX);
        p += 1;
    }
    if strict && at(p).is_some_and(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    let value = if negative {
        0i64.checked_sub_unsigned(n).unwrap_or(i64::MIN)
    } else {
        i64::try_from(n).unwrap_or(i64::MAX)
    };
    Some((value, p))
}

/// The Number of a String: the number it starts with, or 0.
fn string_number(s: &str) -> i64 {
    str2nr(s.as_bytes(), false).map_or(0, |(n, _)| n)
}

/// Vim's `num_divide()`: no error for zero, and the minimum divided by -1
/// is the maximum.
const fn num_divide(n1: i64, n2: i64) -> i64 {
    if n2 == 0 {
        if n1 == 0 {
            i64::MIN
        } else if n1 < 0 {
            -i64::MAX
        } else {
            i64::MAX
        }
    } else if n1 == i64::MIN && n2 == -1 {
        i64::MAX
    } else {
        n1.wrapping_div(n2)
    }
}

/// Vim's `num_modulus()`: 0 for zero. The minimum modulo -1 is 0, where
/// Vim's C would trap.
const fn num_modulus(n1: i64, n2: i64) -> i64 {
    if n2 == 0 {
        0
    } else {
        n1.wrapping_rem(n2)
    }
}

/// Two operands of `+ - * /` with a Float among them: the other as a
/// Number made a Float.
fn float_operands(lhs: &Value, rhs: &Value) -> Result<(f64, f64), VimError> {
    let side = |v: &Value| match v {
        Value::Float(f) => Ok(*f),
        other => other.number().map(number_float),
    };
    Ok((side(lhs)?, side(rhs)?))
}

/// Compare two Strings as `STRCMP()`, or with `ic` as `MB_STRICMP()`, which
/// folds the case of each character.
fn compare_text(a: &str, b: &str, ic: bool) -> Ordering {
    if !ic {
        return a.cmp(b);
    }
    let fold = |c: char| c.to_lowercase().next().unwrap_or(c);
    a.chars().map(fold).cmp(b.chars().map(fold))
}

/// The text of a Float, as Vim's `%g` gives it in a String: six decimals
/// or an exponent, with the zeros at the end dropped but one after the
/// `.`, so 1.0 is "1.0" (`vim_vsnprintf_typval()`).
fn float_text(f: f64) -> String {
    if f.is_nan() {
        return "nan".to_owned();
    }
    if f.is_infinite() {
        return if f > 0.0 { "inf" } else { "-inf" }.to_owned();
    }
    let abs = f.abs();
    if (0.001..10_000_000.0).contains(&abs) || abs == 0.0 {
        let text = format!("{f:.6}");
        return trim_zeros(&text).to_owned();
    }
    let text = format!("{f:.6e}");
    match text.split_once('e') {
        Some((mantissa, exponent)) => format!("{}e{exponent}", trim_zeros(mantissa)),
        None => text,
    }
}

/// Drop the zeros at the end of a number with a `.`, keeping one after it.
fn trim_zeros(text: &str) -> &str {
    let trimmed = text.trim_end_matches('0');
    if trimmed.ends_with('.') {
        text.get(..=trimmed.len()).unwrap_or(text)
    } else {
        trimmed
    }
}

/// The text of a `"string"`, its escapes replaced as `eval_string()` does.
///
/// `\<Key>` gives the character of a key that is one (`\<Space>`,
/// `\<C-W>`); any other key keeps its `<...>` text, where Vim stores the
/// key's internal code. Bytes that are no UTF-8 become U+FFFD, and a NUL
/// ends the string, as it ends Vim's.
fn unescape(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        i += 1;
        if b != b'\\' {
            out.push(b);
            continue;
        }
        let Some(&c) = bytes.get(i) else {
            break;
        };
        i += 1;
        match c {
            b'b' => out.push(0x08),
            b'e' => out.push(0x1b),
            b'f' => out.push(0x0c),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'x' | b'X' | b'u' | b'U' if bytes.get(i).is_some_and(u8::is_ascii_hexdigit) => {
                let max = match c {
                    b'x' | b'X' => 2,
                    b'u' => 4,
                    _ => 8,
                };
                let mut nr: u32 = 0;
                let mut n = 0;
                while let Some(d) = bytes
                    .get(i)
                    .and_then(|&d| char::from(d).to_digit(16))
                    .filter(|_| n < max)
                {
                    nr = nr.wrapping_shl(4) | d;
                    i += 1;
                    n += 1;
                }
                if matches!(c, b'x' | b'X') {
                    out.push(nr.to_le_bytes().first().copied().unwrap_or(0));
                } else {
                    push_char(&mut out, char::from_u32(nr).unwrap_or('\u{fffd}'));
                }
            }
            b'0'..=b'7' => {
                // Up to three octal digits, kept to a byte.
                let mut nr = c - b'0';
                for _ in 0..2 {
                    match bytes.get(i) {
                        Some(&d @ b'0'..=b'7') => {
                            nr = nr.wrapping_mul(8).wrapping_add(d - b'0');
                            i += 1;
                        }
                        _ => break,
                    }
                }
                out.push(nr);
            }
            b'<' => {
                let key = text
                    .get(i..)
                    .and_then(|rest| rest.find('>').map(|end| (rest, end)))
                    .and_then(|(rest, end)| Some((key_char(rest.get(..end)?)?, end)));
                match key {
                    Some((ch, end)) => {
                        push_char(&mut out, ch);
                        i += end + 1;
                    }
                    None => out.push(b'<'),
                }
            }
            // Any other character stands for itself.
            _ => out.push(c),
        }
    }
    if let Some(nul) = out.iter().position(|&b| b == 0) {
        out.truncate(nul);
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn push_char(out: &mut Vec<u8>, c: char) {
    let mut buf = [0; 4];
    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
}

/// The character of the key named `name` (inside `<>`), when it is one.
fn key_char(name: &str) -> Option<char> {
    if ["NL", "NewLine", "LineFeed", "LF"]
        .iter()
        .any(|n| name.eq_ignore_ascii_case(n))
    {
        return Some('\n');
    }
    let key = KeyEvent::from_vim_notation(&format!("<{name}>"))?;
    let modifiers = key.modifiers();
    match key.key() {
        Key::Char(c) if modifiers.is_empty() => Some(c),
        Key::Enter if modifiers.is_empty() => Some('\r'),
        Key::Escape if modifiers.is_empty() => Some('\x1b'),
        Key::Tab if modifiers.is_empty() => Some('\t'),
        Key::Char(c) if modifiers == Modifiers::SHIFT && c.is_ascii_alphabetic() => {
            Some(c.to_ascii_uppercase())
        }
        // Ctrl-@ is NUL, which a String cannot hold.
        Key::Char(c) if modifiers == Modifiers::CTRL && c != '@' => {
            let upper = c.to_ascii_uppercase();
            match upper {
                'A'..='Z' | '[' | '\\' | ']' | '^' | '_' => char::from_u32(u32::from(upper) & 0x1f),
                '?' => Some('\x7f'),
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "script_expr_tests.rs"]
mod tests;
