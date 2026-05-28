use vespertide_core::{CheckViolationStrategy, TableConstraint};

use super::super::helpers::quote_ident;
use super::super::types::{BuiltQuery, DatabaseBackend, RawSql};
use super::{QueryError, TableDef, rebuild_sqlite_table_with_added_constraint};

/// Build SQL for adding a CHECK constraint, plus the F4 pre-cleanup
/// statement dictated by `strategy`.
///
/// The cleanup is always emitted (on a table with no violating rows it
/// is a no-op). SQL pattern (PG/MySQL/SQLite uniform):
///
/// - **`NullifyViolatingColumn { column }`**: `UPDATE table SET <column> = NULL WHERE NOT (<expr>);`
/// - **`DeleteViolatingRows`**:               `DELETE FROM table WHERE NOT (<expr>);`
///
/// SQL standard treats `NULL` in the predicate as *not TRUE* (the
/// `WHERE NOT (<expr>)` clause naturally excludes rows whose column is
/// already NULL), so no explicit `IS NOT NULL` guard is needed - rows
/// that already conform to the new constraint via NULL are left
/// untouched on both `NullifyViolatingColumn` and `DeleteViolatingRows`.
#[expect(
    clippy::too_many_arguments,
    reason = "CHECK builder mirrors action fields plus SQLite schema context plus the F4 cleanup strategy; ConstraintContext is a deferred refactor"
)]
pub(super) fn build_check(
    backend: DatabaseBackend,
    table: &str,
    name: &str,
    expr: &str,
    strategy: &CheckViolationStrategy,
    constraint: &TableConstraint,
    current_schema: &[TableDef],
    pending_constraints: &[TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    let cleanup = build_check_violation_cleanup(backend, table, expr, strategy)?;

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

    let pg_sql = format!(
        "ALTER TABLE {} ADD CONSTRAINT {} CHECK ({expr})",
        quote_ident(table, DatabaseBackend::Postgres),
        quote_ident(name, DatabaseBackend::Postgres)
    );
    let mysql_sql = format!(
        "ALTER TABLE {} ADD CONSTRAINT {} CHECK ({expr})",
        quote_ident(table, DatabaseBackend::MySql),
        quote_ident(name, DatabaseBackend::MySql)
    );

    let mut queries = cleanup;
    queries.push(BuiltQuery::Raw(RawSql::per_backend(
        pg_sql.clone(),
        mysql_sql,
        pg_sql,
    )));
    Ok(queries)
}

/// Emit the F4 pre-cleanup statement (UPDATE / DELETE) ahead of the
/// `ADD CONSTRAINT CHECK`. Returns an empty `Vec` only when the
/// strategy variant is unrecognised (future-proofing for
/// `non_exhaustive`).
fn build_check_violation_cleanup(
    backend: DatabaseBackend,
    table: &str,
    expr: &str,
    strategy: &CheckViolationStrategy,
) -> Result<Vec<BuiltQuery>, QueryError> {
    let quoted_table = quote_ident(table, backend);

    let sql = match strategy {
        CheckViolationStrategy::NullifyViolatingColumn { column } => {
            let quoted_col = quote_ident(column.as_str(), backend);
            format!("UPDATE {quoted_table} SET {quoted_col} = NULL WHERE NOT ({expr})")
        }
        CheckViolationStrategy::DeleteViolatingRows => {
            format!("DELETE FROM {quoted_table} WHERE NOT ({expr})")
        }
        _ => {
            return Err(QueryError::UnsupportedAction(format!(
                "AddConstraint(Check) on '{table}': unsupported strategy variant"
            )));
        }
    };

    Ok(vec![BuiltQuery::Raw(RawSql::uniform(sql))])
}
