use super::*;
use crate::schema::{ReferenceAction, SimpleColumnType};
use rstest::rstest;

fn default_column() -> ColumnDef {
    ColumnDef {
        name: "email".into(),
        r#type: ColumnType::Simple(SimpleColumnType::Text),
        nullable: true,
        default: None,
        comment: None,
        primary_key: None,
        unique: None,
        index: None,
        foreign_key: None,
    }
}

#[test]
fn migration_action_wire_format_round_trip() {
    let canonical = r#"{"type":"create_table","table":"user","columns":[],"constraints":[]}"#;
    let parsed: MigrationAction = serde_json::from_str(canonical).expect("parse");
    let reserialized = serde_json::to_string(&parsed).expect("serialize");

    assert_eq!(
        reserialized, canonical,
        "wire format MUST be byte-identical"
    );
}

#[test]
fn migration_action_rename_column_wire_format() {
    let canonical = r#"{"type":"rename_column","table":"orders","from":"old","to":"new"}"#;
    let parsed: MigrationAction = serde_json::from_str(canonical).expect("parse");
    let reserialized = serde_json::to_string(&parsed).expect("serialize");

    assert_eq!(reserialized, canonical);
}

#[test]
fn migration_plan_real_example_round_trip() {
    let plan_json = include_str!("../../../../examples/app/migrations/0001_init.vespertide.json");
    let parsed: MigrationPlan =
        serde_json::from_str(plan_json).expect("real migration plan parses");
    let reserialized = serde_json::to_string(&parsed).expect("serialize");
    let reparsed: MigrationPlan = serde_json::from_str(&reserialized).expect("round-trip");

    assert_eq!(
        parsed, reparsed,
        "semantic content preserved across round-trip"
    );
}

#[rstest]
#[case::create_table(
        MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![],
            constraints: vec![],
        },
        "CreateTable: users"
    )]
#[case::delete_table(
        MigrationAction::DeleteTable {
            table: "users".into(),
        },
        "DeleteTable: users"
    )]
#[case::add_column(
        MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(default_column()),
            fill_with: None,
        },
        "AddColumn: users.email"
    )]
#[case::rename_column(
        MigrationAction::RenameColumn {
            table: "users".into(),
            from: "old_name".into(),
            to: "new_name".into(),
        },
        "RenameColumn: users.old_name -> new_name"
    )]
#[case::delete_column(
        MigrationAction::DeleteColumn {
            table: "users".into(),
            column: "email".into(),
        },
        "DeleteColumn: users.email"
    )]
#[case::modify_column_type(
        MigrationAction::ModifyColumnType {
            table: "users".into(),
            column: "age".into(),
            new_type: ColumnType::Simple(SimpleColumnType::Integer),
            fill_with: None,
        },
        "ModifyColumnType: users.age"
    )]
#[case::add_constraint_index_with_name(
        MigrationAction::AddConstraint {
            table: "users".into(),
            constraint: TableConstraint::Index {
                name: Some("ix_users__email".into()),
                columns: vec!["email".into()],
            },
        },
        "AddConstraint: users.ix_users__email (INDEX)"
    )]
#[case::add_constraint_index_without_name(
        MigrationAction::AddConstraint {
            table: "users".into(),
            constraint: TableConstraint::Index {
                name: None,
                columns: vec!["email".into()],
            },
        },
        "AddConstraint: users.INDEX"
    )]
#[case::remove_constraint_index_with_name(
        MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: TableConstraint::Index {
                name: Some("ix_users__email".into()),
                columns: vec!["email".into()],
            },
        },
        "RemoveConstraint: users.ix_users__email (INDEX)"
    )]
#[case::remove_constraint_index_without_name(
        MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: TableConstraint::Index {
                name: None,
                columns: vec!["email".into()],
            },
        },
        "RemoveConstraint: users.INDEX"
    )]
#[case::rename_table(
        MigrationAction::RenameTable {
            from: "old_table".into(),
            to: "new_table".into(),
        },
        "RenameTable: old_table -> new_table"
    )]
fn test_display_basic_actions(#[case] action: MigrationAction, #[case] expected: &str) {
    assert_eq!(action.to_string(), expected);
}

