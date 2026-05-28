//! Hover tests for CHECK constraint expressions (`constraints[*].expr`).
//!
//! Verifies the new check-expr hover sub-handler:
//! 1. Inside a parseable expr → markdown describes the parsed structure.
//! 2. Inside an unparseable expr → graceful (no panic), markdown notes
//!    it could not be structurally parsed.
//! 3. Regression — hovering on `ref_table` still returns the existing
//!    foreign-key hover, proving dispatch ordering did not break FK
//!    or column hover.

use std::str::FromStr;

use tower_lsp_server::ls_types::Uri;
use vespertide_lsp::{DocumentFormat, DocumentStore, ParserPool, WorkspaceIndex, compute_hover};

fn uri(path: &str) -> Uri {
    Uri::from_str(&format!("file:///{path}")).unwrap()
}

#[test]
fn h_s1_hover_inside_and_describes_structure() {
    let pool = ParserPool::new();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();
    let src = r#"{"name":"users","columns":[{"name":"age","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"chk_age","expr":"age > 0 AND age < 150"}]}"#;
    let tree = pool.parse(src, DocumentFormat::Json);

    // Cursor inside the `AND` keyword in the expression.
    let needle = "age > 0 AND";
    let needle_pos = src.find(needle).expect("needle present");
    let pos = needle_pos + needle.len() - 2; // somewhere inside "AND"

    let hover = compute_hover(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos)
        .expect("hover inside a parseable CHECK expression must return Some");

    // Markdown must reflect the parsed AND-of-2 structure.
    assert!(
        hover.markdown.contains("AND"),
        "markdown should describe the AND structure, got: {}",
        hover.markdown
    );
    assert!(
        hover.markdown.to_lowercase().contains("condition")
            || hover.markdown.to_lowercase().contains("predicate"),
        "markdown should refer to conditions/predicates, got: {}",
        hover.markdown
    );
    assert!(
        hover.markdown.contains("age > 0"),
        "markdown should mention the first sub-expression `age > 0`, got: {}",
        hover.markdown
    );
    assert!(
        hover.markdown.contains("age < 150"),
        "markdown should mention the second sub-expression `age < 150`, got: {}",
        hover.markdown
    );

    // byte_range must be inside the expr value, not 0..1 fallback.
    let expr_inner_start = src.find(r#""expr":""#).unwrap() + r#""expr":""#.len();
    let expr_inner_end = src[expr_inner_start..].find('"').unwrap() + expr_inner_start;
    assert!(
        hover.byte_range.start >= expr_inner_start
            && hover.byte_range.end <= expr_inner_end
            && hover.byte_range.start < hover.byte_range.end,
        "byte_range must lie inside the expr value [{expr_inner_start}..{expr_inner_end}), got {:?}",
        hover.byte_range
    );
}

#[test]
fn h_s2_hover_unparseable_is_graceful() {
    let pool = ParserPool::new();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();
    // `LOWER(x) = 1` is intentionally outside the dialect-neutral subset
    // recognised by `parse_check_expr` and folds to `Unparseable`.
    let src = r#"{"name":"t","columns":[{"name":"x","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"c","expr":"LOWER(x) = 1"}]}"#;
    let tree = pool.parse(src, DocumentFormat::Json);

    let needle = "LOWER(";
    let pos = src.find(needle).expect("needle present") + 2; // inside "LOWER"

    // Must not panic regardless of which branch the impl chooses.
    let hover = compute_hover(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos);

    // Allowed: Some with a note that structure couldn't be parsed, OR None.
    if let Some(h) = hover {
        let lower = h.markdown.to_lowercase();
        assert!(
            lower.contains("check") && (lower.contains("parse") || lower.contains("structure")),
            "Unparseable-case markdown must mention CHECK + parse/structure, got: {}",
            h.markdown
        );
    }
}

#[test]
fn h_s3_hover_on_ref_table_still_works() {
    let pool = ParserPool::new();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();

    // Register a `user` table so the FK target resolves.
    let user_uri = uri("user.json");
    let user_src = r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#;
    let user_tree = pool.parse(user_src, DocumentFormat::Json).unwrap();
    idx.upsert(&user_uri, user_src, &user_tree);
    docs.open(
        user_uri.clone(),
        "json".to_string(),
        1,
        user_src.to_string(),
    );

    // Model contains both a FK and a CHECK constraint to exercise dispatch.
    let post_src = r#"{"name":"post","columns":[{"name":"author_id","type":"integer","nullable":false,"foreign_key":{"ref_table":"user","ref_columns":["id"]}}],"constraints":[{"type":"check","name":"chk_pos","expr":"author_id > 0"}]}"#;
    let post_tree = pool.parse(post_src, DocumentFormat::Json);
    let pos = post_src.find(r#""ref_table":"user""#).unwrap() + 14;

    let hover = compute_hover(
        post_src,
        DocumentFormat::Json,
        post_tree.as_ref(),
        &idx,
        &docs,
        pos,
    )
    .expect("hover on ref_table must still resolve");

    assert!(
        hover.markdown.contains("Target table"),
        "FK hover should still produce the target-table preview, got: {}",
        hover.markdown
    );
    assert!(
        hover.markdown.contains("user"),
        "FK hover should still mention the target table name, got: {}",
        hover.markdown
    );
}
