use crate::schema::names::{ColumnName, TableName};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableValidationError {
    DuplicateColumnName {
        table: TableName,
        column: ColumnName,
    },
    DuplicateIndexColumn {
        index_name: String,
        column_name: String,
    },
    InvalidForeignKeyFormat {
        column_name: String,
        value: String,
    },
    /// Internal invariant violation in normalization; valid user input should not trigger this.
    InvariantViolation {
        context: String,
    },
}

impl std::fmt::Display for TableValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TableValidationError::DuplicateColumnName { table, column } => {
                write!(f, "table '{table}' has duplicate column name '{column}'")
            }
            TableValidationError::DuplicateIndexColumn {
                index_name,
                column_name,
            } => {
                write!(
                    f,
                    "Duplicate index '{index_name}' on column '{column_name}': the same index name cannot be applied to the same column multiple times"
                )
            }
            TableValidationError::InvalidForeignKeyFormat { column_name, value } => {
                write!(
                    f,
                    "Invalid foreign key format '{value}' on column '{column_name}': expected 'table.column' format"
                )
            }
            TableValidationError::InvariantViolation { context } => {
                write!(
                    f,
                    "internal table normalization invariant violated: {context}"
                )
            }
        }
    }
}

impl std::error::Error for TableValidationError {}
