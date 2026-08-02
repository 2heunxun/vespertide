use std::collections::HashMap;

use vespertide_core::TableDef;
use vespertide_core::schema::column::{ColumnType, ComplexColumnType, EnumValues};
use vespertide_core::schema::constraint::TableConstraint;
use vespertide_core::schema::names::ColumnName;
use vespertide_core::schema::reference::ReferenceAction;
use vespertide_naming::{to_pascal_case, to_screaming_snake_case};

use super::types::column_type_to_prisma;

struct PkInfo {
    columns: Vec<String>,
    auto_increment: bool,
}

fn extract_pk_info(constraints: &[TableConstraint]) -> PkInfo {
    for c in constraints {
        if let TableConstraint::PrimaryKey {
            auto_increment,
            columns,
            ..
        } = c
        {
            return PkInfo {
                columns: columns.iter().map(ToString::to_string).collect(),
                auto_increment: *auto_increment,
            };
        }
    }
    PkInfo {
        columns: Vec::new(),
        auto_increment: false,
    }
}

struct FkInfo<'a> {
    ref_table: &'a str,
    ref_cols: &'a [ColumnName],
    on_delete: Option<&'a ReferenceAction>,
    on_update: Option<&'a ReferenceAction>,
}

pub(super) struct BackRelation {
    pub(super) source_table: String,
    pub(super) fk_col: String,
    pub(super) is_one_to_one: bool,
    pub(super) relation_name: Option<String>,
}

pub(super) fn back_rel_field(br: &BackRelation) -> (String, String) {
    let source_pascal = to_pascal_case(&br.source_table);
    let rel_type = if br.is_one_to_one {
        format!("{source_pascal}?")
    } else {
        format!("{source_pascal}[]")
    };

    let field_name = if br.relation_name.is_some() {
        let rel_field = infer_relation_field_name(&br.fk_col);
        format!("{rel_field}_{}", br.source_table)
    } else {
        br.source_table.clone()
    };

    (field_name, rel_type)
}

