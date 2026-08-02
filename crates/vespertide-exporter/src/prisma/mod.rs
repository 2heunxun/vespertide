mod enums;
mod render;
mod types;

use std::collections::HashSet;

use crate::orm::OrmExporter;
use vespertide_core::TableDef;

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
    let ambiguous = enums::ambiguous_enum_identifiers(tables);
    let mut seen_enums: HashSet<String> = HashSet::new();
    let mut parts: Vec<String> = Vec::new();
    for table in tables {
        for (name, values) in enums::collect_table_enums(table) {
            let identifier = enums::enum_identifier(table.name.as_str(), name, &ambiguous);
            if seen_enums.insert(identifier.clone()) {
                parts.push(enums::render_enum(&identifier, values));
            }
        }
    }

    for table in tables {
        parts.push(render::render_model(table, tables, &ambiguous));
    }

    parts.join("\n\n") + "\n"
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
    let ambiguous = enums::ambiguous_enum_identifiers(schema);
    let mut parts: Vec<String> = Vec::new();
    for (name, values) in enums::collect_table_enums(table) {
        let identifier = enums::enum_identifier(table.name.as_str(), name, &ambiguous);
        parts.push(enums::render_enum(&identifier, values));
    }
    parts.push(render::render_model(table, schema, &ambiguous));
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

    /// Two tables may declare the same enum name with different values; the SQL
    /// layer keeps them apart as `{table}_{enum}` types, and a single Prisma file
    /// must do the same or both columns silently get the first table's values.
    #[test]
    fn render_schema_qualifies_same_named_enums_that_differ() {
        let orders = table_with_enum("orders", "status", &["new", "paid"]);
        let tickets = table_with_enum("tickets", "status", &["open", "closed"]);
        let schema = render_schema(&[orders, tickets]);

        assert!(schema.contains("enum OrdersStatus {"));
        assert!(schema.contains("enum TicketsStatus {"));
        assert!(!schema.contains("enum Status {"));
        assert!(schema.contains("  st OrdersStatus"));
        assert!(schema.contains("  st TicketsStatus"));
    }

    /// Distinct enum names can collapse onto one `PascalCase` identifier, so the
    /// clash has to be judged after the conversion, not on the declared name.
    #[test]
    fn render_schema_qualifies_enums_whose_names_collapse_to_one_identifier() {
        let orders = table_with_enum("orders", "doc_status", &["new", "paid"]);
        let tickets = table_with_enum("tickets", "docStatus", &["open", "closed"]);
        let schema = render_schema(&[orders, tickets]);

        assert!(schema.contains("enum OrdersDocStatus {"));
        assert!(schema.contains("enum TicketsDocStatus {"));
        assert!(schema.contains("  st OrdersDocStatus"));
        assert!(schema.contains("  st TicketsDocStatus"));
    }

    fn table_with_enum(name: &str, enum_name: &str, values: &[&str]) -> TableDef {
        use vespertide_core::schema::column::{ColumnType, ComplexColumnType, EnumValues};
        use vespertide_core::schema::primary_key::PrimaryKeySyntax;
        use vespertide_core::{ColumnDef, SimpleColumnType};

        TableDef {
            name: name.into(),
            description: None,
            columns: vec![
                ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false)
                    .primary_key(PrimaryKeySyntax::Bool(true)),
                ColumnDef::new(
                    "st",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: enum_name.into(),
                        values: EnumValues::String(
                            values.iter().copied().map(Into::into).collect(),
                        ),
                    }),
                    false,
                ),
            ],
            constraints: vec![],
        }
        .normalize()
        .expect("fixture table normalizes")
    }
}
