//! Newtype wrappers for schema identifiers (tables, columns, indexes).
//!
//! These wrap `String` to provide compile-time type safety: a function
//! taking `TableName` cannot accidentally receive a `ColumnName`. Wire
//! format is preserved exactly via `#[serde(transparent)]` — JSON
//! migration scripts, schema files, and CLI output deserialize/serialize
//! byte-identically with the previous String-alias version.
//!
//! Convention: always `snake_case`, enforced by CLI / planner naming
//! validation rather than by the type system.

use std::fmt;

/// The name of a database table, always in `snake_case` by convention.
///
/// Construction:
///
/// ```rust
/// use vespertide_core::schema::names::TableName;
///
/// let via_new: TableName = TableName::new("user");
/// let via_from: TableName = "user".into();
///
/// assert_eq!(via_new.as_str(), "user");
/// assert!(via_new == "user");
/// assert_eq!(via_new.to_string(), "user");
/// assert_eq!(via_new, via_from);
/// ```
///
/// JSON wire format is byte-identical to a plain `String` thanks to
/// `#[serde(transparent)]`. See [`ColumnName`] for a serde round-trip example.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
#[cfg_attr(feature = "schema", schemars(transparent))]
pub struct TableName(String);

/// The name of a table column, always in `snake_case` by convention.
///
/// Construction and serde round-trip:
///
/// ```rust
/// use vespertide_core::schema::names::ColumnName;
///
/// let col: ColumnName = ColumnName::new("email");
/// let via_from: ColumnName = "email".into();
///
/// assert_eq!(col.as_str(), "email");
/// assert!(col == "email");
/// assert_eq!(col, via_from);
///
/// // Wire format is byte-identical to a plain JSON string.
/// let json = serde_json::to_string(&col).unwrap();
/// assert_eq!(json, r#""email""#);
/// let back: ColumnName = serde_json::from_str(&json).unwrap();
/// assert_eq!(back, col);
/// ```
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
#[cfg_attr(feature = "schema", schemars(transparent))]
pub struct ColumnName(String);

/// The name of a database index, conventionally `ix_{table}__{columns}`.
///
/// Construction:
///
/// ```rust
/// use vespertide_core::schema::names::IndexName;
///
/// let idx: IndexName = IndexName::new("ix_user__email");
/// let via_from: IndexName = "ix_user__email".into();
///
/// assert_eq!(idx.as_str(), "ix_user__email");
/// assert!(idx == "ix_user__email");
/// assert_eq!(idx.to_string(), "ix_user__email");
/// assert_eq!(idx, via_from);
/// ```
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
#[cfg_attr(feature = "schema", schemars(transparent))]
pub struct IndexName(String);