#[test]
fn test_display_raw_sql_truncates_unicode_without_panicking() {
    let sql = "COMMENT ON COLUMN 한국어테이블.이름 IS '日本語 café 📊';".repeat(3);
    let action = MigrationAction::RawSql { sql };

    let display = action.to_string();

    assert!(display.starts_with("RawSql: COMMENT ON COLUMN 한국어테이블"));
    assert!(display.ends_with("..."));
}

#[rstest]
#[case::create_table(
    MigrationAction::CreateTable {
        table: "users".into(),
        columns: vec![],
        constraints: vec![],
    },
    Some("users")
)]
#[case::rename_table(
    MigrationAction::RenameTable {
        from: "old_users".into(),
        to: "users".into(),
    },
    Some("old_users")
)]
#[case::raw_sql(MigrationAction::RawSql { sql: "SELECT 1".into() }, None)]
fn test_table_name(#[case] action: MigrationAction, #[case] expected: Option<&str>) {
    assert_eq!(action.table_name(), expected);
}

#[rstest]
#[case::add_constraint_primary_key(
        MigrationAction::AddConstraint {
            table: "users".into(),
            constraint: TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            },
        },
        "AddConstraint: users.PRIMARY KEY"
    )]
#[case::add_constraint_unique_with_name(
        MigrationAction::AddConstraint {
            table: "users".into(),
            constraint: TableConstraint::Unique {
                name: Some("uq_email".into()),
                columns: vec!["email".into()],
            },
        },
        "AddConstraint: users.uq_email (UNIQUE)"
    )]
#[case::add_constraint_unique_without_name(
        MigrationAction::AddConstraint {
            table: "users".into(),
            constraint: TableConstraint::Unique {
                name: None,
                columns: vec!["email".into()],
            },
        },
        "AddConstraint: users.UNIQUE"
    )]
#[case::add_constraint_foreign_key_with_name(
        MigrationAction::AddConstraint {
            table: "posts".into(),
            constraint: TableConstraint::ForeignKey {
                name: Some("fk_user".into()),
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some(ReferenceAction::Cascade),
                on_update: None,
            },
        },
        "AddConstraint: posts.fk_user (FOREIGN KEY)"
    )]
#[case::add_constraint_foreign_key_without_name(
        MigrationAction::AddConstraint {
            table: "posts".into(),
            constraint: TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            },
        },
        "AddConstraint: posts.FOREIGN KEY"
    )]
#[case::add_constraint_check(
        MigrationAction::AddConstraint {
            table: "users".into(),
            constraint: TableConstraint::Check {
                name: "chk_age".into(),
                expr: "age > 0".into(),
            },
        },
        "AddConstraint: users.chk_age (CHECK)"
    )]
fn test_display_add_constraint(#[case] action: MigrationAction, #[case] expected: &str) {
    assert_eq!(action.to_string(), expected);
}

#[rstest]
#[case::remove_constraint_primary_key(
        MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            },
        },
        "RemoveConstraint: users.PRIMARY KEY"
    )]
#[case::remove_constraint_unique_with_name(
        MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: TableConstraint::Unique {
                name: Some("uq_email".into()),
                columns: vec!["email".into()],
            },
        },
        "RemoveConstraint: users.uq_email (UNIQUE)"
    )]
#[case::remove_constraint_unique_without_name(
        MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: TableConstraint::Unique {
                name: None,
                columns: vec!["email".into()],
            },
        },
        "RemoveConstraint: users.UNIQUE"
    )]
#[case::remove_constraint_foreign_key_with_name(
        MigrationAction::RemoveConstraint {
            table: "posts".into(),
            constraint: TableConstraint::ForeignKey {
                name: Some("fk_user".into()),
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            },
        },
        "RemoveConstraint: posts.fk_user (FOREIGN KEY)"
    )]
#[case::remove_constraint_foreign_key_without_name(
        MigrationAction::RemoveConstraint {
            table: "posts".into(),
            constraint: TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            },
        },
        "RemoveConstraint: posts.FOREIGN KEY"
    )]
#[case::remove_constraint_check(
        MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: TableConstraint::Check {
                name: "chk_age".into(),
                expr: "age > 0".into(),
            },
        },
        "RemoveConstraint: users.chk_age (CHECK)"
    )]
