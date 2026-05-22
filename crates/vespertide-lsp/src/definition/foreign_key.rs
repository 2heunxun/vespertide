//! Foreign-key go-to-definition for `ref_table` and `ref_columns` values.
//!
//! Behaviour matrix:
//! | Cursor sits in                          | Resolves to                                |
//! |-----------------------------------------|--------------------------------------------|
//! | `ref_table: "<X>"`                       | top-level `name` of table X                |
//! | `ref_columns: ["<Y>", ...]` (any entry)  | column named Y inside the FK's target table|
//!
//! The target file may be open in the editor or sit only on disk — both
//! resolve. For disk-only targets we point at byte `0..0` because parsing
//! the file lives outside this module; the client opens it and the user
//! sees the document.

use std::ops::Range;
use std::path::Path;
use std::str::FromStr;

use tower_lsp_server::ls_types::Uri;

use crate::store::DocumentStore;
use crate::workspace_index::WorkspaceIndex;
use crate::workspace_tables::WorkspaceTables;

use super::DomainLocation;

/// Strategy that locates the precise byte range inside the target file.
enum TargetLookup<'a> {
    /// Top-level `name` value of the table.
    TableName,
    /// A specific column inside the `columns` array.
    Column(&'a str),
}

pub(super) fn try_definition(
    node: tree_sitter::Node<'_>,
    source: &str,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
    disk_tables: Option<&WorkspaceTables>,
) -> Option<DomainLocation> {
    if let Some(loc) = try_ref_table_definition(node, source, index, docs, disk_tables) {
        return Some(loc);
    }
    try_ref_columns_definition(node, source, index, docs, disk_tables)
}

fn try_ref_table_definition(
    node: tree_sitter::Node<'_>,
    source: &str,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
    disk_tables: Option<&WorkspaceTables>,
) -> Option<DomainLocation> {
    let pair = enclosing_pair_with_key(node, source, "ref_table")?;
    let value = pair.named_child(1)?;
    let target_name = strip_quotes(&source[value.byte_range()]).to_string();

    resolve_target(
        &target_name,
        &TargetLookup::TableName,
        index,
        docs,
        disk_tables,
    )
}

fn try_ref_columns_definition(
    node: tree_sitter::Node<'_>,
    source: &str,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
    disk_tables: Option<&WorkspaceTables>,
) -> Option<DomainLocation> {
    // Locate the enclosing `ref_columns` pair. Skipping ancestry-by-kind
    // would be fragile across YAML (where inline `[x]` parses as a
    // `flow_node`, not a `flow_sequence`) — finding the pair directly is
    // grammar-agnostic.
    let string_node = enclosing_string(node)?;
    let ref_columns_pair = enclosing_pair_with_key(string_node, source, "ref_columns")?;
    let ref_columns_value = ref_columns_pair.named_child(1)?;
    // Ensure the cursor sits in the VALUE side of the pair, not the key.
    if !ref_columns_value
        .byte_range()
        .contains(&string_node.start_byte())
    {
        return None;
    }

    let column_name = strip_quotes(&source[string_node.byte_range()]).to_string();
    let fk_object_raw = ref_columns_pair.parent()?;
    let fk_object = skip_yaml_wrappers(fk_object_raw)?;
    let ref_table_value = direct_child_value(fk_object, source, "ref_table")?;
    let target_table = strip_quotes(ref_table_value).to_string();

    resolve_target(
        &target_table,
        &TargetLookup::Column(&column_name),
        index,
        docs,
        disk_tables,
    )
}

fn resolve_target(
    table_name: &str,
    lookup: &TargetLookup<'_>,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
    disk_tables: Option<&WorkspaceTables>,
) -> Option<DomainLocation> {
    // Prefer the open document (may carry unsaved edits with accurate ranges).
    if let Some(loc) = index.lookup(table_name) {
        let byte_range = docs
            .with_doc(&loc.uri, |text, tree| {
                let tree = tree?;
                match lookup {
                    TargetLookup::TableName => find_top_level_name_range(tree, text),
                    TargetLookup::Column(column) => find_column_name_range(tree, text, column),
                }
            })
            .flatten()
            .unwrap_or(0..0);
        return Some(DomainLocation {
            uri: loc.uri,
            byte_range,
        });
    }

    // Fall back to the on-disk model so closed files still navigate.
    let path = disk_tables?.model_path(table_name)?;
    let uri = path_to_file_uri(&path)?;
    Some(DomainLocation {
        uri,
        byte_range: 0..0,
    })
}

/// Skip past tree-sitter-yaml's pure wrapper nodes (`flow_node`,
/// `block_node`). Returns the first ancestor that has a meaningful kind.
fn skip_yaml_wrappers(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    let mut current = node;
    while matches!(current.kind(), "flow_node" | "block_node") {
        current = current.parent()?;
    }
    Some(current)
}

fn enclosing_string(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    let mut current = Some(node);
    while let Some(candidate) = current {
        match candidate.kind() {
            "string"
            | "double_quote_scalar"
            | "single_quote_scalar"
            | "string_scalar"
            | "plain_scalar" => return Some(candidate),
            // String_content is the inner span without quotes; climb to the
            // surrounding `string` node so the parent is the JSON array.
            "string_content" => return candidate.parent(),
            "array" | "object" | "pair" | "block_mapping_pair" | "block_mapping"
            | "block_sequence" | "flow_mapping" | "flow_sequence" => return None,
            _ => {}
        }
        current = candidate.parent();
    }
    None
}

