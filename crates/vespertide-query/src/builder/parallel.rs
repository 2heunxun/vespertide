use rayon::prelude::*;
use vespertide_core::{MigrationAction, MigrationPlan, TableConstraint, TableDef};
use vespertide_planner::apply_action;

use super::{PlanQueries, action_target_table};
use crate::DatabaseBackend;
use crate::error::QueryError;
use crate::parallel_config::PLAN_QUERY_PAR_ACTION_MIN_LEN;
use crate::sql::build_action_queries_with_pending;

struct PreparedAction {
    action: MigrationAction,
    schema: Vec<TableDef>,
    pending_constraints: Vec<TableConstraint>,
}

pub(super) fn build_plan_queries_in_parallel(
    plan: &MigrationPlan,
    current_schema: &[TableDef],
) -> Result<Vec<PlanQueries>, QueryError> {
    let prepared_actions = prepare_actions(plan, current_schema);

    prepared_actions
        .par_iter()
        .with_min_len(PLAN_QUERY_PAR_ACTION_MIN_LEN)
        .map(build_prepared_action_queries)
        .collect()
}

fn prepare_actions(plan: &MigrationPlan, current_schema: &[TableDef]) -> Vec<PreparedAction> {
    let mut prepared_actions = Vec::with_capacity(plan.actions.len());
    let mut evolving_schema = current_schema.to_vec();

    for (i, action) in plan.actions.iter().enumerate() {
        prepared_actions.push(PreparedAction {
            action: action.clone(),
            schema: evolving_schema.clone(),
            pending_constraints: pending_constraints_for_action(plan, i, action),
        });

        let _ = apply_action(&mut evolving_schema, action);
    }

    prepared_actions
}

fn pending_constraints_for_action(
    plan: &MigrationPlan,
    action_index: usize,
    action: &MigrationAction,
) -> Vec<TableConstraint> {
    let Some(table) = action_target_table(action) else {
        return vec![];
    };

    plan.actions[action_index + 1..]
        .iter()
        .filter_map(|a| {
            if let MigrationAction::AddConstraint {
                table: t,
                constraint,
            } = a
                && t == table
                && matches!(
                    constraint,
                    TableConstraint::Index { .. } | TableConstraint::Unique { .. }
                )
            {
                Some(constraint.clone())
            } else {
                None
            }
        })
        .collect()
}

fn build_prepared_action_queries(prepared: &PreparedAction) -> Result<PlanQueries, QueryError> {
    let postgres_queries = build_action_queries_with_pending(
        DatabaseBackend::Postgres,
        &prepared.action,
        &prepared.schema,
        &prepared.pending_constraints,
    )?;
    let mysql_queries = build_action_queries_with_pending(
        DatabaseBackend::MySql,
        &prepared.action,
        &prepared.schema,
        &prepared.pending_constraints,
    )?;
    let sqlite_queries = build_action_queries_with_pending(
        DatabaseBackend::Sqlite,
        &prepared.action,
        &prepared.schema,
        &prepared.pending_constraints,
    )?;

    Ok(PlanQueries {
        action: prepared.action.clone(),
        postgres: postgres_queries,
        mysql: mysql_queries,
        sqlite: sqlite_queries,
    })
}
