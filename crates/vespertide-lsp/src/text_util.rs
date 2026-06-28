//! Small text helpers shared across LSP features.

/// Strip surrounding JSON/SQL quote characters from a scalar's raw text.
/// Greedily trims leading/trailing double (`"`) then single (`'`) quotes
/// after trimming whitespace — the canonical form used across the LSP's
/// JSON/YAML scalar handling (a JSON `"..."` wrapper plus an inner SQL
/// `'...'` literal both peel cleanly).
#[must_use]
pub(crate) fn strip_quotes(s: &str) -> &str {
    s.trim()
        .trim_start_matches('"')
        .trim_end_matches('"')
        .trim_start_matches('\'')
        .trim_end_matches('\'')
}

/// UTF-8 slice of a tree-sitter node's byte range. Returns `None` when the
/// slice is not valid UTF-8 — defensive only; the LSP parsers produce valid
/// UTF-8 spans on every source we feed them. Single source of truth for the
/// `std::str::from_utf8(&source[node.byte_range()]).ok()` chain that used to
/// be open-coded across `diagnostics/locator`, `code_actions`, and friends.
#[must_use]
pub(crate) fn node_text<'a>(
    node: tree_sitter::Node<'_>,
    source: &'a [u8],
) -> Option<&'a str> {
    std::str::from_utf8(&source[node.byte_range()]).ok()
}
