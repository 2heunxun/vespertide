#![expect(
    clippy::doc_markdown,
    reason = "narrative prose: SQL terms (Compare, IsNull, Between) appear as plain words intentionally"
)]
//! Fault **F-novel-1** - CHECK self-contradiction detection.
//!
//! A CHECK constraint whose top-level `AND` conjuncts contain a
//! demonstrable contradiction on the same column. Every row would
//! be rejected by the database because no value can satisfy all
//! conjuncts simultaneously. Almost always an authoring error.
//!
//! # Recognised contradictions
//!
//! For two predicates `P1` and `P2` referencing the same column,
//! the comparator flags these patterns as **demonstrably
//! contradictory**:
//!
//! 1. **Range impossibility** (`Compare` x `Compare`):
//!    - `col > N` and `col < M` where `N >= M`
//!      (no value can be both greater than N and less than M when N >= M)
//!    - `col >= N` and `col <= M` where `N > M`
//!    - `col >= N` and `col < M` where `N >= M`
//!    - `col > N` and `col <= M` where `N >= M`
//! 2. **Boundary impossibility** (same literal):
//!    - `col >= N` and `col < N` (strict less excludes the boundary)
//!    - `col > N` and `col <= N` (strict greater excludes the boundary)
//! 3. **Equality conflict** (`Compare(Eq)` x `Compare(Eq)`):
//!    - `col = X` and `col = Y` where `X != Y` and same literal type
//! 4. **Equality vs not-equality**:
//!    - `col = X` and `col != X` (same literal)
//! 5. **Null conflict** (`IsNull` x `IsNull`):
//!    - `col IS NULL` and `col IS NOT NULL`
//! 6. **Null vs equality** (CHECK passes on NULL by SQL semantics,
//!    but inside an AND with `IS NOT NULL` the equality demands a
//!    non-NULL value matching X — combined with `IS NULL` on same
//!    column, the AND can never be satisfied):
//!    - `col IS NULL` and `col = X`
//!    - `col IS NULL` and `col != X`
//!    - `col IS NULL` and `col > X` (or any non-IS Compare)
//!
//! # Suppression rules (conservative, false-positive 0)
//!
//! - `OR` branches are not analysed (would require proving *every*
//!   branch contradicts — much harder, and the resulting "always
//!   false OR" tautology is rare in real schemas).
//! - `NOT` wrappers are not unfolded — `NOT (col > 5)` is treated
//!   as opaque to keep the comparator simple.
//! - Mixed-type literals (string compared to integer, etc.) silently
//!   pass — F-novel-4 (type-mismatch) covers those.
//! - `BETWEEN` is decomposed into `>=` + `<=` for the contradiction
//!   check.
//! - Different columns never contradict each other (we don't model
//!   inter-column constraints).
//!
//! # Why hard error
//!
//! Mirrors F86 / F-novel-15: this is a *deterministic* failure -
//! the constraint rejects every row by construction. A prompt
//! would add friction without offering a meaningful choice; the
//! only correct fix is to edit the model.

use std::cmp::Ordering;

use vespertide_core::{TableConstraint, TableDef};

use super::check_expr_parser::{CheckExpr, Literal, Op, parse};
use crate::error::PlannerError;

/// Inspect every table-level CHECK constraint on `table`. If the
/// expression's top-level AND conjuncts contain a contradictory
/// pair on the same column, raise
/// [`PlannerError::CheckSelfContradiction`] on the first such
/// violation.
///
/// Static: no data access. Pure structural / textual analysis.
pub(super) fn validate_self_contradiction(table: &TableDef) -> Result<(), PlannerError> {
    find_self_contradictions(table)
        .into_iter()
        .next()
        .map_or(Ok(()), Err)
}

/// Inspect every table-level CHECK constraint on `table` and collect each
/// constraint whose expression contains a demonstrable self-contradiction.
///
/// Unlike `validate_self_contradiction`, this does not stop at the first
/// faulty constraint. It is used by editor diagnostics so independent CHECK
/// mistakes in one model all get their own squiggle.
pub fn find_self_contradictions(table: &TableDef) -> Vec<PlannerError> {
    let mut errors = Vec::new();
    for constraint in &table.constraints {
        let TableConstraint::Check { name, expr, .. } = constraint else {
            continue;
        };
        let parsed = parse(expr);
        if let Some(contradiction) = find_contradiction(&parsed) {
            errors.push(PlannerError::CheckSelfContradiction {
                table: table.name.to_string(),
                check_name: name.clone(),
                column: contradiction.column,
                first: contradiction.first,
                second: contradiction.second,
            });
        }
    }
    errors
}