fn enclosing_pair_with_key<'tree>(
    node: tree_sitter::Node<'tree>,
    source: &str,
    expected_key: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut current = Some(node);
    while let Some(candidate) = current {
        if is_pair(candidate) && pair_key_is(candidate, source, expected_key) {
            return Some(candidate);
        }
        current = candidate.parent();
    }
    None
}

fn pair_key_is(pair: tree_sitter::Node<'_>, source: &str, expected: &str) -> bool {
    pair.named_child(0)
        .is_some_and(|key| strip_quotes(&source[key.byte_range()]) == expected)
}

fn direct_child_value<'a>(
    object: tree_sitter::Node<'_>,
    source: &'a str,
    target_key: &str,
) -> Option<&'a str> {
    let mut cursor = object.walk();
    for child in object.children(&mut cursor) {
        if is_pair(child) && pair_key_is(child, source, target_key) {
            let value = child.named_child(1)?;
            return Some(&source[value.byte_range()]);
        }
    }
    None
}

fn find_column_name_range(
    tree: &tree_sitter::Tree,
    text: &str,
    column_name: &str,
) -> Option<Range<usize>> {
    let columns_value = find_columns_array(tree.root_node(), text.as_bytes())?;
    walk_for_named_column(columns_value, text.as_bytes(), column_name)
}

fn find_columns_array<'tree>(
    node: tree_sitter::Node<'tree>,
    source: &[u8],
) -> Option<tree_sitter::Node<'tree>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_pair(child) && key_is(child, source, "columns") {
            return child.named_child(1);
        }
        if let Some(found) = find_columns_array(child, source) {
            return Some(found);
        }
    }
    None
}

fn walk_for_named_column(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    column_name: &str,
) -> Option<Range<usize>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_mapping(child)
            && let Some(name_pair) = direct_named_child_pair(child, source, "name")
            && let Some(name_value) = name_pair.named_child(1)
        {
            let raw = std::str::from_utf8(&source[name_value.byte_range()]).unwrap_or("");
            if strip_quotes(raw) == column_name {
                // Highlight the column's `name` value range — that's where
                // the user expects the cursor to land.
                return Some(name_value.byte_range());
            }
        }
        if let Some(range) = walk_for_named_column(child, source, column_name) {
            return Some(range);
        }
    }
    None
}

fn direct_named_child_pair<'tree>(
    object: tree_sitter::Node<'tree>,
    source: &[u8],
    target_key: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut cursor = object.walk();
    object
        .children(&mut cursor)
        .find(|&child| is_pair(child) && key_is(child, source, target_key))
}

fn path_to_file_uri(path: &Path) -> Option<Uri> {
    let path_str = path.to_str()?;
    // Normalize to forward slashes for the URI; on Windows the drive letter
    // gets a leading slash so `C:\a\b` becomes `file:///C:/a/b`.
    let normalized = path_str.replace('\\', "/");
    let url = if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else {
        format!("file:///{normalized}")
    };
    Uri::from_str(&url).ok()
}

fn find_top_level_name_range(tree: &tree_sitter::Tree, text: &str) -> Option<Range<usize>> {
    let root = tree.root_node();
    let mapping = first_mapping(root)?;
    find_direct_name_range(mapping, text.as_bytes())
        .or_else(|| walk_for_name(root, text.as_bytes()))
}

fn first_mapping(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    if is_mapping(node) {
        return Some(node);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = first_mapping(child) {
            return Some(found);
        }
    }
    None
}

fn find_direct_name_range(node: tree_sitter::Node<'_>, source: &[u8]) -> Option<Range<usize>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_pair(child) && key_is(child, source, "name") {
            let value = child.named_child(1)?;
            return Some(value.byte_range());
        }
    }
    None
}

fn walk_for_name(node: tree_sitter::Node<'_>, source: &[u8]) -> Option<Range<usize>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_pair(child) && key_is(child, source, "name") {
            let value = child.named_child(1)?;
            return Some(value.byte_range());
        }
        if let Some(found) = walk_for_name(child, source) {
            return Some(found);
        }
    }
    None
}

fn key_is(node: tree_sitter::Node<'_>, source: &[u8], expected: &str) -> bool {
    let Some(key) = node.named_child(0) else {
        return false;
    };
    let text = &source[key.byte_range()];
    let key_str = std::str::from_utf8(text).unwrap_or("");
    strip_quotes(key_str) == expected
}

fn is_mapping(node: tree_sitter::Node<'_>) -> bool {
    matches!(node.kind(), "object" | "block_mapping")
}

fn is_pair(node: tree_sitter::Node<'_>) -> bool {
    matches!(node.kind(), "pair" | "block_mapping_pair")
}

fn strip_quotes(s: &str) -> &str {
    s.trim()
        .trim_start_matches('"')
        .trim_end_matches('"')
        .trim_start_matches('\'')
        .trim_end_matches('\'')
}
