//! Shared helpers for locating table-level CHECK `expr` strings in the
//! tree-sitter parse tree.
//!
//! Eliminates the 4-way duplication that previously had
//! `references::resolver`, `references::search`, `rename`, and
//! `code_actions` each re-implementing "is this pair a CHECK `expr`?" /
//! "what table owns this CHECK?" walks. Both JSON (`pair` / `object`) and
//! YAML (`block_mapping_pair` / `block_mapping` / `flow_mapping`, with
//! `flow_node` / `block_node` wrappers around values) are handled — every
//! public helper accepts the source as `&[u8]` and returns format-agnostic
//! results.

use std::ops::Range;

use tree_sitter::{Node, Tree};

use crate::text_util::strip_quotes;

/// Cursor-based result of [`find_check_expr_at`]: the CHECK `expr` string
/// the cursor sits in, the inner byte range of its predicate text, and the
/// name of the owning table.
pub(crate) struct CheckExprAt {
    /// Byte range of the CHECK predicate text inside the document,
    /// excluding surrounding quotes / YAML block-scalar indicators.
    /// Same range [`crate::check_expr_range::expr_inner_range`] returns.
    pub inner: Range<usize>,
    /// Owning table (the outermost mapping's `name` value).
    #[expect(dead_code, reason = "field reserved for future use")]
    pub table: String,
}

/// If `byte_offset` lands inside a table-level CHECK `expr` string,
/// return its [`CheckExprAt`] context. Returns `None` for any non-CHECK
/// position (cursor outside any string, cursor on the key side, cursor on
/// a string that is not a CHECK constraint's `expr` value).
pub(crate) fn find_check_expr_at(
    tree: &Tree,
    source: &[u8],
    byte_offset: usize,
) -> Option<CheckExprAt> {
    let node = node_at_byte(tree, byte_offset)?;
    let string_node = enclosing_string(node)?;

    let pair = enclosing_pair(string_node)?;
    let key = pair.named_child(0)?;
    let key_text = std::str::from_utf8(source.get(key.byte_range())?).ok()?;
    if strip_quotes(key_text) != "expr" {
        return None;
    }
    // The cursor's string must be the VALUE side, not the key.
    let value = pair.named_child(1)?;
    if !value.byte_range().contains(&string_node.start_byte()) {
        return None;
    }
    if !is_check_constraint_pair(source, pair) {
        return None;
    }

    let inner = crate::check_expr_range::expr_inner_range(string_node)?;
    let table = owning_table_name(source, pair)?;

    Some(CheckExprAt { inner, table })
}

/// True when `expr_pair` (a pair whose key is `expr`) belongs to a CHECK
/// constraint object — i.e. it sits next to a sibling `type: "check"`
/// pair inside a `constraints` array element.
///
/// The caller is responsible for verifying that `expr_pair.key == "expr"`;
/// this helper only inspects the sibling `type` value.
pub(crate) fn is_check_constraint_pair(source: &[u8], expr_pair: Node<'_>) -> bool {
    sibling_value(source, expr_pair, "type").is_some_and(|v| v == "check")
}

/// The owning table name — the document's outermost mapping's `name`
/// value. Walks up from `node` to the outermost `object`/`block_mapping`/
/// `flow_mapping` and returns its direct `name` child's stripped scalar.
pub(crate) fn owning_table_name(source: &[u8], node: Node<'_>) -> Option<String> {
    let mut current = node.parent();
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
        if !matches!(child.kind(), "pair" | "block_mapping_pair") {
            continue;
        }
        let Some(key) = child.named_child(0) else {
            continue;
        };
        let Some(key_text) = std::str::from_utf8(source.get(key.byte_range())?).ok() else {
            continue;
        };
        if strip_quotes(key_text) != "name" {
            continue;
        }
        let value = child.named_child(1)?;
        let actual = peel_wrapper(value);
        let text = std::str::from_utf8(source.get(actual.byte_range())?).ok()?;
        return Some(strip_quotes(text).to_string());
    }
    None
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Look up a sibling pair's scalar value within the same mapping (the
/// `pair`'s direct parent, peeling YAML `flow_node`/`block_node` wrappers).
fn sibling_value(source: &[u8], pair: Node<'_>, target_key: &str) -> Option<String> {
    let object_raw = pair.parent()?;
    let object = peel_wrapper(object_raw);
    let mut cursor = object.walk();
    for child in object.children(&mut cursor) {
        if !matches!(child.kind(), "pair" | "block_mapping_pair") {
            continue;
        }
        let Some(key) = child.named_child(0) else {
            continue;
        };
        let Some(key_text) = std::str::from_utf8(source.get(key.byte_range())?).ok() else {
            continue;
        };
        if strip_quotes(key_text) != target_key {
            continue;
        }
        let value = child.named_child(1)?;
        let actual = peel_wrapper(value);
        let text = std::str::from_utf8(source.get(actual.byte_range())?).ok()?;
        return Some(strip_quotes(text).to_string());
    }
    None
}

/// If `node` is a YAML `flow_node` / `block_node` wrapper, descend into
/// its first named child (the underlying mapping / scalar). Otherwise
/// return `node` unchanged. JSON nodes are passed through.
fn peel_wrapper(node: Node<'_>) -> Node<'_> {
    match node.kind() {
        "flow_node" | "block_node" => node.named_child(0).unwrap_or(node),
        _ => node,
    }
}

/// Closest ancestor `pair` / `block_mapping_pair`.
fn enclosing_pair(node: Node<'_>) -> Option<Node<'_>> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if matches!(candidate.kind(), "pair" | "block_mapping_pair") {
            return Some(candidate);
        }
        current = candidate.parent();
    }
    None
}

/// Closest ancestor that is a JSON / YAML string scalar. Stops at
/// structural boundaries (arrays, objects, mappings, pairs) so a cursor
/// that lives between strings does not accidentally bind to a string
/// further up the tree.
fn enclosing_string(node: Node<'_>) -> Option<Node<'_>> {
    let mut current = Some(node);
    while let Some(candidate) = current {
        match candidate.kind() {
            "string"
            | "double_quote_scalar"
            | "single_quote_scalar"
            | "string_scalar"
            | "plain_scalar"
            | "block_scalar" => return Some(candidate),
            "string_content" => return candidate.parent(),
            "array" | "object" | "pair" | "block_mapping_pair" | "block_mapping"
            | "block_sequence" | "flow_mapping" | "flow_sequence" => return None,
            _ => {}
        }
        current = candidate.parent();
    }
    None
}

/// Descend from the root to the deepest node whose byte range contains
/// `byte_offset`.
fn node_at_byte(tree: &Tree, byte_offset: usize) -> Option<Node<'_>> {
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
