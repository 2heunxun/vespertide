use super::*;
use vespertide_core::ReferenceAction;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn plan_with(actions: Vec<MigrationAction>) -> MigrationPlan {
    MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions,
    }
}

fn fk(
    name: Option<&str>,
    columns: Vec<&str>,
    ref_table: &str,
    ref_columns: Vec<&str>,
    on_delete: Option<ReferenceAction>,
    on_update: Option<ReferenceAction>,
) -> TableConstraint {
    TableConstraint::ForeignKey {
        name: name.map(ToString::to_string),
        columns: columns.into_iter().map(Into::into).collect(),
        ref_table: ref_table.into(),
        ref_columns: ref_columns.into_iter().map(Into::into).collect(),
        on_delete,
        on_update,
        orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
    }
}

fn replace(table: &str, from: TableConstraint, to: TableConstraint) -> MigrationAction {
    MigrationAction::ReplaceConstraint {
        table: table.into(),
        from,
        to,
    }
}

// ---------------------------------------------------------------------------
// Warned: on_delete changes
// ---------------------------------------------------------------------------

#[test]
fn on_delete_cascade_to_restrict_is_warned() {
    let plan = plan_with(vec![replace(
        "orders",
        fk(
            Some("fk_orders__user"),
            vec!["user_id"],
            "users",
            vec!["id"],
            Some(ReferenceAction::Cascade),
            None,
        ),
        fk(
            Some("fk_orders__user"),
            vec!["user_id"],
            "users",
            vec!["id"],
            Some(ReferenceAction::Restrict),
            None,
        ),
    )]);

    let warnings = find_fk_policy_changes(&plan);

    assert_eq!(warnings.len(), 1);
    let w = &warnings[0];
    assert_eq!(w.action_index, 0);
    assert_eq!(w.table, "orders");
    assert_eq!(w.constraint_name.as_deref(), Some("fk_orders__user"));
    assert_eq!(w.columns, vec!["user_id"]);
    assert_eq!(w.ref_table, "users");
    assert_eq!(w.ref_columns, vec!["id"]);

    let delete = w.on_delete_change.as_ref().expect("on_delete delta");
    assert_eq!(delete.before, Some(ReferenceAction::Cascade));
    assert_eq!(delete.after, Some(ReferenceAction::Restrict));

    assert!(w.on_update_change.is_none(), "on_update unchanged");
}

#[test]
fn on_delete_restrict_to_cascade_is_warned_with_reversed_delta() {
    let plan = plan_with(vec![replace(
        "orders",
        fk(
            None,
            vec!["user_id"],
            "users",
            vec!["id"],
            Some(ReferenceAction::Restrict),
            None,
        ),
        fk(
            None,
            vec!["user_id"],
            "users",
            vec!["id"],
            Some(ReferenceAction::Cascade),
            None,
        ),
    )]);

    let warnings = find_fk_policy_changes(&plan);

    assert_eq!(warnings.len(), 1);
    let delete = warnings[0]
        .on_delete_change
        .as_ref()
        .expect("on_delete delta");
    assert_eq!(delete.before, Some(ReferenceAction::Restrict));
    assert_eq!(delete.after, Some(ReferenceAction::Cascade));
}

#[test]
fn on_update_only_change_is_warned_separately() {
    let plan = plan_with(vec![replace(
        "orders",
        fk(
            Some("fk"),
            vec!["uid"],
            "users",
            vec!["id"],
            Some(ReferenceAction::Cascade),
            Some(ReferenceAction::NoAction),
        ),
        fk(
            Some("fk"),
            vec!["uid"],
            "users",
            vec!["id"],
            Some(ReferenceAction::Cascade),
            Some(ReferenceAction::Cascade),
        ),
    )]);

    let warnings = find_fk_policy_changes(&plan);
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0].on_delete_change.is_none(),
        "on_delete unchanged"
    );
    let upd = warnings[0]
        .on_update_change
        .as_ref()
        .expect("on_update delta");
    assert_eq!(upd.before, Some(ReferenceAction::NoAction));
    assert_eq!(upd.after, Some(ReferenceAction::Cascade));
}

#[test]
fn both_on_delete_and_on_update_change_are_reported_together() {
    let plan = plan_with(vec![replace(
        "orders",
        fk(
            Some("fk"),
            vec!["uid"],
            "users",
            vec!["id"],
            Some(ReferenceAction::Cascade),
            Some(ReferenceAction::Cascade),
        ),
        fk(
            Some("fk"),
            vec!["uid"],
            "users",
            vec!["id"],
            Some(ReferenceAction::SetNull),
            Some(ReferenceAction::Restrict),
        ),
    )]);

    let warnings = find_fk_policy_changes(&plan);
    assert_eq!(warnings.len(), 1);
    let w = &warnings[0];

    let delete = w.on_delete_change.as_ref().expect("on_delete delta");
    assert_eq!(delete.before, Some(ReferenceAction::Cascade));
    assert_eq!(delete.after, Some(ReferenceAction::SetNull));

    let update = w.on_update_change.as_ref().expect("on_update delta");
    assert_eq!(update.before, Some(ReferenceAction::Cascade));
    assert_eq!(update.after, Some(ReferenceAction::Restrict));
}

// ---------------------------------------------------------------------------
// Not warned: equivalent policies / unrelated changes
// ---------------------------------------------------------------------------

