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

pub mod mapper;
mod validation;

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
    }

    // Tier 2: serde parse.
    let parsed = match format {
        DocumentFormat::Json => validation::try_parse_json(text, &mut diagnostics),
        DocumentFormat::Yaml => validation::try_parse_yaml(text, &mut diagnostics),
    };

    // Tier 3: planner validation (only if serde succeeded).
    if let Some(table) = parsed {
        validation::validate_table(&table, &mut diagnostics);
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
