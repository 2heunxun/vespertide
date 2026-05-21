//! Validation routines: syntax → parse → planner.

use tower_lsp_server::ls_types::Uri;
use vespertide_core::TableDef;

use super::{DomainDiagnostic, Severity};

/// Parsed table plus source context for workspace-wide validation.
pub struct WorkspaceTable {
    /// URI that owns this table definition.
    pub uri: Uri,
    /// Normalized table definition used by planner validation.
    pub table: TableDef,
    /// Raw document text used for byte-range location.
    pub source: String,
    /// Parsed tree-sitter tree for source range lookup.
    pub tree: tree_sitter::Tree,
}

pub(super) fn collect_syntax_errors(tree: &tree_sitter::Tree, out: &mut Vec<DomainDiagnostic>) {
    let root = tree.root_node();
    if root.has_error() {
        walk_for_errors(root, out);
    }
}

fn walk_for_errors(node: tree_sitter::Node<'_>, out: &mut Vec<DomainDiagnostic>) {
    if node.is_error() || node.is_missing() {
        out.push(DomainDiagnostic {
            byte_range: node.byte_range(),
            severity: Severity::Error,
            message: if node.is_missing() {
                format!("Missing {}", node.kind())
            } else {
                "Syntax error".to_string()
            },
            code: "syntax-error".to_string(),
        });
        return;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_for_errors(child, out);
    }
}

pub(super) fn try_parse_json(text: &str, out: &mut Vec<DomainDiagnostic>) -> Option<TableDef> {
    match serde_json::from_str::<TableDef>(text) {
        Ok(table) => normalize_table(&table, out),
        Err(e) => {
            let byte = byte_offset_for_line_col(text, e.line(), e.column());
            out.push(DomainDiagnostic {
                byte_range: byte..(byte + 1).min(text.len()),
                severity: Severity::Error,
                message: format!("JSON parse error: {e}"),
                code: "parse-error".to_string(),
            });
            None
        }
    }
}

pub(super) fn try_parse_yaml(text: &str, out: &mut Vec<DomainDiagnostic>) -> Option<TableDef> {
    match serde_yaml::from_str::<TableDef>(text) {
        Ok(table) => normalize_table(&table, out),
        Err(e) => {
            let byte = e.location().map_or(0, |loc| loc.index().min(text.len()));
            out.push(DomainDiagnostic {
                byte_range: byte..(byte + 1).min(text.len()),
                severity: Severity::Error,
                message: format!("YAML parse error: {e}"),
                code: "parse-error".to_string(),
            });
            None
        }
    }
}

/// Run `TableDef::normalize()` so inline constraints participate in planner validation.
fn normalize_table(table: &TableDef, out: &mut Vec<DomainDiagnostic>) -> Option<TableDef> {
    match table.normalize() {
        Ok(table) => Some(table),
        Err(e) => {
            out.push(DomainDiagnostic {
                byte_range: 0..1,
                severity: Severity::Error,
                message: e.to_string(),
                code: "validate-schema".to_string(),
            });
            None
        }
    }
}

pub(super) fn validate_table(table: &TableDef, out: &mut Vec<DomainDiagnostic>) {
    // Single-table validation. `vespertide_planner::validate_schema` expects
    // `&[TableDef]`; for LSP per-file diagnostics, run on a singleton slice.
    if let Err(e) = vespertide_planner::validate_schema(std::slice::from_ref(table)) {
        out.push(DomainDiagnostic {
            byte_range: 0..1,
            severity: Severity::Error,
            message: e.to_string(),
            code: "validate-schema".to_string(),
        });
    }
}

pub(super) fn validate_workspace(
    workspace: &[WorkspaceTable],
    current_uri: &Uri,
    out: &mut Vec<DomainDiagnostic>,
) {
    let tables: Vec<TableDef> = workspace.iter().map(|entry| entry.table.clone()).collect();
    let Err(err) = vespertide_planner::validate_schema(&tables) else {
        return;
    };

    let Some(location) = super::locator::ErrorLocation::from_planner_error(&err) else {
        push_validate_error(out, 0..1, err.to_string());
        return;
    };

    let Some(target) = workspace
        .iter()
        .find(|entry| entry.table.name.as_str() == location.table.as_str())
    else {
        push_validate_error(out, 0..1, err.to_string());
        return;
    };

    if target.uri != *current_uri {
        return;
    }

    let byte_range = if let Some(column) = &location.column {
        super::locator::locate_column(&target.tree, &target.source, column)
    } else if let Some(constraint) = &location.constraint {
        super::locator::locate_constraint(&target.tree, &target.source, constraint)
    } else {
        super::locator::locate_top_name(&target.tree, &target.source).unwrap_or(0..1)
    };

    push_validate_error(out, byte_range, err.to_string());
}

fn push_validate_error(
    out: &mut Vec<DomainDiagnostic>,
    byte_range: std::ops::Range<usize>,
    message: String,
) {
    out.push(DomainDiagnostic {
        byte_range,
        severity: Severity::Error,
        message,
        code: "validate-schema".to_string(),
    });
}

fn byte_offset_for_line_col(text: &str, line: usize, col: usize) -> usize {
    // serde_json line/column values are 1-indexed.
    let line_zero = line.saturating_sub(1);
    let col_zero = col.saturating_sub(1);
    let mut byte = 0;

    for (idx, line_text) in text.split_inclusive('\n').enumerate() {
        if idx == line_zero {
            return byte + col_zero.min(line_text.len().saturating_sub(1));
        }
        byte += line_text.len();
    }

    byte.min(text.len())
}
