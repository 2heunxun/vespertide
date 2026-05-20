mod enums;
mod render;
mod types;

use crate::orm::OrmExporter;
use vespertide_core::TableDef;

pub use render::{export, render_entity};

pub struct SqlAlchemyExporter;

impl OrmExporter for SqlAlchemyExporter {
    fn render_entity(&self, table: &TableDef) -> Result<String, String> {
        render_entity(table)
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_edge_cases;
