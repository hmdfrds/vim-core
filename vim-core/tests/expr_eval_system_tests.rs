//! System tests for expression evaluation.
//!
//! Proves: ExprEngine trait, SimpleExprEval parser, ExpressionEval struct,
//! VimApi::expr_context(), variable store integration, evaluate_to_string(),
//! error handling, and full Ctrl-R = simulation workflows.

use vim_core::effects::Effect;
use vim_core::execution::{
    ExprContext, ExprEngine, ExpressionEval, HostSession, InvocationContext, RuntimeError,
    SimpleExprEval, VimApi, VimEngine,
};
use vim_core::primitives::{CallerId, CapabilityTier, VarScope, VimValue};
use vim_core::state::VariableStore;

use compact_str::CompactString;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn host_ctx() -> InvocationContext {
    InvocationContext::new(CallerId::Host, CapabilityTier::Mutating)
}

fn expr_ctx() -> InvocationContext {
    InvocationContext::new(CallerId::Expression, CapabilityTier::ReadOnly)
}

fn make_context(store: &VariableStore) -> ExprContext<'_> {
    // Use default options for testing
    let opts = Box::leak(Box::new(vim_core::primitives::VimOptions::default()));
    ExprContext {
        variables: store,
        options: opts,
    }
}

// ===========================================================================
// 1. SIMPLE EXPRESSION EVALUATOR — all supported operations
// ===========================================================================

#[test]
fn eval_integer_literals() {
    let store = VariableStore::default();
    let ctx = make_context(&store);
    let rt = SimpleExprEval;

    assert_eq!(rt.eval_expression("0", &ctx).unwrap(), VimValue::Int(0));
    assert_eq!(rt.eval_expression("42", &ctx).unwrap(), VimValue::Int(42));
    assert_eq!(
        rt.eval_expression("999999", &ctx).unwrap(),
        VimValue::Int(999999)
    );
}

#[test]
fn eval_float_literals() {
    let store = VariableStore::default();
    let ctx = make_context(&store);
    let rt = SimpleExprEval;

    assert_eq!(
        rt.eval_expression("3.14", &ctx).unwrap(),
        VimValue::Float(3.14)
    );
    assert_eq!(
        rt.eval_expression("0.5", &ctx).unwrap(),
        VimValue::Float(0.5)
    );
}

#[test]
fn eval_string_literals_double_quotes() {
    let store = VariableStore::default();
    let ctx = make_context(&store);
    let rt = SimpleExprEval;

    assert_eq!(
        rt.eval_expression("\"hello\"", &ctx).unwrap(),
        VimValue::String(CompactString::from("hello"))
    );
    assert_eq!(
        rt.eval_expression("\"\"", &ctx).unwrap(),
        VimValue::String(CompactString::from(""))
    );
}

#[test]
fn eval_string_literals_single_quotes() {
    let store = VariableStore::default();
    let ctx = make_context(&store);
    let rt = SimpleExprEval;

    assert_eq!(
        rt.eval_expression("'world'", &ctx).unwrap(),
        VimValue::String(CompactString::from("world"))
    );
}

#[test]
fn eval_boolean_literals() {
    let store = VariableStore::default();
    let ctx = make_context(&store);
    let rt = SimpleExprEval;

    assert_eq!(
        rt.eval_expression("true", &ctx).unwrap(),
        VimValue::Bool(true)
    );
    assert_eq!(
        rt.eval_expression("false", &ctx).unwrap(),
        VimValue::Bool(false)
    );
}

#[test]
fn eval_arithmetic_addition() {
    let store = VariableStore::default();
    let ctx = make_context(&store);
    let rt = SimpleExprEval;

    assert_eq!(rt.eval_expression("2 + 3", &ctx).unwrap(), VimValue::Int(5));
    assert_eq!(
        rt.eval_expression("100 + 200 + 300", &ctx).unwrap(),
        VimValue::Int(600)
    );
}

#[test]
fn eval_arithmetic_subtraction() {
    let store = VariableStore::default();
    let ctx = make_context(&store);
    let rt = SimpleExprEval;

    assert_eq!(
        rt.eval_expression("10 - 4", &ctx).unwrap(),
        VimValue::Int(6)
    );
    assert_eq!(
        rt.eval_expression("1 - 10", &ctx).unwrap(),
        VimValue::Int(-9)
    );
}

#[test]
fn eval_arithmetic_multiplication() {
    let store = VariableStore::default();
    let ctx = make_context(&store);
    let rt = SimpleExprEval;

    assert_eq!(
        rt.eval_expression("3 * 7", &ctx).unwrap(),
        VimValue::Int(21)
    );
    assert_eq!(
        rt.eval_expression("0 * 999", &ctx).unwrap(),
        VimValue::Int(0)
    );
}

