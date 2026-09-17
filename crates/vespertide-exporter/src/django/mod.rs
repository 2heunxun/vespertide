mod enums;
mod render;
mod types;

use crate::orm::OrmExporter;
use vespertide_config::DjangoConfig;
use vespertide_core::TableDef;

pub use render::{
    export, export_with_config, render_entity, render_entity_with_schema,
    render_entity_with_schema_and_config,
};

pub struct DjangoExporter;

impl OrmExporter for DjangoExporter {
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

/// Django exporter that honors `vespertide.json`'s `django` config section
/// (currently an optional `app_label` written into every model's `Meta`
/// class). Mirrors `seaorm::SeaOrmExporterWithConfig`.
pub struct DjangoExporterWithConfig<'a> {
    config: &'a DjangoConfig,
}

impl<'a> DjangoExporterWithConfig<'a> {
    pub fn new(config: &'a DjangoConfig) -> Self {
        Self { config }
    }

    pub fn render_entity_with_schema(
        &self,
        table: &TableDef,
        schema: &[TableDef],
    ) -> Result<String, String> {
        render_entity_with_schema_and_config(table, schema, self.config.app_label())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use vespertide_core::ReferenceAction;
    use vespertide_core::schema::column::{ColumnType, SimpleColumnType};

    use super::types::{django_field_type, reference_action_str};

    #[rstest]
    #[case::small_int(SimpleColumnType::SmallInt, "models.SmallIntegerField")]
    #[case::integer(SimpleColumnType::Integer, "models.IntegerField")]
    #[case::big_int(SimpleColumnType::BigInt, "models.BigIntegerField")]
    #[case::real(SimpleColumnType::Real, "models.FloatField")]
    #[case::double_precision(SimpleColumnType::DoublePrecision, "models.FloatField")]
    #[case::text(SimpleColumnType::Text, "models.TextField")]
    #[case::xml(SimpleColumnType::Xml, "models.TextField")]
    #[case::boolean(SimpleColumnType::Boolean, "models.BooleanField")]
    #[case::date(SimpleColumnType::Date, "models.DateField")]
    #[case::time(SimpleColumnType::Time, "models.TimeField")]
    #[case::timestamp(SimpleColumnType::Timestamp, "models.DateTimeField")]
    #[case::timestamptz(SimpleColumnType::Timestamptz, "models.DateTimeField")]
    #[case::interval(SimpleColumnType::Interval, "models.DurationField")]
    #[case::bytea(SimpleColumnType::Bytea, "models.BinaryField")]
    #[case::uuid(SimpleColumnType::Uuid, "models.UUIDField")]
    #[case::json(SimpleColumnType::Json, "models.JSONField")]
    #[case::inet(SimpleColumnType::Inet, "models.GenericIPAddressField")]
    #[case::cidr(SimpleColumnType::Cidr, "models.GenericIPAddressField")]
    #[case::macaddr(SimpleColumnType::Macaddr, "models.CharField")]
    fn simple_types_map_to_field_classes(#[case] ty: SimpleColumnType, #[case] expected: &str) {
        assert_eq!(
            django_field_type(&ColumnType::Simple(ty), false, false),
            expected
        );
    }

    #[rstest]
    #[case::small_int(SimpleColumnType::SmallInt, "models.SmallAutoField")]
    #[case::integer(SimpleColumnType::Integer, "models.AutoField")]
    #[case::big_int(SimpleColumnType::BigInt, "models.BigAutoField")]
    fn auto_increment_primary_keys_map_to_auto_fields(
        #[case] ty: SimpleColumnType,
        #[case] expected: &str,
    ) {
        assert_eq!(
            django_field_type(&ColumnType::Simple(ty), true, true),
            expected
        );
    }

    #[rstest]
    #[case::cascade(ReferenceAction::Cascade, "models.CASCADE")]
    #[case::restrict(ReferenceAction::Restrict, "models.RESTRICT")]
    #[case::set_null(ReferenceAction::SetNull, "models.SET_NULL")]
    #[case::set_default(ReferenceAction::SetDefault, "models.SET_DEFAULT")]
    #[case::no_action(ReferenceAction::NoAction, "models.DO_NOTHING")]
    fn reference_actions_map_to_on_delete(#[case] action: ReferenceAction, #[case] expected: &str) {
        assert_eq!(reference_action_str(&action), expected);
    }
}
