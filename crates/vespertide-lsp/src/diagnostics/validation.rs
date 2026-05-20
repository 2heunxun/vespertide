//! Validation routines: syntax → parse → planner.

use vespertide_core::TableDef;

use super::{DomainDiagnostic, Severity};

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
