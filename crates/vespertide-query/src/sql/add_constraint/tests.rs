use super::*;
use crate::sql::types::DatabaseBackend;
use insta::{assert_snapshot, with_settings};
use rstest::rstest;
use vespertide_core::{
    ColumnDef, ColumnType, ReferenceAction, SimpleColumnType, TableConstraint, TableDef,
};
#[rstest]
#[case::add_constraint_primary_key_postgres(
        "add_constraint_primary_key_postgres",
        DatabaseBackend::Postgres,
        &["ALTER TABLE \"users\" ADD PRIMARY KEY (\"id\")"]
    )]
#[case::add_constraint_primary_key_mysql(
        "add_constraint_primary_key_mysql",
        DatabaseBackend::MySql,
        &["ALTER TABLE `users` ADD PRIMARY KEY (`id`)"]
    )]
#[case::add_constraint_primary_key_sqlite(
        "add_constraint_primary_key_sqlite",
        DatabaseBackend::Sqlite,
        &["CREATE TABLE \"users_temp\""]
    )]
#[case::add_constraint_unique_named_postgres(
        "add_constraint_unique_named_postgres",
        DatabaseBackend::Postgres,
        &["CREATE UNIQUE INDEX \"uq_users__uq_email\" ON \"users\" (\"email\")"]
    )]
#[case::add_constraint_unique_named_mysql(
        "add_constraint_unique_named_mysql",
        DatabaseBackend::MySql,
        &["CREATE UNIQUE INDEX `uq_users__uq_email` ON `users` (`email`)"]
    )]
#[case::add_constraint_unique_named_sqlite(
        "add_constraint_unique_named_sqlite",
        DatabaseBackend::Sqlite,
        &["CREATE UNIQUE INDEX \"uq_users__uq_email\" ON \"users\" (\"email\")"]
    )]
#[case::add_constraint_foreign_key_postgres(
        "add_constraint_foreign_key_postgres",
        DatabaseBackend::Postgres,
        &["FOREIGN KEY (\"user_id\")", "REFERENCES \"users\" (\"id\")", "ON DELETE CASCADE", "ON UPDATE RESTRICT"]
    )]
#[case::add_constraint_foreign_key_mysql(
        "add_constraint_foreign_key_mysql",
        DatabaseBackend::MySql,
        &["FOREIGN KEY (`user_id`)", "REFERENCES `users` (`id`)", "ON DELETE CASCADE", "ON UPDATE RESTRICT"]
    )]
#[case::add_constraint_foreign_key_sqlite(
        "add_constraint_foreign_key_sqlite",
        DatabaseBackend::Sqlite,
        &["CREATE TABLE \"users_temp\""]
    )]
#[case::add_constraint_check_named_postgres(
        "add_constraint_check_named_postgres",
        DatabaseBackend::Postgres,
        &["ADD CONSTRAINT \"chk_age\" CHECK (age > 0)"]
    )]
#[case::add_constraint_check_named_mysql(
        "add_constraint_check_named_mysql",
        DatabaseBackend::MySql,
        &["ADD CONSTRAINT `chk_age` CHECK (age > 0)"]
    )]
#[case::add_constraint_check_named_sqlite(
        "add_constraint_check_named_sqlite",
        DatabaseBackend::Sqlite,
        &["CREATE TABLE \"users_temp\""]
    )]
