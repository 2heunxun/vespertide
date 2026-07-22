use super::*;
pub(super) use crate::test_support::CwdGuard;
pub(super) use rstest::rstest;
pub(super) use serial_test::serial;
pub(super) use std::fs as std_fs;
pub(super) use tempfile::tempdir;
pub(super) use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType, TableConstraint};

mod prisma;

fn write_config() {
    let cfg = VespertideConfig::default();
    let text = serde_json::to_string_pretty(&cfg).unwrap();
    std_fs::write("vespertide.json", text).unwrap();
}

fn write_model(path: &Path, table: &TableDef) {
    if let Some(parent) = path.parent() {
        std_fs::create_dir_all(parent).unwrap();
    }
    std_fs::write(path, serde_json::to_string_pretty(table).unwrap()).unwrap();
}

fn sample_table(name: &str) -> TableDef {
    TableDef {
        name: name.into(),
        description: None,
        columns: vec![ColumnDef {
            name: "id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Integer),
            nullable: false,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    }
}

#[tokio::test]
#[serial]
async fn export_writes_seaorm_files_to_default_dir() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    write_config();

    let model = sample_table("users");
    write_model(Path::new("models/users.json"), &model);

    cmd_export(OrmArg::Seaorm, None, DatabaseBackend::Postgres)
        .await
        .unwrap();

    let out = PathBuf::from("src/models/users.rs");
    assert!(out.exists());
    let content = std_fs::read_to_string(out).unwrap();
    assert!(content.contains("#[sea_orm(table_name = \"users\")]"));

    // mod.rs wiring at root
    let root_mod = PathBuf::from("src/models/mod.rs");
    assert!(root_mod.exists());
    let root_mod_content = std_fs::read_to_string(root_mod).unwrap();
    assert!(root_mod_content.contains("pub mod users;"));
}

#[tokio::test]
#[serial]
async fn export_respects_custom_output_dir() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    write_config();

    let model = sample_table("posts");
    write_model(Path::new("models/blog/posts.json"), &model);

    let custom = PathBuf::from("out_dir");
    cmd_export(
        OrmArg::Seaorm,
        Some(custom.clone()),
        DatabaseBackend::Postgres,
    )
    .await
    .unwrap();

    let out = custom.join("blog/posts.rs");
    assert!(out.exists());
    let content = std_fs::read_to_string(out).unwrap();
    assert!(content.contains("#[sea_orm(table_name = \"posts\")]"));

    // nested mod.rs wiring
    let root_mod = custom.join("mod.rs");
    let blog_mod = custom.join("blog/mod.rs");
    assert!(root_mod.exists());
    assert!(blog_mod.exists());
    let root_mod_content = std_fs::read_to_string(root_mod).unwrap();
    let blog_mod_content = std_fs::read_to_string(blog_mod).unwrap();
    assert!(root_mod_content.contains("pub mod blog;"));
    assert!(blog_mod_content.contains("pub mod posts;"));
}

#[tokio::test]
#[serial]
async fn export_with_sqlalchemy_sets_py_extension() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    write_config();

    let model = sample_table("items");
    write_model(Path::new("models/items.json"), &model);

    cmd_export(OrmArg::Sqlalchemy, None, DatabaseBackend::Postgres)
        .await
        .unwrap();

    let out = PathBuf::from("src/models/items.py");
    assert!(out.exists());
    let content = std_fs::read_to_string(out).unwrap();
    assert!(content.contains("items"));
}

#[tokio::test]
#[serial]
async fn export_with_sqlmodel_sets_py_extension() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    write_config();

    let model = sample_table("orders");
    write_model(Path::new("models/orders.json"), &model);

    cmd_export(OrmArg::Sqlmodel, None, DatabaseBackend::Postgres)
        .await
        .unwrap();

    let out = PathBuf::from("src/models/orders.py");
    assert!(out.exists());
    let content = std_fs::read_to_string(out).unwrap();
    assert!(content.contains("orders"));
}

#[tokio::test]
#[serial]
async fn load_models_recursive_returns_empty_when_absent() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let models = load_models_recursive(Path::new("no_models")).await.unwrap();
    assert!(models.is_empty());
}

#[tokio::test]
#[serial]
async fn load_models_recursive_ignores_non_model_files() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    write_config();

    std_fs::create_dir_all("models").unwrap();
    std_fs::write("models/ignore.txt", "hello").unwrap();
    write_model(Path::new("models/valid.json"), &sample_table("valid"));

    let models = load_models_recursive(Path::new("models")).await.unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].0.name, "valid");
}

