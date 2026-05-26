use super::*;
use insta::{assert_snapshot, with_settings};
use rstest::rstest;
use vespertide_core::schema::primary_key::PrimaryKeySyntax;
use vespertide_core::{
    ColumnDef, ColumnType, MigrationAction, ReferenceAction, SimpleColumnType, TableConstraint,
};

fn col(name: &str, ty: ColumnType) -> ColumnDef {
    ColumnDef::new(name, ty, true)
}

mod dispatch;
mod helpers;
mod naming;
