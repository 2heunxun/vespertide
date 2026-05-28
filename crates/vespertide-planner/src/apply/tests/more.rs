use super::*;

// Tests for CreateTable normalizing inline constraints
#[test]
fn create_table_normalizes_inline_unique() {
    let mut col_with_unique = col("email", ColumnType::Simple(SimpleColumnType::Text));
    col_with_unique.unique = Some(vespertide_core::StrOrBoolOrArray::Bool(true));

    let mut schema = vec![];
    apply_action(
        &mut schema,
        &MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![col_with_unique],
            constraints: vec![],
        },
    )
    .unwrap();

    // Inline unique: true should be normalized to a TableConstraint::Unique
    assert!(
        schema[0]
            .constraints
            .iter()
            .any(|c| matches!(c, TableConstraint::Unique { columns, .. } if columns == &["email"])),
        "Expected a Unique constraint on 'email', got: {:?}",
        schema[0].constraints
    );
}

#[test]
fn create_table_normalizes_inline_index() {
    let mut col_with_index = col("email", ColumnType::Simple(SimpleColumnType::Text));
    col_with_index.index = Some(vespertide_core::StrOrBoolOrArray::Bool(true));

    let mut schema = vec![];
    apply_action(
        &mut schema,
        &MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![col_with_index],
            constraints: vec![],
        },
    )
    .unwrap();

    // Inline index: true should be normalized to a TableConstraint::Index
    assert!(
        schema[0]
            .constraints
            .iter()
            .any(|c| matches!(c, TableConstraint::Index { columns, .. } if columns == &["email"])),
        "Expected an Index constraint on 'email', got: {:?}",
        schema[0].constraints
    );
}

#[test]
fn create_table_normalizes_inline_primary_key() {
    let mut col_with_pk = col("id", ColumnType::Simple(SimpleColumnType::Integer));
    col_with_pk.primary_key =
        Some(vespertide_core::schema::primary_key::PrimaryKeySyntax::Bool(true));

    let mut schema = vec![];
    apply_action(
        &mut schema,
        &MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![col_with_pk],
            constraints: vec![],
        },
    )
    .unwrap();

    assert!(
        schema[0].constraints.iter().any(
            |c| matches!(c, TableConstraint::PrimaryKey { columns, .. } if columns == &["id"])
        ),
        "Expected a PrimaryKey constraint on 'id', got: {:?}",
        schema[0].constraints
    );
}

// Tests for AddColumn normalizing inline constraints
#[test]
fn add_column_normalizes_inline_unique() {
    let mut schema = vec![table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![],
    )];

    let mut col_with_unique = col("email", ColumnType::Simple(SimpleColumnType::Text));
    col_with_unique.unique = Some(vespertide_core::StrOrBoolOrArray::Bool(true));

    apply_action(
        &mut schema,
        &MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(col_with_unique),
            fill_with: None,
        },
    )
    .unwrap();

    assert!(
        schema[0]
            .constraints
            .iter()
            .any(|c| matches!(c, TableConstraint::Unique { columns, .. } if columns == &["email"])),
        "Expected a Unique constraint on 'email' after AddColumn, got: {:?}",
        schema[0].constraints
    );
}

#[test]
fn add_column_normalizes_inline_index() {
    let mut schema = vec![table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![],
    )];

    let mut col_with_index = col("email", ColumnType::Simple(SimpleColumnType::Text));
    col_with_index.index = Some(vespertide_core::StrOrBoolOrArray::Bool(true));

    apply_action(
        &mut schema,
        &MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(col_with_index),
            fill_with: None,
        },
    )
    .unwrap();

    assert!(
        schema[0]
            .constraints
            .iter()
            .any(|c| matches!(c, TableConstraint::Index { columns, .. } if columns == &["email"])),
        "Expected an Index constraint on 'email' after AddColumn, got: {:?}",
        schema[0].constraints
    );
}

