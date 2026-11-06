//! Tests for the config expression evaluator.

use super::*;
use crate::execution::config::TrustAll;
use proptest::prelude::*;

/// Options with 'ignorecase' as given and 'textwidth' 33.
fn options(ignorecase: bool) -> VimOptions {
    let mut options = VimOptions::default();
    options.set_ignorecase(ignorecase);
    options.set_textwidth(33);
    options
}

/// Evaluate `text`, with 'textwidth' 33 in effect and 20 as the global
/// value.
fn eval_with(
    text: &str,
    variables: &VariableStore,
    ignorecase: bool,
) -> Result<Value, Vec<String>> {
    let options = options(ignorecase);
    let mut global = options.clone();
    global.set_textwidth(20);
    let env = Env {
        variables,
        options: &options,
        global_options: &global,
        host: &TrustAll,
    };
    let evaluated = evaluate(text, &env);
    match evaluated.value {
        Some(value) if evaluated.errors.is_empty() => Ok(value),
        _ => Err(evaluated.errors.iter().map(ToString::to_string).collect()),
    }
}

fn eval(text: &str) -> Result<Value, Vec<String>> {
    eval_with(text, &VariableStore::default(), false)
}

fn number(text: &str) -> i64 {
    match eval(text) {
        Ok(Value::Number(n)) => n,
        other => panic!("{text}: {other:?}"),
    }
}

fn string(text: &str) -> String {
    match eval(text) {
        Ok(Value::String(s)) => s,
        other => panic!("{text}: {other:?}"),
    }
}

fn errors(text: &str) -> Vec<String> {
    eval(text).expect_err(text)
}

fn echoed(text: &str) -> Vec<String> {
    let variables = VariableStore::default();
    let options = options(false);
    let env = Env {
        variables: &variables,
        options: &options,
        global_options: &options,
        host: &TrustAll,
    };
    echo(text, &env)
        .into_iter()
        .map(|shown| match shown {
            Echoed::Text(text) => text,
            Echoed::Error(error) => error.to_string(),
        })
        .collect()
}

// ── has() ─────────────────────────────────────────────────────────────

#[test]
fn has_answers_as_vim_9_1_0_does() {
    // f_has(), has_patch()
    assert_eq!(number("!has('nvim')"), 1);
    assert_eq!(number("has('eval')"), 1);
    assert_eq!(number("has('EVAL')"), 1);
    assert_eq!(number("has('patch-9.1.0')"), 1);
    assert_eq!(number("has('patch-9.1.1')"), 0);
    assert_eq!(number("has('patch-8.2.5000')"), 1);
    assert_eq!(number("has('patch-9.10.0')"), 0);
    assert_eq!(number("has('patch-10.0.0')"), 0);
    assert_eq!(number("has('patch0')"), 1);
    assert_eq!(number("has('patch1')"), 0);
    assert_eq!(number("has('vim9script')"), 0);
    assert_eq!(number("has ('eval')"), 1);
}

#[test]
fn has_with_a_check_tells_a_feature_name() {
    // f_has(): a {check} that is not zero returns whether the name is a
    // feature at all.
    assert_eq!(number("has('vim9script', 1)"), 1);
    assert_eq!(number("has('nvim', 1)"), 0);
    assert_eq!(number("has('patch-99.0.0', 1)"), 1);
    assert_eq!(number("has('eval', 0)"), 1);
}

#[test]
fn has_counts_its_arguments() {
    assert_eq!(
        errors("has()"),
        ["E119: Not enough arguments for function: has"]
    );
    assert_eq!(
        errors("has(1, 2, 3)"),
        ["E118: Too many arguments for function: has"]
    );
    assert_eq!(number("has('eval',)"), 1);
    assert_eq!(
        errors("has(,1)"),
        ["E116: Invalid arguments for function has"]
    );
    assert_eq!(
        errors("has(1 2)"),
        ["E116: Invalid arguments for function has"]
    );
}

