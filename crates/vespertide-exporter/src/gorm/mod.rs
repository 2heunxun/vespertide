use std::collections::{HashMap, HashSet};

use crate::orm::OrmExporter;
use vespertide_core::schema::column::{
    ColumnType, ComplexColumnType, EnumValues, SimpleColumnType,
};
use vespertide_core::schema::constraint::TableConstraint;
use vespertide_core::{ColumnDef, DefaultValue, ReferenceAction, TableDef};

/// Track which Go imports are actually used to generate minimal import statements.
#[derive(Default)]
struct UsedImports {
    needs_time: bool,
    needs_uuid: bool,
    needs_datatypes: bool,
    needs_decimal: bool,
}

impl UsedImports {
    fn add_column_type(&mut self, col_type: &ColumnType) {
        match col_type {
            ColumnType::Simple(ty) => match ty {
                SimpleColumnType::Date
                | SimpleColumnType::Time
                | SimpleColumnType::Timestamp
                | SimpleColumnType::Timestamptz => {
                    self.needs_time = true;
                }
                SimpleColumnType::Uuid => {
                    self.needs_uuid = true;
                }
                SimpleColumnType::Json => {
                    self.needs_datatypes = true;
                }
                _ => {}
            },
            ColumnType::Complex(ty) => {
                if let ComplexColumnType::Numeric { .. } = ty {
                    self.needs_decimal = true;
                }
                if let ComplexColumnType::Custom { custom_type } = ty {
                    if custom_type.to_uppercase() == "JSONB" {
                        self.needs_datatypes = true;
                    }
                }
            }
        }
    }
}

pub struct GormExporter;

impl OrmExporter for GormExporter {
    fn render_entity(&self, table: &TableDef) -> Result<String, String> {
        render_entity(table)
    }

    fn render_entity_with_schema(
        &self,
        table: &TableDef,
        schema: &[TableDef],
    ) -> Result<String, String> {
        render_entity_with_schema(table, schema)
    }
}

/// Render a GORM entity for the given table definition.
pub fn render_entity(table: &TableDef) -> Result<String, String> {
    render_entity_inner(table, &[])
}

/// Render a GORM entity with full schema context for reverse-relation (HasMany) generation.
pub fn render_entity_with_schema(table: &TableDef, schema: &[TableDef]) -> Result<String, String> {
    render_entity_inner(table, schema)
}

