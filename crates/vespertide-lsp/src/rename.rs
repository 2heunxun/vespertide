//! Workspace-wide rename for tables and columns.
//!
//! Reuses the references engine: every occurrence the references provider
//! would return for the cursor's symbol becomes a `TextEdit` that replaces
//! the byte range with `new_name`. The declaration is always included so
//! the rename is symmetrical (definition + every usage).
//!
//! Returned [`DomainRename`] is grouped by URI so the backend can pack it
//! into an LSP `WorkspaceEdit` without further reshuffling.

use std::collections::BTreeMap;
use std::ops::Range;

use tower_lsp_server::ls_types::Uri;

use crate::parser::DocumentFormat;
use crate::references::{self, ReferenceSymbol};
use crate::store::DocumentStore;
use crate::workspace_index::WorkspaceIndex;
use crate::workspace_tables::WorkspaceTables;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainTextEdit {
    pub byte_range: Range<usize>,
    pub new_text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DomainRename {
    /// `URI → edits to apply` (deterministic iteration order).
    pub edits: BTreeMap<Uri, Vec<DomainTextEdit>>,
    /// The symbol the rename is targeting. Useful for telemetry / logs.
    pub symbol: Option<ReferenceSymbol>,
}

/// Validate `new_name` and produce a workspace-wide rename plan.
///
/// Returns `None` when:
///   * the cursor is not on a renameable symbol,
///   * `new_name` is empty, identical to the old name, or contains
///     invalid characters (whitespace, quotes, control chars).
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn compute(
    source: &str,
    format: DocumentFormat,
    tree: Option<&tree_sitter::Tree>,
    current_uri: &Uri,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
    disk_tables: Option<&WorkspaceTables>,
    byte_offset: usize,
    new_name: &str,
) -> Option<DomainRename> {
    if !is_valid_identifier(new_name) {
        return None;
    }
    let symbol = references::resolve_symbol(source, tree, current_uri, byte_offset)?;
    let old_name = match &symbol {
        ReferenceSymbol::Table { name } => name.clone(),
        ReferenceSymbol::Column { column, .. } => column.clone(),
    };
    if old_name == new_name {
        return None;
    }

    let refs = references::compute(
        source,
        format,
        tree,
        current_uri,
        index,
        docs,
        disk_tables,
        byte_offset,
        true, // rename ALWAYS includes the declaration.
    );

    if refs.is_empty() {
        return None;
    }

    let mut edits: BTreeMap<Uri, Vec<DomainTextEdit>> = BTreeMap::new();
    for reference in refs {
        edits.entry(reference.uri).or_default().push(DomainTextEdit {
            byte_range: reference.byte_range,
            new_text: new_name.to_string(),
        });
    }

    // Sort within each file by reverse byte order so applying edits front-to-back
    // never invalidates ranges that come later. We sort descending; the LSP
    // client is expected to apply edits in document order, so deduplicate
    // overlaps and present them sorted ASCENDING here (clients sort, but
    // sending in document order avoids edge cases).
    for file_edits in edits.values_mut() {
        file_edits.sort_by_key(|e| e.byte_range.start);
        file_edits.dedup_by(|a, b| a.byte_range == b.byte_range && a.new_text == b.new_text);
    }

    Some(DomainRename {
        edits,
        symbol: Some(symbol),
    })
}

/// Names must look like SQL/JSON identifiers — no whitespace, quotes,
/// brackets, or control characters. We intentionally keep this strict so
/// the resulting file is still valid JSON without re-escaping.
fn is_valid_identifier(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    name.chars().all(|c| {
        !c.is_whitespace()
            && !c.is_control()
            && !matches!(c, '"' | '\'' | '\\' | ',' | ':' | '[' | ']' | '{' | '}')
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ParserPool;
    use std::str::FromStr;

    fn uri(p: &str) -> Uri {
        Uri::from_str(&format!("file:///{p}")).unwrap()
    }

    #[test]
    fn rejects_empty_or_whitespace_or_same_name() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"user","columns":[{"name":"id","type":"integer"}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let pos = src.find(r#""name":"user""#).unwrap() + 9;

        for bad in ["", " ", "\"", "user", "a b"] {
            assert!(
                compute(
                    src,
                    DocumentFormat::Json,
                    tree.as_ref(),
                    &uri("user.json"),
                    &idx,
                    &docs,
                    None,
                    pos,
                    bad,
                )
                .is_none(),
                "rename to `{bad}` should be rejected"
            );
        }
    }

    #[test]
    fn rename_table_produces_edits_in_declaration_and_each_ref_file() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();

        let user_src = r#"{"name":"user","columns":[{"name":"id","type":"integer","primary_key":true}]}"#;
        let user_uri = uri("user.json");
        let user_tree = pool.parse(user_src, DocumentFormat::Json).unwrap();
        idx.upsert(&user_uri, user_src, &user_tree);
        docs.open(user_uri.clone(), "json".to_string(), 1, user_src.to_string());

        let post_src = r#"{"name":"post","columns":[{"name":"author_id","type":"integer","foreign_key":{"ref_table":"user","ref_columns":["id"]}}]}"#;
        let post_uri = uri("post.json");
        let post_tree = pool.parse(post_src, DocumentFormat::Json).unwrap();
        idx.upsert(&post_uri, post_src, &post_tree);
        docs.open(post_uri.clone(), "json".to_string(), 1, post_src.to_string());

        let pos = user_src.find(r#""name":"user""#).unwrap() + 9;
        let plan = compute(
            user_src,
            DocumentFormat::Json,
            Some(&user_tree),
            &user_uri,
            &idx,
            &docs,
            None,
            pos,
            "account",
        )
        .expect("rename should succeed");

        assert!(
            plan.edits.contains_key(&user_uri),
            "declaration file must be edited"
        );
        assert!(
            plan.edits.contains_key(&post_uri),
            "reference file must be edited"
        );
        for file_edits in plan.edits.values() {
            for edit in file_edits {
                assert_eq!(edit.new_text, "account");
            }
        }
    }

    /// Regression — `"id"` → `a` must produce `"a"`, not bare `a`. The
    /// references engine used to hand out the JSON `string` node's full
    /// range (quotes included), which made rename eat the quotes and break
    /// JSON parsing.
    #[test]
    fn rename_preserves_surrounding_json_quotes() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"user","columns":[{"name":"id","type":"integer","primary_key":true}]}"#;
        let user_uri = uri("user.json");
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();
        idx.upsert(&user_uri, src, &tree);
        docs.open(user_uri.clone(), "json".to_string(), 1, src.to_string());

        let pos = src.find(r#""name":"id""#).unwrap() + 9;
        let plan = compute(
            src,
            DocumentFormat::Json,
            Some(&tree),
            &user_uri,
            &idx,
            &docs,
            None,
            pos,
            "a",
        )
        .expect("rename should succeed");

        let file_edits = plan.edits.get(&user_uri).expect("edits for user.json");
        assert_eq!(file_edits.len(), 1);
        let edit = &file_edits[0];
        assert_eq!(&src[edit.byte_range.clone()], "id", "must replace ONLY inside the quotes");

        // Apply the edit and confirm the result is still valid JSON.
        let mut after = String::from(&src[..edit.byte_range.start]);
        after.push_str(&edit.new_text);
        after.push_str(&src[edit.byte_range.end..]);
        assert!(after.contains(r#""a""#), "result must keep the quotes: {after}");
        serde_json::from_str::<serde_json::Value>(&after)
            .expect("rename output must still parse as JSON");
    }

    #[test]
    fn rename_column_scoped_to_its_owning_table() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();

        let user_src = r#"{"name":"user","columns":[{"name":"email","type":"text","unique":true}]}"#;
        let user_uri = uri("user.json");
        let user_tree = pool.parse(user_src, DocumentFormat::Json).unwrap();
        idx.upsert(&user_uri, user_src, &user_tree);
        docs.open(user_uri.clone(), "json".to_string(), 1, user_src.to_string());

        let post_src = r#"{"name":"post","columns":[{"name":"author_email","type":"text","foreign_key":{"ref_table":"user","ref_columns":["email"]}}]}"#;
        let post_uri = uri("post.json");
        let post_tree = pool.parse(post_src, DocumentFormat::Json).unwrap();
        idx.upsert(&post_uri, post_src, &post_tree);
        docs.open(post_uri.clone(), "json".to_string(), 1, post_src.to_string());

        // An unrelated `email` column on another table — must NOT be renamed.
        let other_src = r#"{"name":"other","columns":[{"name":"email","type":"text"}]}"#;
        let other_uri = uri("other.json");
        let other_tree = pool.parse(other_src, DocumentFormat::Json).unwrap();
        idx.upsert(&other_uri, other_src, &other_tree);
        docs.open(other_uri.clone(), "json".to_string(), 1, other_src.to_string());

        let pos = user_src.find(r#""name":"email""#).unwrap() + 10;
        let plan = compute(
            user_src,
            DocumentFormat::Json,
            Some(&user_tree),
            &user_uri,
            &idx,
            &docs,
            None,
            pos,
            "mail",
        )
        .expect("rename should succeed");

        assert!(plan.edits.contains_key(&user_uri));
        assert!(plan.edits.contains_key(&post_uri));
        assert!(
            !plan.edits.contains_key(&other_uri),
            "unrelated `other.email` column must not be in the rename plan"
        );
    }
}
