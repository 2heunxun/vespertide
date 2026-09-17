use std::collections::{HashMap, HashSet};

use super::enums::render_enum;
use super::types::{UsedImports, build_field_kwargs, django_field_type, reference_action_str};
use crate::constraint_scan::{
    primary_key, primary_key_columns, single_column_fk_details, single_column_uniques,
};
use crate::utils::common::{claim_binding, collect_composite_fks};
use vespertide_core::schema::column::{ColumnType, ComplexColumnType};
use vespertide_core::schema::constraint::TableConstraint;
use vespertide_core::{ReferenceAction, TableDef};
use vespertide_naming::{
    IdentifierStart, build_unique_constraint_name, pluralize, sanitize_identifier,
};

pub fn render_entity(table: &TableDef) -> Result<String, String> {
    let mut used = UsedImports::default();
    let body = render_entity_part(table, &mut used, &[], None);
    Ok(assemble_with_imports(&used, &[body]))
}

/// Render a single table with full schema context so many-to-many junction
/// tables can be recognized and exposed as `ManyToManyField(..., through=...)`.
pub fn render_entity_with_schema(table: &TableDef, schema: &[TableDef]) -> Result<String, String> {
    render_entity_with_schema_and_config(table, schema, None)
}

/// Same as [`render_entity_with_schema`], but with an optional `app_label`
/// (from `vespertide.json`'s `django` config) written into every model's
/// `Meta` class.
pub fn render_entity_with_schema_and_config(
    table: &TableDef,
    schema: &[TableDef],
    app_label: Option<&str>,
) -> Result<String, String> {
    let mut used = UsedImports::default();
    let m2m_fields = find_many_to_many_fields(table, schema);
    let body = render_entity_part(table, &mut used, &m2m_fields, app_label);
    Ok(assemble_with_imports(&used, &[body]))
}

pub fn export(schema: &[TableDef]) -> Result<String, String> {
    export_with_config(schema, None)
}

/// Same as [`export`], but with an optional `app_label` written into every
/// model's `Meta` class.
pub fn export_with_config(schema: &[TableDef], app_label: Option<&str>) -> Result<String, String> {
    let mut used = UsedImports::default();
    let parts: Vec<String> = schema
        .iter()
        .map(|t| {
            let m2m_fields = find_many_to_many_fields(t, schema);
            render_entity_part(t, &mut used, &m2m_fields, app_label)
        })
        .collect();
    Ok(assemble_with_imports(&used, &parts))
}

/// Recognize many-to-many junction tables (composite PK, 2+ FKs, all FK
/// columns part of the PK) that reference `table`, and render the
/// corresponding `ManyToManyField` lines for the *other* side of each
/// junction. Purely self-referential junctions (every FK pointing back at
/// `table`) are skipped rather than guessed at.
fn find_many_to_many_fields(table: &TableDef, schema: &[TableDef]) -> Vec<String> {
    let mut matches: Vec<(String, String)> = Vec::new(); // (target_table, junction_table)

    for other in schema {
        if other.name == table.name {
            continue;
        }

        let other_pk: HashSet<String> = other
            .constraints
            .iter()
            .filter_map(|c| {
                if let TableConstraint::PrimaryKey { columns, .. } = c {
                    Some(
                        columns
                            .iter()
                            .map(|c| c.as_str().to_owned())
                            .collect::<Vec<_>>(),
                    )
                } else {
                    None
                }
            })
            .flatten()
            .collect();
        if other_pk.len() < 2 {
            continue;
        }

        let fks: Vec<(Vec<String>, String)> = other
            .constraints
            .iter()
            .filter_map(|c| {
                if let TableConstraint::ForeignKey {
                    columns, ref_table, ..
                } = c
                {
                    Some((
                        columns.iter().map(|c| c.as_str().to_owned()).collect(),
                        ref_table.as_str().to_owned(),
                    ))
                } else {
                    None
                }
            })
            .collect();
        if fks.len() < 2 {
            continue;
        }

        let all_fk_cols_in_pk = fks
            .iter()
            .all(|(cols, _)| cols.iter().all(|c| other_pk.contains(c.as_str())));
        if !all_fk_cols_in_pk {
            continue;
        }

        if !fks
            .iter()
            .any(|(_, ref_table)| ref_table.as_str() == table.name.as_str())
        {
            continue;
        }
        if fks
            .iter()
            .all(|(_, ref_table)| ref_table.as_str() == table.name.as_str())
        {
            continue;
        }

        for (_, ref_table) in &fks {
            if ref_table.as_str() == table.name.as_str() {
                continue;
            }
            if schema.iter().any(|t| t.name.as_str() == ref_table.as_str()) {
                matches.push((ref_table.clone(), other.name.as_str().to_owned()));
            }
        }
    }

    let mut target_counts: HashMap<String, usize> = HashMap::new();
    for (target, _) in &matches {
        *target_counts.entry(target.clone()).or_default() += 1;
    }

    let mut used_names: HashSet<String> = HashSet::new();
    matches
        .iter()
        .map(|(target, junction)| {
            let base = pluralize(target);
            let field_name = if target_counts.get(target).copied().unwrap_or(0) > 1 {
                claim_binding(format!("{base}_via_{junction}"), &mut used_names)
            } else {
                claim_binding(base, &mut used_names)
            };
            let target_class = sanitize_identifier(&to_pascal_case(target), IdentifierStart::Underscore);
            let junction_class =
                sanitize_identifier(&to_pascal_case(junction), IdentifierStart::Underscore);
            format!(
                "    {field_name} = models.ManyToManyField(\"{target_class}\", through=\"{junction_class}\", related_name=\"+\")"
            )
        })
        .collect()
}

