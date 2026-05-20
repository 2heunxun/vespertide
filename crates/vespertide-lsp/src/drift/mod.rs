//! Drift detection — KILLER FEATURE.
//!
//! Compares current model files against the schema reconstructed from applied
//! migrations. Surfaces drift as workspace-level diagnostics so users can
//! generate a migration before forgetting.
//!
//! No live DB connection required — pure file-based comparison. This is the
//! feature no competitor (Prisma/sqls/postgres-lsp) provides.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use tower_lsp_server::ls_types::Uri;
use vespertide_config::VespertideConfig;
use vespertide_core::MigrationAction;

use crate::store::DocumentStore;
use crate::workspace_index::WorkspaceIndex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainDrift {
    /// URI of the model file with drift.
    pub uri: Uri,
    /// Number of pending actions for this table.
    pub pending_count: usize,
    /// Brief summary of changes.
    pub summary: String,
}

/// Compute drift across the entire workspace.
///
/// Returns an empty vector when `vespertide.json` is not found or any loader /
/// planner step fails. Drift diagnostics are best-effort and must never block
/// normal LSP feedback.
pub fn compute(
    workspace_root: &Path,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
) -> Vec<DomainDrift> {
    let _ = docs;

    let Some((project_root, config)) = find_and_load_config(workspace_root) else {
        return Vec::new();
    };

    let Ok(current_models) = vespertide_loader::load_models_from_dir(Some(project_root.clone()))
    else {
        return Vec::new();
    };
    let Ok(applied_plans) = vespertide_loader::load_migrations_from_dir(Some(project_root.clone()))
    else {
        return Vec::new();
    };

    let Ok(baseline) = vespertide_planner::schema_from_plans(&applied_plans) else {
        return Vec::new();
    };
    let Ok(plan) = vespertide_planner::diff_schemas(&baseline, &current_models) else {
        return Vec::new();
    };

    if plan.actions.is_empty() {
        return Vec::new();
    }

    let mut by_table: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for action in &plan.actions {
        let Some(table_name) = action.table_name() else {
            continue;
        };
        by_table
            .entry(table_name.to_string())
            .or_default()
            .push(action_summary(action));
    }

    let models_dir = resolve_models_dir(&project_root, &config);
    by_table
        .into_iter()
        .filter_map(|(name, summaries)| {
            let uri = index
                .lookup(&name)
                .map(|loc| loc.uri)
                .or_else(|| guess_uri(&models_dir, &name))?;
            let pending_count = summaries.len();
            Some(DomainDrift {
                uri,
                pending_count,
                summary: format!(
                    "{pending_count} pending change(s): {}",
                    summaries.join(", ")
                ),
            })
        })
        .collect()
}

fn find_and_load_config(start: &Path) -> Option<(PathBuf, VespertideConfig)> {
    let mut current = if start.is_file() {
        start.parent()
    } else {
        Some(start)
    };

    while let Some(dir) = current {
        let candidate = dir.join("vespertide.json");
        if candidate.exists() {
            let config = vespertide_loader::load_config_from_path(candidate).ok()?;
            return Some((dir.to_path_buf(), config));
        }
        current = dir.parent();
    }

    None
}

fn resolve_models_dir(root: &Path, config: &VespertideConfig) -> PathBuf {
    root.join(config.models_dir())
}

fn guess_uri(models_dir: &Path, table_name: &str) -> Option<Uri> {
    for ext in ["json", "yaml", "yml"] {
        let path = models_dir.join(format!("{table_name}.{ext}"));
        if path.exists() {
            return path_to_uri(&path);
        }
    }
    None
}

fn path_to_uri(path: &Path) -> Option<Uri> {
    let mut path_text = path.to_string_lossy().replace('\\', "/");
    if !path_text.starts_with('/') {
        path_text = format!("/{path_text}");
    }
    Uri::from_str(&format!("file://{path_text}")).ok()
}

fn action_summary(action: &MigrationAction) -> String {
    match action {
        MigrationAction::CreateTable { .. } => "CreateTable".to_string(),
        MigrationAction::DeleteTable { .. } => "DeleteTable".to_string(),
        MigrationAction::AddColumn { column, .. } => format!("AddColumn({})", column.name),
        MigrationAction::RenameColumn { from, to, .. } => format!("RenameColumn({from}→{to})"),
        MigrationAction::DeleteColumn { column, .. } => format!("DeleteColumn({column})"),
        MigrationAction::ModifyColumnType { column, .. } => {
            format!("ModifyColumnType({column})")
        }
        MigrationAction::ModifyColumnNullable { column, .. } => {
            format!("ModifyColumnNullable({column})")
        }
        MigrationAction::ModifyColumnDefault { column, .. } => {
            format!("ModifyColumnDefault({column})")
        }
        MigrationAction::ModifyColumnComment { column, .. } => {
            format!("ModifyColumnComment({column})")
        }
        MigrationAction::AddConstraint { .. } => "AddConstraint".to_string(),
        MigrationAction::RemoveConstraint { .. } => "RemoveConstraint".to_string(),
        MigrationAction::ReplaceConstraint { .. } => "ReplaceConstraint".to_string(),
        MigrationAction::RenameTable { .. } => "RenameTable".to_string(),
        MigrationAction::RawSql { .. } => "RawSql".to_string(),
        _ => "Unknown".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn no_config_returns_empty() {
        let tmp = tempdir().unwrap();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let drifts = compute(tmp.path(), &idx, &docs);

        assert!(drifts.is_empty());
    }
}
