//! Detect column defaults that demonstrably violate a table-level CHECK
//! constraint.
//!
//! This is fault **F86** in the data-dependent migration fault taxonomy:
//! every `INSERT` that relies on the column's default value is rejected by
//! the database at runtime. The migration itself succeeds — only the first
//! application `INSERT` discovers the mismatch.
//!
//! Vespertide rejects this *statically* during `validate_schema`. The
//! checker recognises two simple CHECK shapes (intentionally narrow to
//! avoid embedding a SQL parser):
//!
//! ```text
//!   <column> <op>  <literal>          // op ∈ { > >= < <= = <> != }
//!   <column> IN (<lit>, <lit>, ...)
//! ```
//!
//! Anything else (function calls, AND/OR composition, subqueries, casts,
//! references to other columns) is treated as *unparseable* and silently
//! passes — by design, since misjudging a complex expression as violated
//! would block legitimate schemas.

use vespertide_core::{DefaultValue, TableConstraint, TableDef};

use crate::error::PlannerError;

/// Inspect every column in `table`: if it has a default value AND there is
/// a table-level CHECK constraint that this checker can parse as a simple
/// pattern over the column, evaluate the default against the constraint
/// and raise [`PlannerError::DefaultViolatesCheck`] on mismatch.
///
/// Static: no data access. Pure structural / textual analysis.
pub(super) fn validate_default_vs_check(table: &TableDef) -> Result<(), PlannerError> {
    for column in &table.columns {
        let Some(default) = column.default.as_ref() else {
            continue;
        };
        let column_name = column.name.as_str();

        for constraint in &table.constraints {
            let TableConstraint::Check { name, expr, .. } = constraint else {
                continue;
            };
            let Some(parsed) = parse_simple_check(expr, column_name) else {
                continue; // unparseable shape — silent pass by design
            };
            if !check_satisfied(&parsed, default) {
                return Err(PlannerError::DefaultViolatesCheck {
                    table: table.name.to_string(),
                    column: column_name.to_string(),
                    default_value: default.to_sql(),
                    check_name: name.clone(),
                    check_expr: expr.clone(),
                });
            }
        }
    }
    Ok(())
}

/// Recognised shapes of a CHECK expression that this checker can evaluate.
#[derive(Debug, Clone, PartialEq)]
enum SimpleCheck {
    /// `<column> <op> <literal>`.
    Op { op: Op, value: Literal },
    /// `<column> IN (<lit>, <lit>, ...)`. Empty list is rejected at parse
    /// time so we never see it here.
    In(Vec<Literal>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// Lightweight literal representation that survives `DefaultValue`
/// comparison without dragging in a full SQL grammar.
///
/// `Eq` is intentionally not derived because the `Float` variant wraps an
/// `f64`. Equality is computed by [`literal_equals`] which folds Integer
/// and Float into the same numeric axis.
#[derive(Debug, Clone, PartialEq)]
enum Literal {
    Integer(i64),
    Float(f64),
    /// SQL string literal *as written*, e.g. `'pending'`. Single quotes
    /// are preserved so equality with `DefaultValue::String`'s `to_sql()`
    /// output works without ad-hoc unquoting.
    String(String),
    Bool(bool),
    Null,
}

/// Boolean shim so callers outside `check_default` can ask "is this
/// CHECK in the narrow recognisable shape against this column?" without
/// touching the private [`SimpleCheck`] / [`Literal`] / [`Op`] types.
/// Used by [`super::check_additions`] (F4) to identify the target
/// column of an added CHECK.
pub(super) fn matches_simple_check(expr: &str, column: &str) -> bool {
    parse_simple_check(expr, column).is_some()
}

fn parse_simple_check(expr: &str, column: &str) -> Option<SimpleCheck> {
    let trimmed = expr.trim();

    // Try IN list first because `<col> IN ...` would otherwise look like
    // a comparison op below.
    if let Some(rest) = strip_column_then_keyword(trimmed, column, "IN") {
        return parse_paren_list(rest.trim()).map(SimpleCheck::In);
    }

    // Try `<col> <op> <literal>`. Longer operators first so `<=` is not
    // misread as `<`.
    for &(token, op) in &[
        (">=", Op::Ge),
        ("<=", Op::Le),
        ("<>", Op::Ne),
        ("!=", Op::Ne),
        (">", Op::Gt),
        ("<", Op::Lt),
        ("=", Op::Eq),
    ] {
        if let Some((lhs, rhs)) = split_once_outside_strings(trimmed, token)
            && lhs.trim() == column
        {
            let value = parse_literal(rhs.trim())?;
            return Some(SimpleCheck::Op { op, value });
        }
    }
    None
}

/// `column IN (rest)` — return `Some(rest)` when the trimmed expression
/// starts with the column identifier followed by the `IN` keyword
/// (case-insensitive, whitespace-tolerant).
fn strip_column_then_keyword<'a>(expr: &'a str, column: &str, keyword: &str) -> Option<&'a str> {
    let rest = expr.strip_prefix(column)?;
    let rest = rest.trim_start();
    let keyword_upper = keyword.to_ascii_uppercase();
    let rest_upper = rest.get(..keyword_upper.len())?.to_ascii_uppercase();
    if rest_upper != keyword_upper {
        return None;
    }
    // The next char must be whitespace or `(` so we don't match `INTEGER`.
    let after = rest.get(keyword_upper.len()..)?;
    if after.chars().next()? != ' ' && after.chars().next()? != '(' {
        return None;
    }
    Some(after.trim_start())
}

