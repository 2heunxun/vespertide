#![allow(clippy::module_inception)]

use super::*;
use rstest::rstest;
pub(super) use std::collections::BTreeSet;
pub(super) use vespertide_core::TableDef;
pub(super) use vespertide_core::{
    ColumnDef, ColumnType, MigrationAction, SimpleColumnType, TableConstraint,
    schema::{primary_key::PrimaryKeySyntax, str_or_bool::StrOrBoolOrArray},
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

fn table(
    name: &str,
    columns: Vec<ColumnDef>,
    constraints: Vec<vespertide_core::TableConstraint>,
) -> TableDef {
    TableDef {
        name: name.to_string(),
        description: None,
        columns,
        constraints,
    }
}

fn idx(name: &str, columns: Vec<&str>) -> TableConstraint {
    TableConstraint::Index {
        name: Some(name.to_string()),
        columns: columns
            .into_iter()
            .map(std::string::ToString::to_string)
            .collect(),
    }
}

mod basic;
mod column_changes;
mod constraint_performance;
mod constraint_removal;
mod coverage;
mod diff_tables;
mod enums;
mod fk_ordering;
mod inline_constraints;
mod ordering_sort;
mod primary_key_changes;