fn render_entity_part(
    table: &TableDef,
    used: &mut UsedImports,
    extra_fields: &[String],
    app_label: Option<&str>,
) -> String {
    let mut lines: Vec<String> = Vec::new();

    // --- Constraint lookups ---
    let pk_columns = primary_key_columns(&table.constraints);

    let auto_increment = table.constraints.iter().any(|c| {
        matches!(
            c,
            TableConstraint::PrimaryKey {
                auto_increment: true,
                ..
            }
        )
    });

    let is_composite_pk = pk_columns.len() > 1;

    // Column order (not just membership) matters for CompositePrimaryKey's
    // positional args, so capture it separately from the `pk_columns` set.
    let pk_columns_ordered = primary_key(&table.constraints)
        .map(TableConstraint::columns)
        .unwrap_or_default();

    let single_unique_cols = single_column_uniques(&table.constraints);
    let fk_map = single_column_fk_details(&table.constraints);

    // Enum class names for this table's columns
    let enum_class_map: HashMap<&str, String> = table
        .columns
        .iter()
        .filter_map(|col| {
            if let ColumnType::Complex(ComplexColumnType::Enum { name, .. }) = &col.r#type {
                Some((
                    col.name.as_str(),
                    sanitize_identifier(&to_pascal_case(name), IdentifierStart::Underscore),
                ))
            } else {
                None
            }
        })
        .collect();

    // --- Enum class definitions ---
    let mut seen_enums: HashSet<String> = HashSet::new();
    for col in &table.columns {
        if let ColumnType::Complex(ComplexColumnType::Enum { name, values }) = &col.r#type {
            let class_name =
                sanitize_identifier(&to_pascal_case(name), IdentifierStart::Underscore);
            if seen_enums.insert(class_name.clone()) {
                render_enum(&mut lines, &class_name, values);
                lines.push(String::new());
            }
        }
    }

    // --- Class declaration ---
    let class_name = sanitize_identifier(&to_pascal_case(&table.name), IdentifierStart::Underscore);
    if let Some(ref desc) = table.description {
        lines.push(format!("class {class_name}(models.Model):"));
        lines.push(format!("    \"\"\"{}\"\"\"", desc.replace('\n', " ")));
        lines.push(String::new());
    } else {
        lines.push(format!("class {class_name}(models.Model):"));
    }

    // Composite PK: Django (5.2+) represents this natively via
    // `pk = models.CompositePrimaryKey(...)`, referencing each column by its
    // attname (a ForeignKey's attname is always `{field_name}_id`, regardless
    // of any `db_column` override). Without this, Django would fall back to
    // adding its own implicit auto `id` PK, which doesn't correspond to any
    // real uniqueness constraint on the actual table.
    // Rendered after the fields, which is where the attnames come from,
    // but emitted here at the top of the class body.
    let composite_pk_at = lines.len();

    // --- Fields ---
    // Sanitizing distinct column names (e.g. `a_id` -> `a`, `a` -> `a`) can
    // collapse two originally-distinct columns onto the same Python
    // attribute name; disambiguate with a numeric suffix rather than
    // silently emitting a duplicate class attribute.
    let mut used_field_names: HashSet<String> = HashSet::new();
    let mut attnames: HashMap<&str, String> = HashMap::new();
    for col in &table.columns {
        let is_pk = pk_columns.contains(col.name.as_str());
        let is_unique = single_unique_cols.contains(col.name.as_str());

        if let Some(ref comment) = col.comment {
            lines.push(format!("    # {}", comment.replace('\n', " ")));
        }

        let effective_pk = is_pk && !is_composite_pk;
        let attname = if let Some(fk) = fk_map.get(col.name.as_str()) {
            let field_name = render_fk_field(
                &mut lines,
                &col.name,
                fk.ref_table,
                fk.on_delete,
                fk.on_update,
                effective_pk,
                is_unique,
                col.nullable,
                &mut used_field_names,
            );
            // A ForeignKey's attname is `{field}_id` whatever `db_column` says.
            format!("{field_name}_id")
        } else {
            let field_type = django_field_type(
                &col.r#type,
                effective_pk,
                auto_increment && !is_composite_pk,
            );
            let field_name = claim_binding(
                sanitize_identifier(col.name.as_str(), IdentifierStart::Underscore),
                &mut used_field_names,
            );
            let db_column = if field_name == col.name.as_str() {
                None
            } else {
                Some(col.name.as_str())
            };
            let kwargs = build_field_kwargs(
                &col.r#type,
                effective_pk,
                is_unique,
                col.nullable,
                col.default.as_ref(),
                enum_class_map.get(col.name.as_str()).map(String::as_str),
                db_column,
                used,
            );
            let kwargs_str = kwargs.join(", ");
            if kwargs_str.is_empty() {
                lines.push(format!("    {field_name} = {field_type}()"));
            } else {
                lines.push(format!("    {field_name} = {field_type}({kwargs_str})"));
            }
            field_name
        };
        attnames.insert(col.name.as_str(), attname);
    }

    if is_composite_pk {
        let args = pk_columns_ordered
            .iter()
            .map(|col| format!("\"{}\"", attname_of(&attnames, col.as_str())))
            .collect::<Vec<_>>()
            .join(", ");
        lines.insert(
            composite_pk_at,
            format!("    pk = models.CompositePrimaryKey({args})"),
        );
    }

    for line in extra_fields {
        lines.push(line.clone());
    }

    // Composite (multi-column) FKs have no native Django ORM field — surface
    // them as a comment rather than silently dropping the relationship info.
    // The individual columns still render above as plain scalar fields, and
    // referential integrity is enforced by the generated database schema.
    for fk in collect_composite_fks(table) {
        let local = fk.local_cols.join(", ");
        let refs = fk.ref_cols.join(", ");
        lines.push(format!(
            "    # composite foreign key: ({local}) -> {}({refs})",
            fk.ref_table
        ));
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
    if let Some(label) = app_label {
        lines.push(format!("        app_label = \"{label}\""));
    }

    if !indexes.is_empty() {
        lines.push("        indexes = [".into());
        for (name, cols) in &indexes {
            let fields = cols
                .iter()
                .map(|c| format!("\"{}\"", attname_of(&attnames, c)))
                .collect::<Vec<_>>()
                .join(", ");
            if let Some(n) = name {
                lines.push(format!(
                    "            models.Index(fields=[{fields}], name=\"{n}\"),"
                ));
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
                .map(|c| format!("\"{}\"", attname_of(&attnames, c)))
                .collect::<Vec<_>>()
                .join(", ");
            // `name` is required on every Django constraint, so an unnamed
            // source constraint takes the name the SQL layer gives it.
            let n = name.map_or_else(
                || build_unique_constraint_name(&table.name, cols, None),
                str::to_string,
            );
            lines.push(format!(
                "            models.UniqueConstraint(fields=[{fields}], name=\"{n}\"),"
            ));
        }
        lines.push("        ]".into());
    }

    lines.push(String::new());
    lines.join("\n")
}

#[expect(
    clippy::too_many_arguments,
    reason = "all params are independent field-rendering inputs; a context struct would add noise without reducing coupling"
)]
fn render_fk_field(
    lines: &mut Vec<String>,
    col_name: &str,
    ref_table: &str,
    on_delete: Option<&ReferenceAction>,
    on_update: Option<&ReferenceAction>,
    is_pk: bool,
    is_unique: bool,
    nullable: bool,
    used_field_names: &mut HashSet<String>,
) -> String {
    let (field_name, db_column) = fk_field_name(col_name);
    // The `_id` strip can collapse two distinct columns onto the same
    // attribute name (e.g. `a_id` -> `a` colliding with a real column `a`).
    let deduped_field_name = claim_binding(field_name.clone(), used_field_names);
    let db_column =
        db_column.or_else(|| (deduped_field_name != field_name).then(|| col_name.to_string()));
    let field_name = deduped_field_name;
    let ref_class = sanitize_identifier(&to_pascal_case(ref_table), IdentifierStart::Underscore);
    let on_delete_str = on_delete.map_or("models.RESTRICT", reference_action_str);

    let _ = on_update; // Django ForeignKey has no on_update param; silently ignored

    let mut kwargs = vec![
        format!("\"{ref_class}\""),
        format!("on_delete={on_delete_str}"),
    ];
    if is_pk {
        kwargs.push("primary_key=True".into());
    }
    if let Some(db_col) = db_column {
        kwargs.push(format!("db_column=\"{db_col}\""));
    }
    kwargs.push("related_name=\"+\"".into());
    if nullable && !is_pk {
        kwargs.push("null=True".into());
        kwargs.push("blank=True".into());
    }

    // A FK that is the PK or unique holds at most one row per target: Django's
    // one-to-one. `ForeignKey(unique=True)` only draws fields.W342 pointing here.
    let field_class = if is_pk || is_unique {
        "models.OneToOneField"
    } else {
        "models.ForeignKey"
    };
    let kwargs_str = kwargs.join(", ");
    lines.push(format!("    {field_name} = {field_class}({kwargs_str})"));
    field_name
}

