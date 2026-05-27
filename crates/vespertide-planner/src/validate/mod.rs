mod check_default;
mod constraint_drops;
mod dangling_fk_drops;
mod default_changes;
mod enums;
mod fk_policy_changes;
mod foreign_keys;
mod plan;
mod schema;
mod timezone_conversion;
mod type_narrowing;

pub use constraint_drops::{ConstraintDropWarning, find_constraint_drops_without_replacement};
pub use dangling_fk_drops::{DanglingFkDrop, find_dangling_fk_drops};
pub use default_changes::{
    DefaultChangeKind, DefaultChangeWarning, RiskLevel, find_default_changes,
};
pub use fk_policy_changes::{
    FkPolicyChangeWarning, PolicyDelta, find_fk_policy_changes, render_reference_action,
};
pub use foreign_keys::{MissingFkSupportingIndex, find_missing_fk_supporting_indexes};
pub use plan::{
    EnumFillWithRequired, FillWithRequired, find_missing_enum_fill_with, find_missing_fill_with,
    find_plan_violations, validate_migration_plan,
};
pub use schema::{find_schema_violations, validate_schema};
pub use timezone_conversion::{
    TimezoneConversionDirection, TimezoneConversionWarning, find_timezone_conversions,
};
pub use type_narrowing::{
    NarrowingKind, TypeNarrowingWarning, find_type_narrowings, is_narrowing,
};

#[cfg(test)]
mod tests;
