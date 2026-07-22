use super::*;

#[rstest]
#[case::postgres(
    DatabaseBackend::Postgres,
    "provider = \"postgresql\"",
    "@db.Timestamptz",
    None
)]
#[case::mysql(
    DatabaseBackend::MySql,
    "provider = \"mysql\"",
    "@db.Timestamp",
    Some("@db.Timestamptz")
)]
#[case::sqlite(
    DatabaseBackend::Sqlite,
    "provider = \"sqlite\"",
    "DateTime",
    Some("@db.")
)]
#[tokio::test]
#[serial]
async fn export_prisma_writes_provider_specific_schema(
    #[case] backend: DatabaseBackend,
    #[case] provider_line: &str,
    #[case] expected: &str,
    #[case] absent: Option<&str>,
) {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    write_config();

    let mut model = sample_table("events");
    model.columns.push(ColumnDef {
        name: "occurred_at".into(),
        r#type: ColumnType::Simple(SimpleColumnType::Timestamptz),
        nullable: false,
        default: None,
        comment: None,
        primary_key: None,
        unique: None,
        index: None,
        foreign_key: None,
    });
    write_model(Path::new("models/events.json"), &model);

    cmd_export(OrmArg::Prisma, None, backend).await.unwrap();

    let out = PathBuf::from("src/models/schema.prisma");
    assert!(out.exists());
    let content = std_fs::read_to_string(out).unwrap();
    assert!(content.contains(provider_line));
    assert!(content.contains(expected));
    if let Some(absent) = absent {
        assert!(!content.contains(absent));
    }
}

#[test]
fn build_output_path_prisma_uses_prisma_extension() {
    use std::path::Path;
    let root = Path::new("src/models");
    let out = build_output_path(root, Path::new("user.json"), Orm::Prisma);
    assert_eq!(out, Path::new("src/models/user.prisma"));
}

#[tokio::test]
#[serial]
async fn clean_export_dir_removes_stale_prisma_files() {
    let tmp = tempdir().unwrap();
    let root = tmp.path().join("out");
    std_fs::create_dir_all(&root).unwrap();
    std_fs::write(root.join("schema.prisma"), "stale").unwrap();
    std_fs::write(root.join("keep.txt"), "keep").unwrap();

    clean_export_dir(&root, Orm::Prisma).await.unwrap();

    assert!(!root.join("schema.prisma").exists());
    assert!(root.join("keep.txt").exists());
}
