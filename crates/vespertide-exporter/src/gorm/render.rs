use std::collections::{HashMap, HashSet};

use super::enums::render_enum;
use super::types::{UsedImports, go_type_for_column_mapped};
use crate::utils::common::claim_binding;
use vespertide_core::schema::column::{
    ColumnType, ComplexColumnType, EnumValues, SimpleColumnType,
};
use vespertide_core::schema::constraint::TableConstraint;
use vespertide_core::schema::names::ColumnName;
use vespertide_core::{ColumnDef, DefaultValue, ReferenceAction, TableDef};
use vespertide_naming::{IdentifierStart, sanitize_identifier};

/// The Go imports the columns of `tables` need.
pub(super) fn imports_for<'a>(tables: impl IntoIterator<Item = &'a TableDef>) -> UsedImports {
    let mut used = UsedImports::default();
    for col in tables.into_iter().flat_map(|table| &table.columns) {
        used.add_column_type(&col.r#type);
    }
    used
}

/// The `package` clause and the import block, stdlib first.
pub(super) fn render_header(package_name: &str, used_imports: &UsedImports) -> Vec<String> {
    let mut lines = vec![format!("package {package_name}"), String::new()];

    let has_stdlib = used_imports.needs_time;
    let has_external =
        used_imports.needs_uuid || used_imports.needs_datatypes || used_imports.needs_decimal;

    if has_stdlib || has_external {
        lines.push("import (".into());
        if has_stdlib {
            lines.push("    \"time\"".into());
        }
        if has_stdlib && has_external {
            lines.push(String::new());
        }
        if used_imports.needs_datatypes {
            lines.push("    \"gorm.io/datatypes\"".into());
        }
        if used_imports.needs_uuid {
            lines.push("    \"github.com/google/uuid\"".into());
        }
        if used_imports.needs_decimal {
            lines.push("    \"github.com/shopspring/decimal\"".into());
        }
        lines.push(")".into());
        lines.push(String::new());
    }
    lines
}

/// Everything below the header for one table: enum types, the struct, and
/// its methods.
pub(super) fn render_table_body(table: &TableDef, schema: &[TableDef]) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();

    let struct_name =
        sanitize_identifier(&to_pascal_case(&table.name), IdentifierStart::Underscore);

    // Find enum names that appear in multiple schema tables (need qualified Go type names)
    let conflicting_enums: HashSet<String> = {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for col in &table.columns {
            if let ColumnType::Complex(ComplexColumnType::Enum { name, .. }) = &col.r#type {
                counts
                    .entry(sanitize_identifier(
                        &to_pascal_case(name),
                        IdentifierStart::Underscore,
                    ))
                    .or_insert(1);
            }
        }
        for other in schema {
            if other.name == table.name {
                continue;
            }
            let mut seen = HashSet::new();
            for col in &other.columns {
                if let ColumnType::Complex(ComplexColumnType::Enum { name, .. }) = &col.r#type {
                    let pascal =
                        sanitize_identifier(&to_pascal_case(name), IdentifierStart::Underscore);
                    if seen.insert(pascal.clone()) {
                        *counts.entry(pascal).or_default() += 1;
                    }
                }
            }
        }
        counts
            .into_iter()
            .filter(|(_, c)| *c > 1)
            .map(|(n, _)| n)
            .collect()
    };

    // Collect enums defined in this table's columns, with qualified names where needed
    let enums: Vec<(&str, &EnumValues, String)> = table
        .columns
        .iter()
        .filter_map(|col| {
            if let ColumnType::Complex(ComplexColumnType::Enum { name, values }) = &col.r#type {
                let pascal =
                    sanitize_identifier(&to_pascal_case(name), IdentifierStart::Underscore);
                let qualified = if conflicting_enums.contains(&pascal) {
                    format!("{struct_name}{pascal}")
                } else {
                    pascal
                };
                Some((name.as_str(), values, qualified))
            } else {
                None
            }
        })
        .collect();
    let enum_name_map: HashMap<&str, String> = enums
        .iter()
        .map(|(name, _, qualified)| (*name, qualified.clone()))
        .collect();

    let fk_by_column = collect_fk_info(&table.constraints);

    let pk_columns: HashSet<String> = table
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
        .map(|c| c.as_str().to_owned())
        .collect();

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

    let single_unique_columns: HashSet<String> = table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::Unique { columns, .. } = c {
                if columns.len() == 1 {
                    Some(columns[0].as_str().to_owned())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    let index_map = collect_index_info(&table.constraints);
    let composite_unique_map = collect_composite_unique_info(&table.constraints);

    let reverse_relations = find_reverse_relations(&table.name, schema);

    // --- Enum type declarations ---
    // Two columns of one table may share an enum; Go rejects the second
    // declaration of the same type.
    let mut declared_enums: HashSet<&str> = HashSet::new();
    for (_, values, qualified_name) in &enums {
        if !declared_enums.insert(qualified_name.as_str()) {
            continue;
        }
        render_enum(&mut lines, qualified_name, values);
        lines.push(String::new());
    }

    // --- Struct definition ---
    if let Some(ref desc) = table.description {
        lines.push(format!("// {}", desc.replace('\n', " ")));
    }

    lines.push(format!("type {struct_name} struct {{"));

    // Every real column's field name is reserved up front so belongs-to
    // relation fields (single-column and composite) can detect a collision
    // regardless of which column — FK or plain — happens to come first in
    // the table definition.
    let used_field_names: HashSet<String> = table
        .columns
        .iter()
        .map(|c| to_go_field_name(&c.name))
        .collect();
    let mut used_relation_names = used_field_names.clone();

    for col in &table.columns {
        let is_pk = pk_columns.contains(col.name.as_str());
        let is_unique = single_unique_columns.contains(col.name.as_str());
        let indexes = index_map
            .get(col.name.as_str())
            .map_or(&[][..], Vec::as_slice);
        let composite_unique_name = composite_unique_map.get(col.name.as_str());

        if let Some(ref comment) = col.comment {
            lines.push(format!("    // {}", comment.replace('\n', " ")));
        }

        render_column_field(
            &mut lines,
            col,
            is_pk,
            auto_increment && !is_composite_pk,
            is_unique,
            indexes,
            composite_unique_name,
            &enum_name_map,
        );

        if let Some(fk) = fk_by_column.get(col.name.as_str()) {
            render_fk_relation_field(&mut lines, col, fk, &mut used_relation_names);
        }
    }

    // Composite (multi-column) FK relation fields. GORM supports composite
    // associations via comma-separated `foreignKey`/`references` tags, unlike
    // Django which has no native equivalent.
    for fk in collect_composite_fk_info(&table.constraints) {
        render_composite_fk_relation_field(&mut lines, &fk, &mut used_relation_names);
    }

    // Reverse relation fields (HasMany) derived from schema context
    for rel in &reverse_relations {
        let mut constraint_parts: Vec<String> = Vec::new();
        if let Some(ref action) = rel.on_delete {
            constraint_parts.push(format!("OnDelete:{}", action.to_sql_keyword()));
        }
        if let Some(ref action) = rel.on_update {
            constraint_parts.push(format!("OnUpdate:{}", action.to_sql_keyword()));
        }
        let fk_field = to_go_field_name(&rel.fk_column);
        let gorm_tag = if constraint_parts.is_empty() {
            format!("foreignKey:{fk_field}")
        } else {
            format!(
                "foreignKey:{fk_field};constraint:{}",
                constraint_parts.join(",")
            )
        };
        lines.push(format!(
            "    {field_name} []{ref_struct} `gorm:\"{gorm_tag}\" json:\"-\"`",
            field_name = rel.field_name,
            ref_struct =
                sanitize_identifier(&to_pascal_case(&rel.ref_table), IdentifierStart::Underscore),
        ));
    }

    lines.push("}".into());
    lines.push(String::new());

    // GORM would otherwise derive the table name by pluralizing the struct
    // name, which does not reproduce an arbitrary database name.
    lines.push(format!(
        "func ({struct_name}) TableName() string {{ return \"{name}\" }}",
        name = table.name,
    ));
    lines.push(String::new());

    lines
}

// ---------------------------------------------------------------------------
// FK info collection
// ---------------------------------------------------------------------------

struct FkInfo {
    ref_table: String,
    on_delete: Option<ReferenceAction>,
    on_update: Option<ReferenceAction>,
}

struct CompositeFkInfo {
    local_cols: Vec<String>,
    ref_table: String,
    ref_cols: Vec<String>,
    on_delete: Option<ReferenceAction>,
    on_update: Option<ReferenceAction>,
}

fn collect_composite_fk_info(constraints: &[TableConstraint]) -> Vec<CompositeFkInfo> {
    constraints
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
                && columns.len() > 1
                && columns.len() == ref_columns.len()
            {
                return Some(CompositeFkInfo {
                    local_cols: columns.iter().map(|c| c.as_str().to_owned()).collect(),
                    ref_table: ref_table.as_str().to_owned(),
                    ref_cols: ref_columns.iter().map(|c| c.as_str().to_owned()).collect(),
                    on_delete: on_delete.clone(),
                    on_update: on_update.clone(),
                });
            }
            None
        })
        .collect()
}

