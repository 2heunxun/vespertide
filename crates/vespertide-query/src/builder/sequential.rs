use vespertide_core::{MigrationAction, MigrationPlan, TableConstraint, TableDef};
use vespertide_planner::apply_action;

use super::{PlanQueries, action_target_table};
use crate::DatabaseBackend;
use crate::error::QueryError;
use crate::sql::build_action_queries_with_pending;

pub(super) fn build_plan_queries_sequentially(
    plan: &MigrationPlan,
    current_schema: &[TableDef],
) -> Result<Vec<PlanQueries>, QueryError> {
    let mut queries: Vec<PlanQueries> = Vec::new();
    let mut evolving_schema = current_schema.to_vec();

    for (i, action) in plan.actions.iter().enumerate() {
        // For SQLite rebuilds, avoid recreating pending indexes that later actions create.
        let action_table = action_target_table(action);
        let pending_constraints: Vec<TableConstraint> = if let Some(table) = action_table {
            plan.actions[i + 1..]
                .iter()
                .filter_map(|a| {
                    if let MigrationAction::AddConstraint {
                        table: t,
                        constraint,
                    } = a
                    {
                        if t == table
                            && matches!(
                                constraint,
                                TableConstraint::Index { .. } | TableConstraint::Unique { .. }
                            )
                        {
                            Some(constraint.clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            vec![]
        };

        let postgres_queries = build_action_queries_with_pending(
            DatabaseBackend::Postgres,
            action,
            &evolving_schema,
            &pending_constraints,
        )?;
        let mysql_queries = build_action_queries_with_pending(
            DatabaseBackend::MySql,
            action,
            &evolving_schema,
            &pending_constraints,
        )?;
        let sqlite_queries = build_action_queries_with_pending(
            DatabaseBackend::Sqlite,
            action,
            &evolving_schema,
            &pending_constraints,
        )?;
        queries.push(PlanQueries {
            action: action.clone(),
            postgres: postgres_queries,
            mysql: mysql_queries,
            sqlite: sqlite_queries,
        });

        // Apply the action to update the schema for the next iteration
        // Note: We ignore errors here because some actions (like DeleteTable) may reference
        // tables that don't exist in the provided current_schema. This is OK for SQL generation
        // purposes - we still generate the correct SQL, and the schema evolution is best-effort.
        let _ = apply_action(&mut evolving_schema, action);
    }
    Ok(queries)
}
