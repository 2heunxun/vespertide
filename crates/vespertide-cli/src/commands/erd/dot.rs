use dot_writer::{Attributes, DotWriter, RankDirection, Shape};
use vespertide_core::{ColumnDef, TableDef};

use super::{
    ForeignKeyRelation, collect_foreign_key_relations, column_markers, sanitize_identifier,
};

pub fn render_dot(tables: &[TableDef]) -> String {
    DotWriter::write_string(|writer| {
        writer.set_pretty_print(true);

        let mut digraph = writer.digraph();
        digraph.set_rank_direction(RankDirection::LeftRight);
        digraph.set("bgcolor", "transparent", true);

        {
            let mut node_attributes = digraph.node_attributes();
            node_attributes.set_shape(Shape::Record);
            node_attributes.set("fontname", "Helvetica", true);
        }

        {
            let mut edge_attributes = digraph.edge_attributes();
            edge_attributes.set("fontname", "Helvetica", true);
        }

        for table in tables {
            let mut node = digraph.node_named(sanitize_identifier(&table.name));
            node.set_shape(Shape::Record);
            node.set_label(&record_label(table));
        }

        for relation in collect_foreign_key_relations(tables) {
            let mut edge_attributes = digraph
                .edge(
                    sanitize_identifier(&relation.child_table),
                    sanitize_identifier(&relation.parent_table),
                )
                .attributes();
            edge_attributes.set_label(&relationship_label(&relation));
        }
    })
}

fn record_label(table: &TableDef) -> String {
    let mut fields = Vec::with_capacity(table.columns.len() + 1);
    fields.push(escape_record_field(&table.name));

    for column in &table.columns {
        fields.push(column_record_field(table, column));
    }

    format!("{{{}}}", fields.join("|"))
}

fn column_record_field(table: &TableDef, column: &ColumnDef) -> String {
    format!(
        "{}: {}{}",
        escape_record_field(&column.name),
        escape_record_field(&column.r#type.to_display_string()),
        escape_record_field(&column_markers(table, column))
    )
}

fn relationship_label(relation: &ForeignKeyRelation) -> String {
    format!(
        "{}: {} -> {}",
        relation.cardinality.label(),
        relation.child_columns.join(", "),
        relation.parent_columns.join(", ")
    )
}

fn escape_record_field(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());

    for ch in value.chars() {
        match ch {
            '\\' | '{' | '}' | '|' | '<' | '>' | '"' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            _ => escaped.push(ch),
        }
    }

    escaped
}
