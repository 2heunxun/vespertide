//! Drift detection — KILLER FEATURE.
//!
//! Compares current model files against the schema reconstructed from applied
//! migrations. Surfaces drift as workspace-level diagnostics so users can
//! generate a migration before forgetting.
//!
//! No live DB connection required — pure file-based comparison. This is the
//! feature no competitor (Prisma/sqls/postgres-lsp) provides.

mod cache;

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, OnceLock};

use tower_lsp_server::ls_types::Uri;
use tree_sitter::Tree;
use vespertide_config::VespertideConfig;
use vespertide_core::{ColumnDef, ColumnType, MigrationAction, TableConstraint, TableDef};

use crate::diagnostics::{
    ErrorField, locate_column, locate_column_field, locate_constraint, locate_top_name,
};
use crate::parser::{DocumentFormat, ParserPool};
use crate::store::DocumentStore;
use crate::workspace_index::WorkspaceIndex;

pub use cache::DriftCache;

const _: fn(&vespertide_planner::PlannerError) -> Option<crate::diagnostics::ErrorLocation> =
    crate::diagnostics::ErrorLocation::from_planner_error;

type DriftRecord = (DriftKind, Option<Range<usize>>, String);

/// Categorizes a single migration action for drift diagnostics.
///
/// Each variant corresponds to a specific type of schema change. The `code()` method
/// returns a stable diagnostic code suitable for LSP clients.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriftKind {
    CreateTable,
    DeleteTable,
    RenameTable {
        from: String,
        to: String,
    },
    AddColumn {
        column: String,
    },
    DeleteColumn {
        column: String,
    },
    RenameColumn {
        from: String,
        to: String,
    },
    ModifyColumnType {
        column: String,
        before: String,
        after: String,
    },
    ModifyColumnNullable {
        column: String,
        before: bool,
        after: bool,
    },
    ModifyColumnDefault {
        column: String,
        before: Option<String>,
        after: Option<String>,
    },
    ModifyColumnComment {
        column: String,
        before: Option<String>,
        after: Option<String>,
    },
    AddConstraint {
        name: Option<String>,
    },
    RemoveConstraint {
        name: Option<String>,
    },
    ReplaceConstraint {
        name: Option<String>,
    },
    RawSql,
}

