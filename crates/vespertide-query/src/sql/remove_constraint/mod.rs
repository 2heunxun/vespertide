mod mysql;
mod postgres;
mod sqlite;

#[cfg(test)]
mod tests;

use sea_query::Alias;

use vespertide_core::{TableConstraint, TableDef};

use super::types::{BuiltQuery, DatabaseBackend};
use crate::error::QueryError;

pub fn build_remove_constraint(
    backend: DatabaseBackend,
    table: &str,
    constraint: &TableConstraint,
    current_schema: &[TableDef],
    pending_constraints: &[TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    if backend == DatabaseBackend::Sqlite && sqlite::requires_rebuild(constraint) {
        return sqlite::build_remove_constraint(
            table,
            constraint,
            current_schema,
            pending_constraints,
        );
    }

    match backend {
        DatabaseBackend::Postgres => Ok(postgres::build_remove_constraint(table, constraint)),
        DatabaseBackend::MySql => Ok(mysql::build_remove_constraint(table, constraint)),
        DatabaseBackend::Sqlite => build_drop_index(table, constraint),
    }
}

fn build_drop_index(
    table: &str,
    constraint: &TableConstraint,
) -> Result<Vec<BuiltQuery>, QueryError> {
    let TableConstraint::Index { name, columns } = constraint else {
        return Err(QueryError::Other(format!(
            "SQLite constraint '{}' requires a table rebuild",
            constraint_kind(constraint)
        )));
    };

    let index_name = vespertide_naming::build_index_name(table, columns, name.as_deref());
    let idx_drop = sea_query::Index::drop()
        .table(Alias::new(table))
        .name(&index_name)
        .to_owned();
    Ok(vec![BuiltQuery::DropIndex(Box::new(idx_drop))])
}

fn constraint_kind(constraint: &TableConstraint) -> &'static str {
    match constraint {
        TableConstraint::PrimaryKey { .. } => "primary key",
        TableConstraint::Unique { .. } => "unique",
        TableConstraint::ForeignKey { .. } => "foreign key",
        TableConstraint::Index { .. } => "index",
        TableConstraint::Check { .. } => "check",
        _ => unreachable!("TableConstraint is #[non_exhaustive]; all variants are matched above"),
    }
}
