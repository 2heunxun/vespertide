//! Resolve "what symbol is the cursor on?" for the references provider.

use tower_lsp_server::ls_types::Uri;

use super::ReferenceSymbol;

/// Walk up from the cursor and decide whether it sits on a table or column
/// reference. Returns `None` for non-reference positions.
pub(super) fn resolve(
    source: &str,
    tree: Option<&tree_sitter::Tree>,
    current_uri: &Uri,
    byte_offset: usize,
) -> Option<ReferenceSymbol> {
    let _ = current_uri;
    let tree = tree?;
    let node = node_at_byte(tree, byte_offset)?;
    let string_node = enclosing_string(node)?;
    let raw = source.get(string_node.byte_range())?;
    let symbol_text = strip_quotes(raw);
    if symbol_text.is_empty() {
        return None;
    }

    // What pair owns the string?
    let pair = enclosing_pair(string_node)?;
    let key = pair.named_child(0)?;
    let key_text = strip_quotes(source.get(key.byte_range())?);

    // Make sure the cursor is on the VALUE side, not the key side.
    let value = pair.named_child(1)?;
    if !value.byte_range().contains(&string_node.start_byte()) {
        return None;
    }

    match key_text {
        // Top-level table name OR foreign_key.ref_table.
        "name" if is_top_level_pair(pair) => Some(ReferenceSymbol::Table {
            name: symbol_text.to_string(),
        }),
        "ref_table" => Some(ReferenceSymbol::Table {
            name: symbol_text.to_string(),
        }),
        // Column reference: either inside a column object's `name` pair, or
        // inside the `ref_columns` array element.
        "name" => {
            let owning_table = enclosing_table_name(pair, source)?;
            Some(ReferenceSymbol::Column {
                table: owning_table,
                column: symbol_text.to_string(),
            })
        }
        "ref_columns" => {
            let fk_object = pair.parent().and_then(skip_yaml_wrappers)?;
            let ref_table_raw = direct_child_value(fk_object, source, "ref_table")?;
            Some(ReferenceSymbol::Column {
                table: strip_quotes(ref_table_raw).to_string(),
                column: symbol_text.to_string(),
            })
        }
        _ => None,
    }
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
            "string_content" => return candidate.parent(),
            // Stop at structural boundaries.
            "array" | "object" | "pair" | "block_mapping_pair" | "block_mapping"
            | "block_sequence" | "flow_mapping" | "flow_sequence" => return None,
            _ => {}
        }
        current = candidate.parent();
    }
    None
}

fn enclosing_pair(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if matches!(candidate.kind(), "pair" | "block_mapping_pair") {
            return Some(candidate);
        }
        current = candidate.parent();
    }
    None
}

/// A pair is "top level" when its direct ancestor mapping is itself the
/// outermost mapping of the document. This is how we distinguish the
/// table's own `name: ...` from a column's `name: ...`.
fn is_top_level_pair(pair: tree_sitter::Node<'_>) -> bool {
    let Some(parent_mapping) = pair.parent() else {
        return false;
    };
    if !matches!(
        parent_mapping.kind(),
        "object" | "block_mapping" | "flow_mapping"
    ) {
        return false;
    }
    // Walk above the parent mapping; if we encounter another mapping with
    // the same kind, we are nested and therefore not top level.
    let mut current = parent_mapping.parent();
    while let Some(candidate) = current {
        if matches!(
            candidate.kind(),
            "object" | "block_mapping" | "flow_mapping"
        ) {
            return false;
        }
        current = candidate.parent();
    }
    true
}

/// Given a column's `name` pair, find the owning table's top-level name.
fn enclosing_table_name(name_pair: tree_sitter::Node<'_>, source: &str) -> Option<String> {
    // The pair we got is inside a column object. Walk up to the document's
    // outermost mapping and return its direct `name` value.
    let mut current = name_pair.parent();
    let mut outer = None;
    while let Some(candidate) = current {
        if matches!(
            candidate.kind(),
            "object" | "block_mapping" | "flow_mapping"
        ) {
            outer = Some(candidate);
        }
        current = candidate.parent();
    }
    let outer = outer?;

    let mut cursor = outer.walk();
    for child in outer.children(&mut cursor) {
        if matches!(child.kind(), "pair" | "block_mapping_pair") {
            let key = child.named_child(0)?;
            let key_text = strip_quotes(source.get(key.byte_range())?);
            if key_text == "name" {
                let value = child.named_child(1)?;
                return Some(strip_quotes(source.get(value.byte_range())?).to_string());
            }
        }
    }
    None
}

fn direct_child_value<'a>(
    object: tree_sitter::Node<'_>,
    source: &'a str,
    target_key: &str,
) -> Option<&'a str> {
    let mut cursor = object.walk();
    for child in object.children(&mut cursor) {
        if matches!(child.kind(), "pair" | "block_mapping_pair")
            && let Some(key) = child.named_child(0)
            && let Some(key_text) = source.get(key.byte_range())
            && strip_quotes(key_text) == target_key
            && let Some(value) = child.named_child(1)
            && let Some(text) = source.get(value.byte_range())
        {
            return Some(text);
        }
    }
    None
}

fn skip_yaml_wrappers(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    let mut current = node;
    while matches!(current.kind(), "flow_node" | "block_node") {
        current = current.parent()?;
    }
    Some(current)
}

fn node_at_byte(tree: &tree_sitter::Tree, byte_offset: usize) -> Option<tree_sitter::Node<'_>> {
    let root = tree.root_node();
    let mut current = root;
    'outer: loop {
        let mut cursor = current.walk();
        for child in current.children(&mut cursor) {
            if child.byte_range().contains(&byte_offset) {
                current = child;
                continue 'outer;
            }
        }
        return Some(current);
    }
}

fn strip_quotes(s: &str) -> &str {
    s.trim()
        .trim_start_matches('"')
        .trim_end_matches('"')
        .trim_start_matches('\'')
        .trim_end_matches('\'')
}
