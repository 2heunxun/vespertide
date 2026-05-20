mod render;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_edge_cases;
mod types;

use crate::orm::OrmExporter;
use crate::parallel_config::{JPA_EXPORT_PAR_TABLE_MIN_LEN, JPA_EXPORT_PAR_TABLE_THRESHOLD};
use rayon::prelude::*;
use vespertide_core::TableDef;

use self::types::UsedImports;

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

/// Render JPA entities for a schema, using parallel rendering for larger schemas.
pub fn render_entities(schema: &[TableDef]) -> Result<Vec<String>, String> {
    let rendered = if schema.len() < JPA_EXPORT_PAR_TABLE_THRESHOLD {
        schema
            .iter()
            .map(render::render_entity_with_imports)
            .collect::<Vec<_>>()
    } else {
        schema
            .par_iter()
            .with_min_len(JPA_EXPORT_PAR_TABLE_MIN_LEN)
            .map(render::render_entity_with_imports)
            .collect::<Vec<_>>()
    };

    Ok(merge_rendered_entities(rendered).entities)
}

struct RenderedEntities {
    entities: Vec<String>,
    _imports: UsedImports,
}

fn merge_rendered_entities(rendered: Vec<(String, UsedImports)>) -> RenderedEntities {
    let mut entities = Vec::with_capacity(rendered.len());
    let mut imports = UsedImports::default();

    for (entity, local_imports) in rendered {
        entities.push(entity);
        imports.merge(local_imports);
    }

    RenderedEntities {
        entities,
        _imports: imports,
    }
}
