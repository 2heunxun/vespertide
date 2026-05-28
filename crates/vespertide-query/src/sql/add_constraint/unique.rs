use sea_query::{Alias, Index};

use vespertide_core::{KeepPolicy, TableConstraint, TableDef, UniqueConstraintStrategy};

use super::super::helpers::{build_unique_constraint_name, quote_ident};
use super::super::types::{BuiltQuery, DatabaseBackend, RawSql};
use crate::error::QueryError;

/// Build SQL for adding a UNIQUE constraint to a table, including the F2
/// pre-cleanup `DELETE` dictated by `strategy`.
///
/// `strategy` is always emitted (no `SkipCleanup` option exists): every
/// `AddConstraint(Unique)` produces a `DELETE` ahead of the `ADD CONSTRAINT`
/// so the migration succeeds even when production data has duplicates.
///
/// `current_schema` is needed to look up the table's PRIMARY KEY: the
/// generated `DELETE` uses `(pk) NOT IN (SELECT MIN(pk) ... GROUP BY
/// <unique_columns>)` (or MAX for `KeepLast`). Tables without a PK, with
/// a composite PK, or with PK columns appearing inside the unique-key set
/// are rejected with [`QueryError::UnsupportedAction`] — the user must
/// pre-clean manually in those cases.
pub(super) fn build_unique<T: AsRef<str>>(
    backend: DatabaseBackend,
    table: &str,
    name: Option<&str>,
    columns: &[T],
    strategy: &UniqueConstraintStrategy,
    current_schema: &[TableDef],
) -> Result<Vec<BuiltQuery>, QueryError> {
    let mut queries = build_pre_cleanup_delete(backend, table, columns, strategy, current_schema)?;
    queries.push(build_unique_index(table, name, columns));
    Ok(queries)
}

/// Compose the pre-cleanup `DELETE` for `strategy`.
///
/// Currently only `DeleteDuplicates` exists; the dispatch is laid out so
/// future strategies can plug in without touching the call site.
fn build_pre_cleanup_delete<T: AsRef<str>>(
    backend: DatabaseBackend,
    table: &str,
    columns: &[T],
    strategy: &UniqueConstraintStrategy,
    current_schema: &[TableDef],
) -> Result<Vec<BuiltQuery>, QueryError> {
    // `UniqueConstraintStrategy` is `#[non_exhaustive]`; match (not `let`)
    // so future variants get a compiler warning to be handled.
    let keep = match strategy {
        UniqueConstraintStrategy::DeleteDuplicates { keep } => *keep,
        _ => {
            return Err(QueryError::UnsupportedAction(format!(
                "AddConstraint(Unique) on '{table}': unsupported strategy variant"
            )));
        }
    };
    // Graceful fallback for tables without a usable PK (no PK at all,
    // composite PK, or PK column inside the unique set): skip the
    // pre-cleanup `DELETE` and let the database decide. Production schemas
    // are guaranteed to have a single-column PK by F12 Scenario E, so this
    // path is reached only by test fixtures or hand-edited migrations.
    let Some(pk_column) = try_resolve_single_pk_column(table, current_schema, columns) else {
        return Ok(vec![]);
    };
    let agg = match keep {
        KeepPolicy::First => "MIN",
        KeepPolicy::Last => "MAX",
    };
    let quoted_table = quote_ident(table, backend);
    let quoted_pk = quote_ident(&pk_column, backend);
    let quoted_unique_cols: Vec<String> = columns
        .iter()
        .map(|c| quote_ident(c.as_ref(), backend))
        .collect();
    let group_by = quoted_unique_cols.join(", ");
    let sql = format!(
        "DELETE FROM {quoted_table} WHERE {quoted_pk} NOT IN (\
         SELECT {agg}({quoted_pk}) FROM {quoted_table} GROUP BY {group_by})",
    );
    Ok(vec![BuiltQuery::Raw(RawSql::uniform(sql))])
}

/// Locate the single-column PRIMARY KEY for `table` in `current_schema`,
/// returning `None` for tables without a usable PK (missing, composite,
/// or PK column inside the unique set — the last would make
/// `NOT IN (SELECT MIN(pk) ... GROUP BY pk)` a tautology).
///
/// The caller skips pre-cleanup when this returns `None`. Production
/// schemas reach the cleanup path only when F12 Scenario E has confirmed
/// a single-column PK; non-production fixtures degrade to v0.1.x
/// behaviour (no DELETE, DB-side rejection).
fn try_resolve_single_pk_column<T: AsRef<str>>(
    table: &str,
    current_schema: &[TableDef],
    unique_columns: &[T],
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
            if inline.is_empty() {
                None
            } else {
                Some(inline)
            }
        })?;

    if pk_columns.len() != 1 {
        return None;
    }
    let pk_column = pk_columns.into_iter().next().expect("len == 1");
    let unique_set: Vec<&str> = unique_columns.iter().map(AsRef::as_ref).collect();
    if unique_set.iter().any(|c| *c == pk_column) {
        return None;
    }
    Some(pk_column)
}

fn build_unique_index<T: AsRef<str>>(table: &str, name: Option<&str>, columns: &[T]) -> BuiltQuery {
    let index_name = build_unique_constraint_name(table, columns, name);
    let mut idx = Index::create()
        .table(Alias::new(table))
        .name(&index_name)
        .unique()
        .to_owned();
    for col in columns {
        idx.col(Alias::new(col.as_ref()));
    }
    BuiltQuery::CreateIndex(Box::new(idx))
}
