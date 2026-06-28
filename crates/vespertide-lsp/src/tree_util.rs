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

/// Is `node` a YAML/JSON key-value pair? `pair` is the JSON grammar's name,
/// `block_mapping_pair` is the YAML grammar's. Hoisted here so every LSP
/// feature module can call one canonical helper instead of carrying its own
/// byte-identical copy (was duplicated in 6 sibling modules pre-0.2.0).
pub(crate) fn is_pair(node: tree_sitter::Node<'_>) -> bool {
    matches!(node.kind(), "pair" | "block_mapping_pair")
}

/// Walk the tree depth-first and return the first node whose kind is a
/// top-level mapping (`object`/`block_mapping`/`flow_mapping`). Every LSP
/// feature that wants the document's outermost table object needs this
/// primitive — previously duplicated in 4 sibling modules pre-0.2.0.
pub(crate) fn find_outer_mapping(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    if matches!(node.kind(), "object" | "block_mapping" | "flow_mapping") {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = find_outer_mapping(child) {
            return Some(found);
        }
    }
    None
}

/// Peel tree-sitter-yaml's `flow_node` / `block_node` wrappers so downstream
/// `match`es see the real kind (no-op on JSON, whose grammar has no such
/// wrappers). Fused while-let so the empty-wrapper case (no usable
/// `named_child(0)`) and the kind-mismatch case share the same loop exit —
/// no defensive `return` line that depends on a tree-sitter-yaml release
/// producing empty wrappers. Hoisted here so every LSP feature module can
/// call one canonical helper instead of carrying its own byte-identical
/// copy (was duplicated in 4 sibling modules pre-0.2.0, including one
/// `unwrap_flow_node` rename in `completion::context`).
pub(crate) fn unwrap_yaml_node(node: tree_sitter::Node<'_>) -> tree_sitter::Node<'_> {
    let mut current = node;
    while matches!(current.kind(), "flow_node" | "block_node")
        && let Some(inner) = current
            .named_child(0)
            .filter(|inner| inner.id() != current.id())
    {
        current = inner;
    }
    current
}

/// Walk the parent chain looking for the nearest [`is_pair`] ancestor whose
/// key (after [`crate::text_util::strip_quotes`]) equals `expected_key`.
///
/// Centralises the two byte-identical private copies that lived in
/// `definition::foreign_key` and `completion::context`. Uses the safe
/// `source.get(..)` form so mid-edit byte ranges that fall outside `source`
/// return `None` instead of panicking — `tree_sitter` ranges and the
/// document text can momentarily disagree while the user types.
pub(crate) fn enclosing_pair_with_key<'tree>(
    node: tree_sitter::Node<'tree>,
    source: &str,
    expected_key: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut current = Some(node);
    while let Some(candidate) = current {
        if is_pair(candidate)
            && let Some(key) = candidate.named_child(0)
            && let Some(key_text) = source.get(key.byte_range())
            && crate::text_util::strip_quotes(key_text) == expected_key
        {
            return Some(candidate);
        }
        current = candidate.parent();
    }
    None
}

/// Parent-only variant of [`enclosing_pair_with_key`]: walk strictly the
/// PARENT chain (skip `node` itself) looking for the nearest [`is_pair`]
/// ancestor whose key (after [`crate::text_util::strip_quotes`]) equals
/// `expected_key`.
///
/// Centralises the three byte-equivalent `is_inside_*` ancestor walks
/// previously open-coded in `completion::context::is_inside_constraints`,
/// `hover::check_expr::is_inside_constraints`, and
/// `hover::column::is_inside_columns`. Use this when "am I inside a pair
/// whose key is `<X>`?" is the question; use [`enclosing_pair_with_key`]
/// when the cursor's own node may itself be the pair.
pub(crate) fn ancestor_pair_with_key<'tree>(
    node: tree_sitter::Node<'tree>,
    source: &str,
    expected_key: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if is_pair(candidate)
            && let Some(key) = candidate.named_child(0)
            && let Some(key_text) = source.get(key.byte_range())
            && crate::text_util::strip_quotes(key_text) == expected_key
        {
            return Some(candidate);
        }
        current = candidate.parent();
    }
    None
}

/// Find a direct child pair of `object` whose key (after
/// [`crate::text_util::strip_quotes`]) equals `target_key` and return the
/// value-side byte slice **verbatim** (quotes preserved for quoted
/// scalars).
///
/// Centralises the three byte-identical private copies that lived in
/// `definition::foreign_key`, `completion::context`, and
/// `references::resolver`. Each pre-existing caller already stripped
/// quotes off the returned slice, so this helper deliberately keeps the
/// raw form to preserve the existing call-site contract. Uses the safe
/// `source.get(..)` form on both the key and value spans so a stale
/// byte range mid-edit returns `None` instead of panicking.
pub(crate) fn direct_child_value<'a>(
    object: tree_sitter::Node<'_>,
    source: &'a str,
    target_key: &str,
) -> Option<&'a str> {
    let mut cursor = object.walk();
    for child in object.children(&mut cursor) {
        if is_pair(child)
            && let Some(key) = child.named_child(0)
            && let Some(key_text) = source.get(key.byte_range())
            && crate::text_util::strip_quotes(key_text) == target_key
            && let Some(value) = child.named_child(1)
            && let Some(text) = source.get(value.byte_range())
        {
            return Some(text);
        }
    }
    None
}
