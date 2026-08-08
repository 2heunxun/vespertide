use vespertide_core::TableDef;

use crate::{
    jpa::JpaExporter, prisma::PrismaExporter, seaorm::SeaOrmExporter,
    sqlalchemy::SqlAlchemyExporter, sqlmodel::SqlModelExporter,
};

/// Supported ORM targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
pub enum Orm {
    // clap derives value names in kebab-case, so the multi-word variants are
    // pinned to keep the `--orm` values the CLI has always accepted.
    #[cfg_attr(feature = "cli", value(name = "seaorm"))]
    SeaOrm,
    #[cfg_attr(feature = "cli", value(name = "sqlalchemy"))]
    SqlAlchemy,
    #[cfg_attr(feature = "cli", value(name = "sqlmodel"))]
    SqlModel,
    Jpa,
    Prisma,
}

impl Orm {
    /// Extension of the files this ORM's entities are written to.
    pub fn file_extension(self) -> &'static str {
        match self {
            Orm::SeaOrm => "rs",
            Orm::SqlAlchemy | Orm::SqlModel => "py",
            Orm::Jpa => "java",
            Orm::Prisma => "prisma",
        }
    }
}

/// Standardized exporter interface for all supported ORMs.
pub trait OrmExporter {
    fn render_entity(&self, table: &TableDef) -> Result<String, String>;

    /// Render entity with schema context for FK chain resolution.
    /// Default implementation ignores schema context.
    fn render_entity_with_schema(
        &self,
        table: &TableDef,
        _schema: &[TableDef],
    ) -> Result<String, String> {
        self.render_entity(table)
    }
}

/// Render a single table definition for the selected ORM.
pub fn render_entity(orm: Orm, table: &TableDef) -> Result<String, String> {
    match orm {
        Orm::SeaOrm => SeaOrmExporter.render_entity(table),
        Orm::SqlAlchemy => SqlAlchemyExporter.render_entity(table),
        Orm::SqlModel => SqlModelExporter.render_entity(table),
        Orm::Jpa => JpaExporter.render_entity(table),
        Orm::Prisma => PrismaExporter.render_entity(table),
    }
}

/// Render a single table definition with full schema context for FK chain resolution.
pub fn render_entity_with_schema(
    orm: Orm,
    table: &TableDef,
    schema: &[TableDef],
) -> Result<String, String> {
    match orm {
        Orm::SeaOrm => SeaOrmExporter.render_entity_with_schema(table, schema),
        Orm::SqlAlchemy => SqlAlchemyExporter.render_entity_with_schema(table, schema),
        Orm::SqlModel => SqlModelExporter.render_entity_with_schema(table, schema),
        Orm::Jpa => JpaExporter.render_entity_with_schema(table, schema),
        Orm::Prisma => PrismaExporter.render_entity_with_schema(table, schema),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::fixtures::basic_single_pk;
    use rstest::rstest;

    #[rstest]
    #[case::seaorm(Orm::SeaOrm)]
    #[case::sqlalchemy(Orm::SqlAlchemy)]
    #[case::sqlmodel(Orm::SqlModel)]
    #[case::jpa(Orm::Jpa)]
    #[case::prisma(Orm::Prisma)]
    fn dispatch_render_entity_succeeds(#[case] orm: Orm) {
        let table = basic_single_pk();
        assert!(render_entity(orm, &table).is_ok());
    }

    #[rstest]
    #[case::seaorm(Orm::SeaOrm)]
    #[case::sqlalchemy(Orm::SqlAlchemy)]
    #[case::sqlmodel(Orm::SqlModel)]
    #[case::jpa(Orm::Jpa)]
    #[case::prisma(Orm::Prisma)]
    fn dispatch_render_entity_with_schema_succeeds(#[case] orm: Orm) {
        let table = basic_single_pk();
        let schema = vec![table.clone()];
        assert!(render_entity_with_schema(orm, &table, &schema).is_ok());
    }

    #[rstest]
    #[case::seaorm(Orm::SeaOrm, "rs")]
    #[case::sqlalchemy(Orm::SqlAlchemy, "py")]
    #[case::sqlmodel(Orm::SqlModel, "py")]
    #[case::jpa(Orm::Jpa, "java")]
    #[case::prisma(Orm::Prisma, "prisma")]
    fn file_extension_matches_backend(#[case] orm: Orm, #[case] expected: &str) {
        assert_eq!(orm.file_extension(), expected);
    }

    /// The `--orm` values are user-facing, so the pinned names stay put.
    #[cfg(feature = "cli")]
    #[rstest]
    #[case::seaorm("seaorm", Orm::SeaOrm)]
    #[case::sqlalchemy("sqlalchemy", Orm::SqlAlchemy)]
    #[case::sqlmodel("sqlmodel", Orm::SqlModel)]
    #[case::jpa("jpa", Orm::Jpa)]
    #[case::prisma("prisma", Orm::Prisma)]
    fn value_enum_parses_cli_name(#[case] input: &str, #[case] expected: Orm) {
        assert_eq!(
            clap::ValueEnum::from_str(input, false),
            Ok::<Orm, String>(expected)
        );
    }
}
