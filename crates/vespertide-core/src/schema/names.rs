/// The name of a database table, always in `snake_case` by convention.
///
/// This is a plain `String` alias used throughout the API to make function signatures
/// self-documenting. The naming convention is enforced by the CLI and the planner, not by the
/// type system.
pub type TableName = String;

/// The name of a table column, always in `snake_case` by convention.
///
/// This is a plain `String` alias used throughout the API to make function signatures
/// self-documenting.
pub type ColumnName = String;

/// The name of a database index, conventionally `ix_{table}__{columns}` (double underscore).
///
/// This is a plain `String` alias used throughout the API to make function signatures
/// self-documenting.
pub type IndexName = String;
