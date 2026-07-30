//! Integration tests for expression evaluation.
//!
//! Tests the `ExpressionEval` struct, `SimpleExprEval` runtime, and
//! `VimApi::expr_context()` integration.

use compact_str::CompactString;
use vim_core::execution::{
    ExprContext, ExprEngine, ExpressionEval, HostSession, RuntimeError, SimpleExprEval,
};
use vim_core::primitives::{VarScope, VimOptions, VimValue};
use vim_core::state::VariableStore;

// ═══════════════════════════════════════════════════════════════════════════════
// SimpleExprEval — arithmetic
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn simple_expr_integer_arithmetic() {
    let runtime = SimpleExprEval;
    let vars = VariableStore::default();
    let opts = VimOptions::default();
    let ctx = ExprContext {
        variables: &vars,
        options: &opts,
    };

    assert_eq!(
        runtime.eval_expression("2 + 3", &ctx).unwrap(),
        VimValue::Int(5)
    );
    assert_eq!(
        runtime.eval_expression("10 - 4", &ctx).unwrap(),
        VimValue::Int(6)
    );
    assert_eq!(
        runtime.eval_expression("3 * 7", &ctx).unwrap(),
        VimValue::Int(21)
    );
    assert_eq!(
        runtime.eval_expression("10 / 3", &ctx).unwrap(),
        VimValue::Int(3)
    );
}

