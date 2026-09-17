use std::collections::{HashMap, HashSet};

use super::enums::render_enum;
use super::types::{UsedImports, go_type_for_column_mapped};
use crate::constraint_scan::{
    FkDetails, primary_key_columns, single_column_fk_details, single_column_uniques,
};
use crate::utils::common::{CompositeFk, claim_binding, collect_composite_fks};
use vespertide_core::schema::column::{
    ColumnType, ComplexColumnType, EnumValues, SimpleColumnType,
};
use vespertide_core::schema::constraint::TableConstraint;
use vespertide_core::schema::names::ColumnName;
use vespertide_core::{ColumnDef, DefaultValue, ReferenceAction, TableDef};
use vespertide_naming::{IdentifierStart, pluralize, sanitize_identifier};

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

    let struct_name = exported_go_name(&table.name);

    // Find enum names that appear in multiple schema tables (need qualified Go type names)
    let conflicting_enums: HashSet<String> = {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for col in &table.columns {
            if let ColumnType::Complex(ComplexColumnType::Enum { name, .. }) = &col.r#type {
                counts.entry(exported_go_name(name)).or_insert(1);
            }
        }
        for other in schema {
            if other.name == table.name {
                continue;
            }
            let mut seen = HashSet::new();
            for col in &other.columns {
                if let ColumnType::Complex(ComplexColumnType::Enum { name, .. }) = &col.r#type {
                    let pascal = exported_go_name(name);
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
                let pascal = exported_go_name(name);
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

    let fk_by_column = single_column_fk_details(&table.constraints);
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

    let single_unique_columns = single_column_uniques(&table.constraints);

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

    // One set of taken names for the whole struct, columns first: no relation
    // field — belongs-to, composite or has-many — may take a column's name,
    // whichever order the table declares them in.
    let field_names = column_field_names(table);
    let mut taken: HashSet<String> = field_names.values().cloned().collect();

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
            &field_names[col.name.as_str()],
            is_pk,
            auto_increment && !is_composite_pk,
            is_unique,
            indexes,
            composite_unique_name,
            &enum_name_map,
        );

        if let Some(fk) = fk_by_column.get(col.name.as_str()) {
            render_fk_relation_field(
                &mut lines,
                col,
                &field_names[col.name.as_str()],
                fk,
                &mut taken,
            );
        }
    }

    // Composite (multi-column) FK relation fields. GORM supports composite
    // associations via comma-separated `foreignKey`/`references` tags, unlike
    // Django which has no native equivalent.
    for fk in collect_composite_fks(table) {
        render_composite_fk_relation_field(&mut lines, &fk, &field_names, schema, &mut taken);
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
        let fk_field = field_name_in(schema, &rel.ref_table, &rel.fk_column);
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
            field_name = claim_binding(rel.field_name.clone(), &mut taken),
            ref_struct = exported_go_name(&rel.ref_table),
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
                    exported_go_name(&pluralize(other.name.as_str()))
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
    field_name: &str,
    is_pk: bool,
    auto_increment: bool,
    is_unique: bool,
    indexes: &[IndexInfo],
    composite_unique_name: Option<&String>,
    enum_name_map: &HashMap<&str, String>,
) {
    let go_type = go_type_for_column_mapped(&col.r#type, col.nullable, enum_name_map);
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
    fk_field_name: &str,
    fk: &FkDetails,
    taken: &mut HashSet<String>,
) {
    let ref_struct = exported_go_name(fk.ref_table);
    let mut relation_field_name = go_relation_field_name(&col.name);
    if relation_field_name == fk_field_name {
        relation_field_name = format!("{relation_field_name}{ref_struct}");
    }
    // The name above only rules out colliding with this FK's own scalar
    // field; it can still collide with an unrelated real column (or another
    // relation) elsewhere in the table.
    let relation_field_name = claim_binding(relation_field_name, taken);

    let mut constraint_parts: Vec<String> = Vec::new();
    if let Some(action) = fk.on_delete {
        constraint_parts.push(format!("OnDelete:{}", action.to_sql_keyword()));
    }
    if let Some(action) = fk.on_update {
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
    fk: &CompositeFk,
    field_names: &HashMap<&str, String>,
    schema: &[TableDef],
    taken: &mut HashSet<String>,
) {
    let ref_struct = exported_go_name(fk.ref_table);

    let relation_field_name = claim_binding(ref_struct.clone(), taken);

    let fk_fields: Vec<String> = fk
        .local_cols
        .iter()
        .map(|c| field_names[*c].clone())
        .collect();
    let ref_fields: Vec<String> = fk
        .ref_cols
        .iter()
        .map(|c| field_name_in(schema, fk.ref_table, c))
        .collect();

    let mut constraint_parts: Vec<String> = Vec::new();
    if let Some(action) = fk.on_delete {
        constraint_parts.push(format!("OnDelete:{}", action.to_sql_keyword()));
    }
    if let Some(action) = fk.on_update {
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

/// Exported Go identifier for a database name: PascalCase, with a digit-led
/// start given an upper-case letter prefix. GORM skips unexported struct
/// fields, and a `_`-led type is unreachable from other packages.
pub(super) fn exported_go_name(s: &str) -> String {
    export(&to_pascal_case(s))
}

/// Go field name for a column: [`exported_go_name`] with Go's `ID` initialism.
pub(super) fn to_go_field_name(s: &str) -> String {
    export(&go_initialisms(&to_pascal_case(s)))
}

/// Go field name for every column of `table`, claimed in declaration order so
/// two columns that map to one Go name (`user_id`, `userId`) get distinct
/// fields.
pub(super) fn column_field_names(table: &TableDef) -> HashMap<&str, String> {
    let mut taken = HashSet::new();
    table
        .columns
        .iter()
        .map(|col| {
            (
                col.name.as_str(),
                claim_binding(to_go_field_name(&col.name), &mut taken),
            )
        })
        .collect()
}

/// The field name `column` has in `table_name`'s struct: its claimed name when
/// that table is part of `schema`, the plain derivation otherwise (a
/// single-table render knows nothing about its FK targets).
fn field_name_in(schema: &[TableDef], table_name: &str, column: &str) -> String {
    schema
        .iter()
        .find(|t| t.name.as_str() == table_name)
        .and_then(|t| column_field_names(t).remove(column))
        .unwrap_or_else(|| to_go_field_name(column))
}

/// Go field name for a belongs-to relation: the FK column without its `_id`
/// suffix, in PascalCase.
pub(super) fn go_relation_field_name(fk_column: &str) -> String {
    exported_go_name(vespertide_naming::infer_relation_field_name(fk_column))
}

fn export(pascal: &str) -> String {
    let mut name = sanitize_identifier(pascal, IdentifierStart::Letter);
    // `Letter` copies the case of the first letter it finds (`x1users`), and Go
    // exports by case. The first byte is always an ASCII letter after
    // sanitizing, so the slice cannot split a character.
    name[..1].make_ascii_uppercase();
    name
}

/// Every `Id` that ends a PascalCase word becomes `ID`, as Go spells the
/// initialism; `Identity` and `Idx` keep their words.
fn go_initialisms(pascal: &str) -> String {
    let chars: Vec<char> = pascal.chars().collect();
    let mut out = String::with_capacity(pascal.len());
    let mut i = 0;
    while i < chars.len() {
        let ends_word = chars.get(i + 2).is_none_or(|c| !c.is_ascii_lowercase());
        if chars[i] == 'I' && chars.get(i + 1) == Some(&'d') && ends_word {
            out.push_str("ID");
            i += 2;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}
