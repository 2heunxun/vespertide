use std::fmt::Write as _;

use vespertide_core::{ColumnType, ComplexColumnType, EnumValues, SimpleColumnType, TableDef};

use super::{
    collect_foreign_key_relations, is_foreign_key_column, is_primary_key_column,
    sanitize_identifier,
};

pub fn render_mermaid(tables: &[TableDef]) -> String {
    let mut output = String::from("erDiagram\n");

    for table in tables {
        writeln!(output, "  {} {{", sanitize_identifier(&table.name))
            .expect("write Mermaid table header");

        for column in &table.columns {
            let primary_key = if is_primary_key_column(table, &column.name) {
                " PK"
            } else {
                ""
            };
            let foreign_key = if is_foreign_key_column(table, &column.name) {
                " FK"
            } else {
                ""
            };

            writeln!(
                output,
                "    {} {}{}{}",
                column_type_to_mermaid(&column.r#type),
                sanitize_identifier(&column.name),
                primary_key,
                foreign_key
            )
            .expect("write Mermaid column");
        }

        writeln!(output, "  }}").expect("write Mermaid table footer");
    }

    for relation in collect_foreign_key_relations(tables) {
        writeln!(
            output,
            "  {} ||--o{{ {} : \"{}\"",
            sanitize_identifier(&relation.parent_table),
            sanitize_identifier(&relation.child_table),
            escape_mermaid_label(&relation.child_columns.join(", "))
        )
        .expect("write Mermaid relationship");
    }

    output
}

fn column_type_to_mermaid(column_type: &ColumnType) -> &'static str {
    match column_type {
        ColumnType::Simple(simple) => simple_column_type_to_mermaid(simple),
        ColumnType::Complex(complex) => complex_column_type_to_mermaid(complex),
    }
}

fn simple_column_type_to_mermaid(column_type: &SimpleColumnType) -> &'static str {
    match column_type {
        SimpleColumnType::SmallInt | SimpleColumnType::Integer | SimpleColumnType::BigInt => "int",
        SimpleColumnType::Real | SimpleColumnType::DoublePrecision => "float",
        SimpleColumnType::Boolean => "boolean",
        SimpleColumnType::Date => "date",
        SimpleColumnType::Time => "time",
        SimpleColumnType::Timestamp | SimpleColumnType::Timestamptz => "datetime",
        SimpleColumnType::Bytea => "binary",
        SimpleColumnType::Uuid => "uuid",
        SimpleColumnType::Json => "json",
        _ => "string",
    }
}

fn complex_column_type_to_mermaid(column_type: &ComplexColumnType) -> &'static str {
    match column_type {
        ComplexColumnType::Numeric { .. } => "decimal",
        ComplexColumnType::Enum { values, .. } => match values {
            EnumValues::String(_) => "string",
            EnumValues::Integer(_) => "int",
        },
        _ => "string",
    }
}

fn escape_mermaid_label(label: &str) -> String {
    label.replace('\\', "\\\\").replace('"', "\\\"")
}
