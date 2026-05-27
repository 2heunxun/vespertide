use super::*;

/// Integration test: FK column nullable→not-null triggers `handle_delete_null_rows` (line 489)
#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_handles_delete_null_rows_for_fk_column() {
    use vespertide_core::MigrationPlan;
    use vespertide_core::schema::foreign_key::{ForeignKeyDef, ForeignKeySyntax};

    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());

    let cfg = write_config();
    std_fs::create_dir_all(cfg.migrations_dir()).unwrap();

    // Write v1 migration: create "orders" table with nullable user_id
    let v1 = MigrationPlan {
        id: "v1-id".to_string(),
        comment: Some("init".to_string()),
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::CreateTable {
            table: "orders".into(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: true, // nullable in v1
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![
                TableConstraint::PrimaryKey {
                    auto_increment: false,
                    columns: vec!["id".into()],
                },
                TableConstraint::ForeignKey {
                    name: Some("fk_orders__user_id".into()),
                    columns: vec!["user_id".into()],
                    ref_table: "users".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                },
            ],
        }],
    };
    let v1_path = cfg.migrations_dir().join("0001_init.vespertide.json");
    std_fs::write(&v1_path, serde_json::to_string_pretty(&v1).unwrap()).unwrap();

    // Write updated model: user_id is now NOT NULL
    let models_dir = PathBuf::from("models");
    std_fs::create_dir_all(&models_dir).unwrap();
    let users_model = TableDef {
        name: "users".into(),
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
        }],
    };
    std_fs::write(
        models_dir.join("users.json"),
        serde_json::to_string_pretty(&users_model).unwrap(),
    )
    .unwrap();

    let model = TableDef {
        name: "orders".into(),
        description: None,
        columns: vec![
            ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "user_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false, // NOT NULL now
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: Some(ForeignKeySyntax::Object(ForeignKeyDef {
                    ref_table: "users".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                })),
            },
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
        }],
    };
    std_fs::write(
        models_dir.join("orders.json"),
        serde_json::to_string_pretty(&model).unwrap(),
    )
    .unwrap();

    // Mock prompts
    let recreate_prompt = |_: &[RecreateTableRequired]| -> Result<bool> { Ok(true) };
    let delete_prompt = |_table: &str, _col: &str| -> Result<bool> { Ok(true) };
    let fill_prompt = |_p: &str, _d: &str| -> Result<String> {
        panic!("fill prompt should not be called — FK handled by delete_null_rows");
    };
    let enum_prompt = |_p: &str, _v: &[String]| -> Result<String> {
        panic!("enum prompt should not be called");
    };
    let enum_bare_prompt = |_p: &str, _v: &[String]| -> Result<String> {
        panic!("enum bare prompt should not be called");
    };

    let result = cmd_revision_core(
        "make user_id required".into(),
        vec![],
        vec![],
        RevisionPromptFns {
            recreate: recreate_prompt,
            delete_null_rows: delete_prompt,
            fill_with: fill_prompt,
            enum_quoted: enum_prompt,
            enum_bare: enum_bare_prompt,
            // F30 / FK policy change is irrelevant to these scenarios:
            // assert via panic so any unexpected detection breaks the test.
            fk_policy_change: |_: &[vespertide_planner::FkPolicyChangeWarning]| -> Result<bool> {
                panic!("fk_policy_change prompt should not be called")
            },
            // F6 / type narrowing is irrelevant to these scenarios: assert
            // via panic so any unexpected detection breaks the test.
            type_narrowing: |_: &[vespertide_planner::TypeNarrowingWarning]|
                -> Result<Option<Vec<vespertide_core::NarrowingStrategy>>> {
                panic!("type_narrowing prompt should not be called")
            },
            // F20 / timezone conversion likewise must not fire here.
            timezone_conversion: |_: &[vespertide_planner::TimezoneConversionWarning]|
                -> Result<Option<Vec<String>>> {
                panic!("timezone_conversion prompt should not be called")
            },
            // F7-(b) / RemapEnumValues likewise: integer enum value drift
            // is not in scope for these scenarios. Auto-approve so the
            // existing flow proceeds unchanged when no remap action exists.
            remap_enum_values: |_: &vespertide_core::MigrationPlan| -> Result<bool> {
                Ok(true)
            },
            // F10/F8/F22 drop resolution: these scenarios add columns only,
            // so no DeleteColumn / DeleteTable actions exist and the prompt
            // should never fire. Panic guards against silent flow drift.
            drop_resolution: |_: &vespertide_planner::DropResolution| -> Result<
                Option<vespertide_planner::DropChoice>,
            > {
                panic!("drop_resolution prompt should not be called")
            },
            // F15 default-change resolution: these scenarios touch new
            // columns only, never `ModifyColumnDefault`, so the prompt
            // should never fire. Panic guards against silent flow drift.
            default_change: |_: &vespertide_planner::DefaultChangeWarning| -> Result<
                Option<crate::commands::revision::prompts::DefaultChoice>,
            > {
                panic!("default_change prompt should not be called")
            },
            // F2 unique-addition resolution: these scenarios add columns or
            // create tables only, never `AddConstraint(Unique)` on an
            // existing column, so the prompt should never fire. Panic
            // guards against silent flow drift.
            unique_addition: |_: &vespertide_planner::UniqueAdditionWarning| -> Result<
                Option<crate::commands::revision::prompts::UniqueAdditionChoice>,
            > {
                panic!("unique_addition prompt should not be called")
            },
        },
    )
    .await;

    assert!(
        result.is_ok(),
        "cmd_revision_core failed: {:?}",
        result.err()
    );

    // Verify migration was created
    let entries: Vec<_> = std_fs::read_dir(cfg.migrations_dir())
        .unwrap()
        .filter_map(std::result::Result::ok)
        .collect();
    // Should have 2 files: v1 + new v2
    assert_eq!(entries.len(), 2);
}

