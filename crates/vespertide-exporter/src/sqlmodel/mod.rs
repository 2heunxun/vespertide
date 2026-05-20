mod enums;
mod render;
mod types;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_edge_cases;

use crate::orm::OrmExporter;
use vespertide_core::TableDef;

pub use render::render_entity;

pub struct SqlModelExporter;

impl OrmExporter for SqlModelExporter {
    fn render_entity(&self, table: &TableDef) -> Result<String, String> {
        render_entity(table)
    }
}
