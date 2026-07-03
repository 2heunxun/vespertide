//! Shared constraint-scan helpers used by the ORM renderers.
//!
//! Every backend needs the same lookup sets when rendering a table:
//! the columns covered by table-level primary keys, the columns that
//! carry a single-column unique constraint, and the columns that carry
//! a single-column index. Centralising the scans keeps the four
//! renderers from drifting apart.

use std::collections::HashSet;

use vespertide_core::TableConstraint;

/// Collect the column names covered by table-level `PrimaryKey` constraints.
///
/// Lookup-only, ordering unused.
pub(crate) fn primary_key_columns(constraints: &[TableConstraint]) -> HashSet<String> {
    let mut keys = HashSet::new();
    for constraint in constraints {
        if let TableConstraint::PrimaryKey { columns, .. } = constraint {
            for col in columns {
                keys.insert(col.to_string());
            }
        }
    }
    keys
}

/// Collect the column names that carry a single-column `Unique` constraint.
///
/// Lookup-only, ordering unused.
pub(crate) fn single_column_uniques(constraints: &[TableConstraint]) -> HashSet<String> {
    let mut unique_cols = HashSet::new();
    for constraint in constraints {
        if let TableConstraint::Unique { columns, .. } = constraint
            && columns.len() == 1
        {
            unique_cols.insert(columns[0].to_string());
        }
    }
    unique_cols
}

/// Collect the column names that carry a single-column `Index` constraint.
///
/// Lookup-only, ordering unused.
pub(crate) fn single_column_indexes(constraints: &[TableConstraint]) -> HashSet<String> {
    let mut indexed_cols = HashSet::new();
    for constraint in constraints {
        if let TableConstraint::Index { columns, .. } = constraint
            && columns.len() == 1
        {
            indexed_cols.insert(columns[0].to_string());
        }
    }
    indexed_cols
}
