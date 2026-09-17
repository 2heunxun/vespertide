use std::collections::HashMap;

use rstest::rstest;
use vespertide_core::schema::column::{
    ColumnType, ComplexColumnType, EnumValues, SimpleColumnType,
};
use vespertide_core::schema::constraint::TableConstraint;
use vespertide_core::{ColumnDef, ReferenceAction, TableDef};

use super::render::{go_relation_field_name, to_go_field_name};
use super::types::go_type_for_column_mapped;
use super::{render_entity, render_entity_with_schema};

mod relations;

fn col(name: &str, ty: ColumnType) -> ColumnDef {
    ColumnDef {
        name: name.into(),
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

// -----------------------------------------------------------------------
// Type mapping unit tests
// -----------------------------------------------------------------------

#[rstest]
#[case(ColumnType::Simple(SimpleColumnType::SmallInt), false, "int16")]
#[case(ColumnType::Simple(SimpleColumnType::Integer), false, "int32")]
#[case(ColumnType::Simple(SimpleColumnType::BigInt), false, "int64")]
#[case(ColumnType::Simple(SimpleColumnType::Real), false, "float32")]
#[case(
    ColumnType::Simple(SimpleColumnType::DoublePrecision),
    false,
    "float64"
)]
#[case(ColumnType::Simple(SimpleColumnType::Text), false, "string")]
#[case(ColumnType::Simple(SimpleColumnType::Boolean), false, "bool")]
#[case(ColumnType::Simple(SimpleColumnType::Timestamp), false, "time.Time")]
#[case(ColumnType::Simple(SimpleColumnType::Timestamptz), false, "time.Time")]
#[case(ColumnType::Simple(SimpleColumnType::Date), false, "time.Time")]
#[case(ColumnType::Simple(SimpleColumnType::Time), false, "time.Time")]
#[case(ColumnType::Simple(SimpleColumnType::Uuid), false, "uuid.UUID")]
#[case(ColumnType::Simple(SimpleColumnType::Json), false, "datatypes.JSON")]
#[case(ColumnType::Simple(SimpleColumnType::Bytea), false, "[]byte")]
#[case(ColumnType::Simple(SimpleColumnType::Inet), false, "string")]
#[case(ColumnType::Complex(ComplexColumnType::Varchar { length: 255 }), false, "string")]
#[case(ColumnType::Complex(ComplexColumnType::Numeric { precision: 10, scale: 2 }), false, "decimal.Decimal")]
#[case(ColumnType::Complex(ComplexColumnType::Custom { custom_type: "JSONB".into() }), false, "datatypes.JSON")]
#[case(ColumnType::Complex(ComplexColumnType::Custom { custom_type: "jsonb".into() }), false, "datatypes.JSON")]
#[case(ColumnType::Complex(ComplexColumnType::Custom { custom_type: "TEXT".into() }), false, "string")]
#[case(ColumnType::Simple(SimpleColumnType::Integer), true, "*int32")]
#[case(ColumnType::Simple(SimpleColumnType::Text), true, "*string")]
#[case(ColumnType::Simple(SimpleColumnType::Timestamp), true, "*time.Time")]
fn test_go_type_mapping(
    #[case] col_type: ColumnType,
    #[case] nullable: bool,
    #[case] expected: &str,
) {
    assert_eq!(
        go_type_for_column_mapped(&col_type, nullable, &HashMap::new()),
        expected
    );
}

#[rstest]
#[case("user_id", "UserID")]
#[case("id", "ID")]
#[case("created_at", "CreatedAt")]
#[case("profile_image", "ProfileImage")]
#[case("media_id", "MediaID")]
#[case("identity", "Identity")]
#[case("idx", "Idx")]
#[case("1st_place", "X1stPlace")]
fn test_to_go_field_name(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(to_go_field_name(input), expected);
}

#[rstest]
#[case("user_id", "User")]
#[case("author_id", "Author")]
#[case("parent_id", "Parent")]
#[case("node", "Node")]
fn test_go_relation_field_name(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(go_relation_field_name(input), expected);
}

