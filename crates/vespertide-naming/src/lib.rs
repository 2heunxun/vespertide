//! Naming conventions and helpers for vespertide database schema management.
//!
//! This crate provides consistent naming functions for database objects like
//! indexes, constraints, and foreign keys. It has no dependencies and can be
//! used by any other vespertide crate.

// ============================================================================
// Constraint Naming (for SQL generation)
// ============================================================================

/// Generate index name from table name, columns, and optional user-provided key.
/// Always includes table name to avoid conflicts across tables.
/// Uses double underscore to separate table name from the rest.
/// Format: ix_{table}__{key} or ix_{table}__{col1}_{col2}...
pub fn build_index_name<T: AsRef<str>>(table: &str, columns: &[T], key: Option<&str>) -> String {
    build_constraint_name("ix_", table, columns, key)
}

/// Generate unique constraint name from table name, columns, and optional user-provided key.
/// Always includes table name to avoid conflicts across tables.
/// Uses double underscore to separate table name from the rest.
/// Format: uq_{table}__{key} or uq_{table}__{col1}_{col2}...
pub fn build_unique_constraint_name<T: AsRef<str>>(
    table: &str,
    columns: &[T],
    key: Option<&str>,
) -> String {
    build_constraint_name("uq_", table, columns, key)
}

/// Generate foreign key constraint name from table name, columns, and optional user-provided key.
/// Always includes table name to avoid conflicts across tables.
/// Uses double underscore to separate table name from the rest.
/// Format: fk_{table}__{key} or fk_{table}__{col1}_{col2}...
pub fn build_foreign_key_name<T: AsRef<str>>(
    table: &str,
    columns: &[T],
    key: Option<&str>,
) -> String {
    build_constraint_name("fk_", table, columns, key)
}

/// Shared body for the three constraint name builders above.
///
/// Folds the `{prefix}{table}__{key|sorted-columns}` template into a single
/// pre-sized `String` so the auto-named branch ( `key.is_none()`) does only
/// two allocations: the column-sort scratchpad (`Vec<&str>`) and the final
/// `String`. The previous implementation went through `format!(... join("_"))`
/// which allocated an extra intermediate `String` for the joined columns
/// before formatting them into the final result.
fn build_constraint_name<T: AsRef<str>>(
    prefix: &str,
    table: &str,
    columns: &[T],
    key: Option<&str>,
) -> String {
    if let Some(k) = key {
        let mut out = String::with_capacity(prefix.len() + table.len() + 2 + k.len());
        out.push_str(prefix);
        out.push_str(table);
        out.push_str("__");
        out.push_str(k);
        out
    } else {
        let cols_capacity: usize = columns
            .iter()
            .map(|c| c.as_ref().len() + 1)
            .sum::<usize>()
            .saturating_sub(1);
        let mut out = String::with_capacity(prefix.len() + table.len() + 2 + cols_capacity);
        out.push_str(prefix);
        out.push_str(table);
        out.push_str("__");
        write_sorted_columns(&mut out, columns);
        out
    }
}

/// Sort the column slice into a local scratchpad and write the columns into
/// `out` joined by `'_'`. Replaces the previous `sort_columns_for_name(...).join("_")`
/// pair which allocated a fresh `String` for the joined columns; here the
/// columns go directly into the caller-supplied buffer.
fn write_sorted_columns<T: AsRef<str>>(out: &mut String, columns: &[T]) {
    let mut sorted: Vec<&str> = columns.iter().map(AsRef::as_ref).collect();
    sorted.sort_unstable();
    for (i, c) in sorted.iter().enumerate() {
        if i > 0 {
            out.push('_');
        }
        out.push_str(c);
    }
}

/// Generate CHECK constraint name for `SQLite` enum column.
/// Uses double underscore to separate table name from the rest.
/// Format: chk_{table}__{column}
pub fn build_check_constraint_name(table: &str, column: &str) -> String {
    // Build in one exact-size allocation like the sibling constraint-name
    // builders above, instead of routing through `format!`. Output is
    // byte-identical: `chk_{table}__{column}` (`"chk_"` = 4 bytes, `"__"` = 2).
    let mut out = String::with_capacity(4 + table.len() + 2 + column.len());
    out.push_str("chk_");
    out.push_str(table);
    out.push_str("__");
    out.push_str(column);
    out
}