/// Integration test: non-FK column nullable→not-null triggers `collect_fill_with_values` (lines 494-495)
#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_handles_fill_with_for_non_fk_column() {
    use vespertide_core::MigrationPlan;

    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());

    let cfg = write_config();
    std_fs::create_dir_all(cfg.migrations_dir()).unwrap();

    // Write v1 migration: create "users" table with nullable email
    let v1 = MigrationPlan {
        id: "v1-id".to_string(),
        comment: Some("init".to_string()),
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: true, // nullable in v1
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            }],
        }],
    };
    let v1_path = cfg.migrations_dir().join("0001_init.vespertide.json");
    std_fs::write(&v1_path, serde_json::to_string_pretty(&v1).unwrap()).unwrap();

    // Write updated model: email is now NOT NULL (no default)
    let models_dir = PathBuf::from("models");
    std_fs::create_dir_all(&models_dir).unwrap();
    let model = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "email".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: false, // NOT NULL now
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
        }],
    };
    std_fs::write(
        models_dir.join("users.json"),
        serde_json::to_string_pretty(&model).unwrap(),
    )
    .unwrap();

    // Mock prompts
    let recreate_prompt = |_: &[RecreateTableRequired]| -> Result<bool> { Ok(true) };
    let delete_prompt = |_table: &str, _col: &str| -> Result<bool> { Ok(false) };
    let fill_prompt = |_p: &str, _d: &str| -> Result<String> { Ok("'unknown'".to_string()) };
    let enum_prompt = |_p: &str, _v: &[String]| -> Result<String> {
        panic!("enum prompt should not be called");
    };
    let enum_bare_prompt = |_p: &str, _v: &[String]| -> Result<String> {
        panic!("enum bare prompt should not be called");
    };

    let result = cmd_revision_core(
        "make email required".into(),
        vec![],
        vec![],
        RevisionPromptFns {
            recreate: recreate_prompt,
            delete_null_rows: delete_prompt,
            fill_with: fill_prompt,
            enum_quoted: enum_prompt,
            enum_bare: enum_bare_prompt,
            // F30 / FK policy change is irrelevant to these scenarios:
            // assert via panic so any unexpected detection breaks the test.
            fk_policy_change: |_: &[vespertide_planner::FkPolicyChangeWarning]| -> Result<bool> {
                panic!("fk_policy_change prompt should not be called")
            },
            // F6 / type narrowing is irrelevant to these scenarios: assert
            // via panic so any unexpected detection breaks the test.
            type_narrowing: |_: &[vespertide_planner::TypeNarrowingWarning]|
                -> Result<Option<Vec<vespertide_core::NarrowingStrategy>>> {
                panic!("type_narrowing prompt should not be called")
            },
            // F20 / timezone conversion likewise must not fire here.
            timezone_conversion: |_: &[vespertide_planner::TimezoneConversionWarning]|
                -> Result<Option<Vec<String>>> {
                panic!("timezone_conversion prompt should not be called")
            },
            // F7-(b) / RemapEnumValues likewise: integer enum value drift
            // is not in scope for these scenarios. Auto-approve so the
            // existing flow proceeds unchanged when no remap action exists.
            remap_enum_values: |_: &vespertide_core::MigrationPlan| -> Result<bool> {
                Ok(true)
            },
            // F10/F8/F22 drop resolution: these scenarios add columns only,
            // so no DeleteColumn / DeleteTable actions exist and the prompt
            // should never fire. Panic guards against silent flow drift.
            drop_resolution: |_: &vespertide_planner::DropResolution| -> Result<
                Option<vespertide_planner::DropChoice>,
            > {
                panic!("drop_resolution prompt should not be called")
            },
            // F15 default-change resolution: these scenarios touch new
            // columns only, never `ModifyColumnDefault`, so the prompt
            // should never fire. Panic guards against silent flow drift.
            default_change: |_: &vespertide_planner::DefaultChangeWarning| -> Result<
                Option<crate::commands::revision::prompts::DefaultChoice>,
            > {
                panic!("default_change prompt should not be called")
            },
            // F2 unique-addition resolution: these scenarios add columns or
            // create tables only, never `AddConstraint(Unique)` on an
            // existing column, so the prompt should never fire. Panic
            // guards against silent flow drift.
            unique_addition: |_: &vespertide_planner::UniqueAdditionWarning| -> Result<
                Option<crate::commands::revision::prompts::UniqueAdditionChoice>,
            > {
                panic!("unique_addition prompt should not be called")
            },
        },
    )
    .await;

    assert!(
        result.is_ok(),
        "cmd_revision_core failed: {:?}",
        result.err()
    );

    // Verify migration was written with fill_with
    let entries: Vec<_> = std_fs::read_dir(cfg.migrations_dir())
        .unwrap()
        .filter_map(std::result::Result::ok)
        .collect();
    assert_eq!(entries.len(), 2);

    // Read the v2 migration and verify fill_with was applied
    let v2_path = entries
        .iter()
        .find(|e| e.file_name().to_string_lossy().contains("0002"))
        .expect("v2 migration not found");
    let v2_content = std_fs::read_to_string(v2_path.path()).unwrap();
    assert!(
        v2_content.contains("fill_with"),
        "Expected fill_with in migration, got: {v2_content}"
    );
}