#[test]
fn no_policy_difference_is_not_warned() {
    let plan = plan_with(vec![replace(
        "orders",
        fk(
            Some("fk"),
            vec!["uid"],
            "users",
            vec!["id"],
            Some(ReferenceAction::Cascade),
            None,
        ),
        // Same policies — replacement is for some other reason (e.g. ref_columns reorder).
        fk(
            Some("fk"),
            vec!["uid"],
            "users",
            vec!["id"],
            Some(ReferenceAction::Cascade),
            None,
        ),
    )]);

    let warnings = find_fk_policy_changes(&plan);
    assert!(warnings.is_empty());
}

#[test]
fn none_vs_some_no_action_is_treated_as_unchanged() {
    // `None` and `Some(NoAction)` are semantically equivalent at the DB
    // level. Flipping between them must not trigger a false positive.
    let plan = plan_with(vec![replace(
        "orders",
        fk(Some("fk"), vec!["uid"], "users", vec!["id"], None, None),
        fk(
            Some("fk"),
            vec!["uid"],
            "users",
            vec!["id"],
            Some(ReferenceAction::NoAction),
            Some(ReferenceAction::NoAction),
        ),
    )]);

    let warnings = find_fk_policy_changes(&plan);
    assert!(warnings.is_empty());
}

#[test]
fn replace_non_fk_constraint_is_ignored() {
    // ReplaceConstraint can swap any constraint kind. Only FK→FK is in
    // scope for F30.
    let plan = plan_with(vec![replace(
        "users",
        TableConstraint::Unique {
            name: Some("uq".into()),
            columns: vec!["email".into()],
            strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                keep: vespertide_core::KeepPolicy::First,
            },
        },
        TableConstraint::Unique {
            name: Some("uq".into()),
            columns: vec!["email".into(), "tenant_id".into()],
            strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                keep: vespertide_core::KeepPolicy::First,
            },
        },
    )]);

    let warnings = find_fk_policy_changes(&plan);
    assert!(warnings.is_empty());
}

#[test]
fn add_constraint_and_remove_constraint_are_ignored() {
    let plan = plan_with(vec![
        MigrationAction::AddConstraint {
            table: "orders".into(),
            constraint: fk(
                Some("fk_new"),
                vec!["uid"],
                "users",
                vec!["id"],
                Some(ReferenceAction::Cascade),
                None,
            ),
        },
        MigrationAction::RemoveConstraint {
            table: "orders".into(),
            constraint: fk(
                Some("fk_old"),
                vec!["uid"],
                "users",
                vec!["id"],
                Some(ReferenceAction::Restrict),
                None,
            ),
        },
    ]);

    let warnings = find_fk_policy_changes(&plan);
    assert!(
        warnings.is_empty(),
        "F30 must only inspect ReplaceConstraint"
    );
}

// ---------------------------------------------------------------------------
// Aggregation + edge cases
// ---------------------------------------------------------------------------

#[test]
fn mixed_plan_returns_only_policy_changes_with_correct_indices() {
    let plan = plan_with(vec![
        // 0  unrelated AddConstraint
        MigrationAction::AddConstraint {
            table: "users".into(),
            constraint: TableConstraint::Index {
                name: Some("ix".into()),
                columns: vec!["email".into()],
            },
        },
        // 1  FK→FK with same policy (no warn)
        replace(
            "orders",
            fk(
                Some("fk_a"),
                vec!["uid"],
                "users",
                vec!["id"],
                Some(ReferenceAction::Cascade),
                None,
            ),
            fk(
                Some("fk_a"),
                vec!["uid"],
                "users",
                vec!["id"],
                Some(ReferenceAction::Cascade),
                None,
            ),
        ),
        // 2  FK→FK with on_delete change (WARN)
        replace(
            "audit",
            fk(
                Some("fk_b"),
                vec!["aid"],
                "actions",
                vec!["id"],
                Some(ReferenceAction::Cascade),
                None,
            ),
            fk(
                Some("fk_b"),
                vec!["aid"],
                "actions",
                vec!["id"],
                Some(ReferenceAction::Restrict),
                None,
            ),
        ),
        // 3  FK→FK with on_update change (WARN)
        replace(
            "audit",
            fk(
                Some("fk_c"),
                vec!["uid"],
                "users",
                vec!["id"],
                None,
                Some(ReferenceAction::Cascade),
            ),
            fk(
                Some("fk_c"),
                vec!["uid"],
                "users",
                vec!["id"],
                None,
                Some(ReferenceAction::SetNull),
            ),
        ),
    ]);

    let warnings = find_fk_policy_changes(&plan);

    assert_eq!(warnings.len(), 2);
    assert_eq!(warnings[0].action_index, 2);
    assert!(warnings[0].on_delete_change.is_some());
    assert!(warnings[0].on_update_change.is_none());
    assert_eq!(warnings[1].action_index, 3);
    assert!(warnings[1].on_delete_change.is_none());
    assert!(warnings[1].on_update_change.is_some());
}

#[test]
fn empty_plan_returns_empty_warnings() {
    let plan = plan_with(vec![]);
    assert!(find_fk_policy_changes(&plan).is_empty());
}

#[test]
fn render_reference_action_covers_all_known_variants() {
    assert_eq!(
        render_reference_action(Some(&ReferenceAction::Cascade)),
        "CASCADE"
    );
    assert_eq!(
        render_reference_action(Some(&ReferenceAction::Restrict)),
        "RESTRICT"
    );
    assert_eq!(
        render_reference_action(Some(&ReferenceAction::SetNull)),
        "SET NULL"
    );
    assert_eq!(
        render_reference_action(Some(&ReferenceAction::SetDefault)),
        "SET DEFAULT"
    );
    assert_eq!(
        render_reference_action(Some(&ReferenceAction::NoAction)),
        "NO ACTION"
    );
    assert_eq!(render_reference_action(None), "NO ACTION");
}