// Implement common ergonomics. Each newtype gets the same impl block via a
// declarative macro to avoid 60 lines of triplication.
macro_rules! impl_name_newtype {
    ($ty:ident) => {
        impl $ty {
            #[must_use]
            pub fn new(s: impl Into<String>) -> Self {
                Self(s.into())
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            #[must_use]
            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl From<String> for $ty {
            fn from(s: String) -> Self {
                Self(s)
            }
        }

        impl From<&str> for $ty {
            fn from(s: &str) -> Self {
                Self(s.to_string())
            }
        }

        impl From<$ty> for String {
            fn from(t: $ty) -> Self {
                t.0
            }
        }

        impl From<&$ty> for String {
            fn from(t: &$ty) -> Self {
                t.0.clone()
            }
        }

        impl AsRef<str> for $ty {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl std::ops::Deref for $ty {
            type Target = str;
            fn deref(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl fmt::Debug for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Debug::fmt(&self.0, f)
            }
        }

        impl std::borrow::Borrow<str> for $ty {
            fn borrow(&self) -> &str {
                &self.0
            }
        }

        impl PartialEq<str> for $ty {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }

        impl PartialEq<&str> for $ty {
            fn eq(&self, other: &&str) -> bool {
                &self.0 == *other
            }
        }

        impl PartialEq<String> for $ty {
            fn eq(&self, other: &String) -> bool {
                &self.0 == other
            }
        }
    };
}

impl_name_newtype!(TableName);
impl_name_newtype!(ColumnName);
impl_name_newtype!(IndexName);

/// Join a slice of [`ColumnName`]s with a separator using a single buffer
/// — no intermediate `Vec<String>` allocation.
///
/// Mirrors the buffer-push pattern used by
/// `vespertide_query::sql::helpers::quote_idents`. Folds four open-coded
/// `cols.iter().map(...).collect::<Vec<_>>().join(sep)` callsites in
/// `vespertide-planner::validate::*` into a single shared helper.
///
/// ```rust
/// use vespertide_core::schema::names::{ColumnName, join_column_names};
///
/// let cols: Vec<ColumnName> = vec!["a".into(), "b".into(), "c".into()];
/// assert_eq!(join_column_names(&cols, ", "), "a, b, c");
/// assert_eq!(join_column_names(&cols, "_"), "a_b_c");
/// assert_eq!(join_column_names(&[], ", "), "");
/// ```
#[must_use]
pub fn join_column_names(columns: &[ColumnName], separator: &str) -> String {
    let mut out = String::new();
    for (i, c) in columns.iter().enumerate() {
        if i > 0 {
            out.push_str(separator);
        }
        out.push_str(c.as_str());
    }
    out
}

#[cfg(test)]
mod tests {
    //! Coverage-closure tests for the `impl_name_newtype!` expansions.
    //! Tarpaulin attributes hits at the macro definition lines (91, 92 for
    //! `new`, 119, 120 for `From<$ty> for String`). Doctests do not run
    //! under tarpaulin, so we exercise the same paths from real `#[test]`s.
    use super::*;

    #[test]
    fn table_name_new_constructs_from_str_literal() {
        // Covers lines 91, 92 via TableName::new.
        let name = TableName::new("user");
        assert_eq!(name.as_str(), "user");
    }

    #[test]
    fn column_name_new_constructs_from_owned_string() {
        // Covers lines 91, 92 via ColumnName::new (different newtype).
        let name = ColumnName::new(String::from("email"));
        assert_eq!(name.as_str(), "email");
    }

    #[test]
    fn index_name_new_constructs_from_str_ref() {
        // Covers lines 91, 92 via IndexName::new.
        let owned = "ix_user__email".to_string();
        let name = IndexName::new(&*owned);
        assert_eq!(name.as_str(), "ix_user__email");
    }

    #[test]
    fn table_name_into_string_via_from() {
        // Covers lines 119, 120 (`From<TableName> for String`).
        let name = TableName::new("orders");
        let s: String = String::from(name);
        assert_eq!(s, "orders");
    }

    #[test]
    fn column_name_into_string_via_from() {
        // Covers lines 119, 120 (`From<ColumnName> for String`).
        let name = ColumnName::new("created_at");
        let s: String = String::from(name);
        assert_eq!(s, "created_at");
    }

    #[test]
    fn index_name_into_string_via_from() {
        // Covers lines 119, 120 (`From<IndexName> for String`).
        let name = IndexName::new("ix_orders__id");
        let s: String = String::from(name);
        assert_eq!(s, "ix_orders__id");
    }

    #[test]
    fn join_column_names_empty_returns_empty_string() {
        // Empty-slice path: `for ... in []` never runs, returns the
        // pristine `String::new()`. Locks the empty-input contract that
        // the deleted `validate::constraint_drops::join_columns` helper
        // used to assert directly.
        let cols: Vec<ColumnName> = vec![];
        assert_eq!(join_column_names(&cols, ", "), "");
        assert_eq!(join_column_names(&cols, "_"), "");
    }

    #[test]
    fn join_column_names_comma_separator() {
        // The validate-diagnostic site shape: `"a, b, c"`. Single-column
        // case must skip the separator entirely (no leading `", "`).
        let cols: Vec<ColumnName> = vec!["a".into(), "b".into(), "c".into()];
        assert_eq!(join_column_names(&cols, ", "), "a, b, c");

        let single: Vec<ColumnName> = vec!["only".into()];
        assert_eq!(join_column_names(&single, ", "), "only");
    }

    #[test]
    fn join_column_names_underscore_separator() {
        // The `ix_{table}__{cols}` index-name builder shape: `"fk_id"`.
        // Locks the separator variation against the validate
        // `build_suggested_index_name` callsite.
        let cols: Vec<ColumnName> = vec!["fk".into(), "id".into()];
        assert_eq!(join_column_names(&cols, "_"), "fk_id");

        let composite: Vec<ColumnName> = vec!["tenant_id".into(), "user_id".into()];
        assert_eq!(join_column_names(&composite, "_"), "tenant_id_user_id");
    }
}
