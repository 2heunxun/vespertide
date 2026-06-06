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

#[cfg(test)]
#[expect(
    unsafe_code,
    reason = "tests that drive the parallel builder path must set/unset VESPERTIDE_PLAN_QUERY_PAR_THRESHOLD via std::env::{set_var, remove_var}; serialized via #[serial] to avoid cross-test races"
)]
mod tests {
    use super::*;
    use crate::builder::build_plan_queries;
    use crate::sql::types::DatabaseBackend as _Backend;
    use serial_test::serial;
    use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType};

    fn nn_col(name: &str, ty: SimpleColumnType) -> ColumnDef {
        ColumnDef::new(name, ColumnType::Simple(ty), false)
    }

    /// The parallel `build_plan_queries_in_parallel` path activates when the
    /// plan size meets `VESPERTIDE_PLAN_QUERY_PAR_THRESHOLD`. The env override is
    /// process-wide so this test is `#[serial]` to avoid colliding with other
    /// builder tests.
    #[test]
    #[serial]
    fn build_plan_queries_uses_parallel_path_under_threshold_override() {
        // SAFETY: serialized via #[serial]; this is a per-process env var that
        // the parallel_config helper re-reads on every call.
        unsafe {
            std::env::set_var("VESPERTIDE_PLAN_QUERY_PAR_THRESHOLD", "1");
        }

        let plan = MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![
                MigrationAction::CreateTable {
                    table: "a".into(),
                    columns: vec![nn_col("id", SimpleColumnType::Integer)],
                    constraints: vec![],
                },
                MigrationAction::CreateTable {
                    table: "b".into(),
                    columns: vec![nn_col("id", SimpleColumnType::Integer)],
                    constraints: vec![],
                },
                MigrationAction::AddConstraint {
                    table: "a".into(),
                    constraint: TableConstraint::Index {
                        name: None,
                        columns: vec!["id".into()],
                    },
                },
            ],
        };
        let result = build_plan_queries(&plan, &[]).unwrap();
        assert_eq!(result.len(), 3);

        // SAFETY: same as above - serialized via #[serial].
        unsafe {
            std::env::remove_var("VESPERTIDE_PLAN_QUERY_PAR_THRESHOLD");
        }
    }

    /// The parallel preparer must skip pending-constraint collection for actions
    /// without a target table (RawSql / RenameTable). Drives the
    /// `action_target_table -> None` branch inside `pending_constraints_for_action`.
    #[test]
    #[serial]
    fn build_plan_queries_parallel_path_skips_no_target_actions() {
        // SAFETY: serialized via #[serial].
        unsafe {
            std::env::set_var("VESPERTIDE_PLAN_QUERY_PAR_THRESHOLD", "1");
        }

        let plan = MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![
                MigrationAction::RawSql {
                    sql: "SELECT 1;".into(),
                },
                MigrationAction::RenameTable {
                    from: "a".into(),
                    to: "b".into(),
                },
            ],
        };
        let result = build_plan_queries(&plan, &[]).unwrap();
        assert_eq!(result.len(), 2);

        // SAFETY: same as above.
        unsafe {
            std::env::remove_var("VESPERTIDE_PLAN_QUERY_PAR_THRESHOLD");
        }

        // touch the parallel-imported alias to keep the `use` warning-free
        let _ = _Backend::Postgres;
    }
}
