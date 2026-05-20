mod direct;
mod sqlite_rebuild;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use sea_query::{Alias, Expr, Query};

use vespertide_core::{ColumnType, TableDef};

use self::direct::build_modify_column_type_direct;
use self::sqlite_rebuild::build_modify_column_type_sqlite_temp_table;
use super::types::{BuiltQuery, DatabaseBackend};
use crate::error::QueryError;

/// Build UPDATE statements for `fill_with` mappings (removed enum values → replacement values).
/// Each entry generates: UPDATE "table" SET "column" = 'replacement' WHERE "column" = '`removed_value`'
fn build_fill_with_updates(
    table: &str,
    column: &str,
    fill_with: &BTreeMap<String, String>,
) -> Vec<BuiltQuery> {
    fill_with
        .iter()
        .map(|(removed_value, replacement)| {
            let update_stmt = Query::update()
                .table(Alias::new(table))
                .value(Alias::new(column), Expr::val(replacement.as_str()))
                .and_where(Expr::col(Alias::new(column)).eq(removed_value.as_str()))
                .to_owned();
            BuiltQuery::Update(Box::new(update_stmt))
        })
        .collect()
}

pub fn build_modify_column_type(
    backend: DatabaseBackend,
    table: &str,
    column: &str,
    new_type: &ColumnType,
    fill_with: Option<&BTreeMap<String, String>>,
    current_schema: &[TableDef],
    pending_constraints: &[vespertide_core::TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    // SQLite does not support direct column type modification, so use temporary table approach
    if backend == DatabaseBackend::Sqlite {
        return build_modify_column_type_sqlite_temp_table(
            backend,
            table,
            column,
            new_type,
            fill_with,
            current_schema,
            pending_constraints,
        );
    }

    // PostgreSQL, MySQL, etc. can use ALTER TABLE directly
    Ok(build_modify_column_type_direct(
        backend,
        table,
        column,
        new_type,
        fill_with,
        current_schema,
    ))
}