fn test_add_constraint(
    #[case] title: &str,
    #[case] backend: DatabaseBackend,
    #[case] expected: &[&str],
) {
    let constraint = if title.contains("primary_key") {
        TableConstraint::PrimaryKey {
            columns: vec!["id".into()],
            auto_increment: false,
        }
    } else if title.contains("unique") {
        TableConstraint::Unique {
            name: Some("uq_email".into()),
            columns: vec!["email".into()],
        }
    } else if title.contains("foreign_key") {
        TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: Some(ReferenceAction::Cascade),
            on_update: Some(ReferenceAction::Restrict),
        }
    } else {
        TableConstraint::Check {
            name: "chk_age".into(),
            expr: "age > 0".into(),
        }
    };
    let current_schema = vec![TableDef {
        name: "users".into(),
        description: None,
        columns: if title.contains("foreign_key") {
            vec![
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
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ]
        } else {
            vec![
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
                    name: if title.contains("check") {
                        "age".into()
                    } else {
                        "email".into()
                    },
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ]
        },
        constraints: vec![],
    }];
    let result = build_add_constraint(backend, "users", &constraint, &current_schema, &[]).unwrap();
    let sql = result[0].build(backend);
    for exp in expected {
        assert!(
            sql.contains(exp),
            "Expected SQL to contain '{exp}', got: {sql}"
        );
    }
    with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("add_constraint_{}", title) }, {
        assert_snapshot!(result.iter().map(|q| q.build(backend)).collect::<Vec<String>>().join("\n"));
    });
}
#[test]
fn test_add_constraint_primary_key_sqlite_table_not_found() {
    let constraint = TableConstraint::PrimaryKey {
        columns: vec!["id".into()],
        auto_increment: false,
    };
    let current_schema = vec![]; // Empty schema - table not found
    let result = build_add_constraint(
        DatabaseBackend::Sqlite,
        "users",
        &constraint,
        &current_schema,
        &[],
    );
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("Table 'users' not found in current schema"));
}

#[test]
fn add_check_constraint_escapes_adversarial_identifiers() {
    let constraint = TableConstraint::Check {
        name: "chk_age\"quote".into(),
        expr: "age > 0".into(),
    };
    let current_schema = vec![TableDef {
        name: "users\"archive".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "age".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Integer),
            nullable: false,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![],
    }];

    let pg_sql = build_add_constraint(
        DatabaseBackend::Postgres,
        "users\"archive",
        &constraint,
        &current_schema,
        &[],
    )
    .unwrap()[0]
        .build(DatabaseBackend::Postgres);
    assert_eq!(
        pg_sql,
        "ALTER TABLE \"users\"\"archive\" ADD CONSTRAINT \"chk_age\"\"quote\" CHECK (age > 0)"
    );

    let mysql_constraint = TableConstraint::Check {
        name: "chk_age`quote".into(),
        expr: "age > 0".into(),
    };
    let mysql_sql = build_add_constraint(
        DatabaseBackend::MySql,
        "users`archive",
        &mysql_constraint,
        &current_schema,
        &[],
    )
    .unwrap()[0]
        .build(DatabaseBackend::MySql);
    assert_eq!(
        mysql_sql,
        "ALTER TABLE `users``archive` ADD CONSTRAINT `chk_age``quote` CHECK (age > 0)"
    );
}

