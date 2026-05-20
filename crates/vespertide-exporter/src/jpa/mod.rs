mod render;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_edge_cases;
mod types;

use crate::orm::OrmExporter;
use vespertide_core::TableDef;

pub struct JpaExporter;

impl OrmExporter for JpaExporter {
    fn render_entity(&self, table: &TableDef) -> Result<String, String> {
        render_entity(table)
    }
}

/// Render a JPA entity for the given table definition.
pub fn render_entity(table: &TableDef) -> Result<String, String> {
    Ok(render::render_entity_inner(table))
}
