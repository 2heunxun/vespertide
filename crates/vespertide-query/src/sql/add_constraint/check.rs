use vespertide_core::TableConstraint;

use super::super::helpers::quote_ident;
use super::super::types::{BuiltQuery, DatabaseBackend, RawSql};
use super::{QueryError, TableDef, rebuild_sqlite_table_with_added_constraint};

pub(super) fn build_check(
    backend: DatabaseBackend,
    table: &str,
    name: &str,
    expr: &str,
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
    Ok(vec![BuiltQuery::Raw(RawSql::per_backend(
        pg_sql.clone(),
        mysql_sql,
        pg_sql,
    ))])
}
