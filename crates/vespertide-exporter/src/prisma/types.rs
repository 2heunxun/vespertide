use vespertide_core::schema::column::{ColumnType, ComplexColumnType, SimpleColumnType};

use vespertide_query::DatabaseBackend;

use super::enums::to_pascal_case;

pub(super) fn column_type_to_prisma(
    ty: &ColumnType,
    nullable: bool,
    provider: DatabaseBackend,
) -> (String, Option<String>) {
    let q = if nullable { "?" } else { "" };

    match ty {
        // vespertide stores MySQL UUIDs as BINARY(16) (see vespertide-query's
        // uuid mapping), so the Prisma scalar itself differs per provider.
        ColumnType::Simple(SimpleColumnType::Uuid) => {
            let (base, native) = match provider {
                DatabaseBackend::Postgres => ("String", Some("@db.Uuid")),
                DatabaseBackend::MySql => ("Bytes", Some("@db.Binary(16)")),
                DatabaseBackend::Sqlite => ("String", None),
            };
            (format!("{base}{q}"), native.map(str::to_string))
        }
        ColumnType::Simple(simple) => {
            // (scalar, postgres native attr, mysql native attr) — mirrors the
            // per-backend DDL mapping in vespertide-query's `apply_simple_column_type`.
            // SQLite never gets native attrs (its connector supports none).
            let (base, pg_native, mysql_native) = match simple {
                SimpleColumnType::SmallInt => ("Int", Some("@db.SmallInt"), Some("@db.SmallInt")),
                SimpleColumnType::Integer => ("Int", None, None),
                SimpleColumnType::BigInt => ("BigInt", None, None),
                SimpleColumnType::Real => ("Float", Some("@db.Real"), Some("@db.Float")),
                SimpleColumnType::DoublePrecision => ("Float", None, None),
                SimpleColumnType::Text => ("String", Some("@db.Text"), Some("@db.Text")),
                SimpleColumnType::Boolean => ("Boolean", None, None),
                SimpleColumnType::Date => ("DateTime", Some("@db.Date"), Some("@db.Date")),
                SimpleColumnType::Time => ("DateTime", Some("@db.Time"), Some("@db.Time")),
                SimpleColumnType::Timestamp => {
                    ("DateTime", Some("@db.Timestamp"), Some("@db.Timestamp"))
                }
                // MySQL has no TIMESTAMPTZ; the DDL degrades it to TIMESTAMP.
                SimpleColumnType::Timestamptz => {
                    ("DateTime", Some("@db.Timestamptz"), Some("@db.Timestamp"))
                }
                SimpleColumnType::Bytea => ("Bytes", None, None),
                SimpleColumnType::Json => ("Json", None, None),
                SimpleColumnType::Inet => ("String", Some("@db.Inet"), Some("@db.Text")),
                SimpleColumnType::Xml => ("String", Some("@db.Xml"), Some("@db.Text")),
                // Interval/Cidr/Macaddr have no Prisma native type on PostgreSQL;
                // the MySQL DDL degrades all three to TEXT.
                SimpleColumnType::Interval | SimpleColumnType::Cidr | SimpleColumnType::Macaddr => {
                    ("String", None, Some("@db.Text"))
                }
                // Unknown/future simple types fall back to a plain String column.
                // `SimpleColumnType` is #[non_exhaustive]; every current variant is
                // matched above, so this arm is currently unreachable.
                #[cfg(not(tarpaulin_include))]
                _ => ("String", None, None),
            };
            let native = match provider {
                DatabaseBackend::Postgres => pg_native,
                DatabaseBackend::MySql => mysql_native,
                DatabaseBackend::Sqlite => None,
            };
            (format!("{base}{q}"), native.map(str::to_string))
        }
        ColumnType::Complex(complex) => match complex {
            ComplexColumnType::Varchar { length } => (
                format!("String{q}"),
                sized_native(provider, format!("@db.VarChar({length})")),
            ),
            ComplexColumnType::Char { length } => (
                format!("String{q}"),
                sized_native(provider, format!("@db.Char({length})")),
            ),
            ComplexColumnType::Numeric { precision, scale } => (
                format!("Decimal{q}"),
                sized_native(provider, format!("@db.Decimal({precision}, {scale})")),
            ),
            ComplexColumnType::Custom { custom_type } => {
                (format!("Unsupported(\"{custom_type}\"){q}"), None)
            }
            ComplexColumnType::Enum { name, .. } => {
                let pascal = to_pascal_case(name);
                (format!("{pascal}{q}"), None)
            }
            // Unknown/future complex types fall back to a plain String column.
            // `ComplexColumnType` is #[non_exhaustive]; every current variant is
            // matched above, so this arm is currently unreachable.
            #[cfg(not(tarpaulin_include))]
            _ => (format!("String{q}"), None),
        },
    }
}

