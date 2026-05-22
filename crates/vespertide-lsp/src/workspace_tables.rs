//! Disk-discovered workspace tables.
//!
//! The LSP keeps open documents in [`DocumentStore`](crate::DocumentStore), but
//! cross-file features also need models that have not been opened by the editor.
//! This cache is populated from `vespertide.json` + `vespertide-loader` on
//! initialize and refreshed after document changes. [`BTreeMap`] keeps
//! iteration deterministic across the workspace.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use vespertide_core::TableDef;

#[derive(Debug, Default)]
pub struct WorkspaceTables {
    inner: RwLock<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    root: Option<PathBuf>,
    by_name: BTreeMap<String, TableDef>,
    /// `table_name → absolute file path` recorded during refresh. Needed
    /// because filenames don't always match the declared `name`
    /// (`media.vespertide.json` declares `name: media`, but
    /// `models/my_table.json` is also valid and declares `name: user`).
    path_by_name: BTreeMap<String, PathBuf>,
}

impl WorkspaceTables {
    pub fn new() -> Self {
        Self::default()
    }

    /// Walk up from `start` looking for `vespertide.json`, then load all models.
    ///
    /// Returns `true` only when a config was found and at least one table loaded.
    pub fn refresh(&self, start: &Path) -> bool {
        let Some(root) = find_workspace_root(start) else {
            *self.inner.write().unwrap() = Inner::default();
            return false;
        };

        let Ok(config) = vespertide_loader::load_config_from_path(root.join("vespertide.json"))
        else {
            return false;
        };
        let models_dir = root.join(config.models_dir());

        // Walk the models directory ourselves so we capture
        // (table_name, file_path) — vespertide-loader's public API only
        // returns the parsed tables and drops the filename.
        let mut by_name: BTreeMap<String, TableDef> = BTreeMap::new();
        let mut path_by_name: BTreeMap<String, PathBuf> = BTreeMap::new();
        collect_models(&models_dir, &mut by_name, &mut path_by_name);
        let count = by_name.len();

        *self.inner.write().unwrap() = Inner {
            root: Some(root),
            by_name,
            path_by_name,
        };

        count > 0
    }

    pub fn get(&self, name: &str) -> Option<TableDef> {
        self.inner.read().unwrap().by_name.get(name).cloned()
    }

    pub fn names(&self) -> Vec<String> {
        self.inner.read().unwrap().by_name.keys().cloned().collect()
    }

    pub fn all(&self) -> Vec<(String, TableDef)> {
        self.inner
            .read()
            .unwrap()
            .by_name
            .iter()
            .map(|(name, table)| (name.clone(), table.clone()))
            .collect()
    }

    pub fn root(&self) -> Option<PathBuf> {
        self.inner.read().unwrap().root.clone()
    }

    /// Look up the on-disk file path that declared `table_name`. Cached at
    /// `refresh()` time so the lookup is filename-agnostic — works for
    /// `media.json`, `media.vespertide.json`, or `models/whatever.json`
    /// regardless of the filename convention.
    pub fn model_path(&self, table_name: &str) -> Option<PathBuf> {
        self.inner
            .read()
            .unwrap()
            .path_by_name
            .get(table_name)
            .cloned()
    }
}

/// Recursively walk `dir` collecting every `.json` / `.yaml` / `.yml`
/// model file. For each file we parse + normalize the `TableDef` and
/// record `(name → table)` alongside `(name → path)`.
///
/// On parse / normalize failure we silently skip the file: the diagnostics
/// engine will surface a parse error for any opened model, and silently
/// skipping disk-only invalid files keeps the workspace cache from
/// blocking on a single corrupted model.
fn collect_models(
    dir: &Path,
    by_name: &mut BTreeMap<String, TableDef>,
    path_by_name: &mut BTreeMap<String, PathBuf>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_models(&path, by_name, path_by_name);
            continue;
        }
        if !is_model_file(&path) {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(table) = parse_table(&path, &content) else {
            continue;
        };
        let Ok(normalized) = table.normalize() else {
            continue;
        };
        let name = normalized.name.clone();
        by_name.insert(name.clone(), normalized);
        path_by_name.insert(name, path);
    }
}

