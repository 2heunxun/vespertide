mod enums;
mod render;
mod types;

use std::collections::HashSet;

use crate::orm::OrmExporter;
use vespertide_config::PrismaConfig;
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

/// Prisma exporter with configuration support.
///
/// Assembles a complete `schema.prisma` file from a full table list.
pub struct PrismaExporterWithConfig<'a> {
    pub config: &'a PrismaConfig,
}

impl<'a> PrismaExporterWithConfig<'a> {
    pub fn new(config: &'a PrismaConfig) -> Self {
        Self { config }
    }

    /// Render a complete `schema.prisma` file for all tables.
    ///
    /// Output order: datasource → generator → (globally deduped) enum blocks → model blocks.
    pub fn render_schema(&self, tables: &[TableDef]) -> String {
        let mut seen_enums: HashSet<String> = HashSet::new();
        let mut enum_blocks: Vec<String> = Vec::new();
        for table in tables {
            for (name, values) in collect_table_enums(table) {
                if seen_enums.insert(name.to_string()) {
                    enum_blocks.push(enums::render_enum(name, values));
                }
            }
        }

        let mut parts: Vec<String> = Vec::new();

        let provider = self.config.provider();
        let mut datasource = vec![
            "datasource db {".to_string(),
            format!("  provider = \"{provider}\""),
            "  url      = env(\"DATABASE_URL\")".to_string(),
        ];
        if let Some(rm) = self.config.relation_mode() {
            datasource.push(format!("  relationMode = \"{rm}\""));
        }
        datasource.push("}".to_string());
        parts.push(datasource.join("\n"));

        let mut generator = vec![
            "generator client {".to_string(),
            "  provider = \"prisma-client-js\"".to_string(),
        ];
        if let Some(output) = self.config.client_output() {
            generator.push(format!("  output   = \"{output}\""));
        }
        generator.push("}".to_string());
        parts.push(generator.join("\n"));

        parts.extend(enum_blocks);

        for table in tables {
            parts.push(render::render_model(table, tables));
        }

        parts.join("\n\n") + "\n"
    }
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

/// Render enum blocks + model block without schema context (no back-relations).
pub fn render_entity(table: &TableDef) -> String {
    render_entity_with_schema(table, &[])
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
/// cross-ORM test harness can dispatch Prisma through a single call. The
/// `datasource`/`generator` wrapper lives in [`PrismaExporterWithConfig`].
pub fn export(schema: &[TableDef]) -> Result<String, String> {
    Ok(schema
        .iter()
        .map(|table| render_entity_with_schema(table, schema))
        .collect::<Vec<_>>()
        .join("\n\n"))
}

/// Test-only accessor for the internal `to_pascal_case` helper, mirroring the
/// other ORM backends so the cross-ORM consolidation test can exercise it
/// without making the helper generally public.
#[cfg(test)]
pub fn to_pascal_case_for_tests(s: &str) -> String {
    enums::to_pascal_case(s)
}