fn test_display_remove_constraint(#[case] action: MigrationAction, #[case] expected: &str) {
    assert_eq!(action.to_string(), expected);
}

#[rstest]
#[case::raw_sql_short(
        MigrationAction::RawSql {
            sql: "SELECT 1".into(),
        },
        "RawSql: SELECT 1"
    )]
fn test_display_raw_sql_short(#[case] action: MigrationAction, #[case] expected: &str) {
    assert_eq!(action.to_string(), expected);
}

#[test]
fn test_display_raw_sql_long() {
    let action = MigrationAction::RawSql {
        sql: "SELECT * FROM users WHERE id = 1 AND name = 'test' AND email = 'test@example.com'"
            .into(),
    };
    let result = action.to_string();
    assert!(result.starts_with("RawSql: "));
    assert!(result.ends_with("..."));
    assert!(result.len() > 10);
}

#[rstest]
#[case::modify_column_nullable_to_not_null(
        MigrationAction::ModifyColumnNullable {
            table: "users".into(),
            column: "email".into(),
            nullable: false,
            fill_with: None,
            delete_null_rows: None,
        },
        "ModifyColumnNullable: users.email -> NOT NULL"
    )]
#[case::modify_column_nullable_to_null(
        MigrationAction::ModifyColumnNullable {
            table: "users".into(),
            column: "email".into(),
            nullable: true,
            fill_with: None,
            delete_null_rows: None,
        },
        "ModifyColumnNullable: users.email -> NULL"
    )]
fn test_display_modify_column_nullable(#[case] action: MigrationAction, #[case] expected: &str) {
    assert_eq!(action.to_string(), expected);
}

#[rstest]
#[case::modify_column_default_set(
        MigrationAction::ModifyColumnDefault {
            table: "users".into(),
            column: "status".into(),
            new_default: Some("'active'".into()),
        },
        "ModifyColumnDefault: users.status -> 'active'"
    )]
#[case::modify_column_default_drop(
        MigrationAction::ModifyColumnDefault {
            table: "users".into(),
            column: "status".into(),
            new_default: None,
        },
        "ModifyColumnDefault: users.status -> (none)"
    )]
fn test_display_modify_column_default(#[case] action: MigrationAction, #[case] expected: &str) {
    assert_eq!(action.to_string(), expected);
}

#[rstest]
#[case::modify_column_comment_set(
        MigrationAction::ModifyColumnComment {
            table: "users".into(),
            column: "email".into(),
            new_comment: Some("User email address".into()),
        },
        "ModifyColumnComment: users.email -> 'User email address'"
    )]
#[case::modify_column_comment_drop(
        MigrationAction::ModifyColumnComment {
            table: "users".into(),
            column: "email".into(),
            new_comment: None,
        },
        "ModifyColumnComment: users.email -> (none)"
    )]
fn test_display_modify_column_comment(#[case] action: MigrationAction, #[case] expected: &str) {
    assert_eq!(action.to_string(), expected);
}

#[test]
fn test_display_modify_column_comment_long() {
    // Test truncation for long comments (> 30 chars)
    let action = MigrationAction::ModifyColumnComment {
        table: "users".into(),
        column: "email".into(),
        new_comment: Some("This is a very long comment that should be truncated in display".into()),
    };
    let result = action.to_string();
    assert!(result.contains("..."));
    assert!(result.contains("This is a very long comment"));
    // Should be truncated at 27 chars + "..."
    assert!(!result.contains("truncated in display"));
}

// Tests for with_prefix
#[test]
fn test_action_with_prefix_create_table() {
    let action = MigrationAction::CreateTable {
        table: "users".into(),
        columns: vec![default_column()],
        constraints: vec![TableConstraint::ForeignKey {
            name: Some("fk_org".into()),
            columns: vec!["org_id".into()],
            ref_table: "organizations".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        }],
    };
    let prefixed = action.with_prefix("myapp_");
    if let MigrationAction::CreateTable {
        table, constraints, ..
    } = prefixed
    {
        assert_eq!(table.as_str(), "myapp_users");
        if let TableConstraint::ForeignKey { ref_table, .. } = &constraints[0] {
            assert_eq!(ref_table.as_str(), "myapp_organizations");
        }
    } else {
        panic!("Expected CreateTable");
    }
}