/// Parse `( lit, lit, lit )` into a `Vec<Literal>`. Returns `None` if the
/// surface shape isn't a parenthesised comma-separated literal list.
fn parse_paren_list(rest: &str) -> Option<Vec<Literal>> {
    let stripped = rest.strip_prefix('(')?.strip_suffix(')')?;
    let mut items = Vec::new();
    for chunk in split_top_level_commas(stripped) {
        items.push(parse_literal(chunk.trim())?);
    }
    if items.is_empty() {
        return None;
    }
    Some(items)
}

/// Comma-split that respects single-quoted strings (so `'a,b', 'c'` becomes
/// two chunks). No nested parens / functions allowed — those land us in
/// "unparseable, silent pass" territory.
fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut in_string = false;
    let mut start = 0usize;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\'' {
            // doubled quote is SQL escape; skip both
            if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                i += 2;
                continue;
            }
            in_string = !in_string;
        } else if b == b',' && !in_string {
            out.push(&s[start..i]);
            start = i + 1;
        }
        i += 1;
    }
    out.push(&s[start..]);
    out
}

/// Like `str::split_once`, but skips matches inside single-quoted strings
/// so `status = 'a=b'` parses correctly.
fn split_once_outside_strings<'a>(s: &'a str, sep: &str) -> Option<(&'a str, &'a str)> {
    let sep_bytes = sep.as_bytes();
    let bytes = s.as_bytes();
    let mut in_string = false;
    let mut i = 0;
    while i + sep_bytes.len() <= bytes.len() {
        let b = bytes[i];
        if b == b'\'' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                i += 2;
                continue;
            }
            in_string = !in_string;
            i += 1;
            continue;
        }
        if !in_string && &bytes[i..i + sep_bytes.len()] == sep_bytes {
            return Some((&s[..i], &s[i + sep_bytes.len()..]));
        }
        i += 1;
    }
    None
}

fn parse_literal(s: &str) -> Option<Literal> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("NULL") {
        return Some(Literal::Null);
    }
    if s.eq_ignore_ascii_case("TRUE") {
        return Some(Literal::Bool(true));
    }
    if s.eq_ignore_ascii_case("FALSE") {
        return Some(Literal::Bool(false));
    }
    if s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2 {
        return Some(Literal::String(s.to_string()));
    }
    if let Ok(i) = s.parse::<i64>() {
        return Some(Literal::Integer(i));
    }
    if let Ok(f) = s.parse::<f64>() {
        return Some(Literal::Float(f));
    }
    None
}

