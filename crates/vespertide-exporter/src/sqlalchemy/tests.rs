use super::render::{to_pascal_case, to_screaming_snake_case};
use super::types::UsedTypes;
use super::*;
use insta::assert_snapshot;
use rstest::rstest;
use vespertide_core::schema::column::NumValue;
use vespertide_core::schema::column::{
    ColumnType, ComplexColumnType, EnumValues, SimpleColumnType,
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
    assert!(result.contains("SmallInteger"));
    assert!(result.contains("BigInteger"));
    assert!(result.contains("Float")); // Real and DoublePrecision
    assert!(result.contains("Boolean"));
    assert!(result.contains("Date"));
    assert!(result.contains("Time"));
    assert!(result.contains("DateTime"));
    assert!(result.contains("Interval"));
    assert!(result.contains("LargeBinary"));
    assert!(result.contains("Uuid"));
    assert!(result.contains("JSON"));
    assert!(result.contains("String(255)")); // Inet, Cidr, Macaddr
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
    assert!(result.contains("String(100)")); // Varchar
    assert!(result.contains("String(10)")); // Char
    assert!(result.contains("Numeric(10, 2)"));
    assert!(result.contains("\"CUSTOM_TYPE\""));
    assert!(result.contains("from decimal import Decimal"));
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
fn test_table_with_server_default() {
    let table = TableDef {
        name: "with_defaults".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
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
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
        }],
    };

    let result = render_entity(&table).unwrap();
    assert!(result.contains("server_default=text(\"now()\")"));
    assert!(result.contains("server_default='active'"));
    assert!(result.contains("server_default=\"0\""));
    assert!(result.contains("from sqlalchemy import")); // Should include text
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
    assert!(result.contains("class NoDesc(DeclarativeBase):"));
    assert!(!result.contains("\"\"\""));
}

#[test]
fn test_to_pascal_case_empty_segment() {
    // Test case with consecutive underscores creating empty segments
    assert_eq!(to_pascal_case("a__b"), "AB");
    assert_eq!(to_pascal_case(""), "");
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
    // Composite FK should not generate ForeignKey() for individual columns.
    assert!(!result.contains("ForeignKey(\"other.id1\")"));
    assert!(!result.contains("ForeignKey(\"other.id2\")"));
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
fn test_used_types_smallint() {
    let mut used = UsedTypes::default();
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::SmallInt), false);
    assert!(used.sa_types.contains("SmallInteger"));
}

#[test]
fn test_used_types_integer() {
    let mut used = UsedTypes::default();
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Integer), false);
    assert!(used.sa_types.contains("Integer"));
}

#[test]
fn test_used_types_bigint() {
    let mut used = UsedTypes::default();
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::BigInt), false);
    assert!(used.sa_types.contains("BigInteger"));
}

#[test]
fn test_used_types_real() {
    let mut used = UsedTypes::default();
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Real), false);
    assert!(used.sa_types.contains("Float"));
}

#[test]
fn test_used_types_double_precision() {
    let mut used = UsedTypes::default();
    used.add_column_type(
        &ColumnType::Simple(SimpleColumnType::DoublePrecision),
        false,
    );
    assert!(used.sa_types.contains("Float"));
}

#[test]
fn test_used_types_text() {
    let mut used = UsedTypes::default();
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Text), false);
    assert!(used.sa_types.contains("Text"));
}

#[test]
fn test_used_types_boolean() {
    let mut used = UsedTypes::default();
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Boolean), false);
    assert!(used.sa_types.contains("Boolean"));
}

#[test]
fn test_used_types_date() {
    let mut used = UsedTypes::default();
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Date), false);
    assert!(used.sa_types.contains("Date"));
    assert!(used.datetime_types.contains("date"));
}

#[test]
fn test_used_types_time() {
    let mut used = UsedTypes::default();
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Time), false);
    assert!(used.sa_types.contains("Time"));
    assert!(used.datetime_types.contains("time"));
}

#[test]
fn test_used_types_timestamp() {
    let mut used = UsedTypes::default();
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Timestamp), false);
    assert!(used.sa_types.contains("DateTime"));
    assert!(used.datetime_types.contains("datetime"));
}

#[test]
fn test_used_types_timestamptz() {
    let mut used = UsedTypes::default();
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Timestamptz), false);
    assert!(used.sa_types.contains("DateTime"));
    assert!(used.datetime_types.contains("datetime"));
}

#[test]
fn test_used_types_interval() {
    let mut used = UsedTypes::default();
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Interval), false);
    assert!(used.sa_types.contains("Interval"));
}

#[test]
fn test_used_types_bytea() {
    let mut used = UsedTypes::default();
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Bytea), false);
    assert!(used.sa_types.contains("LargeBinary"));
}

#[test]
fn test_used_types_uuid() {
    let mut used = UsedTypes::default();
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Uuid), false);
    assert!(used.sa_types.contains("Uuid"));
    assert!(used.needs_uuid);
}

#[test]
fn test_used_types_json() {
    let mut used = UsedTypes::default();
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Json), false);
    assert!(used.sa_types.contains("JSON"));
}

#[test]
fn test_used_types_inet() {
    let mut used = UsedTypes::default();
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Inet), false);
    assert!(used.sa_types.contains("String"));
}

#[test]
fn test_used_types_cidr() {
    let mut used = UsedTypes::default();
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Cidr), false);
    assert!(used.sa_types.contains("String"));
}

#[test]
fn test_used_types_macaddr() {
    let mut used = UsedTypes::default();
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Macaddr), false);
    assert!(used.sa_types.contains("String"));
}