#[test]
fn an_argument_that_fails_gives_e116_for_each_call_around_it() {
    // get_func_tv()
    assert_eq!(
        errors("has(exists(nosuch))"),
        [
            "E121: Undefined variable: nosuch",
            "E116: Invalid arguments for function exists",
            "E116: Invalid arguments for function has"
        ]
    );
}

#[test]
fn more_than_twenty_arguments_give_e740() {
    let args = vec!["1"; 21].join(", ");
    assert_eq!(
        errors(&format!("Foo({args})")),
        ["E740: Too many arguments for function Foo"]
    );
}

// ── && || ?: ──────────────────────────────────────────────────────────

#[test]
fn logic_short_circuits() {
    // eval2(), eval3(): the side not taken is read, not evaluated.
    assert_eq!(number("0 || 1 && 0"), 0);
    assert_eq!(number("0 && Foo()"), 0);
    assert_eq!(number("1 || Foo()"), 1);
    assert_eq!(number("1 && 2 || 0"), 1);
    assert_eq!(number("0 || 0 || 3"), 1);
    assert_eq!(
        errors("0 && Foo("),
        ["E15: Invalid expression: \"0 && Foo(\""]
    );
    assert_eq!(errors("0 && \"abc"), ["E114: Missing double quote: \"abc"]);
}

#[test]
fn ternary_takes_one_branch() {
    // eval1()
    assert_eq!(number("1 ? 2 : 3"), 2);
    assert_eq!(number("0 ? 2 : 3"), 3);
    assert_eq!(number("0 ? nosuch : 3"), 3);
    assert_eq!(number("1 ? 1 ? 2 : 3 : 4"), 2);
    assert_eq!(number("0 ? 1 : 0 ? 2 : 3"), 3);
    assert_eq!(number("1 + 1 ? 5 : 6"), 5);
    assert_eq!(errors("1 ? 2"), ["E109: Missing ':' after '?'"]);
    assert_eq!(errors("1 : 2"), ["E488: Trailing characters: : 2"]);
}

#[test]
fn a_float_is_no_truth_value() {
    // tv_get_number_chk()
    assert_eq!(errors("1.5 && 1"), ["E805: Using a Float as a Number"]);
    assert_eq!(errors("1.5 ? 1 : 2"), ["E805: Using a Float as a Number"]);
}

// ── variables ─────────────────────────────────────────────────────────

#[test]
fn version_names_read_vim_9_1() {
    assert_eq!(number("v:version >= 800"), 1);
    assert_eq!(number("version == 901"), 1);
    assert_eq!(number("v:versionlong"), 9_010_000);
    assert_eq!(eval("v:true"), Ok(Value::Bool(true)));
    assert_eq!(errors("v:nosuch"), ["E121: Undefined variable: v:nosuch"]);
}

#[test]
fn variables_read_the_store() {
    let mut variables = VariableStore::default();
    variables.set(VarScope::Global, "x", VimValue::Int(7));
    variables.set(VarScope::Buffer, "y", VimValue::String("b".into()));
    variables.set(VarScope::Global, "l", VimValue::List(Vec::new()));
    let eval = |text: &str| eval_with(text, &variables, false);
    assert_eq!(eval("g:x + x"), Ok(Value::Number(14)));
    assert_eq!(eval("b:y"), Ok(Value::String("b".into())));
    assert_eq!(eval("exists('g:x')"), Ok(Value::Number(1)));
    assert_eq!(eval("exists('b:x')"), Ok(Value::Number(0)));
    assert_eq!(
        eval("g:l"),
        Err(vec![
            "E319: Sorry, the command is not available in this version: g:l".into()
        ])
    );
    assert_eq!(
        eval("s:x"),
        Err(vec!["E121: Undefined variable: s:x".into()])
    );
}

// ── literals ──────────────────────────────────────────────────────────

