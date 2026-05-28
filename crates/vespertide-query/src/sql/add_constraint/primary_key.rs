use vespertide_core::{KeepPolicy, PrimaryKeyAdditionStrategy, TableConstraint};

use super::super::helpers::{quote_ident, quote_idents};
use super::super::types::{BuiltQuery, DatabaseBackend, RawSql};
use super::{QueryError, TableDef, rebuild_sqlite_table_with_added_constraint};

/// Build SQL for adding a PRIMARY KEY, plus the F5 pre-cleanup
/// statement dictated by `strategy`.
///
/// Cleanup SQL is uniform across PG/MySQL/SQLite:
///
/// - **`DeleteDuplicates { keep: First }`**: `DELETE FROM t WHERE
///   <old_pk> NOT IN (SELECT MIN(<old_pk>) FROM t GROUP BY <new_pk>)`
/// - **`DeleteDuplicates { keep: Last }`**: `MAX(<old_pk>)` variant.
///
/// When the table lacks a single-column baseline PK (no PK, composite
/// PK, or the only PK column appears inside the new PK set), cleanup
/// is skipped silently. Mirrors F2's `try_resolve_single_pk_column`
/// fallback policy.
pub(super) fn build_primary_key<T: AsRef<str>>(
    backend: DatabaseBackend,
    table: &str,
    columns: &[T],
    strategy: &PrimaryKeyAdditionStrategy,
    constraint: &TableConstraint,
    current_schema: &[TableDef],
    pending_constraints: &[TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    let cleanup = build_pk_pre_cleanup(backend, table, columns, strategy, current_schema);

    if backend == DatabaseBackend::Sqlite {
        let mut queries = cleanup;
        queries.extend(rebuild_sqlite_table_with_added_constraint(
            backend,
            table,
            constraint,
            current_schema,
            pending_constraints,
        )?);
        return Ok(queries);
    }

    let pg_cols = quote_idents(columns, DatabaseBackend::Postgres);
    let mysql_cols = quote_idents(columns, DatabaseBackend::MySql);
    let pg_table = quote_ident(table, DatabaseBackend::Postgres);
    let mysql_table = quote_ident(table, DatabaseBackend::MySql);
    let pg_sql = format!("ALTER TABLE {pg_table} ADD PRIMARY KEY ({pg_cols})");
    let mysql_sql = format!("ALTER TABLE {mysql_table} ADD PRIMARY KEY ({mysql_cols})");

    let mut queries = cleanup;
    queries.push(BuiltQuery::Raw(RawSql::per_backend(
        pg_sql.clone(),
        mysql_sql,
        pg_sql,
    )));
    Ok(queries)
}

fn build_pk_pre_cleanup<T: AsRef<str>>(
    backend: DatabaseBackend,
    table: &str,
    new_pk_columns: &[T],
    strategy: &PrimaryKeyAdditionStrategy,
    current_schema: &[TableDef],
) -> Vec<BuiltQuery> {
    let keep = match strategy {
        PrimaryKeyAdditionStrategy::DeleteDuplicates { keep } => *keep,
        _ => return vec![],
    };
    let Some(old_pk_column) =
        try_resolve_single_pk_column(table, current_schema, new_pk_columns)
    else {
        return vec![];
    };
    let agg = match keep {
        KeepPolicy::First => "MIN",
        KeepPolicy::Last => "MAX",
    };
    let quoted_table = quote_ident(table, backend);
    let quoted_old_pk = quote_ident(&old_pk_column, backend);
    let group_by = new_pk_columns
        .iter()
        .map(|c| quote_ident(c.as_ref(), backend))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "DELETE FROM {quoted_table} WHERE {quoted_old_pk} NOT IN (\
         SELECT {agg}({quoted_old_pk}) FROM {quoted_table} GROUP BY {group_by})"
    );
    vec![BuiltQuery::Raw(RawSql::uniform(sql))]
}

fn try_resolve_single_pk_column<T: AsRef<str>>(
    table: &str,
    current_schema: &[TableDef],
    new_pk_columns: &[T],
) -> Option<String> {
    let table_def = current_schema.iter().find(|t| t.name.as_str() == table)?;

    let pk_columns: Vec<String> = table_def
        .constraints
        .iter()
        .find_map(|c| {
            if let TableConstraint::PrimaryKey { columns, .. } = c {
                Some(columns.iter().map(ToString::to_string).collect())
            } else {
                None
            }
        })
        .or_else(|| {
            let inline: Vec<String> = table_def
                .columns
                .iter()
                .filter(|col| col.primary_key.is_some())
                .map(|col| col.name.to_string())
                .collect();
            if inline.is_empty() { None } else { Some(inline) }
        })?;

    if pk_columns.len() != 1 {
        return None;
    }
    let pk_column = pk_columns.into_iter().next().expect("len == 1");
    let new_set: Vec<&str> = new_pk_columns.iter().map(AsRef::as_ref).collect();
    if new_set.iter().any(|c| *c == pk_column) {
        return None;
    }
    Some(pk_column)
}
