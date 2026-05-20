//! Concurrent document store keyed by URI.
//!
//! [`DashMap`] is justified here as a performance-critical hot path:
//! `textDocument/didChange` arrives per-document and concurrently. All other
//! maps in the workspace use [`BTreeMap`](std::collections::BTreeMap) per the
//! AGENTS.md policy; this is the documented exception.

use dashmap::DashMap;
use tower_lsp_server::ls_types::Uri;

use crate::document::DocumentState;
use crate::parser::{DocumentFormat, ParserPool};

/// Thread-safe map of open documents.
#[derive(Debug)]
pub struct DocumentStore {
    docs: DashMap<Uri, DocumentState>,
    parser_pool: ParserPool,
}

impl DocumentStore {
    /// Build an empty store with a fresh parser pool.
    #[must_use]
    pub fn new() -> Self {
        Self {
            docs: DashMap::new(),
            parser_pool: ParserPool::new(),
        }
    }

    /// Handle `textDocument/didOpen`.
    ///
    /// Format is inferred from the URI extension; unknown extensions default
    /// to [`DocumentFormat::Json`].
    pub fn open(&self, uri: Uri, language_id: String, version: i32, text: String) {
        let format = DocumentFormat::from_uri(&uri).unwrap_or(DocumentFormat::Json);
        let state = DocumentState::new(language_id, version, text, format, &self.parser_pool);
        self.docs.insert(uri, state);
    }

    /// Handle a full-sync `textDocument/didChange`.
    pub fn update_full(&self, uri: &Uri, text: String, version: i32) {
        if let Some(mut entry) = self.docs.get_mut(uri) {
            entry.update_full(text, version, &self.parser_pool);
        }
    }

    /// Handle `textDocument/didClose`.
    pub fn close(&self, uri: &Uri) {
        self.docs.remove(uri);
    }

    /// Number of currently-open documents.
    #[must_use]
    pub fn len(&self) -> usize {
        self.docs.len()
    }

    /// `true` if no documents are open.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    /// Borrow a document's current text. Returns `None` if not open.
    pub fn with_text<R>(&self, uri: &Uri, f: impl FnOnce(&str) -> R) -> Option<R> {
        self.docs.get(uri).map(|state| f(state.text()))
    }

    /// Borrow a document's text and tree-sitter tree atomically.
    /// Returns `None` if the document is not open.
    pub fn with_doc<R>(
        &self,
        uri: &Uri,
        f: impl FnOnce(&str, Option<&tree_sitter::Tree>) -> R,
    ) -> Option<R> {
        self.docs
            .get(uri)
            .map(|state| f(state.text(), state.tree.as_ref()))
    }
}

impl Default for DocumentStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use tower_lsp_server::ls_types::Uri;

    fn uri(path: &str) -> Uri {
        Uri::from_str(&format!("file:///{path}")).unwrap()
    }

    #[test]
    fn open_insert_update_close() {
        let store = DocumentStore::new();
        let u = uri("test.json");
        assert!(store.is_empty());

        store.open(
            u.clone(),
            "json".to_string(),
            1,
            r#"{"name": "user"}"#.to_string(),
        );
        assert_eq!(store.len(), 1);

        store.update_full(&u, r#"{"name": "post"}"#.to_string(), 2);
        let text = store
            .with_text(&u, std::string::ToString::to_string)
            .unwrap();
        assert!(text.contains("post"));

        store.close(&u);
        assert!(store.is_empty());
    }
}
