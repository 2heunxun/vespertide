use super::render::{infer_fk_field_name, to_camel_case, to_pascal_case};
use super::types::column_type_to_java;
use super::*;
use insta::assert_snapshot;
use rstest::rstest;
use vespertide_core::schema::column::{
    ColumnType, ComplexColumnType, EnumValues, SimpleColumnType,
};
use vespertide_core::schema::constraint::TableConstraint;
use vespertide_core::{ColumnDef, TableDef};
use vespertide_core::{DefaultValue, NumValue};

fn col(name: &str, ty: ColumnType) -> ColumnDef {
    ColumnDef {
        name: name.to_string(),
        r#type: ty,
        nullable: false,
        default: None,
        comment: None,
        primary_key: None,
        unique: None,
        index: None,
        foreign_key: None,
    }
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
            auto_increment: true,
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
                            name: "low".into(),
                            value: 0,
                        },
                        NumValue {
                            name: "medium".into(),
                            value: 10,
                        },
                        NumValue {
                            name: "high".into(),
                            value: 20,
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
            ColumnDef {
                name: "author_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            col("title", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["author_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            },
            TableConstraint::Index {
                name: Some("ix_posts__author_id".into()),
                columns: vec!["author_id".into()],
            },
        ],
    };

    let result = render_entity(&table).unwrap();
    assert_snapshot!(result);
}

#[test]
fn test_table_with_all_simple_types() {
    let table = TableDef {
        name: "type_test".into(),
        description: None,
        columns: vec![
            col(
                "col_smallint",
                ColumnType::Simple(SimpleColumnType::SmallInt),
            ),
            col("col_integer", ColumnType::Simple(SimpleColumnType::Integer)),
            col("col_bigint", ColumnType::Simple(SimpleColumnType::BigInt)),
            col("col_real", ColumnType::Simple(SimpleColumnType::Real)),
            col(
                "col_double",
                ColumnType::Simple(SimpleColumnType::DoublePrecision),
            ),
            col("col_text", ColumnType::Simple(SimpleColumnType::Text)),
            col("col_boolean", ColumnType::Simple(SimpleColumnType::Boolean)),
            col("col_date", ColumnType::Simple(SimpleColumnType::Date)),
            col("col_time", ColumnType::Simple(SimpleColumnType::Time)),
            col(
                "col_timestamp",
                ColumnType::Simple(SimpleColumnType::Timestamp),
            ),
            col(
                "col_timestamptz",
                ColumnType::Simple(SimpleColumnType::Timestamptz),
            ),
            col(
                "col_interval",
                ColumnType::Simple(SimpleColumnType::Interval),
            ),
            col("col_bytea", ColumnType::Simple(SimpleColumnType::Bytea)),
            col("col_uuid", ColumnType::Simple(SimpleColumnType::Uuid)),
            col("col_json", ColumnType::Simple(SimpleColumnType::Json)),
            col("col_inet", ColumnType::Simple(SimpleColumnType::Inet)),
            col("col_cidr", ColumnType::Simple(SimpleColumnType::Cidr)),
            col("col_macaddr", ColumnType::Simple(SimpleColumnType::Macaddr)),
            col("col_xml", ColumnType::Simple(SimpleColumnType::Xml)),
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["col_integer".into()],
        }],
    };

    let result = render_entity(&table).unwrap();
    assert_snapshot!(result);
}

#[test]
fn test_table_with_complex_types() {
    let table = TableDef {
        name: "products".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(
                "name",
                ColumnType::Complex(ComplexColumnType::Varchar { length: 200 }),
            ),
            col(
                "price",
                ColumnType::Complex(ComplexColumnType::Numeric {
                    precision: 12,
                    scale: 2,
                }),
            ),
            col(
                "code",
                ColumnType::Complex(ComplexColumnType::Char { length: 10 }),
            ),
            col(
                "metadata",
                ColumnType::Complex(ComplexColumnType::Custom {
                    custom_type: "JSONB".into(),
                }),
            ),
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
        }],
    };

    let result = render_entity(&table).unwrap();
    assert_snapshot!(result);
}

