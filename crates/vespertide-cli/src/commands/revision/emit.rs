use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use anyhow::Result;
use vespertide_core::{MigrationAction, MigrationPlan, TableConstraint, TableDef};

/// Apply `fill_with` values to a migration plan.
pub(super) fn apply_fill_with_to_plan(
    plan: &mut MigrationPlan,
    fill_values: &HashMap<(String, String), String>,
) {
    for action in &mut plan.actions {
        match action {
            MigrationAction::AddColumn {
                table,
                column,
                fill_with,
            } => {
                if fill_with.is_none()
                    && let Some(value) = fill_values.get(&(table.clone(), column.name.clone()))
                {
                    *fill_with = Some(value.clone());
                }
            }
            MigrationAction::ModifyColumnNullable {
                table,
                column,
                fill_with,
                ..
            } => {
                if fill_with.is_none()
                    && let Some(value) = fill_values.get(&(table.clone(), column.clone()))
                {
                    *fill_with = Some(value.clone());
                }
            }
            _ => {}
        }
    }
}

/// Apply `delete_null_rows` flags to matching `ModifyColumnNullable` actions.
pub(super) fn apply_delete_null_rows_to_plan(
    plan: &mut MigrationPlan,
    delete_set: &HashSet<(String, String)>,
) {
    for action in &mut plan.actions {
        if let MigrationAction::ModifyColumnNullable {
            table,
            column,
            nullable,
            delete_null_rows,
            ..
        } = action
            && !*nullable
            && delete_null_rows.is_none()
            && delete_set.contains(&(table.clone(), column.clone()))
        {
            *delete_null_rows = Some(true);
        }
    }
}
/// Apply collected enum `fill_with` mappings to the migration plan.
pub(super) fn apply_enum_fill_with_to_plan(
    plan: &mut MigrationPlan,
    collected: &[(usize, BTreeMap<String, String>)],
) {
    for (action_index, mappings) in collected {
        if let Some(MigrationAction::ModifyColumnType { fill_with, .. }) =
            plan.actions.get_mut(*action_index)
        {
            match fill_with {
                Some(existing) => {
                    existing.extend(mappings.clone());
                }
                None => {
                    *fill_with = Some(mappings.clone());
                }
            }
        }
    }
}
/// Reason why a table needs to be recreated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RecreateReason {
    /// A new non-nullable FK column is being added.
    AddColumnWithFk,
    /// A FK constraint is being added to an existing non-nullable column.
    AddFkToExistingColumn,
}

/// A table that needs to be recreated because of a non-nullable FK constraint issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RecreateTableRequired {
    pub(super) table: String,
    pub(super) column: String,
    pub(super) reason: RecreateReason,
}

