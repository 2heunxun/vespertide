use super::*;
use insta::{assert_snapshot, with_settings};
use rstest::rstest;
use vespertide_core::schema::primary_key::PrimaryKeySyntax;
use vespertide_core::{
    ColumnDef, ColumnType, ComplexColumnType, EnumValues, NumValue, SimpleColumnType, StringOrBool,
    TableConstraint, TableDef,
};

#[rstest]
#[case("basic_single_pk", TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "display_name".into(), r#type: ColumnType::Simple(SimpleColumnType::Text), nullable: true, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
        ],
        constraints: vec![TableConstraint::PrimaryKey { auto_increment: false, columns: vec!["id".into()] }],
    })]
#[case("composite_pk", TableDef {
        name: "accounts".into(),
        description: None,
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "tenant_id".into(), r#type: ColumnType::Simple(SimpleColumnType::BigInt), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
        ],
        constraints: vec![TableConstraint::PrimaryKey { auto_increment: false, columns: vec!["id".into(), "tenant_id".into()] }],
    })]
#[case("fk_single", TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "user_id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "title".into(), r#type: ColumnType::Simple(SimpleColumnType::Text), nullable: true, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
        ],
        constraints: vec![
            TableConstraint::PrimaryKey { auto_increment: false, columns: vec!["id".into()] },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            },
        ],
    })]
#[case("fk_composite", TableDef {
        name: "invoices".into(),
        description: None,
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "customer_id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "customer_tenant_id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
        ],
        constraints: vec![
            TableConstraint::PrimaryKey { auto_increment: false, columns: vec!["id".into()] },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["customer_id".into(), "customer_tenant_id".into()],
                ref_table: "customers".into(),
                ref_columns: vec!["id".into(), "tenant_id".into()],
                on_delete: None,
                on_update: None,
            },
        ],
    })]
#[case("inline_pk", TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Uuid), nullable: false, default: Some("gen_random_uuid()".into()), comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef { name: "email".into(), r#type: ColumnType::Simple(SimpleColumnType::Text), nullable: false, default: None, comment: None, primary_key: None, unique: Some(vespertide_core::StrOrBoolOrArray::Bool(true)), index: None, foreign_key: None },
        ],
        constraints: vec![],
    })]
#[case("pk_and_fk_together", {
        use vespertide_core::schema::foreign_key::{ForeignKeyDef, ForeignKeySyntax};
        use vespertide_core::schema::reference::ReferenceAction;
        let mut table = TableDef {
            name: "article_user".into(),
            description: None,
            columns: vec![
                ColumnDef {
                    name: "article_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: Some(PrimaryKeySyntax::Bool(true)),
                    unique: None,
                    index: Some(vespertide_core::StrOrBoolOrArray::Bool(true)),
                    foreign_key: Some(ForeignKeySyntax::Object(ForeignKeyDef {
                        ref_table: "article".into(),
                        ref_columns: vec!["id".into()],
                        on_delete: Some(ReferenceAction::Cascade),
                        on_update: None,
                    })),
                },
                ColumnDef {
                    name: "user_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: Some(PrimaryKeySyntax::Bool(true)),
                    unique: None,
                    index: Some(vespertide_core::StrOrBoolOrArray::Bool(true)),
                    foreign_key: Some(ForeignKeySyntax::Object(ForeignKeyDef {
                        ref_table: "user".into(),
                        ref_columns: vec!["id".into()],
                        on_delete: Some(ReferenceAction::Cascade),
                        on_update: None,
                    })),
                },
                ColumnDef {
                    name: "author_order".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: Some("1".into()),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "role".into(),
                    r#type: ColumnType::Complex(vespertide_core::ComplexColumnType::Varchar { length: 20 }),
                    nullable: false,
                    default: Some("'contributor'".into()),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "is_lead".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Boolean),
                    nullable: false,
                    default: Some("false".into()),
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
            constraints: vec![],
        };
        // Normalize to convert inline constraints to table-level
        table = table.normalize().unwrap();
        table
    })]
#[case("enum_type", TableDef {
        name: "orders".into(),
        description: None,
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "order_status".into(),
                    values: EnumValues::String(vec!["pending".into(), "shipped".into(), "delivered".into()])
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
        constraints: vec![],
    })]
