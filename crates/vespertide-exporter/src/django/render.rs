use std::collections::{HashMap, HashSet};

use super::enums::render_enum;
use super::types::{UsedImports, build_field_kwargs, django_field_type, reference_action_str};
use vespertide_core::schema::column::{ColumnType, ComplexColumnType};
use vespertide_core::schema::constraint::TableConstraint;
use vespertide_core::{ReferenceAction, TableDef};

pub fn render_entity(table: &TableDef) -> Result<String, String> {
    let mut used = UsedImports::default();
    let body = render_entity_part(table, &mut used);
    Ok(assemble_with_imports(&used, &[body]))
}

pub fn export(schema: &[TableDef]) -> Result<String, String> {
    let mut used = UsedImports::default();
    let parts: Vec<String> = schema
        .iter()
        .map(|t| render_entity_part(t, &mut used))
        .collect();
    Ok(assemble_with_imports(&used, &parts))
}

fn render_entity_part(table: &TableDef, used: &mut UsedImports) -> String {
    let mut lines: Vec<String> = Vec::new();

    // --- Constraint lookups ---
    let pk_columns: HashSet<String> = table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::PrimaryKey { columns, .. } = c {
                Some(columns.iter().map(|c| c.as_str().to_owned()).collect::<Vec<_>>())
            } else {
                None
            }
        })
        .flatten()
        .collect();

    let auto_increment = table
        .constraints
        .iter()
        .any(|c| matches!(c, TableConstraint::PrimaryKey { auto_increment: true, .. }));

    let is_composite_pk = pk_columns.len() > 1;

    let single_unique_cols: HashSet<String> = table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::Unique { columns, .. } = c {
                if columns.len() == 1 { Some(columns[0].as_str().to_owned()) } else { None }
            } else {
                None
            }
        })
        .collect();

    // single-column FK info: col_name → (ref_table, on_delete, on_update)
    let fk_map: HashMap<String, (&str, Option<&ReferenceAction>, Option<&ReferenceAction>)> =
        table
            .constraints
            .iter()
            .filter_map(|c| {
                if let TableConstraint::ForeignKey {
                    columns,
                    ref_table,
                    ref_columns,
                    on_delete,
                    on_update,
                    ..
                } = c
                {
                    if columns.len() == 1 && ref_columns.len() == 1 {
                        return Some((
                            columns[0].as_str().to_owned(),
                            (ref_table.as_str(), on_delete.as_ref(), on_update.as_ref()),
                        ));
                    }
                }
                None
            })
            .collect();

    // Enum class names for this table's columns
    let enum_class_map: HashMap<&str, String> = table
        .columns
        .iter()
        .filter_map(|col| {
            if let ColumnType::Complex(ComplexColumnType::Enum { name, .. }) = &col.r#type {
                Some((col.name.as_str(), to_pascal_case(name)))
            } else {
                None
            }
        })
        .collect();

    // --- Enum class definitions ---
    let mut seen_enums: HashSet<String> = HashSet::new();
    for col in &table.columns {
        if let ColumnType::Complex(ComplexColumnType::Enum { name, values }) = &col.r#type {
            let class_name = to_pascal_case(name);
            if seen_enums.insert(class_name.clone()) {
                render_enum(&mut lines, &class_name, values);
                lines.push(String::new());
            }
        }
    }

    // --- Class declaration ---
    let class_name = to_pascal_case(&table.name);
    if let Some(ref desc) = table.description {
        lines.push(format!("class {class_name}(models.Model):"));
        lines.push(format!("    \"\"\"{}\"\"\"", desc.replace('\n', " ")));
        lines.push(String::new());
    } else {
        lines.push(format!("class {class_name}(models.Model):"));
    }

    // --- Fields ---
    for col in &table.columns {
        let is_pk = pk_columns.contains(col.name.as_str());
        let is_unique = single_unique_cols.contains(col.name.as_str());

        if let Some(ref comment) = col.comment {
            lines.push(format!("    # {}", comment.replace('\n', " ")));
        }

        if let Some(&(ref_table, on_delete, on_update)) = fk_map.get(col.name.as_str()) {
            render_fk_field(&mut lines, &col.name, ref_table, on_delete, on_update, col.nullable);
        } else {
            let effective_pk = is_pk && !is_composite_pk;
            let field_type = django_field_type(
                &col.r#type,
                effective_pk,
                auto_increment && !is_composite_pk,
            );
            let kwargs = build_field_kwargs(
                &col.r#type,
                effective_pk,
                auto_increment && !is_composite_pk,
                is_unique,
                col.nullable,
                col.default.as_ref(),
                enum_class_map.get(col.name.as_str()).map(String::as_str),
                used,
            );
            let kwargs_str = kwargs.join(", ");
            if kwargs_str.is_empty() {
                lines.push(format!("    {} = {}()", col.name, field_type));
            } else {
                lines.push(format!("    {} = {}({})", col.name, field_type, kwargs_str));
            }
        }
    }

    // --- Meta class ---
    let indexes: Vec<_> = table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::Index { name, columns } = c {
                Some((name.as_deref(), columns.as_slice()))
            } else {
                None
            }
        })
        .collect();

    let composite_uniques: Vec<_> = table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::Unique { name, columns, .. } = c {
                if columns.len() > 1 {
                    Some((name.as_deref(), columns.as_slice()))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    lines.push(String::new());
    lines.push("    class Meta:".into());
    lines.push(format!("        db_table = \"{}\"", table.name));

    if !indexes.is_empty() {
        lines.push("        indexes = [".into());
        for (name, cols) in &indexes {
            let fields = cols
                .iter()
                .map(|c| format!("\"{c}\""))
                .collect::<Vec<_>>()
                .join(", ");
            if let Some(n) = name {
                lines.push(format!("            models.Index(fields=[{fields}], name=\"{n}\"),"));
            } else {
                lines.push(format!("            models.Index(fields=[{fields}]),"));
            }
        }
        lines.push("        ]".into());
    }

    if !composite_uniques.is_empty() {
        lines.push("        constraints = [".into());
        for (name, cols) in &composite_uniques {
            let fields = cols
                .iter()
                .map(|c| format!("\"{c}\""))
                .collect::<Vec<_>>()
                .join(", ");
            if let Some(n) = name {
                lines.push(format!(
                    "            models.UniqueConstraint(fields=[{fields}], name=\"{n}\"),"
                ));
            } else {
                lines.push(format!(
                    "            models.UniqueConstraint(fields=[{fields}]),"
                ));
            }
        }
        lines.push("        ]".into());
    }

    lines.push(String::new());
    lines.join("\n")
}

fn render_fk_field(
    lines: &mut Vec<String>,
    col_name: &str,
    ref_table: &str,
    on_delete: Option<&ReferenceAction>,
    on_update: Option<&ReferenceAction>,
    nullable: bool,
) {
    let (field_name, db_column) = fk_field_name(col_name);
    let ref_class = to_pascal_case(ref_table);
    let on_delete_str = on_delete
        .map(reference_action_str)
        .unwrap_or("models.RESTRICT");

    let _ = on_update; // Django ForeignKey has no on_update param; silently ignored

    let mut kwargs = vec![
        format!("\"{ref_class}\""),
        format!("on_delete={on_delete_str}"),
    ];
    if let Some(db_col) = db_column {
        kwargs.push(format!("db_column=\"{db_col}\""));
    }
    kwargs.push("related_name=\"+\"".into());
    if nullable {
        kwargs.push("null=True".into());
        kwargs.push("blank=True".into());
    }

    let kwargs_str = kwargs.join(", ");
    lines.push(format!(
        "    {field_name} = models.ForeignKey({kwargs_str})"
    ));
}

/// Returns (field_name, Option<db_column>).
/// If col_name ends with `_id`, strip it — Django automatically appends `_id`.
/// Otherwise, emit db_column explicitly so Django uses the raw column name.
fn fk_field_name(col_name: &str) -> (String, Option<String>) {
    if let Some(base) = col_name.strip_suffix("_id") {
        (base.to_string(), None)
    } else {
        (col_name.to_string(), Some(col_name.to_string()))
    }
}

fn assemble_with_imports(used: &UsedImports, parts: &[String]) -> String {
    let mut lines: Vec<String> = Vec::new();

    lines.push("from __future__ import annotations".into());
    lines.push(String::new());

    if used.needs_timezone {
        lines.push("from django.utils import timezone".into());
    }
    if used.needs_uuid_default {
        lines.push("import uuid".into());
    }

    lines.push("from django.db import models".into());
    lines.push(String::new());
    lines.push(String::new());

    lines.push(parts.join("\n"));
    lines.join("\n")
}

pub(super) fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect()
}

