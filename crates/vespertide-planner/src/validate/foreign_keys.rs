use std::collections::{BTreeMap, HashSet};

use crate::error::PlannerError;

pub(super) fn validate_foreign_key_constraint(
    table_name: &str,
    table_columns: &HashSet<&str>,
    table_map: &BTreeMap<&str, HashSet<&str>>,
    columns: &[String],
    ref_table: &str,
    ref_columns: &[String],
) -> Result<(), PlannerError> {
    if columns.is_empty() {
        return Err(PlannerError::EmptyConstraintColumns(
            table_name.to_string(),
            "ForeignKey".to_string(),
        ));
    }
    if ref_columns.is_empty() {
        return Err(PlannerError::EmptyConstraintColumns(
            ref_table.to_string(),
            "ForeignKey (ref_columns)".to_string(),
        ));
    }

    let ref_table_columns = table_map.get(ref_table).ok_or_else(|| {
        PlannerError::ForeignKeyTableNotFound(
            table_name.to_string(),
            columns.join(", "),
            ref_table.to_string(),
        )
    })?;

    for col in columns {
        if !table_columns.contains(col.as_str()) {
            return Err(PlannerError::ConstraintColumnNotFound(
                table_name.to_string(),
                "ForeignKey".to_string(),
                col.clone(),
            ));
        }
    }

    for ref_col in ref_columns {
        if !ref_table_columns.contains(ref_col.as_str()) {
            return Err(PlannerError::ForeignKeyColumnNotFound(
                table_name.to_string(),
                columns.join(", "),
                ref_table.to_string(),
                ref_col.clone(),
            ));
        }
    }

    if columns.len() != ref_columns.len() {
        return Err(PlannerError::ForeignKeyColumnNotFound(
            table_name.to_string(),
            format!(
                "column count mismatch: {} != {}",
                columns.len(),
                ref_columns.len()
            ),
            ref_table.to_string(),
            String::new(),
        ));
    }

    Ok(())
}