#[case("enum_nullable", TableDef {
        name: "tasks".into(),
        description: None,
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef {
                name: "priority".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "task_priority".into(),
                    values: EnumValues::String(vec!["low".into(), "medium".into(), "high".into(), "critical".into()])
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
        constraints: vec![],
    })]
#[case("enum_multiple_columns", TableDef {
        name: "products".into(),
        description: None,
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef {
                name: "category".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "product_category".into(),
                    values: EnumValues::String(vec!["electronics".into(), "clothing".into(), "food".into()])
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
                name: "availability".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "availability_status".into(),
                    values: EnumValues::String(vec!["in_stock".into(), "out_of_stock".into(), "pre_order".into()])
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
        constraints: vec![],
    })]
#[case("enum_shared", TableDef {
        name: "documents".into(),
        description: None,
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "doc_status".into(),
                    values: EnumValues::String(vec!["draft".into(), "published".into(), "archived".into()])
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
                name: "review_status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "doc_status".into(),
                    values: EnumValues::String(vec!["draft".into(), "published".into(), "archived".into()])
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
        constraints: vec![],
    })]
#[case("enum_special_values", TableDef {
        name: "events".into(),
        description: None,
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef {
                name: "severity".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "event_severity".into(),
                    values: EnumValues::String(vec!["info-level".into(), "warning_level".into(), "ERROR_LEVEL".into(), "1critical".into()])
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
        constraints: vec![],
    })]
#[case("unique_and_indexed", TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef { name: "email".into(), r#type: ColumnType::Simple(SimpleColumnType::Text), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "username".into(), r#type: ColumnType::Simple(SimpleColumnType::Text), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "department".into(), r#type: ColumnType::Simple(SimpleColumnType::Text), nullable: true, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "status".into(), r#type: ColumnType::Simple(SimpleColumnType::Text), nullable: false, default: Some("'active'".into()), comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
        ],
        constraints: vec![
            TableConstraint::Unique { name: None, columns: vec!["email".into()] },
            TableConstraint::Unique { name: Some("uq_username".into()), columns: vec!["username".into()] },
            TableConstraint::Index { name: Some("idx_department".into()), columns: vec!["department".into()] },
        ],
    })]
#[case("enum_with_default", TableDef {
        name: "tasks".into(),
        description: None,
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "task_status".into(),
                    values: EnumValues::String(vec!["pending".into(), "in_progress".into(), "completed".into()])
                }),
                nullable: false,
                default: Some("'pending'".into()),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef { name: "priority".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: Some("0".into()), comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "is_archived".into(), r#type: ColumnType::Simple(SimpleColumnType::Boolean), nullable: false, default: Some("false".into()), comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
        ],
        constraints: vec![],
    })]
#[case("table_level_pk", TableDef {
        name: "orders".into(),
        description: None,
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Uuid), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "customer_id".into(), r#type: ColumnType::Simple(SimpleColumnType::Uuid), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "total".into(), r#type: ColumnType::Simple(SimpleColumnType::Real), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
        ],
        constraints: vec![
            TableConstraint::PrimaryKey { columns: vec!["id".into()], auto_increment: false },
        ],
    })]
#[case("jsonb_custom_type", TableDef {
        name: "json_struct".into(),
        description: None,
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef { name: "json_data".into(), r#type: ColumnType::Simple(SimpleColumnType::Json), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "jsonb_data".into(), r#type: ColumnType::Complex(ComplexColumnType::Custom { custom_type: "JSONB".into() }), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "jsonb_nullable".into(), r#type: ColumnType::Complex(ComplexColumnType::Custom { custom_type: "jsonb".into() }), nullable: true, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
        ],
        constraints: vec![],
    })]
fn render_entity_snapshots(#[case] name: &str, #[case] table: TableDef) {
    let rendered = render_entity(&table);
    with_settings!({ snapshot_suffix => format!("params_{}", name) }, {
        assert_snapshot!(rendered);
    });
}

// Helper to create a simple table with PK
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

fn table_with_pk(name: &str, columns: Vec<ColumnDef>, pk_cols: Vec<&str>) -> TableDef {
    TableDef {
        name: name.into(),
        description: None,
        columns,
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: pk_cols.into_iter().map(String::from).collect(),
        }],
    }
}