#[test]
fn test_used_types_xml() {
    let mut used = UsedTypes::default();
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Xml), false);
    assert!(used.sa_types.contains("Text"));
}

#[test]
fn test_used_types_varchar() {
    let mut used = UsedTypes::default();
    used.add_column_type(
        &ColumnType::Complex(ComplexColumnType::Varchar { length: 100 }),
        false,
    );
    assert!(used.sa_types.contains("String"));
}

#[test]
fn test_used_types_char() {
    let mut used = UsedTypes::default();
    used.add_column_type(
        &ColumnType::Complex(ComplexColumnType::Char { length: 10 }),
        false,
    );
    assert!(used.sa_types.contains("String"));
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
    assert!(used.sa_types.contains("Numeric"));
    assert!(used.needs_decimal);
}

#[test]
fn test_used_types_custom() {
    let mut used = UsedTypes::default();
    let initial_count = used.sa_types.len();
    used.add_column_type(
        &ColumnType::Complex(ComplexColumnType::Custom {
            custom_type: "FOO".into(),
        }),
        false,
    );
    // Custom type doesn't add any sa_types - verify count unchanged
    assert_eq!(used.sa_types.len(), initial_count);
}

#[test]
fn test_used_types_enum_string() {
    let mut used = UsedTypes::default();
    used.add_column_type(
        &ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["a".into()]),
        }),
        false,
    );
    assert!(used.sa_types.contains("Enum"));
}

#[test]
fn test_used_types_enum_integer() {
    let mut used = UsedTypes::default();
    used.add_column_type(
        &ColumnType::Complex(ComplexColumnType::Enum {
            name: "priority".into(),
            values: EnumValues::Integer(vec![NumValue {
                name: "Low".into(),
                value: 0,
            }]),
        }),
        false,
    );
    assert!(used.sa_types.contains("Integer"));
}

#[test]
fn test_used_types_nullable_sets_optional() {
    let mut used = UsedTypes::default();
    assert!(!used.needs_optional);
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Integer), true);
    assert!(used.needs_optional);
}

/// Comprehensive test that exercises ALL branches of `add_column_type` in a single test.
/// This ensures tarpaulin sees all branches as covered.
#[test]
fn test_used_types_all_branches_comprehensive() {
    let mut used = UsedTypes::default();

    // Simple types - each branch
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::SmallInt), false);
    assert!(used.sa_types.contains("SmallInteger"));

    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Integer), false);
    assert!(used.sa_types.contains("Integer"));

    used.add_column_type(&ColumnType::Simple(SimpleColumnType::BigInt), false);
    assert!(used.sa_types.contains("BigInteger"));

    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Real), false);
    assert!(used.sa_types.contains("Float"));

    used.add_column_type(
        &ColumnType::Simple(SimpleColumnType::DoublePrecision),
        false,
    );
    // Float already added by Real

    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Text), false);
    assert!(used.sa_types.contains("Text"));

    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Boolean), false);
    assert!(used.sa_types.contains("Boolean"));

    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Date), false);
    assert!(used.sa_types.contains("Date"));
    assert!(used.datetime_types.contains("date"));

    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Time), false);
    assert!(used.sa_types.contains("Time"));
    assert!(used.datetime_types.contains("time"));

    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Timestamp), false);
    assert!(used.sa_types.contains("DateTime"));
    assert!(used.datetime_types.contains("datetime"));

    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Timestamptz), false);
    // DateTime already added

    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Interval), false);
    assert!(used.sa_types.contains("Interval"));

    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Bytea), false);
    assert!(used.sa_types.contains("LargeBinary"));

    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Uuid), false);
    assert!(used.sa_types.contains("Uuid"));
    assert!(used.needs_uuid);

    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Json), false);
    assert!(used.sa_types.contains("JSON"));

    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Inet), false);
    assert!(used.sa_types.contains("String"));

    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Cidr), false);
    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Macaddr), false);

    used.add_column_type(&ColumnType::Simple(SimpleColumnType::Xml), false);
    // Text already added

    // Complex types
    used.add_column_type(
        &ColumnType::Complex(ComplexColumnType::Varchar { length: 100 }),
        false,
    );
    used.add_column_type(
        &ColumnType::Complex(ComplexColumnType::Char { length: 10 }),
        false,
    );

    used.add_column_type(
        &ColumnType::Complex(ComplexColumnType::Numeric {
            precision: 10,
            scale: 2,
        }),
        false,
    );
    assert!(used.sa_types.contains("Numeric"));
    assert!(used.needs_decimal);

    used.add_column_type(
        &ColumnType::Complex(ComplexColumnType::Custom {
            custom_type: "FOO".into(),
        }),
        false,
    );
    // Custom doesn't add any type

    used.add_column_type(
        &ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["a".into()]),
        }),
        false,
    );
    assert!(used.sa_types.contains("Enum"));

    used.add_column_type(
        &ColumnType::Complex(ComplexColumnType::Enum {
            name: "priority".into(),
            values: EnumValues::Integer(vec![NumValue {
                name: "Low".into(),
                value: 0,
            }]),
        }),
        false,
    );
    // Integer already added

    // Test nullable
    let mut used2 = UsedTypes::default();
    assert!(!used2.needs_optional);
    used2.add_column_type(&ColumnType::Simple(SimpleColumnType::Integer), true);
    assert!(used2.needs_optional);
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
        result.contains(r#"server_default="{\"hello\": \"world\"}"#),
        "Expected escaped quotes in server_default, got: {result}"
    );
}