// Tests for ModifyColumnNullable
#[test]
fn apply_modify_column_nullable_success() {
    let mut schema = vec![table(
        "users",
        vec![col("email", ColumnType::Simple(SimpleColumnType::Text))],
        vec![],
    )];

    // Initially nullable: true (from col helper)
    assert!(schema[0].columns[0].nullable);

    apply_action(
        &mut schema,
        &MigrationAction::ModifyColumnNullable {
            table: "users".into(),
            column: "email".into(),
            nullable: false,
            fill_with: None,
            delete_null_rows: None,
        },
    )
    .unwrap();

    assert!(!schema[0].columns[0].nullable);
}

#[test]
fn apply_modify_column_nullable_table_not_found() {
    let mut schema = vec![];

    let err = apply_action(
        &mut schema,
        &MigrationAction::ModifyColumnNullable {
            table: "users".into(),
            column: "email".into(),
            nullable: false,
            fill_with: None,
            delete_null_rows: None,
        },
    )
    .unwrap_err();

    assert_err_kind(&err, ErrKind::TableNotFound);
}

#[test]
fn apply_modify_column_nullable_column_not_found() {
    let mut schema = vec![table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![],
    )];

    let err = apply_action(
        &mut schema,
        &MigrationAction::ModifyColumnNullable {
            table: "users".into(),
            column: "email".into(),
            nullable: false,
            fill_with: None,
            delete_null_rows: None,
        },
    )
    .unwrap_err();

    assert_err_kind(&err, ErrKind::ColumnNotFound);
}

// Tests for ModifyColumnDefault
#[test]
fn apply_modify_column_default_set() {
    let mut schema = vec![table(
        "users",
        vec![col("status", ColumnType::Simple(SimpleColumnType::Text))],
        vec![],
    )];

    // Initially no default
    assert!(schema[0].columns[0].default.is_none());

    apply_action(
        &mut schema,
        &MigrationAction::ModifyColumnDefault {
            table: "users".into(),
            column: "status".into(),
            new_default: Some("'active'".into()),
            backfill: None,
        },
    )
    .unwrap();

    assert_eq!(
        schema[0].columns[0].default,
        Some(vespertide_core::StringOrBool::String("'active'".into()))
    );
}

#[test]
fn apply_modify_column_default_drop() {
    let mut col_with_default = col("status", ColumnType::Simple(SimpleColumnType::Text));
    col_with_default.default = Some(vespertide_core::StringOrBool::String("'active'".into()));

    let mut schema = vec![table("users", vec![col_with_default], vec![])];

    apply_action(
        &mut schema,
        &MigrationAction::ModifyColumnDefault {
            table: "users".into(),
            column: "status".into(),
            new_default: None,
            backfill: None,
        },
    )
    .unwrap();

    assert!(schema[0].columns[0].default.is_none());
}

#[test]
fn apply_modify_column_default_table_not_found() {
    let mut schema = vec![];

    let err = apply_action(
        &mut schema,
        &MigrationAction::ModifyColumnDefault {
            table: "users".into(),
            column: "status".into(),
            new_default: Some("'active'".into()),
            backfill: None,
        },
    )
    .unwrap_err();

    assert_err_kind(&err, ErrKind::TableNotFound);
}

#[test]
fn apply_modify_column_default_column_not_found() {
    let mut schema = vec![table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![],
    )];

    let err = apply_action(
        &mut schema,
        &MigrationAction::ModifyColumnDefault {
            table: "users".into(),
            column: "status".into(),
            new_default: Some("'active'".into()),
            backfill: None,
        },
    )
    .unwrap_err();

    assert_err_kind(&err, ErrKind::ColumnNotFound);
}