fn table_with_pk_and_fk(
    name: &str,
    columns: Vec<ColumnDef>,
    pk_cols: Vec<&str>,
    fks: Vec<(Vec<&str>, &str, Vec<&str>)>,
) -> TableDef {
    let mut constraints = vec![TableConstraint::PrimaryKey {
        auto_increment: false,
        columns: pk_cols.into_iter().map(String::from).collect(),
    }];
    for (cols, ref_table, ref_cols) in fks {
        constraints.push(TableConstraint::ForeignKey {
            name: None,
            columns: cols.into_iter().map(String::from).collect(),
            ref_table: ref_table.into(),
            ref_columns: ref_cols.into_iter().map(String::from).collect(),
            on_delete: None,
            on_update: None,
        });
    }
    TableDef {
        name: name.into(),
        description: None,
        columns,
        constraints,
    }
}

#[rstest]
#[case("many_to_many_article")]
#[case("many_to_many_user")]
#[case("many_to_many_missing_target")]
#[case("many_to_many_multiple_junctions")]
#[case("composite_fk_parent")]
#[case("not_junction_single_pk")]
#[case("not_junction_fk_not_in_pk_other")]
#[case("not_junction_fk_not_in_pk_another")]
#[case("multiple_fk_same_table")]
#[case("username_fk")]
#[case("multiple_reverse_relations")]
#[case("dual_reverse_relations")]
#[case("triple_reverse_relations")]
#[case("multiple_has_one_relations")]
fn render_entity_with_schema_snapshots(#[case] name: &str) {
    use vespertide_core::SimpleColumnType::*;

    let (table, schema) = match name {
        "many_to_many_article" => {
            let article = table_with_pk(
                "article",
                vec![col("id", ColumnType::Simple(BigInt))],
                vec!["id"],
            );
            let user = table_with_pk(
                "user",
                vec![col("id", ColumnType::Simple(Uuid))],
                vec!["id"],
            );
            let article_user = table_with_pk_and_fk(
                "article_user",
                vec![
                    col("article_id", ColumnType::Simple(BigInt)),
                    col("user_id", ColumnType::Simple(Uuid)),
                ],
                vec!["article_id", "user_id"],
                vec![
                    (vec!["article_id"], "article", vec!["id"]),
                    (vec!["user_id"], "user", vec!["id"]),
                ],
            );
            (article.clone(), vec![article, user, article_user])
        }
        "many_to_many_user" => {
            let article = table_with_pk(
                "article",
                vec![col("id", ColumnType::Simple(BigInt))],
                vec!["id"],
            );
            let user = table_with_pk(
                "user",
                vec![col("id", ColumnType::Simple(Uuid))],
                vec!["id"],
            );
            let article_user = table_with_pk_and_fk(
                "article_user",
                vec![
                    col("article_id", ColumnType::Simple(BigInt)),
                    col("user_id", ColumnType::Simple(Uuid)),
                ],
                vec!["article_id", "user_id"],
                vec![
                    (vec!["article_id"], "article", vec!["id"]),
                    (vec!["user_id"], "user", vec!["id"]),
                ],
            );
            (user.clone(), vec![article, user, article_user])
        }
        "many_to_many_missing_target" => {
            let article = table_with_pk(
                "article",
                vec![col("id", ColumnType::Simple(BigInt))],
                vec!["id"],
            );
            let article_user = table_with_pk_and_fk(
                "article_user",
                vec![
                    col("article_id", ColumnType::Simple(BigInt)),
                    col("user_id", ColumnType::Simple(Uuid)),
                ],
                vec!["article_id", "user_id"],
                vec![
                    (vec!["article_id"], "article", vec!["id"]),
                    (vec!["user_id"], "user", vec!["id"]), // user not in schema
                ],
            );
            (article.clone(), vec![article, article_user])
        }
        "many_to_many_multiple_junctions" => {
            // Test case: user has M2M to media via TWO different junction tables
            // This triggers relation_enum for M2M relations (line 664)
            let user = table_with_pk(
                "user",
                vec![col("id", ColumnType::Simple(Uuid))],
                vec!["id"],
            );
            let media = table_with_pk(
                "media",
                vec![col("id", ColumnType::Simple(Uuid))],
                vec!["id"],
            );
            // First junction: user_media_role (e.g., user's role-based access to media)
            let user_media_role = table_with_pk_and_fk(
                "user_media_role",
                vec![
                    col("user_id", ColumnType::Simple(Uuid)),
                    col("media_id", ColumnType::Simple(Uuid)),
                ],
                vec!["user_id", "media_id"],
                vec![
                    (vec!["user_id"], "user", vec!["id"]),
                    (vec!["media_id"], "media", vec!["id"]),
                ],
            );
            // Second junction: user_media_favorite (e.g., user's favorites)
            let user_media_favorite = table_with_pk_and_fk(
                "user_media_favorite",
                vec![
                    col("user_id", ColumnType::Simple(Uuid)),
                    col("media_id", ColumnType::Simple(Uuid)),
                ],
                vec!["user_id", "media_id"],
                vec![
                    (vec!["user_id"], "user", vec!["id"]),
                    (vec!["media_id"], "media", vec!["id"]),
                ],
            );
            (
                user.clone(),
                vec![user, media, user_media_role, user_media_favorite],
            )
        }
        "composite_fk_parent" => {
            let parent = table_with_pk(
                "parent",
                vec![
                    col("id1", ColumnType::Simple(Integer)),
                    col("id2", ColumnType::Simple(Integer)),
                ],
                vec!["id1", "id2"],
            );
            let child_one = table_with_pk_and_fk(
                "child_one",
                vec![
                    col("parent_id1", ColumnType::Simple(Integer)),
                    col("parent_id2", ColumnType::Simple(Integer)),
                ],
                vec!["parent_id1", "parent_id2"],
                vec![(
                    vec!["parent_id1", "parent_id2"],
                    "parent",
                    vec!["id1", "id2"],
                )],
            );
            let child_many = table_with_pk_and_fk(
                "child_many",
                vec![
                    col("id", ColumnType::Simple(Integer)),
                    col("parent_id1", ColumnType::Simple(Integer)),
                    col("parent_id2", ColumnType::Simple(Integer)),
                ],
                vec!["id"],
                vec![(
                    vec!["parent_id1", "parent_id2"],
                    "parent",
                    vec!["id1", "id2"],
                )],
            );
            (parent.clone(), vec![parent, child_one, child_many])
        }
        "not_junction_single_pk" => {
            let other = table_with_pk(
                "other",
                vec![col("id", ColumnType::Simple(Integer))],
                vec!["id"],
            );
            let regular = table_with_pk_and_fk(
                "regular",
                vec![
                    col("id", ColumnType::Simple(Integer)),
                    col("other_id", ColumnType::Simple(Integer)),
                ],
                vec!["id"], // single column PK
                vec![(vec!["other_id"], "other", vec!["id"])],
            );
            (other.clone(), vec![other, regular])
        }
        "not_junction_fk_not_in_pk_other" => {
            let other = table_with_pk(
                "other",
                vec![col("id", ColumnType::Simple(Integer))],
                vec!["id"],
            );
            let another = table_with_pk(
                "another",
                vec![col("id", ColumnType::Simple(Integer))],
                vec!["id"],
            );
            let not_junction = table_with_pk_and_fk(
                "not_junction",
                vec![
                    col("id", ColumnType::Simple(Integer)),
                    col("other_id", ColumnType::Simple(Integer)),
                    col("another_id", ColumnType::Simple(Integer)),
                ],
                vec!["id", "other_id"], // another_id not in PK
                vec![
                    (vec!["other_id"], "other", vec!["id"]),
                    (vec!["another_id"], "another", vec!["id"]),
                ],
            );
            (other.clone(), vec![other, another, not_junction])
        }
        "not_junction_fk_not_in_pk_another" => {
            let other = table_with_pk(
                "other",
                vec![col("id", ColumnType::Simple(Integer))],
                vec!["id"],
            );
            let another = table_with_pk(
                "another",
                vec![col("id", ColumnType::Simple(Integer))],
                vec!["id"],
            );
            let not_junction = table_with_pk_and_fk(
                "not_junction",
                vec![
                    col("id", ColumnType::Simple(Integer)),
                    col("other_id", ColumnType::Simple(Integer)),
                    col("another_id", ColumnType::Simple(Integer)),
                ],
                vec!["id", "other_id"], // another_id not in PK
                vec![
                    (vec!["other_id"], "other", vec!["id"]),
                    (vec!["another_id"], "another", vec!["id"]),
                ],
            );
            (another.clone(), vec![other, another, not_junction])
        }
        "multiple_fk_same_table" => {
            let user = table_with_pk(
                "user",
                vec![col("id", ColumnType::Simple(Uuid))],
                vec!["id"],
            );
            let post = table_with_pk_and_fk(
                "post",
                vec![
                    col("id", ColumnType::Simple(Uuid)),
                    col("creator_user_id", ColumnType::Simple(Uuid)),
                    col("used_by_user_id", ColumnType::Simple(Uuid)),
                ],
                vec!["id"],
                vec![
                    (vec!["creator_user_id"], "user", vec!["id"]),
                    (vec!["used_by_user_id"], "user", vec!["id"]),
                ],
            );
            (post.clone(), vec![user, post])
        }
        "username_fk" => {
            let user = table_with_pk(
                "user",
                vec![col("username", ColumnType::Simple(Text))],
                vec!["username"],
            );
            let session = table_with_pk_and_fk(
                "session",
                vec![
                    col("id", ColumnType::Simple(Uuid)),
                    col("username", ColumnType::Simple(Text)),
                ],
                vec!["id"],
                vec![(vec!["username"], "user", vec!["username"])],
            );
            (session.clone(), vec![user, session])
        }
        "multiple_reverse_relations" => {
            // Test case where user has multiple has_one relations from profile
            let user = table_with_pk(
                "user",
                vec![col("id", ColumnType::Simple(Uuid))],
                vec!["id"],
            );
            let profile = table_with_pk_and_fk(
                "profile",
                vec![
                    col("id", ColumnType::Simple(Uuid)),
                    col("preferred_user_id", ColumnType::Simple(Uuid)),
                    col("backup_user_id", ColumnType::Simple(Uuid)),
                ],
                vec!["id"],
                vec![
                    (vec!["preferred_user_id"], "user", vec!["id"]),
                    (vec!["backup_user_id"], "user", vec!["id"]),
                ],
            );
            (user.clone(), vec![user, profile])
        }
        "dual_reverse_relations" => {
            let dual = table_with_pk(
                "dual",
                vec![col("username", ColumnType::Simple(Text))],
                vec!["username"],
            );
            let dual_rel = table_with_pk_and_fk(
                "dual_rel",
                vec![
                    col("username", ColumnType::Simple(Text)),
                    col("checker_username", ColumnType::Simple(Text)),
                ],
                vec!["username", "checker_username"],
                vec![
                    (vec!["username"], "dual", vec!["username"]),
                    (vec!["checker_username"], "dual", vec!["username"]),
                ],
            );
            (dual.clone(), vec![dual, dual_rel])
        }
        "triple_reverse_relations" => {
            let dual = table_with_pk(
                "dual",
                vec![col("username", ColumnType::Simple(Text))],
                vec!["username"],
            );
            let triple_rel = table_with_pk_and_fk(
                "triple_rel",
                vec![
                    col("username", ColumnType::Simple(Text)),
                    col("checker_username", ColumnType::Simple(Text)),
                    col("other_username", ColumnType::Simple(Text)),
                ],
                vec!["username", "checker_username", "other_username"],
                vec![
                    (vec!["username"], "dual", vec!["username"]),
                    (vec!["checker_username"], "dual", vec!["username"]),
                    (vec!["other_username"], "dual", vec!["username"]),
                ],
            );
            (dual.clone(), vec![dual, triple_rel])
        }
        "multiple_has_one_relations" => {
            // Test case where user has multiple has_one relations (UNIQUE FK)
            let user = table_with_pk(
                "user",
                vec![col("id", ColumnType::Simple(Uuid))],
                vec!["id"],
            );
            let settings = table_with_pk_and_fk(
                "settings",
                vec![
                    col("id", ColumnType::Simple(Uuid)),
                    col("created_by_user_id", ColumnType::Simple(Uuid)),
                    col("updated_by_user_id", ColumnType::Simple(Uuid)),
                ],
                vec!["id"],
                vec![
                    (vec!["created_by_user_id"], "user", vec!["id"]),
                    (vec!["updated_by_user_id"], "user", vec!["id"]),
                ],
            );
            // Add unique constraints to make them has_one (coverage for line 553)
            let mut settings_with_unique = settings;
            settings_with_unique
                .constraints
                .push(TableConstraint::Unique {
                    name: None,
                    columns: vec!["created_by_user_id".into()],
                });
            settings_with_unique
                .constraints
                .push(TableConstraint::Unique {
                    name: None,
                    columns: vec!["updated_by_user_id".into()],
                });
            (user.clone(), vec![user, settings_with_unique])
        }
        _ => panic!("Unknown test case: {name}"),
    };

    let rendered = render_entity_with_schema(&table, &schema);
    with_settings!({ snapshot_suffix => format!("schema_{}", name) }, {
        assert_snapshot!(rendered);
    });
}