#[test]
fn string_escapes_follow_eval_string() {
    assert_eq!(string(r#""\<Space>""#), " ");
    assert_eq!(string(r#""\<C-W>""#), "\x17");
    assert_eq!(
        string(r#""\<CR>\<Esc>\<Tab>\<lt>\<Bar>\<Bslash>""#),
        "\r\x1b\t<|\\"
    );
    assert_eq!(string(r#""\<F5>""#), "<F5>");
    assert_eq!(string(r#""\<nosuch>""#), "<nosuch>");
    assert_eq!(string(r#""a\tb""#), "a\tb");
    assert_eq!(string(r#""\\""#), "\\");
    assert_eq!(string(r#""\"""#), "\"");
    assert_eq!(string("'it''s'"), "it's");
    assert_eq!(string(r"'a\tb'"), "a\\tb");
    assert_eq!(string(r#""\x41""#), "A");
    assert_eq!(string(r#""\101""#), "A");
    assert_eq!(string(r#""\u00e9""#), "é");
    assert_eq!(string(r#""\U0001F600""#), "😀");
    assert_eq!(string(r#""\x""#), "x");
    assert_eq!(string(r#""\q""#), "q");
    // "\400" wraps to a NUL byte, which ends the string.
    assert_eq!(string(r#""a\400b""#), "a");
}

#[test]
fn an_unterminated_string_names_its_text() {
    assert_eq!(errors("\"abc"), ["E114: Missing double quote: \"abc"]);
    assert_eq!(errors("'abc"), ["E115: Missing single quote: 'abc"]);
}

#[test]
fn numbers_take_every_base() {
    // eval_number(), vim_str2nr() with STR2NR_ALL
    assert_eq!(number("0x1F"), 31);
    assert_eq!(number("017"), 15);
    assert_eq!(number("019"), 19);
    assert_eq!(number("08"), 8);
    assert_eq!(number("0b101"), 5);
    assert_eq!(number("0o17"), 15);
    assert_eq!(number("99999999999999999999"), i64::MAX);
    assert_eq!(errors("1x"), ["E15: Invalid expression: \"1x\""]);
    assert_eq!(errors(".5"), ["E15: Invalid expression: \".5\""]);
}

#[test]
fn a_string_is_a_number_by_its_start() {
    // tv_get_number_chk()
    assert_eq!(number("\"0100\" + 0"), 64);
    assert_eq!(number("\"+8\" + 0"), 0);
    assert_eq!(number("\"6bar\" + 0"), 6);
    assert_eq!(number("\"-12\" + 0"), -12);
    assert_eq!(number("\"abc\" + 0"), 0);
}

#[test]
fn floats_follow_legacy_script() {
    assert_eq!(eval("1.5 + 1"), Ok(Value::Float(2.5)));
    assert_eq!(eval("2.5e3"), Ok(Value::Float(2500.0)));
    assert_eq!(string("1.5 . \"x\""), "1.5x");
    assert_eq!(string("1.0 . ''"), "1.0");
    assert_eq!(string("1.0e10 . ''"), "1.0e10");
    assert_eq!(string("1 / 0.0 . ''"), "inf");
    // No float after ".", so a version string stays one.
    assert_eq!(string("1.2.3"), "123");
    assert_eq!(string("\"a\" . 1.5"), "a15");
    assert_eq!(errors("1.5 % 2"), ["E804: Cannot use '%' with Float"]);
    assert_eq!(eval("!1.5"), Ok(Value::Float(0.0)));
    assert_eq!(
        errors("1.5 == \"1.5\""),
        ["E892: Using a String as a Float"]
    );
}

// ── operators ─────────────────────────────────────────────────────────

#[test]
fn comparisons_follow_ignorecase() {
    // eval4(), typval_compare()
    let off = |text: &str| eval_with(text, &VariableStore::default(), false);
    let on = |text: &str| eval_with(text, &VariableStore::default(), true);
    assert_eq!(off("\"abc\" == \"ABC\""), Ok(Value::Number(0)));
    assert_eq!(off("\"abc\" ==? \"ABC\""), Ok(Value::Number(1)));
    assert_eq!(on("\"abc\" == \"ABC\""), Ok(Value::Number(1)));
    assert_eq!(on("\"abc\" ==# \"ABC\""), Ok(Value::Number(0)));
    assert_eq!(on("\"a\" is \"A\""), Ok(Value::Number(1)));
    assert_eq!(off("\"ABC\" =~ 'abc'"), Ok(Value::Number(0)));
    assert_eq!(off("\"ABC\" =~? 'abc'"), Ok(Value::Number(1)));
    assert_eq!(on("\"b\" < \"A\""), Ok(Value::Number(0)));
}

#[test]
fn comparisons_take_the_type_vim_takes() {
    assert_eq!(number("\"10\" == 10"), 1);
    assert_eq!(number("\"abc\" == 0"), 1);
    assert_eq!(number("\"xterm-256\" =~ 'xterm'"), 1);
    assert_eq!(number("\"xterm\" !~ '^vt'"), 1);
    assert_eq!(number("'a' is 'a'"), 1);
    assert_eq!(number("1 is \"1\""), 0);
    assert_eq!(number("1 isnot \"1\""), 1);
    assert_eq!(number("v:true == 1"), 1);
    assert_eq!(number("v:true is 1"), 0);
    assert_eq!(string("v:true . ''"), "v:true");
    assert_eq!(errors("1 == 1 == 1"), ["E488: Trailing characters: == 1"]);
}

#[test]
fn a_pattern_that_does_not_compile_gives_its_error() {
    let result = errors("\"abc\" =~ 'a\\('");
    assert_eq!(result.len(), 1);
    assert!(result[0].starts_with("E54"), "{result:?}");
}

#[test]
fn arithmetic_never_panics() {
    // num_divide(), num_modulus(); overflow wraps.
    assert_eq!(number("7 / 0"), i64::MAX);
    assert_eq!(number("-7 / 0"), -i64::MAX);
    assert_eq!(number("0 / 0"), i64::MIN);
    assert_eq!(number("(-9223372036854775807 - 1) / -1"), i64::MAX);
    assert_eq!(number("5 % 0"), 0);
    assert_eq!(number("(-9223372036854775807 - 1) % -1"), 0);
    assert_eq!(number("-(-9223372036854775807 - 1)"), i64::MIN);
    assert_eq!(number("9223372036854775807 + 1"), i64::MIN);
    assert_eq!(number("7 / 2 * 2 + 7 % 2"), 7);
    assert_eq!(number("1 + 2 * 3 - 4"), 3);
}

#[test]
fn prefix_operators_loop() {
    // eval9_leader()
    assert_eq!(number("!\"abc\""), 1);
    assert_eq!(number("-\"3\""), -3);
    assert_eq!(number("!!5"), 1);
    assert_eq!(number("- - 1"), 1);
    assert_eq!(number("-(1 + 2)"), -3);
    assert_eq!(number("!has('nvim') && 1"), 1);
}

#[test]
fn concatenation_reads_dot_and_dots() {
    assert_eq!(string("'a' . 'b' .. 1"), "ab1");
    assert_eq!(string("1 . 2"), "12");
}

// ── options ───────────────────────────────────────────────────────────

#[test]
fn options_read_their_value() {
    // eval_option()
    assert_eq!(number("&tw"), 33);
    assert_eq!(number("&textwidth"), 33);
    assert_eq!(number("&l:tw"), 33);
    assert_eq!(number("&g:tw"), 20);
    assert_eq!(number("&ic"), 0);
    assert_eq!(
        eval_with("&ic", &VariableStore::default(), true),
        Ok(Value::Number(1))
    );
    assert_eq!(string("&selection"), "inclusive");
    assert_eq!(errors("&nosuch"), ["E113: Unknown option: nosuch"]);
    assert_eq!(errors("&g:nosuch"), ["E113: Unknown option: nosuch"]);
    assert_eq!(errors("& + 1"), ["E112: Option name missing: & + 1"]);
    assert_eq!(number("0 && &nosuch"), 0);
}

#[test]
fn options_this_build_lacks_read_as_empty() {
    // eval_option() with gov_hidden_*: a Vim option vim-core lacks.
    assert_eq!(string("&mouse"), "");
    assert_eq!(string("&t_Co"), "");
    assert_eq!(string("&g:enc"), "");
    assert_eq!(number("&compatible"), 0);
    assert_eq!(number("&l:report"), 0);
    assert_eq!(string("&cms"), "// %s");
}

#[test]
fn environment_variables_are_empty() {
    // Deviation: Vim reads the process environment.
    assert_eq!(string("$HOME"), "");
    assert_eq!(number("exists('$HOME')"), 0);
}

// ── exists() ──────────────────────────────────────────────────────────

#[test]
fn exists_answers_as_f_exists() {
    assert_eq!(number("exists('&tw')"), 1);
    assert_eq!(number("exists('+tw')"), 1);
    assert_eq!(number("exists('&g:tw')"), 1);
    assert_eq!(number("exists('&tw x')"), 0);
    assert_eq!(number("exists('&nosuch')"), 0);
    // eval_option(): an option this build lacks exists, but does not work.
    assert_eq!(number("exists('&mouse')"), 1);
    assert_eq!(number("exists('+mouse')"), 0);
    assert_eq!(number("exists('&t_Co')"), 1);
    assert_eq!(number("exists('+cms')"), 1);
    assert_eq!(number("exists(':set')"), 2);
    assert_eq!(number("exists(':se')"), 1);
    assert_eq!(number("exists(':sil')"), 1);
    assert_eq!(number("exists(':silent')"), 2);
    assert_eq!(number("exists(':nosuchcmd')"), 0);
    assert_eq!(number("exists(':set x')"), 0);
    assert_eq!(number("exists(':2match')"), 2);
    assert_eq!(number("exists(':2set')"), 0);
    assert_eq!(number("exists('*has')"), 1);
    assert_eq!(number("exists('*exists(')"), 1);
    assert_eq!(number("exists('?has')"), 1);
    assert_eq!(number("exists('*Foo')"), 0);
    assert_eq!(number("exists('v:version')"), 1);
    assert_eq!(number("exists('version')"), 1);
    assert_eq!(number("exists('g:nosuch')"), 0);
    assert_eq!(number("exists('#BufRead')"), 0);
}

// ── errors ────────────────────────────────────────────────────────────

#[test]
fn errors_name_what_failed() {
    assert_eq!(errors("Foo()"), ["E117: Unknown function: Foo"]);
    assert_eq!(errors("nosuch"), ["E121: Undefined variable: nosuch"]);
    assert_eq!(errors("1 +"), ["E15: Invalid expression: \"1 +\""]);
    assert_eq!(errors("1 + )"), ["E15: Invalid expression: \")\""]);
    assert_eq!(errors("1 2"), ["E488: Trailing characters: 2"]);
    assert_eq!(errors("(1"), ["E110: Missing ')'"]);
    assert_eq!(errors("(1, 2)"), ["E110: Missing ')'"]);
}

#[test]
fn forms_vim_has_and_this_evaluator_lacks_give_e319() {
    for text in [
        "[1, 2]",
        "{'a': 1}",
        "#{a: 1}",
        "@a",
        "1 ?? 2",
        "1 << 2",
        "'abc'[0]",
        "'abc'->len()",
        "0z00",
        "$'x'",
        "g:{x}",
    ] {
        assert_eq!(
            errors(text),
            [format!(
                "E319: Sorry, the command is not available in this version: {text}"
            )],
            "{text}"
        );
    }
}

#[test]
fn nesting_stops_at_three_hundred() {
    // eval9(): 300 levels, as Vim built with MSVC allows.
    let nested = |n: usize| format!("{}1{}", "(".repeat(n), ")".repeat(n));
    assert_eq!(number(&nested(299)), 1);
    let error = errors(&nested(300));
    assert_eq!(error.len(), 1);
    assert!(
        error[0].starts_with("E1169: Expression too recursive: 1)"),
        "{error:?}"
    );
    let calls = format!("{}1{}", "has(".repeat(300), ")".repeat(300));
    assert!(errors(&calls)[0].starts_with("E1169"));
}

#[test]
fn deep_nesting_needs_no_stack() {
    let handle = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let opens = "(".repeat(100_000);
            let error = errors(&opens);
            assert!(error[0].starts_with("E1169"), "{error:?}");
            assert_eq!(number(&format!("{}1", "!".repeat(100_000))), 1);
            assert_eq!(number(&format!("{}1", "1 ? 1 : ".repeat(100_000))), 1);
        })
        .unwrap();
    handle.join().unwrap();
}

// ── :echo ─────────────────────────────────────────────────────────────

#[test]
fn echo_joins_its_values() {
    // ex_echo()
    assert_eq!(echoed("\"a\" 1 'b'"), ["a 1 b"]);
    assert_eq!(echoed(""), [""]);
    assert_eq!(echoed("\"a|b\""), ["a|b"]);
    assert_eq!(echoed("1.0"), ["1.0"]);
}

#[test]
fn echo_stops_at_an_expression_that_fails() {
    assert_eq!(
        echoed("\"a\" nosuch \"b\""),
        ["a", "E121: Undefined variable: nosuch"]
    );
    assert_eq!(echoed("1, 2"), ["1", "E15: Invalid expression: \", 2\""]);
    assert_eq!(echoed("1 +"), ["E15: Invalid expression: \"1 +\""]);
}

// ── :let ──────────────────────────────────────────────────────────────

#[test]
fn let_reads_its_target_and_operator() {
    // ex_let(), skip_var_one()
    let variable = |scope, name, written| {
        Target::Variable(Variable {
            scope,
            name,
            written,
        })
    };
    let cases = [
        ("g:x = 1", variable(VarScope::Global, "x", "g:x"), None, "1"),
        ("x.=1", variable(VarScope::Global, "x", "x"), Some('.'), "1"),
        (
            "x ..= 'a'",
            variable(VarScope::Global, "x", "x"),
            Some('.'),
            "'a'",
        ),
        (
            "b:y -= 2",
            variable(VarScope::Buffer, "y", "b:y"),
            Some('-'),
            "2",
        ),
        (
            "g:a#b %= 2",
            variable(VarScope::Global, "a#b", "g:a#b"),
            Some('%'),
            "2",
        ),
        (
            "&l:tw += 2",
            Target::Option(OptionLayer::Local, "tw"),
            Some('+'),
            "2",
        ),
        (
            "&g:ic = 1",
            Target::Option(OptionLayer::Global, "ic"),
            None,
            "1",
        ),
        (
            "&cms = x",
            Target::Option(OptionLayer::Both, "cms"),
            None,
            "x",
        ),
    ];
    for (arg, target, op, expr) in cases {
        assert_eq!(
            parse_let(arg),
            Some(Assignment { target, op, expr }),
            "{arg}"
        );
    }
}

#[test]
fn let_forms_the_evaluator_lacks_are_none() {
    for arg in [
        "s:x = 1",
        "w:x = 1",
        "v:x = 1",
        "version = 1",
        "[a, b] = l",
        "x =<< END",
        "x",
        "x y",
        "@a = 1",
        "$X = 1",
        "x[0] = 1",
        "d.k = 1",
        "g: = 1",
        "b:x#y = 1",
        "{x} = 1",
        "& = 1",
    ] {
        assert_eq!(parse_let(arg), None, "{arg}");
    }
}

#[test]
fn let_operators_follow_tv_op() {
    let n = Value::Number;
    let s = |t: &str| Value::String(t.to_owned());
    assert_eq!(assign_op('+', &n(1), &n(2)), Ok(n(3)));
    assert_eq!(assign_op('+', &s("2"), &n(3)), Ok(n(5)));
    assert_eq!(assign_op('/', &n(7), &n(0)), Ok(n(i64::MAX)));
    assert_eq!(assign_op('.', &n(5), &n(1)), Ok(s("51")));
    assert_eq!(
        assign_op('.', &s("a"), &Value::Bool(true)),
        Ok(s("av:true"))
    );
    assert_eq!(
        assign_op('*', &n(2), &Value::Float(1.5)),
        Ok(Value::Float(3.0))
    );
    assert_eq!(
        assign_op('-', &Value::Float(1.5), &s("1")),
        Ok(Value::Float(0.5))
    );
    for (op, current, value) in [
        ('.', n(1), Value::Float(1.5)),
        ('%', Value::Float(1.5), n(1)),
        ('.', Value::Float(1.5), s("x")),
        ('+', n(1), Value::Bool(true)),
        ('+', Value::Bool(true), n(1)),
    ] {
        assert_eq!(
            assign_op(op, &current, &value),
            Err(VimError::WrongVariableType(op)),
            "{op} {current:?} {value:?}"
        );
    }
}

// ── where an expression ends ──────────────────────────────────────────

/// A generated expression, and whether it is a single operand.
fn expression() -> impl Strategy<Value = (String, bool)> {
    let leaf = prop_oneof![
        Just("0"),
        Just("42"),
        Just("0x1F"),
        // No exponent: after `.` Vim reads `2.5e3` as 2, then `.`, then E15.
        Just("1.5"),
        Just("\"a\""),
        Just("\"a|b\""),
        Just("\"q\\\"q\""),
        Just("\"\\<Space>\""),
        Just("'it''s'"),
        Just("'|'"),
        Just("g:x"),
        Just("foo"),
        Just("v:version"),
        Just("&tw"),
        Just("&g:ic"),
        Just("$HOME"),
        Just("has('eval')"),
        Just("exists ( \"x\" )"),
    ]
    .prop_map(|s| (s.to_owned(), true));
    leaf.prop_recursive(4, 32, 3, |inner| {
        let operand = |(text, single): (String, bool)| {
            if single {
                text
            } else {
                format!("({text})")
            }
        };
        prop_oneof![
            (
                inner.clone(),
                prop::sample::select(vec![
                    "||", "&&", "==", "!=#", "=~?", "is", "isnot", "<", ">=", "+", "-", ".", "..",
                    "*", "/", "%"
                ]),
                inner.clone()
            )
                .prop_map(move |(a, op, b)| (format!("{} {op} {}", operand(a), operand(b)), false)),
            (inner.clone(), inner.clone(), inner.clone()).prop_map(move |(c, t, e)| (
                format!("{} ? {} : {}", operand(c), operand(t), operand(e)),
                false
            )),
            inner
                .clone()
                .prop_map(|(text, _)| (format!("({text})"), true)),
            inner
                .clone()
                .prop_map(move |e| (format!("!{}", operand(e)), true)),
            (inner.clone(), inner).prop_map(move |(a, b)| (format!("Foo({}, {})", a.0, b.0), true)),
        ]
    })
}

proptest! {
    #[test]
    fn the_parser_ends_where_the_splitter_ends(expr in expression()) {
        let (expr, _) = expr;
        let line = format!("let v = {expr} | x");
        let pieces = crate::grammar::split_ex_pipeline(&line);
        let piece = format!("let v = {expr}");
        prop_assert_eq!(pieces.first().map(AsRef::as_ref), Some(piece.as_str()));
        prop_assert_eq!(pieces.get(1).map(AsRef::as_ref), Some("x"));

        let variables = VariableStore::default();
        let options = VimOptions::default();
        let env = Env { variables: &variables, options: &options, global_options: &options, host: &TrustAll };
        let text = format!("{expr} | x");
        let mut parser = Parser::new(&text, &env);
        let result = parser.expr1(false);
        prop_assert!(result.is_ok(), "{:?}", parser.errors);
        parser.skip_blanks();
        prop_assert_eq!(parser.pos, expr.len() + 1);
    }
}
