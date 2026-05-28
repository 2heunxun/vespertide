use super::*;
use crate::sql::types::DatabaseBackend;
use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType, TableConstraint, TableDef};

#[test]
fn test_add_constraint_foreign_key_sqlite_table_not_found() {
    let constraint = TableConstraint::ForeignKey {
        name: Some("fk_user".into()),
        columns: vec!["user_id".into()],
        ref_table: "users".into(),
        ref_columns: vec!["id".into()],
        on_delete: None,
        on_update: None,
        orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
    };
    let current_schema = vec![]; // Empty schema - table not found
    let result = build_add_constraint(
        DatabaseBackend::Sqlite,
        "posts",
        &constraint,
        &current_schema,
        &[],
    );
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("Table 'posts' not found in current schema"));
}

#[test]
fn test_add_constraint_foreign_key_sqlite_with_check_constraints() {
    let constraint = TableConstraint::ForeignKey {
        name: Some("fk_user".into()),
        columns: vec!["user_id".into()],
        ref_table: "users".into(),
        ref_columns: vec!["id".into()],
        on_delete: None,
        on_update: None,
        orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
    };
    let current_schema = vec![TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "user_id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Integer),
            nullable: true,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![TableConstraint::Check {
            name: "chk_user_id".into(),
            expr: "user_id > 0".into(),
            strategy: vespertide_core::CheckViolationStrategy::default(),
        }],
    }];
    let result = build_add_constraint(
        DatabaseBackend::Sqlite,
        "posts",
        &constraint,
        &current_schema,
        &[],
    );
    assert!(result.is_ok());
    let queries = result.unwrap();
    let sql = queries
        .iter()
        .map(|q| q.build(DatabaseBackend::Sqlite))
        .collect::<Vec<String>>()
        .join("\n");
    assert!(sql.contains("CONSTRAINT \"chk_user_id\" CHECK"));
}

#[test]
fn test_add_constraint_foreign_key_sqlite_with_indexes() {
    let constraint = TableConstraint::ForeignKey {
        name: Some("fk_user".into()),
        columns: vec!["user_id".into()],
        ref_table: "users".into(),
        ref_columns: vec!["id".into()],
        on_delete: None,
        on_update: None,
        orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
    };
    let current_schema = vec![TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "user_id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Integer),
            nullable: true,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![TableConstraint::Index {
            name: Some("idx_user_id".into()),
            columns: vec!["user_id".into()],
        }],
    }];
    let result = build_add_constraint(
        DatabaseBackend::Sqlite,
        "posts",
        &constraint,
        &current_schema,
        &[],
    );
    assert!(result.is_ok());
    let queries = result.unwrap();
    let sql = queries
        .iter()
        .map(|q| q.build(DatabaseBackend::Sqlite))
        .collect::<Vec<String>>()
        .join("\n");
    assert!(sql.contains("CREATE INDEX"));
    assert!(sql.contains("idx_user_id"));
}

#[test]
fn test_add_constraint_foreign_key_sqlite_with_unique_constraint() {
    let constraint = TableConstraint::ForeignKey {
        name: Some("fk_user".into()),
        columns: vec!["user_id".into()],
        ref_table: "users".into(),
        ref_columns: vec!["id".into()],
        on_delete: None,
        on_update: None,
        orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
    };
    let current_schema = vec![TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "user_id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Integer),
            nullable: true,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![TableConstraint::Unique {
            name: Some("uq_user_id".into()),
            columns: vec!["user_id".into()],
            strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates { keep: vespertide_core::KeepPolicy::First },
        }],
    }];
    let result = build_add_constraint(
        DatabaseBackend::Sqlite,
        "posts",
        &constraint,
        &current_schema,
        &[],
    );
    assert!(result.is_ok());
    let queries = result.unwrap();
    let sql = queries
        .iter()
        .map(|q| q.build(DatabaseBackend::Sqlite))
        .collect::<Vec<String>>()
        .join("\n");
    assert!(sql.contains("CREATE TABLE"));
}

#[test]
fn test_add_constraint_foreign_key_sqlite_without_existing_check() {
    let constraint = TableConstraint::ForeignKey {
        name: Some("fk_user".into()),
        columns: vec!["user_id".into()],
        ref_table: "users".into(),
        ref_columns: vec!["id".into()],
        on_delete: None,
        on_update: None,
        orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
    };
    let current_schema = vec![TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "user_id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Integer),
            nullable: true,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![], // No existing CHECK constraints
    }];
    let result = build_add_constraint(
        DatabaseBackend::Sqlite,
        "posts",
        &constraint,
        &current_schema,
        &[],
    );
    assert!(result.is_ok());
    let queries = result.unwrap();
    let sql = queries
        .iter()
        .map(|q| q.build(DatabaseBackend::Sqlite))
        .collect::<Vec<String>>()
        .join("\n");
    assert!(sql.contains("CREATE TABLE"));
    assert!(sql.contains("FOREIGN KEY"));
}
