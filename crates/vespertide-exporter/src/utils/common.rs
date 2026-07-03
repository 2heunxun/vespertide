//! Cross-language helpers shared by every ORM exporter backend.

/// Join items as a double-quoted, comma-separated list: `"a", "b", "c"`.
///
/// Consolidates the quoted-comma-join pattern previously copy-pasted across
/// the JPA, `SeaORM`, `SQLAlchemy` and `SQLModel` renderers, and builds the
/// result in a single buffer instead of collecting an intermediate
/// `Vec<String>` per call site.
pub(crate) fn join_quoted<T: AsRef<str>>(items: &[T]) -> String {
    let mut out = String::new();
    for item in items {
        if !out.is_empty() {
            out.push_str(", ");
        }
        out.push('"');
        out.push_str(item.as_ref());
        out.push('"');
    }
    out
}

/// Append one comma-separated attribute `fragment` into `buf`, inserting a
/// `", "` separator before every fragment except the first.
///
/// Replaces the `Vec<String>` + `.join(", ")` pattern used to assemble ORM
/// column-attribute lists (`@Column(name = ..., nullable = false)`,
/// `mapped_column(String, primary_key=True)`): fragments are written straight
/// into a single buffer with no intermediate `Vec` or per-fragment `String`
/// allocations. Output is byte-identical to `fragments.join(", ")`.
pub(crate) fn push_attr(buf: &mut String, fragment: &str) {
    if !buf.is_empty() {
        buf.push_str(", ");
    }
    buf.push_str(fragment);
}

/// Join FK-target columns as a double-quoted, `ref_table`-qualified,
/// comma-separated list: `"tbl.col1", "tbl.col2"`.
///
/// Consolidates the identical `ForeignKeyConstraint([...], [...])` target
/// rendering previously copy-pasted across the `SQLAlchemy` and `SQLModel`
/// renderers, and builds the result in a single buffer instead of collecting an
/// intermediate `Vec<String>` (one `String` per column) per call site.
pub(crate) fn join_qualified_refs(ref_table: &str, ref_cols: &[&str]) -> String {
    let mut out = String::new();
    for col in ref_cols {
        if !out.is_empty() {
            out.push_str(", ");
        }
        out.push('"');
        out.push_str(ref_table);
        out.push('.');
        out.push_str(col);
        out.push('"');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_slice_yields_empty_string() {
        assert_eq!(join_quoted::<&str>(&[]), "");
    }

    #[test]
    fn single_item_is_quoted_without_separator() {
        assert_eq!(join_quoted(&["id"]), "\"id\"");
    }

    #[test]
    fn multiple_items_are_comma_separated() {
        assert_eq!(join_quoted(&["a", "b", "c"]), "\"a\", \"b\", \"c\"");
    }

    #[test]
    fn qualified_refs_empty_slice_yields_empty_string() {
        assert_eq!(join_qualified_refs("user", &[]), "");
    }

    #[test]
    fn qualified_refs_single_column_is_table_qualified() {
        assert_eq!(join_qualified_refs("user", &["id"]), "\"user.id\"");
    }

    #[test]
    fn qualified_refs_multiple_columns_are_comma_separated() {
        assert_eq!(
            join_qualified_refs("account", &["tenant_id", "id"]),
            "\"account.tenant_id\", \"account.id\""
        );
    }

    #[test]
    fn push_attr_matches_join_semantics() {
        let mut buf = String::new();
        push_attr(&mut buf, "name = \"id\"");
        assert_eq!(buf, "name = \"id\"");
        push_attr(&mut buf, "nullable = false");
        push_attr(&mut buf, "unique = true");
        assert_eq!(buf, "name = \"id\", nullable = false, unique = true");
        assert_eq!(
            buf,
            ["name = \"id\"", "nullable = false", "unique = true"].join(", ")
        );
    }

    #[test]
    fn push_attr_first_fragment_has_no_leading_separator() {
        let mut buf = String::new();
        push_attr(&mut buf, "String");
        assert_eq!(buf, "String");
    }
}
