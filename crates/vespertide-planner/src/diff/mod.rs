use std::collections::BTreeMap;

use vespertide_core::{MigrationAction, MigrationPlan, TableDef};

use crate::error::PlannerError;

mod columns;
mod constraints;
mod ordering;
mod tables;

#[cfg(test)]
mod tests;

/// Diff two schema snapshots into a migration plan.
/// Schemas are normalized for comparison purposes, but the original (non-normalized)
/// tables are used in migration actions to preserve inline constraint definitions.
pub fn diff_schemas(from: &[TableDef], to: &[TableDef]) -> Result<MigrationPlan, PlannerError> {
    for table in from.iter().chain(to) {
        table
            .validate_unique_column_names()
            .map_err(|e| PlannerError::TableValidation(e.to_string()))?;
    }

    let mut actions: Vec<MigrationAction> = Vec::new();

    let from_normalized = tables::normalize_schema(from)?;
    let to_normalized = tables::normalize_schema(to)?;

    // Use BTreeMap for consistent ordering
    // Normalized versions for comparison
    let from_map: BTreeMap<_, _> = from_normalized
        .iter()
        .map(|t| (t.name.as_str(), t))
        .collect();
    let to_map: BTreeMap<_, _> = to_normalized.iter().map(|t| (t.name.as_str(), t)).collect();

    // Original (non-normalized) versions for migration storage
    let to_original_map: BTreeMap<_, _> = to.iter().map(|t| (t.name.as_str(), t)).collect();

    tables::diff_deleted_tables(&mut actions, &from_map, &to_map);

    // Update existing tables and their indexes/columns.
    for (name, to_tbl) in &to_map {
        if let Some(from_tbl) = from_map.get(name) {
            let deleted_columns = columns::diff_columns(&mut actions, name, from_tbl, to_tbl);
            constraints::diff_constraints(&mut actions, name, from_tbl, to_tbl, &deleted_columns);
        }
    }

    tables::diff_created_tables(&mut actions, &from_map, &to_map, &to_original_map)?;

    // Sort DeleteTable actions so tables with FK dependencies are deleted first
    ordering::sort_delete_tables(&mut actions, &from_map);

    // Sort so CreateTable comes before AddConstraint that references the new table
    ordering::sort_create_before_add_constraint(&mut actions);

    // Sort so ModifyColumnDefault comes before ModifyColumnType when removing enum values
    // that were used as the default
    ordering::sort_enum_default_dependencies(&mut actions, &from_map);

    Ok(MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 0,
        actions,
    })
}
