//! Drift detection integration tests.

use tempfile::tempdir;
use vespertide_lsp::{DocumentStore, WorkspaceIndex, compute_drift};

#[test]
fn no_config_no_drift() {
    let tmp = tempdir().unwrap();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();
    let drifts = compute_drift(tmp.path(), &idx, &docs);

    assert!(drifts.is_empty());
}
