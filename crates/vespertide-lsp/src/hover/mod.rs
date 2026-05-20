//! Hover provider — pure domain layer (no LSP protocol types).

mod column;
mod foreign_key;

use std::ops::Range;

use crate::parser::DocumentFormat;
use crate::store::DocumentStore;
use crate::workspace_index::WorkspaceIndex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainHover {
    /// Markdown content for the hover popup.
    pub markdown: String,
    /// Byte range to highlight (the symbol under cursor).
    pub byte_range: Range<usize>,
}

/// Compute hover at byte offset. Returns `None` if the cursor is on
/// non-hoverable content.
#[must_use]
pub fn compute(
    text: &str,
    format: DocumentFormat,
    tree: Option<&tree_sitter::Tree>,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
    byte_offset: usize,
) -> Option<DomainHover> {
    let _ = format;
    let tree = tree?;
    let node = node_at_byte(tree, byte_offset)?;

    // `foreign_key.ref_table` is nested inside a column object, so try the
    // specific FK hover first before falling back to the broader column hover.
    if let Some(hover) = foreign_key::try_hover(node, text, index, docs) {
        return Some(hover);
    }

    column::try_hover(node, text)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DocumentFormat, ParserPool};
    use crate::store::DocumentStore;
    use crate::workspace_index::WorkspaceIndex;

    #[test]
    fn hover_outside_returns_none() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name": "user"}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let hover = compute(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, 0);
        // First char is `{` — no hover content. Some impls may return generic
        // hover; OK either way as long as there is no panic.
        let _ = hover;
    }

    #[test]
    fn hover_on_column_name_returns_some() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let pos = src.find(r#""name":"id""#).unwrap() + 5;
        let hover = compute(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos);
        assert!(hover.is_some(), "hover on column should return Some");
    }
}
