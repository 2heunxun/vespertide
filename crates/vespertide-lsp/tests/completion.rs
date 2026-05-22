use std::str::FromStr;

use tempfile::tempdir;
use tower_lsp_server::ls_types::Uri;
use vespertide_lsp::{
    DocumentFormat, DocumentStore, ParserPool, WorkspaceIndex, WorkspaceTables, compute_completion,
    compute_completion_with_workspace_tables,
};

fn uri(path: &str) -> Uri {
    Uri::from_str(&format!("file:///{path}")).unwrap()
}

#[test]
fn completion_inside_column_type_string_offers_simple_plus_replacing_snippets() {
    let pool = ParserPool::new();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();
    let src = r#"{"name":"u","columns":[{"name":"id","type":"","nullable":false}]}"#;
    let tree = pool.parse(src, DocumentFormat::Json);
    let pos = src.find(r#""type":"""#).unwrap() + 8;
    let items = compute_completion(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos);

    // Simple scalar types insert in-place (no replacement range).
    let integer = items.iter().find(|i| i.label == "integer").unwrap();
    assert!(integer.replace_range_bytes.is_none());

    // Object snippets ARE present but each replaces the whole string
    // literal (quotes included) so JSON stays valid after acceptance.
    let string_start = src.rfind(r#""""#).unwrap();
    let string_end = string_start + 2;
    for label in ["varchar(N)", "char(N)", "numeric(P,S)", "enum"] {
        let snippet = items
            .iter()
            .find(|i| i.label == label)
            .unwrap_or_else(|| panic!("snippet `{label}` should be offered"));
        let range = snippet
            .replace_range_bytes
            .as_ref()
            .unwrap_or_else(|| panic!("`{label}` must carry replace_range_bytes"));
        assert_eq!(range.start, string_start, "{label} start");
        assert_eq!(range.end, string_end, "{label} end");
    }
}

#[test]
fn completion_at_bare_column_type_value_offers_object_snippets() {
    let pool = ParserPool::new();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();
    // No quotes around the value slot.
    let src = r#"{"name":"u","columns":[{"name":"id","type":,"nullable":false}]}"#;
    let tree = pool.parse(src, DocumentFormat::Json);
    let pos = src.find(r#""type":"#).unwrap() + 7;
    let items = compute_completion(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos);

    assert!(items.iter().any(|item| item.label == "varchar(N)"));
    assert!(items.iter().any(|item| item.label == "integer"));
}

#[test]
fn cross_file_ref_columns() {
    let pool = ParserPool::new();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();

    let user_uri = uri("user.json");
    let user_src = r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"email","type":"text","nullable":false}]}"#;
    let user_tree = pool.parse(user_src, DocumentFormat::Json).unwrap();
    idx.upsert(&user_uri, user_src, &user_tree);
    docs.open(user_uri, "json".to_string(), 1, user_src.to_string());

    let post_src = r#"{"name":"post","columns":[{"name":"author_id","type":"integer","nullable":false,"foreign_key":{"ref_table":"user","ref_columns":[""]}}]}"#;
    let post_tree = pool.parse(post_src, DocumentFormat::Json);
    let pos = post_src.find(r#"["""#).unwrap() + 2;
    let items = compute_completion(
        post_src,
        DocumentFormat::Json,
        post_tree.as_ref(),
        &idx,
        &docs,
        pos,
    );
    let labels = items
        .iter()
        .map(|item| item.label.as_str())
        .collect::<Vec<_>>();

    assert!(
        labels.contains(&"id"),
        "should suggest 'id' column. got: {labels:?}"
    );
    assert!(
        labels.contains(&"email"),
        "should suggest 'email' column. got: {labels:?}"
    );
}

#[test]
fn completion_for_on_delete_returns_actions() {
    let pool = ParserPool::new();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();
    let src = r#"{"name":"p","columns":[{"name":"x","type":"integer","nullable":false,"foreign_key":{"ref_table":"u","ref_columns":["id"],"on_delete":""}}]}"#;
    let tree = pool.parse(src, DocumentFormat::Json);
    let pos = src.find(r#""on_delete":"""#).unwrap() + 14;
    let items = compute_completion(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos);

    assert!(items.iter().any(|item| item.label == "cascade"));
    assert!(items.iter().any(|item| item.label == "set_null"));
}

#[test]
fn disk_workspace_tables_feed_ref_column_completion() {
    let tmp = tempdir().unwrap();
    let models_dir = tmp.path().join("models");
    std::fs::create_dir_all(&models_dir).unwrap();
    std::fs::write(
        tmp.path().join("vespertide.json"),
        r#"{"modelsDir":"models","migrationsDir":"migrations","tableNamingCase":"snake","columnNamingCase":"snake","modelFormat":"json"}"#,
    )
    .unwrap();
    std::fs::write(
        models_dir.join("user.json"),
        r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"email","type":"text","nullable":false}]}"#,
    )
    .unwrap();

    let pool = ParserPool::new();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();
    let disk_tables = WorkspaceTables::new();
    assert!(disk_tables.refresh(tmp.path()));

    let post_src = r#"{"name":"post","columns":[{"name":"author_id","type":"integer","nullable":false,"foreign_key":{"ref_table":"user","ref_columns":[""]}}]}"#;
    let post_tree = pool.parse(post_src, DocumentFormat::Json);
    let pos = post_src.find(r#"["""#).unwrap() + 2;
    let items = compute_completion_with_workspace_tables(
        post_src,
        DocumentFormat::Json,
        post_tree.as_ref(),
        &idx,
        &docs,
        &disk_tables,
        pos,
    );
    let labels = items
        .iter()
        .map(|item| item.label.as_str())
        .collect::<Vec<_>>();

    assert!(labels.contains(&"id"), "labels: {labels:?}");
    assert!(labels.contains(&"email"), "labels: {labels:?}");
}

