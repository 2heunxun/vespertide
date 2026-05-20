pub mod dot;
pub mod mermaid;
pub mod svg;

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::ValueEnum;
use vespertide_core::schema::foreign_key::ForeignKeySyntax;
use vespertide_core::{ColumnDef, ReferenceAction, TableConstraint, TableDef};

use crate::utils::{load_config, load_models};

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum ErdFormat {
    Svg,
    Mermaid,
    Dot,
}

pub async fn cmd_erd(format: ErdFormat, output: Option<PathBuf>) -> Result<()> {
    let config = load_config()?;
    let tables = normalize_tables(load_models(&config)?)?;

    let rendered = match format {
        ErdFormat::Svg => svg::render_svg(&tables).map_err(anyhow::Error::msg)?,
        ErdFormat::Mermaid => mermaid::render_mermaid(&tables),
        ErdFormat::Dot => dot::render_dot(&tables),
    };

    if let Some(path) = output {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("create ERD output directory {}", parent.display()))?;
        }

        tokio::fs::write(&path, rendered)
            .await
            .with_context(|| format!("write ERD output {}", path.display()))?;
        println!("ERD exported to {}", path.display());
    } else {
        print!("{rendered}");
    }

    Ok(())
}

fn normalize_tables(tables: Vec<TableDef>) -> Result<Vec<TableDef>> {
    tables
        .into_iter()
        .map(|table| {
            table
                .normalize()
                .with_context(|| format!("normalize table '{}'", table.name))
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct ForeignKeyRelation {
    pub child_table: String,
    pub child_columns: Vec<String>,
    pub parent_table: String,
    pub parent_columns: Vec<String>,
    pub on_delete: Option<ReferenceAction>,
    pub on_update: Option<ReferenceAction>,
}

pub(super) fn collect_foreign_key_relations(tables: &[TableDef]) -> BTreeSet<ForeignKeyRelation> {
    let mut relations = BTreeSet::new();

    for table in tables {
        for constraint in &table.constraints {
            if let TableConstraint::ForeignKey {
                columns,
                ref_table,
                ref_columns,
                on_delete,
                on_update,
                ..
            } = constraint
            {
                relations.insert(ForeignKeyRelation {
                    child_table: table.name.clone(),
                    child_columns: columns.clone(),
                    parent_table: ref_table.clone(),
                    parent_columns: ref_columns.clone(),
                    on_delete: on_delete.clone(),
                    on_update: on_update.clone(),
                });
            }
        }

        for column in &table.columns {
            if let Some(foreign_key) = &column.foreign_key
                && let Some(relation) = inline_foreign_key_relation(table, column, foreign_key)
            {
                relations.insert(relation);
            }
        }
    }

    relations
}

pub(super) fn is_primary_key_column(table: &TableDef, column_name: &str) -> bool {
    table
        .columns
        .iter()
        .any(|column| column.name == column_name && column.primary_key.is_some())
        || table.constraints.iter().any(|constraint| {
            matches!(
                constraint,
                TableConstraint::PrimaryKey { columns, .. }
                    if columns.iter().any(|column| column == column_name)
            )
        })
}

pub(super) fn is_foreign_key_column(table: &TableDef, column_name: &str) -> bool {
    table
        .columns
        .iter()
        .any(|column| column.name == column_name && column.foreign_key.is_some())
        || table.constraints.iter().any(|constraint| {
            matches!(
                constraint,
                TableConstraint::ForeignKey { columns, .. }
                    if columns.iter().any(|column| column == column_name)
            )
        })
}

pub(super) fn column_markers(table: &TableDef, column: &ColumnDef) -> String {
    let mut markers = Vec::new();
    if is_primary_key_column(table, &column.name) {
        markers.push("PK");
    }
    if is_foreign_key_column(table, &column.name) {
        markers.push("FK");
    }

    if markers.is_empty() {
        String::new()
    } else {
        format!(" ({})", markers.join(", "))
    }
}

pub(super) fn sanitize_identifier(input: &str) -> String {
    let mut identifier = String::new();

    for (index, ch) in input.chars().enumerate() {
        if ch == '_' || ch.is_ascii_alphanumeric() {
            if index == 0 && ch.is_ascii_digit() {
                identifier.push('_');
            }
            identifier.push(ch);
        } else {
            identifier.push('_');
        }
    }

    if identifier.is_empty() {
        "_".to_string()
    } else {
        identifier
    }
}

fn inline_foreign_key_relation(
    table: &TableDef,
    column: &ColumnDef,
    foreign_key: &ForeignKeySyntax,
) -> Option<ForeignKeyRelation> {
    let (parent_table, parent_columns, on_delete, on_update) = match foreign_key {
        ForeignKeySyntax::String(reference) => {
            let (table, columns) = parse_reference(reference)?;
            (table, columns, None, None)
        }
        ForeignKeySyntax::Reference(reference) => {
            let (table, columns) = parse_reference(&reference.references)?;
            (
                table,
                columns,
                reference.on_delete.clone(),
                reference.on_update.clone(),
            )
        }
        ForeignKeySyntax::Object(definition) => (
            definition.ref_table.clone(),
            definition.ref_columns.clone(),
            definition.on_delete.clone(),
            definition.on_update.clone(),
        ),
    };

    Some(ForeignKeyRelation {
        child_table: table.name.clone(),
        child_columns: vec![column.name.clone()],
        parent_table,
        parent_columns,
        on_delete,
        on_update,
    })
}

fn parse_reference(reference: &str) -> Option<(String, Vec<String>)> {
    let mut parts = reference.split('.');
    let table = parts.next()?;
    let column = parts.next()?;

    if parts.next().is_some() || table.is_empty() || column.is_empty() {
        return None;
    }

    Some((table.to_string(), vec![column.to_string()]))
}
