//! Schema diffing and migration planning.
//!
//! - [`diff_schemas`]: compute typed `MigrationAction`s between two schemas
//! - [`apply_action`]: replay an action onto a baseline schema
//! - [`validate_schema`], [`validate_migration_plan`]: ensure invariants

pub mod apply;
pub mod diff;
pub mod error;
mod parallel_config;
pub mod plan;
pub mod schema;
pub mod validate;

pub use apply::apply_action;
pub use diff::diff_schemas;
pub use error::PlannerError;
pub use plan::{plan_next_migration, plan_next_migration_with_baseline};
pub use schema::schema_from_plans;
pub use validate::{
    ConstraintDropWarning, EnumFillWithRequired, FillWithRequired, FkPolicyChangeWarning,
    MissingFkSupportingIndex, NarrowingKind, PolicyDelta, TimezoneConversionDirection,
    TimezoneConversionWarning, TypeNarrowingWarning, find_constraint_drops_without_replacement,
    find_fk_policy_changes, find_missing_enum_fill_with, find_missing_fill_with,
    find_missing_fk_supporting_indexes, find_timezone_conversions, find_type_narrowings,
    is_narrowing, render_reference_action, validate_migration_plan, validate_schema,
};
