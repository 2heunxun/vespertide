mod render;
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

/// Test-only accessor for the JPA `to_pascal_case` helper.
///
/// Allows the cross-ORM consolidation test in `crate::tests` to exercise
/// the JPA naming helper without making it `pub(crate)` for the entire
/// crate.
#[cfg(test)]
pub(crate) fn to_pascal_case_for_tests(s: &str) -> String {
    render::to_pascal_case(s)
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

#[cfg(test)]
mod tests {
    use super::render::{infer_fk_field_name, to_camel_case};
    use super::types::column_type_to_java;
    use vespertide_core::{ColumnType, ComplexColumnType, EnumValues};

    #[test]
    fn test_column_type_to_java_string_enum() {
        let ty = ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["a".into()]),
        });
        assert_eq!(column_type_to_java(&ty), "String");
    }

    #[rstest::rstest]
    #[case("created_at", "createdAt")]
    #[case("id", "id")]
    #[case("user_profile_image", "userProfileImage")]
    #[case("", "")]
    fn test_to_camel_case(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(to_camel_case(input), expected);
    }

    #[rstest::rstest]
    #[case("customer_id", "customer")]
    #[case("author_user_id", "authorUser")]
    #[case("parent", "parent")]
    fn test_infer_fk_field_name(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(infer_fk_field_name(input), expected);
    }
}
