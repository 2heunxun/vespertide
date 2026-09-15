use super::*;
use insta::assert_snapshot;

/// `app_label` is the one `django` config setting, and it only reaches the
/// generated `Meta` class through `DjangoExporterWithConfig`. Exporting with it
/// set is what proves the CLI takes that path.
#[tokio::test]
#[serial]
async fn export_django_writes_the_configured_app_label_into_meta() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let mut cfg = serde_json::to_value(VespertideConfig::default()).unwrap();
    cfg["django"] = serde_json::json!({ "appLabel": "storefront" });
    std_fs::write(
        "vespertide.json",
        serde_json::to_string_pretty(&cfg).unwrap(),
    )
    .unwrap();
    write_model(Path::new("models/gadgets.json"), &sample_table("gadgets"));

    cmd_export(Orm::Django, None).await.unwrap();

    let written = std_fs::read_to_string(PathBuf::from("src/models/gadgets.py")).unwrap();
    assert_snapshot!(written);
}

#[tokio::test]
async fn clean_export_dir_removes_py_files_for_django() {
    let tmp = tempdir().unwrap();
    let root = tmp.path().join("export_dir");
    std_fs::create_dir_all(&root).unwrap();

    std_fs::write(root.join("old_model.py"), "# python file").unwrap();
    std_fs::write(root.join("keep.rs"), "// keep this").unwrap();

    clean_export_dir(&root, Orm::Django).await.unwrap();

    assert!(!root.join("old_model.py").exists());
    assert!(root.join("keep.rs").exists());
}
