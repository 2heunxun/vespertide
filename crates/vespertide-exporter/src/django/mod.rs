mod enums;
mod render;
mod types;

use crate::orm::OrmExporter;
use vespertide_core::TableDef;

pub use render::{export, render_entity};

pub struct DjangoExporter;

impl OrmExporter for DjangoExporter {
    fn render_entity(&self, table: &TableDef) -> Result<String, String> {
        render_entity(table)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use rstest::rstest;
    use vespertide_core::schema::column::{EnumValues, SimpleColumnType};
    use vespertide_core::schema::constraint::TableConstraint;
    use vespertide_core::{
        ColumnType, ComplexColumnType, DefaultValue, NumValue, ReferenceAction, TableDef,
    };

    fn col(name: &str, ty: ColumnType) -> vespertide_core::ColumnDef {
        vespertide_core::ColumnDef::new(name, ty, false)
    }

    fn nullable_col(name: &str, ty: ColumnType) -> vespertide_core::ColumnDef {
        vespertide_core::ColumnDef::new(name, ty, true)
    }

    fn auto_pk(columns: &[&str]) -> TableConstraint {
        TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: columns.iter().copied().map(Into::into).collect(),
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }
    }

    fn pk(columns: &[&str]) -> TableConstraint {
        TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: columns.iter().copied().map(Into::into).collect(),
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }
    }

    fn fk(col: &str, ref_table: &str, on_delete: Option<ReferenceAction>) -> TableConstraint {
        TableConstraint::ForeignKey {
            name: None,
            columns: vec![col.into()],
            ref_table: ref_table.into(),
            ref_columns: vec!["id".into()],
            on_delete,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        }
    }

    // -----------------------------------------------------------------------
    // Basic table with autoincrement PK + nullable field
    // -----------------------------------------------------------------------

    #[test]
    fn test_basic_table() {
        let table = TableDef {
            name: "users".into(),
            description: Some("User accounts".into()),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col(
                    "email",
                    ColumnType::Complex(ComplexColumnType::Varchar { length: 255 }),
                ),
                nullable_col("name", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            constraints: vec![
                auto_pk(&["id"]),
                TableConstraint::Unique {
                    name: None,
                    columns: vec!["email".into()],
                    strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                        keep: vespertide_core::KeepPolicy::First,
                    },
                },
            ],
        };
        assert_snapshot!(render_entity(&table).unwrap());
    }

    // -----------------------------------------------------------------------
    // FK field: `_id` suffix stripping
    // -----------------------------------------------------------------------

    #[test]
    fn test_table_with_fk() {
        let table = TableDef {
            name: "posts".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("author_id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("title", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            constraints: vec![
                auto_pk(&["id"]),
                fk("author_id", "users", Some(ReferenceAction::Cascade)),
            ],
        };
        assert_snapshot!(render_entity(&table).unwrap());
    }

    // -----------------------------------------------------------------------
    // TextChoices enum
    // -----------------------------------------------------------------------

    #[test]
    fn test_table_with_string_enum() {
        let table = TableDef {
            name: "orders".into(),
            description: None,
            columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), {
                let mut c = col(
                    "status",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: "order_status".into(),
                        values: EnumValues::String(vec![
                            "pending".into(),
                            "shipped".into(),
                            "delivered".into(),
                        ]),
                    }),
                );
                c.default = Some(DefaultValue::String("'pending'".into()));
                c
            }],
            constraints: vec![auto_pk(&["id"])],
        };
        assert_snapshot!(render_entity(&table).unwrap());
    }

    // -----------------------------------------------------------------------
    // IntegerChoices enum
    // -----------------------------------------------------------------------

    #[test]
    fn test_table_with_integer_enum() {
        let table = TableDef {
            name: "tasks".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col(
                    "priority",
                    ColumnType::Complex(ComplexColumnType::Enum {
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
                ),
            ],
            constraints: vec![pk(&["id"])],
        };
        assert_snapshot!(render_entity(&table).unwrap());
    }

    // -----------------------------------------------------------------------
    // Composite PK
    // -----------------------------------------------------------------------

    #[test]
    fn test_composite_pk() {
        let table = TableDef {
            name: "order_items".into(),
            description: None,
            columns: vec![
                col("order_id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("product_id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("quantity", ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            constraints: vec![pk(&["order_id", "product_id"])],
        };
        assert_snapshot!(render_entity(&table).unwrap());
    }

    // -----------------------------------------------------------------------
    // Indexes and composite unique in Meta
    // -----------------------------------------------------------------------

    #[test]
    fn test_indexes_and_composite_unique() {
        let table = TableDef {
            name: "articles".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col(
                    "slug",
                    ColumnType::Complex(ComplexColumnType::Varchar { length: 200 }),
                ),
                col("author_id", ColumnType::Simple(SimpleColumnType::Integer)),
                col(
                    "created_at",
                    ColumnType::Simple(SimpleColumnType::Timestamptz),
                ),
            ],
            constraints: vec![
                auto_pk(&["id"]),
                TableConstraint::Index {
                    name: Some("ix_articles__created_at".into()),
                    columns: vec!["created_at".into()],
                },
                TableConstraint::Unique {
                    name: Some("uq_articles__slug_author".into()),
                    columns: vec!["slug".into(), "author_id".into()],
                    strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                        keep: vespertide_core::KeepPolicy::First,
                    },
                },
            ],
        };
        assert_snapshot!(render_entity(&table).unwrap());
    }

    // -----------------------------------------------------------------------
    // server default (NOW()) → timezone.now
    // -----------------------------------------------------------------------

    #[test]
    fn test_server_default_timezone() {
        let table = TableDef {
            name: "events".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                {
                    let mut c = col(
                        "created_at",
                        ColumnType::Simple(SimpleColumnType::Timestamptz),
                    );
                    c.default = Some(DefaultValue::String("NOW()".into()));
                    c
                },
                {
                    let mut c = col("count", ColumnType::Simple(SimpleColumnType::Integer));
                    c.default = Some(DefaultValue::Integer(0));
                    c
                },
            ],
            constraints: vec![auto_pk(&["id"])],
        };
        let result = render_entity(&table).unwrap();
        assert!(result.contains("from django.utils import timezone"));
        assert!(result.contains("default=timezone.now"));
        assert!(result.contains("default=0"));
        assert_snapshot!(result);
    }

    // -----------------------------------------------------------------------
    // Type coverage — all simple types
    // -----------------------------------------------------------------------

    #[rstest]
    #[case::small_int(SimpleColumnType::SmallInt, "models.SmallIntegerField")]
    #[case::bigint(SimpleColumnType::BigInt, "models.BigIntegerField")]
    #[case::real(SimpleColumnType::Real, "models.FloatField")]
    #[case::text(SimpleColumnType::Text, "models.TextField")]
    #[case::boolean(SimpleColumnType::Boolean, "models.BooleanField")]
    #[case::date(SimpleColumnType::Date, "models.DateField")]
    #[case::time(SimpleColumnType::Time, "models.TimeField")]
    #[case::timestamp(SimpleColumnType::Timestamp, "models.DateTimeField")]
    #[case::uuid(SimpleColumnType::Uuid, "models.UUIDField")]
    #[case::json(SimpleColumnType::Json, "models.JSONField")]
    #[case::bytea(SimpleColumnType::Bytea, "models.BinaryField")]
    #[case::inet(SimpleColumnType::Inet, "models.GenericIPAddressField")]
    #[case::interval(SimpleColumnType::Interval, "models.DurationField")]
    #[case::macaddr(SimpleColumnType::Macaddr, "models.CharField")]
    fn test_simple_type_mapping(#[case] ty: SimpleColumnType, #[case] expected: &str) {
        let table = TableDef {
            name: "t".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("val", ColumnType::Simple(ty)),
            ],
            constraints: vec![pk(&["id"])],
        };
        let result = render_entity(&table).unwrap();
        assert!(
            result.contains(expected),
            "expected {expected} in:\n{result}"
        );
    }

    #[rstest]
    #[case::small_auto(SimpleColumnType::SmallInt, "models.SmallAutoField")]
    #[case::big_auto(SimpleColumnType::BigInt, "models.BigAutoField")]
    fn test_auto_pk_field_types(#[case] ty: SimpleColumnType, #[case] expected: &str) {
        let table = TableDef {
            name: "t".into(),
            description: None,
            columns: vec![col("id", ColumnType::Simple(ty))],
            constraints: vec![auto_pk(&["id"])],
        };
        let result = render_entity(&table).unwrap();
        assert!(
            result.contains(expected),
            "expected {expected} in:\n{result}"
        );
    }

    #[test]
    fn test_numeric_field() {
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
            constraints: vec![auto_pk(&["id"])],
        };
        let result = render_entity(&table).unwrap();
        assert!(
            result.contains("models.DecimalField"),
            "expected DecimalField"
        );
        assert!(result.contains("max_digits=10"), "expected max_digits=10");
        assert!(
            result.contains("decimal_places=2"),
            "expected decimal_places=2"
        );
    }

    #[test]
    fn test_custom_type_field() {
        let table = TableDef {
            name: "docs".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col(
                    "data",
                    ColumnType::Complex(ComplexColumnType::Custom {
                        custom_type: "JSONB".into(),
                    }),
                ),
            ],
            constraints: vec![auto_pk(&["id"])],
        };
        let result = render_entity(&table).unwrap();
        // Custom type → models.TextField (Django has no native JSONB)
        assert!(
            result.contains("data = models.TextField()"),
            "expected Custom→TextField in:\n{result}"
        );
    }

    #[test]
    fn test_uuid_default() {
        let mut id_col = col("id", ColumnType::Simple(SimpleColumnType::Uuid));
        id_col.default = Some(DefaultValue::String("gen_random_uuid()".into()));
        let table = TableDef {
            name: "sessions".into(),
            description: None,
            columns: vec![id_col],
            constraints: vec![pk(&["id"])],
        };
        let result = render_entity(&table).unwrap();
        assert!(result.contains("import uuid"), "expected uuid import");
        assert!(
            result.contains("default=uuid.uuid4"),
            "expected uuid4 callable"
        );
    }

    #[test]
    fn test_export_multi_table() {
        let users = TableDef {
            name: "users".into(),
            description: None,
            columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            constraints: vec![auto_pk(&["id"])],
        };
        let posts = TableDef {
            name: "posts".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("author_id", ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            constraints: vec![
                auto_pk(&["id"]),
                fk("author_id", "users", Some(ReferenceAction::Cascade)),
            ],
        };
        let result = export(&[users, posts]).unwrap();
        assert!(result.contains("class Users(models.Model):"));
        assert!(result.contains("class Posts(models.Model):"));
    }

    #[test]
    fn test_nullable_fk_with_db_column() {
        // FK column without `_id` suffix → emits db_column kwarg
        // Nullable FK → emits null=True, blank=True
        let table = TableDef {
            name: "comments".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                nullable_col("parent", ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            constraints: vec![
                auto_pk(&["id"]),
                fk("parent", "comments", Some(ReferenceAction::SetNull)),
            ],
        };
        let result = render_entity(&table).unwrap();
        assert!(
            result.contains(r#"db_column="parent""#),
            "expected db_column kwarg"
        );
        assert!(
            result.contains("null=True"),
            "expected null=True for nullable FK"
        );
        assert!(
            result.contains("blank=True"),
            "expected blank=True for nullable FK"
        );
    }
}