fn is_model_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("json" | "yaml" | "yml")
    )
}

fn parse_table(path: &Path, content: &str) -> Option<TableDef> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("json") => serde_json::from_str(content).ok(),
        Some("yaml" | "yml") => serde_yaml::from_str(content).ok(),
        _ => None,
    }
}

fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    let mut current = if start.is_file() {
        start.parent()
    } else {
        Some(start)
    };

    while let Some(dir) = current {
        if dir.join("vespertide.json").exists() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }

    None
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn no_config_refresh_returns_false() {
        let tmp = tempdir().unwrap();
        let tables = WorkspaceTables::new();

        assert!(!tables.refresh(tmp.path()));
        assert!(tables.names().is_empty());
    }

    #[test]
    fn loads_models_when_config_present() {
        let tmp = tempdir().unwrap();
        let models_dir = tmp.path().join("models");
        fs::create_dir_all(&models_dir).unwrap();
        fs::write(
            tmp.path().join("vespertide.json"),
            r#"{"modelsDir":"models","migrationsDir":"migrations","tableNamingCase":"snake","columnNamingCase":"snake","modelFormat":"json"}"#,
        )
        .unwrap();
        fs::write(
            models_dir.join("user.json"),
            r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#,
        )
        .unwrap();

        let tables = WorkspaceTables::new();

        assert!(tables.refresh(tmp.path()));
        assert!(tables.names().contains(&"user".to_string()));
        assert!(tables.get("user").is_some());
        assert_eq!(tables.root().as_deref(), Some(tmp.path()));
        assert_eq!(
            tables.model_path("user"),
            Some(models_dir.join("user.json"))
        );
    }

    /// Regression — `media.vespertide.json` declares `name: media`. The
    /// old `model_path` only tried `media.json`, missed the double
    /// extension, and made `collect_workspace_tables` drop the model. The
    /// planner then reported `foreign key references non-existent table`
    /// even though hover (which uses `get(name)` directly) showed
    /// `Target table: media (on disk)`.
    #[test]
    fn model_path_resolves_double_extension_filenames() {
        let tmp = tempdir().unwrap();
        let models_dir = tmp.path().join("models");
        fs::create_dir_all(&models_dir).unwrap();
        fs::write(
            tmp.path().join("vespertide.json"),
            r#"{"modelsDir":"models","migrationsDir":"migrations","tableNamingCase":"snake","columnNamingCase":"snake","modelFormat":"json"}"#,
        )
        .unwrap();
        fs::write(
            models_dir.join("media.vespertide.json"),
            r#"{"name":"media","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#,
        )
        .unwrap();

        let tables = WorkspaceTables::new();
        assert!(tables.refresh(tmp.path()));
        assert!(tables.names().contains(&"media".to_string()));
        assert_eq!(
            tables.model_path("media"),
            Some(models_dir.join("media.vespertide.json")),
            "double-extension files must be discoverable by their `name`"
        );
    }

    #[test]
    fn model_path_resolves_when_filename_disagrees_with_name() {
        let tmp = tempdir().unwrap();
        let models_dir = tmp.path().join("models");
        fs::create_dir_all(&models_dir).unwrap();
        fs::write(
            tmp.path().join("vespertide.json"),
            r#"{"modelsDir":"models","migrationsDir":"migrations","tableNamingCase":"snake","columnNamingCase":"snake","modelFormat":"json"}"#,
        )
        .unwrap();
        // Filename `something_weird.json` but the declared name is `user`.
        fs::write(
            models_dir.join("something_weird.json"),
            r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#,
        )
        .unwrap();

        let tables = WorkspaceTables::new();
        assert!(tables.refresh(tmp.path()));
        assert_eq!(
            tables.model_path("user"),
            Some(models_dir.join("something_weird.json")),
            "model_path must follow the declared `name`, not the filename"
        );
    }
}
