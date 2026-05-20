use std::collections::{BTreeMap, HashSet};

use rayon::prelude::*;
use vespertide_core::{TableConstraint, TableDef, schema::primary_key::PrimaryKeySyntax};

use super::enums::validate_column;
use super::foreign_keys::validate_foreign_key_constraint;
use crate::error::PlannerError;
use crate::parallel_config::{VALIDATE_SCHEMA_PAR_MIN_LEN, validate_schema_par_threshold};

/// Validate a schema for data integrity issues.
/// Checks for:
/// - Duplicate table names
/// - Foreign keys referencing non-existent tables
/// - Foreign keys referencing non-existent columns
/// - Indexes referencing non-existent columns
/// - Constraints referencing non-existent columns
/// - Empty constraint column lists
pub fn validate_schema(schema: &[TableDef]) -> Result<(), PlannerError> {
    // Check for duplicate table names
    let mut table_names = HashSet::new();
    for table in schema {
        if !table_names.insert(&table.name) {
            return Err(PlannerError::DuplicateTableName(table.name.clone()));
        }
    }

    // Build a map of table names to their column names for quick lookup
    // perf: BTreeMap gives deterministic validation traversal and avoids hashing for small schemas.
    let table_map: BTreeMap<_, _> = schema
        .iter()
        .map(|t| {
            let columns: HashSet<_> = t.columns.iter().map(|c| c.name.as_str()).collect();
            (t.name.as_str(), columns)
        })
        .collect();

    // Validate each table. Collect the indexed errors first so parallel validation
    // reports the same earliest table error as the sequential path.
    let earliest_err = if schema.len() < validate_schema_par_threshold() {
        schema
            .iter()
            .enumerate()
            .filter_map(|(index, table)| {
                validate_table_entry(table, &table_map)
                    .err()
                    .map(|e| (index, e))
            })
            .min_by_key(|(index, _)| *index)
            .map(|(_, err)| err)
    } else {
        schema
            .par_iter()
            .with_min_len(VALIDATE_SCHEMA_PAR_MIN_LEN)
            .enumerate()
            .filter_map(|(index, table)| {
                validate_table_entry(table, &table_map)
                    .err()
                    .map(|e| (index, e))
            })
            .min_by_key(|(index, _)| *index)
            .map(|(_, err)| err)
    };

    if let Some(err) = earliest_err {
        return Err(err);
    }

    Ok(())
}

fn validate_table_entry(
    table: &TableDef,
    table_map: &BTreeMap<&str, HashSet<&str>>,
) -> Result<(), PlannerError> {
    table
        .validate_unique_column_names()
        .map_err(|e| PlannerError::TableValidation(e.to_string()))?;
    validate_table(table, table_map)
}

pub(super) fn validate_table(
    table: &TableDef,
    table_map: &BTreeMap<&str, HashSet<&str>>,
) -> Result<(), PlannerError> {
    let table_columns: HashSet<_> = table.columns.iter().map(|c| c.name.as_str()).collect();

    // Check that the table has a primary key
    // Primary key can be defined either:
    // 1. As a table-level constraint (TableConstraint::PrimaryKey)
    // 2. As an inline column definition (column.primary_key = Some(...))
    let has_table_pk = table
        .constraints
        .iter()
        .any(|c| matches!(c, TableConstraint::PrimaryKey { .. }));
    let has_inline_pk = table.columns.iter().any(|c| c.primary_key.is_some());

    if !has_table_pk && !has_inline_pk {
        return Err(PlannerError::MissingPrimaryKey(table.name.clone()));
    }

    // Validate auto_increment columns have integer types
    for constraint in &table.constraints {
        if let TableConstraint::PrimaryKey {
            auto_increment: true,
            columns,
        } = constraint
        {
            for col_name in columns {
                if let Some(column) = table.columns.iter().find(|c| c.name == *col_name)
                    && !column.r#type.supports_auto_increment()
                {
                    return Err(PlannerError::InvalidAutoIncrement(
                        table.name.clone(),
                        col_name.clone(),
                        format!("{:?}", column.r#type),
                    ));
                }
            }
        }
    }

    // Validate auto_increment on inline primary_key definitions
    for column in &table.columns {
        if let Some(pk_syntax) = &column.primary_key {
            let has_auto_increment = match pk_syntax {
                PrimaryKeySyntax::Bool(_) => false,
                PrimaryKeySyntax::Object(pk_def) => pk_def.auto_increment,
            };
            if has_auto_increment && !column.r#type.supports_auto_increment() {
                return Err(PlannerError::InvalidAutoIncrement(
                    table.name.clone(),
                    column.name.clone(),
                    format!("{:?}", column.r#type),
                ));
            }
        }
    }

    // Validate columns (enum types)
    for column in &table.columns {
        validate_column(column, &table.name)?;
    }

    // Validate constraints (including indexes)
    for constraint in &table.constraints {
        validate_constraint(constraint, &table.name, &table_columns, table_map)?;
    }

    Ok(())
}

fn validate_constraint(
    constraint: &TableConstraint,
    table_name: &str,
    table_columns: &HashSet<&str>,
    table_map: &BTreeMap<&str, HashSet<&str>>,
) -> Result<(), PlannerError> {
    match constraint {
        TableConstraint::PrimaryKey { columns, .. } => {
            if columns.is_empty() {
                return Err(PlannerError::EmptyConstraintColumns(
                    table_name.to_string(),
                    "PrimaryKey".to_string(),
                ));
            }
            for col in columns {
                if !table_columns.contains(col.as_str()) {
                    return Err(PlannerError::ConstraintColumnNotFound(
                        table_name.to_string(),
                        "PrimaryKey".to_string(),
                        col.clone(),
                    ));
                }
            }
        }
        TableConstraint::Unique { columns, .. } => {
            if columns.is_empty() {
                return Err(PlannerError::EmptyConstraintColumns(
                    table_name.to_string(),
                    "Unique".to_string(),
                ));
            }
            for col in columns {
                if !table_columns.contains(col.as_str()) {
                    return Err(PlannerError::ConstraintColumnNotFound(
                        table_name.to_string(),
                        "Unique".to_string(),
                        col.clone(),
                    ));
                }
            }
        }
        TableConstraint::ForeignKey {
            columns,
            ref_table,
            ref_columns,
            ..
        } => validate_foreign_key_constraint(
            table_name,
            table_columns,
            table_map,
            columns,
            ref_table,
            ref_columns,
        )?,
        TableConstraint::Check { .. } => {
            // Check constraints are just expressions, no validation needed
        }
        TableConstraint::Index { name, columns } => {
            if columns.is_empty() {
                let index_name = name.clone().unwrap_or_else(|| "(unnamed)".to_string());
                return Err(PlannerError::EmptyConstraintColumns(
                    table_name.to_string(),
                    format!("Index({index_name})"),
                ));
            }

            for col in columns {
                if !table_columns.contains(col.as_str()) {
                    let index_name = name.clone().unwrap_or_else(|| "(unnamed)".to_string());
                    return Err(PlannerError::IndexColumnNotFound(
                        table_name.to_string(),
                        index_name,
                        col.clone(),
                    ));
                }
            }
        }
        _ => unreachable!("TableConstraint is #[non_exhaustive]; all variants are matched above"),
    }

    Ok(())
}
