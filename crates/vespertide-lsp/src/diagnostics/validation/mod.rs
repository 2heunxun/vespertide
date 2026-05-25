//! Validation routines: syntax → parse → planner.

mod cache;
mod parse;
mod types;
mod visitors;

use tower_lsp_server::ls_types::Uri;
use vespertide_core::TableDef;

use super::{DomainDiagnostic, Severity};

pub(super) use parse::{try_parse_json, try_parse_yaml};
pub(super) use visitors::collect_all;
// Per-collector entry points exist only as test oracles for
// `fused_walk_matches_unfused_pipeline` (see diagnostics/mod.rs). Production
// uses the fused `collect_all` path exclusively.
#[cfg(test)]
pub(super) use visitors::{
    collect_complex_type_violations, collect_duplicate_column_names, collect_syntax_errors,
    collect_unknown_column_types,
};

/// Parsed table plus source context for workspace-wide validation.
pub struct WorkspaceTable {
    /// URI that owns this table definition.
    pub uri: Uri,
    /// Normalized table definition used by planner validation.
    pub table: TableDef,
    /// Raw document text used for byte-range location.
    pub source: String,
    /// Parsed tree-sitter tree for source range lookup.
    pub tree: Option<tree_sitter::Tree>,
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

/// Compare the file's basename to its declared table `name` and surface a
/// warning when they diverge. This catches accidental renames where the
/// user changes `"name"` but forgets to rename the file (or vice versa).
///
/// Path → basename rules (longest extension wins):
///   `foo.vespertide.json` → `foo`
///   `foo.vespertide.yaml` → `foo`
///   `foo.vespertide.yml`  → `foo`
///   `foo.json` / `foo.yaml` / `foo.yml` → `foo`
pub(super) fn check_filename_table_name_mismatch(
    text: &str,
    uri: &Uri,
    tree: Option<&tree_sitter::Tree>,
    table_name: &str,
    out: &mut Vec<DomainDiagnostic>,
) {
    let Some(file_basename) = file_basename_of(uri) else {
        return;
    };
    if file_basename == table_name {
        return;
    }
    let byte_range = super::locator::locate_top_name(tree, text).unwrap_or(0..1);
    out.push(DomainDiagnostic {
        byte_range,
        severity: Severity::Warning,
        message: format!(
            "Table name `{table_name}` does not match file basename `{file_basename}`. \
             Rename one to keep them in sync."
        ),
        code: "filename-mismatch".to_string(),
    });
}

fn file_basename_of(uri: &Uri) -> Option<String> {
    let path = crate::position::uri_to_path(uri)?;
    let file_name = path.file_name()?.to_str()?;
    let stripped = file_name
        .strip_suffix(".vespertide.json")
        .or_else(|| file_name.strip_suffix(".vespertide.yaml"))
        .or_else(|| file_name.strip_suffix(".vespertide.yml"))
        .or_else(|| file_name.strip_suffix(".json"))
        .or_else(|| file_name.strip_suffix(".yaml"))
        .or_else(|| file_name.strip_suffix(".yml"))
        .unwrap_or(file_name);
    Some(stripped.to_string())
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
        if let Some(field) = location.field {
            super::locator::locate_column_field(target.tree.as_ref(), &target.source, column, field)
        } else {
            super::locator::locate_column(target.tree.as_ref(), &target.source, column)
        }
    } else if let Some(constraint) = &location.constraint {
        super::locator::locate_constraint(target.tree.as_ref(), &target.source, constraint)
    } else {
        super::locator::locate_top_name(target.tree.as_ref(), &target.source).unwrap_or(0..1)
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
