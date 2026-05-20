mod check;
mod foreign_key;
mod index;
mod primary_key;
#[cfg(test)]
mod tests;
mod unique;

use sea_query::{Alias, Query, Table};
use vespertide_core::{TableConstraint, TableDef};

use super::helpers::{build_sqlite_temp_table_create, recreate_indexes_after_rebuild};
use super::rename_table::build_rename_table;
use super::types::{BuiltQuery, DatabaseBackend};
use crate::error::QueryError;

pub fn build_add_constraint(
    backend: DatabaseBackend,
    table: &str,
    constraint: &TableConstraint,
    current_schema: &[TableDef],
    pending_constraints: &[TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    match constraint {
        TableConstraint::PrimaryKey { columns, .. } => primary_key::build_primary_key(
            backend,
            table,
            columns,
            constraint,
            current_schema,
            pending_constraints,
        ),
        TableConstraint::Unique { name, columns } => {
            Ok(unique::build_unique(table, name.as_deref(), columns))
        }
        TableConstraint::ForeignKey {
            name,
            columns,
            ref_table,
            ref_columns,
            on_delete,
            on_update,
        } => foreign_key::build_foreign_key(
            backend,
            table,
            name.as_deref(),
            columns,
            ref_table,
            ref_columns,
            on_delete.as_ref(),
            on_update.as_ref(),
            constraint,
            current_schema,
            pending_constraints,
        ),
        TableConstraint::Index { name, columns } => {
            Ok(index::build_index(table, name.as_deref(), columns))
        }
        TableConstraint::Check { name, expr } => check::build_check(
            backend,
            table,
            name,
            expr,
            constraint,
            current_schema,
            pending_constraints,
        ),
        _ => unreachable!("TableConstraint is #[non_exhaustive]; all variants are matched above"),
    }
}

pub(super) fn merge_constraint(
    existing: &[TableConstraint],
    constraint: &TableConstraint,
) -> Vec<TableConstraint> {
    let mut out = Vec::with_capacity(existing.len() + 1);
    let mut replaced = false;
    for c in existing {
        if constraints_overlap(c, constraint) {
            if !replaced {
                out.push(constraint.clone());
                replaced = true;
            }
        } else {
            out.push(c.clone());
        }
    }
    if !replaced {
        out.push(constraint.clone());
    }
    out
}

pub(super) fn constraints_overlap(a: &TableConstraint, b: &TableConstraint) -> bool {
    match (a, b) {
        (
            TableConstraint::ForeignKey {
                columns: a_cols, ..
            },
            TableConstraint::ForeignKey {
                columns: b_cols, ..
            },
        )
        | (
            TableConstraint::PrimaryKey {
                columns: a_cols, ..
            },
            TableConstraint::PrimaryKey {
                columns: b_cols, ..
            },
        ) => a_cols == b_cols,
        (
            TableConstraint::Check {
                name: a_name,
                expr: a_expr,
            },
            TableConstraint::Check {
                name: b_name,
                expr: b_expr,
            },
        ) => a_name == b_name && a_expr == b_expr,
        _ => false,
    }
}

pub(super) fn rebuild_sqlite_table_with_added_constraint(
    backend: DatabaseBackend,
    table: &str,
    constraint: &TableConstraint,
    current_schema: &[TableDef],
    pending_constraints: &[TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    let table_def = current_schema.iter().find(|t| t.name == table).ok_or_else(|| {
        QueryError::Other(format!(
            "Table '{table}' not found in current schema. SQLite requires current schema information to add constraints."
        ))
    })?;
    let new_constraints = merge_constraint(&table_def.constraints, constraint);
    let temp_table = format!("{table}_temp");
    let create_query = build_sqlite_temp_table_create(
        backend,
        &temp_table,
        table,
        &table_def.columns,
        &new_constraints,
    );
    let column_aliases: Vec<Alias> = table_def
        .columns
        .iter()
        .map(|c| Alias::new(&c.name))
        .collect();
    let mut select_query = Query::select();
    for col_alias in &column_aliases {
        select_query.column(col_alias.clone());
    }
    select_query.from(Alias::new(table));
    let insert_stmt = Query::insert()
        .into_table(Alias::new(&temp_table))
        .columns(column_aliases)
        .select_from(select_query)
        .unwrap()
        .to_owned();
    let insert_query = BuiltQuery::Insert(Box::new(insert_stmt));
    let drop_table = Table::drop().table(Alias::new(table)).to_owned();
    let drop_query = BuiltQuery::DropTable(Box::new(drop_table));
    let rename_query = build_rename_table(&temp_table, table);
    let index_queries =
        recreate_indexes_after_rebuild(table, &table_def.constraints, pending_constraints);
    let mut queries = vec![create_query, insert_query, drop_query, rename_query];
    queries.extend(index_queries);
    Ok(queries)
}
