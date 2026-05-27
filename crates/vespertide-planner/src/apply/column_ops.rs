use std::collections::HashMap;

use vespertide_core::{
    ColumnDef, ColumnName, ColumnType, ComplexColumnType, EnumValues, TableConstraint, TableDef,
};

use crate::error::PlannerError;

pub(super) fn add_column(
    schema: &mut [TableDef],
    table: &str,
    column: &ColumnDef,
) -> Result<(), PlannerError> {
    let tbl = find_table_mut(schema, table)?;
    if tbl.columns.iter().any(|c| c.name == column.name) {
        Err(PlannerError::ColumnExists(
            table.to_string(),
            column.name.to_string(),
        ))
    } else {
        tbl.columns.push(column.clone());
        // Re-normalize to promote any inline constraints on the new column
        // to table-level TableConstraint entries.
        // perf: move the table out before normalization to avoid cloning the full table twice.
        let table_to_normalize = std::mem::replace(
            tbl,
            TableDef {
                name: table.to_string().into(),
                description: None,
                columns: Vec::new(),
                constraints: Vec::new(),
            },
        );
        let normalized = table_to_normalize.normalize().map_err(|e| {
            PlannerError::TableValidation(format!(
                "Failed to normalize table '{}' after adding column '{}': {}",
                table, column.name, e
            ))
        })?;
        *tbl = normalized;
        Ok(())
    }
}

pub(super) fn delete_column(
    schema: &mut [TableDef],
    table: &str,
    column: &str,
) -> Result<(), PlannerError> {
    let tbl = find_table_mut(schema, table)?;
    let before = tbl.columns.len();
    tbl.columns.retain(|c| c.name != column);
    if tbl.columns.len() == before {
        Err(PlannerError::ColumnNotFound(
            table.to_string(),
            column.to_string(),
        ))
    } else {
        drop_column_from_constraints(&mut tbl.constraints, column);
        Ok(())
    }
}

pub(super) fn rename_column(
    schema: &mut [TableDef],
    table: &str,
    from: &str,
    to: &str,
) -> Result<(), PlannerError> {
    let tbl = find_table_mut(schema, table)?;
    let col = tbl
        .columns
        .iter_mut()
        .find(|c| c.name == from)
        .ok_or_else(|| PlannerError::ColumnNotFound(table.to_string(), from.to_string()))?;
    col.name = to.into();
    rename_column_in_constraints(&mut tbl.constraints, from, to);
    Ok(())
}

pub(super) fn modify_column_type(
    schema: &mut [TableDef],
    table: &str,
    column: &str,
    new_type: &ColumnType,
) -> Result<(), PlannerError> {
    find_column_mut(schema, table, column)?.r#type = new_type.clone();
    Ok(())
}

/// Rewrite the stored `value` of every integer-enum variant whose current
/// value appears as a key in `mapping`. The column type and variant names
/// are left untouched; only the numeric values shift. No-op when the
/// column is not an integer enum (defensive — the diff layer should never
/// emit `RemapEnumValues` for non-integer-enum columns, but apply must not
/// panic in that case).
pub(super) fn remap_enum_values(
    schema: &mut [TableDef],
    table: &str,
    column: &str,
    mapping: &[(i64, i64)],
) -> Result<(), PlannerError> {
    let col = find_column_mut(schema, table, column)?;
    if let ColumnType::Complex(ComplexColumnType::Enum {
        values: EnumValues::Integer(items),
        ..
    }) = &mut col.r#type
    {
        let lookup: HashMap<i64, i64> = mapping.iter().copied().collect();
        for item in items.iter_mut() {
            if let Some(&new_val) = lookup.get(&item.value) {
                item.value = new_val;
            }
        }
    }
    Ok(())
}

pub(super) fn modify_column_nullable(
    schema: &mut [TableDef],
    table: &str,
    column: &str,
    nullable: bool,
) -> Result<(), PlannerError> {
    find_column_mut(schema, table, column)?.nullable = nullable;
    Ok(())
}

pub(super) fn modify_column_default(
    schema: &mut [TableDef],
    table: &str,
    column: &str,
    new_default: Option<&str>,
) -> Result<(), PlannerError> {
    find_column_mut(schema, table, column)?.default = new_default.map(Into::into);
    Ok(())
}

pub(super) fn modify_column_comment(
    schema: &mut [TableDef],
    table: &str,
    column: &str,
    new_comment: Option<&String>,
) -> Result<(), PlannerError> {
    find_column_mut(schema, table, column)?.comment = new_comment.cloned();
    Ok(())
}

fn find_table_mut<'a>(
    schema: &'a mut [TableDef],
    table: &str,
) -> Result<&'a mut TableDef, PlannerError> {
    schema
        .iter_mut()
        .find(|t| t.name == table)
        .ok_or_else(|| PlannerError::TableNotFound(table.to_string()))
}

fn find_column_mut<'a>(
    schema: &'a mut [TableDef],
    table: &str,
    column: &str,
) -> Result<&'a mut ColumnDef, PlannerError> {
    find_table_mut(schema, table)?
        .columns
        .iter_mut()
        .find(|c| c.name == column)
        .ok_or_else(|| PlannerError::ColumnNotFound(table.to_string(), column.to_string()))
}

fn rename_column_in_constraints(constraints: &mut [TableConstraint], from: &str, to: &str) {
    for constraint in constraints {
        match constraint {
            TableConstraint::PrimaryKey { columns, .. }
            | TableConstraint::Unique { columns, .. }
            | TableConstraint::Index { columns, .. } => rename_column_refs(columns, from, to),
            TableConstraint::ForeignKey {
                columns,
                ref_columns,
                ..
            } => {
                rename_column_refs(columns, from, to);
                rename_column_refs(ref_columns, from, to);
            }
            TableConstraint::Check { .. } => {}
            _ => {
                unreachable!("TableConstraint is #[non_exhaustive]; all variants are matched above")
            }
        }
    }
}

fn rename_column_refs(columns: &mut [ColumnName], from: &str, to: &str) {
    for c in columns {
        if c == from {
            *c = to.into();
        }
    }
}

fn drop_column_from_constraints(constraints: &mut Vec<TableConstraint>, column: &str) {
    constraints.retain_mut(|c| match c {
        TableConstraint::PrimaryKey { columns, .. }
        | TableConstraint::Unique { columns, .. }
        | TableConstraint::Index { columns, .. } => {
            columns.retain(|c| c != column);
            !columns.is_empty()
        }
        TableConstraint::ForeignKey { columns, .. } => {
            columns.retain(|c| c != column);
            !columns.is_empty()
        }
        TableConstraint::Check { .. } => true,
        _ => unreachable!("TableConstraint is #[non_exhaustive]; all variants are matched above"),
    });
}

#[cfg(test)]
pub(super) fn rename_column_in_constraints_for_test(
    constraints: &mut [TableConstraint],
    from: &str,
    to: &str,
) {
    rename_column_in_constraints(constraints, from, to);
}