/// VarChar/Char/Decimal natives are spelled identically on PostgreSQL and
/// MySQL; SQLite supports no native attrs at all.
fn sized_native(provider: DatabaseBackend, attr: String) -> Option<String> {
    match provider {
        DatabaseBackend::Postgres | DatabaseBackend::MySql => Some(attr),
        DatabaseBackend::Sqlite => None,
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use vespertide_core::schema::column::{ColumnType, ComplexColumnType, SimpleColumnType};

    use super::*;

    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres, "String", Some("@db.Uuid"))]
    #[case::mysql(DatabaseBackend::MySql, "Bytes", Some("@db.Binary(16)"))]
    #[case::sqlite(DatabaseBackend::Sqlite, "String", None)]
    fn uuid_scalar_and_native_follow_provider(
        #[case] provider: DatabaseBackend,
        #[case] scalar: &str,
        #[case] native: Option<&str>,
    ) {
        let ty = ColumnType::Simple(SimpleColumnType::Uuid);
        let (rendered, attr) = column_type_to_prisma(&ty, false, provider);
        assert_eq!(rendered, scalar);
        assert_eq!(attr.as_deref(), native);
    }

    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres, Some("@db.Timestamptz"))]
    #[case::mysql(DatabaseBackend::MySql, Some("@db.Timestamp"))]
    #[case::sqlite(DatabaseBackend::Sqlite, None)]
    fn timestamptz_native_follows_provider(
        #[case] provider: DatabaseBackend,
        #[case] native: Option<&str>,
    ) {
        let ty = ColumnType::Simple(SimpleColumnType::Timestamptz);
        let (rendered, attr) = column_type_to_prisma(&ty, false, provider);
        assert_eq!(rendered, "DateTime");
        assert_eq!(attr.as_deref(), native);
    }

    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres, None)]
    #[case::mysql(DatabaseBackend::MySql, Some("@db.Text"))]
    #[case::sqlite(DatabaseBackend::Sqlite, None)]
    fn interval_native_follows_provider(
        #[case] provider: DatabaseBackend,
        #[case] native: Option<&str>,
    ) {
        let ty = ColumnType::Simple(SimpleColumnType::Interval);
        let (rendered, attr) = column_type_to_prisma(&ty, false, provider);
        assert_eq!(rendered, "String");
        assert_eq!(attr.as_deref(), native);
    }

    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres, Some("@db.Inet"))]
    #[case::mysql(DatabaseBackend::MySql, Some("@db.Text"))]
    #[case::sqlite(DatabaseBackend::Sqlite, None)]
    fn inet_native_follows_provider(
        #[case] provider: DatabaseBackend,
        #[case] native: Option<&str>,
    ) {
        let ty = ColumnType::Simple(SimpleColumnType::Inet);
        let (rendered, attr) = column_type_to_prisma(&ty, true, provider);
        assert_eq!(rendered, "String?");
        assert_eq!(attr.as_deref(), native);
    }

    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres, Some("@db.VarChar(255)"))]
    #[case::mysql(DatabaseBackend::MySql, Some("@db.VarChar(255)"))]
    #[case::sqlite(DatabaseBackend::Sqlite, None)]
    fn varchar_native_follows_provider(
        #[case] provider: DatabaseBackend,
        #[case] native: Option<&str>,
    ) {
        let ty = ColumnType::Complex(ComplexColumnType::Varchar { length: 255 });
        let (rendered, attr) = column_type_to_prisma(&ty, false, provider);
        assert_eq!(rendered, "String");
        assert_eq!(attr.as_deref(), native);
    }

    /// Exhaustive oracle for every simple type except `Uuid` (whose scalar
    /// itself is provider-dependent — covered by its dedicated test above).
    /// Each row is independent test data, so flipping any production mapping
    /// tuple (mutation testing) fails here immediately.
    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn simple_type_mapping_matrix(#[case] provider: DatabaseBackend) {
        use SimpleColumnType as S;
        // (type, scalar, postgres attr, mysql attr) — sqlite is always attr-less.
        let rows: &[(S, &str, Option<&str>, Option<&str>)] = &[
            (
                S::SmallInt,
                "Int",
                Some("@db.SmallInt"),
                Some("@db.SmallInt"),
            ),
            (S::Integer, "Int", None, None),
            (S::BigInt, "BigInt", None, None),
            (S::Real, "Float", Some("@db.Real"), Some("@db.Float")),
            (S::DoublePrecision, "Float", None, None),
            (S::Text, "String", Some("@db.Text"), Some("@db.Text")),
            (S::Boolean, "Boolean", None, None),
            (S::Date, "DateTime", Some("@db.Date"), Some("@db.Date")),
            (S::Time, "DateTime", Some("@db.Time"), Some("@db.Time")),
            (
                S::Timestamp,
                "DateTime",
                Some("@db.Timestamp"),
                Some("@db.Timestamp"),
            ),
            (
                S::Timestamptz,
                "DateTime",
                Some("@db.Timestamptz"),
                Some("@db.Timestamp"),
            ),
            (S::Interval, "String", None, Some("@db.Text")),
            (S::Bytea, "Bytes", None, None),
            (S::Json, "Json", None, None),
            (S::Inet, "String", Some("@db.Inet"), Some("@db.Text")),
            (S::Cidr, "String", None, Some("@db.Text")),
            (S::Macaddr, "String", None, Some("@db.Text")),
            (S::Xml, "String", Some("@db.Xml"), Some("@db.Text")),
        ];

        for (ty, scalar, pg, mysql) in rows {
            let expected = match provider {
                DatabaseBackend::Postgres => *pg,
                DatabaseBackend::MySql => *mysql,
                DatabaseBackend::Sqlite => None,
            };
            let (rendered, attr) = column_type_to_prisma(&ColumnType::Simple(*ty), false, provider);
            assert_eq!(rendered, *scalar, "{ty:?} scalar under {provider:?}");
            assert_eq!(attr.as_deref(), expected, "{ty:?} attr under {provider:?}");
        }
    }

    #[rstest]
    #[case::postgres(
        DatabaseBackend::Postgres,
        Some("@db.Char(3)"),
        Some("@db.Decimal(10, 2)")
    )]
    #[case::mysql(
        DatabaseBackend::MySql,
        Some("@db.Char(3)"),
        Some("@db.Decimal(10, 2)")
    )]
    #[case::sqlite(DatabaseBackend::Sqlite, None, None)]
    fn char_and_numeric_natives_follow_provider(
        #[case] provider: DatabaseBackend,
        #[case] char_native: Option<&str>,
        #[case] numeric_native: Option<&str>,
    ) {
        let (rendered, attr) = column_type_to_prisma(
            &ColumnType::Complex(ComplexColumnType::Char { length: 3 }),
            false,
            provider,
        );
        assert_eq!(rendered, "String");
        assert_eq!(attr.as_deref(), char_native);

        let (rendered, attr) = column_type_to_prisma(
            &ColumnType::Complex(ComplexColumnType::Numeric {
                precision: 10,
                scale: 2,
            }),
            true,
            provider,
        );
        assert_eq!(rendered, "Decimal?");
        assert_eq!(attr.as_deref(), numeric_native);
    }

    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn custom_and_enum_types_are_provider_invariant(#[case] provider: DatabaseBackend) {
        let (rendered, attr) = column_type_to_prisma(
            &ColumnType::Complex(ComplexColumnType::Custom {
                custom_type: "ltree".into(),
            }),
            false,
            provider,
        );
        assert_eq!(rendered, "Unsupported(\"ltree\")");
        assert_eq!(attr, None);

        let (rendered, attr) = column_type_to_prisma(
            &ColumnType::Complex(ComplexColumnType::Enum {
                name: "order_status".into(),
                values: vespertide_core::schema::column::EnumValues::String(vec![
                    "open".into(),
                    "closed".into(),
                ]),
            }),
            false,
            provider,
        );
        assert_eq!(rendered, "OrderStatus");
        assert_eq!(attr, None);
    }
}