/// What Django calls a column inside `Meta.indexes`, `Meta.constraints` and
/// `CompositePrimaryKey`: the declared field name, or a ForeignKey's attname.
/// Those three resolve against field names only — never `db_column` — so a
/// column whose name had to be escaped is unreachable under its database
/// spelling.
fn attname_of<'a>(attnames: &'a HashMap<&str, String>, column: &'a str) -> &'a str {
    attnames.get(column).map_or(column, String::as_str)
}

/// Returns (field_name, Option<db_column>).
/// If col_name ends with `_id`, strip it — Django automatically appends `_id`.
/// Otherwise, emit db_column explicitly so Django uses the raw column name.
/// Either way, `field_name` is sanitized into a valid Python identifier; if
/// that sanitization (or the `_id` strip) changes anything, `db_column` is
/// set to the original column name so the DB mapping isn't lost.
fn fk_field_name(col_name: &str) -> (String, Option<String>) {
    if let Some(base) = col_name.strip_suffix("_id") {
        let sanitized = sanitize_identifier(base, IdentifierStart::Underscore);
        if sanitized == base {
            (sanitized, None)
        } else {
            (sanitized, Some(col_name.to_string()))
        }
    } else {
        (
            sanitize_identifier(col_name, IdentifierStart::Underscore),
            Some(col_name.to_string()),
        )
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

pub(super) use crate::python_naming::to_pascal_case;

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_to_pascal_case_double_underscore() {
        // An empty segment between two underscores contributes nothing
        assert_eq!(to_pascal_case("order__item"), "OrderItem");
        assert_eq!(to_pascal_case("_leading"), "Leading");
        assert_eq!(to_pascal_case("trailing_"), "Trailing");
    }
}
