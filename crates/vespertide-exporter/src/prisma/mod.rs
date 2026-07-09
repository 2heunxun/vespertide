mod enums;
mod render;
mod types;

use std::collections::HashSet;

use crate::orm::OrmExporter;
use vespertide_config::PrismaConfig;
use vespertide_core::TableDef;
use vespertide_core::schema::column::{ColumnType, ComplexColumnType, EnumValues};

pub struct PrismaExporter;

/// Target database provider for the generated `schema.prisma`.
///
/// A Prisma schema declares exactly one datasource provider, and native
/// `@db.*` type attributes are validated against that provider's connector —
/// so the same tables render differently per backend. Mirrors the backends
/// supported by `vespertide-query`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrismaProvider {
    #[default]
    Postgres,
    MySql,
    Sqlite,
}

impl PrismaProvider {
    /// The `provider = "..."` string used in the `datasource` block.
    pub fn as_datasource_str(self) -> &'static str {
        match self {
            PrismaProvider::Postgres => "postgresql",
            PrismaProvider::MySql => "mysql",
            PrismaProvider::Sqlite => "sqlite",
        }
    }
}

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
    pub provider: PrismaProvider,
}

impl<'a> PrismaExporterWithConfig<'a> {
    pub fn new(config: &'a PrismaConfig) -> Self {
        Self::with_provider(config, PrismaProvider::default())
    }

    pub fn with_provider(config: &'a PrismaConfig, provider: PrismaProvider) -> Self {
        Self { config, provider }
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

        // No `url` here: current Prisma moved connection config out of the
        // schema file (`prisma.config.ts`), which also matches vespertide's
        // models-only scope.
        let mut datasource = vec![
            "datasource db {".to_string(),
            format!("  provider = \"{}\"", self.provider.as_datasource_str()),
        ];
        if let Some(rm) = self.config.relation_mode() {
            datasource.push(format!("  relationMode = \"{}\"", rm.as_str()));
        }
        datasource.push("}".to_string());
        parts.push(datasource.join("\n"));

        parts.push("generator client {\n  provider = \"prisma-client-js\"\n}".to_string());

        parts.extend(enum_blocks);

        for table in tables {
            parts.push(render::render_model(table, tables, self.provider));
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

/// Render enum blocks + model block without schema context.
///
/// Passes the table itself as a one-element schema so that self-referential FK
/// back-relations are always emitted (Prisma requires both sides of a relation to
/// be present in the model, including self-referential ones).
pub fn render_entity(table: &TableDef) -> String {
    render_entity_with_schema(table, std::slice::from_ref(table))
}

/// Render enum blocks + model block with full schema context (includes back-relations).
///
/// Uses the default provider (PostgreSQL) — provider-specific output goes
/// through [`PrismaExporterWithConfig::with_provider`].
pub fn render_entity_with_schema(table: &TableDef, schema: &[TableDef]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (name, values) in collect_table_enums(table) {
        parts.push(enums::render_enum(name, values));
    }
    parts.push(render::render_model(
        table,
        schema,
        PrismaProvider::default(),
    ));
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

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::tests::fixtures::basic_single_pk;

    #[rstest]
    #[case::postgres(PrismaProvider::Postgres, "provider = \"postgresql\"")]
    #[case::mysql(PrismaProvider::MySql, "provider = \"mysql\"")]
    #[case::sqlite(PrismaProvider::Sqlite, "provider = \"sqlite\"")]
    fn render_schema_emits_configured_provider(
        #[case] provider: PrismaProvider,
        #[case] expected_line: &str,
    ) {
        let config = PrismaConfig::default();
        let tables = vec![basic_single_pk()];
        let schema =
            PrismaExporterWithConfig::with_provider(&config, provider).render_schema(&tables);

        assert!(schema.contains(expected_line));
        assert!(!schema.contains("output"));
        assert!(!schema.contains("relationMode"));
    }

    #[test]
    fn new_defaults_to_postgresql_provider() {
        let config = PrismaConfig::default();
        let tables = vec![basic_single_pk()];
        let schema = PrismaExporterWithConfig::new(&config).render_schema(&tables);

        assert!(schema.contains("provider = \"postgresql\""));
    }

    #[test]
    fn render_schema_emits_relation_mode_when_configured() {
        // `PrismaConfig` is `#[non_exhaustive]` in a different crate, so it can't be
        // built with a struct literal here — go through its real construction path
        // (JSON deserialization, same as loading `vespertide.json`) instead.
        let config: PrismaConfig = serde_json::from_str(r#"{"relationMode":"prisma"}"#).unwrap();
        let tables = vec![basic_single_pk()];
        let schema = PrismaExporterWithConfig::new(&config).render_schema(&tables);

        assert!(schema.contains("relationMode = \"prisma\""));
    }
}