pub(super) fn to_upper_snake_case(s: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c == '-' || c == ' ' {
            if !result.ends_with('_') {
                result.push('_');
            }
        } else if c == '_' {
            result.push('_');
        } else if c.is_uppercase() && i > 0 && !result.ends_with('_') {
            // Only split on camelCase transitions (lowercase/digit → uppercase).
            // Adjacent uppercase letters (e.g. "ERROR") are not split.
            let prev = chars[i - 1];
            if prev.is_lowercase() || prev.is_ascii_digit() {
                result.push('_');
            }
            result.push(c);
        } else {
            result.push(c.to_ascii_uppercase());
        }
    }
    // Python identifiers cannot start with a digit
    if result.starts_with(|c: char| c.is_ascii_digit()) {
        result.insert(0, '_');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    #[case("pending", "PENDING")]
    #[case("in_progress", "IN_PROGRESS")]
    #[case("inProgress", "IN_PROGRESS")]
    #[case("ERROR_LEVEL", "ERROR_LEVEL")]
    #[case("info-level", "INFO_LEVEL")]
    #[case("1critical", "_1CRITICAL")]
    fn test_to_upper_snake_case(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(to_upper_snake_case(input), expected);
    }

    #[rstest::rstest]
    #[case("author_id", "author", None)]
    #[case("user_id", "user", None)]
    #[case("parent", "parent", Some("parent"))]
    #[case("ref", "ref", Some("ref"))]
    fn test_fk_field_name(
        #[case] col: &str,
        #[case] expected_field: &str,
        #[case] expected_db_col: Option<&str>,
    ) {
        let (field, db_col) = fk_field_name(col);
        assert_eq!(field, expected_field);
        assert_eq!(db_col.as_deref(), expected_db_col);
    }
}
