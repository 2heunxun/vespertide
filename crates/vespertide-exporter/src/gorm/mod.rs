mod enums;
mod render;
mod types;

use crate::orm::OrmExporter;
use render::{imports_for, render_header, render_table_body};
use vespertide_config::DEFAULT_GORM_PACKAGE_NAME;
use vespertide_core::TableDef;

pub struct GormExporter;

impl OrmExporter for GormExporter {
    fn render_entity(&self, table: &TableDef) -> Result<String, String> {
        render_entity(table)
    }

    fn render_entity_with_schema(
        &self,
        table: &TableDef,
        schema: &[TableDef],
    ) -> Result<String, String> {
        render_entity_with_schema(table, schema)
    }
}

/// GORM exporter that honors `vespertide.json`'s `gorm` config section
/// (currently the effective Go package name — see
/// `VespertideConfig::gorm_package_name`, which resolves an explicit
/// `gorm.package_name` or infers one from the actual export directory —
/// emitted at the top of every file). Mirrors `seaorm::SeaOrmExporterWithConfig`.
pub struct GormExporterWithConfig<'a> {
    package_name: &'a str,
}

impl<'a> GormExporterWithConfig<'a> {
    /// `package_name` is the already-resolved effective package name (see
    /// `VespertideConfig::gorm_package_name`), not the raw `GormConfig`
    /// field — resolving requires the actual export directory, which the
    /// `GormConfig` alone doesn't know.
    pub fn new(package_name: &'a str) -> Self {
        Self { package_name }
    }

    pub fn render_entity(&self, table: &TableDef) -> Result<String, String> {
        Ok(render_entity_inner_with_package(
            table,
            &[],
            self.package_name,
        ))
    }

    pub fn render_entity_with_schema(
        &self,
        table: &TableDef,
        schema: &[TableDef],
    ) -> Result<String, String> {
        Ok(render_entity_inner_with_package(
            table,
            schema,
            self.package_name,
        ))
    }
}

/// Render a GORM entity for the given table definition.
pub fn render_entity(table: &TableDef) -> Result<String, String> {
    Ok(render_entity_inner(table, &[]))
}

/// Render a GORM entity with full schema context for reverse-relation (HasMany) generation.
pub fn render_entity_with_schema(table: &TableDef, schema: &[TableDef]) -> Result<String, String> {
    Ok(render_entity_inner(table, schema))
}

fn render_entity_inner(table: &TableDef, schema: &[TableDef]) -> String {
    render_entity_inner_with_package(table, schema, DEFAULT_GORM_PACKAGE_NAME)
}

fn render_entity_inner_with_package(
    table: &TableDef,
    schema: &[TableDef],
    package_name: &str,
) -> String {
    let mut lines = render_header(package_name, &imports_for(std::slice::from_ref(table)));
    lines.extend(render_table_body(table, schema));
    lines.join("\n")
}

/// Render a whole schema as one Go source file: a single `package` clause,
/// one import block covering every table, then each table's declarations.
/// Concatenating per-table files instead would repeat the `package` clause,
/// which Go rejects.
pub fn export(schema: &[TableDef]) -> Result<String, String> {
    let mut lines = render_header(DEFAULT_GORM_PACKAGE_NAME, &imports_for(schema));
    for (i, table) in schema.iter().enumerate() {
        if i > 0 {
            lines.push(String::new());
        }
        lines.extend(render_table_body(table, schema));
    }
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests;