#[tokio::test]
#[serial]
async fn load_models_recursive_parses_yaml_branch() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    write_config();

    std_fs::create_dir_all("models").unwrap();
    let table = sample_table("yaml_table");
    let yaml = serde_yaml::to_string(&table).unwrap();
    std_fs::write("models/yaml_table.yaml", yaml).unwrap();

    let models = load_models_recursive(Path::new("models")).await.unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].0.name, "yaml_table");
}

#[tokio::test]
#[serial]
async fn ensure_mod_chain_adds_to_existing_file_without_trailing_newline() {
    let tmp = tempdir().unwrap();
    let root = tmp.path().join("src/models");
    std_fs::create_dir_all(&root).unwrap();
    std_fs::write(root.join("mod.rs"), "pub mod existing;").unwrap();

    ensure_mod_chain(&root, Path::new("blog/posts.rs"))
        .await
        .unwrap();

    let root_mod = std_fs::read_to_string(root.join("mod.rs")).unwrap();
    let blog_mod = std_fs::read_to_string(root.join("blog/mod.rs")).unwrap();
    assert!(root_mod.contains("pub mod existing;"));
    assert!(root_mod.contains("pub mod blog;"));
    assert!(blog_mod.contains("pub mod posts;"));
    // ensure newline appended if missing
    assert!(root_mod.ends_with('\n'));
}

#[tokio::test]
async fn ensure_mod_chain_no_components_is_noop() {
    let tmp = tempdir().unwrap();
    let root = tmp.path().join("src/models");
    std_fs::create_dir_all(&root).unwrap();
    // empty path should not error
    assert!(ensure_mod_chain(&root, Path::new("")).await.is_ok());
}

#[test]
#[serial]
fn resolve_export_dir_prefers_override() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    write_config();
    let cfg = VespertideConfig::default();
    let override_dir = PathBuf::from("custom_out");
    let resolved = super::resolve_export_dir(Some(override_dir.clone()), &cfg);
    assert_eq!(resolved, override_dir);
}

#[rstest]
#[case(OrmArg::Seaorm, Orm::SeaOrm)]
#[case(OrmArg::Sqlalchemy, Orm::SqlAlchemy)]
#[case(OrmArg::Sqlmodel, Orm::SqlModel)]
#[case(OrmArg::Jpa, Orm::Jpa)]
#[case(OrmArg::Prisma, Orm::Prisma)]
fn orm_arg_maps_to_enum(#[case] arg: OrmArg, #[case] expected: Orm) {
    assert_eq!(Orm::from(arg), expected);
}

#[rstest]
#[case("normal_name", "normal_name")]
#[case("user copy", "user_copy")]
#[case("user  copy", "user__copy")]
#[case("user-copy", "user-copy")]
#[case("user.copy", "user_copy")]
#[case("user copy.json", "user_copy_json")]
fn test_sanitize_filename(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(sanitize_filename(input), expected);
}

#[test]
fn build_output_path_sanitizes_spaces() {
    use std::path::Path;
    let root = Path::new("src/models");
    let rel_path = Path::new("user copy.json");
    let out = build_output_path(root, rel_path, Orm::SeaOrm);
    assert_eq!(out, Path::new("src/models/user_copy.rs"));

    let rel_path2 = Path::new("blog/post name.yaml");
    let out2 = build_output_path(root, rel_path2, Orm::SeaOrm);
    assert_eq!(out2, Path::new("src/models/blog/post_name.rs"));
}

#[test]
fn build_output_path_handles_file_without_extension() {
    use std::path::Path;
    let root = Path::new("src/models");
    // File without extension - covers line 88 (else branch)
    let rel_path = Path::new("users");
    let out = build_output_path(root, rel_path, Orm::SeaOrm);
    assert_eq!(out, Path::new("src/models/users.rs"));

    let out_py = build_output_path(root, rel_path, Orm::SqlAlchemy);
    assert_eq!(out_py, Path::new("src/models/users.py"));
}

#[test]
fn build_output_path_handles_special_path_components() {
    use std::path::Path;
    let root = Path::new("src/models");
    // Path with CurDir component (.) - covers line 78 (non-Normal component branch)
    let rel_path = Path::new("./blog/posts.json");
    let out = build_output_path(root, rel_path, Orm::SeaOrm);
    // The . component gets pushed via the else branch
    assert!(out.to_string_lossy().contains("posts"));

    // Path with ParentDir component (..)
    let rel_path2 = Path::new("../other/items.yaml");
    let out2 = build_output_path(root, rel_path2, Orm::SeaOrm);
    assert!(out2.to_string_lossy().contains("items"));
}