#[test]
fn test_action_with_prefix_delete_table() {
    let action = MigrationAction::DeleteTable {
        table: "users".into(),
    };
    let prefixed = action.with_prefix("myapp_");
    if let MigrationAction::DeleteTable { table } = prefixed {
        assert_eq!(table.as_str(), "myapp_users");
    } else {
        panic!("Expected DeleteTable");
    }
}

#[test]
fn test_action_with_prefix_add_column() {
    let action = MigrationAction::AddColumn {
        table: "users".into(),
        column: Box::new(default_column()),
        fill_with: None,
    };
    let prefixed = action.with_prefix("myapp_");
    if let MigrationAction::AddColumn { table, .. } = prefixed {
        assert_eq!(table.as_str(), "myapp_users");
    } else {
        panic!("Expected AddColumn");
    }
}

#[test]
fn test_action_with_prefix_rename_table() {
    let action = MigrationAction::RenameTable {
        from: "old_table".into(),
        to: "new_table".into(),
    };
    let prefixed = action.with_prefix("myapp_");
    if let MigrationAction::RenameTable { from, to } = prefixed {
        assert_eq!(from.as_str(), "myapp_old_table");
        assert_eq!(to.as_str(), "myapp_new_table");
    } else {
        panic!("Expected RenameTable");
    }
}

#[test]
fn test_action_with_prefix_raw_sql_unchanged() {
    let action = MigrationAction::RawSql {
        sql: "SELECT * FROM users".into(),
    };
    let prefixed = action.with_prefix("myapp_");
    if let MigrationAction::RawSql { sql } = prefixed {
        // RawSql is not modified - user is responsible for table names
        assert_eq!(sql, "SELECT * FROM users");
    } else {
        panic!("Expected RawSql");
    }
}

#[test]
fn test_action_with_prefix_empty_prefix() {
    let action = MigrationAction::CreateTable {
        table: "users".into(),
        columns: vec![],
        constraints: vec![],
    };
    let prefixed = action.clone().with_prefix("");
    if let MigrationAction::CreateTable { table, .. } = prefixed {
        assert_eq!(table.as_str(), "users");
    }
}

#[test]
fn test_migration_plan_with_prefix() {
    let plan = MigrationPlan {
        id: String::new(),
        comment: Some("test".into()),
        created_at: None,
        version: 1,
        actions: vec![
            MigrationAction::CreateTable {
                table: "users".into(),
                columns: vec![],
                constraints: vec![],
            },
            MigrationAction::CreateTable {
                table: "posts".into(),
                columns: vec![],
                constraints: vec![TableConstraint::ForeignKey {
                    name: Some("fk_user".into()),
                    columns: vec!["user_id".into()],
                    ref_table: "users".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                }],
            },
        ],
    };
    let prefixed = plan.with_prefix("myapp_");
    assert_eq!(prefixed.actions.len(), 2);

    if let MigrationAction::CreateTable { table, .. } = &prefixed.actions[0] {
        assert_eq!(table.as_str(), "myapp_users");
    }
    if let MigrationAction::CreateTable {
        table, constraints, ..
    } = &prefixed.actions[1]
    {
        assert_eq!(table.as_str(), "myapp_posts");
        if let TableConstraint::ForeignKey { ref_table, .. } = &constraints[0] {
            assert_eq!(ref_table.as_str(), "myapp_users");
        }
    }
}

#[test]
fn test_action_with_prefix_rename_column() {
    let action = MigrationAction::RenameColumn {
        table: "users".into(),
        from: "name".into(),
        to: "full_name".into(),
    };
    let prefixed = action.with_prefix("myapp_");
    if let MigrationAction::RenameColumn { table, from, to } = prefixed {
        assert_eq!(table.as_str(), "myapp_users");
        assert_eq!(from.as_str(), "name");
        assert_eq!(to.as_str(), "full_name");
    } else {
        panic!("Expected RenameColumn");
    }
}

#[test]
fn test_action_with_prefix_delete_column() {
    let action = MigrationAction::DeleteColumn {
        table: "users".into(),
        column: "old_field".into(),
    };
    let prefixed = action.with_prefix("myapp_");
    if let MigrationAction::DeleteColumn { table, column } = prefixed {
        assert_eq!(table.as_str(), "myapp_users");
        assert_eq!(column.as_str(), "old_field");
    } else {
        panic!("Expected DeleteColumn");
    }
}