fn render_entity_inner(table: &TableDef, schema: &[TableDef]) -> Result<String, String> {
    let mut lines: Vec<String> = Vec::new();

    let struct_name = to_pascal_case(&table.name);

    // Find enum names that appear in multiple schema tables (need qualified Go type names)
    let conflicting_enums: HashSet<String> = {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for col in &table.columns {
            if let ColumnType::Complex(ComplexColumnType::Enum { name, .. }) = &col.r#type {
                counts.entry(to_pascal_case(name)).or_insert(1);
            }
        }
        for other in schema {
            if other.name == table.name {
                continue;
            }
            let mut seen = HashSet::new();
            for col in &other.columns {
                if let ColumnType::Complex(ComplexColumnType::Enum { name, .. }) = &col.r#type {
                    let pascal = to_pascal_case(name);
                    if seen.insert(pascal.clone()) {
                        *counts.entry(pascal).or_default() += 1;
                    }
                }
            }
        }
        counts.into_iter().filter(|(_, c)| *c > 1).map(|(n, _)| n).collect()
    };

    // Collect enums defined in this table's columns, with qualified names where needed
    let enums: Vec<(&str, &EnumValues, String)> = table
        .columns
        .iter()
        .filter_map(|col| {
            if let ColumnType::Complex(ComplexColumnType::Enum { name, values }) = &col.r#type {
                let pascal = to_pascal_case(name);
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
        .collect();

    let auto_increment = table.constraints.iter().any(|c| {
        matches!(c, TableConstraint::PrimaryKey { auto_increment: true, .. })
    });

    let is_composite_pk = pk_columns.len() > 1;

    let single_unique_columns: HashSet<String> = table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::Unique { columns, .. } = c {
                if columns.len() == 1 { Some(columns[0].clone()) } else { None }
            } else {
                None
            }
        })
        .collect();

    let index_map = collect_index_info(&table.constraints);
    let composite_unique_map = collect_composite_unique_info(&table.constraints);

    let mut used_imports = UsedImports::default();
    for col in &table.columns {
        used_imports.add_column_type(&col.r#type);
    }

    let reverse_relations = find_reverse_relations(&table.name, schema);

    // --- Package declaration ---
    lines.push("package models".into());
    lines.push(String::new());

    // --- Imports ---
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

    // --- Enum type declarations ---
    for (_, values, qualified_name) in &enums {
        render_enum(&mut lines, qualified_name, values);
        lines.push(String::new());
    }

    // --- Struct definition ---
    if let Some(ref desc) = table.description {
        lines.push(format!("// {}", desc.replace('\n', " ")));
    }

    lines.push(format!("type {struct_name} struct {{"));

    for col in &table.columns {
        let is_pk = pk_columns.contains(&col.name);
        let is_unique = single_unique_columns.contains(&col.name);
        let indexes = index_map.get(&col.name).map(|v| v.as_slice()).unwrap_or(&[]);
        let composite_unique_name = composite_unique_map.get(&col.name);

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

        if let Some(fk) = fk_by_column.get(&col.name) {
            render_fk_relation_field(&mut lines, col, fk);
        }
    }

    // Reverse relation fields (HasMany) derived from schema context
    for rel in &reverse_relations {
        let mut constraint_parts: Vec<String> = Vec::new();
        if let Some(ref action) = rel.on_delete {
            constraint_parts.push(format!("OnDelete:{}", reference_action_str(action)));
        }
        if let Some(ref action) = rel.on_update {
            constraint_parts.push(format!("OnUpdate:{}", reference_action_str(action)));
        }
        let fk_field = to_go_field_name(&rel.fk_column);
        let gorm_tag = if constraint_parts.is_empty() {
            format!("foreignKey:{fk_field}")
        } else {
            format!("foreignKey:{fk_field};constraint:{}", constraint_parts.join(","))
        };
        lines.push(format!(
            "    {field_name} []{ref_struct} `gorm:\"{gorm_tag}\" json:\"-\"`",
            field_name = rel.field_name,
            ref_struct = to_pascal_case(&rel.ref_table),
        ));
    }

    lines.push("}".into());
    lines.push(String::new());

    // --- TableName() method ---
    if needs_table_name_method(&table.name, &struct_name) {
        lines.push(format!(
            "func ({struct_name}) TableName() string {{ return \"{name}\" }}",
            name = table.name,
        ));
        lines.push(String::new());
    }

    Ok(lines.join("\n"))
}

// ---------------------------------------------------------------------------
// FK info collection
// ---------------------------------------------------------------------------

struct FkInfo {
    ref_table: String,
    on_delete: Option<ReferenceAction>,
    on_update: Option<ReferenceAction>,
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
                        columns[0].clone(),
                        FkInfo {
                            ref_table: ref_table.clone(),
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
                map.entry(col.clone())
                    .or_default()
                    .push(IndexInfo { name: name.clone() });
            }
        }
    }
    map
}

