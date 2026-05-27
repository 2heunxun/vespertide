use std::fmt;

use thiserror::Error;

/// Aggregates multiple [`PlannerError`]s into a single error so that batch
/// validators can report every violation at once.
///
/// The `Display` implementation renders a numbered list (1-indexed) of the
/// nested errors, preserving their order. Use this wherever multiple,
/// independently-meaningful failures must be surfaced from a single
/// validation pass — e.g. [`crate::validate::find_schema_violations`] or
/// [`crate::validate::find_plan_violations`].
#[derive(Debug)]
pub struct MultipleErrors(pub Vec<PlannerError>);

impl fmt::Display for MultipleErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{} validation violation(s):", self.0.len())?;
        for (idx, err) in self.0.iter().enumerate() {
            writeln!(f, "  {}. {err}", idx + 1)?;
        }
        write!(f, "Fix all of the above before re-running this command.")
    }
}

impl std::error::Error for MultipleErrors {}

#[derive(Debug, Error)]
pub enum PlannerError {
    /// Wraps two or more independent [`PlannerError`]s reported in a single
    /// validation pass. Boxed via [`MultipleErrors`] to keep the enum size
    /// small (`Vec<PlannerError>` would otherwise inflate every variant).
    #[error("{0}")]
    Multiple(Box<MultipleErrors>),
    #[error("table already exists: {0}")]
    TableExists(String),
    #[error("table not found: {0}")]
    TableNotFound(String),
    #[error("column already exists: {0}.{1}")]
    ColumnExists(String, String),
    #[error("column not found: {0}.{1}")]
    ColumnNotFound(String, String),
    #[error("index not found: {0}.{1}")]
    IndexNotFound(String, String),
    #[error("duplicate table name: {0}")]
    DuplicateTableName(String),
    #[error("foreign key references non-existent table: {0}.{1} -> {2}")]
    ForeignKeyTableNotFound(String, String, String),
    #[error("foreign key references non-existent column: {0}.{1} -> {2}.{3}")]
    ForeignKeyColumnNotFound(String, String, String, String),
    #[error("index references non-existent column: {0}.{1} -> {2}")]
    IndexColumnNotFound(String, String, String),
    #[error("constraint references non-existent column: {0}.{1} -> {2}")]
    ConstraintColumnNotFound(String, String, String),
    #[error("constraint has empty column list: {0}.{1}")]
    EmptyConstraintColumns(String, String),
    #[error("AddColumn requires fill_with when column is NOT NULL without default: {0}.{1}")]
    MissingFillWith(String, String),
    #[error("table validation error: {0}")]
    TableValidation(String),
    #[error("table '{0}' must have a primary key")]
    MissingPrimaryKey(String),
    #[error("enum '{0}' in column '{1}.{2}' has duplicate variant name: '{3}'")]
    DuplicateEnumVariantName(String, String, String, String),
    #[error("enum '{0}' in column '{1}.{2}' has duplicate value: {3}")]
    DuplicateEnumValue(String, String, String, i64),
    #[error("{0}")]
    InvalidEnumDefault(#[from] Box<InvalidEnumDefaultError>),
    #[error(
        "auto_increment on non-integer column: {0}.{1} (type {2} does not support auto_increment)"
    )]
    InvalidAutoIncrement(String, String, String),
    #[error(
        "default value violates CHECK constraint: {table}.{column} default = {default_value} \
         fails CHECK ({check_expr}) — every INSERT relying on this default will be rejected by \
         the database. Change the default to satisfy the constraint, or relax the constraint."
    )]
    DefaultViolatesCheck {
        table: String,
        column: String,
        default_value: String,
        check_name: String,
        check_expr: String,
    },
    /// Fault **F9**: a column or table is being dropped while a foreign key on
    /// another table still references it, with no matching cleanup in the
    /// same plan.
    ///
    /// Surviving foreign keys whose `(ref_table, ref_column)` is removed by
    /// this plan would silently break referential integrity once applied.
    /// The plan must either drop the offending FK (`RemoveConstraint`) or
    /// drop the entire referencing table in the same migration.
    ///
    /// - `dropped_table` is the table whose column or whole row is being removed.
    /// - `dropped_column` is `Some(col)` for `DeleteColumn`; `None` for
    ///   `DeleteTable` (the whole table is going away).
    /// - `referencing_table` is the *other* table whose FK is now dangling.
    /// - `referencing_constraint` is the FK constraint's declared name, or
    ///   `None` when the FK was inlined without an explicit name.
    #[error(
        "cannot drop {}: foreign key {} on table `{referencing_table}` references it. \
         Either drop that foreign key in the same migration, or drop the `{referencing_table}` table.",
        format_dropped(dropped_table, dropped_column.as_deref()),
        format_fk(referencing_constraint.as_deref()),
    )]
    DanglingForeignKeyAfterDrop {
        dropped_table: String,
        dropped_column: Option<String>,
        referencing_table: String,
        referencing_constraint: Option<String>,
    },
}

fn format_dropped(table: &str, column: Option<&str>) -> String {
    match column {
        Some(col) => format!("column `{table}.{col}`"),
        None => format!("table `{table}`"),
    }
}

fn format_fk(name: Option<&str>) -> String {
    match name {
        Some(n) => format!("`{n}`"),
        None => "(unnamed)".to_string(),
    }
}

/// An enum column has a default or `fill_with` value not in the allowed set.
#[derive(Debug, Error)]
#[error(
    "enum '{enum_name}' in column '{table_name}.{column_name}' has invalid {value_type} value '{value}': not in allowed values [{allowed}]"
)]
pub struct InvalidEnumDefaultError {
    pub enum_name: String,
    pub table_name: String,
    pub column_name: String,
    pub value_type: String,
    pub value: String,
    pub allowed: String,
}