fn check_satisfied(check: &SimpleCheck, default: &DefaultValue) -> bool {
    match check {
        SimpleCheck::Op { op, value } => evaluate_op(*op, default, value),
        SimpleCheck::In(list) => list.iter().any(|v| literal_equals(default, v)),
    }
}

fn evaluate_op(op: Op, default: &DefaultValue, target: &Literal) -> bool {
    match (default, target) {
        (DefaultValue::Integer(a), Literal::Integer(b)) => apply_op_i64(op, *a, *b),
        (DefaultValue::Float(a), Literal::Float(b)) => apply_op_f64(op, *a, *b),
        (DefaultValue::Integer(a), Literal::Float(b)) => apply_op_f64(op, i64_to_f64(*a), *b),
        (DefaultValue::Float(a), Literal::Integer(b)) => apply_op_f64(op, *a, i64_to_f64(*b)),
        (DefaultValue::String(a), Literal::String(b)) => apply_op_str(op, a, b),
        (DefaultValue::Bool(a), Literal::Bool(b)) => apply_op_bool(op, *a, *b),
        // Type mismatch — can't evaluate confidently. Treat as satisfied
        // to avoid false positives on `default: "now()"` style expressions
        // we don't recognise as a literal.
        _ => true,
    }
}

/// Lossy widening cast confined to this module so the precision-loss
/// `#[expect]` lives in exactly one place. `f64` only has 52-bit mantissa;
/// `i64` defaults outside `±2^53` will round, but checks involving
/// `2^53+`-sized defaults are vanishingly rare and the F86 detector
/// intentionally errs on the side of "silent pass" when ambiguous.
#[expect(
    clippy::cast_precision_loss,
    reason = "CHECK evaluation: rounding integers beyond 2^53 is acceptable since F86 silent-passes on ambiguity anyway"
)]
fn i64_to_f64(v: i64) -> f64 {
    v as f64
}

fn apply_op_i64(op: Op, a: i64, b: i64) -> bool {
    match op {
        Op::Eq => a == b,
        Op::Ne => a != b,
        Op::Lt => a < b,
        Op::Le => a <= b,
        Op::Gt => a > b,
        Op::Ge => a >= b,
    }
}

fn apply_op_f64(op: Op, a: f64, b: f64) -> bool {
    match op {
        // NaN handling: any comparison with NaN is false except !=.
        Op::Eq => (a - b).abs() < f64::EPSILON,
        Op::Ne => (a - b).abs() >= f64::EPSILON,
        Op::Lt => a < b,
        Op::Le => a <= b,
        Op::Gt => a > b,
        Op::Ge => a >= b,
    }
}

fn apply_op_str(op: Op, a: &str, b: &str) -> bool {
    match op {
        Op::Eq => a == b,
        Op::Ne => a != b,
        Op::Lt => a < b,
        Op::Le => a <= b,
        Op::Gt => a > b,
        Op::Ge => a >= b,
    }
}

fn apply_op_bool(op: Op, a: bool, b: bool) -> bool {
    match op {
        Op::Eq => a == b,
        Op::Ne => a != b,
        // Ordering on booleans is not idiomatic; refuse to judge so the
        // user keeps full control.
        _ => true,
    }
}

fn literal_equals(default: &DefaultValue, lit: &Literal) -> bool {
    match (default, lit) {
        (DefaultValue::Integer(a), Literal::Integer(b)) => a == b,
        (DefaultValue::Float(a), Literal::Float(b)) => (a - b).abs() < f64::EPSILON,
        (DefaultValue::Integer(a), Literal::Float(b)) => {
            (i64_to_f64(*a) - b).abs() < f64::EPSILON
        }
        (DefaultValue::Float(a), Literal::Integer(b)) => {
            (a - i64_to_f64(*b)).abs() < f64::EPSILON
        }
        (DefaultValue::String(a), Literal::String(b)) => a == b,
        (DefaultValue::Bool(a), Literal::Bool(b)) => a == b,
        _ => false,
    }
}
