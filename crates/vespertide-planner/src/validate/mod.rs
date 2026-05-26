mod constraint_drops;
mod enums;
mod fk_policy_changes;
mod foreign_keys;
mod plan;
mod schema;

pub use constraint_drops::{ConstraintDropWarning, find_constraint_drops_without_replacement};
pub use fk_policy_changes::{
    FkPolicyChangeWarning, PolicyDelta, find_fk_policy_changes, render_reference_action,
};
pub use foreign_keys::{MissingFkSupportingIndex, find_missing_fk_supporting_indexes};
pub use plan::{
    EnumFillWithRequired, FillWithRequired, find_missing_enum_fill_with, find_missing_fill_with,
    validate_migration_plan,
};
pub use schema::validate_schema;

#[cfg(test)]
mod tests;