impl DriftKind {
    /// Returns a stable diagnostic code for this drift kind.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::CreateTable => "drift-create-table",
            Self::DeleteTable => "drift-delete-table",
            Self::RenameTable { .. } => "drift-rename-table",
            Self::AddColumn { .. } => "drift-add-column",
            Self::DeleteColumn { .. } => "drift-delete-column",
            Self::RenameColumn { .. } => "drift-rename-column",
            Self::ModifyColumnType { .. } => "drift-modify-type",
            Self::ModifyColumnNullable { .. } => "drift-modify-nullable",
            Self::ModifyColumnDefault { .. } => "drift-modify-default",
            Self::ModifyColumnComment { .. } => "drift-modify-comment",
            Self::AddConstraint { .. } => "drift-add-constraint",
            Self::RemoveConstraint { .. } => "drift-remove-constraint",
            Self::ReplaceConstraint { .. } => "drift-replace-constraint",
            Self::RawSql => "drift-raw-sql",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainDrift {
    /// URI of the model file with drift.
    pub uri: Uri,
    /// Specific drift category for diagnostic codes and downstream routing.
    pub kind: DriftKind,
    /// Source byte range to anchor the diagnostic, when one is available.
    pub byte_range: Option<Range<usize>>,
    /// User-facing drift message.
    pub message: String,
}

impl DomainDrift {
    /// Convert into a `DomainDiagnostic`. Returns `None` when `byte_range`
    /// is `None` — those drifts have no anchorable position and are
    /// dropped silently (matches the current behaviour of skipping unknown
    /// positions).
    #[must_use]
    pub fn into_domain_diagnostic(self) -> Option<crate::diagnostics::DomainDiagnostic> {
        let range = self.byte_range?;
        Some(crate::diagnostics::DomainDiagnostic {
            byte_range: range,
            severity: crate::diagnostics::Severity::Information,
            message: self.message,
            code: self.kind.code().to_string(),
        })
    }
}

/// Same as `compute` but reuses a per-instance `DriftCache` to skip loading
/// models / migrations when no input file has changed since the last call.
/// The backend should hold one `Arc<DriftCache>` for the server lifetime and
/// pass it here on every `did_change`-triggered drift refresh.
///
/// Returns an empty vector when `vespertide.json` is not found or any loader /
/// planner step fails. Drift diagnostics are best-effort and must never block
/// normal LSP feedback.
#[must_use]
pub fn compute_with_cache(
    workspace_root: &Path,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
    cache: &DriftCache,
) -> Vec<DomainDrift> {
    static SHARED_POOL: OnceLock<ParserPool> = OnceLock::new();

    let Some((project_root, config_mtime)) = find_config_and_mtime(workspace_root) else {
        return Vec::new();
    };

    let models_dir_path = project_root.join("models");
    let migrations_dir_path = project_root.join("migrations");
    let max_model_mtime = cache::max_mtime_in_dir(&models_dir_path);
    let max_migration_mtime = cache::max_mtime_in_dir(&migrations_dir_path);
    let fingerprint = cache::docstore_fingerprint(docs);

    if let Some(cached_drifts) = cache.get_drifts(
        &project_root,
        config_mtime,
        max_model_mtime,
        max_migration_mtime,
        fingerprint,
    ) {
        return (*cached_drifts).clone();
    }

    let Some(loaded) = loaded_state_with_cache(
        &project_root,
        config_mtime,
        max_model_mtime,
        max_migration_mtime,
        cache,
    ) else {
        return Vec::new();
    };

    debug_assert_eq!(
        loaded.models_dir,
        resolve_models_dir(&project_root, &loaded.config)
    );

    let Ok(plan) = vespertide_planner::diff_schemas(&loaded.baseline, &loaded.current_models)
    else {
        return Vec::new();
    };

    if plan.actions.is_empty() {
        let drifts_arc = Arc::new(Vec::new());
        cache.store_drifts(
            project_root,
            config_mtime,
            max_model_mtime,
            max_migration_mtime,
            fingerprint,
            Arc::clone(&drifts_arc),
        );
        return (*drifts_arc).clone();
    }

    let parser_pool = SHARED_POOL.get_or_init(ParserPool::new);
    let mut drifts = Vec::new();

    for action in &plan.actions {
        let Some(table_name) = action.table_name() else {
            continue;
        };
        let Some(uri) = index
            .lookup(table_name)
            .map(|loc| loc.uri)
            .or_else(|| guess_uri(&loaded.models_dir, table_name))
        else {
            continue;
        };
        let Some((source, tree)) = source_and_tree(&uri, docs, parser_pool) else {
            continue;
        };
        let Some((kind, byte_range, message)) =
            action_to_drift(action, &loaded.baseline, &source, tree.as_ref())
        else {
            continue;
        };

        drifts.push(DomainDrift {
            uri,
            kind,
            byte_range,
            message,
        });
    }

    let drifts_arc = Arc::new(drifts);
    cache.store_drifts(
        project_root,
        config_mtime,
        max_model_mtime,
        max_migration_mtime,
        fingerprint,
        Arc::clone(&drifts_arc),
    );

    (*drifts_arc).clone()
}

fn loaded_state_with_cache(
    project_root: &Path,
    config_mtime: std::time::SystemTime,
    max_model_mtime: std::time::SystemTime,
    max_migration_mtime: std::time::SystemTime,
    cache: &DriftCache,
) -> Option<Arc<cache::LoadedState>> {
    if let Some(hit) = cache.get(
        project_root,
        config_mtime,
        max_model_mtime,
        max_migration_mtime,
    ) {
        return Some(hit);
    }

    let config =
        vespertide_loader::load_config_from_path(project_root.join("vespertide.json")).ok()?;
    let current_models =
        vespertide_loader::load_models_from_dir(Some(project_root.to_path_buf())).ok()?;
    let applied_plans =
        vespertide_loader::load_migrations_from_dir(Some(project_root.to_path_buf())).ok()?;
    let baseline = vespertide_planner::schema_from_plans(&applied_plans).ok()?;
    let models_dir = resolve_models_dir(project_root, &config);
    let loaded = Arc::new(cache::LoadedState {
        config,
        current_models,
        baseline,
        models_dir,
    });
    cache.store(
        project_root.to_path_buf(),
        config_mtime,
        max_model_mtime,
        max_migration_mtime,
        Arc::clone(&loaded),
    );
    Some(loaded)
}

/// Compute drift across the entire workspace.
///
/// Returns an empty vector when `vespertide.json` is not found or any loader /
/// planner step fails. Drift diagnostics are best-effort and must never block
/// normal LSP feedback.
#[must_use]
pub fn compute(
    workspace_root: &Path,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
) -> Vec<DomainDrift> {
    static SHARED_CACHE: OnceLock<DriftCache> = OnceLock::new();
    compute_with_cache(
        workspace_root,
        index,
        docs,
        SHARED_CACHE.get_or_init(DriftCache::new),
    )
}

fn find_config_and_mtime(start: &Path) -> Option<(PathBuf, std::time::SystemTime)> {
    let mut current = if start.is_file() {
        start.parent()
    } else {
        Some(start)
    };

    while let Some(dir) = current {
        let candidate = dir.join("vespertide.json");
        if let Ok(meta) = std::fs::metadata(&candidate)
            && meta.is_file()
        {
            return Some((dir.to_path_buf(), meta.modified().ok()?));
        }
        current = dir.parent();
    }

    None
}

fn resolve_models_dir(root: &Path, config: &VespertideConfig) -> PathBuf {
    root.join(config.models_dir())
}

fn guess_uri(models_dir: &Path, table_name: &str) -> Option<Uri> {
    let mut path = models_dir.join(table_name);
    for ext in ["json", "yaml", "yml"] {
        path.set_extension(ext);
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

fn source_and_tree(
    uri: &Uri,
    docs: &DocumentStore,
    parser_pool: &ParserPool,
) -> Option<(String, Option<Tree>)> {
    docs.with_doc(uri, |source, tree| (source.to_string(), tree.cloned()))
        .or_else(|| source_and_tree_from_disk(uri, parser_pool))
}

fn source_and_tree_from_disk(
    uri: &Uri,
    parser_pool: &ParserPool,
) -> Option<(String, Option<Tree>)> {
    let path = crate::position::uri_to_path(uri)?;
    let source = std::fs::read_to_string(path).ok()?;
    let tree = DocumentFormat::from_uri(uri).and_then(|format| parser_pool.parse(&source, format));
    Some((source, tree))
}

/// Look up a column in the baseline schema by table and column name.
pub(crate) fn lookup_baseline_column<'a>(
    baseline: &'a [TableDef],
    table_name: &str,
    column_name: &str,
) -> Option<&'a ColumnDef> {
    baseline
        .iter()
        .find(|t| t.name == table_name)
        .and_then(|table| table.columns.iter().find(|c| c.name == column_name))
}

/// Render a column type as a human-readable string.
pub(crate) fn render_column_type(t: &ColumnType) -> String {
    match t {
        ColumnType::Simple(st) => format!("{st:?}"),
        ColumnType::Complex(ct) => format!("{ct:?}"),
    }
}

/// Render a default value as a human-readable string.
pub(crate) fn render_default(d: Option<&str>) -> String {
    match d {
        Some(v) => format!("\"{v}\""),
        None => "<none>".to_string(),
    }
}

/// Render a nullable flag as a human-readable string.
pub(crate) fn render_nullable(n: bool) -> String {
    if n {
        "nullable".to_string()
    } else {
        "not null".to_string()
    }
}

/// Render a comment as a human-readable string.
pub(crate) fn render_comment(c: Option<&str>) -> String {
    match c {
        Some(v) => format!("\"{v}\""),
        None => "<none>".to_string(),
    }
}

fn action_to_drift(
    action: &MigrationAction,
    baseline: &[TableDef],
    source: &str,
    tree: Option<&Tree>,
) -> Option<DriftRecord> {
    match action {
        MigrationAction::CreateTable { table, .. } => Some((
            DriftKind::CreateTable,
            locate_table_name(tree, source),
            format!("Table '{table}' is in the model but not in any applied migration"),
        )),
        MigrationAction::DeleteTable { table } => Some((
            DriftKind::DeleteTable,
            locate_table_name(tree, source),
            format!("Table '{table}' is in applied migrations but missing from the model"),
        )),
        MigrationAction::RenameTable { from, to } => Some((
            DriftKind::RenameTable {
                from: from.to_string(),
                to: to.to_string(),
            },
            locate_table_name(tree, source),
            format!("Table rename drift: applied '{from}' → model '{to}'"),
        )),
        MigrationAction::AddColumn { column, .. } => Some((
            DriftKind::AddColumn {
                column: column.name.to_string(),
            },
            locate_column_range(tree, source, &column.name),
            format!(
                "Column '{}' is in the model but not in any applied migration",
                column.name
            ),
        )),
        MigrationAction::DeleteColumn { column, .. } => Some((
            DriftKind::DeleteColumn {
                column: column.to_string(),
            },
            locate_table_name(tree, source),
            format!("Column '{column}' is in applied migrations but missing from the model"),
        )),
        MigrationAction::RenameColumn { from, to, .. } => Some((
            DriftKind::RenameColumn {
                from: from.to_string(),
                to: to.to_string(),
            },
            locate_column_range(tree, source, to),
            format!("Column rename drift: applied '{from}' → model '{to}'"),
        )),
        MigrationAction::ModifyColumnType {
            table,
            column,
            new_type,
            ..
        } => Some(modify_column_type_drift(
            baseline, table, column, new_type, source, tree,
        )),
        MigrationAction::ModifyColumnNullable {
            table,
            column,
            nullable,
            ..
        } => Some(modify_column_nullable_drift(
            baseline, table, column, *nullable, source, tree,
        )),
        MigrationAction::ModifyColumnDefault {
            table,
            column,
            new_default,
        } => Some(modify_column_default_drift(
            baseline,
            table,
            column,
            new_default.as_ref(),
            source,
            tree,
        )),
        MigrationAction::ModifyColumnComment {
            table,
            column,
            new_comment,
        } => Some(modify_column_comment_drift(
            baseline,
            table,
            column,
            new_comment.as_ref(),
            source,
            tree,
        )),
        MigrationAction::AddConstraint { constraint, .. } => {
            Some(add_constraint_drift(constraint, source, tree))
        }
        MigrationAction::RemoveConstraint { constraint, .. } => {
            Some(remove_constraint_drift(constraint, source, tree))
        }
        MigrationAction::ReplaceConstraint { from, to, .. } => {
            Some(replace_constraint_drift(from, to, source, tree))
        }
        MigrationAction::RawSql { .. } => Some((
            DriftKind::RawSql,
            None,
            "Raw SQL drift — typed introspection unavailable".to_string(),
        )),
        _ => None,
    }
}

fn modify_column_type_drift(
    baseline: &[TableDef],
    table: &str,
    column: &str,
    new_type: &ColumnType,
    source: &str,
    tree: Option<&Tree>,
) -> DriftRecord {
    let before = lookup_baseline_column(baseline, table, column).map_or_else(
        || "<unknown>".to_string(),
        |baseline_column| render_column_type(&baseline_column.r#type),
    );
    let after = render_column_type(new_type);
    (
        DriftKind::ModifyColumnType {
            column: column.to_string(),
            before: before.clone(),
            after: after.clone(),
        },
        locate_column_field_range(tree, source, column, ErrorField::Type),
        format!("Type drift on '{column}': applied {before} → model {after}"),
    )
}

fn modify_column_nullable_drift(
    baseline: &[TableDef],
    table: &str,
    column: &str,
    nullable: bool,
    source: &str,
    tree: Option<&Tree>,
) -> DriftRecord {
    let before = lookup_baseline_column(baseline, table, column).map_or(!nullable, |c| c.nullable);
    let before_s = render_nullable(before);
    let after_s = render_nullable(nullable);
    (
        DriftKind::ModifyColumnNullable {
            column: column.to_string(),
            before,
            after: nullable,
        },
        locate_column_field_range(tree, source, column, ErrorField::Nullable),
        format!("Nullable drift on '{column}': applied {before_s} → model {after_s}"),
    )
}

fn modify_column_default_drift(
    baseline: &[TableDef],
    table: &str,
    column: &str,
    new_default: Option<&String>,
    source: &str,
    tree: Option<&Tree>,
) -> DriftRecord {
    let before = lookup_baseline_column(baseline, table, column).and_then(|c| {
        c.default
            .as_ref()
            .map(vespertide_core::DefaultValue::to_sql)
    });
    let after = new_default.cloned();
    let before_s = render_default(before.as_deref());
    let after_s = render_default(after.as_deref());
    (
        DriftKind::ModifyColumnDefault {
            column: column.to_string(),
            before,
            after,
        },
        locate_column_field_range(tree, source, column, ErrorField::Default),
        format!("Default drift on '{column}': applied {before_s} → model {after_s}"),
    )
}

fn modify_column_comment_drift(
    baseline: &[TableDef],
    table: &str,
    column: &str,
    new_comment: Option<&String>,
    source: &str,
    tree: Option<&Tree>,
) -> DriftRecord {
    let before = lookup_baseline_column(baseline, table, column).and_then(|c| c.comment.clone());
    let after = new_comment.cloned();
    let before_s = render_comment(before.as_deref());
    let after_s = render_comment(after.as_deref());
    (
        DriftKind::ModifyColumnComment {
            column: column.to_string(),
            before,
            after,
        },
        locate_column_field_range(tree, source, column, ErrorField::Comment),
        format!("Comment drift on '{column}': applied {before_s} → model {after_s}"),
    )
}

fn add_constraint_drift(
    constraint: &TableConstraint,
    source: &str,
    tree: Option<&Tree>,
) -> DriftRecord {
    let name = constraint_name(constraint).map(str::to_string);
    let label = name.as_deref().unwrap_or("<unnamed>");
    (
        DriftKind::AddConstraint { name: name.clone() },
        locate_constraint_range(tree, source, name.as_deref()),
        format!("Constraint added in model: {label}"),
    )
}

fn remove_constraint_drift(
    constraint: &TableConstraint,
    source: &str,
    tree: Option<&Tree>,
) -> DriftRecord {
    let name = constraint_name(constraint).map(str::to_string);
    let label = name.as_deref().unwrap_or("<unnamed>");
    (
        DriftKind::RemoveConstraint { name: name.clone() },
        locate_constraint_range(tree, source, name.as_deref()),
        format!("Constraint in applied migrations missing from model: {label}"),
    )
}

fn replace_constraint_drift(
    from: &TableConstraint,
    to: &TableConstraint,
    source: &str,
    tree: Option<&Tree>,
) -> DriftRecord {
    let name = constraint_name(to).map(str::to_string);
    let from_label = constraint_name(from).unwrap_or("<unnamed>");
    let to_label = name.as_deref().unwrap_or("<unnamed>");
    (
        DriftKind::ReplaceConstraint { name: name.clone() },
        locate_constraint_range(tree, source, name.as_deref()),
        format!("Constraint replaced: {from_label} → {to_label}"),
    )
}

fn locate_table_name(tree: Option<&Tree>, source: &str) -> Option<Range<usize>> {
    tree.map(|tree| locate_top_name(Some(tree), source).unwrap_or(0..1))
}

fn locate_column_range(
    tree: Option<&Tree>,
    source: &str,
    column_name: &str,
) -> Option<Range<usize>> {
    tree.map(|tree| locate_column(Some(tree), source, column_name))
}

fn locate_column_field_range(
    tree: Option<&Tree>,
    source: &str,
    column_name: &str,
    field: ErrorField,
) -> Option<Range<usize>> {
    tree.map(|tree| locate_column_field(Some(tree), source, column_name, field))
}

fn locate_constraint_range(
    tree: Option<&Tree>,
    source: &str,
    name: Option<&str>,
) -> Option<Range<usize>> {
    tree.map(|tree| {
        name.map(|name| locate_constraint(Some(tree), source, name))
            .or_else(|| locate_top_name(Some(tree), source))
            .unwrap_or(0..1)
    })
}

fn constraint_name(constraint: &TableConstraint) -> Option<&str> {
    match constraint {
        TableConstraint::Unique { name, .. }
        | TableConstraint::ForeignKey { name, .. }
        | TableConstraint::Index { name, .. } => name.as_deref(),
        TableConstraint::Check { name, .. } => Some(name.as_str()),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
