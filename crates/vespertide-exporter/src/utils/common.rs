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
}
