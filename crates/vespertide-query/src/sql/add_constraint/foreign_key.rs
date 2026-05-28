use sea_query::{Alias, ForeignKey};
use vespertide_core::{ForeignKeyOrphanStrategy, ReferenceAction, TableConstraint};

use super::super::helpers::{quote_ident, to_sea_fk_action};
use super::super::types::{BuiltQuery, DatabaseBackend, RawSql};
use super::{QueryError, TableDef, rebuild_sqlite_table_with_added_constraint};

/// Render `ON DELETE <kw>` / `ON UPDATE <kw>` clause text, or empty
/// string when the action is `None`. Used by the F11 PG raw-SQL path.
fn render_fk_action_clause(prefix: &str, action: Option<&ReferenceAction>) -> String {
    action.map_or_else(String::new, |a| format!(" {prefix} {}", a.to_sql_keyword()))
}

/// Build the SQL for `AddConstraint(ForeignKey)` plus the F3 pre-cleanup
/// statement dictated by `orphan_strategy`.
///
/// The cleanup is always emitted; on a table with no orphan rows it is a
/// no-op (zero rows updated/deleted). Skipping it entirely would require
/// proving statically that orphans cannot exist - vespertide treats the
/// planner-side check (`find_fk_orphan_additions`) as advisory, not as a
/// proof of absence.
///
/// SQL pattern (PG/MySQL/SQLite uniform):
///
/// - **`NullifyOrphans`**: `UPDATE child SET <fk_cols> = NULL WHERE (<fk_col_i> IS NOT NULL OR ...) AND NOT EXISTS (SELECT 1 FROM parent WHERE <join>);`
/// - **`DeleteOrphans`**:  `DELETE FROM child WHERE NOT EXISTS (SELECT 1 FROM parent WHERE <join>);`
///
/// The `IS NOT NULL` guard on `NullifyOrphans` preserves rows whose
/// composite FK is wholly NULL (SQL standard treats such rows as
/// FK-exempt). Single-column FK collapses to a single `IS NOT NULL`.
#[expect(
    clippy::too_many_arguments,
    reason = "composite foreign-key builder mirrors FK action fields plus SQLite schema context plus the F3 orphan strategy; ForeignKeyContext is a deferred refactor"
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
    orphan_strategy: ForeignKeyOrphanStrategy,
    constraint: &TableConstraint,
    current_schema: &[TableDef],
    pending_constraints: &[TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    let cleanup =
        build_fk_orphan_cleanup(backend, table, columns, ref_table, ref_columns, orphan_strategy)?;

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

    let fk_name = vespertide_naming::build_foreign_key_name(table, columns, name);
    let mut queries = cleanup;

    if backend == DatabaseBackend::Postgres {
        // F11: PG NOT VALID + VALIDATE 2-step. Both statements run
        // inside the migration transaction so PG rollback reverts the
        // pair on failure - no partial-apply zombie. The sea-query
        // `ForeignKey` builder has no `NOT VALID` switch, so we emit
        // raw SQL here. MySQL still uses the builder below.
        let quoted_table = quote_ident(table, backend);
        let quoted_name = quote_ident(&fk_name, backend);
        let quoted_ref_table = quote_ident(ref_table, backend);
        let cols = columns
            .iter()
            .map(|c| quote_ident(c.as_ref(), backend))
            .collect::<Vec<_>>()
            .join(", ");
        let ref_cols = ref_columns
            .iter()
            .map(|c| quote_ident(c.as_ref(), backend))
            .collect::<Vec<_>>()
            .join(", ");
        let on_delete_clause = render_fk_action_clause("ON DELETE", on_delete);
        let on_update_clause = render_fk_action_clause("ON UPDATE", on_update);

        let add_not_valid = format!(
            "ALTER TABLE {quoted_table} ADD CONSTRAINT {quoted_name} \
             FOREIGN KEY ({cols}) REFERENCES {quoted_ref_table} ({ref_cols})\
             {on_delete_clause}{on_update_clause} NOT VALID"
        );
        let validate = format!("ALTER TABLE {quoted_table} VALIDATE CONSTRAINT {quoted_name}");
        queries.push(BuiltQuery::Raw(RawSql::uniform(add_not_valid)));
        queries.push(BuiltQuery::Raw(RawSql::uniform(validate)));
        return Ok(queries);
    }

    // MySQL: single statement via sea-query builder (no NOT VALID).
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

    queries.push(BuiltQuery::CreateForeignKey(Box::new(fk)));
    Ok(queries)
}

/// Emit the F3 pre-cleanup statement (UPDATE / DELETE) ahead of the
/// `ADD CONSTRAINT FOREIGN KEY`. Returns an empty `Vec` only when the
/// strategy variant is unrecognised (future-proofing for `non_exhaustive`).
fn build_fk_orphan_cleanup<T: AsRef<str>, U: AsRef<str>>(
    backend: DatabaseBackend,
    child_table: &str,
    child_columns: &[T],
    ref_table: &str,
    ref_columns: &[U],
    strategy: ForeignKeyOrphanStrategy,
) -> Result<Vec<BuiltQuery>, QueryError> {
    if child_columns.len() != ref_columns.len() {
        return Err(QueryError::SchemaError(format!(
            "FK on '{child_table}': child columns ({}) and ref columns ({}) length mismatch",
            child_columns.len(),
            ref_columns.len()
        )));
    }
    if child_columns.is_empty() {
        return Ok(vec![]);
    }

    let quoted_child = quote_ident(child_table, backend);
    let quoted_ref = quote_ident(ref_table, backend);

    // Correlated `NOT EXISTS` join condition: `parent.pk_i = child.fk_i AND ...`
    let join_cond: Vec<String> = child_columns
        .iter()
        .zip(ref_columns.iter())
        .map(|(c, r)| {
            let qc = quote_ident(c.as_ref(), backend);
            let qr = quote_ident(r.as_ref(), backend);
            format!("{quoted_ref}.{qr} = {quoted_child}.{qc}")
        })
        .collect();
    let join_cond = join_cond.join(" AND ");

    let not_exists =
        format!("NOT EXISTS (SELECT 1 FROM {quoted_ref} WHERE {join_cond})");

    let sql = match &strategy {
        ForeignKeyOrphanStrategy::NullifyOrphans => {
            // SET <col_i> = NULL, ...
            let set_clause: Vec<String> = child_columns
                .iter()
                .map(|c| {
                    let qc = quote_ident(c.as_ref(), backend);
                    format!("{qc} = NULL")
                })
                .collect();
            let set_clause = set_clause.join(", ");

            // NULL row guard: skip rows whose FK columns are all NULL.
            // Composite FK uses `<col_i> IS NOT NULL OR ...`; single-column FK collapses to one term.
            let null_guard: Vec<String> = child_columns
                .iter()
                .map(|c| format!("{} IS NOT NULL", quote_ident(c.as_ref(), backend)))
                .collect();
            let null_guard = null_guard.join(" OR ");

            format!(
                "UPDATE {quoted_child} SET {set_clause} WHERE ({null_guard}) AND {not_exists}"
            )
        }
        ForeignKeyOrphanStrategy::DeleteOrphans => {
            format!("DELETE FROM {quoted_child} WHERE {not_exists}")
        }
        _ => {
            return Err(QueryError::UnsupportedAction(format!(
                "AddConstraint(ForeignKey) on '{child_table}': unsupported orphan_strategy variant"
            )));
        }
    };

    Ok(vec![BuiltQuery::Raw(RawSql::uniform(sql))])
}