#[test]
fn test_action_with_prefix_modify_column_type() {
    let action = MigrationAction::ModifyColumnType {
        table: "users".into(),
        column: "age".into(),
        new_type: ColumnType::Simple(SimpleColumnType::BigInt),
        fill_with: None,
    };
    let prefixed = action.with_prefix("myapp_");
    if let MigrationAction::ModifyColumnType {
        table,
        column,
        new_type,
        fill_with,
    } = prefixed
    {
        assert_eq!(table.as_str(), "myapp_users");
        assert_eq!(column.as_str(), "age");
        assert!(matches!(
            new_type,
            ColumnType::Simple(SimpleColumnType::BigInt)
        ));
        assert_eq!(fill_with, None);
    } else {
        panic!("Expected ModifyColumnType");
    }
}

#[test]
fn test_action_with_prefix_modify_column_nullable() {
    let action = MigrationAction::ModifyColumnNullable {
        table: "users".into(),
        column: "email".into(),
        nullable: false,
        fill_with: Some("default@example.com".into()),
        delete_null_rows: None,
    };
    let prefixed = action.with_prefix("myapp_");
    if let MigrationAction::ModifyColumnNullable {
        table,
        column,
        nullable,
        fill_with,
        delete_null_rows,
    } = prefixed
    {
        assert_eq!(table.as_str(), "myapp_users");
        assert_eq!(column.as_str(), "email");
        assert!(!nullable);
        assert_eq!(fill_with, Some("default@example.com".into()));
        assert_eq!(delete_null_rows, None);
    } else {
        panic!("Expected ModifyColumnNullable");
    }
}

#[test]
fn test_action_with_prefix_modify_column_default() {
    let action = MigrationAction::ModifyColumnDefault {
        table: "users".into(),
        column: "status".into(),
        new_default: Some("active".into()),
    };
    let prefixed = action.with_prefix("myapp_");
    if let MigrationAction::ModifyColumnDefault {
        table,
        column,
        new_default,
    } = prefixed
    {
        assert_eq!(table.as_str(), "myapp_users");
        assert_eq!(column.as_str(), "status");
        assert_eq!(new_default, Some("active".into()));
    } else {
        panic!("Expected ModifyColumnDefault");
    }
}

#[test]
fn test_action_with_prefix_modify_column_comment() {
    let action = MigrationAction::ModifyColumnComment {
        table: "users".into(),
        column: "bio".into(),
        new_comment: Some("User biography".into()),
    };
    let prefixed = action.with_prefix("myapp_");
    if let MigrationAction::ModifyColumnComment {
        table,
        column,
        new_comment,
    } = prefixed
    {
        assert_eq!(table.as_str(), "myapp_users");
        assert_eq!(column.as_str(), "bio");
        assert_eq!(new_comment, Some("User biography".into()));
    } else {
        panic!("Expected ModifyColumnComment");
    }
}

#[test]
fn test_action_with_prefix_add_constraint() {
    let action = MigrationAction::AddConstraint {
        table: "posts".into(),
        constraint: TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        },
    };
    let prefixed = action.with_prefix("myapp_");
    if let MigrationAction::AddConstraint { table, constraint } = prefixed {
        assert_eq!(table.as_str(), "myapp_posts");
        if let TableConstraint::ForeignKey { ref_table, .. } = constraint {
            assert_eq!(ref_table.as_str(), "myapp_users");
        } else {
            panic!("Expected ForeignKey constraint");
        }
    } else {
        panic!("Expected AddConstraint");
    }
}

#[test]
fn test_action_with_prefix_remove_constraint() {
    let action = MigrationAction::RemoveConstraint {
        table: "posts".into(),
        constraint: TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        },
    };
    let prefixed = action.with_prefix("myapp_");
    if let MigrationAction::RemoveConstraint { table, constraint } = prefixed {
        assert_eq!(table.as_str(), "myapp_posts");
        if let TableConstraint::ForeignKey { ref_table, .. } = constraint {
            assert_eq!(ref_table.as_str(), "myapp_users");
        } else {
            panic!("Expected ForeignKey constraint");
        }
    } else {
        panic!("Expected RemoveConstraint");
    }
}

