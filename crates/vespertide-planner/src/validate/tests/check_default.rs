use super::*;
use crate::validate::validate_schema;
use vespertide_core::DefaultValue;

fn validate_one(table: TableDef) -> Result<(), PlannerError> {
    // F86 sits behind `validate_table_entry`, which is private; route every
    // assertion through the public `validate_schema` entry so the
    // check_default hook actually fires.
    validate_schema(&[table])
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn col_with_default(name: &str, ty: ColumnType, default: DefaultValue) -> ColumnDef {
    let mut c = col(name, ty);
    c.nullable = false;
    c.default = Some(default);
    c
}

fn check_constraint(name: &str, expr: &str) -> TableConstraint {
    TableConstraint::Check {
        name: name.to_string(),
        expr: expr.to_string(),
        strategy: vespertide_core::CheckViolationStrategy::default(),
    }
}

fn pk_col(name: &str) -> ColumnDef {
    let mut c = col(name, ColumnType::Simple(SimpleColumnType::Integer));
    c.nullable = false;
    c.primary_key = Some(
        vespertide_core::schema::primary_key::PrimaryKeySyntax::Bool(true),
    );
    c
}

fn table_with(
    name: &str,
    payload_col: ColumnDef,
    checks: Vec<TableConstraint>,
) -> TableDef {
    let mut constraints = checks;
    constraints.push(TableConstraint::PrimaryKey {
        auto_increment: false,
        columns: vec!["id".into()],
    });
    table("the_table", vec![pk_col("id"), payload_col], constraints)
        .with_name_for_test(name)
}

trait WithNameForTest {
    fn with_name_for_test(self, _name: &str) -> Self;
}
impl WithNameForTest for TableDef {
    fn with_name_for_test(mut self, name: &str) -> Self {
        self.name = name.into();
        self
    }
}

fn is_default_violates_check(err: &PlannerError) -> bool {
    matches!(err, PlannerError::DefaultViolatesCheck { .. })
}

// ---------------------------------------------------------------------------
// Violations: each comparison op + IN list rejection
// ---------------------------------------------------------------------------

#[test]
fn integer_default_zero_violates_check_amount_gt_zero() {
    let table = table_with(
        "orders",
        col_with_default(
            "amount",
            ColumnType::Simple(SimpleColumnType::Integer),
            DefaultValue::Integer(0),
        ),
        vec![check_constraint("chk_positive", "amount > 0")],
    );
    let err = validate_one(table).expect_err("default 0 should violate amount > 0");
    assert!(is_default_violates_check(&err), "got: {err:?}");
}

#[test]
fn integer_default_zero_violates_check_amount_ge_one() {
    let table = table_with(
        "orders",
        col_with_default(
            "amount",
            ColumnType::Simple(SimpleColumnType::Integer),
            DefaultValue::Integer(0),
        ),
        vec![check_constraint("chk_one_plus", "amount >= 1")],
    );
    let err = validate_one(table).expect_err("default 0 should violate amount >= 1");
    assert!(is_default_violates_check(&err));
}

#[test]
fn integer_default_100_violates_check_amount_lt_50() {
    let table = table_with(
        "orders",
        col_with_default(
            "amount",
            ColumnType::Simple(SimpleColumnType::Integer),
            DefaultValue::Integer(100),
        ),
        vec![check_constraint("chk_max", "amount < 50")],
    );
    let err = validate_one(table).unwrap_err();
    assert!(is_default_violates_check(&err));
}

#[test]
fn string_default_violates_in_list() {
    let table = table_with(
        "users",
        col_with_default(
            "status",
            ColumnType::Simple(SimpleColumnType::Text),
            DefaultValue::String("'banned'".into()),
        ),
        vec![check_constraint(
            "chk_status",
            "status IN ('active', 'inactive', 'pending')",
        )],
    );
    let err = validate_one(table).unwrap_err();
    assert!(is_default_violates_check(&err));
    if let PlannerError::DefaultViolatesCheck { default_value, .. } = err {
        assert_eq!(default_value, "'banned'");
    }
}

#[test]
fn string_default_violates_equality() {
    let table = table_with(
        "users",
        col_with_default(
            "role",
            ColumnType::Simple(SimpleColumnType::Text),
            DefaultValue::String("'admin'".into()),
        ),
        vec![check_constraint("chk_role", "role = 'user'")],
    );
    assert!(is_default_violates_check(
        &validate_one(table).unwrap_err()
    ));
}

#[test]
fn integer_default_violates_ne() {
    let table = table_with(
        "orders",
        col_with_default(
            "amount",
            ColumnType::Simple(SimpleColumnType::Integer),
            DefaultValue::Integer(0),
        ),
        vec![check_constraint("chk_not_zero", "amount <> 0")],
    );
    assert!(is_default_violates_check(
        &validate_one(table).unwrap_err()
    ));
}

// ---------------------------------------------------------------------------
// Satisfied: every op passes when the default fits
// ---------------------------------------------------------------------------

#[test]
fn integer_default_one_satisfies_amount_gt_zero() {
    let table = table_with(
        "orders",
        col_with_default(
            "amount",
            ColumnType::Simple(SimpleColumnType::Integer),
            DefaultValue::Integer(1),
        ),
        vec![check_constraint("chk_positive", "amount > 0")],
    );
    assert!(validate_one(table).is_ok());
}

#[test]
fn string_default_satisfies_in_list() {
    let table = table_with(
        "users",
        col_with_default(
            "status",
            ColumnType::Simple(SimpleColumnType::Text),
            DefaultValue::String("'active'".into()),
        ),
        vec![check_constraint(
            "chk_status",
            "status IN ('active', 'inactive')",
        )],
    );
    assert!(validate_one(table).is_ok());
}

#[test]
fn boolean_default_satisfies_equality() {
    let table = table_with(
        "flags",
        col_with_default(
            "enabled",
            ColumnType::Simple(SimpleColumnType::Boolean),
            DefaultValue::Bool(true),
        ),
        vec![check_constraint("chk_enabled", "enabled = true")],
    );
    assert!(validate_one(table).is_ok());
}

// ---------------------------------------------------------------------------
// Silent pass: complex expressions intentionally not evaluated
// ---------------------------------------------------------------------------

#[test]
fn function_call_check_is_silent_pass() {
    let table = table_with(
        "users",
        col_with_default(
            "email",
            ColumnType::Simple(SimpleColumnType::Text),
            DefaultValue::String("''".into()),
        ),
        vec![check_constraint("chk_email_shape", "length(email) > 0")],
    );
    // The default '' has length 0 which *would* violate, but the checker
    // does not parse function calls. Silent pass is the design choice.
    assert!(validate_one(table).is_ok());
}

#[test]
fn and_composed_check_is_silent_pass() {
    let table = table_with(
        "orders",
        col_with_default(
            "amount",
            ColumnType::Simple(SimpleColumnType::Integer),
            DefaultValue::Integer(50),
        ),
        vec![check_constraint(
            "chk_range",
            "amount > 0 AND amount < 100",
        )],
    );
    // Default 50 *would* satisfy, but AND-composition isn't evaluated
    // either way — silent pass holds.
    assert!(validate_one(table).is_ok());
}

#[test]
fn check_referring_to_a_different_column_is_silent_pass() {
    // CHECK on `total` while `amount` has the default — the checker only
    // evaluates checks whose LHS matches the column being defaulted.
    let table = table_with(
        "orders",
        col_with_default(
            "amount",
            ColumnType::Simple(SimpleColumnType::Integer),
            DefaultValue::Integer(0),
        ),
        vec![check_constraint("chk_total", "total > 0")],
    );
    assert!(validate_one(table).is_ok());
}

#[test]
fn check_with_function_call_default_is_silent_pass_on_default_side() {
    // `now()` style defaults are stored as a String value (not Integer/Float),
    // so even with a parseable CHECK the type mismatch path triggers silent pass.
    let table = table_with(
        "events",
        col_with_default(
            "at",
            ColumnType::Simple(SimpleColumnType::Timestamp),
            DefaultValue::String("now()".into()),
        ),
        vec![check_constraint("chk_some_int", "at > 0")],
    );
    assert!(validate_one(table).is_ok());
}

// ---------------------------------------------------------------------------
// Aggregation: only the right error fires
// ---------------------------------------------------------------------------

#[test]
fn no_default_means_no_check_against_check() {
    // Column has no default — F86 has nothing to evaluate.
    let table = table_with(
        "orders",
        col("amount", ColumnType::Simple(SimpleColumnType::Integer)),
        vec![check_constraint("chk_positive", "amount > 0")],
    );
    assert!(validate_one(table).is_ok());
}

#[test]
fn table_without_check_constraints_is_passthrough() {
    let table = table_with(
        "orders",
        col_with_default(
            "amount",
            ColumnType::Simple(SimpleColumnType::Integer),
            DefaultValue::Integer(0),
        ),
        vec![],
    );
    assert!(validate_one(table).is_ok());
}

#[test]
fn error_message_includes_all_context() {
    let table = table_with(
        "orders",
        col_with_default(
            "amount",
            ColumnType::Simple(SimpleColumnType::Integer),
            DefaultValue::Integer(0),
        ),
        vec![check_constraint("chk_positive", "amount > 0")],
    );
    let err = validate_one(table).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("orders.amount"));
    assert!(msg.contains("amount > 0"));
    assert!(msg.contains('0'));
}
