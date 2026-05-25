use super::*;

#[test]
fn validate_schema_rejects_duplicate_column_names() {
    let schema = vec![table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("id", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        vec![pk(vec!["id"])],
    )];

    let err = validate_schema(&schema).unwrap_err();

    assert!(matches!(err, PlannerError::TableValidation(_)));
    assert_eq!(
        err.to_string(),
        "table validation error: table 'users' has duplicate column name 'id'"
    );
}

#[test]
fn validate_schema_fk_ref_column_not_found() {
    let schema = vec![table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![
            pk(vec!["id"]),
            TableConstraint::ForeignKey {
                name: Some("fk_bad".into()),
                columns: vec!["id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["nonexistent".into()],
                on_delete: None,
                on_update: None,
            },
        ],
    )];

    let result = validate_schema(&schema);

    assert!(
        matches!(
            result,
            Err(PlannerError::ForeignKeyColumnNotFound(_, _, _, _))
        ),
        "FK pointing to non-existent column should trigger ForeignKeyColumnNotFound, got: {result:?}"
    );
}

#[test]
fn validate_schema_duplicate_enum_variant_name() {
    let schema = vec![table(
        "orders",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "status_enum".into(),
                    values: EnumValues::String(vec!["active".into(), "active".into()]),
                }),
            ),
        ],
        vec![pk(vec!["id"])],
    )];

    let result = validate_schema(&schema);

    assert!(
        matches!(
            result,
            Err(PlannerError::DuplicateEnumVariantName(_, _, _, _))
        ),
        "duplicate enum variant should trigger DuplicateEnumVariantName, got: {result:?}"
    );
}

#[test]
fn validate_schema_rejects_numeric_scale_greater_than_precision() {
    let schema = vec![table(
        "prices",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(
                "amount",
                ColumnType::Complex(ComplexColumnType::Numeric {
                    precision: 5,
                    scale: 10,
                }),
            ),
        ],
        vec![pk(vec!["id"])],
    )];

    let err = validate_schema(&schema).unwrap_err();

    assert!(matches!(err, PlannerError::TableValidation(_)));
    assert!(
        err.to_string()
            .contains("scale (10) must be <= precision (5)")
    );
}

#[test]
fn validate_schema_accepts_numeric_scale_equal_to_precision() {
    let schema = vec![table(
        "prices",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(
                "amount",
                ColumnType::Complex(ComplexColumnType::Numeric {
                    precision: 5,
                    scale: 5,
                }),
            ),
        ],
        vec![pk(vec!["id"])],
    )];

    assert!(validate_schema(&schema).is_ok());
}

#[test]
fn validate_schema_rejects_integer_enum_values_outside_i32_range() {
    let table = table(
        "tasks",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(
                "priority",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "task_priority".into(),
                    values: EnumValues::Integer(vec![
                        NumValue {
                            name: "low".into(),
                            value: 0,
                        },
                        NumValue {
                            name: "too_large".into(),
                            value: 9_999_999_999,
                        },
                    ]),
                }),
            ),
        ],
        vec![pk(vec!["id"])],
    );

    let err = validate_schema(&[table]).unwrap_err();

    assert!(matches!(err, PlannerError::TableValidation(_)));
    assert!(
        err.to_string()
            .contains("integer enum value 9999999999 is outside i32 range")
    );
}

#[rstest]
#[case::valid_schema(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![TableConstraint::PrimaryKey{ auto_increment: false, columns: vec!["id".into()] }],
        )],
        None
    )]
#[case::duplicate_table(
        vec![
            table("users", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![]),
            table("users", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![]),
        ],
        Some(is_duplicate as fn(&PlannerError) -> bool)
    )]
#[case::fk_missing_table(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![pk(vec!["id"]), TableConstraint::ForeignKey {
                name: None,
                columns: vec!["id".into()],
                ref_table: "nonexistent".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            }],
        )],
        Some(is_fk_table as fn(&PlannerError) -> bool)
    )]
