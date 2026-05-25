use vespertide_core::TableConstraint;

use super::super::helpers::{quote_ident, quote_idents};
use super::super::types::{BuiltQuery, DatabaseBackend, RawSql};
use super::{QueryError, TableDef, rebuild_sqlite_table_with_added_constraint};

pub(super) fn build_primary_key<T: AsRef<str>>(
    backend: DatabaseBackend,
    table: &str,
    columns: &[T],
    constraint: &TableConstraint,
    current_schema: &[TableDef],
    pending_constraints: &[TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    if backend == DatabaseBackend::Sqlite {
        return rebuild_sqlite_table_with_added_constraint(
            backend,
            table,
            constraint,
            current_schema,
            pending_constraints,
        );
    }
    let pg_cols = quote_idents(columns, DatabaseBackend::Postgres);
    let mysql_cols = quote_idents(columns, DatabaseBackend::MySql);
    let pg_table = quote_ident(table, DatabaseBackend::Postgres);
    let mysql_table = quote_ident(table, DatabaseBackend::MySql);
    let pg_sql = format!("ALTER TABLE {pg_table} ADD PRIMARY KEY ({pg_cols})");
    let mysql_sql = format!("ALTER TABLE {mysql_table} ADD PRIMARY KEY ({mysql_cols})");
    Ok(vec![BuiltQuery::Raw(RawSql::per_backend(
        pg_sql.clone(),
        mysql_sql,
        pg_sql,
    ))])
}