#[test]
fn test_add_constraint_primary_key_sqlite_with_check_constraints() {
    let constraint = TableConstraint::PrimaryKey {
        columns: vec!["id".into()],
        auto_increment: false,
    };
    let current_schema = vec![TableDef {
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
        constraints: vec![TableConstraint::Check {
            name: "chk_id".into(),
            expr: "id > 0".into(),
        }],
    }];
    let result = build_add_constraint(
        DatabaseBackend::Sqlite,
        "users",
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
    assert!(sql.contains("CONSTRAINT \"chk_id\" CHECK"));
}
#[test]
fn test_add_constraint_primary_key_sqlite_with_indexes() {
    let constraint = TableConstraint::PrimaryKey {
        columns: vec!["id".into()],
        auto_increment: false,
    };
    let current_schema = vec![TableDef {
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
        constraints: vec![TableConstraint::Index {
            name: Some("idx_id".into()),
            columns: vec!["id".into()],
        }],
    }];
    let result = build_add_constraint(
        DatabaseBackend::Sqlite,
        "users",
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
    assert!(sql.contains("idx_id"));
}
#[test]
fn test_add_constraint_primary_key_sqlite_with_unique_constraint() {
    let constraint = TableConstraint::PrimaryKey {
        columns: vec!["id".into()],
        auto_increment: false,
    };
    let current_schema = vec![TableDef {
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
        constraints: vec![TableConstraint::Unique {
            name: Some("uq_email".into()),
            columns: vec!["email".into()],
        }],
    }];
    let result = build_add_constraint(
        DatabaseBackend::Sqlite,
        "users",
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
#[path = "tests_foreign_key.rs"]
mod foreign_key_tests;
#[test]
fn test_add_constraint_check_sqlite_table_not_found() {
    let constraint = TableConstraint::Check {
        name: "chk_age".into(),
        expr: "age > 0".into(),
    };
    let current_schema = vec![]; // Empty schema - table not found
    let result = build_add_constraint(
        DatabaseBackend::Sqlite,
        "users",
        &constraint,
        &current_schema,
        &[],
    );
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("Table 'users' not found in current schema"));
}
#[test]
fn test_add_constraint_check_sqlite_without_existing_check() {
    let constraint = TableConstraint::Check {
        name: "chk_age".into(),
        expr: "age > 0".into(),
    };
    let current_schema = vec![TableDef {
        name: "users".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "age".into(),
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
        "users",
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
    assert!(sql.contains("CONSTRAINT \"chk_age\" CHECK"));
}
#[test]
fn test_add_constraint_primary_key_sqlite_without_existing_check() {
    let constraint = TableConstraint::PrimaryKey {
        columns: vec!["id".into()],
        auto_increment: false,
    };
    let current_schema = vec![TableDef {
        name: "users".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "id".into(),
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
        "users",
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
    assert!(sql.contains("PRIMARY KEY"));
}

#[test]
fn test_add_constraint_check_sqlite_with_indexes() {
    let constraint = TableConstraint::Check {
        name: "chk_age".into(),
        expr: "age > 0".into(),
    };
    let current_schema = vec![TableDef {
        name: "users".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "age".into(),
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
            name: Some("idx_age".into()),
            columns: vec!["age".into()],
        }],
    }];
    let result = build_add_constraint(
        DatabaseBackend::Sqlite,
        "users",
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
    assert!(sql.contains("idx_age"));
}
#[test]
fn test_add_constraint_check_sqlite_with_unique_constraint() {
    let constraint = TableConstraint::Check {
        name: "chk_age".into(),
        expr: "age > 0".into(),
    };
    let current_schema = vec![TableDef {
        name: "users".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "age".into(),
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
            name: Some("uq_age".into()),
            columns: vec!["age".into()],
        }],
    }];
    let result = build_add_constraint(
        DatabaseBackend::Sqlite,
        "users",
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
fn test_add_constraint_composite_primary_key_postgres() {
    let constraint = TableConstraint::PrimaryKey {
        columns: vec!["user_id".into(), "role_id".into()],
        auto_increment: false,
    };
    let current_schema = vec![TableDef {
        name: "user_roles".into(),
        description: None,
        columns: vec![
            ColumnDef {
                name: "user_id".into(),
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
                name: "role_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![],
    }];
    let result = build_add_constraint(
        DatabaseBackend::Postgres,
        "user_roles",
        &constraint,
        &current_schema,
        &[],
    )
    .unwrap();
    let sql = result[0].build(DatabaseBackend::Postgres);
    assert!(sql.contains("ADD PRIMARY KEY"));
    assert!(sql.contains("\"user_id\""));
    assert!(sql.contains("\"role_id\""));
}
#[test]
fn test_add_constraint_composite_primary_key_mysql() {
    let constraint = TableConstraint::PrimaryKey {
        columns: vec!["user_id".into(), "role_id".into()],
        auto_increment: false,
    };
    let current_schema = vec![TableDef {
        name: "user_roles".into(),
        description: None,
        columns: vec![
            ColumnDef {
                name: "user_id".into(),
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
                name: "role_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![],
    }];
    let result = build_add_constraint(
        DatabaseBackend::MySql,
        "user_roles",
        &constraint,
        &current_schema,
        &[],
    )
    .unwrap();
    let sql = result[0].build(DatabaseBackend::MySql);
    assert!(sql.contains("ADD PRIMARY KEY"));
    assert!(sql.contains("`user_id`"));
    assert!(sql.contains("`role_id`"));
}
#[test]
fn test_constraints_overlap_primary_key_same_columns() {
    let a = TableConstraint::PrimaryKey {
        columns: vec!["id".into()],
        auto_increment: false,
    };
    let b = TableConstraint::PrimaryKey {
        columns: vec!["id".into()],
        auto_increment: true,
    };
    assert!(constraints_overlap(&a, &b));
}
#[test]
fn test_constraints_overlap_primary_key_different_columns() {
    let a = TableConstraint::PrimaryKey {
        columns: vec!["id".into()],
        auto_increment: false,
    };
    let b = TableConstraint::PrimaryKey {
        columns: vec!["uid".into()],
        auto_increment: false,
    };
    assert!(!constraints_overlap(&a, &b));
}
#[test]
fn test_constraints_overlap_check_same() {
    let a = TableConstraint::Check {
        name: "chk_age".into(),
        expr: "age > 0".into(),
    };
    let b = TableConstraint::Check {
        name: "chk_age".into(),
        expr: "age > 0".into(),
    };
    assert!(constraints_overlap(&a, &b));
}
#[test]
fn test_constraints_overlap_check_different_name() {
    let a = TableConstraint::Check {
        name: "chk_age".into(),
        expr: "age > 0".into(),
    };
    let b = TableConstraint::Check {
        name: "chk_age2".into(),
        expr: "age > 0".into(),
    };
    assert!(!constraints_overlap(&a, &b));
}
#[test]
fn test_constraints_overlap_check_different_expr() {
    let a = TableConstraint::Check {
        name: "chk_age".into(),
        expr: "age > 0".into(),
    };
    let b = TableConstraint::Check {
        name: "chk_age".into(),
        expr: "age > 10".into(),
    };
    assert!(!constraints_overlap(&a, &b));
}
#[test]
fn test_constraints_overlap_different_variants() {
    let a = TableConstraint::PrimaryKey {
        columns: vec!["id".into()],
        auto_increment: false,
    };
    let b = TableConstraint::Check {
        name: "chk".into(),
        expr: "id > 0".into(),
    };
    assert!(!constraints_overlap(&a, &b));
}
#[test]
fn test_constraints_overlap_fk_same_columns() {
    let a = TableConstraint::ForeignKey {
        name: None,
        columns: vec!["user_id".into()],
        ref_table: "users".into(),
        ref_columns: vec!["id".into()],
        on_delete: None,
        on_update: None,
    };
    let b = TableConstraint::ForeignKey {
        name: Some("fk".into()),
        columns: vec!["user_id".into()],
        ref_table: "other".into(),
        ref_columns: vec!["oid".into()],
        on_delete: Some(ReferenceAction::Cascade),
        on_update: None,
    };
    assert!(constraints_overlap(&a, &b));
}
#[test]
fn test_merge_constraint_replaces_overlapping() {
    let existing = vec![
        TableConstraint::PrimaryKey {
            columns: vec!["id".into()],
            auto_increment: false,
        },
        TableConstraint::Index {
            name: None,
            columns: vec!["email".into()],
        },
    ];
    let new_pk = TableConstraint::PrimaryKey {
        columns: vec!["id".into()],
        auto_increment: true,
    };
    let result = merge_constraint(&existing, &new_pk);
    assert_eq!(result.len(), 2); // replaced, not added
}
#[test]
fn test_merge_constraint_appends_non_overlapping() {
    let existing = vec![TableConstraint::Index {
        name: None,
        columns: vec!["email".into()],
    }];
    let new_pk = TableConstraint::PrimaryKey {
        columns: vec!["id".into()],
        auto_increment: false,
    };
    let result = merge_constraint(&existing, &new_pk);
    assert_eq!(result.len(), 2); // appended
}
#[test]
fn test_extract_check_clauses_with_mixed_constraints() {
    let constraints = vec![
        TableConstraint::Check {
            name: "chk1".into(),
            expr: "a > 0".into(),
        },
        TableConstraint::PrimaryKey {
            columns: vec!["id".into()],
            auto_increment: false,
        },
        TableConstraint::Check {
            name: "chk2".into(),
            expr: "b < 100".into(),
        },
        TableConstraint::Unique {
            name: Some("uq".into()),
            columns: vec!["email".into()],
        },
    ];
    let clauses = crate::sql::helpers::extract_check_clauses(&constraints);
    assert_eq!(clauses.len(), 2);
    assert!(clauses[0].contains("chk1"));
    assert!(clauses[1].contains("chk2"));
}
#[test]
fn test_extract_check_clauses_with_no_check_constraints() {
    let constraints = vec![
        TableConstraint::PrimaryKey {
            columns: vec!["id".into()],
            auto_increment: false,
        },
        TableConstraint::Unique {
            name: None,
            columns: vec!["email".into()],
        },
    ];
    let clauses = crate::sql::helpers::extract_check_clauses(&constraints);
    assert!(clauses.is_empty());
}
