//! Expression evaluation entry point.
//!
//! [`ExpressionEval`] wraps an [`ExprContext`] and a [`ExprEngine`] to
//! evaluate expressions for `Ctrl-R =` insertion and `:if`/`:while` conditions.

use compact_str::CompactString;

use crate::execution::api::VimApi;
use crate::execution::api_error::ApiError;
use crate::execution::expr_engine::{ExprContext, ExprEngine};
use crate::primitives::VimValue;

// ═══════════════════════════════════════════════════════════════════════════════
// ExpressionEval
// ═══════════════════════════════════════════════════════════════════════════════

/// Expression evaluation entry point.
///
/// Wraps a [`ExprEngine`] and an [`ExprContext`] to evaluate expressions.
/// Used by hosts to fulfill `HostRequest::EvaluateExpression`.
pub struct ExpressionEval<'a> {
    runtime: &'a dyn ExprEngine,
    context: ExprContext<'a>,
}

impl<'a> ExpressionEval<'a> {
    /// Create from a runtime and pre-built context.
    #[inline]
    #[must_use]
    pub fn new(runtime: &'a dyn ExprEngine, context: ExprContext<'a>) -> Self {
        Self { runtime, context }
    }

    /// Create from a [`VimApi`] handle, extracting the context automatically.
    #[inline]
    #[must_use]
    pub fn from_api(api: &'a VimApi<'a>, runtime: &'a dyn ExprEngine) -> Self {
        Self {
            runtime,
            context: api.expr_context(),
        }
    }

    /// Evaluate an expression and return the result as a [`VimValue`].
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::RuntimeError`] if the expression fails to parse or evaluate.
    pub fn evaluate(&self, expr: &str) -> Result<VimValue, ApiError> {
        self.runtime
            .eval_expression(expr, &self.context)
            .map_err(|e| ApiError::RuntimeError(CompactString::from(e.to_string())))
    }

    /// Evaluate an expression and convert the result to a string.
    ///
    /// Used for `Ctrl-R =` insertion where the expression result must be
    /// inserted as text into the buffer.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::RuntimeError`] if the expression fails to parse or evaluate.
    pub fn evaluate_to_string(&self, expr: &str) -> Result<String, ApiError> {
        let value = self.evaluate(expr)?;
        Ok(vim_value_to_string(&value))
    }
}

/// Convert a [`VimValue`] to its string representation for insertion.
fn vim_value_to_string(value: &VimValue) -> String {
    match value {
        VimValue::Nil => String::new(),
        VimValue::Bool(b) => {
            if *b {
                "1".to_owned()
            } else {
                "0".to_owned()
            }
        }
        VimValue::Int(n) => n.to_string(),
        VimValue::Float(f) => format!("{f}"),
        VimValue::String(s) => s.to_string(),
        VimValue::List(items) => items
            .iter()
            .map(vim_value_to_string)
            .collect::<Vec<_>>()
            .join(" "),
        VimValue::Map(_) => "{...}".to_owned(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Unit tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::expr_engine::SimpleExprEval;
    use crate::primitives::VimOptions;
    use crate::state::VariableStore;

    fn make_expression_eval<'a>(
        runtime: &'a SimpleExprEval,
        variables: &'a VariableStore,
        options: &'a VimOptions,
    ) -> ExpressionEval<'a> {
        let context = ExprContext { variables, options };
        ExpressionEval::new(runtime, context)
    }

    #[test]
    fn evaluate_integer() {
        let runtime = SimpleExprEval;
        let vars = VariableStore::default();
        let opts = VimOptions::default();
        let expr_eval = make_expression_eval(&runtime, &vars, &opts);
        assert_eq!(expr_eval.evaluate("42").unwrap(), VimValue::Int(42));
    }

    #[test]
    fn evaluate_to_string_int() {
        let runtime = SimpleExprEval;
        let vars = VariableStore::default();
        let opts = VimOptions::default();
        let expr_eval = make_expression_eval(&runtime, &vars, &opts);
        assert_eq!(expr_eval.evaluate_to_string("2 + 3").unwrap(), "5");
    }

    #[test]
    fn evaluate_to_string_string() {
        let runtime = SimpleExprEval;
        let vars = VariableStore::default();
        let opts = VimOptions::default();
        let expr_eval = make_expression_eval(&runtime, &vars, &opts);
        assert_eq!(
            expr_eval
                .evaluate_to_string("\"hello\" . \" world\"")
                .unwrap(),
            "hello world"
        );
    }

    #[test]
    fn evaluate_to_string_bool() {
        let runtime = SimpleExprEval;
        let vars = VariableStore::default();
        let opts = VimOptions::default();
        let expr_eval = make_expression_eval(&runtime, &vars, &opts);
        assert_eq!(expr_eval.evaluate_to_string("true").unwrap(), "1");
        assert_eq!(expr_eval.evaluate_to_string("false").unwrap(), "0");
    }

    #[test]
    fn evaluate_to_string_nil() {
        let runtime = SimpleExprEval;
        let mut vars = VariableStore::default();
        let opts = VimOptions::default();
        vars.set(crate::primitives::VarScope::Global, "x", VimValue::Nil);
        let expr_eval = make_expression_eval(&runtime, &vars, &opts);
        assert_eq!(expr_eval.evaluate_to_string("g:x").unwrap(), "");
    }

    #[test]
    fn evaluate_error_maps_to_api_error() {
        let runtime = SimpleExprEval;
        let vars = VariableStore::default();
        let opts = VimOptions::default();
        let expr_eval = make_expression_eval(&runtime, &vars, &opts);
        let err = expr_eval.evaluate("1 / 0").unwrap_err();
        assert!(matches!(err, ApiError::RuntimeError(_)));
    }
}