// -----------------------------------------------------------------------
// Conflicting enum names across tables → qualified Go type name
// -----------------------------------------------------------------------

#[test]
fn test_conflicting_enum_names_qualified() {
    let orders = TableDef {
        name: "orders".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "status".into(),
                    values: EnumValues::String(vec!["pending".into(), "done".into()]),
                }),
            ),
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };
    let tasks = TableDef {
        name: "tasks".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "status".into(),
                    values: EnumValues::String(vec!["open".into(), "closed".into()]),
                }),
            ),
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };
    let schema = vec![orders.clone(), tasks];
    let result = render_entity_with_schema(&orders, &schema).unwrap();
    assert!(
        result.contains("OrdersStatus"),
        "Expected qualified enum name 'OrdersStatus' in:\n{result}"
    );
}

// -----------------------------------------------------------------------
// Char column → type:char + size in GORM tag
// -----------------------------------------------------------------------

#[test]
fn test_char_type_column() {
    let table = TableDef {
        name: "codes".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(
                "code",
                ColumnType::Complex(ComplexColumnType::Char { length: 3 }),
            ),
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };
    let result = render_entity(&table).unwrap();
    assert!(
        result.contains("type:char"),
        "Expected type:char in GORM tag"
    );
    assert!(result.contains("size:3"), "Expected size:3 in GORM tag");
}

// -----------------------------------------------------------------------
// FK field name collision: infer == go_field → disambiguate with ref struct
// -----------------------------------------------------------------------

#[test]
fn test_fk_relation_field_name_collision() {
    // Column "user" (no _id suffix): infer→"User", go_field→"User" → same → "UserUsers"
    let table = TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("user", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    };
    let result = render_entity(&table).unwrap();
    assert!(
        result.contains("UserUsers"),
        "Expected disambiguated relation name 'UserUsers' in:\n{result}"
    );
}

// -----------------------------------------------------------------------
// Reverse relation disambiguation: two FKs to same target → ByField suffix
// -----------------------------------------------------------------------

