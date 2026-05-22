//! Diagnostics — pure domain layer.
//!
//! `DomainDiagnostic` has zero LSP types. The backend (or external callers)
//! translate to `tower_lsp_server::ls_types::Diagnostic` via `mapper`.
//!
//! Validation tiers:
//! 1. Tree-sitter syntax errors → `Severity::Error`
//! 2. serde parse failure → `Severity::Error`
//! 3. `vespertide_planner::validate_schema` (per-table) → `Severity::Error` / `Severity::Warning`
//! 4. (future, drift detection) → `Severity::Information`

use std::ops::Range;

use crate::parser::DocumentFormat;
use crate::workspace_index::WorkspaceIndex;

pub mod locator;
pub mod mapper;
pub mod validation;

pub use validation::WorkspaceTable;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainDiagnostic {
    /// Byte range in source text [start, end).
    pub byte_range: Range<usize>,
    pub severity: Severity,
    pub message: String,
    /// Stable diagnostic code (e.g., "syntax-error", "fk-missing", "validate-schema").
    pub code: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Information,
    Hint,
}

/// Compute diagnostics for a document. Pure function — no I/O, no LSP types.
#[must_use]
pub fn compute(
    text: &str,
    format: DocumentFormat,
    tree: Option<&tree_sitter::Tree>,
    _index: &WorkspaceIndex,
) -> Vec<DomainDiagnostic> {
    let mut diagnostics = Vec::new();

    // Tier 1: syntax errors from tree-sitter.
    if let Some(tree) = tree {
        validation::collect_syntax_errors(tree, &mut diagnostics);
        // Tier 1.5a: precise unknown-type detection BEFORE serde, because
        // serde's untagged-enum errors report a misleading byte position.
        validation::collect_unknown_column_types(tree, text, &mut diagnostics);
        // Tier 1.5b: precise complex-type detection (missing kind, missing
        // required fields, duplicate enum values).
        validation::collect_complex_type_violations(tree, text, &mut diagnostics);
        // Tier 1.5c: duplicate column-name detection — surface on the
        // SECOND offending column object, not on the table.
        validation::collect_duplicate_column_names(tree, text, &mut diagnostics);
    }

    let had_typed_pre_check = had_typed_pre_check(&diagnostics);

    // Tier 2: serde parse.
    let parsed = if had_typed_pre_check {
        None
    } else {
        match format {
            DocumentFormat::Json => validation::try_parse_json(text, &mut diagnostics),
            DocumentFormat::Yaml => validation::try_parse_yaml(text, &mut diagnostics),
        }
    };

    // Tier 3: planner validation (only if serde succeeded).
    if let Some(table) = parsed {
        validation::validate_table(&table, &mut diagnostics);
    }

    diagnostics
}

/// True when at least one tree-sitter-level pre-pass already pinpointed a
/// type-shape error. Used to suppress redundant (and mis-positioned) serde
/// diagnostics for the same root cause.
fn had_typed_pre_check(diagnostics: &[DomainDiagnostic]) -> bool {
    diagnostics.iter().any(|d| {
        matches!(
            d.code.as_str(),
            "unknown-type" | "complex-type" | "duplicate-column"
        )
    })
}