/// Find actions that require table recreation due to non-nullable FK constraints.
///
/// Two cases are detected:
/// 1. **`AddColumn` with FK**: A new non-nullable FK column is being added (no default).
/// 2. **AddConstraint(FK) on existing column**: A FK constraint is being added to an
///    existing non-nullable column without a default.
///
/// In both cases, existing rows cannot satisfy the foreign key constraint,
/// so the table must be recreated (`DeleteTable` + `CreateTable`).
pub(super) fn find_non_nullable_fk_add_columns(
    plan: &MigrationPlan,
    current_models: &[TableDef],
) -> Vec<RecreateTableRequired> {
    // Collect FK columns from AddConstraint actions; lookup-only, ordering unused.
    let mut fk_columns: HashSet<(String, String)> = HashSet::new();
    for action in &plan.actions {
        if let MigrationAction::AddConstraint {
            table,
            constraint: TableConstraint::ForeignKey { columns, .. },
        } = action
        {
            for col in columns {
                fk_columns.insert((table.clone(), col.clone()));
            }
        }
    }

    // Collect columns being added in this migration (to distinguish new vs existing); lookup-only, ordering unused.
    let mut added_columns: HashSet<(String, String)> = HashSet::new();
    for action in &plan.actions {
        if let MigrationAction::AddColumn { table, column, .. } = action {
            added_columns.insert((table.clone(), column.name.clone()));
        }
    }

    let mut result = Vec::new();

    // Case 1: AddColumn with FK (new non-nullable FK column)
    for action in &plan.actions {
        if let MigrationAction::AddColumn { table, column, .. } = action {
            let has_fk = column.foreign_key.is_some()
                || fk_columns.contains(&(table.clone(), column.name.clone()));
            if has_fk && !column.nullable && column.default.is_none() {
                result.push(RecreateTableRequired {
                    table: table.clone(),
                    column: column.name.clone(),
                    reason: RecreateReason::AddColumnWithFk,
                });
            }
        }
    }

    // Case 2: AddConstraint(FK) on existing non-nullable column
    for action in &plan.actions {
        if let MigrationAction::AddConstraint {
            table,
            constraint: TableConstraint::ForeignKey { columns, .. },
        } = action
        {
            for col_name in columns {
                // Skip if this column is being added in this migration (handled by Case 1)
                if added_columns.contains(&(table.clone(), col_name.clone())) {
                    continue;
                }
                // Look up column in current models to check nullability
                if let Some(model) = current_models
                    .iter()
                    .find(|m| m.name.as_str() == table.as_str())
                    && let Some(col_def) = model
                        .columns
                        .iter()
                        .find(|c| c.name.as_str() == col_name.as_str())
                    && !col_def.nullable
                    && col_def.default.is_none()
                {
                    result.push(RecreateTableRequired {
                        table: table.clone(),
                        column: col_name.clone(),
                        reason: RecreateReason::AddFkToExistingColumn,
                    });
                }
            }
        }
    }

    result
}

/// Rewrite the migration plan to recreate tables instead of adding columns.
/// Removes all column/constraint actions targeting the recreated tables and replaces
/// them with `DeleteTable` + `CreateTable` using the full target model.
pub(super) fn rewrite_plan_for_recreation(
    plan: &mut MigrationPlan,
    recreate_tables: &[RecreateTableRequired],
    current_models: &[TableDef],
) {
    let tables_to_recreate: BTreeSet<&str> =
        recreate_tables.iter().map(|r| r.table.as_str()).collect();

    // Remove all column/constraint actions targeting recreated tables
    plan.actions.retain(|action| {
        let table = match action {
            MigrationAction::AddColumn { table, .. }
            | MigrationAction::DeleteColumn { table, .. }
            | MigrationAction::RenameColumn { table, .. }
            | MigrationAction::ModifyColumnType { table, .. }
            | MigrationAction::ModifyColumnNullable { table, .. }
            | MigrationAction::ModifyColumnDefault { table, .. }
            | MigrationAction::ModifyColumnComment { table, .. }
            | MigrationAction::AddConstraint { table, .. }
            | MigrationAction::RemoveConstraint { table, .. }
            | MigrationAction::ReplaceConstraint { table, .. } => Some(table.as_str()),
            _ => None,
        };
        table.is_none_or(|t| !tables_to_recreate.contains(t))
    });

    // Add DeleteTable + CreateTable for each recreated table
    for table_name in &tables_to_recreate {
        if let Some(model) = current_models
            .iter()
            .find(|m| m.name.as_str() == *table_name)
        {
            plan.actions.push(MigrationAction::DeleteTable {
                table: table_name.to_string(),
            });
            plan.actions.push(MigrationAction::CreateTable {
                table: model.name.clone(),
                columns: model.columns.clone(),
                constraints: model.constraints.clone(),
            });
        }
    }
}

pub(super) fn handle_recreate_requirements<F>(
    plan: &mut MigrationPlan,
    current_models: &[TableDef],
    prompt_fn: F,
) -> Result<()>
where
    F: Fn(&[RecreateTableRequired]) -> Result<bool>,
{
    let recreate_tables = find_non_nullable_fk_add_columns(plan, current_models);
    if recreate_tables.is_empty() {
        return Ok(());
    }

    if !prompt_fn(&recreate_tables)? {
        anyhow::bail!(
            "Migration cancelled. To proceed without recreation, make the column nullable or add it with a default value that references an existing row."
        );
    }

    rewrite_plan_for_recreation(plan, &recreate_tables, current_models);
    Ok(())
}
