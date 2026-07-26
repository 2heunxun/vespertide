mod enums;
mod render;
mod types;

use std::collections::HashSet;

use crate::orm::OrmExporter;
use vespertide_core::TableDef;
use vespertide_core::schema::column::{ColumnType, ComplexColumnType, EnumValues};

pub struct PrismaExporter;

impl OrmExporter for PrismaExporter {
    fn render_entity(&self, table: &TableDef) -> Result<String, String> {
        Ok(render_entity(table))
    }

    fn render_entity_with_schema(
        &self,
        table: &TableDef,
        schema: &[TableDef],
    ) -> Result<String, String> {
        Ok(render_entity_with_schema(table, schema))
    }
}

/// Render every table into one Prisma schema file.
///
/// Output order: (globally deduped) enum blocks → model blocks.
///
/// No `datasource` or `generator` block is emitted: those describe the user's
/// project rather than their schema, and pinning a `provider` would make the
/// output backend-specific. Users pair this file with their own via Prisma's
/// multi-file schema directory, exactly as the other backends emit models only.
pub fn render_schema(tables: &[TableDef]) -> String {
    let mut seen_enums: HashSet<String> = HashSet::new();
    let mut parts: Vec<String> = Vec::new();
    for table in tables {
        for (name, values) in collect_table_enums(table) {
            if seen_enums.insert(name.to_string()) {
                parts.push(enums::render_enum(name, values));
            }
        }
    }

    for table in tables {
        parts.push(render::render_model(table, tables));
    }

    parts.join("\n\n") + "\n"
}

fn collect_table_enums(table: &TableDef) -> Vec<(&str, &EnumValues)> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for col in &table.columns {
        if let ColumnType::Complex(ComplexColumnType::Enum { name, values }) = &col.r#type
            && seen.insert(name.as_str())
        {
            result.push((name.as_str(), values));
        }
    }
    result
}

/// Render enum blocks + model block without schema context.
///
/// Passes the table itself as a one-element schema so that self-referential FK
/// back-relations are always emitted (Prisma requires both sides of a relation to
/// be present in the model, including self-referential ones).
pub fn render_entity(table: &TableDef) -> String {
    render_entity_with_schema(table, std::slice::from_ref(table))
}

/// Render enum blocks + model block with full schema context (includes back-relations).
pub fn render_entity_with_schema(table: &TableDef, schema: &[TableDef]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (name, values) in collect_table_enums(table) {
        parts.push(enums::render_enum(name, values));
    }
    parts.push(render::render_model(table, schema));
    parts.join("\n\n")
}

/// Multi-table entry point: render every table (enum + model blocks) with full
/// schema context and join them. Mirrors the other ORMs' `export` so the
/// cross-ORM test harness can dispatch Prisma through a single call. Unlike
/// [`render_schema`], enums are deduplicated per table rather than globally.
pub fn export(schema: &[TableDef]) -> Result<String, String> {
    Ok(schema
        .iter()
        .map(|table| render_entity_with_schema(table, schema))
        .collect::<Vec<_>>()
        .join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::fixtures::basic_single_pk;

    /// The file must stay usable under any provider, so it carries neither a
    /// `datasource`/`generator` block nor a backend-specific `@db.*` attribute.
    #[test]
    fn render_schema_emits_models_only() {
        let tables = vec![basic_single_pk()];
        let schema = render_schema(&tables);

        assert!(schema.starts_with("model "));
        assert!(!schema.contains("datasource"));
        assert!(!schema.contains("generator"));
        assert!(!schema.contains("provider"));
        assert!(!schema.contains("@db."));
    }

    #[test]
    fn render_schema_emits_shared_enum_block_once() {
        let t1 = crate::tests::fixtures::enum_shared();
        let mut t2 = crate::tests::fixtures::enum_shared();
        t2.name = "archived_documents".into();
        let schema = render_schema(&[t1, t2]);

        assert_eq!(schema.matches("enum DocStatus {").count(), 1);
    }
}