#[test]
fn simple_expr_float() {
    let runtime = SimpleExprEval;
    let vars = VariableStore::default();
    let opts = VimOptions::default();
    let ctx = ExprContext {
        variables: &vars,
        options: &opts,
    };

    assert_eq!(
        runtime.eval_expression("3.14", &ctx).unwrap(),
        VimValue::Float(3.14)
    );
    assert_eq!(
        runtime.eval_expression("1.5 + 2.5", &ctx).unwrap(),
        VimValue::Float(4.0)
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// SimpleExprEval — strings
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn simple_expr_string_literals() {
    let runtime = SimpleExprEval;
    let vars = VariableStore::default();
    let opts = VimOptions::default();
    let ctx = ExprContext {
        variables: &vars,
        options: &opts,
    };

    assert_eq!(
        runtime.eval_expression("\"hello\"", &ctx).unwrap(),
        VimValue::String(CompactString::from("hello"))
    );
    assert_eq!(
        runtime.eval_expression("'world'", &ctx).unwrap(),
        VimValue::String(CompactString::from("world"))
    );
}

#[test]
fn simple_expr_string_concatenation() {
    let runtime = SimpleExprEval;
    let vars = VariableStore::default();
    let opts = VimOptions::default();
    let ctx = ExprContext {
        variables: &vars,
        options: &opts,
    };

    assert_eq!(
        runtime
            .eval_expression("\"hello\" . \" world\"", &ctx)
            .unwrap(),
        VimValue::String(CompactString::from("hello world"))
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// SimpleExprEval — variables
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn simple_expr_variable_access() {
    let runtime = SimpleExprEval;
    let mut vars = VariableStore::default();
    vars.set(VarScope::Global, "myvar", VimValue::Int(42));
    vars.set(
        VarScope::Buffer,
        "name",
        VimValue::String(CompactString::from("test")),
    );
    let opts = VimOptions::default();
    let ctx = ExprContext {
        variables: &vars,
        options: &opts,
    };

    assert_eq!(
        runtime.eval_expression("g:myvar", &ctx).unwrap(),
        VimValue::Int(42)
    );
    assert_eq!(
        runtime.eval_expression("b:name", &ctx).unwrap(),
        VimValue::String(CompactString::from("test"))
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// SimpleExprEval — comparisons
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn simple_expr_comparisons() {
    let runtime = SimpleExprEval;
    let vars = VariableStore::default();
    let opts = VimOptions::default();
    let ctx = ExprContext {
        variables: &vars,
        options: &opts,
    };

    assert_eq!(
        runtime.eval_expression("1 == 1", &ctx).unwrap(),
        VimValue::Bool(true)
    );
    assert_eq!(
        runtime.eval_expression("1 == 2", &ctx).unwrap(),
        VimValue::Bool(false)
    );
    assert_eq!(
        runtime.eval_expression("2 > 3", &ctx).unwrap(),
        VimValue::Bool(false)
    );
    assert_eq!(
        runtime.eval_expression("3 > 2", &ctx).unwrap(),
        VimValue::Bool(true)
    );
    assert_eq!(
        runtime.eval_expression("5 != 5", &ctx).unwrap(),
        VimValue::Bool(false)
    );
    assert_eq!(
        runtime.eval_expression("2 <= 2", &ctx).unwrap(),
        VimValue::Bool(true)
    );
    assert_eq!(
        runtime.eval_expression("3 >= 4", &ctx).unwrap(),
        VimValue::Bool(false)
    );
    assert_eq!(
        runtime.eval_expression("1 < 2", &ctx).unwrap(),
        VimValue::Bool(true)
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// SimpleExprEval — parentheses and unary
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn simple_expr_parentheses_and_precedence() {
    let runtime = SimpleExprEval;
    let vars = VariableStore::default();
    let opts = VimOptions::default();
    let ctx = ExprContext {
        variables: &vars,
        options: &opts,
    };

    assert_eq!(
        runtime.eval_expression("(2 + 3) * 4", &ctx).unwrap(),
        VimValue::Int(20)
    );
    // Without parens: 2 + (3 * 4) = 14
    assert_eq!(
        runtime.eval_expression("2 + 3 * 4", &ctx).unwrap(),
        VimValue::Int(14)
    );
}

#[test]
fn simple_expr_unary_minus() {
    let runtime = SimpleExprEval;
    let vars = VariableStore::default();
    let opts = VimOptions::default();
    let ctx = ExprContext {
        variables: &vars,
        options: &opts,
    };

    assert_eq!(
        runtime.eval_expression("-5", &ctx).unwrap(),
        VimValue::Int(-5)
    );
    assert_eq!(
        runtime.eval_expression("-(3 + 2)", &ctx).unwrap(),
        VimValue::Int(-5)
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Error cases
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn simple_expr_error_undefined_variable() {
    let runtime = SimpleExprEval;
    let vars = VariableStore::default();
    let opts = VimOptions::default();
    let ctx = ExprContext {
        variables: &vars,
        options: &opts,
    };

    let err = runtime.eval_expression("g:nope", &ctx).unwrap_err();
    assert!(matches!(err, RuntimeError::RuntimeError(_)));
}

#[test]
fn simple_expr_error_division_by_zero() {
    let runtime = SimpleExprEval;
    let vars = VariableStore::default();
    let opts = VimOptions::default();
    let ctx = ExprContext {
        variables: &vars,
        options: &opts,
    };

    let err = runtime.eval_expression("1 / 0", &ctx).unwrap_err();
    assert!(matches!(err, RuntimeError::RuntimeError(_)));
}

#[test]
fn simple_expr_error_syntax() {
    let runtime = SimpleExprEval;
    let vars = VariableStore::default();
    let opts = VimOptions::default();
    let ctx = ExprContext {
        variables: &vars,
        options: &opts,
    };

    let err = runtime.eval_expression("+ +", &ctx).unwrap_err();
    assert!(matches!(err, RuntimeError::CompileError(_)));

    let err = runtime.eval_expression("", &ctx).unwrap_err();
    assert!(matches!(err, RuntimeError::CompileError(_)));

    let err = runtime.eval_expression("\"unterminated", &ctx).unwrap_err();
    assert!(matches!(err, RuntimeError::CompileError(_)));
}

// ═══════════════════════════════════════════════════════════════════════════════
// ExpressionEval — integration with VimApi
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn expression_eval_from_api() {
    use vim_core::execution::{InvocationContext, VimApi};
    use vim_core::primitives::{CallerId, CapabilityTier};

    let session = HostSession::new("hello\nworld");
    let invocation = InvocationContext::new(CallerId::Internal, CapabilityTier::ReadOnly);
    let api = VimApi::from_session(&session, invocation);
    let runtime = SimpleExprEval;

    let expr_eval = ExpressionEval::from_api(&api, &runtime);
    assert_eq!(expr_eval.evaluate("2 + 2").unwrap(), VimValue::Int(4));
    assert_eq!(expr_eval.evaluate_to_string("3 * 5").unwrap(), "15");
}

#[test]
fn expression_eval_evaluate_to_string() {
    let runtime = SimpleExprEval;
    let vars = VariableStore::default();
    let opts = VimOptions::default();
    let ctx = ExprContext {
        variables: &vars,
        options: &opts,
    };

    let expr_eval = ExpressionEval::new(&runtime, ctx);
    assert_eq!(expr_eval.evaluate_to_string("42").unwrap(), "42");
    assert_eq!(expr_eval.evaluate_to_string("\"hello\"").unwrap(), "hello");
    assert_eq!(expr_eval.evaluate_to_string("true").unwrap(), "1");
    assert_eq!(expr_eval.evaluate_to_string("false").unwrap(), "0");
}

// ═══════════════════════════════════════════════════════════════════════════════
// ExprEngine trait — object safety
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn plugin_runtime_is_object_safe() {
    // Verify the trait can be used as a trait object.
    let runtime: &dyn ExprEngine = &SimpleExprEval;
    let vars = VariableStore::default();
    let opts = VimOptions::default();
    let ctx = ExprContext {
        variables: &vars,
        options: &opts,
    };
    assert_eq!(
        runtime.eval_expression("1 + 1", &ctx).unwrap(),
        VimValue::Int(2)
    );
}
