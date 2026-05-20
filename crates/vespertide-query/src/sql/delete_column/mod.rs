use sea_query::{Alias, Index};

use vespertide_core::{ColumnType, ComplexColumnType, TableConstraint, TableDef};

use self::direct::build_direct_delete_column;
use self::sqlite_rebuild::build_delete_column_sqlite_temp_table;
use super::types::{BuiltQuery, DatabaseBackend};

mod direct;
mod sqlite_rebuild;

#[cfg(test)]
mod tests;

/// Build SQL to delete a column, optionally with DROP TYPE for enum columns (`PostgreSQL`).
///
/// For `SQLite`: Handles constraint removal before dropping the column:
/// - Unique/Index constraints: Dropped via DROP INDEX
/// - ForeignKey/PrimaryKey constraints: Uses temp table approach (recreate table without column)
///
/// `SQLite` doesn't cascade constraint drops when a column is dropped.
pub fn build_delete_column(
    backend: DatabaseBackend,
    table: &str,
    column: &str,
    column_type: Option<&ColumnType>,
    current_schema: &[TableDef],
    pending_constraints: &[TableConstraint],
) -> Vec<BuiltQuery> {
    let mut stmts = Vec::new();

    // SQLite: Check if we need special handling for constraints.
    if backend == DatabaseBackend::Sqlite
        && let Some(table_def) = current_schema.iter().find(|t| t.name == table)
        && let Some(sqlite_queries) = sqlite_constraint_handling(
            table,
            column,
            table_def,
            column_type,
            pending_constraints,
            &mut stmts,
        )
    {
        return sqlite_queries;
    }

    stmts.extend(build_direct_delete_column(table, column, column_type));
    stmts
}

fn sqlite_constraint_handling(
    table: &str,
    column: &str,
    table_def: &TableDef,
    column_type: Option<&ColumnType>,
    pending_constraints: &[TableConstraint],
    stmts: &mut Vec<BuiltQuery>,
) -> Option<Vec<BuiltQuery>> {
    // If the column has an enum type, SQLite embeds a CHECK constraint in CREATE TABLE.
    // ALTER TABLE DROP COLUMN fails if the column is referenced by any CHECK.
    // Must use temp table approach.
    if let Some(col_def) = table_def.columns.iter().find(|c| c.name == column)
        && let ColumnType::Complex(ComplexColumnType::Enum { .. }) = &col_def.r#type
    {
        return Some(build_delete_column_sqlite_temp_table(
            table,
            column,
            table_def,
            column_type,
            pending_constraints,
        ));
    }

    // Handle constraints referencing the deleted column.
    for constraint in &table_def.constraints {
        match constraint {
            // Check constraints may reference the column in their expression.
            // SQLite can't DROP COLUMN if a CHECK references it — use temp table.
            TableConstraint::Check { expr, .. } => {
                // Check if the expression references the column (e.g. "status" IN (...)).
                if expr.contains(&format!("\"{column}\"")) || expr.contains(column) {
                    return Some(build_delete_column_sqlite_temp_table(
                        table,
                        column,
                        table_def,
                        column_type,
                        pending_constraints,
                    ));
                }
            }
            // For column-based constraints, check if they reference the deleted column.
            _ if !constraint.columns().iter().any(|c| c == column) => {}
            // FK/PK require temp table approach - return immediately.
            TableConstraint::ForeignKey { .. } | TableConstraint::PrimaryKey { .. } => {
                return Some(build_delete_column_sqlite_temp_table(
                    table,
                    column,
                    table_def,
                    column_type,
                    pending_constraints,
                ));
            }
            // Unique/Index: drop the index first, then drop column below.
            TableConstraint::Unique { name, columns } => {
                let index_name = vespertide_naming::build_unique_constraint_name(
                    table,
                    columns,
                    name.as_deref(),
                );
                let drop_idx = Index::drop()
                    .name(&index_name)
                    .table(Alias::new(table))
                    .to_owned();
                stmts.push(BuiltQuery::DropIndex(Box::new(drop_idx)));
            }
            TableConstraint::Index { name, columns } => {
                let index_name =
                    vespertide_naming::build_index_name(table, columns, name.as_deref());
                let drop_idx = Index::drop()
                    .name(&index_name)
                    .table(Alias::new(table))
                    .to_owned();
                stmts.push(BuiltQuery::DropIndex(Box::new(drop_idx)));
            }
            _ => {
                unreachable!("TableConstraint is #[non_exhaustive]; all variants are matched above")
            }
        }
    }

    None
}
