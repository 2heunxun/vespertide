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
    ColumnDef {
        name: name.to_string(),
        r#type: ty,
        nullable: true,
        default: None,
        comment: None,
        primary_key: None,
        unique: None,
        index: None,
        foreign_key: None,
    }
}

fn table(name: &str, columns: Vec<ColumnDef>, constraints: Vec<TableConstraint>) -> TableDef {
    TableDef {
        name: name.to_string(),
        description: None,
        columns,
        constraints,
    }
}

fn idx(name: &str, columns: Vec<&str>) -> TableConstraint {
    TableConstraint::Index {
        name: Some(name.to_string()),
        columns: columns
            .into_iter()
            .map(std::string::ToString::to_string)
            .collect(),
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
        columns: columns
            .into_iter()
            .map(std::string::ToString::to_string)
            .collect(),
    }
}

#[path = "tests/enum_fill_with.rs"]
mod enum_fill_with;
#[path = "tests/fill_with.rs"]
mod fill_with;
#[path = "tests/plan_validation.rs"]
mod plan_validation;
#[path = "tests/schema_cases.rs"]
mod schema_cases;
