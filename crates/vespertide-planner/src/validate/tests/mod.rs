pub(super) use super::*;
pub(super) use crate::error::PlannerError;
pub(super) use crate::validate::schema::validate_table;
pub(super) use rstest::rstest;
pub(super) use vespertide_core::schema::primary_key::{PrimaryKeyDef, PrimaryKeySyntax};
pub(super) use vespertide_core::{
    ColumnDef, ColumnType, ComplexColumnType, DefaultValue, EnumValues, MigrationAction,
    MigrationPlan, NumValue, SimpleColumnType, TableConstraint, TableDef,
};

/// Test column helper. Defaults to `nullable: false` to match the
/// production-model pattern (every example/* model in this repo declares
/// `nullable` explicitly and the typed-schema convention is NOT NULL
/// unless stated otherwise). Tests that need a nullable column should
/// call [`col_nullable`].
fn col(name: &str, ty: ColumnType) -> ColumnDef {
    ColumnDef::new(name, ty, false)
}

/// Test column helper for the rare fixture that needs `nullable: true`.
/// Not currently used inside `tests/` — fixtures that want a nullable
/// column write the struct literal directly. Kept available as a hook
/// for future tests that exercise nullable-column behaviour without
/// duplicating the `ColumnDef::new(..., true)` boilerplate.
#[expect(dead_code, reason = "kept available for future nullable-column fixtures")]
fn col_nullable(name: &str, ty: ColumnType) -> ColumnDef {
    ColumnDef::new(name, ty, true)
}

fn table(name: &str, columns: Vec<ColumnDef>, constraints: Vec<TableConstraint>) -> TableDef {
    TableDef {
        name: name.into(),
        description: None,
        columns,
        constraints,
    }
}

fn idx(name: &str, columns: Vec<&str>) -> TableConstraint {
    TableConstraint::Index {
        name: Some(name.to_string()),
        columns: columns.into_iter().map(Into::into).collect(),
    }
}

fn is_duplicate(err: &PlannerError) -> bool {
    matches!(err, PlannerError::DuplicateTableName(_))
}

fn is_fk_table(err: &PlannerError) -> bool {
    matches!(err, PlannerError::ForeignKeyTableNotFound(_, _, _))
}

fn is_fk_column(err: &PlannerError) -> bool {
    matches!(err, PlannerError::ForeignKeyColumnNotFound(_, _, _, _))
}

fn is_index_column(err: &PlannerError) -> bool {
    matches!(err, PlannerError::IndexColumnNotFound(_, _, _))
}

fn is_constraint_column(err: &PlannerError) -> bool {
    matches!(err, PlannerError::ConstraintColumnNotFound(_, _, _))
}

fn is_empty_columns(err: &PlannerError) -> bool {
    matches!(err, PlannerError::EmptyConstraintColumns(_, _))
}

fn is_missing_pk(err: &PlannerError) -> bool {
    matches!(err, PlannerError::MissingPrimaryKey(_))
}

fn pk(columns: Vec<&str>) -> TableConstraint {
    TableConstraint::PrimaryKey {
        auto_increment: false,
        columns: columns.into_iter().map(Into::into).collect(),
        strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
    }
}

mod check_default;
mod constraint_drops;
mod dangling_fk_drops;
mod enum_fill_with;
mod fill_with;
mod fk_policy_changes;
mod fk_supporting_index;
mod plan_validation;
mod schema_cases;
mod timezone_conversion;
mod type_narrowing;
