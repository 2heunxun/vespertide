use vespertide_core::schema::column::{ColumnType, ComplexColumnType, SimpleColumnType};
use vespertide_naming::to_pascal_case;

/// Maps a vespertide column type to a `(Prisma scalar type, optional trailing
/// `// ...` comment)` pair.
///
/// The output is backend-neutral: no `@db.*` native attributes are emitted, so
/// the same model body is valid under every Prisma provider. Physical column
/// types are owned by vespertide's own DDL generation, not the Prisma schema.
pub(super) fn column_type_to_prisma(ty: &ColumnType, nullable: bool) -> (String, Option<String>) {
    let q = if nullable { "?" } else { "" };

    match ty {
        // vespertide's MySQL DDL stores uuid as BINARY(16) — the one physical
        // type that diverges from the neutral scalar, so the field carries a
        // warning comment instead of a backend-specific type.
        ColumnType::Simple(SimpleColumnType::Uuid) => (
            format!("String{q}"),
            Some("stored as binary(16) on MySQL backends".to_string()),
        ),
        ColumnType::Simple(simple) => {
            let base = match simple {
                SimpleColumnType::SmallInt | SimpleColumnType::Integer => "Int",
                SimpleColumnType::BigInt => "BigInt",
                SimpleColumnType::Real | SimpleColumnType::DoublePrecision => "Float",
                SimpleColumnType::Boolean => "Boolean",
                SimpleColumnType::Date
                | SimpleColumnType::Time
                | SimpleColumnType::Timestamp
                | SimpleColumnType::Timestamptz => "DateTime",
                SimpleColumnType::Bytea => "Bytes",
                SimpleColumnType::Json => "Json",
                SimpleColumnType::Text
                | SimpleColumnType::Interval
                | SimpleColumnType::Inet
                | SimpleColumnType::Cidr
                | SimpleColumnType::Macaddr
                | SimpleColumnType::Xml => "String",
                _ => unreachable!(
                    "SimpleColumnType is #[non_exhaustive]; all variants are matched above"
                ),
            };
            (format!("{base}{q}"), None)
        }
        ColumnType::Complex(complex) => match complex {
            ComplexColumnType::Varchar { .. } | ComplexColumnType::Char { .. } => {
                (format!("String{q}"), None)
            }
            ComplexColumnType::Numeric { .. } => (format!("Decimal{q}"), None),
            ComplexColumnType::Custom { custom_type } => {
                (format!("Unsupported(\"{custom_type}\"){q}"), None)
            }
            ComplexColumnType::Enum { name, .. } => {
                let pascal = to_pascal_case(name);
                (format!("{pascal}{q}"), None)
            }
            _ => unreachable!(
                "ComplexColumnType is #[non_exhaustive]; all variants are matched above"
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use vespertide_core::schema::column::{ColumnType, ComplexColumnType, SimpleColumnType};

    use super::*;

    /// Exhaustive oracle for every simple type. Each row is independent test
    /// data, so flipping any production mapping arm fails here immediately.
    #[rstest]
    #[case::small_int(SimpleColumnType::SmallInt, "Int")]
    #[case::integer(SimpleColumnType::Integer, "Int")]
    #[case::big_int(SimpleColumnType::BigInt, "BigInt")]
    #[case::real(SimpleColumnType::Real, "Float")]
    #[case::double_precision(SimpleColumnType::DoublePrecision, "Float")]
    #[case::boolean(SimpleColumnType::Boolean, "Boolean")]
    #[case::date(SimpleColumnType::Date, "DateTime")]
    #[case::time(SimpleColumnType::Time, "DateTime")]
    #[case::timestamp(SimpleColumnType::Timestamp, "DateTime")]
    #[case::timestamptz(SimpleColumnType::Timestamptz, "DateTime")]
    #[case::bytea(SimpleColumnType::Bytea, "Bytes")]
    #[case::json(SimpleColumnType::Json, "Json")]
    #[case::text(SimpleColumnType::Text, "String")]
    #[case::interval(SimpleColumnType::Interval, "String")]
    #[case::inet(SimpleColumnType::Inet, "String")]
    #[case::cidr(SimpleColumnType::Cidr, "String")]
    #[case::macaddr(SimpleColumnType::Macaddr, "String")]
    #[case::xml(SimpleColumnType::Xml, "String")]
    fn simple_types_map_to_neutral_scalars(#[case] simple: SimpleColumnType, #[case] scalar: &str) {
        let (rendered, comment) = column_type_to_prisma(&ColumnType::Simple(simple), false);
        assert_eq!(rendered, scalar);
        assert_eq!(comment, None);
    }

    #[test]
    fn uuid_maps_to_string_with_mysql_binary16_note() {
        let ty = ColumnType::Simple(SimpleColumnType::Uuid);
        let (rendered, comment) = column_type_to_prisma(&ty, false);
        assert_eq!(rendered, "String");
        assert_eq!(
            comment.as_deref(),
            Some("stored as binary(16) on MySQL backends")
        );

        let (rendered, _) = column_type_to_prisma(&ty, true);
        assert_eq!(rendered, "String?");
    }

    #[test]
    fn nullable_appends_question_mark() {
        let ty = ColumnType::Simple(SimpleColumnType::Timestamptz);
        let (rendered, comment) = column_type_to_prisma(&ty, true);
        assert_eq!(rendered, "DateTime?");
        assert_eq!(comment, None);
    }

    #[test]
    fn sized_complex_types_drop_size_info() {
        let (rendered, comment) = column_type_to_prisma(
            &ColumnType::Complex(ComplexColumnType::Varchar { length: 255 }),
            false,
        );
        assert_eq!(rendered, "String");
        assert_eq!(comment, None);

        let (rendered, comment) = column_type_to_prisma(
            &ColumnType::Complex(ComplexColumnType::Char { length: 3 }),
            false,
        );
        assert_eq!(rendered, "String");
        assert_eq!(comment, None);

        let (rendered, comment) = column_type_to_prisma(
            &ColumnType::Complex(ComplexColumnType::Numeric {
                precision: 10,
                scale: 2,
            }),
            true,
        );
        assert_eq!(rendered, "Decimal?");
        assert_eq!(comment, None);
    }

    #[test]
    fn custom_and_enum_types_render_without_comment() {
        let (rendered, comment) = column_type_to_prisma(
            &ColumnType::Complex(ComplexColumnType::Custom {
                custom_type: "ltree".into(),
            }),
            false,
        );
        assert_eq!(rendered, "Unsupported(\"ltree\")");
        assert_eq!(comment, None);

        let (rendered, comment) = column_type_to_prisma(
            &ColumnType::Complex(ComplexColumnType::Enum {
                name: "order_status".into(),
                values: vespertide_core::schema::column::EnumValues::String(vec![
                    "open".into(),
                    "closed".into(),
                ]),
            }),
            false,
        );
        assert_eq!(rendered, "OrderStatus");
        assert_eq!(comment, None);
    }
}