#[case::fk_missing_column(
        vec![
            table("posts", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![pk(vec!["id"])]),
            table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![pk(vec!["id"]), TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["id".into()],
                    ref_table: "posts".into(),
                    ref_columns: vec!["nonexistent".into()],
                    on_delete: None,
                    on_update: None,
                }],
            ),
        ],
        Some(is_fk_column as fn(&PlannerError) -> bool)
    )]
#[case::fk_local_missing_column(
        vec![
            table("posts", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![pk(vec!["id"])]),
            table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![pk(vec!["id"]), TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["missing".into()],
                    ref_table: "posts".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                }],
            ),
        ],
        Some(is_constraint_column as fn(&PlannerError) -> bool)
    )]
#[case::fk_valid(
        vec![
            table(
                "posts",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![pk(vec!["id"])],
            ),
            table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), col("post_id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![pk(vec!["id"]), TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["post_id".into()],
                    ref_table: "posts".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                }],
            ),
        ],
        None
    )]
#[case::index_missing_column(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![pk(vec!["id"]), idx("idx_name", vec!["nonexistent"])],
        )],
        Some(is_index_column as fn(&PlannerError) -> bool)
    )]
#[case::constraint_missing_column(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![TableConstraint::PrimaryKey{ auto_increment: false, columns: vec!["nonexistent".into()] }],
        )],
        Some(is_constraint_column as fn(&PlannerError) -> bool)
    )]
#[case::unique_empty_columns(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![pk(vec!["id"]), TableConstraint::Unique {
                name: Some("u".into()),
                columns: vec![],
            }],
        )],
        Some(is_empty_columns as fn(&PlannerError) -> bool)
    )]
#[case::unique_missing_column(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![pk(vec!["id"]), TableConstraint::Unique {
                name: None,
                columns: vec!["missing".into()],
            }],
        )],
        Some(is_constraint_column as fn(&PlannerError) -> bool)
    )]
#[case::empty_primary_key(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![TableConstraint::PrimaryKey{ auto_increment: false, columns: vec![] }],
        )],
        Some(is_empty_columns as fn(&PlannerError) -> bool)
    )]
#[case::fk_column_count_mismatch(
        vec![
            table(
                "posts",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![pk(vec!["id"])],
            ),
            table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), col("post_id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![pk(vec!["id"]), TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["id".into(), "post_id".into()],
                    ref_table: "posts".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                }],
            ),
        ],
        Some(is_fk_column as fn(&PlannerError) -> bool)
    )]
#[case::fk_empty_columns(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![pk(vec!["id"]), TableConstraint::ForeignKey {
                name: None,
                columns: vec![],
                ref_table: "posts".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            }],
        )],
        Some(is_empty_columns as fn(&PlannerError) -> bool)
    )]
#[case::fk_empty_ref_columns(
        vec![
            table(
                "posts",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![pk(vec!["id"])],
            ),
            table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![pk(vec!["id"]), TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["id".into()],
                    ref_table: "posts".into(),
                    ref_columns: vec![],
                    on_delete: None,
                    on_update: None,
                }],
            ),
        ],
        Some(is_empty_columns as fn(&PlannerError) -> bool)
    )]
#[case::index_empty_columns(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![pk(vec!["id"]), TableConstraint::Index {
                name: Some("idx".into()),
                columns: vec![],
            }],
        )],
        Some(is_empty_columns as fn(&PlannerError) -> bool)
    )]
#[case::index_valid(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), col("name", ColumnType::Simple(SimpleColumnType::Text))],
            vec![pk(vec!["id"]), idx("idx_name", vec!["name"])],
        )],
        None
    )]
#[case::check_constraint_ok(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![pk(vec!["id"]), TableConstraint::Check {
                name: "ck".into(),
                expr: "id > 0".into(),
            }],
        )],
        None
    )]
#[case::missing_primary_key(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
        )],
        Some(is_missing_pk as fn(&PlannerError) -> bool)
    )]
fn validate_schema_cases(
    #[case] schema: Vec<TableDef>,
    #[case] expected_err: Option<fn(&PlannerError) -> bool>,
) {
    let result = validate_schema(&schema);
    match expected_err {
        None => assert!(result.is_ok()),
        Some(pred) => {
            let err = result.unwrap_err();
            assert!(pred(&err), "unexpected error: {err:?}");
        }
    }
}
