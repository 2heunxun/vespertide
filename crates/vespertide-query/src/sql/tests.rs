use super::*;
use insta::{assert_snapshot, with_settings};
use rstest::rstest;
use vespertide_core::schema::primary_key::PrimaryKeySyntax;
use vespertide_core::{
    ColumnDef, ColumnType, MigrationAction, ReferenceAction, SimpleColumnType, TableConstraint,
};

fn col(name: &str, ty: ColumnType) -> ColumnDef {
    ColumnDef {
        name: name.to_string(),
        r#type: ty,
        nullable: true,
        default: None,
        comment: None,
        primary_key: None,
        unique: None,
        index: None,
        foreign_key: None,
    }
}

include!("tests/dispatch.rs");
include!("tests/naming.rs");
