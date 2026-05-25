use sea_query::{Alias, ForeignKey};
use vespertide_core::{ReferenceAction, TableConstraint};

use super::super::helpers::to_sea_fk_action;
use super::super::types::{BuiltQuery, DatabaseBackend};
use super::{QueryError, TableDef, rebuild_sqlite_table_with_added_constraint};

#[expect(
    clippy::too_many_arguments,
    reason = "composite foreign-key builder mirrors FK action fields plus SQLite schema context; ForeignKeyContext is a deferred refactor"
)]
pub(super) fn build_foreign_key<T: AsRef<str>, U: AsRef<str>>(
    backend: DatabaseBackend,
    table: &str,
    name: Option<&str>,
    columns: &[T],
    ref_table: &str,
    ref_columns: &[U],
    on_delete: Option<&ReferenceAction>,
    on_update: Option<&ReferenceAction>,
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
    let fk_name = vespertide_naming::build_foreign_key_name(table, columns, name);
    let mut fk = ForeignKey::create();
    fk.name(&fk_name);
    fk.from_tbl(Alias::new(table));
    for col in columns {
        fk.from_col(Alias::new(col.as_ref()));
    }
    fk.to_tbl(Alias::new(ref_table));
    for col in ref_columns {
        fk.to_col(Alias::new(col.as_ref()));
    }
    if let Some(action) = on_delete {
        fk.on_delete(to_sea_fk_action(action));
    }
    if let Some(action) = on_update {
        fk.on_update(to_sea_fk_action(action));
    }
    Ok(vec![BuiltQuery::CreateForeignKey(Box::new(fk))])
}
