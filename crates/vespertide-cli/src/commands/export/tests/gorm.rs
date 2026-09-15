use super::*;
use insta::assert_snapshot;

/// Go requires the `package` clause to match the directory the files live in,
/// so the effective package name comes from the real write target rather than
/// the config's static default. Exporting into a non-default directory is what
/// tells the two apart.
#[tokio::test]
#[serial]
async fn export_gorm_takes_its_package_name_from_the_export_directory() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    write_config();
    write_model(Path::new("models/widgets.json"), &sample_table("widgets"));

    cmd_export(Orm::Gorm, Some(PathBuf::from("generated/store")))
        .await
        .unwrap();

    let written = std_fs::read_to_string(PathBuf::from("generated/store/widgets.go")).unwrap();
    assert_snapshot!(written);
}

#[test]
fn build_output_path_gorm_go_extension() {
    let root = Path::new("src/models");
    let out = build_output_path(root, Path::new("user.json"), Orm::Gorm);
    assert_eq!(out, Path::new("src/models/user.go"));
}

#[tokio::test]
async fn clean_export_dir_removes_go_files_for_gorm() {
    let tmp = tempdir().unwrap();
    let root = tmp.path().join("export_dir");
    std_fs::create_dir_all(&root).unwrap();

    std_fs::write(root.join("model.go"), "// go file").unwrap();
    std_fs::write(root.join("keep.rs"), "// keep this").unwrap();

    clean_export_dir(&root, Orm::Gorm).await.unwrap();

    assert!(!root.join("model.go").exists());
    assert!(root.join("keep.rs").exists());
}
