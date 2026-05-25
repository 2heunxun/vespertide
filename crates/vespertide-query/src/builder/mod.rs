use vespertide_core::{MigrationAction, MigrationPlan, TableDef};

use crate::DatabaseBackend;
use crate::error::QueryError;
use crate::parallel_config::plan_query_par_action_threshold;
use crate::sql::BuiltQuery;

mod parallel;
mod sequential;
mod transaction;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlanQueriesOptions {
    /// Wrap the generated statement stream in a plan-level transaction.
    pub wrap_in_transaction: bool,
}

#[derive(Debug)]
pub struct PlanQueries {
    pub action: MigrationAction,
    pub postgres: Vec<BuiltQuery>,
    pub mysql: Vec<BuiltQuery>,
    pub sqlite: Vec<BuiltQuery>,
}

impl PlanQueries {
    /// Wrap each backend's full plan statement stream with transaction boundaries.
    ///
    /// `SQLite`'s no-DROP-CONSTRAINT temp-table rebuild pattern is canonical; wrapping
    /// the ordered create-temp/insert/drop/rename/reindex sequence in a transaction
    /// protects it from mid-sequence failures. `MySQL` accepts `BEGIN`/`COMMIT`, but
    /// most DDL implicitly commits and is not transactional on that backend.
    #[must_use]
    pub fn into_transactional(mut queries: Vec<Self>) -> Vec<Self> {
        transaction::wrap_backend_queries(&mut queries, DatabaseBackend::Postgres);
        transaction::wrap_backend_queries(&mut queries, DatabaseBackend::MySql);
        transaction::wrap_backend_queries(&mut queries, DatabaseBackend::Sqlite);
        queries
    }
}

/// Extract the target table name from any migration action.
/// Returns `None` for `RawSql` (no table) and `RenameTable` (ambiguous).
fn action_target_table(action: &MigrationAction) -> Option<&str> {
    match action {
        MigrationAction::RenameTable { .. } | MigrationAction::RawSql { .. } => None,
        _ => action.table_name(),
    }
}

/// Build SQL queries for a full migration plan with sequential schema evolution.
///
/// Each action is built against the schema state AFTER previous actions have been
/// applied; this is required for `SQLite` temp-table rebuilds that need the
/// current column list.
///
/// # Errors
/// Returns [`QueryError`] if any action fails to compile to SQL.
pub fn build_plan_queries(
    plan: &MigrationPlan,
    current_schema: &[TableDef],
) -> Result<Vec<PlanQueries>, QueryError> {
    if plan.actions.len() < plan_query_par_action_threshold() {
        return sequential::build_plan_queries_sequentially(plan, current_schema);
    }

    parallel::build_plan_queries_in_parallel(plan, current_schema)
}

/// Build SQL queries with explicit options (e.g., transaction wrapping).
///
/// See [`PlanQueriesOptions`] for available knobs.
pub fn build_plan_queries_with_options(
    plan: &MigrationPlan,
    current_schema: &[TableDef],
    options: PlanQueriesOptions,
) -> Result<Vec<PlanQueries>, QueryError> {
    let queries = build_plan_queries(plan, current_schema)?;
    if options.wrap_in_transaction {
        Ok(PlanQueries::into_transactional(queries))
    } else {
        Ok(queries)
    }
}
