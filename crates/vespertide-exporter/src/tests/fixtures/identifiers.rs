//! Names that collide, or need escaping, once mapped into a host language.

use vespertide_core::schema::column::SimpleColumnType;
use vespertide_core::schema::constraint::TableConstraint;
use vespertide_core::{ReferenceAction, TableDef};

use super::{fk, nullable_simple, pk, simple};

/// Relation field names that run into a struct's own columns: a composite FK
/// whose target struct name is already taken by two columns (`order_regions`,
/// `order_regions2`), a has-many whose pluralized source name is a column
/// (`users.posts`), and a self-reference, whose reverse side only appears when
/// the table is rendered with itself in the schema.
pub(crate) fn relation_field_names() -> Vec<TableDef> {
    let order_regions = TableDef {
        name: "order_regions".into(),
        description: None,
        columns: vec![
            simple("order_id", SimpleColumnType::Integer),
            simple("region_id", SimpleColumnType::Integer),
        ],
        constraints: vec![pk(&["order_id", "region_id"])],
    };
    let order_items = TableDef {
        name: "order_items".into(),
        description: None,
        columns: vec![
            simple("id", SimpleColumnType::Integer),
            simple("order_id", SimpleColumnType::Integer),
            simple("region_id", SimpleColumnType::Integer),
            simple("order_regions", SimpleColumnType::Text),
            simple("order_regions2", SimpleColumnType::Text),
        ],
        constraints: vec![
            pk(&["id"]),
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["order_id".into(), "region_id".into()],
                ref_table: "order_regions".into(),
                ref_columns: vec!["order_id".into(), "region_id".into()],
                on_delete: Some(ReferenceAction::Cascade),
                on_update: Some(ReferenceAction::Restrict),
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    };
    let users = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            simple("id", SimpleColumnType::Integer),
            simple("posts", SimpleColumnType::Text),
        ],
        constraints: vec![pk(&["id"])],
    };
    let posts = TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![
            simple("id", SimpleColumnType::Integer),
            simple("user_id", SimpleColumnType::Integer),
        ],
        constraints: vec![pk(&["id"]), fk(&["user_id"], "users", &["id"])],
    };
    let categories = TableDef {
        name: "categories".into(),
        description: None,
        columns: vec![
            simple("id", SimpleColumnType::Integer),
            nullable_simple("parent_id", SimpleColumnType::Integer),
        ],
        constraints: vec![pk(&["id"]), fk(&["parent_id"], "categories", &["id"])],
    };
    [order_regions, order_items, users, posts, categories]
        .into_iter()
        .map(|t| t.normalize().expect("relation_field_names normalizes"))
        .collect()
}