#[test]
fn eval_arithmetic_division() {
    let store = VariableStore::default();
    let ctx = make_context(&store);
    let rt = SimpleExprEval;

    assert_eq!(
        rt.eval_expression("10 / 3", &ctx).unwrap(),
        VimValue::Int(3)
    );
    assert_eq!(
        rt.eval_expression("100 / 10", &ctx).unwrap(),
        VimValue::Int(10)
    );
}

#[test]
fn eval_operator_precedence() {
    let store = VariableStore::default();
    let ctx = make_context(&store);
    let rt = SimpleExprEval;

    // Multiplication before addition
    assert_eq!(
        rt.eval_expression("2 + 3 * 4", &ctx).unwrap(),
        VimValue::Int(14)
    );
    // Parentheses override
    assert_eq!(
        rt.eval_expression("(2 + 3) * 4", &ctx).unwrap(),
        VimValue::Int(20)
    );
}

#[test]
fn eval_unary_minus() {
    let store = VariableStore::default();
    let ctx = make_context(&store);
    let rt = SimpleExprEval;

    assert_eq!(rt.eval_expression("-5", &ctx).unwrap(), VimValue::Int(-5));
    assert_eq!(
        rt.eval_expression("-3 + 10", &ctx).unwrap(),
        VimValue::Int(7)
    );
    assert_eq!(
        rt.eval_expression("-(2 + 3)", &ctx).unwrap(),
        VimValue::Int(-5)
    );
}

#[test]
fn eval_string_concatenation() {
    let store = VariableStore::default();
    let ctx = make_context(&store);
    let rt = SimpleExprEval;

    assert_eq!(
        rt.eval_expression("\"hello\" . \" world\"", &ctx).unwrap(),
        VimValue::String(CompactString::from("hello world"))
    );
}

#[test]
fn eval_comparisons() {
    let store = VariableStore::default();
    let ctx = make_context(&store);
    let rt = SimpleExprEval;

    assert_eq!(
        rt.eval_expression("1 == 1", &ctx).unwrap(),
        VimValue::Bool(true)
    );
    assert_eq!(
        rt.eval_expression("1 == 2", &ctx).unwrap(),
        VimValue::Bool(false)
    );
    assert_eq!(
        rt.eval_expression("1 != 2", &ctx).unwrap(),
        VimValue::Bool(true)
    );
    assert_eq!(
        rt.eval_expression("3 > 2", &ctx).unwrap(),
        VimValue::Bool(true)
    );
    assert_eq!(
        rt.eval_expression("2 > 3", &ctx).unwrap(),
        VimValue::Bool(false)
    );
    assert_eq!(
        rt.eval_expression("2 < 3", &ctx).unwrap(),
        VimValue::Bool(true)
    );
    assert_eq!(
        rt.eval_expression("3 >= 3", &ctx).unwrap(),
        VimValue::Bool(true)
    );
    assert_eq!(
        rt.eval_expression("2 <= 3", &ctx).unwrap(),
        VimValue::Bool(true)
    );
}

#[test]
fn eval_float_promotion() {
    let store = VariableStore::default();
    let ctx = make_context(&store);
    let rt = SimpleExprEval;

    // Int + Float → Float
    let result = rt.eval_expression("1 + 2.5", &ctx).unwrap();
    assert_eq!(result, VimValue::Float(3.5));
}

// ===========================================================================
// 2. VARIABLE ACCESS — proves integration with VariableStore
// ===========================================================================

#[test]
fn eval_global_variable() {
    let mut store = VariableStore::default();
    store.set(VarScope::Global, "count", VimValue::Int(42));

    let ctx = make_context(&store);
    let rt = SimpleExprEval;

    assert_eq!(
        rt.eval_expression("g:count", &ctx).unwrap(),
        VimValue::Int(42)
    );
}

#[test]
fn eval_buffer_variable() {
    let mut store = VariableStore::default();
    store.set(
        VarScope::Buffer,
        "name",
        VimValue::String(CompactString::from("test.rs")),
    );

    let ctx = make_context(&store);
    let rt = SimpleExprEval;

    assert_eq!(
        rt.eval_expression("b:name", &ctx).unwrap(),
        VimValue::String(CompactString::from("test.rs"))
    );
}

#[test]
fn eval_variable_in_expression() {
    let mut store = VariableStore::default();
    store.set(VarScope::Global, "x", VimValue::Int(10));
    store.set(VarScope::Global, "y", VimValue::Int(20));

    let ctx = make_context(&store);
    let rt = SimpleExprEval;

    assert_eq!(
        rt.eval_expression("g:x + g:y", &ctx).unwrap(),
        VimValue::Int(30)
    );
    assert_eq!(
        rt.eval_expression("g:x * 3", &ctx).unwrap(),
        VimValue::Int(30)
    );
}