#[test]
fn build_output_path_strips_vespertide_suffix() {
    use std::path::Path;
    let root = Path::new("src/models");

    // .vespertide.json -> .rs (strips ".vespertide" from stem)
    let rel_path = Path::new("user.vespertide.json");
    let out = build_output_path(root, rel_path, Orm::SeaOrm);
    assert_eq!(out, Path::new("src/models/user.rs"));

    // Nested path with .vespertide.json
    let rel_path2 = Path::new("blog/post.vespertide.json");
    let out2 = build_output_path(root, rel_path2, Orm::SeaOrm);
    assert_eq!(out2, Path::new("src/models/blog/post.rs"));

    // .vespertide.yaml -> .py
    let rel_path3 = Path::new("order.vespertide.yaml");
    let out3 = build_output_path(root, rel_path3, Orm::SqlAlchemy);
    assert_eq!(out3, Path::new("src/models/order.py"));

    // Regular .json without .vespertide suffix still works
    let rel_path4 = Path::new("item.json");
    let out4 = build_output_path(root, rel_path4, Orm::SeaOrm);
    assert_eq!(out4, Path::new("src/models/item.rs"));
}

#[tokio::test]
#[serial]
async fn ensure_mod_chain_strips_vespertide_suffix() {
    let tmp = tempdir().unwrap();
    let root = tmp.path().join("src/models");
    std_fs::create_dir_all(&root).unwrap();

    // File with .vespertide suffix should produce mod declaration without it
    ensure_mod_chain(&root, Path::new("user.vespertide.json"))
        .await
        .unwrap();

    let root_mod = std_fs::read_to_string(root.join("mod.rs")).unwrap();
    // Should be "pub mod user;" not "pub mod user_vespertide;"
    assert!(root_mod.contains("pub mod user;"));
    assert!(!root_mod.contains("user_vespertide"));

    // Nested path with .vespertide suffix
    ensure_mod_chain(&root, Path::new("blog/post.vespertide.json"))
        .await
        .unwrap();
    let root_mod = std_fs::read_to_string(root.join("mod.rs")).unwrap();
    let blog_mod = std_fs::read_to_string(root.join("blog/mod.rs")).unwrap();
    assert!(root_mod.contains("pub mod blog;"));
    assert!(blog_mod.contains("pub mod post;"));
    assert!(!root_mod.contains("post_vespertide"));
    assert!(!blog_mod.contains("post_vespertide"));
}

#[tokio::test]
async fn clean_export_dir_removes_rs_files_for_seaorm() {
    let tmp = tempdir().unwrap();
    let root = tmp.path().join("export_dir");
    std_fs::create_dir_all(&root).unwrap();

    // Create some .rs files that should be cleaned
    std_fs::write(root.join("old_model.rs"), "// old rust file").unwrap();
    std_fs::write(root.join("another.rs"), "// another rust file").unwrap();
    // Create a non-.rs file that should NOT be cleaned
    std_fs::write(root.join("keep.txt"), "keep this").unwrap();

    clean_export_dir(&root, Orm::SeaOrm).await.unwrap();

    // .rs files should be gone
    assert!(!root.join("old_model.rs").exists());
    assert!(!root.join("another.rs").exists());
    // .txt file should remain
    assert!(root.join("keep.txt").exists());
}

#[tokio::test]
async fn clean_export_dir_removes_py_files_for_sqlalchemy() {
    let tmp = tempdir().unwrap();
    let root = tmp.path().join("export_dir");
    std_fs::create_dir_all(&root).unwrap();

    // Create some .py files that should be cleaned
    std_fs::write(root.join("old_model.py"), "# old python file").unwrap();
    // Create a .rs file that should NOT be cleaned
    std_fs::write(root.join("keep.rs"), "// keep this").unwrap();

    clean_export_dir(&root, Orm::SqlAlchemy).await.unwrap();

    // .py files should be gone
    assert!(!root.join("old_model.py").exists());
    // .rs file should remain
    assert!(root.join("keep.rs").exists());
}

#[tokio::test]
async fn clean_export_dir_removes_py_files_for_sqlmodel() {
    let tmp = tempdir().unwrap();
    let root = tmp.path().join("export_dir");
    std_fs::create_dir_all(&root).unwrap();

    std_fs::write(root.join("model.py"), "# python file").unwrap();

    clean_export_dir(&root, Orm::SqlModel).await.unwrap();

    assert!(!root.join("model.py").exists());
}

