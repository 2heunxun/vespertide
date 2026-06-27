//! Shared tree-sitter traversal helpers used across every LSP feature.
//!
//! Previously every feature module (hover, definition, references, rename,
//! file_features, check_expr_locate, …) carried its own byte-identical
//! private copy of [`node_at_byte`]. Hoisting the BFS descent into one
//! `pub(crate)` helper kills ~80 lines of pure duplication and prevents
//! the next LSP feature from copy-pasting an eighth variant.
//!
//! NOTE: `completion::context` keeps its own `node_at_byte` because that
//! call site uses [`tree_sitter::Node::descendant_for_byte_range`] for
//! end-of-token cursor semantics, which differs from this BFS descent at
//! byte boundaries. Do not migrate it here.

/// Descend from the root to the deepest node whose byte range contains
/// `byte_offset`.
///
/// Returns `Some(_)` for every well-formed tree — the loop always
/// terminates at a leaf when no child contains the offset — so the
/// `Option` wrapper is preserved purely so existing call sites keep
/// their `?` short-circuits unchanged.
pub(crate) fn node_at_byte(
    tree: &tree_sitter::Tree,
    byte_offset: usize,
) -> Option<tree_sitter::Node<'_>> {
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