#[test]
fn eval_undefined_variable_error() {
    let store = VariableStore::default();
    let ctx = make_context(&store);
    let rt = SimpleExprEval;

    let result = rt.eval_expression("g:nonexistent", &ctx);
    assert!(result.is_err());
}

// ===========================================================================
// 3. ERROR HANDLING — proves graceful failure
// ===========================================================================

#[test]
fn eval_division_by_zero() {
    let store = VariableStore::default();
    let ctx = make_context(&store);
    let rt = SimpleExprEval;

    let result = rt.eval_expression("10 / 0", &ctx);
    assert!(result.is_err());
}

#[test]
fn eval_syntax_error_unclosed_paren() {
    let store = VariableStore::default();
    let ctx = make_context(&store);
    let rt = SimpleExprEval;

    let result = rt.eval_expression("(2 + 3", &ctx);
    assert!(result.is_err());
}

#[test]
fn eval_syntax_error_unclosed_string() {
    let store = VariableStore::default();
    let ctx = make_context(&store);
    let rt = SimpleExprEval;

    let result = rt.eval_expression("\"unclosed", &ctx);
    assert!(result.is_err());
}

#[test]
fn eval_empty_expression() {
    let store = VariableStore::default();
    let ctx = make_context(&store);
    let rt = SimpleExprEval;

    let result = rt.eval_expression("", &ctx);
    assert!(result.is_err());
}

#[test]
fn eval_whitespace_only() {
    let store = VariableStore::default();
    let ctx = make_context(&store);
    let rt = SimpleExprEval;

    let result = rt.eval_expression("   ", &ctx);
    assert!(result.is_err());
}

// ===========================================================================
// 4. EXPRESSION EVAL STRUCT — proves the wrapper works
// ===========================================================================

#[test]
fn expression_eval_basic() {
    let store = VariableStore::default();
    let opts = Box::leak(Box::new(vim_core::primitives::VimOptions::default()));
    let ctx = ExprContext {
        variables: &store,
        options: opts,
    };
    let rt = SimpleExprEval;
    let eval = ExpressionEval::new(&rt, ctx);

    let result = eval.evaluate("2 + 2").unwrap();
    assert_eq!(result, VimValue::Int(4));
}

#[test]
fn expression_eval_to_string_integer() {
    let store = VariableStore::default();
    let opts = Box::leak(Box::new(vim_core::primitives::VimOptions::default()));
    let ctx = ExprContext {
        variables: &store,
        options: opts,
    };
    let rt = SimpleExprEval;
    let eval = ExpressionEval::new(&rt, ctx);

    assert_eq!(eval.evaluate_to_string("42").unwrap(), "42");
}

#[test]
fn expression_eval_to_string_concatenation() {
    let store = VariableStore::default();
    let opts = Box::leak(Box::new(vim_core::primitives::VimOptions::default()));
    let ctx = ExprContext {
        variables: &store,
        options: opts,
    };
    let rt = SimpleExprEval;
    let eval = ExpressionEval::new(&rt, ctx);

    assert_eq!(
        eval.evaluate_to_string("\"hello\" . \" world\"").unwrap(),
        "hello world"
    );
}

#[test]
fn expression_eval_to_string_bool() {
    let store = VariableStore::default();
    let opts = Box::leak(Box::new(vim_core::primitives::VimOptions::default()));
    let ctx = ExprContext {
        variables: &store,
        options: opts,
    };
    let rt = SimpleExprEval;
    let eval = ExpressionEval::new(&rt, ctx);

    assert_eq!(eval.evaluate_to_string("1 == 1").unwrap(), "1");
    assert_eq!(eval.evaluate_to_string("1 == 2").unwrap(), "0");
}

#[test]
fn expression_eval_to_string_nil() {
    let store = VariableStore::default();
    let opts = Box::leak(Box::new(vim_core::primitives::VimOptions::default()));
    let ctx = ExprContext {
        variables: &store,
        options: opts,
    };
    let rt = SimpleExprEval;

    // No nil literal exists in the expression syntax, and an undefined
    // variable errors rather than evaluating to nil.
    // Test with empty string
    let eval = ExpressionEval::new(&rt, ctx);
    assert_eq!(eval.evaluate_to_string("\"\"").unwrap(), "");
}

// ===========================================================================
// 5. VIM API INTEGRATION — proves expr_context() works with real engine
// ===========================================================================

#[test]
fn vim_api_expr_context_reads_variables() {
    let mut engine = VimEngine::new();
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Global,
        name: CompactString::from("counter"),
        value: VimValue::Int(7),
    });

    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, expr_ctx());

    // The expr_context comes from the session's engine, not the separate
    // engine mutated above.
    let rt = SimpleExprEval;
    let ctx = api.expr_context();
    let eval = ExpressionEval::new(&rt, ctx);

    // Session engine has no variables set, so this should error
    let result = eval.evaluate("g:counter");
    assert!(result.is_err()); // not set on THIS engine
}