pub(super) fn collect_back_relations(target_table: &str, schema: &[TableDef]) -> Vec<BackRelation> {
    let mut result = Vec::new();

    for source in schema {
        let fks_to_target: Vec<(&str, &[ColumnName])> = source
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
                    if ref_table.as_str() == target_table && columns.len() == 1 {
                        Some((columns[0].as_str(), ref_columns.as_slice()))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        if fks_to_target.is_empty() {
            continue;
        }

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
                let source_pascal = to_pascal_case(&source.name);
                let rel_pascal = to_pascal_case(&rel_field);
                Some(format!("{source_pascal}{rel_pascal}"))
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

pub(super) fn render_model(table: &TableDef, schema: &[TableDef]) -> String {
    let mut lines: Vec<String> = Vec::new();

    if let Some(desc) = &table.description {
        for line in desc.lines() {
            lines.push(format!("/// {line}"));
        }
    }

    let model_name = to_pascal_case(&table.name);
    lines.push(format!("model {model_name} {{"));

    let pk_info = extract_pk_info(&table.constraints);
    let pk_columns: std::collections::HashSet<&str> =
        pk_info.columns.iter().map(String::as_str).collect();
    let is_composite_pk = pk_info.columns.len() > 1;

    let unique_single: HashMap<&str, Option<&str>> = table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::Unique { name, columns, .. } = c {
                if columns.len() == 1 {
                    Some((columns[0].as_str(), name.as_deref()))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    // FK lookup by column
    let fk_by_col: HashMap<&str, FkInfo<'_>> = table
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
                } else {
                    None
                }
            } else {
                None
            }
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
            let comment = comment.replace('\n', " ");
            lines.push(format!("  /// {comment}"));
        }

        let type_str = column_type_to_prisma(&col.r#type, col.nullable);
        let mut attrs: Vec<String> = Vec::new();

        if is_single_pk {
            attrs.push("@id".into());
            if auto_inc {
                attrs.push("@default(autoincrement())".into());
            }
        }

        if !auto_inc && let Some(ref default) = col.default {
            attrs.push(prisma_default_attr(&default.to_sql(), &col.r#type));
        }

        if let Some(unique_name) = is_unique
            && !is_single_pk
        {
            match unique_name {
                Some(n) => attrs.push(format!("@unique(map: \"{n}\")")),
                None => attrs.push("@unique".into()),
            }
        }

        let attrs_str = if attrs.is_empty() {
            String::new()
        } else {
            format!(" {}", attrs.join(" "))
        };

        lines.push(format!("  {col_name} {type_str}{attrs_str}"));

        // Emit inline relation field for FK columns
        if let Some(fk) = fk_by_col.get(col_name) {
            // `rel_name_segment` drives @relation("…") naming — must stay consistent
            // with the segment computed by collect_back_relations on the other side.
            let rel_name_segment = infer_relation_field_name(col_name);
            // When no `_id` was stripped the inferred name equals the FK column name,
            // which would collide with the scalar field.  Append `_rel` to deduplicate.
            let rel_field_name = if rel_name_segment == col_name {
                format!("{col_name}_rel")
            } else {
                rel_name_segment.clone()
            };
            let rel_model = to_pascal_case(fk.ref_table);
            let rel_type = if col.nullable {
                format!("{rel_model}?")
            } else {
                rel_model.clone()
            };

            let multi_fk = ref_table_fk_count.get(fk.ref_table).copied().unwrap_or(0) > 1;
            let is_self_ref = fk.ref_table == table.name.as_str();
            let needs_name = multi_fk || is_self_ref;

            let mut rel_args: Vec<String> = Vec::new();
            if needs_name {
                // Use rel_name_segment (pre-dedup) so the name matches back-relations.
                let table_pascal = to_pascal_case(&table.name);
                let field_pascal = to_pascal_case(&rel_name_segment);
                rel_args.push(format!("\"{table_pascal}{field_pascal}\""));
            }
            rel_args.push(format!("fields: [{col_name}]"));
            rel_args.push(format!(
                "references: [{}]",
                fk.ref_cols
                    .iter()
                    .map(ColumnName::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            if let Some(od) = fk.on_delete {
                let action = reference_action_to_prisma(od);
                rel_args.push(format!("onDelete: {action}"));
            }
            if let Some(ou) = fk.on_update {
                let action = reference_action_to_prisma(ou);
                rel_args.push(format!("onUpdate: {action}"));
            }

            let rel_args_str = rel_args.join(", ");
            lines.push(format!(
                "  {rel_field_name} {rel_type} @relation({rel_args_str})"
            ));
        }
    }

    // Back-relations from schema context
    if !schema.is_empty() {
        let back_rels = collect_back_relations(&table.name, schema);
        for br in &back_rels {
            let (field_name, rel_type) = back_rel_field(br);
            let rel_attr = match &br.relation_name {
                Some(name) => format!(" @relation(\"{name}\")"),
                None => String::new(),
            };
            lines.push(format!("  {field_name} {rel_type}{rel_attr}"));
        }
    }

    // Blank line before model-level attributes
    lines.push(String::new());

    // Composite PK
    if is_composite_pk {
        let pk_cols = pk_info.columns.join(", ");
        lines.push(format!("  @@id([{pk_cols}])"));
    }

    // Composite unique constraints
    for c in &table.constraints {
        if let TableConstraint::Unique { name, columns, .. } = c
            && columns.len() > 1
        {
            let cols = columns.join(", ");
            if let Some(n) = name {
                lines.push(format!("  @@unique([{cols}], map: \"{n}\")"));
            } else {
                lines.push(format!("  @@unique([{cols}])"));
            }
        }
    }

    // All index constraints
    for c in &table.constraints {
        if let TableConstraint::Index { name, columns } = c {
            let cols = columns.join(", ");
            // `match` instead of `if let Some`: LLVM coverage attributes match
            // arms reliably where this if/else was misattributed as uncovered.
            match name {
                Some(n) => lines.push(format!("  @@index([{cols}], map: \"{n}\")")),
                None => lines.push(format!("  @@index([{cols}])")),
            }
        }
    }

    // @@map (always present since model is PascalCase but table is snake_case)
    let table_name = table.name.as_str();
    lines.push(format!("  @@map(\"{table_name}\")"));
    lines.push("}".into());

    lines.join("\n")
}

fn prisma_default_attr(default_sql: &str, col_type: &ColumnType) -> String {
    // Integer-backed enum: resolve to a variant identifier (SCREAMING_SNAKE), never a bare int.
    if let ColumnType::Complex(ComplexColumnType::Enum {
        values: EnumValues::Integer(int_values),
        ..
    }) = col_type
    {
        let key = default_sql.trim_matches(|c: char| c == '\'' || c == '"');
        // 1) numeric value match → variant name
        if let Ok(n) = key.parse::<i64>()
            && let Some(v) = int_values.iter().find(|v| v.value == n)
        {
            return format!("@default({})", to_screaming_snake_case(&v.name));
        }
        // 2) exact variant-name match → variant name
        if let Some(v) = int_values.iter().find(|v| v.name == key) {
            return format!("@default({})", to_screaming_snake_case(&v.name));
        }
        // 3) no match → dbgenerated fallback (valid PSL; avoids bare-int type error)
        let escaped = key.replace('"', "\\\"");
        return format!("@default(dbgenerated(\"{escaped}\"))");
    }

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
        return format!("@default(dbgenerated(\"{escaped}\"))");
    }

    // String literal with quotes — may be an enum value
    if default_sql.starts_with('\'') || default_sql.starts_with('"') {
        let stripped = default_sql.trim_matches(|c| c == '\'' || c == '"');
        if let ColumnType::Complex(ComplexColumnType::Enum {
            values: EnumValues::String(variants),
            ..
        }) = col_type
            && variants.iter().any(|v| v.as_str() == stripped)
        {
            let variant = to_screaming_snake_case(stripped);
            return format!("@default({variant})");
        }
        let s = stripped.replace('\\', "\\\\").replace('"', "\\\"");
        return format!("@default(\"{s}\")");
    }

    // Numeric
    if default_sql.parse::<f64>().is_ok() {
        return format!("@default({default_sql})");
    }

    // Fallback
    let escaped = default_sql.replace('"', "\\\"");
    format!("@default(dbgenerated(\"{escaped}\"))")
}

fn reference_action_to_prisma(action: &ReferenceAction) -> &'static str {
    match action {
        ReferenceAction::Cascade => "Cascade",
        ReferenceAction::Restrict => "Restrict",
        ReferenceAction::SetNull => "SetNull",
        ReferenceAction::SetDefault => "SetDefault",
        // Includes NoAction and unknown/future referential actions.
        _ => "NoAction",
    }
}

fn infer_relation_field_name(fk_col: &str) -> String {
    fk_col.strip_suffix("_id").unwrap_or(fk_col).to_string()
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use vespertide_core::schema::column::{NumValue, SimpleColumnType};

    use super::*;

    #[rstest]
    #[case::cascade(ReferenceAction::Cascade, "Cascade")]
    #[case::restrict(ReferenceAction::Restrict, "Restrict")]
    #[case::set_null(ReferenceAction::SetNull, "SetNull")]
    #[case::set_default(ReferenceAction::SetDefault, "SetDefault")]
    #[case::no_action(ReferenceAction::NoAction, "NoAction")]
    fn reference_actions_map_to_prisma(#[case] action: ReferenceAction, #[case] expected: &str) {
        assert_eq!(reference_action_to_prisma(&action), expected);
    }

    fn string_enum() -> ColumnType {
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "doc_status".into(),
            values: EnumValues::String(vec!["draft".into(), "in progress".into()]),
        })
    }

    fn integer_enum() -> ColumnType {
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "priority".into(),
            values: EnumValues::Integer(vec![
                NumValue {
                    name: "low".into(),
                    value: 100,
                },
                NumValue {
                    name: "high".into(),
                    value: 200,
                },
            ]),
        })
    }

    #[rstest]
    #[case::bool_true("true", "@default(true)")]
    #[case::bool_false("false", "@default(false)")]
    #[case::now("now()", "@default(now())")]
    #[case::current_timestamp("CURRENT_TIMESTAMP", "@default(now())")]
    #[case::uuid_postgres("gen_random_uuid()", "@default(uuid())")]
    #[case::uuid_generate_v4("uuid_generate_v4()", "@default(uuid())")]
    #[case::uuid_mssql("NEWID()", "@default(uuid())")]
    #[case::other_function("gen_code()", "@default(dbgenerated(\"gen_code()\"))")]
    #[case::quoted_literal("'active'", "@default(\"active\")")]
    #[case::numeric("0", "@default(0)")]
    #[case::bare_word("SOME_CONSTANT", "@default(dbgenerated(\"SOME_CONSTANT\"))")]
    fn default_attr_maps_scalar_forms(#[case] default_sql: &str, #[case] expected: &str) {
        let non_enum = ColumnType::Simple(SimpleColumnType::Text);
        assert_eq!(prisma_default_attr(default_sql, &non_enum), expected);
    }

    #[rstest]
    #[case::string_variant("'draft'", string_enum(), "@default(DRAFT)")]
    #[case::string_variant_normalized("'in progress'", string_enum(), "@default(IN_PROGRESS)")]
    // Emitting `ARCHIVED` would reference a value the enum does not define.
    #[case::string_value_not_declared("'archived'", string_enum(), "@default(\"archived\")")]
    #[case::integer_by_value("100", integer_enum(), "@default(LOW)")]
    #[case::integer_by_name("high", integer_enum(), "@default(HIGH)")]
    #[case::integer_by_quoted_name("'high'", integer_enum(), "@default(HIGH)")]
    #[case::integer_value_not_declared("999", integer_enum(), "@default(dbgenerated(\"999\"))")]
    fn default_attr_resolves_enum_defaults(
        #[case] default_sql: &str,
        #[case] col_type: ColumnType,
        #[case] expected: &str,
    ) {
        assert_eq!(prisma_default_attr(default_sql, &col_type), expected);
    }

    #[test]
    fn fk_on_update_action_is_rendered() {
        let mut table = crate::tests::fixtures::table_with_fk();
        for c in &mut table.constraints {
            if let TableConstraint::ForeignKey { on_update, .. } = c {
                *on_update = Some(ReferenceAction::Cascade);
            }
        }
        let rendered = render_model(&table, std::slice::from_ref(&table));
        assert!(rendered.contains("onUpdate: Cascade"));
    }

    #[test]
    fn named_index_is_rendered_with_map() {
        let table = crate::tests::fixtures::table_with_indexes();
        let rendered = render_model(&table, &[]);
        assert!(rendered.contains("@@index([created_at], map: \"idx_articles_created_at\")"));
        assert!(rendered.contains("@@index([title])"));
    }
}
