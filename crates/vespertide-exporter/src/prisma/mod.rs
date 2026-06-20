use std::collections::{HashMap, HashSet};

use crate::orm::OrmExporter;
use vespertide_config::PrismaConfig;
use vespertide_core::schema::column::{ColumnType, ComplexColumnType, EnumValues, SimpleColumnType};
use vespertide_core::schema::constraint::TableConstraint;
use vespertide_core::schema::names::ColumnName;
use vespertide_core::schema::reference::ReferenceAction;
use vespertide_core::TableDef;

pub struct PrismaExporter;

impl OrmExporter for PrismaExporter {
    fn render_entity(&self, table: &TableDef) -> Result<String, String> {
        Ok(render_entity(table))
    }

    fn render_entity_with_schema(
        &self,
        table: &TableDef,
        schema: &[TableDef],
    ) -> Result<String, String> {
        Ok(render_entity_with_schema(table, schema))
    }
}

/// Prisma exporter with configuration support.
///
/// Assembles a complete `schema.prisma` file from a full table list.
pub struct PrismaExporterWithConfig<'a> {
    pub config: &'a PrismaConfig,
}

impl<'a> PrismaExporterWithConfig<'a> {
    pub fn new(config: &'a PrismaConfig) -> Self {
        Self { config }
    }

    /// Render a complete `schema.prisma` file for all tables.
    ///
    /// Output order: datasource → generator → (globally deduped) enum blocks → model blocks.
    pub fn render_schema(&self, tables: &[TableDef]) -> String {
        let mut seen_enums: HashSet<String> = HashSet::new();
        let mut enum_blocks: Vec<String> = Vec::new();
        for table in tables {
            for (name, values) in collect_table_enums(table) {
                if seen_enums.insert(name.to_string()) {
                    enum_blocks.push(render_enum(name, values));
                }
            }
        }

        let mut parts: Vec<String> = Vec::new();

        let mut datasource = vec![
            "datasource db {".to_string(),
            format!("  provider = \"{}\"", self.config.provider()),
            "  url      = env(\"DATABASE_URL\")".to_string(),
        ];
        if let Some(rm) = self.config.relation_mode() {
            datasource.push(format!("  relationMode = \"{}\"", rm));
        }
        datasource.push("}".to_string());
        parts.push(datasource.join("\n"));

        let mut generator = vec![
            "generator client {".to_string(),
            "  provider = \"prisma-client-js\"".to_string(),
        ];
        if let Some(output) = self.config.client_output() {
            generator.push(format!("  output   = \"{}\"", output));
        }
        generator.push("}".to_string());
        parts.push(generator.join("\n"));

        parts.extend(enum_blocks);

        for table in tables {
            parts.push(render_model(table, tables));
        }

        parts.join("\n\n") + "\n"
    }
}

fn collect_table_enums<'a>(table: &'a TableDef) -> Vec<(&'a str, &'a EnumValues)> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for col in &table.columns {
        if let ColumnType::Complex(ComplexColumnType::Enum { name, values }) = &col.r#type {
            if seen.insert(name.as_str()) {
                result.push((name.as_str(), values));
            }
        }
    }
    result
}

/// Render enum blocks + model block without schema context (no back-relations).
pub fn render_entity(table: &TableDef) -> String {
    render_entity_with_schema(table, &[])
}

/// Render enum blocks + model block with full schema context (includes back-relations).
pub fn render_entity_with_schema(table: &TableDef, schema: &[TableDef]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (name, values) in collect_table_enums(table) {
        parts.push(render_enum(name, values));
    }
    parts.push(render_model(table, schema));
    parts.join("\n\n")
}

/// Multi-table entry point: render every table (enum + model blocks) with full
/// schema context and join them. Mirrors the other ORMs' `export` so the
/// cross-ORM test harness can dispatch Prisma through a single call. The
/// `datasource`/`generator` wrapper lives in [`PrismaExporterWithConfig`].
pub fn export(schema: &[TableDef]) -> Result<String, String> {
    Ok(schema
        .iter()
        .map(|table| render_entity_with_schema(table, schema))
        .collect::<Vec<_>>()
        .join("\n\n"))
}