/// Compute diagnostics with workspace context for cross-file validation.
#[must_use]
pub fn compute_workspace(
    text: &str,
    format: DocumentFormat,
    tree: Option<&tree_sitter::Tree>,
    workspace: &[WorkspaceTable],
    current_uri: &tower_lsp_server::ls_types::Uri,
) -> Vec<DomainDiagnostic> {
    let mut diagnostics = Vec::new();

    if let Some(tree) = tree {
        validation::collect_syntax_errors(tree, &mut diagnostics);
        validation::collect_unknown_column_types(tree, text, &mut diagnostics);
        validation::collect_complex_type_violations(tree, text, &mut diagnostics);
        validation::collect_duplicate_column_names(tree, text, &mut diagnostics);
    }

    let had_typed = had_typed_pre_check(&diagnostics);

    let parsed = if had_typed {
        None
    } else {
        match format {
            DocumentFormat::Json => validation::try_parse_json(text, &mut diagnostics),
            DocumentFormat::Yaml => validation::try_parse_yaml(text, &mut diagnostics),
        }
    };

    if parsed.is_some() {
        // Filename/table-name consistency check — warning-level so the user
        // can still ship, but visible enough not to be missed.
        if let Some(entry) = workspace.iter().find(|t| t.uri == *current_uri) {
            validation::check_filename_table_name_mismatch(
                text,
                current_uri,
                tree,
                entry.table.name.as_str(),
                &mut diagnostics,
            );
        }
        validation::validate_workspace(workspace, current_uri, &mut diagnostics);
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DocumentFormat, ParserPool};

    #[test]
    fn valid_table_no_diagnostics() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let src = r#"{
            "name": "user",
            "columns": [
                { "name": "id", "type": "integer", "nullable": false, "primary_key": true }
            ]
        }"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref(), &idx);
        assert!(diags.is_empty(), "expected zero diagnostics, got {diags:?}");
    }

    #[test]
    fn truncated_json_produces_syntax_error() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let src = r#"{"name": "user","#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref(), &idx);
        assert!(!diags.is_empty());
        assert!(
            diags
                .iter()
                .any(|d| d.code == "syntax-error" || d.code == "parse-error")
        );
    }

    #[test]
    fn unknown_column_type_highlights_type_pair_not_braces() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let src = r#"{"name":"u","columns":[{"name":"id","type":"wrong","nullable":false,"primary_key":true}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref(), &idx);

        let err = diags
            .iter()
            .find(|d| d.code == "unknown-type")
            .expect("expected an unknown-type diagnostic");
        let snippet = &src[err.byte_range.clone()];

        assert!(
            snippet.starts_with(r#""type""#),
            "diagnostic should start at the `type` key, got: {snippet}"
        );
        assert!(
            snippet.contains("wrong"),
            "diagnostic should cover the bad value `wrong`, got: {snippet}"
        );
        assert!(
            !snippet.ends_with('}'),
            "diagnostic must NOT bleed onto the column's closing brace"
        );
        // And the redundant serde parse-error must be suppressed.
        assert!(
            !diags.iter().any(|d| d.code == "parse-error"),
            "parse-error should be suppressed when unknown-type fired"
        );
    }

    #[test]
    fn known_simple_types_produce_no_unknown_type_diagnostic() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref(), &idx);
        assert!(diags.iter().all(|d| d.code != "unknown-type"));
    }

    #[test]
    fn yaml_unknown_column_type_highlights_type_pair() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let src = "name: u\ncolumns:\n  - name: id\n    type: wrong\n    nullable: false\n    primary_key: true\n";
        let tree = pool.parse(src, DocumentFormat::Yaml);
        let diags = compute(src, DocumentFormat::Yaml, tree.as_ref(), &idx);

        let err = diags
            .iter()
            .find(|d| d.code == "unknown-type")
            .expect("YAML unknown-type diagnostic missing");
        let snippet = &src[err.byte_range.clone()];
        assert!(
            snippet.contains("type:"),
            "snippet should cover the YAML `type:` pair, got: {snippet:?}"
        );
        assert!(
            snippet.contains("wrong"),
            "snippet should include the bad value, got: {snippet:?}"
        );
    }

    #[test]
    fn yaml_valid_simple_type_produces_no_unknown_type_diagnostic() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let src = "name: u\ncolumns:\n  - name: id\n    type: uuid\n    nullable: false\n    primary_key: true\n";
        let tree = pool.parse(src, DocumentFormat::Yaml);
        let diags = compute(src, DocumentFormat::Yaml, tree.as_ref(), &idx);
        assert!(diags.iter().all(|d| d.code != "unknown-type"));
    }

    #[test]
    fn yaml_complex_type_object_skips_unknown_type_check() {
        // varchar lives in an object, not a string. The pre-pass must skip it.
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let src = "name: u\ncolumns:\n  - name: title\n    type: {kind: varchar, length: 200}\n    nullable: false\n";
        let tree = pool.parse(src, DocumentFormat::Yaml);
        let diags = compute(src, DocumentFormat::Yaml, tree.as_ref(), &idx);
        assert!(
            diags.iter().all(|d| d.code != "unknown-type"),
            "object type values must not trigger unknown-type, got: {diags:?}"
        );
    }

    #[test]
    fn enum_without_values_field_emits_complex_type_error() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let src = r#"{"name":"u","columns":[{"name":"status","type":{"kind":"enum","name":"s"},"nullable":false}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref(), &idx);

        let err = diags
            .iter()
            .find(|d| d.code == "complex-type")
            .expect("expected complex-type diagnostic");
        assert!(
            err.message.contains("values"),
            "message should mention missing `values`, got: {}",
            err.message
        );
        // No redundant serde parse-error.
        assert!(diags.iter().all(|d| d.code != "parse-error"));
    }

    #[test]
    fn enum_with_empty_values_array_is_flagged() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let src = r#"{"name":"u","columns":[{"name":"s","type":{"kind":"enum","name":"st","values":[]}}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref(), &idx);

        let err = diags
            .iter()
            .find(|d| d.code == "complex-type")
            .expect("empty `values` should be flagged");
        assert!(err.message.contains("non-empty"), "got: {}", err.message);
    }

    #[test]
    fn enum_string_duplicate_value_is_flagged_on_the_offending_element() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let src = r#"{"name":"u","columns":[{"name":"s","type":{"kind":"enum","name":"st","values":["active","banned","active"]}}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref(), &idx);

        let err = diags
            .iter()
            .find(|d| d.code == "complex-type" && d.message.contains("Duplicate enum value"))
            .expect("duplicate enum string value should be flagged");
        // Range must point at the SECOND `"active"`, not the whole column.
        let snippet = &src[err.byte_range.clone()];
        assert_eq!(
            snippet, r#""active""#,
            "diagnostic should land on the duplicate element, got: {snippet}"
        );
        // The second occurrence is later in the file.
        let first = src.find(r#""active""#).unwrap();
        assert!(err.byte_range.start > first);
    }

    #[test]
    fn varchar_without_length_field_is_flagged() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let src = r#"{"name":"u","columns":[{"name":"title","type":{"kind":"varchar"}}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref(), &idx);

        let err = diags
            .iter()
            .find(|d| d.code == "complex-type")
            .expect("varchar without length should be flagged");
        assert!(err.message.contains("length"), "got: {}", err.message);
    }

    #[test]
    fn numeric_missing_precision_and_scale_is_flagged() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let src = r#"{"name":"u","columns":[{"name":"amount","type":{"kind":"numeric"}}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref(), &idx);

        let err = diags
            .iter()
            .find(|d| d.code == "complex-type")
            .expect("numeric without precision/scale should be flagged");
        assert!(err.message.contains("precision"));
        assert!(err.message.contains("scale"));
    }

    #[test]
    fn unknown_complex_kind_is_flagged_on_kind_pair() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let src = r#"{"name":"u","columns":[{"name":"x","type":{"kind":"nope"}}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref(), &idx);

        let err = diags
            .iter()
            .find(|d| d.code == "complex-type" && d.message.contains("Unknown type kind"))
            .expect("unknown kind should be flagged");
        let snippet = &src[err.byte_range.clone()];
        assert!(
            snippet.starts_with("\"kind\""),
            "diagnostic should land on the `kind` pair, got: {snippet}"
        );
    }

    #[test]
    fn integer_enum_duplicate_numeric_value_is_flagged() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let src = r#"{"name":"u","columns":[{"name":"p","type":{"kind":"enum","name":"pl","values":[{"name":"low","value":0},{"name":"high","value":0}]}}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref(), &idx);

        let err = diags
            .iter()
            .find(|d| {
                d.code == "complex-type" && d.message.contains("Duplicate enum numeric value")
            })
            .expect("duplicate integer enum value should be flagged");
        let snippet = &src[err.byte_range.clone()];
        assert_eq!(snippet, "0", "diagnostic should land on the duplicate `0`");
    }

    /// Regression — integer-enum members `{"name": "low", "value": 0}`
    /// inside `type.values` MUST NOT be treated as columns. A recursive
    /// descent over the `columns` array would otherwise see their `name`
    /// fields and either flag false duplicates or land table-level
    /// diagnostics on enum members.
    #[test]
    fn enum_integer_member_name_is_not_treated_as_column() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        // `priority.value` enum has a member literally called `id`, the
        // same as the first column's name. This MUST NOT trigger a
        // duplicate-column diagnostic.
        let src = r#"{
            "name": "u",
            "columns": [
                {"name": "id", "type": "integer", "nullable": false, "primary_key": true},
                {
                    "name": "priority",
                    "type": {
                        "kind": "enum",
                        "name": "pl",
                        "values": [
                            {"name": "id", "value": 0},
                            {"name": "high", "value": 10}
                        ]
                    },
                    "nullable": false
                }
            ]
        }"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref(), &idx);
        assert!(
            diags.iter().all(|d| d.code != "duplicate-column"),
            "enum member name `id` must not collide with column name `id`, got: {diags:#?}"
        );
    }

    #[test]
    fn duplicate_column_name_pinpoints_the_second_occurrence() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"id","type":"text","nullable":true}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref(), &idx);

        let dup = diags
            .iter()
            .find(|d| d.code == "duplicate-column")
            .expect("expected duplicate-column diagnostic");
        let first = src.find(r#""name":"id""#).unwrap();
        // Diagnostic should land on the SECOND `"id"`, not the first.
        assert!(dup.byte_range.start > first + 5);
        let snippet = &src[dup.byte_range.clone()];
        assert_eq!(
            snippet, r#""id""#,
            "diagnostic should highlight the duplicate `id`"
        );

        // And no `validate-schema` "duplicate table name" surfaces here —
        // this is a column-level issue, not a workspace duplication.
        assert!(
            diags
                .iter()
                .all(|d| !d.message.contains("duplicate table name"))
        );
    }

    /// Regression — when `columns` precedes `name` at the top level, the
    /// locator used to walk into the first column and land "table" errors
    /// on a column's `name`, e.g. "duplicate table name: article" showing
    /// up on `id`. Make sure `locate_top_name` returns the OUTER `name`.
    #[test]
    fn locate_top_name_is_not_confused_when_columns_precede_name() {
        use crate::diagnostics::locator;
        let pool = ParserPool::new();
        let src = r#"{
            "columns": [
                {"name": "id", "type": "integer"}
            ],
            "name": "article"
        }"#;
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();
        let range = locator::locate_top_name(Some(&tree), src).expect("range");
        let snippet = &src[range];
        assert!(
            snippet.contains("article"),
            "expected table-level name `article`, got: {snippet}"
        );
    }

    #[test]
    fn missing_columns_field_produces_validation_error() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        // Valid JSON syntax but missing required `columns`.
        let src = r#"{"name": "user"}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref(), &idx);
        // Either serde rejects (parse-error) or validate_schema rejects. Both
        // are acceptable as long as we emit something.
        assert!(!diags.is_empty());
    }
}
