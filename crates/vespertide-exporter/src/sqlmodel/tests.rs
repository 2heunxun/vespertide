use super::enums::{to_pascal_case, to_screaming_snake_case};
use super::render_entity;
use super::types::UsedTypes;
use insta::assert_snapshot;
use rstest::rstest;
use vespertide_core::schema::column::{
    ColumnType, ComplexColumnType, EnumValues, NumValue, SimpleColumnType,
};
use vespertide_core::schema::constraint::TableConstraint;
use vespertide_core::{ColumnDef, TableDef};

fn col(name: &str, ty: ColumnType) -> ColumnDef {
    ColumnDef::new(name, ty, false)
}

#[test]
fn test_basic_table() {
    let table = TableDef {
        name: "users".into(),
        description: Some("User accounts table".into()),
        columns: vec![
            ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: Some("Primary key".into()),
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "email".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: false,
                default: None,
                comment: Some("User email address".into()),
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "name".into(),
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
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
            },
            TableConstraint::Unique {
                name: None,
                columns: vec!["email".into()],
            },
        ],
    };

    let result = render_entity(&table).unwrap();
    assert_snapshot!(result);
}

#[test]
fn test_table_with_enum() {
    let table = TableDef {
        name: "orders".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "order_status".into(),
                    values: EnumValues::String(vec![
                        "pending".into(),
                        "shipped".into(),
                        "delivered".into(),
                    ]),
                }),
                nullable: false,
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

    let result = render_entity(&table).unwrap();
    assert_snapshot!(result);
}

#[test]
fn test_table_with_integer_enum() {
    let table = TableDef {
        name: "tasks".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            ColumnDef {
                name: "priority".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "priority_level".into(),
                    values: EnumValues::Integer(vec![
                        NumValue {
                            name: "Low".into(),
                            value: 0,
                        },
                        NumValue {
                            name: "Medium".into(),
                            value: 1,
                        },
                        NumValue {
                            name: "High".into(),
                            value: 2,
                        },
                    ]),
                }),
                nullable: false,
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

    let result = render_entity(&table).unwrap();
    assert_snapshot!(result);
}

#[test]
fn test_table_with_foreign_key() {
    let table = TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("user_id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("title", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            },
        ],
    };

    let result = render_entity(&table).unwrap();
    assert_snapshot!(result);
}

#[test]
fn test_table_with_composite_foreign_key() {
    let table = TableDef {
        name: "line_items".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("order_id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(
                "order_version",
                ColumnType::Simple(SimpleColumnType::Integer),
            ),
            col("sku", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["order_id".into(), "order_version".into()],
                ref_table: "orders".into(),
                ref_columns: vec!["id".into(), "version".into()],
                on_delete: None,
                on_update: None,
            },
        ],
    };

    let result = render_entity(&table).unwrap();
    assert_snapshot!(result);
}

#[test]
fn test_table_with_indexes() {
    let table = TableDef {
        name: "articles".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("title", ColumnType::Simple(SimpleColumnType::Text)),
            col(
                "created_at",
                ColumnType::Simple(SimpleColumnType::Timestamptz),
            ),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            },
            TableConstraint::Index {
                name: Some("idx_articles_created_at".into()),
                columns: vec!["created_at".into()],
            },
            TableConstraint::Index {
                name: None,
                columns: vec!["title".into()],
            },
        ],
    };

    let result = render_entity(&table).unwrap();
    assert_snapshot!(result);
}

#[test]
fn test_table_with_default_values() {
    let table = TableDef {
        name: "settings".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            ColumnDef {
                name: "is_active".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Boolean),
                nullable: false,
                default: Some("true".into()),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "count".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: Some("0".into()),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "created_at".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Timestamptz),
                nullable: false,
                default: Some("now()".into()),
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

    let result = render_entity(&table).unwrap();
    assert_snapshot!(result);
}

