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
    models_dir: Option<PathBuf>,
    by_name: BTreeMap<String, TableDef>,
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
        let Ok(tables) = vespertide_loader::load_models_from_dir(Some(root.clone())) else {
            return false;
        };

        let by_name: BTreeMap<String, TableDef> = tables
            .into_iter()
            .filter_map(|table| table.normalize().ok())
            .map(|table| (table.name.clone(), table))
            .collect();
        let count = by_name.len();
        let models_dir = root.join(config.models_dir());

        *self.inner.write().unwrap() = Inner {
            root: Some(root),
            models_dir: Some(models_dir),
            by_name,
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

    pub fn model_path(&self, table_name: &str) -> Option<PathBuf> {
        let inner = self.inner.read().unwrap();
        let models_dir = inner.models_dir.as_ref()?;
        ["json", "yaml", "yml"]
            .into_iter()
            .map(|extension| models_dir.join(format!("{table_name}.{extension}")))
            .find(|path| path.exists())
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
}
