//! Cross-file validation tests for workspace-aware diagnostics.

use std::str::FromStr;

use tower_lsp_server::ls_types::Uri;
use vespertide_lsp::diagnostics::validation::WorkspaceTable;
use vespertide_lsp::{DocumentFormat, ParserPool, compute_workspace_diagnostics};

fn uri(path: &str) -> Uri {
    Uri::from_str(&format!("file:///{path}")).unwrap()
}

#[test]
fn cross_file_fk_resolves_to_existing_table() {
    let pool = ParserPool::new();

    let user_src = r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#;
    let user_tree = pool.parse(user_src, DocumentFormat::Json).unwrap();
    let user_table = serde_json::from_str::<vespertide_core::TableDef>(user_src)
        .unwrap()
        .normalize()
        .unwrap();

    let post_src = r#"{"name":"post","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"author_id","type":"integer","nullable":false,"foreign_key":{"ref_table":"user","ref_columns":["id"]}}]}"#;
    let post_tree = pool.parse(post_src, DocumentFormat::Json).unwrap();
    let post_table = serde_json::from_str::<vespertide_core::TableDef>(post_src)
        .unwrap()
        .normalize()
        .unwrap();

    let workspace = vec![
        WorkspaceTable {
            uri: uri("user.json"),
            table: user_table,
            source: user_src.to_string(),
            tree: user_tree,
        },
        WorkspaceTable {
            uri: uri("post.json"),
            table: post_table,
            source: post_src.to_string(),
            tree: post_tree.clone(),
        },
    ];

    let diags = compute_workspace_diagnostics(
        post_src,
        DocumentFormat::Json,
        Some(&post_tree),
        &workspace,
        &uri("post.json"),
    );
    let validate_errs: Vec<_> = diags
        .iter()
        .filter(|diag| diag.code == "validate-schema")
        .collect();

    assert!(
        validate_errs.is_empty(),
        "expected no FK error when target table exists, got: {validate_errs:?}"
    );
}

#[test]
fn cross_file_fk_missing_target_highlights_correct_column() {
    let pool = ParserPool::new();
    let post_src = r#"{"name":"post","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"author_id","type":"integer","nullable":false,"foreign_key":{"ref_table":"nonexistent","ref_columns":["id"]}}]}"#;
    let post_tree = pool.parse(post_src, DocumentFormat::Json).unwrap();
    let post_table = serde_json::from_str::<vespertide_core::TableDef>(post_src)
        .unwrap()
        .normalize()
        .unwrap();

    let workspace = vec![WorkspaceTable {
        uri: uri("post.json"),
        table: post_table,
        source: post_src.to_string(),
        tree: post_tree.clone(),
    }];

    let diags = compute_workspace_diagnostics(
        post_src,
        DocumentFormat::Json,
        Some(&post_tree),
        &workspace,
        &uri("post.json"),
    );
    let err = diags
        .iter()
        .find(|diag| diag.code == "validate-schema" && diag.message.contains("non-existent table"))
        .expect("expected FK error");
    let snippet = &post_src[err.byte_range.clone()];

    assert!(
        snippet.contains("author_id"),
        "expected error to highlight 'author_id' column, got: {snippet}"
    );
    assert_ne!(
        err.byte_range,
        0..1,
        "byte_range should not fall back to 0..1"
    );
}
