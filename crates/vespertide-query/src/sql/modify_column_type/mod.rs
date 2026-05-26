mod direct;
mod sqlite_rebuild;

use std::collections::BTreeMap;

use sea_query::{Alias, Expr, Query};

use vespertide_core::{ColumnType, TableDef};

use self::direct::build_modify_column_type_direct;
use self::sqlite_rebuild::build_modify_column_type_sqlite_temp_table;
use super::types::{BuiltQuery, DatabaseBackend};
use crate::error::QueryError;

/// Build UPDATE statements for `fill_with` mappings (removed enum values → replacement values).
/// Each entry generates: UPDATE "table" SET "column" = 'replacement' WHERE "column" = '`removed_value`'
fn build_fill_with_updates(
    table: &str,
    column: &str,
    fill_with: &BTreeMap<String, String>,
) -> Vec<BuiltQuery> {
    fill_with
        .iter()
        .map(|(removed_value, replacement)| {
            let update_stmt = Query::update()
                .table(Alias::new(table))
                .value(Alias::new(column), Expr::val(replacement.as_str()))
                .and_where(Expr::col(Alias::new(column)).eq(removed_value.as_str()))
                .to_owned();
            BuiltQuery::Update(Box::new(update_stmt))
        })
        .collect()
}

pub fn build_modify_column_type(
    backend: DatabaseBackend,
    table: &str,
    column: &str,
    new_type: &ColumnType,
    fill_with: Option<&BTreeMap<String, String>>,
    current_schema: &[TableDef],
    pending_constraints: &[vespertide_core::TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    // SQLite does not support direct column type modification, so use temporary table approach
    if backend == DatabaseBackend::Sqlite {
        return build_modify_column_type_sqlite_temp_table(
            backend,
            table,
            column,
            new_type,
            fill_with,
            current_schema,
            pending_constraints,
        );
    }

    // PostgreSQL, MySQL, etc. can use ALTER TABLE directly
    Ok(build_modify_column_type_direct(
        backend,
        table,
        column,
        new_type,
        fill_with,
        current_schema,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;
    use vespertide_core::{
        ColumnDef, ColumnType, ComplexColumnType, EnumValues, SimpleColumnType, TableDef,
    };

    #[rstest]
    #[case::modify_column_type_postgres(
        "modify_column_type_postgres",
        DatabaseBackend::Postgres,
        &["ALTER TABLE \"users\"", "\"age\""]
    )]
    #[case::modify_column_type_mysql(
        "modify_column_type_mysql",
        DatabaseBackend::MySql,
        &["ALTER TABLE `users` MODIFY COLUMN `age` varchar(50)"]
    )]
    #[case::modify_column_type_sqlite(
        "modify_column_type_sqlite",
        DatabaseBackend::Sqlite,
        &[]
    )]
    fn test_modify_column_type(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
        #[case] expected: &[&str],
    ) {
        // For SQLite, we need to provide current schema
        let current_schema = vec![TableDef {
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
                    name: "age".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: true,
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

        let result = build_modify_column_type(
            backend,
            "users",
            "age",
            &ColumnType::Complex(ComplexColumnType::Varchar { length: 50 }),
            None,
            &current_schema,
            &[],
        );

        // SQLite may return multiple queries
        let sql = result
            .unwrap()
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<_>>()
            .join(";\n");

        for exp in expected {
            assert!(
                sql.contains(exp),
                "Expected SQL to contain '{exp}', got: {sql}"
            );
        }
        println!("sql: {sql}");

        with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("modify_column_type_{}", title) }, {
            assert_snapshot!(sql);
        });
    }

    #[test]
    fn test_modify_column_type_table_not_found() {
        let result = build_modify_column_type(
            DatabaseBackend::Sqlite,
            "nonexistent_table",
            "age",
            &ColumnType::Simple(SimpleColumnType::BigInt),
            None,
            &[],
            &[],
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Table 'nonexistent_table' not found")
        );
    }

    #[test]
    fn test_modify_column_type_column_not_found() {
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
            constraints: vec![],
        }];
        let result = build_modify_column_type(
            DatabaseBackend::Sqlite,
            "users",
            "nonexistent_column",
            &ColumnType::Simple(SimpleColumnType::BigInt),
            None,
            &current_schema,
            &[],
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Column 'nonexistent_column' not found")
        );
    }

    #[rstest]
    #[case::modify_column_type_with_index_postgres(
        "modify_column_type_with_index_postgres",
        DatabaseBackend::Postgres
    )]
    #[case::modify_column_type_with_index_mysql(
        "modify_column_type_with_index_mysql",
        DatabaseBackend::MySql
    )]
    #[case::modify_column_type_with_index_sqlite(
        "modify_column_type_with_index_sqlite",
        DatabaseBackend::Sqlite
    )]
    fn test_modify_column_type_with_index(#[case] title: &str, #[case] backend: DatabaseBackend) {
        // Test modify column type with indexes
        use vespertide_core::TableConstraint;

        let current_schema = vec![TableDef {
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
                    name: "age".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![TableConstraint::Index {
                name: Some("idx_age".into()),
                columns: vec!["age".into()],
            }],
        }];

        let result = build_modify_column_type(
            backend,
            "users",
            "age",
            &ColumnType::Simple(SimpleColumnType::BigInt),
            None,
            &current_schema,
            &[],
        )
        .unwrap();

        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<_>>()
            .join(";\n");

        // For SQLite, should recreate index
        if matches!(backend, DatabaseBackend::Sqlite) {
            assert!(sql.contains("CREATE INDEX"));
            assert!(sql.contains("idx_age"));
        }

        with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("modify_column_type_with_index_{}", title) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::modify_column_type_with_unique_constraint_postgres(
        "modify_column_type_with_unique_constraint_postgres",
        DatabaseBackend::Postgres
    )]
    #[case::modify_column_type_with_unique_constraint_mysql(
        "modify_column_type_with_unique_constraint_mysql",
        DatabaseBackend::MySql
    )]
    #[case::modify_column_type_with_unique_constraint_sqlite(
        "modify_column_type_with_unique_constraint_sqlite",
        DatabaseBackend::Sqlite
    )]
    fn test_modify_column_type_with_unique_constraint(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
    ) {
        // Test modify column type with unique constraint
        use vespertide_core::TableConstraint;

        let current_schema = vec![TableDef {
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
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![TableConstraint::Unique {
                name: Some("uq_email".into()),
                columns: vec!["email".into()],
            }],
        }];

        let result = build_modify_column_type(
            backend,
            "users",
            "email",
            &ColumnType::Complex(ComplexColumnType::Varchar { length: 255 }),
            None,
            &current_schema,
            &[],
        )
        .unwrap();

        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<_>>()
            .join(";\n");

        // For SQLite, unique constraint should be in CREATE TABLE statement
        if matches!(backend, DatabaseBackend::Sqlite) {
            assert!(sql.contains("CREATE TABLE"));
        }

        with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("modify_column_type_with_unique_constraint_{}", title) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::enum_values_changed_postgres(
        "enum_values_changed_postgres",
        DatabaseBackend::Postgres,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into(), "pending".into()]),
        })
    )]
    #[case::enum_values_changed_mysql(
        "enum_values_changed_mysql",
        DatabaseBackend::MySql,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into(), "pending".into()]),
        })
    )]
    #[case::enum_values_changed_sqlite(
        "enum_values_changed_sqlite",
        DatabaseBackend::Sqlite,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into(), "pending".into()]),
        })
    )]
    #[case::enum_same_values_postgres(
        "enum_same_values_postgres",
        DatabaseBackend::Postgres,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::enum_same_values_mysql(
        "enum_same_values_mysql",
        DatabaseBackend::MySql,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::enum_same_values_sqlite(
        "enum_same_values_sqlite",
        DatabaseBackend::Sqlite,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::enum_name_changed_postgres(
        "enum_name_changed_postgres",
        DatabaseBackend::Postgres,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "old_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "new_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::enum_name_changed_mysql(
        "enum_name_changed_mysql",
        DatabaseBackend::MySql,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "old_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "new_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::enum_name_changed_sqlite(
        "enum_name_changed_sqlite",
        DatabaseBackend::Sqlite,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "old_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "new_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::text_to_enum_postgres(
        "text_to_enum_postgres",
        DatabaseBackend::Postgres,
        ColumnType::Simple(SimpleColumnType::Text),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "user_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::text_to_enum_mysql(
        "text_to_enum_mysql",
        DatabaseBackend::MySql,
        ColumnType::Simple(SimpleColumnType::Text),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "user_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::text_to_enum_sqlite(
        "text_to_enum_sqlite",
        DatabaseBackend::Sqlite,
        ColumnType::Simple(SimpleColumnType::Text),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "user_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::enum_to_text_postgres(
        "enum_to_text_postgres",
        DatabaseBackend::Postgres,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "user_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Simple(SimpleColumnType::Text)
    )]
    #[case::enum_to_text_mysql(
        "enum_to_text_mysql",
        DatabaseBackend::MySql,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "user_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Simple(SimpleColumnType::Text)
    )]
    #[case::enum_to_text_sqlite(
        "enum_to_text_sqlite",
        DatabaseBackend::Sqlite,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "user_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Simple(SimpleColumnType::Text)
    )]
    fn test_modify_enum_types(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
        #[case] old_type: ColumnType,
        #[case] new_type: ColumnType,
    ) {
        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "status".into(),
                r#type: old_type,
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
        }];

        let result = build_modify_column_type(
            backend,
            "users",
            "status",
            &new_type,
            None,
            &current_schema,
            &[],
        )
        .unwrap();

        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<_>>()
            .join(";\n");

        with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("modify_enum_types_{}", title) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::modify_enum_with_default_postgres(
        "modify_enum_with_default_postgres",
        DatabaseBackend::Postgres
    )]
    #[case::modify_enum_with_default_mysql(
        "modify_enum_with_default_mysql",
        DatabaseBackend::MySql
    )]
    #[case::modify_enum_with_default_sqlite(
        "modify_enum_with_default_sqlite",
        DatabaseBackend::Sqlite
    )]
    fn test_modify_enum_with_default_value(#[case] title: &str, #[case] backend: DatabaseBackend) {
        // Test that enum type change handles DEFAULT values correctly
        // PostgreSQL requires: DROP DEFAULT -> change type -> SET DEFAULT
        let current_schema = vec![TableDef {
            name: "reservation_session".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "session_status".into(),
                    values: EnumValues::String(vec!["pending".into(), "confirmed".into()]),
                }),
                nullable: false,
                default: Some("'pending'".into()),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
        }];

        let new_type = ColumnType::Complex(ComplexColumnType::Enum {
            name: "session_status".into(),
            values: EnumValues::String(vec![
                "pending".into(),
                "confirmed".into(),
                "cancelled".into(),
            ]),
        });

        let result = build_modify_column_type(
            backend,
            "reservation_session",
            "status",
            &new_type,
            None,
            &current_schema,
            &[],
        )
        .unwrap();

        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<_>>()
            .join(";\n");

        // PostgreSQL-specific: verify DROP DEFAULT -> TYPE change -> SET DEFAULT order
        if matches!(backend, DatabaseBackend::Postgres) {
            assert!(
                sql.contains("DROP DEFAULT"),
                "Should drop default before type change. SQL: {sql}"
            );
            assert!(
                sql.contains("SET DEFAULT"),
                "Should restore default after type change. SQL: {sql}"
            );

            let drop_default_pos = sql.find("DROP DEFAULT").unwrap();
            let type_change_pos = sql.find("USING").unwrap();
            let set_default_pos = sql.find("SET DEFAULT").unwrap();

            assert!(
                drop_default_pos < type_change_pos,
                "DROP DEFAULT should come before TYPE change"
            );
            assert!(
                type_change_pos < set_default_pos,
                "SET DEFAULT should come after TYPE change"
            );
        }

        with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("modify_enum_with_default_{}", title) }, {
            assert_snapshot!(sql);
        });
    }

    #[test]
    fn test_modify_column_type_to_enum_with_empty_schema() {
        // Test the None branch in line 195-200
        // When current_schema is empty, old_type will be None
        use vespertide_core::ComplexColumnType;

        let result = build_modify_column_type(
            DatabaseBackend::Postgres,
            "users",
            "status",
            &ColumnType::Complex(ComplexColumnType::Enum {
                name: "status_type".into(),
                values: EnumValues::String(vec!["active".into(), "inactive".into()]),
            }),
            None,
            &[], // Empty schema - old_type will be None
            &[],
        );

        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(DatabaseBackend::Postgres))
            .collect::<Vec<String>>()
            .join(";\n");

        // Should create the enum type since old_type is None
        assert!(sql.contains("CREATE TYPE"));
        assert!(sql.contains("status_type"));
        assert!(sql.contains("ALTER TABLE"));
    }

    #[rstest]
    #[case::fill_with_enum_change_postgres(
        "fill_with_enum_change_postgres",
        DatabaseBackend::Postgres,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into(), "banned".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::fill_with_enum_change_sqlite(
        "fill_with_enum_change_sqlite",
        DatabaseBackend::Sqlite,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into(), "banned".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::fill_with_enum_change_mysql(
        "fill_with_enum_change_mysql",
        DatabaseBackend::MySql,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into(), "banned".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    fn test_modify_column_type_with_fill_with(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
        #[case] old_type: ColumnType,
        #[case] new_type: ColumnType,
    ) {
        let mut fill_with_map = std::collections::BTreeMap::new();
        fill_with_map.insert("banned".to_string(), "inactive".to_string());

        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "status".into(),
                r#type: old_type,
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
        }];

        let result = build_modify_column_type(
            backend,
            "users",
            "status",
            &new_type,
            Some(&fill_with_map),
            &current_schema,
            &[],
        )
        .unwrap();

        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<_>>()
            .join(";\n");

        // All backends should include the UPDATE statement for fill_with
        assert!(
            sql.contains("UPDATE"),
            "Expected UPDATE for fill_with mapping, got: {sql}"
        );

        with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("modify_column_type_with_fill_with_{}", title) }, {
            assert_snapshot!(sql);
        });
    }
}
