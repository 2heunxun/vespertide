//! Core data structures for vespertide schema definition and migration planning.
//!
//! - [`TableDef`], [`ColumnDef`]: schema model
//! - [`MigrationAction`], [`MigrationPlan`]: typed migration operations
//! - [`MigrationError`]: runtime migration error type

pub mod action;
#[cfg(feature = "arbitrary")]
pub mod arbitrary;
pub mod migration;
pub mod schema;

pub use action::{MigrationAction, MigrationPlan, NarrowingStrategy};
pub use migration::{MigrationError, MigrationOptions};
pub use schema::{
    ColumnDef, ColumnName, ColumnType, ComplexColumnType, ConstraintKind, DefaultValue, EnumValues,
    IndexDef, IndexName, NumValue, ReferenceAction, SimpleColumnType, StrOrBoolOrArray,
    StringOrBool, TableConstraint, TableDef, TableName, TableValidationError,
};
