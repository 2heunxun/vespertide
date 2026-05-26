pub(super) use super::*;
pub(super) use crate::error::PlannerError;
pub(super) use crate::validate::schema::validate_table;
pub(super) use rstest::rstest;
pub(super) use vespertide_core::schema::primary_key::{PrimaryKeyDef, PrimaryKeySyntax};
pub(super) use vespertide_core::{
    ColumnDef, ColumnType, ComplexColumnType, DefaultValue, EnumValues, MigrationAction,
    MigrationPlan, NumValue, SimpleColumnType, TableConstraint, TableDef,
};

fn col(name: &str, ty: ColumnType) -> ColumnDef {
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
    }
}

mod constraint_drops;
mod enum_fill_with;
mod fill_with;
mod fk_policy_changes;
mod fk_supporting_index;
mod plan_validation;
mod schema_cases;
