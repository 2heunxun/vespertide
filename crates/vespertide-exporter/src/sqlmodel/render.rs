use crate::utils::python::collect_composite_fks;
use vespertide_core::schema::column::{ColumnType, ComplexColumnType, EnumValues};
use vespertide_core::schema::constraint::TableConstraint;
use vespertide_core::{ColumnDef, TableDef};

use super::enums::{render_enum, to_pascal_case};
use super::types::{UsedTypes, column_type_to_python};

/// Render a `SQLModel` model for the given table definition.
#[expect(
    clippy::too_many_lines,
    reason = "SQLModel entity rendering is a linear template emitter"
)]
pub fn render_entity(table: &TableDef) -> Result<String, String> {
    let mut lines: Vec<String> = Vec::new();

    // Collect enums for this table
    let enums: Vec<(&str, &EnumValues)> = table
        .columns
        .iter()
        .filter_map(|col| {
            if let ColumnType::Complex(ComplexColumnType::Enum { name, values }) = &col.r#type {
                Some((name.as_str(), values))
            } else {
                None
            }
        })
        .collect();

    // Collect used types
    let mut used_types = UsedTypes::default();
    for col in &table.columns {
        used_types.add_column_type(&col.r#type, col.nullable);
    }

    // Check for composite indexes
    let has_composite_index = table
        .constraints
        .iter()
        .any(|c| matches!(c, TableConstraint::Index { columns, .. } if columns.len() > 1));
    if has_composite_index {
        used_types.needs_index = true;
    }

    // Check for composite unique constraints
    let has_composite_unique = table
        .constraints
        .iter()
        .any(|c| matches!(c, TableConstraint::Unique { columns, .. } if columns.len() > 1));
    if has_composite_unique {
        used_types.needs_unique_constraint = true;
    }

    let composite_fks = collect_composite_fks(table);
    if !composite_fks.is_empty() {
        used_types.needs_foreign_key_constraint = true;
    }

    // Check for server defaults (function calls like now())
    let has_server_default = table
        .columns
        .iter()
        .any(|c| c.default.as_ref().is_some_and(|d| d.to_sql().contains('(')));
    if has_server_default {
        used_types.needs_text = true;
    }

    // Generate imports
    lines.push("from __future__ import annotations".into());
    lines.push(String::new());
    if !enums.is_empty() {
        lines.push("import enum".into());
    }

    // datetime imports
    let mut datetime_imports: Vec<&str> = used_types.datetime_types.iter().copied().collect();
    datetime_imports.sort_unstable();
    if !datetime_imports.is_empty() {
        lines.push(format!(
            "from datetime import {}",
            datetime_imports.join(", ")
        ));
    }

    if used_types.needs_decimal {
        lines.push("from decimal import Decimal".into());
    }

    if used_types.needs_optional {
        lines.push("from typing import Optional".into());
    }

    if used_types.needs_uuid {
        lines.push("from uuid import UUID".into());
    }

    lines.push(String::new());
    lines.push("from sqlmodel import Field, SQLModel".into());

    // SQLAlchemy imports (only if needed)
    let mut sa_imports: Vec<&str> = Vec::new();
    if used_types.needs_index {
        sa_imports.push("Index");
    }
    if used_types.needs_unique_constraint {
        sa_imports.push("UniqueConstraint");
    }
    if used_types.needs_foreign_key_constraint {
        sa_imports.push("ForeignKeyConstraint");
    }
    if used_types.needs_text {
        sa_imports.push("text");
    }
    if !sa_imports.is_empty() {
        lines.push(format!("from sqlalchemy import {}", sa_imports.join(", ")));
    }

    lines.push(String::new());
    lines.push(String::new());

    // Render enum classes
    for (enum_name, values) in &enums {
        render_enum(&mut lines, enum_name, values);
        lines.push(String::new());
    }

    // Class definition
    let class_name = to_pascal_case(&table.name);

    // Add table description as docstring
    lines.push(format!("class {class_name}(SQLModel, table=True):"));
    if let Some(ref desc) = table.description {
        lines.push(format!("    \"\"\"{}\"\"\"", desc.replace('\n', " ")));
    }

    lines.push(format!("    __tablename__ = \"{}\"", table.name));
    lines.push(String::new());

    // Collect primary key columns; lookup-only, ordering unused.
    let pk_columns: std::collections::HashSet<String> = table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::PrimaryKey { columns, .. } = c {
                Some(columns.clone())
            } else {
                None
            }
        })
        .flatten()
        .collect();

    // Collect unique columns (single-column unique constraints); lookup-only, ordering unused.
    let unique_columns: std::collections::HashSet<String> = table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::Unique { columns, .. } = c {
                if columns.len() == 1 {
                    Some(columns[0].clone())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    // Collect indexed columns (single-column indexes); lookup-only, ordering unused.
    let indexed_columns: std::collections::HashSet<String> = table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::Index { columns, .. } = c {
                if columns.len() == 1 {
                    Some(columns[0].clone())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    // Collect foreign key info; lookup-only, ordering unused.
    let fk_info: std::collections::HashMap<String, (String, String)> = table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::ForeignKey {
                columns,
                ref_table,
                ref_columns,
                ..
            } = c
            {
                if columns.len() == 1 && ref_columns.len() == 1 {
                    Some((
                        columns[0].clone(),
                        (ref_table.clone(), ref_columns[0].clone()),
                    ))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    // Render columns
    for col in &table.columns {
        render_column(
            &mut lines,
            col,
            pk_columns.contains(&col.name),
            unique_columns.contains(&col.name),
            indexed_columns.contains(&col.name),
            fk_info.get(&col.name),
        );
    }

    // Render table args for composite indexes and unique constraints
    let composite_indexes: Vec<_> = table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::Index { name, columns } = c {
                if columns.len() > 1 {
                    Some((name.clone(), columns.clone()))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    let composite_uniques: Vec<_> = table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::Unique { name, columns } = c {
                if columns.len() > 1 {
                    Some((name.clone(), columns.clone()))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    if !composite_indexes.is_empty() || !composite_uniques.is_empty() || !composite_fks.is_empty() {
        lines.push(String::new());
        lines.push("    __table_args__ = (".into());

        for (name, columns) in &composite_indexes {
            let cols_str = columns
                .iter()
                .map(|c| format!("\"{c}\""))
                .collect::<Vec<_>>()
                .join(", ");
            if let Some(idx_name) = name {
                lines.push(format!("        Index(\"{idx_name}\", {cols_str}),"));
            } else {
                lines.push(format!("        Index(None, {cols_str}),"));
            }
        }

        for (name, columns) in &composite_uniques {
            let cols_str = columns
                .iter()
                .map(|c| format!("\"{c}\""))
                .collect::<Vec<_>>()
                .join(", ");
            if let Some(uq_name) = name {
                lines.push(format!(
                    "        UniqueConstraint({cols_str}, name=\"{uq_name}\"),"
                ));
            } else {
                lines.push(format!("        UniqueConstraint({cols_str}),"));
            }
        }

        for fk in &composite_fks {
            let local_cols = fk
                .local_cols
                .iter()
                .map(|col| format!("\"{col}\""))
                .collect::<Vec<_>>()
                .join(", ");
            let ref_cols = fk
                .ref_cols
                .iter()
                .map(|col| format!("\"{}.{}\"", fk.ref_table, col))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!(
                "        ForeignKeyConstraint([{local_cols}], [{ref_cols}]),"
            ));
        }

        lines.push("    )".into());
    }

    lines.push(String::new());

    Ok(lines.join("\n"))
}

pub(super) fn render_column(
    lines: &mut Vec<String>,
    col: &ColumnDef,
    is_pk: bool,
    is_unique: bool,
    is_indexed: bool,
    fk_info: Option<&(String, String)>,
) {
    // Add column comment
    if let Some(ref comment) = col.comment {
        lines.push(format!("    # {}", comment.replace('\n', " ")));
    }

    let python_type = column_type_to_python(&col.r#type, col.nullable);
    let mut field_args: Vec<String> = Vec::new();

    // Default value handling
    if let Some(ref default) = col.default {
        let default_str = default.to_sql();
        // Escape double quotes for embedding in Python strings
        let escaped = default_str.replace('"', "\\\"");
        // For server-side defaults, use sa_column_kwargs
        if default_str.contains('(') {
            field_args.push(format!(
                "sa_column_kwargs={{\"server_default\": text(\"{escaped}\")}}"
            ));
        } else if default_str == "true" {
            field_args.push("default=True".into());
        } else if default_str == "false" {
            field_args.push("default=False".into());
        } else if default_str.starts_with('\'') || default_str.starts_with('"') {
            // String literal - strip quotes for Python
            let stripped = default_str.trim_matches(|c| c == '\'' || c == '"');
            let stripped_escaped = stripped.replace('"', "\\\"");
            field_args.push(format!("default=\"{stripped_escaped}\""));
        } else if default_str.parse::<f64>().is_ok() {
            field_args.push(format!("default={default_str}"));
        } else {
            // Assume it's a server default
            field_args.push(format!(
                "sa_column_kwargs={{\"server_default\": text(\"{escaped}\")}}"
            ));
        }
    } else if col.nullable {
        field_args.push("default=None".into());
    }

    // Primary key
    if is_pk {
        field_args.push("primary_key=True".into());
    }

    // Foreign key
    if let Some((ref_table, ref_col)) = fk_info {
        field_args.push(format!("foreign_key=\"{ref_table}.{ref_col}\""));
    }

    // Unique
    if is_unique && !is_pk {
        field_args.push("unique=True".into());
    }

    // Index (for single-column indexes)
    if is_indexed && !is_pk {
        field_args.push("index=True".into());
    }

    // Build field definition
    let field_str = if field_args.is_empty() {
        "Field(...)".into()
    } else {
        format!("Field({})", field_args.join(", "))
    };

    lines.push(format!("    {}: {} = {}", col.name, python_type, field_str));
}
