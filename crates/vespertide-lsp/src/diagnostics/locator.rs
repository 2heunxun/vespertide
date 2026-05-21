//! Locate planner validation errors in source text.

use std::ops::Range;

use vespertide_planner::PlannerError;

/// Structured location extracted from a planner error.
pub(super) struct ErrorLocation {
    /// Table responsible for the diagnostic.
    pub table: String,
    /// Column responsible for the diagnostic, when the planner provides one.
    pub column: Option<String>,
    /// Constraint or index responsible for the diagnostic, when available.
    pub constraint: Option<String>,
}

impl ErrorLocation {
    /// Extract the table/column/constraint tuple carried by a planner error.
    pub fn from_planner_error(err: &PlannerError) -> Option<Self> {
        use PlannerError::{
            ColumnExists, ColumnNotFound, ConstraintColumnNotFound, DuplicateEnumValue,
            DuplicateEnumVariantName, DuplicateTableName, EmptyConstraintColumns,
            ForeignKeyColumnNotFound, ForeignKeyTableNotFound, IndexColumnNotFound, IndexNotFound,
            InvalidAutoIncrement, InvalidEnumDefault, MissingFillWith, MissingPrimaryKey,
            TableExists, TableNotFound, TableValidation,
        };

        match err {
            TableExists(table)
            | TableNotFound(table)
            | DuplicateTableName(table)
            | MissingPrimaryKey(table) => Some(Self::table(table)),
            TableValidation(_) => None,
            ColumnExists(table, column)
            | ColumnNotFound(table, column)
            | MissingFillWith(table, column)
            | ForeignKeyTableNotFound(table, column, _)
            | ForeignKeyColumnNotFound(table, column, _, _)
            | DuplicateEnumVariantName(_, table, column, _)
            | DuplicateEnumValue(_, table, column, _)
            | InvalidAutoIncrement(table, column, _) => Some(Self::column(table, column)),
            IndexNotFound(table, index) | EmptyConstraintColumns(table, index) => {
                Some(Self::constraint(table, index))
            }
            IndexColumnNotFound(table, index, column)
            | ConstraintColumnNotFound(table, index, column) => Some(Self {
                table: table.clone(),
                column: Some(column.clone()),
                constraint: Some(index.clone()),
            }),
            InvalidEnumDefault(err) => Some(Self {
                table: err.table_name.clone(),
                column: Some(err.column_name.clone()),
                constraint: None,
            }),
        }
    }

    fn table(table: &str) -> Self {
        Self {
            table: table.to_string(),
            column: None,
            constraint: None,
        }
    }

    fn column(table: &str, column: &str) -> Self {
        Self {
            table: table.to_string(),
            column: Some(column.to_string()),
            constraint: None,
        }
    }

    fn constraint(table: &str, constraint: &str) -> Self {
        Self {
            table: table.to_string(),
            column: None,
            constraint: Some(constraint.to_string()),
        }
    }
}

/// Find the source range for a named column object.
///
/// Falls back to the table's top-level `name` value, then `0..1`.
pub(super) fn locate_column(
    tree: &tree_sitter::Tree,
    source: &str,
    column_name: &str,
) -> Range<usize> {
    locate_named_object(tree, source, "columns", column_name)
        .or_else(|| locate_top_name(tree, source))
        .unwrap_or(0..1)
}

/// Find the source range for a named constraint object.
///
/// Falls back to the table's top-level `name` value, then `0..1`.
pub(super) fn locate_constraint(
    tree: &tree_sitter::Tree,
    source: &str,
    constraint_name: &str,
) -> Range<usize> {
    locate_named_object(tree, source, "constraints", constraint_name)
        .or_else(|| locate_top_name(tree, source))
        .unwrap_or(0..1)
}

/// Find the source range for the top-level `name` value.
pub(super) fn locate_top_name(tree: &tree_sitter::Tree, source: &str) -> Option<Range<usize>> {
    walk_for_name_pair(tree.root_node(), source.as_bytes())
}

fn locate_named_object(
    tree: &tree_sitter::Tree,
    source: &str,
    collection_key: &str,
    target_name: &str,
) -> Option<Range<usize>> {
    let collection = find_value_for_key(tree.root_node(), source.as_bytes(), collection_key)?;
    walk_for_named_mapping(collection, source.as_bytes(), target_name)
}

fn find_value_for_key<'tree>(
    node: tree_sitter::Node<'tree>,
    source: &[u8],
    target_key: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_pair(child)
            && pair_key_matches(child, source, target_key)
            && let Some(value) = child.named_child(1)
        {
            return Some(value);
        }
        if let Some(value) = find_value_for_key(child, source, target_key) {
            return Some(value);
        }
    }
    None
}

fn walk_for_named_mapping(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    target_name: &str,
) -> Option<Range<usize>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_mapping(child) && mapping_has_name(child, source, target_name) {
            return Some(child.byte_range());
        }
        if let Some(range) = walk_for_named_mapping(child, source, target_name) {
            return Some(range);
        }
    }
    None
}

fn mapping_has_name(node: tree_sitter::Node<'_>, source: &[u8], target_name: &str) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if !is_pair(child) || !pair_key_matches(child, source, "name") {
            continue;
        }
        let Some(value) = child.named_child(1) else {
            continue;
        };
        if node_text(value, source).is_some_and(|text| strip_quotes(text) == target_name) {
            return true;
        }
    }
    false
}

fn walk_for_name_pair(node: tree_sitter::Node<'_>, source: &[u8]) -> Option<Range<usize>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_pair(child)
            && pair_key_matches(child, source, "name")
            && let Some(value) = child.named_child(1)
        {
            return Some(value.byte_range());
        }
        if let Some(range) = walk_for_name_pair(child, source) {
            return Some(range);
        }
    }
    None
}

fn is_mapping(node: tree_sitter::Node<'_>) -> bool {
    matches!(node.kind(), "object" | "block_mapping")
}

fn is_pair(node: tree_sitter::Node<'_>) -> bool {
    matches!(node.kind(), "pair" | "block_mapping_pair")
}

fn pair_key_matches(node: tree_sitter::Node<'_>, source: &[u8], expected: &str) -> bool {
    let Some(key) = node.named_child(0) else {
        return false;
    };
    node_text(key, source).is_some_and(|text| strip_quotes(text) == expected)
}

fn node_text<'a>(node: tree_sitter::Node<'_>, source: &'a [u8]) -> Option<&'a str> {
    std::str::from_utf8(&source[node.byte_range()]).ok()
}

fn strip_quotes(s: &str) -> &str {
    let trimmed = s.trim();
    trimmed
        .strip_prefix('"')
        .and_then(|without_prefix| without_prefix.strip_suffix('"'))
        .or_else(|| {
            trimmed
                .strip_prefix('\'')
                .and_then(|without_prefix| without_prefix.strip_suffix('\''))
        })
        .unwrap_or(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DocumentFormat, ParserPool};

    #[test]
    fn locate_column_finds_target_byte_range() {
        let pool = ParserPool::new();
        let src = r#"{"name":"user","columns":[{"name":"id","type":"integer"},{"name":"email","type":"text"}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();
        let range = locate_column(&tree, src, "email");
        let snippet = &src[range];

        assert!(snippet.contains(r#""email""#), "got: {snippet}");
    }

    #[test]
    fn locate_column_fallback_to_top_name() {
        let pool = ParserPool::new();
        let src = r#"{"name":"user","columns":[]}"#;
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();
        let range = locate_column(&tree, src, "nonexistent");

        assert!(src[range].contains("user"));
    }
}