// Tests for ModifyColumnComment
#[test]
fn apply_modify_column_comment_set() {
    let mut schema = vec![table(
        "users",
        vec![col("email", ColumnType::Simple(SimpleColumnType::Text))],
        vec![],
    )];

    // Initially no comment
    assert!(schema[0].columns[0].comment.is_none());

    apply_action(
        &mut schema,
        &MigrationAction::ModifyColumnComment {
            table: "users".into(),
            column: "email".into(),
            new_comment: Some("User email address".into()),
        },
    )
    .unwrap();

    assert_eq!(
        schema[0].columns[0].comment,
        Some("User email address".into())
    );
}

#[test]
fn apply_modify_column_comment_drop() {
    let mut col_with_comment = col("email", ColumnType::Simple(SimpleColumnType::Text));
    col_with_comment.comment = Some("User email address".into());

    let mut schema = vec![table("users", vec![col_with_comment], vec![])];

    apply_action(
        &mut schema,
        &MigrationAction::ModifyColumnComment {
            table: "users".into(),
            column: "email".into(),
            new_comment: None,
        },
    )
    .unwrap();

    assert!(schema[0].columns[0].comment.is_none());
}

#[test]
fn apply_modify_column_comment_table_not_found() {
    let mut schema = vec![];

    let err = apply_action(
        &mut schema,
        &MigrationAction::ModifyColumnComment {
            table: "users".into(),
            column: "email".into(),
            new_comment: Some("User email".into()),
        },
    )
    .unwrap_err();

    assert_err_kind(&err, ErrKind::TableNotFound);
}

#[test]
fn apply_modify_column_comment_column_not_found() {
    let mut schema = vec![table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![],
    )];

    let err = apply_action(
        &mut schema,
        &MigrationAction::ModifyColumnComment {
            table: "users".into(),
            column: "email".into(),
            new_comment: Some("User email".into()),
        },
    )
    .unwrap_err();

    assert_err_kind(&err, ErrKind::ColumnNotFound);
}

#[test]
fn apply_replace_constraint_fk() {
    let mut schema = vec![table(
        "posts",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("user_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        vec![TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        }],
    )];

    let from = TableConstraint::ForeignKey {
        name: Some("fk_user".into()),
        columns: vec!["user_id".into()],
        ref_table: "users".into(),
        ref_columns: vec!["id".into()],
        on_delete: None,
        on_update: None,
        orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
    };
    let to = TableConstraint::ForeignKey {
        name: Some("fk_user".into()),
        columns: vec!["user_id".into()],
        ref_table: "users".into(),
        ref_columns: vec!["id".into()],
        on_delete: Some(vespertide_core::ReferenceAction::Cascade),
        on_update: None,
        orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
    };

    apply_action(
        &mut schema,
        &MigrationAction::ReplaceConstraint {
            table: "posts".into(),
            from,
            to: to.clone(),
        },
    )
    .unwrap();
    assert_eq!(schema[0].constraints.len(), 1);
    assert_eq!(schema[0].constraints[0], to);
}

#[test]
fn apply_replace_constraint_table_not_found() {
    let mut schema = vec![];
    let from = idx("ix_old", vec!["col"]);
    let to = idx("ix_new", vec!["col"]);
    let err = apply_action(
        &mut schema,
        &MigrationAction::ReplaceConstraint {
            table: "missing".into(),
            from,
            to,
        },
    )
    .unwrap_err();
    assert_err_kind(&err, ErrKind::TableNotFound);
}

#[test]
fn apply_replace_constraint_no_match_errors() {
    let existing = idx("ix_existing", vec!["col"]);
    let mut schema = vec![table(
        "users",
        vec![col("col", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![existing.clone()],
    )];

    let from = idx("ix_nonexistent", vec!["other"]);
    let to = idx("ix_new", vec!["other"]);
    let err = apply_action(
        &mut schema,
        &MigrationAction::ReplaceConstraint {
            table: "users".into(),
            from,
            to,
        },
    )
    .unwrap_err();

    assert!(matches!(err, PlannerError::TableValidation(_)));
    assert_eq!(schema[0].constraints, vec![existing]);
}
