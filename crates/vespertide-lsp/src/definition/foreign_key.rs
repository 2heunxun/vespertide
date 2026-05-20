//! Foreign-key go-to-definition for `ref_table` values.

use std::ops::Range;

use crate::store::DocumentStore;
use crate::workspace_index::WorkspaceIndex;

use super::DomainLocation;

pub(super) fn try_definition(
    node: tree_sitter::Node<'_>,
    source: &str,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
) -> Option<DomainLocation> {
    let pair = ref_table_pair(node, source)?;
    let value = pair.named_child(1)?;
    let target_name = strip_quotes(&source[value.byte_range()]).to_string();
    let loc = index.lookup(&target_name)?;

    let byte_range = docs
        .with_doc(&loc.uri, |text, tree| {
            let tree = tree?;
            find_top_level_name_range(tree, text)
        })
        .flatten()
        .unwrap_or(0..0);

    Some(DomainLocation {
        uri: loc.uri,
        byte_range,
    })
}

fn ref_table_pair<'tree>(
    node: tree_sitter::Node<'tree>,
    source: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut cur = Some(node);
    while let Some(candidate) = cur {
        if is_pair(candidate)
            && let Some(key) = candidate.named_child(0)
            && strip_quotes(&source[key.byte_range()]) == "ref_table"
        {
            return Some(candidate);
        }
        cur = candidate.parent();
    }
    None
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
