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

/// GORM exporter that emits a caller-chosen `package` clause. Go expects that
/// name to match the directory the file lives in, so the CLI resolves it from
/// the real write target via `vespertide_config::go_package_name`.
pub struct GormExporterWithConfig<'a> {
    package_name: &'a str,
}

impl<'a> GormExporterWithConfig<'a> {
    pub fn new(package_name: &'a str) -> Self {
        Self { package_name }
    }

    /// [`export`] under the configured package name.
    pub fn export(&self, schema: &[TableDef]) -> Result<String, String> {
        Ok(export_with_package(schema, self.package_name))
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
    let mut lines = render_header(
        DEFAULT_GORM_PACKAGE_NAME,
        &imports_for(std::slice::from_ref(table)),
    );
    lines.extend(render_table_body(table, schema));
    lines.join("\n")
}

/// Render a whole schema as one Go source file: a single `package` clause,
/// one import block covering every table, then each table's declarations.
/// Concatenating per-table files instead would repeat the `package` clause,
/// which Go rejects.
pub fn export(schema: &[TableDef]) -> Result<String, String> {
    Ok(export_with_package(schema, DEFAULT_GORM_PACKAGE_NAME))
}

fn export_with_package(schema: &[TableDef], package_name: &str) -> String {
    let mut lines = render_header(package_name, &imports_for(schema));
    for (i, table) in schema.iter().enumerate() {
        if i > 0 {
            lines.push(String::new());
        }
        lines.extend(render_table_body(table, schema));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests;