#[rstest]
#[case("hello_world", "HelloWorld")]
#[case("user_id", "UserId")]
#[case("simple", "Simple")]
fn test_to_pascal_case(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(to_pascal_case(input), expected);
}

#[rstest]
#[case("pending", "PENDING")]
#[case("inProgress", "IN_PROGRESS")]
#[case("order-status", "ORDER_STATUS")]
fn test_to_screaming_snake_case(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(to_screaming_snake_case(input), expected);
}

#[test]
fn test_all_simple_column_types() {
    let table = TableDef {
        name: "all_types".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("small", ColumnType::Simple(SimpleColumnType::SmallInt)),
            col("big", ColumnType::Simple(SimpleColumnType::BigInt)),
            col("real_num", ColumnType::Simple(SimpleColumnType::Real)),
            col(
                "double_num",
                ColumnType::Simple(SimpleColumnType::DoublePrecision),
            ),
            col("text_col", ColumnType::Simple(SimpleColumnType::Text)),
            col("bool_col", ColumnType::Simple(SimpleColumnType::Boolean)),
            col("date_col", ColumnType::Simple(SimpleColumnType::Date)),
            col("time_col", ColumnType::Simple(SimpleColumnType::Time)),
            col("ts_col", ColumnType::Simple(SimpleColumnType::Timestamp)),
            col(
                "tstz_col",
                ColumnType::Simple(SimpleColumnType::Timestamptz),
            ),
            col(
                "interval_col",
                ColumnType::Simple(SimpleColumnType::Interval),
            ),
            col("bytea_col", ColumnType::Simple(SimpleColumnType::Bytea)),
            col("uuid_col", ColumnType::Simple(SimpleColumnType::Uuid)),
            col("json_col", ColumnType::Simple(SimpleColumnType::Json)),
            col("inet_col", ColumnType::Simple(SimpleColumnType::Inet)),
            col("cidr_col", ColumnType::Simple(SimpleColumnType::Cidr)),
            col("macaddr_col", ColumnType::Simple(SimpleColumnType::Macaddr)),
            col("xml_col", ColumnType::Simple(SimpleColumnType::Xml)),
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
        }],
    };

    let result = render_entity(&table).unwrap();
    // Check Python types are correct
    assert!(result.contains("small: int"));
    assert!(result.contains("big: int"));
    assert!(result.contains("real_num: float"));
    assert!(result.contains("double_num: float"));
    assert!(result.contains("text_col: str"));
    assert!(result.contains("bool_col: bool"));
    assert!(result.contains("date_col: date"));
    assert!(result.contains("time_col: time"));
    assert!(result.contains("ts_col: datetime"));
    assert!(result.contains("tstz_col: datetime"));
    assert!(result.contains("interval_col: str"));
    assert!(result.contains("bytea_col: bytes"));
    assert!(result.contains("uuid_col: UUID"));
    assert!(result.contains("json_col: dict"));
    assert!(result.contains("inet_col: str"));
    assert!(result.contains("cidr_col: str"));
    assert!(result.contains("macaddr_col: str"));
    assert!(result.contains("xml_col: str"));
    assert!(result.contains("from datetime import"));
    assert!(result.contains("date"));
    assert!(result.contains("time"));
    assert!(result.contains("datetime"));
    assert!(result.contains("from uuid import UUID"));
}

#[test]
fn test_complex_column_types() {
    let table = TableDef {
        name: "complex_types".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            ColumnDef {
                name: "varchar_col".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Varchar { length: 100 }),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "char_col".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Char { length: 10 }),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "numeric_col".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Numeric {
                    precision: 10,
                    scale: 2,
                }),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "custom_col".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Custom {
                    custom_type: "CUSTOM_TYPE".into(),
                }),
                nullable: false,
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

    let result = render_entity(&table).unwrap();
    assert!(result.contains("varchar_col: str"));
    assert!(result.contains("char_col: str"));
    assert!(result.contains("numeric_col: Decimal"));
    assert!(result.contains("custom_col: str"));
    assert!(result.contains("from decimal import Decimal"));
}

