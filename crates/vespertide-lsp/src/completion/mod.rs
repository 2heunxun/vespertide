//! Completion provider — pure domain layer (no LSP protocol types).

mod context;
mod values;

use crate::parser::DocumentFormat;
use crate::store::DocumentStore;
use crate::workspace_index::WorkspaceIndex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainCompletion {
    pub label: String,
    pub kind: CompletionItemKind,
    /// Markdown documentation, if any.
    pub detail: Option<String>,
    /// Text to insert; may differ from the label for snippets.
    pub insert_text: Option<String>,
    /// Sort priority (smaller = higher). Mirrors the sqls pattern.
    pub sort_priority: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionItemKind {
    /// Enum value, boolean literal, or other scalar value.
    Value,
    /// Object key.
    Property,
    /// Workspace reference, such as a table or column.
    Reference,
    /// Multi-field template.
    Snippet,
}

/// Compute completions at a byte offset. Returns an empty list when no context matches.
#[must_use]
pub fn compute(
    text: &str,
    _format: DocumentFormat,
    tree: Option<&tree_sitter::Tree>,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
    byte_offset: usize,
) -> Vec<DomainCompletion> {
    let Some(tree) = tree else {
        return Vec::new();
    };

    match context::detect(tree, text, byte_offset) {
        context::Context::ColumnType => values::column_types(),
        context::Context::OnDeleteAction | context::Context::OnUpdateAction => {
            values::reference_actions()
        }
        context::Context::Nullable | context::Context::PrimaryKey | context::Context::Unique => {
            values::booleans()
        }
        context::Context::RefTable => values::tables_in_workspace(index),
        context::Context::RefColumns { ref_table } => {
            values::columns_of(ref_table.as_str(), index, docs)
        }
        context::Context::None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ParserPool;

    #[test]
    fn completion_in_column_type_field() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"u","columns":[{"name":"id","type":"i","nullable":false}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let pos = src.find(r#""type":"i""#).unwrap() + 9;
        let items = compute(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos);

        assert!(!items.is_empty(), "should suggest types");
        assert!(items.iter().any(|item| item.label == "integer"));
    }

    #[test]
    fn completion_for_nullable_returns_booleans() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer","nullable":}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let pos = src.find("nullable\":").unwrap() + 10;
        let items = compute(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos);

        assert!(items.iter().any(|item| item.label == "true"));
        assert!(items.iter().any(|item| item.label == "false"));
    }
}
