//! Completion context detection via tree-sitter node ancestry.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Context {
    ColumnType,
    Nullable,
    PrimaryKey,
    Unique,
    OnDeleteAction,
    OnUpdateAction,
    RefTable,
    RefColumns { ref_table: String },
    None,
}

pub(super) fn detect(tree: &tree_sitter::Tree, source: &str, byte_offset: usize) -> Context {
    let Some(node) = node_at_byte(tree, byte_offset) else {
        return Context::None;
    };

    let path = collect_key_path(node, source);
    classify_path(&path, node, source)
}

fn classify_path(path: &[String], cursor_node: tree_sitter::Node<'_>, source: &str) -> Context {
    let last = path.last().map(String::as_str);
    let has = |key: &str| path.iter().any(|part| part == key);

    match last {
        Some("type") if has("columns") => Context::ColumnType,
        Some("nullable") if has("columns") => Context::Nullable,
        Some("primary_key") if has("columns") => Context::PrimaryKey,
        Some("unique") if has("columns") => Context::Unique,
        Some("on_delete") => Context::OnDeleteAction,
        Some("on_update") => Context::OnUpdateAction,
        Some("ref_table") => Context::RefTable,
        Some("ref_columns") => Context::RefColumns {
            ref_table: sibling_ref_table(cursor_node, source).unwrap_or_default(),
        },
        _ => Context::None,
    }
}

fn collect_key_path(node: tree_sitter::Node<'_>, source: &str) -> Vec<String> {
    let mut path = Vec::new();
    let mut current = Some(node);

    while let Some(candidate) = current {
        if is_pair(candidate)
            && let Some(key) = key_text(candidate, source)
        {
            path.push(key.to_string());
        }
        current = candidate.parent();
    }

    path.reverse();
    path
}

fn sibling_ref_table(node: tree_sitter::Node<'_>, source: &str) -> Option<String> {
    let ref_columns_pair = enclosing_pair_with_key(node, source, "ref_columns")?;
    let parent = ref_columns_pair.parent()?;
    direct_child_value(parent, source, "ref_table").map(ToString::to_string)
}

fn enclosing_pair_with_key<'tree>(
    node: tree_sitter::Node<'tree>,
    source: &str,
    expected: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut current = Some(node);

    while let Some(candidate) = current {
        if is_pair(candidate) && key_text(candidate, source) == Some(expected) {
            return Some(candidate);
        }
        current = candidate.parent();
    }

    None
}

fn direct_child_value<'source>(
    node: tree_sitter::Node<'_>,
    source: &'source str,
    expected_key: &str,
) -> Option<&'source str> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_pair(child) && key_text(child, source) == Some(expected_key) {
            return value_text(child, source);
        }
    }

    None
}

fn key_text<'source>(
    pair_node: tree_sitter::Node<'_>,
    source: &'source str,
) -> Option<&'source str> {
    let key = pair_node.named_child(0)?;
    source.get(key.byte_range()).map(strip_quotes)
}

fn value_text<'source>(
    pair_node: tree_sitter::Node<'_>,
    source: &'source str,
) -> Option<&'source str> {
    let value = pair_node.named_child(1)?;
    source.get(value.byte_range()).map(strip_quotes)
}

fn node_at_byte(tree: &tree_sitter::Tree, byte_offset: usize) -> Option<tree_sitter::Node<'_>> {
    let root = tree.root_node();
    if root.end_byte() == 0 {
        return Some(root);
    }

    let start = byte_offset
        .saturating_sub(1)
        .min(root.end_byte().saturating_sub(1));
    let end = byte_offset.min(root.end_byte());
    root.descendant_for_byte_range(start, end).or(Some(root))
}

fn is_pair(node: tree_sitter::Node<'_>) -> bool {
    matches!(node.kind(), "pair" | "block_mapping_pair")
}

fn strip_quotes(text: &str) -> &str {
    text.trim()
        .trim_start_matches('"')
        .trim_end_matches('"')
        .trim_start_matches('\'')
        .trim_end_matches('\'')
}