/// First contradictory pair detected anywhere under an `And` node.
/// Returns `None` when nothing demonstrably contradicts.
fn find_contradiction(expr: &CheckExpr) -> Option<Contradiction> {
    // Top-level And: flatten and pairwise-check.
    if let CheckExpr::And(parts) = expr {
        let flat = flatten_and(parts);
        // Group by column to keep the pairwise loop cheap.
        let by_column = group_predicates_by_column(&flat);
        for (column, preds) in by_column {
            // Pairwise contradiction check within the same column.
            for i in 0..preds.len() {
                for j in (i + 1)..preds.len() {
                    if let Some(c) = check_pair(&column, preds[i], preds[j]) {
                        return Some(c);
                    }
                }
            }
        }
        // Recurse into nested ANDs and ORs - look for a contradiction
        // anywhere in the tree (not just the top-level AND).
        for part in flat {
            if let Some(c) = find_contradiction(part) {
                return Some(c);
            }
        }
        None
    } else if let CheckExpr::Or(parts) = expr {
        // Recurse into OR branches; a contradiction inside any branch
        // is still worth reporting (the branch itself is dead code).
        parts.iter().find_map(find_contradiction)
    } else {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Contradiction {
    column: String,
    first: String,
    second: String,
}

/// Flatten nested `And` nodes into a single Vec of leaf predicates.
/// Stops recursion at non-And nodes (so And-inside-Or is preserved
/// as one entry).
fn flatten_and(parts: &[CheckExpr]) -> Vec<&CheckExpr> {
    let mut out = Vec::new();
    for part in parts {
        match part {
            CheckExpr::And(inner) => out.extend(flatten_and(inner)),
            _ => out.push(part),
        }
    }
    out
}

/// Bucket `Compare` / `In` / `Between` / `IsNull` predicates by the
/// column they reference. Predicates that don't directly reference a
/// single column (And/Or/Not/Unparseable) are skipped.
fn group_predicates_by_column<'a>(flat: &[&'a CheckExpr]) -> Vec<(String, Vec<&'a CheckExpr>)> {
    let mut groups: Vec<(String, Vec<&'a CheckExpr>)> = Vec::new();
    for pred in flat {
        let Some(col) = predicate_column(pred) else {
            continue;
        };
        if let Some((_, existing)) = groups.iter_mut().find(|(c, _)| c == &col) {
            existing.push(pred);
        } else {
            groups.push((col, vec![pred]));
        }
    }
    groups
}

fn predicate_column(expr: &CheckExpr) -> Option<String> {
    match expr {
        CheckExpr::Compare { column, .. }
        | CheckExpr::In { column, .. }
        | CheckExpr::Between { column, .. }
        | CheckExpr::IsNull { column, .. } => Some(column.clone()),
        _ => None,
    }
}

/// Pairwise contradiction check for two predicates on the same column.
fn check_pair(column: &str, a: &CheckExpr, b: &CheckExpr) -> Option<Contradiction> {
    // Try Compare vs Compare in both orderings.
    if let (
        CheckExpr::Compare {
            op: op_a,
            value: va,
            ..
        },
        CheckExpr::Compare {
            op: op_b,
            value: vb,
            ..
        },
    ) = (a, b)
        && let Some((first, second)) = compare_pair_contradicts(*op_a, va, *op_b, vb)
    {
        return Some(Contradiction {
            column: column.to_string(),
            first: format_compare(column, *op_a, &first),
            second: format_compare(column, *op_b, &second),
        });
    }
    // IsNull vs IsNull: opposite negations contradict.
    if let (CheckExpr::IsNull { negated: na, .. }, CheckExpr::IsNull { negated: nb, .. }) = (a, b)
        && na != nb
    {
        return Some(Contradiction {
            column: column.to_string(),
            first: format_is_null(column, *na),
            second: format_is_null(column, *nb),
        });
    }
    // IsNull (positive) vs Compare on same column: AND is unsatisfiable.
    if let Some((isnull_neg, isnull_form, other_form)) = is_null_vs_other(column, a, b) {
        // Only positive `IS NULL` is contradictory with a non-null
        // comparison; `IS NOT NULL` is the *expected* companion of a
        // Compare and never contradicts.
        if !isnull_neg {
            return Some(Contradiction {
                column: column.to_string(),
                first: isnull_form,
                second: other_form,
            });
        }
    }
    None
}

/// Returns `Some((first_label, second_label))` when two Compare
/// predicates on the same column cannot be simultaneously satisfied.
/// The label strings are used by the caller for display; they
/// always echo the literal value passed in.
fn compare_pair_contradicts(
    op_a: Op,
    va: &Literal,
    op_b: Op,
    vb: &Literal,
) -> Option<(String, String)> {
    let cmp = literal_compare(va, vb)?; // Need ordered literals.

    // Equality conflict: col = X AND col = Y where X != Y.
    if op_a == Op::Eq && op_b == Op::Eq && cmp != Ordering::Equal {
        return Some((format_literal(va), format_literal(vb)));
    }
    // Equality vs negation: col = X AND col != X.
    if (op_a == Op::Eq && op_b == Op::Ne || op_a == Op::Ne && op_b == Op::Eq)
        && cmp == Ordering::Equal
    {
        return Some((format_literal(va), format_literal(vb)));
    }

    // Range impossibility: at most one direction each.
    let (lower_op, lower_val, upper_op, upper_val) = match (op_a, op_b) {
        // a is lower bound, b is upper bound:
        (Op::Gt | Op::Ge, Op::Lt | Op::Le) => (op_a, va, op_b, vb),
        // b is lower bound, a is upper bound:
        (Op::Lt | Op::Le, Op::Gt | Op::Ge) => (op_b, vb, op_a, va),
        _ => return None,
    };
    let lower_vs_upper = literal_compare(lower_val, upper_val)?;
    let unsatisfiable = match (lower_op, upper_op) {
        // col > N AND col < M : unsatisfiable when N >= M.
        (Op::Gt, Op::Lt) => matches!(lower_vs_upper, Ordering::Greater | Ordering::Equal),
        // col > N AND col <= M : unsatisfiable when N >= M.
        (Op::Gt, Op::Le) => matches!(lower_vs_upper, Ordering::Greater | Ordering::Equal),
        // col >= N AND col < M : unsatisfiable when N >= M.
        (Op::Ge, Op::Lt) => matches!(lower_vs_upper, Ordering::Greater | Ordering::Equal),
        // col >= N AND col <= M : unsatisfiable when N > M.
        (Op::Ge, Op::Le) => matches!(lower_vs_upper, Ordering::Greater),
        _ => false,
    };
    if unsatisfiable {
        Some((format_literal(va), format_literal(vb)))
    } else {
        None
    }
}

/// When one of `(a, b)` is `IsNull(negated)` and the other is any
/// `Compare`, return the IsNull's `negated` flag plus formatted
/// labels for the user. Returns `None` otherwise.
fn is_null_vs_other(column: &str, a: &CheckExpr, b: &CheckExpr) -> Option<(bool, String, String)> {
    // Normalise to (IsNull, Compare) ordering so the body is written once.
    let (is_null_expr, compare_expr) = match (a, b) {
        (CheckExpr::IsNull { .. }, CheckExpr::Compare { .. }) => (a, b),
        (CheckExpr::Compare { .. }, CheckExpr::IsNull { .. }) => (b, a),
        _ => return None,
    };
    let CheckExpr::IsNull { negated, .. } = is_null_expr else {
        return None;
    };
    let CheckExpr::Compare { op, value, .. } = compare_expr else {
        return None;
    };
    Some((
        *negated,
        format_is_null(column, *negated),
        format_compare(column, *op, &format_literal(value)),
    ))
}

fn literal_compare(a: &Literal, b: &Literal) -> Option<Ordering> {
    match (a, b) {
        (Literal::Integer(x), Literal::Integer(y)) => Some(x.cmp(y)),
        (Literal::Float(x), Literal::Float(y)) => x.partial_cmp(y),
        (Literal::Integer(x), Literal::Float(y)) => i64_to_f64(*x).partial_cmp(y),
        (Literal::Float(x), Literal::Integer(y)) => x.partial_cmp(&i64_to_f64(*y)),
        (Literal::String(x), Literal::String(y)) => Some(x.cmp(y)),
        (Literal::Bool(x), Literal::Bool(y)) => Some(x.cmp(y)),
        // Mixed / Null: can't conclude.
        _ => None,
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "F-novel-1 self-contradiction comparison: rounding integers beyond 2^53 acceptable; conservative comparator silently skips ambiguous cases"
)]
fn i64_to_f64(v: i64) -> f64 {
    v as f64
}

fn format_literal(lit: &Literal) -> String {
    match lit {
        Literal::Integer(i) => i.to_string(),
        Literal::Float(f) => f.to_string(),
        Literal::String(s) => s.clone(),
        Literal::Bool(b) => b.to_string(),
        Literal::Null => "NULL".to_string(),
    }
}

fn format_compare(column: &str, op: Op, value_text: &str) -> String {
    let op_str = match op {
        Op::Eq => "=",
        Op::Ne => "<>",
        Op::Lt => "<",
        Op::Le => "<=",
        Op::Gt => ">",
        Op::Ge => ">=",
    };
    format!("{column} {op_str} {value_text}")
}

fn format_is_null(column: &str, negated: bool) -> String {
    if negated {
        format!("{column} IS NOT NULL")
    } else {
        format!("{column} IS NULL")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vespertide_core::{
        CheckViolationStrategy, ColumnDef, ColumnType, SimpleColumnType, TableDef,
    };

    fn check(name: &str, expr: &str) -> TableConstraint {
        TableConstraint::Check {
            name: name.to_string(),
            expr: expr.to_string(),
            strategy: CheckViolationStrategy::default(),
        }
    }

    fn table(checks: Vec<TableConstraint>) -> TableDef {
        TableDef {
            name: "t".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: Some(
                    vespertide_core::schema::primary_key::PrimaryKeySyntax::Bool(true),
                ),
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: checks,
        }
    }

    // -- Range impossibility ---------------------------------------------

    #[test]
    fn gt_and_lt_range_impossible() {
        let t = table(vec![check("chk", "age > 100 AND age < 0")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    #[test]
    fn gt_and_lt_equal_boundaries_impossible() {
        // col > 5 AND col < 5 — no value satisfies both.
        let t = table(vec![check("chk", "age > 5 AND age < 5")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    #[test]
    fn ge_and_le_reversed_impossible() {
        // col >= 10 AND col <= 5 — empty interval.
        let t = table(vec![check("chk", "age >= 10 AND age <= 5")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    #[test]
    fn ge_and_le_equal_is_valid_singleton() {
        // col >= 5 AND col <= 5 = singleton {5}, non-empty.
        let t = table(vec![check("chk", "age >= 5 AND age <= 5")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    #[test]
    fn ge_and_lt_boundary_impossible() {
        // col >= 5 AND col < 5 — boundary excludes value.
        let t = table(vec![check("chk", "age >= 5 AND age < 5")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    #[test]
    fn gt_and_le_boundary_impossible() {
        // col > 5 AND col <= 5 — boundary excludes value.
        let t = table(vec![check("chk", "age > 5 AND age <= 5")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    #[test]
    fn proper_range_is_valid() {
        let t = table(vec![check("chk", "age > 0 AND age < 100")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    // -- Equality conflict -----------------------------------------------

    #[test]
    fn eq_with_different_literals_contradicts() {
        let t = table(vec![check("chk", "code = 'a' AND code = 'b'")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    #[test]
    fn eq_with_same_literal_is_fine() {
        let t = table(vec![check("chk", "code = 'a' AND code = 'a'")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    #[test]
    fn eq_vs_ne_same_literal_contradicts() {
        let t = table(vec![check("chk", "code = 'a' AND code <> 'a'")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    #[test]
    fn eq_vs_ne_different_literal_is_fine() {
        let t = table(vec![check("chk", "code = 'a' AND code <> 'b'")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    // -- Null conflict ---------------------------------------------------

    #[test]
    fn is_null_and_is_not_null_contradict() {
        let t = table(vec![check(
            "chk",
            "deleted_at IS NULL AND deleted_at IS NOT NULL",
        )]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    #[test]
    fn is_null_and_compare_contradicts() {
        // col IS NULL AND col = 5 — IS NULL demands NULL, = 5 demands non-NULL.
        let t = table(vec![check("chk", "score IS NULL AND score = 5")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    #[test]
    fn is_not_null_and_compare_is_fine() {
        let t = table(vec![check("chk", "score IS NOT NULL AND score = 5")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    #[test]
    fn is_null_alone_is_fine() {
        let t = table(vec![check("chk", "deleted_at IS NULL")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    // -- Different columns never contradict ------------------------------

    #[test]
    fn different_columns_with_opposite_predicates_pass() {
        let t = table(vec![check("chk", "a > 5 AND b < 5")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    #[test]
    fn different_columns_eq_pass() {
        let t = table(vec![check("chk", "a = 'x' AND b = 'y'")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    // -- Mixed types silently pass --------------------------------------

    #[test]
    fn integer_vs_string_literal_silently_passes() {
        // F-novel-4 territory; F-novel-1 doesn't second-guess.
        let t = table(vec![check("chk", "age > 5 AND age < 'foo'")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    // -- Composition -----------------------------------------------------

    #[test]
    fn contradiction_inside_or_branch_is_detected() {
        // The OR as a whole is satisfiable (the other branch works),
        // but the second branch is dead code — surface as warning.
        let t = table(vec![check("chk", "age < 0 OR (age > 100 AND age < 50)")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    #[test]
    fn nested_and_flattens() {
        // ((a AND b) AND c) treated as `a AND b AND c`.
        let t = table(vec![check("chk", "(age > 100 AND age < 200) AND age < 0")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    #[test]
    fn three_conjuncts_pairwise_check() {
        // No pair contradicts: 0 < age < 100, and age != 50.
        let t = table(vec![check("chk", "age > 0 AND age < 100 AND age <> 50")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    // -- Unparseable silently passes ------------------------------------

    #[test]
    fn unparseable_check_silently_passes() {
        let t = table(vec![check("chk", "LENGTH(name) > 0 AND LENGTH(name) < 0")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    #[test]
    fn first_violation_wins_when_multiple_checks_contradict() {
        let t = table(vec![
            check("chk_first", "age > 100 AND age < 0"),
            check("chk_second", "score = 1 AND score = 2"),
        ]);
        let err = validate_self_contradiction(&t).unwrap_err();
        let PlannerError::CheckSelfContradiction { check_name, .. } = err else {
            panic!("expected CheckSelfContradiction");
        };
        assert_eq!(check_name, "chk_first");
    }

    #[test]
    fn or_without_contradiction_passes() {
        let t = table(vec![check("chk", "age < 0 OR age > 100")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    #[test]
    fn finder_collects_two_contradicting_constraints() {
        let t = table(vec![
            check("chk_first", "age > 100 AND age < 0"),
            check("chk_second", "score = 1 AND score = 2"),
        ]);

        let errors = find_self_contradictions(&t);
        assert_eq!(errors.len(), 2);
        assert!(matches!(
            &errors[0],
            PlannerError::CheckSelfContradiction { check_name, .. } if check_name == "chk_first"
        ));
        assert!(matches!(
            &errors[1],
            PlannerError::CheckSelfContradiction { check_name, .. } if check_name == "chk_second"
        ));
    }

    #[test]
    fn finder_collects_one_contradiction_among_valid_constraints() {
        let t = table(vec![
            check("chk_valid", "age > 0 AND age < 100"),
            check("chk_impossible", "score >= 10 AND score <= 5"),
        ]);

        let errors = find_self_contradictions(&t);
        assert_eq!(errors.len(), 1);
        assert!(matches!(
            &errors[0],
            PlannerError::CheckSelfContradiction { check_name, .. } if check_name == "chk_impossible"
        ));
    }
}