fn collect_composite_unique_info(constraints: &[TableConstraint]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for c in constraints {
        if let TableConstraint::Unique { name, columns } = c {
            if columns.len() > 1 {
                let uq_name = name
                    .clone()
                    .unwrap_or_else(|| format!("uq_{}", columns.join("_")));
                for col in columns {
                    map.insert(col.clone(), uq_name.clone());
                }
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
    let mut raw: Vec<(String, String, String, Option<ReferenceAction>, Option<ReferenceAction>)> =
        Vec::new();
    for other in schema {
        if other.name == table_name {
            continue;
        }
        for c in &other.constraints {
            if let TableConstraint::ForeignKey {
                columns,
                ref_table,
                on_delete,
                on_update,
                ..
            } = c
            {
                if ref_table.as_str() == table_name && columns.len() == 1 {
                    let fk_col = columns[0].clone();
                    let pascal = to_pascal_case(&other.name);
                    let base_name =
                        if pascal.ends_with('s') { pascal } else { format!("{pascal}s") };
                    raw.push((
                        other.name.clone(),
                        fk_col,
                        base_name,
                        on_delete.clone(),
                        on_update.clone(),
                    ));
                }
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
            ReverseRelation { field_name, ref_table, fk_column: fk_col, on_delete, on_update }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Enum rendering
// ---------------------------------------------------------------------------

fn render_enum(lines: &mut Vec<String>, name: &str, values: &EnumValues) {
    let type_name = to_pascal_case(name);

    match values {
        EnumValues::String(vals) => {
            lines.push(format!("type {type_name} string"));
            lines.push(String::new());
            lines.push("const (".into());
            for val in vals {
                let const_name = format!("{type_name}{}", to_pascal_case(val));
                lines.push(format!("    {const_name} {type_name} = \"{val}\""));
            }
            lines.push(")".into());
        }
        EnumValues::Integer(vals) => {
            lines.push(format!("type {type_name} int"));
            lines.push(String::new());
            lines.push("const (".into());
            for val in vals {
                let const_name = format!("{type_name}{}", to_pascal_case(&val.name));
                lines.push(format!("    {const_name} {type_name} = {}", val.value));
            }
            lines.push(")".into());
        }
    }
}

// ---------------------------------------------------------------------------
// Field rendering
// ---------------------------------------------------------------------------

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
    let gorm_tag = build_gorm_tag(col, is_pk, auto_increment, is_unique, indexes, composite_unique_name);

    lines.push(format!(
        "    {field_name} {go_type} `gorm:\"{gorm_tag}\" json:\"{json_name}\"`",
        json_name = col.name,
    ));
}

fn render_fk_relation_field(lines: &mut Vec<String>, col: &ColumnDef, fk: &FkInfo) {
    let ref_struct = to_pascal_case(&fk.ref_table);
    let fk_field_name = to_go_field_name(&col.name);
    let mut relation_field_name = infer_relation_field_name(&col.name);
    if relation_field_name == fk_field_name {
        relation_field_name = format!("{relation_field_name}{ref_struct}");
    }

    let mut constraint_parts: Vec<String> = Vec::new();
    if let Some(ref action) = fk.on_delete {
        constraint_parts.push(format!("OnDelete:{}", reference_action_str(action)));
    }
    if let Some(ref action) = fk.on_update {
        constraint_parts.push(format!("OnUpdate:{}", reference_action_str(action)));
    }

    let gorm_tag = if constraint_parts.is_empty() {
        format!("foreignKey:{fk_field_name}")
    } else {
        format!("foreignKey:{fk_field_name};constraint:{}", constraint_parts.join(","))
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

    if let Some(ref default) = col.default {
        if let Some(tag) = build_default_tag(default) {
            parts.push(tag);
        }
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
// Type mapping
// ---------------------------------------------------------------------------

fn go_type_for_column(col_type: &ColumnType, nullable: bool) -> String {
    go_type_for_column_mapped(col_type, nullable, &HashMap::new())
}

fn go_type_for_column_mapped(col_type: &ColumnType, nullable: bool, enum_map: &HashMap<&str, String>) -> String {
    let base = match col_type {
        ColumnType::Complex(ComplexColumnType::Enum { name, .. }) => {
            enum_map.get(name.as_str()).cloned().unwrap_or_else(|| to_pascal_case(name))
        }
        _ => go_base_type(col_type),
    };
    if nullable { format!("*{base}") } else { base }
}

fn go_base_type(col_type: &ColumnType) -> String {
    match col_type {
        ColumnType::Simple(ty) => match ty {
            SimpleColumnType::SmallInt => "int16".to_string(),
            SimpleColumnType::Integer => "int32".to_string(),
            SimpleColumnType::BigInt => "int64".to_string(),
            SimpleColumnType::Real => "float32".to_string(),
            SimpleColumnType::DoublePrecision => "float64".to_string(),
            SimpleColumnType::Text
            | SimpleColumnType::Xml
            | SimpleColumnType::Inet
            | SimpleColumnType::Cidr
            | SimpleColumnType::Macaddr
            | SimpleColumnType::Interval => "string".to_string(),
            SimpleColumnType::Boolean => "bool".to_string(),
            SimpleColumnType::Date
            | SimpleColumnType::Time
            | SimpleColumnType::Timestamp
            | SimpleColumnType::Timestamptz => "time.Time".to_string(),
            SimpleColumnType::Bytea => "[]byte".to_string(),
            SimpleColumnType::Uuid => "uuid.UUID".to_string(),
            SimpleColumnType::Json => "datatypes.JSON".to_string(),
        },
        ColumnType::Complex(ty) => match ty {
            ComplexColumnType::Varchar { .. } | ComplexColumnType::Char { .. } => {
                "string".to_string()
            }
            ComplexColumnType::Custom { custom_type } => {
                if custom_type.to_uppercase() == "JSONB" {
                    "datatypes.JSON".to_string()
                } else {
                    "string".to_string()
                }
            }
            ComplexColumnType::Numeric { .. } => "decimal.Decimal".to_string(),
            ComplexColumnType::Enum { name, .. } => to_pascal_case(name),
        },
    }
}

fn reference_action_str(action: &ReferenceAction) -> &'static str {
    match action {
        ReferenceAction::Cascade => "CASCADE",
        ReferenceAction::Restrict => "RESTRICT",
        ReferenceAction::SetNull => "SET NULL",
        ReferenceAction::SetDefault => "SET DEFAULT",
        ReferenceAction::NoAction => "NO ACTION",
    }
}

// ---------------------------------------------------------------------------
// Naming utilities
// ---------------------------------------------------------------------------

fn to_pascal_case(s: &str) -> String {
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

fn to_go_field_name(s: &str) -> String {
    let pascal = to_pascal_case(s);
    // Apply Go conventions for common abbreviations
    pascal.replace("Id", "ID")
}

fn infer_relation_field_name(fk_column: &str) -> String {
    let base = fk_column.strip_suffix("_id").unwrap_or(fk_column);
    to_pascal_case(base)
}

fn pascal_to_snake(s: &str) -> String {
    let mut result = String::new();
    for c in s.chars() {
        if c.is_uppercase() && !result.is_empty() {
            result.push('_');
        }
        result.extend(c.to_lowercase());
    }
    result
}

fn needs_table_name_method(table_name: &str, struct_name: &str) -> bool {
    let snake = pascal_to_snake(struct_name);
    let gorm_default = format!("{snake}s");
    gorm_default != table_name
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use rstest::rstest;
    use vespertide_core::{DefaultValue, NumValue};

    fn col(name: &str, ty: ColumnType) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            r#type: ty,
            nullable: false,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }
    }

    // -----------------------------------------------------------------------
    // Type mapping unit tests
    // -----------------------------------------------------------------------

    #[rstest]
    #[case(ColumnType::Simple(SimpleColumnType::SmallInt), false, "int16")]
    #[case(ColumnType::Simple(SimpleColumnType::Integer), false, "int32")]
    #[case(ColumnType::Simple(SimpleColumnType::BigInt), false, "int64")]
    #[case(ColumnType::Simple(SimpleColumnType::Real), false, "float32")]
    #[case(ColumnType::Simple(SimpleColumnType::DoublePrecision), false, "float64")]
    #[case(ColumnType::Simple(SimpleColumnType::Text), false, "string")]
    #[case(ColumnType::Simple(SimpleColumnType::Boolean), false, "bool")]
    #[case(ColumnType::Simple(SimpleColumnType::Timestamp), false, "time.Time")]
    #[case(ColumnType::Simple(SimpleColumnType::Timestamptz), false, "time.Time")]
    #[case(ColumnType::Simple(SimpleColumnType::Date), false, "time.Time")]
    #[case(ColumnType::Simple(SimpleColumnType::Time), false, "time.Time")]
    #[case(ColumnType::Simple(SimpleColumnType::Uuid), false, "uuid.UUID")]
    #[case(ColumnType::Simple(SimpleColumnType::Json), false, "datatypes.JSON")]
    #[case(ColumnType::Simple(SimpleColumnType::Bytea), false, "[]byte")]
    #[case(ColumnType::Simple(SimpleColumnType::Inet), false, "string")]
    #[case(ColumnType::Complex(ComplexColumnType::Varchar { length: 255 }), false, "string")]
    #[case(ColumnType::Complex(ComplexColumnType::Numeric { precision: 10, scale: 2 }), false, "decimal.Decimal")]
    #[case(ColumnType::Complex(ComplexColumnType::Custom { custom_type: "JSONB".into() }), false, "datatypes.JSON")]
    #[case(ColumnType::Complex(ComplexColumnType::Custom { custom_type: "jsonb".into() }), false, "datatypes.JSON")]
    #[case(ColumnType::Complex(ComplexColumnType::Custom { custom_type: "TEXT".into() }), false, "string")]
    #[case(ColumnType::Simple(SimpleColumnType::Integer), true, "*int32")]
    #[case(ColumnType::Simple(SimpleColumnType::Text), true, "*string")]
    #[case(ColumnType::Simple(SimpleColumnType::Timestamp), true, "*time.Time")]
    fn test_go_type_mapping(
        #[case] col_type: ColumnType,
        #[case] nullable: bool,
        #[case] expected: &str,
    ) {
        assert_eq!(go_type_for_column(&col_type, nullable), expected);
    }

    #[rstest]
    #[case("user_id", "UserID")]
    #[case("id", "ID")]
    #[case("created_at", "CreatedAt")]
    #[case("profile_image", "ProfileImage")]
    #[case("media_id", "MediaID")]
    fn test_to_go_field_name(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(to_go_field_name(input), expected);
    }

    #[rstest]
    #[case("user_id", "User")]
    #[case("author_id", "Author")]
    #[case("parent_id", "Parent")]
    #[case("node", "Node")]
    fn test_infer_relation_field_name(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(infer_relation_field_name(input), expected);
    }

    #[rstest]
    #[case("User", "user", true)]
    #[case("User", "users", false)]
    #[case("OrderItem", "order_items", false)]
    #[case("OrderItem", "order_item", true)]
    fn test_needs_table_name_method(
        #[case] struct_name: &str,
        #[case] table_name: &str,
        #[case] expected: bool,
    ) {
        assert_eq!(needs_table_name_method(table_name, struct_name), expected);
    }

    // -----------------------------------------------------------------------
    // Snapshot tests (a) simple table - columns only
    // -----------------------------------------------------------------------

    #[test]
    fn test_basic_table() {
        let table = TableDef {
            name: "users".into(),
            description: Some("User accounts".into()),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: Some("Primary key".into()),
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Complex(ComplexColumnType::Varchar { length: 255 }),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "name".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "active".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Boolean),
                    nullable: false,
                    default: Some(DefaultValue::Bool(true)),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![
                TableConstraint::PrimaryKey {
                    auto_increment: true,
                    columns: vec!["id".into()],
                },
                TableConstraint::Unique {
                    name: None,
                    columns: vec!["email".into()],
                },
            ],
        };
        let result = render_entity(&table).unwrap();
        assert_snapshot!(result);
    }

    // -----------------------------------------------------------------------
    // Snapshot tests (b) FK
    // -----------------------------------------------------------------------

    #[test]
    fn test_table_with_foreign_key() {
        let table = TableDef {
            name: "posts".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                ColumnDef {
                    name: "author_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                col("title", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            constraints: vec![
                TableConstraint::PrimaryKey {
                    auto_increment: true,
                    columns: vec!["id".into()],
                },
                TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["author_id".into()],
                    ref_table: "users".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: Some(ReferenceAction::Cascade),
                    on_update: None,
                },
                TableConstraint::Index {
                    name: Some("ix_posts__author_id".into()),
                    columns: vec!["author_id".into()],
                },
            ],
        };
        let result = render_entity(&table).unwrap();
        assert_snapshot!(result);
    }

    // -----------------------------------------------------------------------
    // Snapshot tests (c) enums
    // -----------------------------------------------------------------------

    #[test]
    fn test_table_with_string_enum() {
        let table = TableDef {
            name: "orders".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                ColumnDef {
                    name: "status".into(),
                    r#type: ColumnType::Complex(ComplexColumnType::Enum {
                        name: "order_status".into(),
                        values: EnumValues::String(vec![
                            "pending".into(),
                            "shipped".into(),
                            "delivered".into(),
                        ]),
                    }),
                    nullable: false,
                    default: Some(DefaultValue::String("'pending'".into())),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
            }],
        };
        let result = render_entity(&table).unwrap();
        assert_snapshot!(result);
    }

    #[test]
    fn test_table_with_integer_enum() {
        let table = TableDef {
            name: "tasks".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                ColumnDef {
                    name: "priority".into(),
                    r#type: ColumnType::Complex(ComplexColumnType::Enum {
                        name: "priority_level".into(),
                        values: EnumValues::Integer(vec![
                            NumValue { name: "low".into(), value: 0 },
                            NumValue { name: "medium".into(), value: 10 },
                            NumValue { name: "high".into(), value: 20 },
                        ]),
                    }),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            }],
        };
        let result = render_entity(&table).unwrap();
        assert_snapshot!(result);
    }

    // -----------------------------------------------------------------------
    // Snapshot tests (d) composite PK + nullable
    // -----------------------------------------------------------------------

    #[test]
    fn test_composite_pk_nullable() {
        let table = TableDef {
            name: "order_items".into(),
            description: None,
            columns: vec![
                col("order_id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("product_id", ColumnType::Simple(SimpleColumnType::Integer)),
                ColumnDef {
                    name: "quantity".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "note".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![
                TableConstraint::PrimaryKey {
                    auto_increment: false,
                    columns: vec!["order_id".into(), "product_id".into()],
                },
                TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["order_id".into()],
                    ref_table: "orders".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                },
                TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["product_id".into()],
                    ref_table: "products".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                },
                TableConstraint::Unique {
                    name: Some("uq_order_items__order_product".into()),
                    columns: vec!["order_id".into(), "product_id".into()],
                },
            ],
        };
        let result = render_entity(&table).unwrap();
        assert_snapshot!(result);
    }

    // -----------------------------------------------------------------------
    // All simple types
    // -----------------------------------------------------------------------

    #[test]
    fn test_all_simple_types() {
        let table = TableDef {
            name: "type_test".into(),
            description: None,
            columns: vec![
                col("col_smallint", ColumnType::Simple(SimpleColumnType::SmallInt)),
                col("col_integer", ColumnType::Simple(SimpleColumnType::Integer)),
                col("col_bigint", ColumnType::Simple(SimpleColumnType::BigInt)),
                col("col_real", ColumnType::Simple(SimpleColumnType::Real)),
                col("col_double", ColumnType::Simple(SimpleColumnType::DoublePrecision)),
                col("col_text", ColumnType::Simple(SimpleColumnType::Text)),
                col("col_boolean", ColumnType::Simple(SimpleColumnType::Boolean)),
                col("col_date", ColumnType::Simple(SimpleColumnType::Date)),
                col("col_time", ColumnType::Simple(SimpleColumnType::Time)),
                col("col_timestamp", ColumnType::Simple(SimpleColumnType::Timestamp)),
                col("col_timestamptz", ColumnType::Simple(SimpleColumnType::Timestamptz)),
                col("col_interval", ColumnType::Simple(SimpleColumnType::Interval)),
                col("col_bytea", ColumnType::Simple(SimpleColumnType::Bytea)),
                col("col_uuid", ColumnType::Simple(SimpleColumnType::Uuid)),
                col("col_json", ColumnType::Simple(SimpleColumnType::Json)),
                col("col_inet", ColumnType::Simple(SimpleColumnType::Inet)),
                col("col_cidr", ColumnType::Simple(SimpleColumnType::Cidr)),
                col("col_macaddr", ColumnType::Simple(SimpleColumnType::Macaddr)),
                col("col_xml", ColumnType::Simple(SimpleColumnType::Xml)),
            ],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["col_integer".into()],
            }],
        };
        let result = render_entity(&table).unwrap();
        assert_snapshot!(result);
    }

    // -----------------------------------------------------------------------
    // Snapshot tests (e) JSONB custom type
    // -----------------------------------------------------------------------

    #[test]
    fn test_table_with_jsonb_column() {
        let table = TableDef {
            name: "documents".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("data", ColumnType::Complex(ComplexColumnType::Custom { custom_type: "JSONB".into() })),
                col("meta", ColumnType::Simple(SimpleColumnType::Json)),
            ],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
            }],
        };
        let result = render_entity(&table).unwrap();
        assert_snapshot!(result);
    }

    // -----------------------------------------------------------------------
    // Snapshot tests (f) server-side default skipped
    // -----------------------------------------------------------------------

    #[test]
    fn test_server_default_skipped() {
        let table = TableDef {
            name: "events".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                ColumnDef {
                    name: "created_at".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Timestamptz),
                    nullable: false,
                    default: Some(DefaultValue::String("NOW()".into())),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "count".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: Some(DefaultValue::Integer(0)),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
            }],
        };
        let result = render_entity(&table).unwrap();
        // Server-side function calls must not appear as GORM default tags
        assert!(!result.contains("default:NOW()"));
        // Literal integer defaults are still included
        assert!(result.contains("default:0"));
        assert_snapshot!(result);
    }

    // -----------------------------------------------------------------------
    // Snapshot tests (g) HasMany with on_delete/on_update constraint
    // -----------------------------------------------------------------------

    #[test]
    fn test_has_many_with_constraint() {
        let users = TableDef {
            name: "users".into(),
            description: None,
            columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
            }],
        };
        let posts = TableDef {
            name: "posts".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("user_id", ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            constraints: vec![
                TableConstraint::PrimaryKey {
                    auto_increment: true,
                    columns: vec!["id".into()],
                },
                TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["user_id".into()],
                    ref_table: "users".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: Some(ReferenceAction::Cascade),
                    on_update: Some(ReferenceAction::Restrict),
                },
            ],
        };
        let schema = vec![users.clone(), posts.clone()];
        let result = render_entity_with_schema(&users, &schema).unwrap();
        assert!(result.contains("OnDelete:CASCADE"));
        assert!(result.contains("OnUpdate:RESTRICT"));
        assert_snapshot!(result);
    }
}