#[test]
fn test_table_with_composite_index() {
    let table = TableDef {
        name: "composite_index".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("name", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            },
            TableConstraint::Index {
                name: Some("idx_tenant_name".into()),
                columns: vec!["tenant_id".into(), "name".into()],
            },
        ],
    };

    let result = render_entity(&table).unwrap();
    assert!(result.contains("from sqlalchemy import Index"));
    assert!(result.contains("Index(\"idx_tenant_name\""));
}

#[test]
fn test_table_with_composite_unique() {
    let table = TableDef {
        name: "composite_unique".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("name", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            },
            TableConstraint::Unique {
                name: Some("uq_tenant_name".into()),
                columns: vec!["tenant_id".into(), "name".into()],
            },
        ],
    };

    let result = render_entity(&table).unwrap();
    assert!(result.contains("UniqueConstraint"));
    assert!(result.contains("uq_tenant_name"));
}

#[test]
fn test_nullable_enum() {
    let table = TableDef {
        name: "nullable_enum".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "status_type".into(),
                    values: EnumValues::String(vec!["active".into(), "inactive".into()]),
                }),
                nullable: true,
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

    let result = render_entity(&table).unwrap();
    assert!(result.contains("Optional[StatusType]"));
}

#[test]
fn test_string_default_value() {
    let table = TableDef {
        name: "string_defaults".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: false,
                default: Some("'active'".into()),
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

    let result = render_entity(&table).unwrap();
    assert!(result.contains("default=\"active\""));
}

#[test]
fn test_false_boolean_default() {
    let table = TableDef {
        name: "bool_defaults".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            ColumnDef {
                name: "is_deleted".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Boolean),
                nullable: false,
                default: Some("false".into()),
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

    let result = render_entity(&table).unwrap();
    assert!(result.contains("default=False"));
}

#[test]
fn test_unknown_default_as_server_default() {
    let table = TableDef {
        name: "unknown_defaults".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            ColumnDef {
                name: "code".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: false,
                default: Some("gen_code()".into()),
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

    let result = render_entity(&table).unwrap();
    assert!(result.contains("sa_column_kwargs"));
    assert!(result.contains("server_default"));
    assert!(result.contains("gen_code()"));
}

#[test]
fn test_table_without_description() {
    let table = TableDef {
        name: "no_desc".into(),
        description: None,
        columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
        }],
    };

    let result = render_entity(&table).unwrap();
    assert!(result.contains("class NoDesc(SQLModel, table=True):"));
    assert!(!result.contains("\"\"\""));
}

#[test]
fn test_to_pascal_case_empty_segment() {
    assert_eq!(to_pascal_case("a__b"), "AB");
    assert_eq!(to_pascal_case(""), "");
}

#[test]
fn test_no_sqlalchemy_imports_when_not_needed() {
    let table = TableDef {
        name: "simple".into(),
        description: None,
        columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
        }],
    };

    let result = render_entity(&table).unwrap();
    // Should not have sqlalchemy imports for simple tables
    assert!(!result.contains("from sqlalchemy import"));
}

#[test]
fn test_composite_foreign_key_uses_table_args() {
    let table = TableDef {
        name: "composite_fk".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("ref_id1", ColumnType::Simple(SimpleColumnType::Integer)),
            col("ref_id2", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["ref_id1".into(), "ref_id2".into()],
                ref_table: "other".into(),
                ref_columns: vec!["id1".into(), "id2".into()],
                on_delete: None,
                on_update: None,
            },
        ],
    };

    let result = render_entity(&table).unwrap();
    assert!(result.contains("ForeignKeyConstraint"));
    assert!(result.contains(
        "ForeignKeyConstraint([\"ref_id1\", \"ref_id2\"], [\"other.id1\", \"other.id2\"]),"
    ));
    // Composite FK should not generate foreign_key for individual columns.
    assert!(!result.contains("foreign_key=\"other.id1\""));
    assert!(!result.contains("foreign_key=\"other.id2\""));
}

