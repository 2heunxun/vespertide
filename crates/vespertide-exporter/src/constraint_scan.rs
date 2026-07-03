//! Shared constraint-scan helpers used by the ORM renderers.
//!
//! Every backend needs the same lookup sets when rendering a table:
//! the columns covered by table-level primary keys, the columns that
//! carry a single-column unique constraint, and the columns that carry
//! a single-column index. Centralising the scans keeps the four
//! renderers from drifting apart.

use std::collections::HashSet;

use vespertide_core::{ColumnName, TableConstraint};

/// Collect the column names from every single-column constraint that `extract`
/// matches. Shared body for [`single_column_uniques`] and
/// [`single_column_indexes`], which differ only in the matched
/// `TableConstraint` variant. `extract` returns the constraint's column slice
/// for the variant it cares about, or `None` for every other variant.
fn single_column_scan(
    constraints: &[TableConstraint],
    extract: impl Fn(&TableConstraint) -> Option<&[ColumnName]>,
) -> HashSet<String> {
    let mut cols = HashSet::new();
    for constraint in constraints {
        if let Some(columns) = extract(constraint)
            && columns.len() == 1
        {
            cols.insert(columns[0].to_string());
        }
    }
    cols
}

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
    single_column_scan(constraints, |c| match c {
        TableConstraint::Unique { columns, .. } => Some(columns.as_slice()),
        _ => None,
    })
}

/// Collect the column names that carry a single-column `Index` constraint.
///
/// Lookup-only, ordering unused.
pub(crate) fn single_column_indexes(constraints: &[TableConstraint]) -> HashSet<String> {
    single_column_scan(constraints, |c| match c {
        TableConstraint::Index { columns, .. } => Some(columns.as_slice()),
        _ => None,
    })
}
