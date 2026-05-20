use super::*;

// Explicit coverage tests for lines that tarpaulin might miss in rstest
mod coverage_explicit {
    use super::*;

    #[test]
    fn delete_column_explicit() {
        // Covers lines 292-294: DeleteColumn action inside modified table loop
        let from = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("name", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            vec![],
        )];

        let to = vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(
            &plan.actions[0],
            MigrationAction::DeleteColumn { table, column }
            if table == "users" && column == "name"
        ));
    }

    #[test]
    fn add_column_explicit() {
        // Covers lines 359-362: AddColumn action inside modified table loop
        let from = vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
        )];

        let to = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("email", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(
            &plan.actions[0],
            MigrationAction::AddColumn { table, column, .. }
            if table == "users" && column.name == "email"
        ));
    }

    #[test]
    fn remove_constraint_explicit() {
        // Covers lines 370-372: RemoveConstraint action inside modified table loop
        let from = vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![idx("idx_users_id", vec!["id"])],
        )];

        let to = vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(
            &plan.actions[0],
            MigrationAction::RemoveConstraint { table, constraint }
            if table == "users" && matches!(constraint, TableConstraint::Index { name: Some(n), .. } if n == "idx_users_id")
        ));
    }

    #[test]
    fn add_constraint_explicit() {
        // Covers lines 378-380: AddConstraint action inside modified table loop
        let from = vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
        )];

        let to = vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![idx("idx_users_id", vec!["id"])],
        )];

        let plan = diff_schemas(&from, &to).unwrap();
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(
            &plan.actions[0],
            MigrationAction::AddConstraint { table, constraint }
            if table == "users" && matches!(constraint, TableConstraint::Index { name: Some(n), .. } if n == "idx_users_id")
        ));
    }
}

#[test]
fn test_sort_enum_default_dependencies_swaps_when_old_default_removed() {
    // Scenario: enum column "status" changes from [active, pending, done] → [active, done]
    // and default changes from 'pending' → 'active'.
    // The ModifyColumnDefault must come BEFORE ModifyColumnType.
    use vespertide_core::{ComplexColumnType, DefaultValue, EnumValues};

    let enum_type_old = ColumnType::Complex(ComplexColumnType::Enum {
        name: "status_enum".into(),
        values: EnumValues::String(vec!["active".into(), "pending".into(), "done".into()]),
    });
    let enum_type_new = ColumnType::Complex(ComplexColumnType::Enum {
        name: "status_enum".into(),
        values: EnumValues::String(vec!["active".into(), "done".into()]),
    });

    let from = vec![table(
        "orders",
        vec![{
            let mut c = col("status", enum_type_old);
            c.default = Some(DefaultValue::String("'pending'".into()));
            c
        }],
        vec![],
    )];
    let to = vec![table(
        "orders",
        vec![{
            let mut c = col("status", enum_type_new);
            c.default = Some(DefaultValue::String("'active'".into()));
            c
        }],
        vec![],
    )];

    let plan = diff_schemas(&from, &to).unwrap();

    // Should have both ModifyColumnDefault and ModifyColumnType
    let has_modify_default = plan
        .actions
        .iter()
        .any(|a| matches!(a, MigrationAction::ModifyColumnDefault { .. }));
    let has_modify_type = plan
        .actions
        .iter()
        .any(|a| matches!(a, MigrationAction::ModifyColumnType { .. }));
    assert!(has_modify_default, "Should have ModifyColumnDefault");
    assert!(has_modify_type, "Should have ModifyColumnType");

    // ModifyColumnDefault should come BEFORE ModifyColumnType
    let default_idx = plan
        .actions
        .iter()
        .position(|a| matches!(a, MigrationAction::ModifyColumnDefault { .. }))
        .unwrap();
    let type_idx = plan
        .actions
        .iter()
        .position(|a| matches!(a, MigrationAction::ModifyColumnType { .. }))
        .unwrap();
    assert!(
        default_idx < type_idx,
        "ModifyColumnDefault (idx={default_idx}) must come before ModifyColumnType (idx={type_idx})"
    );
}

#[test]
fn test_delete_column_from_existing_table() {
    // Simple column deletion to cover diff.rs line 339
    let from = vec![table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("name", ColumnType::Simple(SimpleColumnType::Text)),
            col("age", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        vec![],
    )];
    let to = vec![table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            // name and age deleted
        ],
        vec![],
    )];

    let plan = diff_schemas(&from, &to).unwrap();

    let delete_cols: Vec<&str> = plan
        .actions
        .iter()
        .filter_map(|a| match a {
            MigrationAction::DeleteColumn { column, .. } => Some(column.as_str()),
            _ => None,
        })
        .collect();

    assert_eq!(delete_cols.len(), 2);
    assert!(delete_cols.contains(&"name"));
    assert!(delete_cols.contains(&"age"));
}
