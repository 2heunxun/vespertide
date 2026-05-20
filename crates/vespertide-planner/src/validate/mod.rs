mod enums;
mod foreign_keys;
mod plan;
mod schema;

pub use plan::{
    EnumFillWithRequired, FillWithRequired, find_missing_enum_fill_with, find_missing_fill_with,
    validate_migration_plan,
};
pub use schema::validate_schema;

#[cfg(test)]
mod tests;