#[test]
fn test_unnamed_composite_index() {
    let table = TableDef {
        name: "unnamed_index".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("col_a", ColumnType::Simple(SimpleColumnType::Integer)),
            col("col_b", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            },
            TableConstraint::Index {
                name: None,
                columns: vec!["col_a".into(), "col_b".into()],
            },
        ],
    };

    let result = render_entity(&table).unwrap();
    assert!(result.contains("Index(None, \"col_a\", \"col_b\"),"));
}

#[test]
fn test_unnamed_composite_unique() {
    let table = TableDef {
        name: "unnamed_unique".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("col_a", ColumnType::Simple(SimpleColumnType::Integer)),
            col("col_b", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            },
            TableConstraint::Unique {
                name: None,
                columns: vec!["col_a".into(), "col_b".into()],
            },
        ],
    };

    let result = render_entity(&table).unwrap();
    assert!(result.contains("UniqueConstraint(\"col_a\", \"col_b\"),"));
}

#[test]
fn test_non_function_unknown_default() {
    // Test default value that's not a function, not a boolean, not a quoted string, not a number
    let table = TableDef {
        name: "unknown_default".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            ColumnDef {
                name: "value".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: false,
                default: Some("SOME_CONSTANT".into()),
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

    let result = render_entity(&table).unwrap();
    // Non-parseable default should be treated as server_default
    assert!(result.contains("sa_column_kwargs"));
    assert!(result.contains("server_default"));
    assert!(result.contains("SOME_CONSTANT"));
}

#[test]
fn test_used_types_date() {
    let mut used = UsedTypes::default();
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Date), false);
    assert!(used.datetime_types.contains("date"));
}

#[test]
fn test_used_types_time() {
    let mut used = UsedTypes::default();
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Time), false);
    assert!(used.datetime_types.contains("time"));
}

#[test]
fn test_used_types_timestamp() {
    let mut used = UsedTypes::default();
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Timestamp), false);
    assert!(used.datetime_types.contains("datetime"));
}

#[test]
fn test_used_types_timestamptz() {
    let mut used = UsedTypes::default();
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Timestamptz), false);
    assert!(used.datetime_types.contains("datetime"));
}

#[test]
fn test_used_types_uuid() {
    let mut used = UsedTypes::default();
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Uuid), false);
    assert!(used.needs_uuid);
}

#[test]
fn test_used_types_other_simple_types_fallthrough() {
    // Test _ => {} branch with types that don't set datetime/uuid
    let mut used = UsedTypes::default();
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Integer), false);
    assert!(used.datetime_types.is_empty());
    assert!(!used.needs_uuid);
    assert!(!used.needs_decimal);
}

#[test]
fn test_used_types_numeric() {
    let mut used = UsedTypes::default();
    used.add_column_type(
        &ColumnType::Complex(ComplexColumnType::Numeric {
            precision: 10,
            scale: 2,
        }),
        false,
    );
    assert!(used.needs_decimal);
}

#[test]
fn test_used_types_varchar_no_decimal() {
    let mut used = UsedTypes::default();
    used.add_column_type(
        &ColumnType::Complex(ComplexColumnType::Varchar { length: 100 }),
        false,
    );
    assert!(!used.needs_decimal);
}

#[test]
fn test_used_types_nullable_sets_optional() {
    let mut used = UsedTypes::default();
    assert!(!used.needs_optional);
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Integer), true);
    assert!(used.needs_optional);
}

#[test]
fn test_json_default_value_escapes_double_quotes() {
    let table = TableDef {
        name: "configs".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            ColumnDef {
                name: "data".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Json),
                nullable: false,
                default: Some(r#"{"hello": "world"}"#.into()),
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

    let result = render_entity(&table).unwrap();
    assert!(
        result.contains(r#"server_default": text("{\"hello\": \"world\"}"#),
        "Expected escaped quotes in server_default, got: {result}"
    );
}
