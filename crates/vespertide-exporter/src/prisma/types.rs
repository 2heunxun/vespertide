use vespertide_core::schema::column::{ColumnType, ComplexColumnType, SimpleColumnType};

use super::enums::to_pascal_case;

pub(super) fn column_type_to_prisma(ty: &ColumnType, nullable: bool) -> (String, Option<String>) {
    let q = if nullable { "?" } else { "" };

    match ty {
        ColumnType::Simple(simple) => {
            let (base, native) = match simple {
                SimpleColumnType::SmallInt => ("Int", Some("@db.SmallInt")),
                SimpleColumnType::Integer => ("Int", None),
                SimpleColumnType::BigInt => ("BigInt", None),
                SimpleColumnType::Real => ("Float", Some("@db.Real")),
                SimpleColumnType::DoublePrecision => ("Float", None),
                SimpleColumnType::Text => ("String", Some("@db.Text")),
                SimpleColumnType::Boolean => ("Boolean", None),
                SimpleColumnType::Date => ("DateTime", Some("@db.Date")),
                SimpleColumnType::Time => ("DateTime", Some("@db.Time")),
                SimpleColumnType::Timestamp => ("DateTime", Some("@db.Timestamp")),
                SimpleColumnType::Timestamptz => ("DateTime", Some("@db.Timestamptz")),
                SimpleColumnType::Bytea => ("Bytes", None),
                SimpleColumnType::Uuid => ("String", Some("@db.Uuid")),
                SimpleColumnType::Json => ("Json", None),
                SimpleColumnType::Inet => ("String", Some("@db.Inet")),
                SimpleColumnType::Xml => ("String", Some("@db.Xml")),
                // Interval/Cidr/Macaddr have no Prisma native type; they and
                // unknown/future simple types fall back to a plain String column.
                _ => ("String", None),
            };
            (format!("{base}{q}"), native.map(str::to_string))
        }
        ColumnType::Complex(complex) => match complex {
            ComplexColumnType::Varchar { length } => {
                (format!("String{q}"), Some(format!("@db.VarChar({length})")))
            }
            ComplexColumnType::Char { length } => {
                (format!("String{q}"), Some(format!("@db.Char({length})")))
            }
            ComplexColumnType::Numeric { precision, scale } => (
                format!("Decimal{q}"),
                Some(format!("@db.Decimal({precision}, {scale})")),
            ),
            ComplexColumnType::Custom { custom_type } => {
                (format!("Unsupported(\"{custom_type}\"){q}"), None)
            }
            ComplexColumnType::Enum { name, .. } => {
                let pascal = to_pascal_case(name);
                (format!("{pascal}{q}"), None)
            }
            // Unknown/future complex types fall back to a plain String column.
            _ => (format!("String{q}"), None),
        },
    }
}