fn collect_fk_info(constraints: &[TableConstraint]) -> HashMap<String, FkInfo> {
    constraints
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
                    Some((
                        columns[0].as_str().to_owned(),
                        FkInfo {
                            ref_table: ref_table.as_str().to_owned(),
                            on_delete: on_delete.clone(),
                            on_update: on_update.clone(),
                        },
                    ))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Index info collection
// ---------------------------------------------------------------------------

struct IndexInfo {
    name: Option<String>,
}

fn collect_index_info(constraints: &[TableConstraint]) -> HashMap<String, Vec<IndexInfo>> {
    let mut map: HashMap<String, Vec<IndexInfo>> = HashMap::new();
    for c in constraints {
        if let TableConstraint::Index { name, columns } = c {
            for col in columns {
                map.entry(col.as_str().to_owned())
                    .or_default()
                    .push(IndexInfo {
                        name: name.as_ref().map(|n| n.as_str().to_owned()),
                    });
            }
        }
    }
    map
}

fn collect_composite_unique_info(constraints: &[TableConstraint]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for c in constraints {
        if let TableConstraint::Unique { name, columns, .. } = c
            && columns.len() > 1
        {
            let uq_name = name.as_ref().map_or_else(
                || {
                    let parts: Vec<&str> = columns.iter().map(ColumnName::as_str).collect();
                    format!("uq_{}", parts.join("_"))
                },
                |n| n.as_str().to_owned(),
            );
            for col in columns {
                map.insert(col.as_str().to_owned(), uq_name.clone());
            }
        }
    }
    map
}

// ---------------------------------------------------------------------------
// Reverse relation discovery
// ---------------------------------------------------------------------------

struct ReverseRelation {
    field_name: String,
    ref_table: String,
    fk_column: String,
    on_delete: Option<ReferenceAction>,
    on_update: Option<ReferenceAction>,
}

fn find_reverse_relations(table_name: &str, schema: &[TableDef]) -> Vec<ReverseRelation> {
    type RawRelation = (
        String,
        String,
        String,
        Option<ReferenceAction>,
        Option<ReferenceAction>,
    );
    let mut raw: Vec<RawRelation> = Vec::new();
    for other in schema {
        // Note: self-referencing tables (other.name == table_name) are NOT
        // skipped here — a table's own FK column pointing back at itself
        // (e.g. categories.parent_id -> categories.id) must still produce a
        // reverse has-many ("Children") relation on the same struct.
        for c in &other.constraints {
            if let TableConstraint::ForeignKey {
                columns,
                ref_table,
                on_delete,
                on_update,
                ..
            } = c
                && ref_table.as_str() == table_name
                && columns.len() == 1
            {
                let fk_col = columns[0].as_str().to_owned();
                let is_self_ref = other.name.as_str() == table_name;
                let base_name = if is_self_ref {
                    "Children".to_string()
                } else {
                    let pascal = sanitize_identifier(
                        &to_pascal_case(other.name.as_str()),
                        IdentifierStart::Underscore,
                    );
                    if pascal.ends_with('s') {
                        pascal
                    } else {
                        format!("{pascal}s")
                    }
                };
                raw.push((
                    other.name.as_str().to_owned(),
                    fk_col,
                    base_name,
                    on_delete.clone(),
                    on_update.clone(),
                ));
            }
        }
    }

    let mut name_count: HashMap<String, usize> = HashMap::new();
    for (_, _, base_name, _, _) in &raw {
        *name_count.entry(base_name.clone()).or_default() += 1;
    }

    raw.into_iter()
        .map(|(ref_table, fk_col, base_name, on_delete, on_update)| {
            let field_name = if *name_count.get(&base_name).unwrap_or(&0) > 1 {
                format!("{}By{}", base_name, to_go_field_name(&fk_col))
            } else {
                base_name
            };
            ReverseRelation {
                field_name,
                ref_table,
                fk_column: fk_col,
                on_delete,
                on_update,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Field rendering
// ---------------------------------------------------------------------------

#[expect(
    clippy::too_many_arguments,
    reason = "all params are independent field-rendering inputs; a context struct would add noise without reducing coupling"
)]
fn render_column_field(
    lines: &mut Vec<String>,
    col: &ColumnDef,
    is_pk: bool,
    auto_increment: bool,
    is_unique: bool,
    indexes: &[IndexInfo],
    composite_unique_name: Option<&String>,
    enum_name_map: &HashMap<&str, String>,
) {
    let go_type = go_type_for_column_mapped(&col.r#type, col.nullable, enum_name_map);
    let field_name = to_go_field_name(&col.name);
    let gorm_tag = build_gorm_tag(
        col,
        is_pk,
        auto_increment,
        is_unique,
        indexes,
        composite_unique_name,
    );

    lines.push(format!(
        "    {field_name} {go_type} `gorm:\"{gorm_tag}\" json:\"{json_name}\"`",
        json_name = col.name,
    ));
}

fn render_fk_relation_field(
    lines: &mut Vec<String>,
    col: &ColumnDef,
    fk: &FkInfo,
    used_relation_names: &mut HashSet<String>,
) {
    let ref_struct =
        sanitize_identifier(&to_pascal_case(&fk.ref_table), IdentifierStart::Underscore);
    let fk_field_name = to_go_field_name(&col.name);
    let mut relation_field_name = infer_relation_field_name(&col.name);
    if relation_field_name == fk_field_name {
        relation_field_name = format!("{relation_field_name}{ref_struct}");
    }
    // The name above only rules out colliding with this FK's own scalar
    // field; it can still collide with an unrelated real column (or another
    // relation) elsewhere in the table.
    let relation_field_name = claim_binding(relation_field_name, used_relation_names);

    let mut constraint_parts: Vec<String> = Vec::new();
    if let Some(ref action) = fk.on_delete {
        constraint_parts.push(format!("OnDelete:{}", action.to_sql_keyword()));
    }
    if let Some(ref action) = fk.on_update {
        constraint_parts.push(format!("OnUpdate:{}", action.to_sql_keyword()));
    }

    let gorm_tag = if constraint_parts.is_empty() {
        format!("foreignKey:{fk_field_name}")
    } else {
        format!(
            "foreignKey:{fk_field_name};constraint:{}",
            constraint_parts.join(",")
        )
    };

    let type_expr = if col.nullable {
        format!("*{ref_struct}")
    } else {
        ref_struct
    };

    lines.push(format!(
        "    {relation_field_name} {type_expr} `gorm:\"{gorm_tag}\" json:\"-\"`"
    ));
}

/// Render a belongs-to relation field for a composite (multi-column) FK,
/// using GORM's comma-separated `foreignKey`/`references` tag syntax.
fn render_composite_fk_relation_field(
    lines: &mut Vec<String>,
    fk: &CompositeFkInfo,
    used_relation_names: &mut HashSet<String>,
) {
    let ref_struct =
        sanitize_identifier(&to_pascal_case(&fk.ref_table), IdentifierStart::Underscore);

    let relation_field_name = claim_binding(ref_struct.clone(), used_relation_names);

    let fk_fields: Vec<String> = fk.local_cols.iter().map(|c| to_go_field_name(c)).collect();
    let ref_fields: Vec<String> = fk.ref_cols.iter().map(|c| to_go_field_name(c)).collect();

    let mut constraint_parts: Vec<String> = Vec::new();
    if let Some(ref action) = fk.on_delete {
        constraint_parts.push(format!("OnDelete:{}", action.to_sql_keyword()));
    }
    if let Some(ref action) = fk.on_update {
        constraint_parts.push(format!("OnUpdate:{}", action.to_sql_keyword()));
    }

    let gorm_tag = if constraint_parts.is_empty() {
        format!(
            "foreignKey:{};references:{}",
            fk_fields.join(","),
            ref_fields.join(",")
        )
    } else {
        format!(
            "foreignKey:{};references:{};constraint:{}",
            fk_fields.join(","),
            ref_fields.join(","),
            constraint_parts.join(",")
        )
    };

    lines.push(format!(
        "    {relation_field_name} {ref_struct} `gorm:\"{gorm_tag}\" json:\"-\"`"
    ));
}

// ---------------------------------------------------------------------------
// GORM tag building
// ---------------------------------------------------------------------------

fn build_gorm_tag(
    col: &ColumnDef,
    is_pk: bool,
    auto_increment: bool,
    is_unique: bool,
    indexes: &[IndexInfo],
    composite_unique_name: Option<&String>,
) -> String {
    let mut parts: Vec<String> = vec![format!("column:{}", col.name)];

    if is_pk {
        parts.push("primaryKey".into());
    }
    if is_pk && auto_increment {
        parts.push("autoIncrement".into());
    }
    if !col.nullable && !is_pk {
        parts.push("not null".into());
    }
    if is_unique && !is_pk {
        parts.push("unique".into());
    }

    match &col.r#type {
        ColumnType::Simple(SimpleColumnType::Text) => parts.push("type:text".into()),
        ColumnType::Simple(SimpleColumnType::Xml) => parts.push("type:xml".into()),
        ColumnType::Simple(SimpleColumnType::Interval) => parts.push("type:interval".into()),
        ColumnType::Simple(SimpleColumnType::Date) => parts.push("type:date".into()),
        ColumnType::Simple(SimpleColumnType::Time) => parts.push("type:time".into()),
        ColumnType::Simple(SimpleColumnType::Uuid) => parts.push("type:uuid".into()),
        ColumnType::Complex(ComplexColumnType::Varchar { length }) => {
            parts.push(format!("size:{length}"));
        }
        ColumnType::Complex(ComplexColumnType::Char { length }) => {
            parts.push(format!("size:{length}"));
            parts.push("type:char".into());
        }
        ColumnType::Complex(ComplexColumnType::Numeric { precision, scale }) => {
            parts.push(format!("type:numeric({precision},{scale})"));
        }
        ColumnType::Complex(ComplexColumnType::Custom { custom_type }) => {
            parts.push(format!("type:{custom_type}"));
        }
        _ => {}
    }

    if let Some(ref default) = col.default
        && let Some(tag) = build_default_tag(default)
    {
        parts.push(tag);
    }

    for idx in indexes {
        if let Some(ref name) = idx.name {
            parts.push(format!("index:{name}"));
        } else {
            parts.push("index".into());
        }
    }

    if let Some(uq_name) = composite_unique_name {
        parts.push(format!("uniqueIndex:{uq_name}"));
    }

    parts.join(";")
}

fn build_default_tag(default: &DefaultValue) -> Option<String> {
    let sql = default.to_sql();
    if sql.contains('(') {
        return None; // Skip server-side function calls like NOW()
    }
    Some(format!("default:{sql}"))
}

// ---------------------------------------------------------------------------
// Naming utilities
// ---------------------------------------------------------------------------

pub(super) use crate::python_naming::to_pascal_case;

pub(super) fn to_go_field_name(s: &str) -> String {
    let pascal = to_pascal_case(s);
    // Apply Go conventions for common abbreviations
    let pascal = pascal.replace("Id", "ID");
    // Go identifiers can't start with a digit or contain non-alphanumeric
    // characters; a leading `_` is legal (matches Rust module / Java field
    // escaping elsewhere in the exporter).
    sanitize_identifier(&pascal, IdentifierStart::Underscore)
}

pub(super) fn infer_relation_field_name(fk_column: &str) -> String {
    let base = fk_column.strip_suffix("_id").unwrap_or(fk_column);
    sanitize_identifier(&to_pascal_case(base), IdentifierStart::Underscore)
}