// ============================================================================
// YAML coverage — make sure every completion context that works in JSON also
// works in YAML. YAML uses different tree-sitter node kinds (`block_mapping`,
// `block_mapping_pair`, `flow_node`, etc.) so this is a real second axis.
// ============================================================================

#[test]
fn yaml_column_type_in_string_offers_simple_types() {
    let pool = ParserPool::new();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();
    let src = "name: u\ncolumns:\n  - name: id\n    type: \"\"\n    nullable: false\n";
    let tree = pool.parse(src, DocumentFormat::Yaml);
    // Position cursor inside the empty `""` after `type:`.
    let pos = src.find(r#"type: """#).unwrap() + 7;
    let items = compute_completion(src, DocumentFormat::Yaml, tree.as_ref(), &idx, &docs, pos);

    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"integer"),
        "YAML should offer `integer` for type, got: {labels:?}"
    );
    assert!(
        labels.contains(&"uuid"),
        "YAML should offer `uuid` for type, got: {labels:?}"
    );
}

#[test]
fn yaml_ref_table_offers_workspace_tables() {
    let pool = ParserPool::new();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();

    let user_uri = uri("user.yaml");
    let user_src = "name: user\ncolumns:\n  - name: id\n    type: integer\n    primary_key: true\n";
    let user_tree = pool.parse(user_src, DocumentFormat::Yaml).unwrap();
    idx.upsert(&user_uri, user_src, &user_tree);

    let post_src = "name: post\ncolumns:\n  - name: author_id\n    type: integer\n    foreign_key:\n      ref_table: \"\"\n      ref_columns: [id]\n";
    let post_tree = pool.parse(post_src, DocumentFormat::Yaml);
    let pos = post_src.find(r#"ref_table: """#).unwrap() + 12;
    let items = compute_completion(
        post_src,
        DocumentFormat::Yaml,
        post_tree.as_ref(),
        &idx,
        &docs,
        pos,
    );

    assert!(
        items.iter().any(|i| i.label == "user"),
        "YAML ref_table should suggest workspace tables, got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[test]
fn yaml_default_for_timestamp_offers_now() {
    let pool = ParserPool::new();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();
    let src = "name: u\ncolumns:\n  - name: created_at\n    type: timestamp\n    default: \"\"\n";
    let tree = pool.parse(src, DocumentFormat::Yaml);
    let pos = src.find(r#"default: """#).unwrap() + 10;
    let items = compute_completion(src, DocumentFormat::Yaml, tree.as_ref(), &idx, &docs, pos);

    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"now()"),
        "YAML default for timestamp should offer now(), got: {labels:?}"
    );
    assert!(
        labels.contains(&"CURRENT_TIMESTAMP"),
        "YAML default should offer CURRENT_TIMESTAMP, got: {labels:?}"
    );
}

#[test]
fn yaml_default_for_string_enum_offers_only_its_values() {
    let pool = ParserPool::new();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();
    let src = "name: u\ncolumns:\n  - name: status\n    type:\n      kind: enum\n      name: s\n      values: [active, banned]\n    default: \"\"\n";
    let tree = pool.parse(src, DocumentFormat::Yaml);
    let pos = src.rfind(r#"default: """#).unwrap() + 10;
    let items = compute_completion(src, DocumentFormat::Yaml, tree.as_ref(), &idx, &docs, pos);

    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"'active'"),
        "YAML enum default must surface 'active', got: {labels:?}"
    );
    assert!(
        labels.contains(&"'banned'"),
        "YAML enum default must surface 'banned', got: {labels:?}"
    );
    assert!(
        !labels.contains(&"now()"),
        "enum column must not leak timestamp defaults, got: {labels:?}"
    );
}