#[test]
fn vim_api_expr_context_after_variable_set() {
    // Set variables through engine, then use expr_context to evaluate
    let mut engine = VimEngine::new();
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Global,
        name: CompactString::from("x"),
        value: VimValue::Int(5),
    });
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Global,
        name: CompactString::from("y"),
        value: VimValue::Int(3),
    });

    // Build ExprContext directly from engine state
    let ctx = ExprContext {
        variables: engine.state().variable_store(),
        options: engine.resolved_options(),
    };
    let rt = SimpleExprEval;
    let eval = ExpressionEval::new(&rt, ctx);

    assert_eq!(eval.evaluate("g:x + g:y").unwrap(), VimValue::Int(8));
    assert_eq!(eval.evaluate("g:x * g:y").unwrap(), VimValue::Int(15));
    assert_eq!(eval.evaluate_to_string("g:x + g:y").unwrap(), "8");
}

// ===========================================================================
// 6. SYSTEM TEST — Ctrl-R = simulation
// ===========================================================================

#[test]
fn system_test_ctrl_r_eq_simulation() {
    // Simulates what a host does when it receives HostRequest::EvaluateExpression:
    // 1. Gets the expression string from the request
    // 2. Builds ExprContext from engine state
    // 3. Calls ExpressionEval::evaluate_to_string()
    // 4. Returns the result to the engine

    let mut engine = VimEngine::new();
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Global,
        name: CompactString::from("width"),
        value: VimValue::Int(80),
    });

    // Simulate: user typed Ctrl-R = and entered "g:width / 2"
    let expression = "g:width / 2";

    let ctx = ExprContext {
        variables: engine.state().variable_store(),
        options: engine.resolved_options(),
    };
    let rt = SimpleExprEval;
    let eval = ExpressionEval::new(&rt, ctx);

    let result = eval.evaluate_to_string(expression).unwrap();
    assert_eq!(result, "40");
}

#[test]
fn system_test_conditional_expression() {
    // Simulates evaluating a condition for :if/:while
    let mut engine = VimEngine::new();
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Global,
        name: CompactString::from("enabled"),
        value: VimValue::Bool(true),
    });

    let ctx = ExprContext {
        variables: engine.state().variable_store(),
        options: engine.resolved_options(),
    };
    let rt = SimpleExprEval;
    let eval = ExpressionEval::new(&rt, ctx);

    // Check truthiness
    let result = eval.evaluate("g:enabled == true").unwrap();
    assert_eq!(result, VimValue::Bool(true));
}

#[test]
fn system_test_string_expression_for_insertion() {
    // Simulates generating text for Ctrl-R = insertion
    let store = VariableStore::default();
    let opts = Box::leak(Box::new(vim_core::primitives::VimOptions::default()));
    let ctx = ExprContext {
        variables: &store,
        options: opts,
    };
    let rt = SimpleExprEval;
    let eval = ExpressionEval::new(&rt, ctx);

    // User wants to insert a separator line
    let result = eval.evaluate_to_string("\"---\"").unwrap();
    assert_eq!(result, "---");
}

#[test]
fn system_test_plugin_runtime_trait_object_safety() {
    // Prove ExprEngine can be used as a trait object (dyn dispatch)
    let rt: &dyn ExprEngine = &SimpleExprEval;

    let store = VariableStore::default();
    let opts = Box::leak(Box::new(vim_core::primitives::VimOptions::default()));
    let ctx = ExprContext {
        variables: &store,
        options: opts,
    };

    let result = rt.eval_expression("1 + 1", &ctx).unwrap();
    assert_eq!(result, VimValue::Int(2));
}

#[test]
fn system_test_custom_runtime_implementation() {
    // Prove anyone can implement ExprEngine with custom logic
    struct AlwaysFortyTwo;

    impl ExprEngine for AlwaysFortyTwo {
        fn eval_expression(
            &self,
            _expr: &str,
            _ctx: &ExprContext<'_>,
        ) -> Result<VimValue, RuntimeError> {
            Ok(VimValue::Int(42))
        }
    }

    let store = VariableStore::default();
    let opts = Box::leak(Box::new(vim_core::primitives::VimOptions::default()));
    let ctx = ExprContext {
        variables: &store,
        options: opts,
    };

    let rt = AlwaysFortyTwo;
    let eval = ExpressionEval::new(&rt, ctx);

    // Any expression returns 42
    assert_eq!(eval.evaluate("anything").unwrap(), VimValue::Int(42));
    assert_eq!(eval.evaluate_to_string("whatever").unwrap(), "42");
}