#[rstest]
#[case::replace_constraint_primary_key(
        MigrationAction::ReplaceConstraint {
            table: "users".into(),
            from: TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            },
            to: TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
            },
        },
        "ReplaceConstraint: users.PRIMARY KEY"
    )]
#[case::replace_constraint_unique_with_name(
        MigrationAction::ReplaceConstraint {
            table: "users".into(),
            from: TableConstraint::Unique {
                name: None,
                columns: vec!["email".into()],
            },
            to: TableConstraint::Unique {
                name: Some("uq_email".into()),
                columns: vec!["email".into()],
            },
        },
        "ReplaceConstraint: users.uq_email (UNIQUE)"
    )]
#[case::replace_constraint_unique_without_name(
        MigrationAction::ReplaceConstraint {
            table: "users".into(),
            from: TableConstraint::Unique {
                name: Some("uq_email".into()),
                columns: vec!["email".into()],
            },
            to: TableConstraint::Unique {
                name: None,
                columns: vec!["email".into()],
            },
        },
        "ReplaceConstraint: users.UNIQUE"
    )]
#[case::replace_constraint_foreign_key_with_name(
        MigrationAction::ReplaceConstraint {
            table: "posts".into(),
            from: TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            },
            to: TableConstraint::ForeignKey {
                name: Some("fk_user".into()),
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            },
        },
        "ReplaceConstraint: posts.fk_user (FOREIGN KEY)"
    )]
#[case::replace_constraint_foreign_key_without_name(
        MigrationAction::ReplaceConstraint {
            table: "posts".into(),
            from: TableConstraint::ForeignKey {
                name: Some("fk_user".into()),
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            },
            to: TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            },
        },
        "ReplaceConstraint: posts.FOREIGN KEY"
    )]
#[case::replace_constraint_check(
        MigrationAction::ReplaceConstraint {
            table: "users".into(),
            from: TableConstraint::Check {
                name: "chk_age".into(),
                expr: "age > 0".into(),
            },
            to: TableConstraint::Check {
                name: "chk_age".into(),
                expr: "age >= 0".into(),
            },
        },
        "ReplaceConstraint: users.chk_age (CHECK)"
    )]
#[case::replace_constraint_index_with_name(
        MigrationAction::ReplaceConstraint {
            table: "users".into(),
            from: TableConstraint::Index {
                name: None,
                columns: vec!["email".into()],
            },
            to: TableConstraint::Index {
                name: Some("ix_users__email".into()),
                columns: vec!["email".into()],
            },
        },
        "ReplaceConstraint: users.ix_users__email (INDEX)"
    )]
#[case::replace_constraint_index_without_name(
        MigrationAction::ReplaceConstraint {
            table: "users".into(),
            from: TableConstraint::Index {
                name: Some("ix_users__email".into()),
                columns: vec!["email".into()],
            },
            to: TableConstraint::Index {
                name: None,
                columns: vec!["email".into()],
            },
        },
        "ReplaceConstraint: users.INDEX"
    )]
fn test_display_replace_constraint(#[case] action: MigrationAction, #[case] expected: &str) {
    assert_eq!(action.to_string(), expected);
}

#[test]
fn test_action_with_prefix_replace_constraint() {
    let action = MigrationAction::ReplaceConstraint {
        table: "posts".into(),
        from: TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: Some(ReferenceAction::Cascade),
            on_update: None,
        },
        to: TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: Some(ReferenceAction::SetNull),
            on_update: None,
        },
    };
    let prefixed = action.with_prefix("myapp_");
    if let MigrationAction::ReplaceConstraint { table, from, to } = prefixed {
        assert_eq!(table.as_str(), "myapp_posts");
        if let TableConstraint::ForeignKey { ref_table, .. } = from {
            assert_eq!(ref_table.as_str(), "myapp_users");
        } else {
            panic!("Expected ForeignKey constraint in from");
        }
        if let TableConstraint::ForeignKey { ref_table, .. } = to {
            assert_eq!(ref_table.as_str(), "myapp_users");
        } else {
            panic!("Expected ForeignKey constraint in to");
        }
    } else {
        panic!("Expected ReplaceConstraint");
    }
}