/// Test-only accessor for the internal `to_pascal_case` helper, mirroring the
/// other ORM backends so the cross-ORM consolidation test can exercise it
/// without making the helper generally public.
#[cfg(test)]
pub fn to_pascal_case_for_tests(s: &str) -> String {
    to_pascal_case(s)
}

fn render_enum(name: &str, values: &EnumValues) -> String {
    let enum_name = to_pascal_case(name);
    let mut lines = Vec::new();
    lines.push(format!("enum {} {{", enum_name));
    match values {
        EnumValues::String(vals) => {
            for val in vals {
                let variant = to_screaming_snake(val);
                if variant == *val {
                    lines.push(format!("  {}", variant));
                } else {
                    lines.push(format!("  {} @map(\"{}\")", variant, val));
                }
            }
        }
        EnumValues::Integer(vals) => {
            // Prisma doesn't support integer enums natively; emit as string variants with comment
            for val in vals {
                let variant = to_screaming_snake(&val.name);
                lines.push(format!("  {} // = {}", variant, val.value));
            }
        }
    }
    lines.push("}".into());
    lines.join("\n")
}

struct PkInfo {
    columns: Vec<String>,
    auto_increment: bool,
}

fn extract_pk_info(constraints: &[TableConstraint]) -> PkInfo {
    for c in constraints {
        if let TableConstraint::PrimaryKey { auto_increment, columns, .. } = c {
            return PkInfo {
                columns: columns.iter().map(|c| c.to_string()).collect(),
                auto_increment: *auto_increment,
            };
        }
    }
    PkInfo { columns: Vec::new(), auto_increment: false }
}

struct FkInfo<'a> {
    ref_table: &'a str,
    ref_cols: &'a [ColumnName],
    on_delete: Option<&'a ReferenceAction>,
    on_update: Option<&'a ReferenceAction>,
}

