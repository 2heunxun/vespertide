use vespertide_core::{
    ColumnType, ComplexColumnType, EnumValues, MigrationAction, MigrationPlan, TableConstraint,
    TableDef,
};

use super::enums::validate_enum_value;
use crate::error::PlannerError;

/// Validate a migration plan for correctness.
/// Checks for:
/// - `AddColumn` actions with NOT NULL columns without default must have `fill_with`
/// - `ModifyColumnNullable` actions changing from nullable to non-nullable must have `fill_with`
/// - Enum columns with `default/fill_with` values must have valid enum values
pub fn validate_migration_plan(plan: &MigrationPlan) -> Result<(), PlannerError> {
    for action in &plan.actions {
        match action {
            MigrationAction::AddColumn {
                table,
                column,
                fill_with,
            } => {
                // If column is NOT NULL and has no default, fill_with is required
                if !column.nullable && column.default.is_none() && fill_with.is_none() {
                    return Err(PlannerError::MissingFillWith(
                        table.clone(),
                        column.name.clone(),
                    ));
                }

                // Validate enum default/fill_with values
                if let ColumnType::Complex(ComplexColumnType::Enum { name, values }) =
                    &column.r#type
                {
                    if let Some(fill) = fill_with {
                        validate_enum_value(fill, name, values, table, &column.name, "fill_with")?;
                    }
                    if let Some(default) = &column.default {
                        let default_str = default.to_sql();
                        validate_enum_value(
                            &default_str,
                            name,
                            values,
                            table,
                            &column.name,
                            "default",
                        )?;
                    }
                }
            }
            MigrationAction::ModifyColumnNullable {
                table,
                column,
                nullable,
                fill_with,
                delete_null_rows,
            }
                // If changing from nullable to non-nullable, fill_with is required
                if !nullable && fill_with.is_none() && !delete_null_rows.unwrap_or(false) => {
                    return Err(PlannerError::MissingFillWith(table.clone(), column.clone()));
                }
            MigrationAction::ModifyColumnType {
                table,
                column,
                new_type,
                fill_with,
            } => {
                // Validate that fill_with replacement values are valid enum values in the NEW type
                if let (
                    Some(fw),
                    ColumnType::Complex(ComplexColumnType::Enum { name, values, .. }),
                ) = (fill_with, new_type)
                {
                    for replacement in fw.values() {
                        validate_enum_value(replacement, name, values, table, column, "fill_with")?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Describes an action whose `fill_with` is required but missing.
/// Returned by [`find_missing_fill_with`] so callers can prompt the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FillWithRequired {
    /// Index of the action in the migration plan.
    pub action_index: usize,
    /// Table name.
    pub table: String,
    /// Column name.
    pub column: String,
    /// Type of action: "`AddColumn`" or "`ModifyColumnNullable`".
    pub action_type: &'static str,
    /// Column type (for display purposes).
    pub column_type: String,
    /// Default fill value hint for this column type.
    pub default_value: String,
    /// Enum values if the column is an enum type (for selection UI).
    pub enum_values: Option<Vec<String>>,
    /// Whether the current column has a foreign key constraint.
    pub has_foreign_key: bool,
}

/// Find `AddColumn` / `ModifyColumnNullable` actions that need a `fill_with`
/// value because they introduce NOT NULL on a column without a DB default.
pub fn find_missing_fill_with(
    plan: &MigrationPlan,
    current_schema: &[TableDef],
) -> Vec<FillWithRequired> {
    let mut missing = Vec::new();

    for (idx, action) in plan.actions.iter().enumerate() {
        match action {
            MigrationAction::AddColumn {
                table,
                column,
                fill_with,
            }
                // If column is NOT NULL and has no default, fill_with is required
                if !column.nullable && column.default.is_none() && fill_with.is_none() => {
                    missing.push(FillWithRequired {
                        action_index: idx,
                        table: table.clone(),
                        column: column.name.clone(),
                        action_type: "AddColumn",
                        column_type: column.r#type.to_display_string(),
                        default_value: column.r#type.default_fill_value().to_string(),
                        enum_values: column.r#type.enum_variant_names(),
                        has_foreign_key: false,
                    });
                }
            MigrationAction::ModifyColumnNullable {
                table,
                column,
                nullable,
                fill_with,
                delete_null_rows,
            }
                // If changing from nullable to non-nullable, fill_with is required
                // UNLESS the column already has a default value (which will be used)
                if !nullable && fill_with.is_none() && !delete_null_rows.unwrap_or(false) => {
                    // Look up column from the current schema
                    let table_def = current_schema.iter().find(|t| t.name == *table);

                    let col_def =
                        table_def.and_then(|t| t.columns.iter().find(|c| c.name == *column));

                    let has_foreign_key = table_def.is_some_and(|t| t.constraints.iter().any(|constraint| matches!(constraint, TableConstraint::ForeignKey { columns, .. } if columns.iter().any(|col_name| col_name == column))));

                    // If column has a default value, fill_with is not needed
                    if col_def.is_some_and(|c| c.default.is_some()) {
                        continue;
                    }

                    let (col_type_str, default_val, enum_vals) = match col_def {
                        Some(c) => (
                            c.r#type.to_display_string(),
                            c.r#type.default_fill_value().to_string(),
                            c.r#type.enum_variant_names(),
                        ),
                        None => (column.clone(), "''".to_string(), None),
                    };

                    missing.push(FillWithRequired {
                        action_index: idx,
                        table: table.clone(),
                        column: column.clone(),
                        action_type: "ModifyColumnNullable",
                        column_type: col_type_str,
                        default_value: default_val,
                        enum_values: enum_vals,
                        has_foreign_key,
                    });
                }
            _ => {}
        }
    }

    missing
}

/// Describes an enum-narrowing action whose `fill_with` is required but missing.
/// Returned by [`find_missing_enum_fill_with`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumFillWithRequired {
    /// Index of the action in the migration plan.
    pub action_index: usize,
    /// Table name.
    pub table: String,
    /// Column name.
    pub column: String,
    /// Removed enum values that need replacement mappings.
    pub removed_values: Vec<String>,
    /// Remaining valid enum values (for selection UI).
    pub remaining_values: Vec<String>,
}

/// Find `ModifyColumnType` actions that remove enum values, requiring a
/// `fill_with` to substitute for rows still using the removed value.
pub fn find_missing_enum_fill_with(
    plan: &MigrationPlan,
    current_schema: &[TableDef],
) -> Vec<EnumFillWithRequired> {
    let mut missing = Vec::new();

    for (idx, action) in plan.actions.iter().enumerate() {
        if let MigrationAction::ModifyColumnType {
            table,
            column,
            new_type,
            fill_with,
        } = action
        {
            // Only applies to string enum → string enum changes
            let old_type = current_schema
                .iter()
                .find(|t| t.name == *table)
                .and_then(|t| t.columns.iter().find(|c| c.name == *column))
                .map(|c| &c.r#type);

            if let (
                Some(ColumnType::Complex(ComplexColumnType::Enum {
                    values: EnumValues::String(old_values),
                    ..
                })),
                ColumnType::Complex(ComplexColumnType::Enum {
                    values: EnumValues::String(new_values),
                    ..
                }),
            ) = (old_type, new_type)
            {
                // Find removed values (in old but not in new)
                let removed: Vec<String> = old_values
                    .iter()
                    .filter(|v| !new_values.contains(v))
                    .cloned()
                    .collect();

                if removed.is_empty() {
                    continue;
                }

                // Check if fill_with covers all removed values
                let all_covered = match fill_with {
                    Some(fw) => removed.iter().all(|r| fw.contains_key(r)),
                    None => false,
                };

                if !all_covered {
                    // Filter to only uncovered removed values
                    let uncovered: Vec<String> = match fill_with {
                        Some(fw) => removed
                            .into_iter()
                            .filter(|r| !fw.contains_key(r))
                            .collect(),
                        None => removed,
                    };

                    missing.push(EnumFillWithRequired {
                        action_index: idx,
                        table: table.clone(),
                        column: column.clone(),
                        removed_values: uncovered,
                        remaining_values: new_values.clone(),
                    });
                }
            }
        }
    }

    missing
}
