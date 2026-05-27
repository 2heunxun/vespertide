use super::*;

// Direct unit tests for sort_create_before_add_constraint and compare_actions_for_create_order
mod sort_create_before_add_constraint_tests {
    use super::*;
    use crate::diff::ordering::{
        compare_actions_for_create_order, sort_create_before_add_constraint,
    };
    use std::cmp::Ordering;

    fn make_add_column(table: &str, col: &str) -> MigrationAction {
        MigrationAction::AddColumn {
            table: table.into(),
            column: Box::new(ColumnDef {
                name: col.into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }),
            fill_with: None,
        }
    }

    fn make_create_table(name: &str) -> MigrationAction {
        MigrationAction::CreateTable {
            table: name.into(),
            columns: vec![],
            constraints: vec![],
        }
    }

    fn make_add_fk(table: &str, ref_table: &str) -> MigrationAction {
        MigrationAction::AddConstraint {
                    table: table.into(),
                    constraint: TableConstraint::ForeignKey {
                        name: None,
                        columns: vec!["fk_col".into()],
                        ref_table: ref_table.into(),
                        ref_columns: vec!["id".into()],
                        on_delete: None,
                        on_update: None,
                    },
                }
    }

    /// Test line 218: (false, true, _, _) - a is NOT `CreateTable`, b IS `CreateTable`
    /// Direct test of comparison function
    #[test]
    fn test_compare_non_create_vs_create() {
        let created_tables: BTreeSet<String> = ["roles".to_string()].into_iter().collect();

        let add_col = make_add_column("users", "name");
        let create_table = make_create_table("roles");

        // a=AddColumn (non-create), b=CreateTable (create) -> Greater (b comes first)
        let result = compare_actions_for_create_order(&add_col, &create_table, &created_tables);
        assert_eq!(
            result,
            Ordering::Greater,
            "Non-CreateTable vs CreateTable should return Greater"
        );
    }

    /// Test line 216: (true, false, _, _) - a IS `CreateTable`, b is NOT `CreateTable`
    #[test]
    fn test_compare_create_vs_non_create() {
        let created_tables: BTreeSet<String> = ["roles".to_string()].into_iter().collect();

        let create_table = make_create_table("roles");
        let add_col = make_add_column("users", "name");

        // a=CreateTable (create), b=AddColumn (non-create) -> Less (a comes first)
        let result = compare_actions_for_create_order(&create_table, &add_col, &created_tables);
        assert_eq!(
            result,
            Ordering::Less,
            "CreateTable vs Non-CreateTable should return Less"
        );
    }

    /// Test line 214: (true, true, _, _) - both `CreateTable`
    #[test]
    fn test_compare_create_vs_create() {
        let created_tables: BTreeSet<String> = ["roles".to_string(), "categories".to_string()]
            .into_iter()
            .collect();

        let create1 = make_create_table("roles");
        let create2 = make_create_table("categories");

        // Both CreateTable -> Equal (maintain original order)
        let result = compare_actions_for_create_order(&create1, &create2, &created_tables);
        assert_eq!(
            result,
            Ordering::Equal,
            "CreateTable vs CreateTable should return Equal"
        );
    }

    /// Test line 221: (false, false, true, false) - neither `CreateTable`, a refs created, b doesn't
    #[test]
    fn test_compare_refs_vs_non_refs() {
        let created_tables: BTreeSet<String> = ["roles".to_string()].into_iter().collect();

        let add_fk = make_add_fk("users", "roles"); // refs created
        let add_col = make_add_column("posts", "title"); // doesn't ref

        // a refs created, b doesn't -> Greater (a comes after)
        let result = compare_actions_for_create_order(&add_fk, &add_col, &created_tables);
        assert_eq!(
            result,
            Ordering::Greater,
            "FK-ref vs non-ref should return Greater"
        );
    }

    /// Test line 223: (false, false, false, true) - neither `CreateTable`, a doesn't ref, b refs
    #[test]
    fn test_compare_non_refs_vs_refs() {
        let created_tables: BTreeSet<String> = ["roles".to_string()].into_iter().collect();

        let add_col = make_add_column("posts", "title"); // doesn't ref
        let add_fk = make_add_fk("users", "roles"); // refs created

        // a doesn't ref, b refs -> Less (b comes after, a comes first)
        let result = compare_actions_for_create_order(&add_col, &add_fk, &created_tables);
        assert_eq!(
            result,
            Ordering::Less,
            "Non-ref vs FK-ref should return Less"
        );
    }

    /// Test line 225: (false, false, _, _) - neither `CreateTable`, both don't ref
    #[test]
    fn test_compare_non_refs_vs_non_refs() {
        let created_tables: BTreeSet<String> = ["roles".to_string()].into_iter().collect();

        let add_col1 = make_add_column("users", "name");
        let add_col2 = make_add_column("posts", "title");

        // Both don't ref -> Equal
        let result = compare_actions_for_create_order(&add_col1, &add_col2, &created_tables);
        assert_eq!(
            result,
            Ordering::Equal,
            "Non-ref vs non-ref should return Equal"
        );
    }

    /// Test line 225: (false, false, _, _) - neither `CreateTable`, both ref created
    #[test]
    fn test_compare_refs_vs_refs() {
        let created_tables: BTreeSet<String> = ["roles".to_string(), "categories".to_string()]
            .into_iter()
            .collect();

        let add_fk1 = make_add_fk("users", "roles");
        let add_fk2 = make_add_fk("posts", "categories");

        // Both ref -> Equal
        let result = compare_actions_for_create_order(&add_fk1, &add_fk2, &created_tables);
        assert_eq!(
            result,
            Ordering::Equal,
            "FK-ref vs FK-ref should return Equal"
        );
    }

    /// Integration test: sort function works correctly
    #[test]
    fn test_sort_integration() {
        let mut actions = vec![
            make_add_column("t1", "c1"),
            make_add_fk("users", "roles"),
            make_create_table("roles"),
        ];

        sort_create_before_add_constraint(&mut actions);

        // CreateTable first, AddColumn second, AddConstraint FK last
        assert!(matches!(&actions[0], MigrationAction::CreateTable { .. }));
        assert!(matches!(&actions[1], MigrationAction::AddColumn { .. }));
        assert!(matches!(&actions[2], MigrationAction::AddConstraint { .. }));
    }
}