fn render_model(table: &TableDef, schema: &[TableDef]) -> String {
    let mut lines: Vec<String> = Vec::new();

    if let Some(desc) = &table.description {
        for line in desc.lines() {
            lines.push(format!("/// {}", line));
        }
    }

    let model_name = to_pascal_case(&table.name);
    lines.push(format!("model {} {{", model_name));

    let pk_info = extract_pk_info(&table.constraints);
    let pk_columns: HashSet<&str> = pk_info.columns.iter().map(|s| s.as_str()).collect();
    let is_composite_pk = pk_info.columns.len() > 1;

    let unique_single: HashMap<&str, Option<&str>> = table.constraints.iter()
        .filter_map(|c| {
            if let TableConstraint::Unique { name, columns, .. } = c {
                if columns.len() == 1 { Some((columns[0].as_str(), name.as_deref())) } else { None }
            } else { None }
        })
        .collect();

    // FK lookup by column
    let fk_by_col: HashMap<&str, FkInfo<'_>> = table.constraints.iter()
        .filter_map(|c| {
            if let TableConstraint::ForeignKey { columns, ref_table, ref_columns, on_delete, on_update, .. } = c {
                if columns.len() == 1 {
                    Some((
                        columns[0].as_str(),
                        FkInfo {
                            ref_table: ref_table.as_str(),
                            ref_cols: ref_columns.as_slice(),
                            on_delete: on_delete.as_ref(),
                            on_update: on_update.as_ref(),
                        },
                    ))
                } else { None }
            } else { None }
        })
        .collect();

    // Count FKs per ref_table for disambiguation detection
    let mut ref_table_fk_count: HashMap<&str, usize> = HashMap::new();
    for fk in fk_by_col.values() {
        *ref_table_fk_count.entry(fk.ref_table).or_default() += 1;
    }

    // Render scalar fields + inline relation fields
    for col in &table.columns {
        let col_name = col.name.as_str();
        let in_pk = pk_columns.contains(col_name);
        let is_single_pk = in_pk && !is_composite_pk;
        let auto_inc = is_single_pk && pk_info.auto_increment;
        let is_unique = unique_single.get(col_name).copied();

        if let Some(ref comment) = col.comment {
            lines.push(format!("  /// {}", comment.replace('\n', " ")));
        }

        let (type_str, native_attr) = column_type_to_prisma(&col.r#type, col.nullable);
        let mut attrs: Vec<String> = Vec::new();

        if is_single_pk {
            attrs.push("@id".into());
            if auto_inc {
                attrs.push("@default(autoincrement())".into());
            }
        }

        if !auto_inc {
            if let Some(ref default) = col.default {
                attrs.push(prisma_default_attr(default.to_sql(), &col.r#type));
            }
        }

        if let Some(unique_name) = is_unique {
            if !is_single_pk {
                match unique_name {
                    Some(n) => attrs.push(format!("@unique(map: \"{}\")", n)),
                    None => attrs.push("@unique".into()),
                }
            }
        }

        if let Some(ref native) = native_attr {
            attrs.push(native.clone());
        }

        let attrs_str = if attrs.is_empty() {
            String::new()
        } else {
            format!(" {}", attrs.join(" "))
        };

        lines.push(format!("  {} {}{}", col_name, type_str, attrs_str));

        // Emit inline relation field for FK columns
        if let Some(fk) = fk_by_col.get(col_name) {
            let rel_field_name = infer_relation_field_name(col_name);
            let rel_model = to_pascal_case(fk.ref_table);
            let rel_type = if col.nullable {
                format!("{}?", rel_model)
            } else {
                rel_model.clone()
            };

            let multi_fk = ref_table_fk_count.get(fk.ref_table).copied().unwrap_or(0) > 1;
            let is_self_ref = fk.ref_table == table.name.as_str();
            let needs_name = multi_fk || is_self_ref;

            let mut rel_args: Vec<String> = Vec::new();
            if needs_name {
                let rel_name = format!(
                    "{}{}",
                    to_pascal_case(&table.name),
                    to_pascal_case(&rel_field_name)
                );
                rel_args.push(format!("\"{}\"", rel_name));
            }
            rel_args.push(format!("fields: [{}]", col_name));
            rel_args.push(format!(
                "references: [{}]",
                fk.ref_cols.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
            ));
            if let Some(od) = fk.on_delete {
                rel_args.push(format!("onDelete: {}", reference_action_to_prisma(od)));
            }
            if let Some(ou) = fk.on_update {
                rel_args.push(format!("onUpdate: {}", reference_action_to_prisma(ou)));
            }

            lines.push(format!(
                "  {} {} @relation({})",
                rel_field_name,
                rel_type,
                rel_args.join(", ")
            ));
        }
    }

    // Back-relations from schema context
    if !schema.is_empty() {
        let back_rels = collect_back_relations(&table.name, schema);
        for br in &back_rels {
            let (field_name, rel_type) = back_rel_field(br);
            let rel_attr = match &br.relation_name {
                Some(name) => format!(" @relation(\"{}\")", name),
                None => String::new(),
            };
            lines.push(format!("  {} {}{}", field_name, rel_type, rel_attr));
        }
    }

    // Blank line before model-level attributes
    lines.push(String::new());

    // Composite PK
    if is_composite_pk {
        lines.push(format!("  @@id([{}])", pk_info.columns.join(", ")));
    }

    // Composite unique constraints
    for c in &table.constraints {
        if let TableConstraint::Unique { name, columns, .. } = c {
            if columns.len() > 1 {
                let cols = columns.join(", ");
                if let Some(n) = name {
                    lines.push(format!("  @@unique([{}], name: \"{}\")", cols, n));
                } else {
                    lines.push(format!("  @@unique([{}])", cols));
                }
            }
        }
    }

    // All index constraints
    for c in &table.constraints {
        if let TableConstraint::Index { name, columns } = c {
            let cols = columns.join(", ");
            if let Some(n) = name {
                lines.push(format!("  @@index([{}], name: \"{}\")", cols, n));
            } else {
                lines.push(format!("  @@index([{}])", cols));
            }
        }
    }

    // @@map (always present since model is PascalCase but table is snake_case)
    lines.push(format!("  @@map(\"{}\")", table.name));
    lines.push("}".into());

    lines.join("\n")
}

struct BackRelation {
    source_table: String,
    fk_col: String,
    is_one_to_one: bool,
    relation_name: Option<String>,
}

fn back_rel_field(br: &BackRelation) -> (String, String) {
    let source_pascal = to_pascal_case(&br.source_table);
    let rel_type = if br.is_one_to_one {
        format!("{}?", source_pascal)
    } else {
        format!("{}[]", source_pascal)
    };

    // source_table is already the plural table name — use it directly
    let field_name = if br.relation_name.is_some() {
        let rel_field = infer_relation_field_name(&br.fk_col);
        if br.is_one_to_one {
            format!("{}_{}", rel_field, br.source_table)
        } else {
            format!("{}_{}", rel_field, &br.source_table)
        }
    } else if br.is_one_to_one {
        br.source_table.clone()
    } else {
        br.source_table.clone()
    };

    (field_name, rel_type)
}

fn collect_back_relations(target_table: &str, schema: &[TableDef]) -> Vec<BackRelation> {
    let mut result = Vec::new();

    for source in schema {
        let fks_to_target: Vec<(&str, &[ColumnName])> = source.constraints.iter()
            .filter_map(|c| {
                if let TableConstraint::ForeignKey { columns, ref_table, ref_columns, .. } = c {
                    if ref_table.as_str() == target_table && columns.len() == 1 {
                        Some((columns[0].as_str(), ref_columns.as_slice()))
                    } else { None }
                } else { None }
            })
            .collect();

        if fks_to_target.is_empty() { continue; }

        let multi_fk = fks_to_target.len() > 1;
        let is_self_ref = source.name.as_str() == target_table;

        for (fk_col, _) in &fks_to_target {
            let is_unique = source.constraints.iter().any(|c| {
                matches!(c, TableConstraint::Unique { columns, .. }
                    if columns.len() == 1 && columns[0].as_str() == *fk_col)
            });

            let needs_name = multi_fk || is_self_ref;
            let relation_name = if needs_name {
                let rel_field = infer_relation_field_name(fk_col);
                Some(format!(
                    "{}{}",
                    to_pascal_case(&source.name),
                    to_pascal_case(&rel_field)
                ))
            } else {
                None
            };

            result.push(BackRelation {
                source_table: source.name.as_str().to_string(),
                fk_col: fk_col.to_string(),
                is_one_to_one: is_unique,
                relation_name,
            });
        }
    }

    result
}

fn column_type_to_prisma(ty: &ColumnType, nullable: bool) -> (String, Option<String>) {
    let q = if nullable { "?" } else { "" };

    match ty {
        ColumnType::Simple(simple) => {
            let (base, native) = match simple {
                SimpleColumnType::SmallInt => ("Int", Some("@db.SmallInt")),
                SimpleColumnType::Integer => ("Int", None),
                SimpleColumnType::BigInt => ("BigInt", None),
                SimpleColumnType::Real => ("Float", Some("@db.Real")),
                SimpleColumnType::DoublePrecision => ("Float", None),
                SimpleColumnType::Text => ("String", Some("@db.Text")),
                SimpleColumnType::Boolean => ("Boolean", None),
                SimpleColumnType::Date => ("DateTime", Some("@db.Date")),
                SimpleColumnType::Time => ("DateTime", Some("@db.Time")),
                SimpleColumnType::Timestamp => ("DateTime", Some("@db.Timestamp")),
                SimpleColumnType::Timestamptz => ("DateTime", Some("@db.Timestamptz")),
                SimpleColumnType::Interval => ("String", Some("@db.Interval")),
                SimpleColumnType::Bytea => ("Bytes", None),
                SimpleColumnType::Uuid => ("String", Some("@db.Uuid")),
                SimpleColumnType::Json => ("Json", None),
                SimpleColumnType::Inet => ("String", Some("@db.Inet")),
                SimpleColumnType::Cidr => ("String", Some("@db.Cidr")),
                SimpleColumnType::Macaddr => ("String", Some("@db.Macaddr")),
                SimpleColumnType::Xml => ("String", Some("@db.Xml")),
                // Unknown/future simple types fall back to a plain String column.
                _ => ("String", None),
            };
            (format!("{}{}", base, q), native.map(str::to_string))
        }
        ColumnType::Complex(complex) => match complex {
            ComplexColumnType::Varchar { length } => {
                (format!("String{}", q), Some(format!("@db.VarChar({})", length)))
            }
            ComplexColumnType::Char { length } => {
                (format!("String{}", q), Some(format!("@db.Char({})", length)))
            }
            ComplexColumnType::Numeric { precision, scale } => {
                (format!("Decimal{}", q), Some(format!("@db.Decimal({}, {})", precision, scale)))
            }
            ComplexColumnType::Custom { custom_type } => {
                (format!("Unsupported(\"{}\"){}", custom_type, q), None)
            }
            ComplexColumnType::Enum { name, .. } => {
                (format!("{}{}", to_pascal_case(name), q), None)
            }
            // Unknown/future complex types fall back to a plain String column.
            _ => (format!("String{}", q), None),
        },
    }
}

fn prisma_default_attr(default_sql: String, col_type: &ColumnType) -> String {
    if default_sql == "true" {
        return "@default(true)".into();
    }
    if default_sql == "false" {
        return "@default(false)".into();
    }

    let lower = default_sql.to_lowercase();
    if lower.contains("now()") || lower.starts_with("current_timestamp") {
        return "@default(now())".into();
    }
    if lower.contains("gen_random_uuid()")
        || lower.contains("uuid_generate_v4()")
        || lower.contains("newid()")
    {
        return "@default(uuid())".into();
    }

    // Any remaining function call → dbgenerated
    if default_sql.contains('(') {
        let escaped = default_sql.replace('"', "\\\"");
        return format!("@default(dbgenerated(\"{}\"))", escaped);
    }

    // String literal with quotes — may be an enum value
    if default_sql.starts_with('\'') || default_sql.starts_with('"') {
        let stripped = default_sql.trim_matches(|c| c == '\'' || c == '"');
        if let ColumnType::Complex(ComplexColumnType::Enum { values, .. }) = col_type {
            if let EnumValues::String(variants) = values {
                if variants.iter().any(|v| v.as_str() == stripped) {
                    let variant = to_screaming_snake(stripped);
                    return format!("@default({})", variant);
                }
            }
        }
        return format!("@default(\"{}\")", stripped.replace('\\', "\\\\").replace('"', "\\\""));
    }

    // Numeric
    if default_sql.parse::<f64>().is_ok() {
        return format!("@default({})", default_sql);
    }

    // Fallback
    let escaped = default_sql.replace('"', "\\\"");
    format!("@default(dbgenerated(\"{}\"))", escaped)
}

fn reference_action_to_prisma(action: &ReferenceAction) -> &'static str {
    match action {
        ReferenceAction::Cascade => "Cascade",
        ReferenceAction::Restrict => "Restrict",
        ReferenceAction::SetNull => "SetNull",
        ReferenceAction::SetDefault => "SetDefault",
        ReferenceAction::NoAction => "NoAction",
        // Unknown/future referential actions fall back to Prisma's default.
        _ => "NoAction",
    }
}

fn infer_relation_field_name(fk_col: &str) -> String {
    fk_col.strip_suffix("_id").unwrap_or(fk_col).to_string()
}

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

fn to_screaming_snake(s: &str) -> String {
    let mut result = String::new();
    let mut prev_lower = false;
    for ch in s.chars() {
        if ch.is_uppercase() && prev_lower {
            result.push('_');
        }
        if ch.is_alphanumeric() {
            result.push(ch.to_ascii_uppercase());
            prev_lower = ch.is_lowercase();
        } else {
            result.push('_');
            prev_lower = false;
        }
    }
    result.trim_end_matches('_').to_string()
}