/// Generate enum type name with table prefix to avoid conflicts.
/// Always includes table name to ensure uniqueness across tables.
/// Format: {table}_{`enum_name`}
///
/// This prevents conflicts when multiple tables use the same enum name
/// (e.g., "status" or "gender") with potentially different values.
pub fn build_enum_type_name(table: &str, enum_name: &str) -> String {
    format!("{table}_{enum_name}")
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    // ========================================================================
    // Constraint Naming Tests
    // ========================================================================

    /// The three builders monomorphized for `&[&str]` columns.
    /// Column strings are `'static` so the generic builders (whose column
    /// lifetime is fixed at instantiation) can coerce to a single fn pointer.
    type BuildFn = fn(&str, &[&'static str], Option<&str>) -> String;
    /// The three builders monomorphized for `&[String]` columns.
    type BuildStringFn = fn(&str, &[String], Option<&str>) -> String;

    #[rstest]
    #[case::index_with_key(build_index_name, "users", &["email"][..], Some("email_idx"), "ix_users__email_idx")]
    #[case::index_without_key(build_index_name, "users", &["email"][..], None, "ix_users__email")]
    #[case::index_multiple_columns(build_index_name, "users", &["first_name", "last_name"][..], None, "ix_users__first_name_last_name")]
    #[case::unique_with_key(build_unique_constraint_name, "users", &["email"][..], Some("email_unique"), "uq_users__email_unique")]
    #[case::unique_without_key(build_unique_constraint_name, "users", &["email"][..], None, "uq_users__email")]
    #[case::foreign_key_with_key(build_foreign_key_name, "posts", &["user_id"][..], Some("fk_user"), "fk_posts__fk_user")]
    #[case::foreign_key_without_key(build_foreign_key_name, "posts", &["user_id"][..], None, "fk_posts__user_id")]
    fn constraint_name_builders_render_expected(
        #[case] build: BuildFn,
        #[case] table: &str,
        #[case] columns: &[&'static str],
        #[case] key: Option<&str>,
        #[case] expected: &str,
    ) {
        assert_eq!(build(table, columns, key), expected);
    }

    /// Column order must not affect the generated name (`&[&str]` instantiation).
    #[rstest]
    #[case::index(build_index_name, "users", &["last_name", "first_name"][..])]
    #[case::unique(build_unique_constraint_name, "users", &["last_name", "first_name"][..])]
    #[case::foreign_key(build_foreign_key_name, "posts", &["tenant_id", "user_id"][..])]
    fn constraint_name_builders_sort_str_columns_for_deterministic_name(
        #[case] build: BuildFn,
        #[case] table: &str,
        #[case] columns: &[&'static str],
    ) {
        let mut reversed = columns.to_vec();
        reversed.reverse();
        assert_eq!(build(table, columns, None), build(table, &reversed, None));
    }

    /// Column order must not affect the generated name (`&[String]` instantiation).
    #[rstest]
    #[case::index(build_index_name, "users", &["last_name", "first_name"][..])]
    #[case::unique(build_unique_constraint_name, "order_items", &["product_id", "order_id"][..])]
    #[case::foreign_key(build_foreign_key_name, "memberships", &["tenant_id", "account_id"][..])]
    fn constraint_name_builders_sort_string_columns_for_deterministic_name(
        #[case] build: BuildStringFn,
        #[case] table: &str,
        #[case] columns: &[&str],
    ) {
        let columns: Vec<String> = columns.iter().map(ToString::to_string).collect();
        let mut reversed = columns.clone();
        reversed.reverse();
        assert_eq!(build(table, &columns, None), build(table, &reversed, None));
    }

    #[test]
    fn test_build_check_constraint_name() {
        assert_eq!(
            build_check_constraint_name("users", "status"),
            "chk_users__status"
        );
    }

    #[test]
    fn test_build_enum_type_name() {
        assert_eq!(build_enum_type_name("users", "status"), "users_status");
    }

    #[test]
    fn test_build_enum_type_name_with_existing_prefix() {
        // Even if enum_name already has table prefix, we add it
        // User should provide clean enum name (e.g., "status" not "users_status")
        assert_eq!(
            build_enum_type_name("users", "user_status"),
            "users_user_status"
        );
    }

    #[test]
    fn test_build_enum_type_name_prevents_conflicts() {
        // Different tables can have same enum name without conflict
        assert_eq!(build_enum_type_name("users", "gender"), "users_gender");
        assert_eq!(
            build_enum_type_name("employees", "gender"),
            "employees_gender"
        );

        assert_eq!(build_enum_type_name("orders", "status"), "orders_status");
        assert_eq!(
            build_enum_type_name("shipments", "status"),
            "shipments_status"
        );
    }
}