#[test]
fn test_table_with_defaults() {
    let table = TableDef {
        name: "articles".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            ColumnDef {
                name: "published".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Boolean),
                nullable: false,
                default: Some(DefaultValue::Bool(false)),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "view_count".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: Some(DefaultValue::Integer(0)),
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
                default: Some(DefaultValue::String("'draft'".into())),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
        }],
    };

    let result = render_entity(&table).unwrap();
    assert_snapshot!(result);
}

#[test]
fn test_table_with_composite_constraints() {
    let table = TableDef {
        name: "order_items".into(),
        description: None,
        columns: vec![
            col("order_id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("product_id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("quantity", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["order_id".into(), "product_id".into()],
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["order_id".into()],
                ref_table: "orders".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["product_id".into()],
                ref_table: "products".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            },
            TableConstraint::Unique {
                name: Some("uq_order_items__order_product".into()),
                columns: vec!["order_id".into(), "product_id".into()],
            },
            TableConstraint::Index {
                name: Some("ix_order_items__order_id".into()),
                columns: vec!["order_id".into()],
            },
        ],
    };

    let result = render_entity(&table).unwrap();
    assert_snapshot!(result);
}

#[test]
fn test_nullable_columns() {
    let table = TableDef {
        name: "profiles".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            ColumnDef {
                name: "bio".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "avatar_url".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Varchar { length: 500 }),
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
            auto_increment: true,
            columns: vec!["id".into()],
        }],
    };

    let result = render_entity(&table).unwrap();
    assert_snapshot!(result);
}

#[test]
fn test_unnamed_index_and_unique() {
    let table = TableDef {
        name: "events".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("venue_id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("date", ColumnType::Simple(SimpleColumnType::Date)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            },
            TableConstraint::Index {
                name: None,
                columns: vec!["venue_id".into(), "date".into()],
            },
            TableConstraint::Unique {
                name: None,
                columns: vec!["venue_id".into(), "date".into()],
            },
        ],
    };

    let result = render_entity(&table).unwrap();
    assert_snapshot!(result);
}

#[test]
fn test_fk_with_comment_and_auto_increment() {
    let table = TableDef {
        name: "child".into(),
        description: None,
        columns: vec![
            ColumnDef {
                name: "parent_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: Some("References parent table".into()),
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            col("value", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["parent_id".into()],
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["parent_id".into()],
                ref_table: "parent".into(),
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
fn test_server_default_and_true_boolean() {
    let table = TableDef {
        name: "logs".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            ColumnDef {
                name: "active".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Boolean),
                nullable: false,
                default: Some(DefaultValue::Bool(true)),
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
                default: Some(DefaultValue::String("NOW()".into())),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "score".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Real),
                nullable: false,
                default: Some(DefaultValue::Float(1.5)),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "tag".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: false,
                default: Some(DefaultValue::String("UNKNOWN_EXPR".into())),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
        }],
    };

    let result = render_entity(&table).unwrap();
    assert_snapshot!(result);
}

#[test]
fn test_column_type_to_java_string_enum() {
    // Exercises the string enum branch in column_type_to_java
    let ty = ColumnType::Complex(ComplexColumnType::Enum {
        name: "status".into(),
        values: EnumValues::String(vec!["a".into()]),
    });
    assert_eq!(column_type_to_java(&ty), "String");
}

#[rstest]
#[case("order_item", "OrderItem")]
#[case("users", "Users")]
#[case("a", "A")]
#[case("user_profile_image", "UserProfileImage")]
#[case("", "")]
fn test_to_pascal_case(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(to_pascal_case(input), expected);
}

#[rstest]
#[case("created_at", "createdAt")]
#[case("id", "id")]
#[case("user_profile_image", "userProfileImage")]
#[case("", "")]
fn test_to_camel_case(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(to_camel_case(input), expected);
}

#[rstest]
#[case("customer_id", "customer")]
#[case("author_user_id", "authorUser")]
#[case("parent", "parent")]
fn test_infer_fk_field_name(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(infer_fk_field_name(input), expected);
}
