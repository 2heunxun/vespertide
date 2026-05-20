use super::*;

#[test]
fn test_parse_fill_with_args() {
    let args = vec![
        "users.email=default@example.com".to_string(),
        "orders.status=pending".to_string(),
    ];
    let result = parse_fill_with_args(&args);

    assert_eq!(result.len(), 2);
    assert_eq!(
        result.get(&("users".to_string(), "email".to_string())),
        Some(&"default@example.com".to_string())
    );
    assert_eq!(
        result.get(&("orders".to_string(), "status".to_string())),
        Some(&"pending".to_string())
    );
}

#[test]
fn test_parse_fill_with_args_invalid_format() {
    let args = vec![
        "invalid_format".to_string(),
        "no_equals_sign".to_string(),
        "users.email=valid".to_string(),
    ];
    let result = parse_fill_with_args(&args);

    // Only the valid one should be parsed
    assert_eq!(result.len(), 1);
    assert_eq!(
        result.get(&("users".to_string(), "email".to_string())),
        Some(&"valid".to_string())
    );
}

#[test]
fn test_apply_fill_with_to_plan_add_column() {
    use vespertide_core::MigrationPlan;

    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(ColumnDef {
                name: "email".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }),
            fill_with: None,
        }],
    };

    let mut fill_values = HashMap::new();
    fill_values.insert(
        ("users".to_string(), "email".to_string()),
        "'default@example.com'".to_string(),
    );

    apply_fill_with_to_plan(&mut plan, &fill_values);

    match &plan.actions[0] {
        MigrationAction::AddColumn { fill_with, .. } => {
            assert_eq!(fill_with, &Some("'default@example.com'".to_string()));
        }
        _ => panic!("Expected AddColumn action"),
    }
}

#[test]
fn test_apply_fill_with_to_plan_modify_column_nullable() {
    use vespertide_core::MigrationPlan;

    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::ModifyColumnNullable {
            table: "users".into(),
            column: "status".into(),
            nullable: false,
            fill_with: None,
            delete_null_rows: None,
        }],
    };

    let mut fill_values = HashMap::new();
    fill_values.insert(
        ("users".to_string(), "status".to_string()),
        "'active'".to_string(),
    );

    apply_fill_with_to_plan(&mut plan, &fill_values);

    match &plan.actions[0] {
        MigrationAction::ModifyColumnNullable { fill_with, .. } => {
            assert_eq!(fill_with, &Some("'active'".to_string()));
        }
        _ => panic!("Expected ModifyColumnNullable action"),
    }
}

#[test]
fn test_apply_fill_with_to_plan_skips_existing_fill_with() {
    use vespertide_core::MigrationPlan;

    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(ColumnDef {
                name: "email".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }),
            fill_with: Some("'existing@example.com'".to_string()),
        }],
    };

    let mut fill_values = HashMap::new();
    fill_values.insert(
        ("users".to_string(), "email".to_string()),
        "'new@example.com'".to_string(),
    );

    apply_fill_with_to_plan(&mut plan, &fill_values);

    // Should keep existing value, not replace with new
    match &plan.actions[0] {
        MigrationAction::AddColumn { fill_with, .. } => {
            assert_eq!(fill_with, &Some("'existing@example.com'".to_string()));
        }
        _ => panic!("Expected AddColumn action"),
    }
}

#[test]
fn test_apply_fill_with_to_plan_no_match() {
    use vespertide_core::MigrationPlan;

    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(ColumnDef {
                name: "email".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }),
            fill_with: None,
        }],
    };

    let mut fill_values = HashMap::new();
    fill_values.insert(
        ("orders".to_string(), "status".to_string()),
        "'pending'".to_string(),
    );

    apply_fill_with_to_plan(&mut plan, &fill_values);

    // Should remain None since no match
    match &plan.actions[0] {
        MigrationAction::AddColumn { fill_with, .. } => {
            assert_eq!(fill_with, &None);
        }
        _ => panic!("Expected AddColumn action"),
    }
}

#[test]
fn test_apply_fill_with_to_plan_multiple_actions() {
    use vespertide_core::MigrationPlan;

    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![
            MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: None,
            },
            MigrationAction::ModifyColumnNullable {
                table: "orders".into(),
                column: "status".into(),
                nullable: false,
                fill_with: None,
                delete_null_rows: None,
            },
        ],
    };

    let mut fill_values = HashMap::new();
    fill_values.insert(
        ("users".to_string(), "email".to_string()),
        "'user@example.com'".to_string(),
    );
    fill_values.insert(
        ("orders".to_string(), "status".to_string()),
        "'pending'".to_string(),
    );

    apply_fill_with_to_plan(&mut plan, &fill_values);

    match &plan.actions[0] {
        MigrationAction::AddColumn { fill_with, .. } => {
            assert_eq!(fill_with, &Some("'user@example.com'".to_string()));
        }
        _ => panic!("Expected AddColumn action"),
    }

    match &plan.actions[1] {
        MigrationAction::ModifyColumnNullable { fill_with, .. } => {
            assert_eq!(fill_with, &Some("'pending'".to_string()));
        }
        _ => panic!("Expected ModifyColumnNullable action"),
    }
}

#[test]
fn test_apply_fill_with_to_plan_other_actions_ignored() {
    use vespertide_core::MigrationPlan;

    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::DeleteColumn {
            table: "users".into(),
            column: "old_column".into(),
        }],
    };

    let mut fill_values = HashMap::new();
    fill_values.insert(
        ("users".to_string(), "old_column".to_string()),
        "'value'".to_string(),
    );

    // Should not panic or modify anything
    apply_fill_with_to_plan(&mut plan, &fill_values);

    match &plan.actions[0] {
        MigrationAction::DeleteColumn { table, column } => {
            assert_eq!(table, "users");
            assert_eq!(column, "old_column");
        }
        _ => panic!("Expected DeleteColumn action"),
    }
}
