use std::collections::{HashMap, HashSet};

use super::enums::render_enum;
use super::types::{UsedImports, go_type_for_column_mapped};
use crate::constraint_scan::{
    BackRelation, FkDetails, collect_back_relations, primary_key_columns, single_column_fk_details,
    single_column_uniques,
};
use crate::enum_scan::enum_identifiers_shared_across_tables;
use crate::utils::common::{
    CompositeFk, claim_binding, collect_composite_fks, integer_enum_variant_value, unquote,
};
use vespertide_core::schema::column::{
    ColumnType, ComplexColumnType, EnumValues, SimpleColumnType,
};
use vespertide_core::schema::constraint::TableConstraint;
use vespertide_core::{ColumnDef, DefaultValue, ReferenceAction, TableDef};
use vespertide_naming::{
    IdentifierStart, build_index_name, build_unique_constraint_name, pluralize, sanitize_identifier,
};

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

    // Enum names that appear in multiple schema tables need qualified Go type names
    let conflicting_enums = enum_identifiers_shared_across_tables(schema, exported_go_name);

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

    let index_map = collect_index_names(table);
    let composite_unique_map = collect_composite_unique_names(table);

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
                schema,
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

    // Reverse relation fields (has-one / has-many) derived from schema context
    let back_relations = collect_back_relations(&table.name, schema);
    let reverse_names = reverse_field_names(&table.name, &back_relations);
    for (rel, field_name) in back_relations.iter().zip(reverse_names) {
        let foreign_key: Vec<String> = rel
            .fk_columns
            .iter()
            .map(|c| field_name_in(schema, &rel.source_table, c))
            .collect();
        let ref_columns: Vec<&str> = rel.ref_columns.iter().map(String::as_str).collect();
        let gorm_tag = relation_tag(
            &foreign_key,
            &reference_fields(schema, &table.name, &ref_columns),
            rel.on_delete.as_ref(),
            rel.on_update.as_ref(),
        );
        let source_struct = exported_go_name(&rel.source_table);
        let go_type = if rel.is_one_to_one {
            format!("*{source_struct}")
        } else {
            format!("[]{source_struct}")
        };
        lines.push(format!(
            "    {field_name} {go_type} `gorm:\"{gorm_tag}\" json:\"-\"`",
            field_name = claim_binding(field_name, &mut taken),
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
// Index / unique names
// ---------------------------------------------------------------------------

/// Index names per column, spelled as the SQL layer spells them so
/// `AutoMigrate` finds the index the migration created instead of adding a
/// second one. Every column of a composite index carries the same name, which
/// is how GORM groups them.
fn collect_index_names(table: &TableDef) -> HashMap<&str, Vec<String>> {
    let mut map: HashMap<&str, Vec<String>> = HashMap::new();
    for c in &table.constraints {
        if let TableConstraint::Index { name, columns } = c {
            let index_name = build_index_name(&table.name, columns, name.as_deref());
            for col in columns {
                map.entry(col.as_str())
                    .or_default()
                    .push(index_name.clone());
            }
        }
    }
    map
}

/// Composite unique-index name per column, spelled as the SQL layer spells it.
fn collect_composite_unique_names(table: &TableDef) -> HashMap<&str, String> {
    let mut map = HashMap::new();
    for c in &table.constraints {
        if let TableConstraint::Unique { name, columns, .. } = c
            && columns.len() > 1
        {
            let uq_name = build_unique_constraint_name(&table.name, columns, name.as_deref());
            for col in columns {
                map.insert(col.as_str(), uq_name.clone());
            }
        }
    }
    map
}

// ---------------------------------------------------------------------------
// Reverse relation naming
// ---------------------------------------------------------------------------

/// Go field names for `rels`, in order: `Children` for a self-reference,
/// otherwise the source struct — as is for a has-one, pluralized for a
/// has-many. A name more than one relation would take is told apart by the
/// key it hangs on (`SettingsByCreatedByUserID`).
fn reverse_field_names(target: &str, rels: &[BackRelation]) -> Vec<String> {
    let bases: Vec<String> = rels
        .iter()
        .map(|rel| {
            if rel.source_table == target {
                "Children".to_string()
            } else if rel.is_one_to_one {
                exported_go_name(&rel.source_table)
            } else {
                exported_go_name(&pluralize(&rel.source_table))
            }
        })
        .collect();

    rels.iter()
        .zip(&bases)
        .map(|(rel, base)| {
            if bases.iter().filter(|other| *other == base).count() > 1 {
                let key: String = rel.fk_columns.iter().map(|c| to_go_field_name(c)).collect();
                format!("{base}By{key}")
            } else {
                base.clone()
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
    indexes: &[String],
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
    schema: &[TableDef],
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

    let gorm_tag = relation_tag(
        &[fk_field_name.to_string()],
        &reference_fields(schema, fk.ref_table, &[fk.ref_column]),
        fk.on_delete,
        fk.on_update,
    );

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
    let gorm_tag = relation_tag(
        &fk_fields,
        &reference_fields(schema, fk.ref_table, &fk.ref_cols),
        fk.on_delete,
        fk.on_update,
    );

    lines.push(format!(
        "    {relation_field_name} {ref_struct} `gorm:\"{gorm_tag}\" json:\"-\"`"
    ));
}

// ---------------------------------------------------------------------------
// GORM tag building
// ---------------------------------------------------------------------------

/// The `references` fields of a relation on `ref_table`: none when the key is
/// the target's primary key, which GORM assumes. A composite key is always
/// spelled out, since GORM pairs its fields by position.
fn reference_fields(schema: &[TableDef], ref_table: &str, ref_columns: &[&str]) -> Vec<String> {
    if let [column] = ref_columns {
        let is_primary_key = schema
            .iter()
            .find(|t| t.name.as_str() == ref_table)
            .is_none_or(|target| {
                let pk = primary_key_columns(&target.constraints);
                pk.len() == 1 && pk.contains(column)
            });
        if is_primary_key {
            return Vec::new();
        }
    }
    ref_columns
        .iter()
        .map(|c| field_name_in(schema, ref_table, c))
        .collect()
}

/// The `gorm:"..."` tag of a relation field: the fields on the foreign-key
/// side, the fields they reference when GORM could not infer them, and the
/// referential actions.
fn relation_tag(
    foreign_key: &[String],
    references: &[String],
    on_delete: Option<&ReferenceAction>,
    on_update: Option<&ReferenceAction>,
) -> String {
    let mut parts = vec![format!("foreignKey:{}", foreign_key.join(","))];
    if !references.is_empty() {
        parts.push(format!("references:{}", references.join(",")));
    }
    let actions: Vec<String> = [("OnDelete", on_delete), ("OnUpdate", on_update)]
        .into_iter()
        .filter_map(|(key, action)| Some(format!("{key}:{}", action?.to_sql_keyword())))
        .collect();
    if !actions.is_empty() {
        parts.push(format!("constraint:{}", actions.join(",")));
    }
    parts.join(";")
}

fn build_gorm_tag(
    col: &ColumnDef,
    is_pk: bool,
    auto_increment: bool,
    is_unique: bool,
    indexes: &[String],
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
        ColumnType::Simple(SimpleColumnType::Inet) => parts.push("type:inet".into()),
        ColumnType::Simple(SimpleColumnType::Cidr) => parts.push("type:cidr".into()),
        ColumnType::Simple(SimpleColumnType::Macaddr) => parts.push("type:macaddr".into()),
        ColumnType::Complex(ComplexColumnType::Varchar { length }) => {
            parts.push(format!("size:{length}"));
        }
        // GORM only applies `size` to its built-in string type; a bare `type:char`
        // is `char(1)` on every database.
        ColumnType::Complex(ComplexColumnType::Char { length }) => {
            parts.push(format!("type:char({length})"));
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
        && let Some(tag) = build_default_tag(default, &col.r#type)
    {
        parts.push(tag);
    }

    for name in indexes {
        parts.push(format!("index:{name}"));
    }

    if let Some(uq_name) = composite_unique_name {
        parts.push(format!("uniqueIndex:{uq_name}"));
    }

    parts.join(";")
}

fn build_default_tag(default: &DefaultValue, col_type: &ColumnType) -> Option<String> {
    let sql = default.to_sql();
    // A function call has no literal to pin, and `"` or `;` would end the
    // struct tag or the gorm setting early, taking every later tag with it.
    if sql.contains(['(', '"', ';']) {
        return None;
    }
    // An integer enum's default may name a variant; the column stores its value.
    if let ColumnType::Complex(ComplexColumnType::Enum {
        values: EnumValues::Integer(variants),
        ..
    }) = col_type
        && let Some(value) = integer_enum_variant_value(variants, unquote(&sql))
    {
        return Some(format!("default:{value}"));
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
