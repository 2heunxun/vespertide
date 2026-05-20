use sea_query::{Alias, Index};

use super::super::helpers::build_unique_constraint_name;
use super::super::types::BuiltQuery;

pub(super) fn build_unique(table: &str, name: Option<&str>, columns: &[String]) -> Vec<BuiltQuery> {
    let index_name = build_unique_constraint_name(table, columns, name);
    let mut idx = Index::create()
        .table(Alias::new(table))
        .name(&index_name)
        .unique()
        .to_owned();
    for col in columns {
        idx.col(Alias::new(col));
    }
    vec![BuiltQuery::CreateIndex(Box::new(idx))]
}
