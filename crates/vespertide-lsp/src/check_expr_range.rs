//! Shared byte-range extraction for table-level CHECK expression scalars.

use std::ops::Range;

/// Inner byte range of a CHECK `expr` scalar, excluding syntax delimiters.
///
/// Handles JSON strings and YAML scalar wrappers. For YAML block scalars,
/// the returned range starts after the `|` / `>` indicator and runs to the
/// node end; the CHECK lexer trims per-line indentation/newlines later.
pub(crate) fn expr_inner_range(value_node: tree_sitter::Node<'_>) -> Option<Range<usize>> {
    let raw = value_node.byte_range();
    match value_node.kind() {
        "string" | "double_quote_scalar" | "single_quote_scalar" => {
            (raw.end.saturating_sub(raw.start) >= 2).then(|| (raw.start + 1)..(raw.end - 1))
        }
        "plain_scalar" => value_node
            .named_child(0)
            .filter(|child| child.kind() == "string_scalar")
            .map_or_else(|| Some(raw.clone()), |child| Some(child.byte_range())),
        "string_scalar" => Some(raw),
        "block_scalar" => value_node
            .child(0)
            .map(|indicator| indicator.end_byte()..value_node.end_byte()),
        "flow_node" | "block_node" => value_node.named_child(0).and_then(expr_inner_range),
        _ => None,
    }
}
