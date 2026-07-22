mod enums;
mod render;
mod types;

use std::collections::HashSet;

use crate::orm::OrmExporter;
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

/// Render a complete `schema.prisma` file for all tables.
///
/// Output order: datasource → generator → (globally deduped) enum blocks → model blocks.
pub fn render_schema(tables: &[TableDef], provider: PrismaProvider) -> String {
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
    parts.push(format!(
        "datasource db {{\n  provider = \"{}\"\n}}",
        provider.as_datasource_str()
    ));

    parts.push("generator client {\n  provider = \"prisma-client-js\"\n}".to_string());

    parts.extend(enum_blocks);

    for table in tables {
        parts.push(render::render_model(table, tables, provider));
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
///
/// Uses the default provider (PostgreSQL) — provider-specific output goes
/// through [`render_schema`].
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
/// `datasource`/`generator` wrapper lives in [`render_schema`].
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
        let tables = vec![basic_single_pk()];
        let schema = render_schema(&tables, provider);

        assert!(schema.contains(expected_line));
        assert!(!schema.contains("output"));
    }

    #[test]
    fn render_schema_emits_shared_enum_block_once() {
        let t1 = crate::tests::fixtures::enum_shared();
        let mut t2 = crate::tests::fixtures::enum_shared();
        t2.name = "archived_documents".into();
        let schema = render_schema(&[t1, t2], PrismaProvider::Postgres);

        assert_eq!(schema.matches("enum DocStatus {").count(), 1);
    }
}