#[test]
fn test_reverse_relation_disambiguation() {
    let users = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };
    let events = TableDef {
        name: "events".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("creator_id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("attendee_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["creator_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["attendee_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    };
    let schema = vec![users.clone(), events];
    let result = render_entity_with_schema(&users, &schema).unwrap();
    assert!(
        result.contains("EventsByCreatorID"),
        "Expected 'EventsByCreatorID' in:\n{result}"
    );
    assert!(
        result.contains("EventsByAttendeeID"),
        "Expected 'EventsByAttendeeID' in:\n{result}"
    );
}

// -----------------------------------------------------------------------
// A numeric column renders decimal.Decimal and pulls in the decimal import.
// -----------------------------------------------------------------------

#[test]
fn test_numeric_column_gorm() {
    let table = TableDef {
        name: "prices".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(
                "amount",
                ColumnType::Complex(ComplexColumnType::Numeric {
                    precision: 10,
                    scale: 2,
                }),
            ),
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };
    let result = render_entity(&table).unwrap();
    assert!(
        result.contains("type:numeric(10,2)"),
        "expected numeric GORM tag"
    );
    assert!(
        result.contains("decimal.Decimal"),
        "expected decimal.Decimal type"
    );
    assert!(
        result.contains("github.com/shopspring/decimal"),
        "expected decimal import"
    );
}

// -----------------------------------------------------------------------
// An unnamed index renders a bare `index` tag.
// -----------------------------------------------------------------------

#[test]
fn test_unnamed_index_gorm() {
    let table = TableDef {
        name: "searches".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("query", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::Index {
                name: None,
                columns: vec!["query".into()],
            },
        ],
    };
    let result = render_entity(&table).unwrap();
    assert!(result.contains(";index\""), "expected unnamed index tag");
}

// -----------------------------------------------------------------------
// A named index carries its name in the tag.
// -----------------------------------------------------------------------

#[test]
fn test_named_index_gorm() {
    let table = TableDef {
        name: "searches".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("query", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::Index {
                name: Some("ix_searches__query".into()),
                columns: vec!["query".into()],
            },
        ],
    };
    let result = render_entity(&table).unwrap();
    assert!(
        result.contains("index:ix_searches__query"),
        "expected named index tag"
    );
}

// -----------------------------------------------------------------------
// An unnamed composite unique is named after its columns.
// -----------------------------------------------------------------------

#[test]
fn test_unnamed_composite_unique_gorm() {
    let table = TableDef {
        name: "order_lines".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("order_id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(
                "sku",
                ColumnType::Complex(ComplexColumnType::Varchar { length: 50 }),
            ),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::Unique {
                name: None,
                columns: vec!["order_id".into(), "sku".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            },
        ],
    };
    let result = render_entity(&table).unwrap();
    assert!(
        result.contains("uniqueIndex:uq_order_id_sku"),
        "expected auto-generated uniqueIndex name"
    );
}

// -----------------------------------------------------------------------
// A reverse relation from a singular table name renders a pluralized field.
// -----------------------------------------------------------------------

#[test]
fn test_singular_source_table_name() {
    let user = TableDef {
        name: "user".into(),
        description: None,
        columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };
    let comment = TableDef {
        name: "comment".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("user_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user_id".into()],
                ref_table: "user".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    };
    let schema = vec![user.clone(), comment];
    let result = render_entity_with_schema(&user, &schema).unwrap();
    // "comment" → pascal "Comment" → doesn't end with 's' → appended 's' → "Comments"
    assert!(
        result.contains("Comments"),
        "expected 'Comments' plural for singular 'comment' table"
    );
}

// -----------------------------------------------------------------------
// A nullable FK with ON UPDATE renders a pointer relation field carrying both actions.
// -----------------------------------------------------------------------

#[test]
fn test_fk_with_on_update_and_nullable() {
    let posts = TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            ColumnDef {
                name: "author_id".into(),
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
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["author_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some(ReferenceAction::Cascade),
                on_update: Some(ReferenceAction::Restrict),
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    };
    let result = render_entity(&posts).unwrap();
    assert!(
        result.contains("OnUpdate:RESTRICT"),
        "expected OnUpdate constraint"
    );
    assert!(
        result.contains("*Users"),
        "expected nullable FK pointer type"
    );
}

// -----------------------------------------------------------------------
// FK on_delete keywords: SetNull, SetDefault, NoAction
// -----------------------------------------------------------------------

#[rstest]
#[case(ReferenceAction::SetNull, "SET NULL")]
#[case(ReferenceAction::SetDefault, "SET DEFAULT")]
#[case(ReferenceAction::NoAction, "NO ACTION")]
fn test_gorm_fk_on_delete_actions(#[case] action: ReferenceAction, #[case] expected: &str) {
    let table = TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("author_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["author_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some(action),
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    };
    let result = render_entity(&table).unwrap();
    assert!(
        result.contains(&format!("OnDelete:{expected}")),
        "expected OnDelete:{expected} in:\n{result}"
    );
}

// -----------------------------------------------------------------------
// Double-underscore table name
// -----------------------------------------------------------------------

#[test]
fn test_gorm_double_underscore_table_name() {
    // "order__item" splits into ["order", "", "item"]; the empty segment adds nothing
    let table = TableDef {
        name: "order__item".into(),
        description: None,
        columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };
    let result = render_entity(&table).unwrap();
    assert!(
        result.contains("type OrderItem struct"),
        "expected pascal-cased struct with double underscore"
    );
}

// -----------------------------------------------------------------------
// A named composite unique keeps its declared name in the uniqueIndex tag.
// -----------------------------------------------------------------------

#[test]
fn test_named_composite_unique_gorm() {
    let table = TableDef {
        name: "tenants".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("name", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::Unique {
                name: Some("uq_tenant_name".into()),
                columns: vec!["tenant_id".into(), "name".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            },
        ],
    };
    let result = render_entity(&table).unwrap();
    assert!(
        result.contains("uniqueIndex:uq_tenant_name"),
        "expected named uniqueIndex tag in GORM output"
    );
}