#[test]
fn test_to_pascal_case_normal_chars() {
    assert_eq!(to_pascal_case("abc"), "Abc");
    assert_eq!(to_pascal_case("a_b_c"), "ABC");
}

#[test]
fn test_numeric_default_value() {
    use vespertide_core::ComplexColumnType;
    let table = TableDef {
        name: "products".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "price".into(),
            r#type: ColumnType::Complex(ComplexColumnType::Numeric {
                precision: 10,
                scale: 2,
            }),
            nullable: false,
            default: Some("0.00".into()),
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![],
    };
    let rendered = render_entity(&table);
    assert!(rendered.contains("default_value = 0.00"));
}

#[test]
fn render_entity_omits_eq_for_float_models() {
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;

    let table = TableDef {
        name: "measurements".into(),
        description: None,
        columns: vec![
            ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: Some(PrimaryKeySyntax::Bool(true)),
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "score".into(),
                r#type: ColumnType::Simple(SimpleColumnType::DoublePrecision),
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
    };

    let rendered = render_entity(&table);
    assert!(rendered.contains("#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]"));
    assert!(!rendered.contains("#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]"));
}

#[test]
fn test_orm_exporter_trait() {
    use crate::orm::OrmExporter;
    let table = table_with_pk(
        "test",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec!["id"],
    );
    let exporter = SeaOrmExporter;
    let result = exporter.render_entity(&table);
    assert!(result.is_ok());
    assert!(result.unwrap().contains("table_name = \"test\""));
    let schema = vec![table.clone()];
    let result = exporter.render_entity_with_schema(&table, &schema);
    assert!(result.is_ok());
    assert!(result.unwrap().contains("table_name = \"test\""));
}

fn int_enum_table(default_value: &str) -> TableDef {
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;
    TableDef {
        name: "tasks".into(),
        description: None,
        columns: vec![
            ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: Some(PrimaryKeySyntax::Bool(true)),
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "task_status".into(),
                    values: EnumValues::Integer(vec![
                        NumValue {
                            name: "Pending".into(),
                            value: 0,
                        },
                        NumValue {
                            name: "InProgress".into(),
                            value: 1,
                        },
                        NumValue {
                            name: "Completed".into(),
                            value: 100,
                        },
                    ]),
                }),
                nullable: false,
                default: Some(default_value.into()),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![],
    }
}

#[rstest]
#[case::numeric_default("1")]
#[case::non_numeric_default("pending_status")]
fn test_integer_enum_default_value_snapshots(#[case] default_value: &str) {
    let table = int_enum_table(default_value);
    let rendered = render_entity(&table);
    with_settings!({ snapshot_suffix => default_value }, {
        assert_snapshot!(rendered);
    });
}
#[test]
fn test_json_default_value_escapes_double_quotes() {
    let table = TableDef {
        name: "configs".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "data".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Json),
            nullable: false,
            default: Some(r#"{"hello": "world"}"#.into()),
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![],
    };
    let rendered = render_entity(&table);
    assert!(
        rendered.contains(r#"default_value = "{\"hello\": \"world\"}"#),
        "Expected escaped quotes in default_value, got: {rendered}"
    );
}
mod misc_tests;