#[tokio::test]
async fn clean_export_dir_removes_java_files_for_jpa() {
    let tmp = tempdir().unwrap();
    let root = tmp.path().join("export_dir");
    std_fs::create_dir_all(&root).unwrap();

    std_fs::write(root.join("User.java"), "// java entity").unwrap();
    std_fs::write(root.join("Order.java"), "// java entity").unwrap();
    std_fs::write(root.join("keep.rs"), "// keep this").unwrap();

    clean_export_dir(&root, Orm::Jpa).await.unwrap();

    assert!(!root.join("User.java").exists());
    assert!(!root.join("Order.java").exists());
    assert!(root.join("keep.rs").exists());
}

#[tokio::test]
async fn clean_export_dir_handles_missing_directory() {
    let tmp = tempdir().unwrap();
    let root = tmp.path().join("nonexistent_dir");

    // Should not error on missing directory
    let result = clean_export_dir(&root, Orm::SeaOrm).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn clean_dir_recursive_cleans_subdirectories() {
    let tmp = tempdir().unwrap();
    let root = tmp.path().join("export_dir");
    let subdir = root.join("nested");
    std_fs::create_dir_all(&subdir).unwrap();

    // Create files in root and subdirectory
    std_fs::write(root.join("root.rs"), "// root").unwrap();
    std_fs::write(subdir.join("nested.rs"), "// nested").unwrap();
    std_fs::write(subdir.join("keep.txt"), "keep").unwrap();

    clean_dir_recursive(&root, "rs").await.unwrap();

    // .rs files should be gone
    assert!(!root.join("root.rs").exists());
    assert!(!subdir.join("nested.rs").exists());
    // .txt file should remain
    assert!(subdir.join("keep.txt").exists());
    // subdir should still exist (has .txt file)
    assert!(subdir.exists());
}

#[tokio::test]
async fn clean_dir_recursive_removes_empty_subdirectories() {
    let tmp = tempdir().unwrap();
    let root = tmp.path().join("export_dir");
    let subdir = root.join("empty_after_clean");
    std_fs::create_dir_all(&subdir).unwrap();

    // Create only .rs files in subdirectory
    std_fs::write(subdir.join("only.rs"), "// only").unwrap();

    clean_dir_recursive(&root, "rs").await.unwrap();

    // .rs file should be gone
    assert!(!subdir.join("only.rs").exists());
    // Empty subdirectory should be removed
    assert!(!subdir.exists());
}

#[tokio::test]
async fn clean_dir_recursive_handles_non_directory() {
    let tmp = tempdir().unwrap();
    let file_path = tmp.path().join("not_a_dir.txt");
    std_fs::write(&file_path, "content").unwrap();

    // Should not error when called on a file instead of directory
    let result = clean_dir_recursive(&file_path, "rs").await;
    assert!(result.is_ok());
}

#[test]
fn build_output_path_jpa_uses_pascal_case_java_extension() {
    use std::path::Path;
    let root = Path::new("src/models");

    // snake_case model → PascalCase .java
    let rel_path = Path::new("order_item.json");
    let out = build_output_path(root, rel_path, Orm::Jpa);
    assert_eq!(out, Path::new("src/models/OrderItem.java"));

    // Single word
    let rel_path2 = Path::new("users.json");
    let out2 = build_output_path(root, rel_path2, Orm::Jpa);
    assert_eq!(out2, Path::new("src/models/Users.java"));

    // Nested path
    let rel_path3 = Path::new("blog/post_comment.yaml");
    let out3 = build_output_path(root, rel_path3, Orm::Jpa);
    assert_eq!(out3, Path::new("src/models/blog/PostComment.java"));
}

#[test]
fn build_output_path_jpa_strips_vespertide_suffix() {
    use std::path::Path;
    let root = Path::new("src/models");

    let rel_path = Path::new("user.vespertide.json");
    let out = build_output_path(root, rel_path, Orm::Jpa);
    assert_eq!(out, Path::new("src/models/User.java"));
}

#[rstest]
#[case("order_item", "OrderItem")]
#[case("users", "Users")]
#[case("a", "A")]
#[case("user_profile_image", "UserProfileImage")]
#[case("a__b", "AB")]
fn test_to_pascal_case(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(to_pascal_case(input), expected);
}
