//! Foreign key carrying both referential actions.

use vespertide_core::schema::column::SimpleColumnType;
use vespertide_core::schema::constraint::TableConstraint;
use vespertide_core::{ReferenceAction, TableDef};

use super::{pk, simple};

/// The only fixture whose foreign key sets `ON UPDATE` as well as `ON DELETE`.
/// GORM, Prisma and Drizzle render both, Django renders `on_delete` alone
/// (its `ForeignKey` has no update action), and the remaining four drop them
/// — a spread only this fixture pins. The two actions differ so a backend
/// that emits one of them in the other's place is visible.
pub(crate) fn reference_actions() -> Vec<TableDef> {
    let users = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![simple("id", SimpleColumnType::Integer)],
        constraints: vec![pk(&["id"])],
    };
    let posts = TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![
            simple("id", SimpleColumnType::Integer),
            simple("user_id", SimpleColumnType::Integer),
        ],
        constraints: vec![
            pk(&["id"]),
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some(ReferenceAction::Cascade),
                on_update: Some(ReferenceAction::Restrict),
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    };
    [users, posts]
        .into_iter()
        .map(|t| t.normalize().expect("reference_actions normalizes"))
        .collect()
}
