use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use vespertide_core::{TableConstraint, TableDef};

use super::imports::{
    absolute_module_path, resolve_relation_entity_module_path, sanitize_field_name, to_pascal_case,
    to_snake_case, unique_name,
};
use super::render::{primary_key_columns, single_column_unique_set};

/// Extract FK info from a constraint as a tuple.
pub(super) fn as_fk(constraint: &TableConstraint) -> Option<(&[String], &str, &[String])> {
    match constraint {
        TableConstraint::ForeignKey {
            columns,
            ref_table,
            ref_columns,
            ..
        } => Some((
            columns.as_slice(),
            ref_table.as_str(),
            ref_columns.as_slice(),
        )),
        _ => None,
    }
}

/// Resolve FK chain to find the ultimate target table.
/// If the referenced column is itself a FK, follow the chain.
pub(super) fn resolve_fk_target<'a>(
    ref_table: &'a str,
    ref_columns: &[String],
    schema: &'a [TableDef],
) -> (&'a str, Vec<String>) {
    let mut visited = BTreeSet::new();
    resolve_fk_target_inner(ref_table, ref_columns, schema, &mut visited)
}

pub(super) fn resolve_fk_target_inner<'a>(
    ref_table: &'a str,
    ref_columns: &[String],
    schema: &'a [TableDef],
    visited: &mut BTreeSet<(String, String)>,
) -> (&'a str, Vec<String>) {
    // If no schema context or ref_columns is not a single column, return as-is
    if schema.is_empty() || ref_columns.len() != 1 {
        return (ref_table, ref_columns.to_vec());
    }

    let ref_col = &ref_columns[0];
    visited.insert((ref_table.to_string(), ref_col.clone()));

    // Find the referenced table in schema
    let Some(target_table) = schema.iter().find(|t| t.name == ref_table) else {
        return (ref_table, ref_columns.to_vec());
    };

    // Check if the referenced column has a FK constraint and follow the chain
    for constraint in &target_table.constraints {
        let fk_match =
            as_fk(constraint).filter(|(cols, _, _)| cols.len() == 1 && cols[0] == *ref_col);
        if let Some((_, next_table, next_cols)) = fk_match {
            let next_key = (next_table.to_string(), next_cols[0].clone());
            if visited.contains(&next_key) {
                return (ref_table, ref_columns.to_vec());
            }

            return resolve_fk_target_inner(next_table, next_cols, schema, visited);
        }
    }

    // No further FK chain, return current target
    (ref_table, ref_columns.to_vec())
}

pub(super) fn relation_field_defs_with_schema(
    table: &TableDef,
    schema: &[TableDef],
    module_paths: &HashMap<String, Vec<String>>,
    crate_prefix: &str,
) -> Vec<String> {
    let mut out = Vec::new();
    let mut used = HashSet::new();

    // First, collect ALL target entities from both forward and reverse relations
    // to detect when relation_enum is needed (same entity appears multiple times)
    let mut all_target_entities: Vec<String> = Vec::new();

    // Collect forward relation targets (belongs_to)
    for constraint in &table.constraints {
        if let TableConstraint::ForeignKey {
            ref_table,
            ref_columns,
            ..
        } = constraint
        {
            let (resolved_table, _) = resolve_fk_target(ref_table, ref_columns, schema);
            all_target_entities.push(resolved_table.to_string());
        }
    }

    // Collect reverse relation targets (has_one/has_many)
    let reverse_targets = collect_reverse_relation_targets(table, schema);
    all_target_entities.extend(reverse_targets);

    // Count occurrences of each target entity
    // perf: BTreeMap keeps generated relation analysis deterministic without hashing small maps.
    let mut entity_count: BTreeMap<String, usize> = BTreeMap::new();
    for entity in &all_target_entities {
        *entity_count.entry(entity.clone()).or_insert(0) += 1;
    }

    // Group FKs by their target table to detect duplicates within forward relations
    // perf: BTreeMap keeps duplicate-FK grouping deterministic and avoids hash setup overhead.
    let mut fk_by_table: BTreeMap<String, Vec<&TableConstraint>> = BTreeMap::new();
    for constraint in &table.constraints {
        if let TableConstraint::ForeignKey {
            ref_table,
            ref_columns,
            ..
        } = constraint
        {
            let (resolved_table, _) = resolve_fk_target(ref_table, ref_columns, schema);
            fk_by_table
                .entry(resolved_table.to_string())
                .or_default()
                .push(constraint);
        }
    }

    // Track used relation_enum names across all relations
    let mut used_relation_enums: HashSet<String> = HashSet::new();

    // belongs_to relations (this table has FK to other tables)
    for constraint in &table.constraints {
        if let TableConstraint::ForeignKey {
            columns,
            ref_table,
            ref_columns,
            ..
        } = constraint
        {
            // Resolve FK chain to find ultimate target
            let (resolved_table, resolved_columns) =
                resolve_fk_target(ref_table, ref_columns, schema);

            let from = fk_attr_value(columns);
            let to = fk_attr_value(&resolved_columns);

            // Check if there are multiple FKs to the same target table (within forward relations)
            let fks_to_this_table = fk_by_table
                .get(resolved_table)
                .map_or(0, std::vec::Vec::len);

            // Check if this target entity appears multiple times across ALL relations
            let entity_appears_multiple_times =
                entity_count.get(resolved_table).is_some_and(|c| *c > 1);

            // Smart field name inference from FK column names
            let field_base = if columns.len() == 1 {
                infer_field_name_from_fk_column(&columns[0], resolved_table, &to)
            } else {
                sanitize_field_name(resolved_table)
            };

            let field_name = unique_name(&field_base, &mut used);

            // Generate relation_enum if:
            // 1. Multiple FKs to same table within this table's forward relations, OR
            // 2. This target entity appears in both forward and reverse relations
            let needs_relation_enum = fks_to_this_table > 1 || entity_appears_multiple_times;

            let attr = if needs_relation_enum {
                let base_relation_enum = generate_relation_enum_name(columns);
                let relation_enum_name = if used_relation_enums.contains(&base_relation_enum) {
                    format!("{}{}", base_relation_enum, to_pascal_case(&table.name))
                } else {
                    base_relation_enum.clone()
                };
                used_relation_enums.insert(relation_enum_name.clone());
                format!(
                    "    #[sea_orm(belongs_to, relation_enum = \"{relation_enum_name}\", from = \"{from}\", to = \"{to}\")]"
                )
            } else {
                format!("    #[sea_orm(belongs_to, from = \"{from}\", to = \"{to}\")]")
            };

            out.push(attr);
            let entity_path = resolve_relation_entity_module_path(
                &table.name,
                resolved_table,
                module_paths,
                crate_prefix,
            );
            out.push(format!(
                "    pub {field_name}: HasOne<{entity_path}::Entity>,"
            ));
        }
    }

    // has_one/has_many relations (other tables have FK to this table)
    let reverse_relations = reverse_relation_field_defs(
        table,
        schema,
        &mut used,
        &entity_count,
        &mut used_relation_enums,
        module_paths,
        crate_prefix,
    );
    out.extend(reverse_relations);

    out
}

/// Generate a relation enum name from foreign key column names.
/// For "`creator_user_id`", generates "`CreatorUser`".
/// For composite FKs like [`org_id`, `user_id`], generates `OrgUser`.
pub(super) fn generate_relation_enum_name(columns: &[String]) -> String {
    // Take the first column and remove common FK suffixes like "_id"
    let first_col = &columns[0];
    let without_id = if let Some(stripped) = first_col.strip_suffix("_id") {
        stripped
    } else {
        first_col
    };

    to_pascal_case(without_id)
}

pub(super) fn unique_relation_enum_name(
    preferred: String,
    source_table: &str,
    base_relation_enum: &str,
    used_relation_enums: &HashSet<String>,
) -> String {
    if !used_relation_enums.contains(&preferred) {
        return preferred;
    }

    let source_prefixed = format!("{}{}", to_pascal_case(source_table), base_relation_enum);
    if !used_relation_enums.contains(&source_prefixed) {
        return source_prefixed;
    }

    let mut index = 2;
    loop {
        let candidate = format!(
            "{}{}{}",
            to_pascal_case(source_table),
            base_relation_enum,
            index
        );
        if !used_relation_enums.contains(&candidate) {
            return candidate;
        }
        index += 1;
    }
}

pub(super) fn collect_self_ref_junction(
    current_table: &TableDef,
    junction_table: &TableDef,
    junction_pk: &HashSet<String>,
) -> Option<SelfRefJunction> {
    if junction_pk.len() < 2 {
        return None;
    }

    let fks: Vec<_> = junction_table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::ForeignKey {
                columns, ref_table, ..
            } = c
            {
                Some((columns.clone(), ref_table.clone()))
            } else {
                None
            }
        })
        .collect();

    if fks.len() < 2 {
        return None;
    }

    let all_fk_cols_in_pk = fks
        .iter()
        .all(|(cols, _)| cols.iter().all(|c| junction_pk.contains(c)));
    if !all_fk_cols_in_pk {
        return None;
    }

    if !fks
        .iter()
        .all(|(_, ref_table)| ref_table == &current_table.name)
    {
        return None;
    }

    Some(SelfRefJunction {
        junction_table: junction_table.name.clone(),
        role_columns: fks.iter().map(|(cols, _)| cols[0].clone()).collect(),
        role_relations: fks
            .iter()
            .map(|(cols, _)| generate_relation_enum_name(cols))
            .collect(),
    })
}

pub(super) fn self_ref_link_name(
    self_ref_junction: &SelfRefJunction,
    from_idx: usize,
    to_idx: usize,
) -> String {
    format!(
        "{}To{}Via{}",
        to_pascal_case(&self_ref_junction.role_columns[from_idx]),
        to_pascal_case(&self_ref_junction.role_columns[to_idx]),
        to_pascal_case(&self_ref_junction.junction_table)
    )
}

pub(super) fn resolve_self_ref_link_module_path(
    current_table: &str,
    junction_table: &str,
    module_paths: &HashMap<String, Vec<String>>,
    crate_prefix: &str,
) -> String {
    if let (Some(current), Some(target)) = (
        module_paths.get(current_table),
        module_paths.get(junction_table),
    ) {
        let current_parent = current.split_last().map_or(&[][..], |(_, parent)| parent);
        let target_parent = target.split_last().map_or(&[][..], |(_, parent)| parent);

        if current_parent == target_parent {
            return format!("super::{junction_table}");
        }

        if !crate_prefix.is_empty() {
            return absolute_module_path(crate_prefix, target);
        }

        return absolute_module_path("crate::models", target);
    }

    format!("super::{junction_table}")
}

pub(super) fn render_self_ref_link_helpers(
    table: &TableDef,
    schema: &[TableDef],
    module_paths: &HashMap<String, Vec<String>>,
    crate_prefix: &str,
) -> Vec<String> {
    let mut out = Vec::new();

    for other_table in schema {
        if other_table.name == table.name {
            continue;
        }

        let other_pk = primary_key_columns(other_table);
        let Some(self_ref_junction) = collect_self_ref_junction(table, other_table, &other_pk)
        else {
            continue;
        };

        let junction_entity_path = resolve_self_ref_link_module_path(
            &table.name,
            &self_ref_junction.junction_table,
            module_paths,
            crate_prefix,
        );

        for (from_idx, from_role) in self_ref_junction.role_relations.iter().enumerate() {
            for (to_idx, to_role) in self_ref_junction.role_relations.iter().enumerate() {
                if from_idx == to_idx {
                    continue;
                }

                let link_name = self_ref_link_name(&self_ref_junction, from_idx, to_idx);
                out.push(format!("pub struct {link_name};"));
                out.push(format!("impl Linked for {link_name} {{"));
                out.push("    type FromEntity = Entity;".into());
                out.push("    type ToEntity = Entity;".into());
                out.push(String::new());
                out.push("    fn link(&self) -> Vec<RelationDef> {".into());
                out.push("        vec![".into());
                out.push(format!(
                    "            {junction_entity_path}::Relation::{from_role}.def().rev(),"
                ));
                out.push(format!(
                    "            {junction_entity_path}::Relation::{to_role}.def(),"
                ));
                out.push("        ]".into());
                out.push("    }".into());
                out.push("}".into());
                out.push(String::new());
            }
        }
    }

    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }

    out
}

pub(super) fn render_self_ref_query_helpers(table: &TableDef, schema: &[TableDef]) -> Vec<String> {
    let mut methods = Vec::new();
    let mut used_method_names = HashSet::new();

    for other_table in schema {
        if other_table.name == table.name {
            continue;
        }

        let other_pk = primary_key_columns(other_table);
        let Some(self_ref_junction) = collect_self_ref_junction(table, other_table, &other_pk)
        else {
            continue;
        };

        for (from_idx, from_col) in self_ref_junction.role_columns.iter().enumerate() {
            for (to_idx, to_col) in self_ref_junction.role_columns.iter().enumerate() {
                if from_idx == to_idx {
                    continue;
                }

                let link_name = self_ref_link_name(&self_ref_junction, from_idx, to_idx);
                let method_base = format!(
                    "find_{}_via_{}_from_{}",
                    pluralize(&sanitize_field_name(to_col)),
                    sanitize_field_name(&self_ref_junction.junction_table),
                    sanitize_field_name(from_col)
                );
                let method_name = unique_name(&method_base, &mut used_method_names);

                methods.push(format!(
                    "    pub fn {method_name}(&self) -> Select<Entity> {{"
                ));
                methods.push(format!("        self.find_linked({link_name})"));
                methods.push("    }".into());
                methods.push(String::new());
            }
        }
    }

    while methods.last().is_some_and(String::is_empty) {
        methods.pop();
    }

    if methods.is_empty() {
        return methods;
    }

    let mut out = Vec::new();
    out.push("impl Model {".into());
    out.extend(methods);
    out.push("}".into());
    out
}

/// Infer a field name from a single FK column.
/// For "`creator_user_id`" with to="id", tries "`creator_user`" first.
/// If the FK column still follows common suffix naming like `_id`/`_idx`,
/// remove those as fallbacks for intuitive relation names.
/// If that ends with the table name, use the full column name (without the to suffix).
/// Otherwise, fall back to the table name.
///
/// Examples:
/// - FK column: "`creator_user_id`", table: "user", to: "id" -> "`creator_user`"
/// - FK column: "`creator_user_idx`", table: "user", to: "idx" -> "`creator_user`"
/// - FK column: "`user_id`", table: "user", to: "id" -> "user" (falls back to table name)
/// - FK column: "`order_id`", table: "order", to: "`order_number`" -> "order"
/// - FK column: "`order_idx`", table: "order", to: "`order_number`" -> "order"
/// - FK column: "`org_id`", table: "user", to: "id" -> "org"
pub(super) fn infer_field_name_from_fk_column(
    fk_column: &str,
    table_name: &str,
    to: &str,
) -> String {
    let table_lower = table_name.to_lowercase();
    let to_lower = to.to_lowercase();

    // Remove the "to" suffix from FK column (e.g., "user_id" for to="id", "user_idx" for to="idx").
    // If FK column still uses common suffixes like "*_id"/"*_idx", strip them as fallbacks.
    let to_suffix = format!("_{to}");
    let without_suffix = fk_column
        .strip_suffix(&to_suffix)
        .or_else(|| fk_column.strip_suffix("_id"))
        .or_else(|| fk_column.strip_suffix("_idx"))
        .unwrap_or(fk_column);

    let sanitized = sanitize_field_name(without_suffix);
    let sanitized_lower = sanitized.to_lowercase();

    // If the FK column exactly matches the referenced column name, treat it as a natural-key
    // relation and expose the target entity name instead of the raw column name.
    // Also handle compact forms like `username` for `user.name`.
    if sanitized_lower == to_lower || sanitized_lower == format!("{table_lower}{to_lower}") {
        return sanitize_field_name(table_name);
    }

    // If the sanitized name is exactly the table name (e.g., "user_id" -> "user" for table "user"),
    // we need to fall back to the table name for proper disambiguation
    if sanitized_lower == table_lower {
        sanitize_field_name(table_name)
    }
    // If the sanitized name ends with (but is not equal to) the table name, use it as-is
    // This handles cases like "creator_user" for table "user"
    else if sanitized_lower.ends_with(&table_lower) {
        sanitized
    } else {
        // Otherwise, use the inferred name from the column
        sanitized
    }
}

/// Information about a reverse relation to be generated.
pub(super) struct ReverseRelation {
    /// Target entity name (the table that has FK to current table)
    target_entity: String,
    /// Whether it's `has_one` (true) or `has_many` (false)
    is_one_to_one: bool,
    /// Base field name before uniquification
    field_base: String,
    /// Base `relation_enum` name (from FK columns)
    base_relation_enum: String,
    /// Source table name (for disambiguation)
    source_table: String,
    /// Whether the source table has multiple FKs to current table
    has_multiple_fks: bool,
    /// Optional via clause for M2M relations
    via: Option<String>,
    /// Optional `via_rel` clause for reverse diamond relations
    via_rel: Option<String>,
    /// Whether this is a M2M relation (through junction table)
    is_m2m: bool,
}

pub(super) struct SelfRefJunction {
    junction_table: String,
    role_columns: Vec<String>,
    role_relations: Vec<String>,
}

/// Collect target entities from reverse relations (for counting across all relations).
pub(super) fn collect_reverse_relation_targets(
    table: &TableDef,
    schema: &[TableDef],
) -> Vec<String> {
    let mut targets = Vec::new();

    for other_table in schema {
        if other_table.name == table.name {
            continue;
        }

        // Get PK columns for junction table detection
        let other_pk = primary_key_columns(other_table);

        // Check if this is a junction table
        if let Some(m2m_targets) =
            collect_many_to_many_targets(table, other_table, &other_pk, schema)
        {
            targets.extend(m2m_targets);
            continue;
        }

        // Check for direct FK to this table
        for constraint in &other_table.constraints {
            if let TableConstraint::ForeignKey { ref_table, .. } = constraint
                && ref_table == &table.name
            {
                targets.push(other_table.name.clone());
            }
        }
    }

    targets
}

/// Collect target entities from a junction table for M2M relations.
pub(super) fn collect_many_to_many_targets(
    current_table: &TableDef,
    junction_table: &TableDef,
    junction_pk: &HashSet<String>,
    schema: &[TableDef],
) -> Option<Vec<String>> {
    if junction_pk.len() < 2 {
        return None;
    }

    let fks: Vec<_> = junction_table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::ForeignKey {
                columns, ref_table, ..
            } = c
            {
                Some((columns.clone(), ref_table.clone()))
            } else {
                None
            }
        })
        .collect();

    if fks.len() < 2 {
        return None;
    }

    let all_fk_cols_in_pk = fks
        .iter()
        .all(|(cols, _)| cols.iter().all(|c| junction_pk.contains(c)));

    if !all_fk_cols_in_pk {
        return None;
    }

    fks.iter()
        .find(|(_, ref_table)| ref_table == &current_table.name)?;

    let mut targets = Vec::new();

    // Junction table itself
    targets.push(junction_table.name.clone());

    // Target tables via M2M
    for (_, ref_table) in &fks {
        if ref_table == &current_table.name {
            continue;
        }
        let target_exists = schema.iter().any(|t| &t.name == ref_table);
        if target_exists {
            targets.push(ref_table.clone());
        }
    }

    Some(targets)
}

/// Generate reverse relation fields (`has_one/has_many`) for tables that reference this table.
pub(super) fn reverse_relation_field_defs(
    table: &TableDef,
    schema: &[TableDef],
    used: &mut HashSet<String>,
    entity_count: &BTreeMap<String, usize>,
    used_relation_enums: &mut HashSet<String>,
    module_paths: &HashMap<String, Vec<String>>,
    crate_prefix: &str,
) -> Vec<String> {
    reverse_relation_field_defs_inner(ReverseRelationFieldCtx {
        table,
        schema,
        used,
        entity_count,
        used_relation_enums,
        module_paths,
        crate_prefix,
    })
}

struct ReverseRelationFieldCtx<'a> {
    table: &'a TableDef,
    schema: &'a [TableDef],
    used: &'a mut HashSet<String>,
    entity_count: &'a BTreeMap<String, usize>,
    used_relation_enums: &'a mut HashSet<String>,
    module_paths: &'a HashMap<String, Vec<String>>,
    crate_prefix: &'a str,
}

#[expect(
    clippy::too_many_lines,
    reason = "relation rendering keeps collection and emission logic together"
)]
fn reverse_relation_field_defs_inner(ctx: ReverseRelationFieldCtx<'_>) -> Vec<String> {
    let ReverseRelationFieldCtx {
        table,
        schema,
        used,
        entity_count,
        used_relation_enums,
        module_paths,
        crate_prefix,
    } = ctx;
    // First pass: collect all reverse relations
    let mut relations: Vec<ReverseRelation> = Vec::new();

    // Count how many FKs from each table reference this table
    let mut fk_count_per_table: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for other_table in schema {
        if other_table.name == table.name {
            continue;
        }
        for constraint in &other_table.constraints {
            if let TableConstraint::ForeignKey { ref_table, .. } = constraint
                && ref_table == &table.name
            {
                *fk_count_per_table
                    .entry(other_table.name.clone())
                    .or_insert(0) += 1;
            }
        }
    }

    // Collect all relations from all tables
    for other_table in schema {
        if other_table.name == table.name {
            continue;
        }

        // Get PK and unique columns for the other table
        let other_pk = primary_key_columns(other_table);
        let other_unique = single_column_unique_set(&other_table.constraints);

        // Check if this is a junction table (composite PK with multiple FKs)
        if let Some(m2m_relations) =
            collect_many_to_many_relations(table, other_table, &other_pk, schema)
        {
            relations.extend(m2m_relations);
            continue;
        }

        for constraint in &other_table.constraints {
            if let TableConstraint::ForeignKey {
                columns, ref_table, ..
            } = constraint
            {
                // Check if this FK references our table
                if ref_table == &table.name {
                    // Determine if it's has_one or has_many
                    let is_one_to_one = if columns.len() == 1 {
                        let col = &columns[0];
                        let is_sole_pk = other_pk.len() == 1 && other_pk.contains(col);
                        let is_unique = other_unique.contains(col);
                        is_sole_pk || is_unique
                    } else {
                        columns.len() == other_pk.len()
                            && columns.iter().all(|c| other_pk.contains(c))
                    };

                    let has_multiple_fks = fk_count_per_table
                        .get(&other_table.name)
                        .is_some_and(|count| *count > 1);

                    // Generate base field name
                    let base_relation_enum = generate_relation_enum_name(columns);
                    let field_base = if has_multiple_fks {
                        let lowercase_enum = to_snake_case(&base_relation_enum);
                        if is_one_to_one {
                            lowercase_enum
                        } else {
                            format!(
                                "{}_{}",
                                lowercase_enum,
                                pluralize(&sanitize_field_name(&other_table.name))
                            )
                        }
                    } else if is_one_to_one {
                        sanitize_field_name(&other_table.name)
                    } else {
                        pluralize(&sanitize_field_name(&other_table.name))
                    };

                    relations.push(ReverseRelation {
                        target_entity: other_table.name.clone(),
                        is_one_to_one,
                        field_base,
                        base_relation_enum,
                        source_table: other_table.name.clone(),
                        has_multiple_fks,
                        via: None,
                        via_rel: Some(generate_relation_enum_name(columns)),
                        is_m2m: false,
                    });
                }
            }
        }
    }

    // Second pass: generate output with relation_enum when needed
    let mut out = Vec::new();

    for rel in relations {
        let relation_type = if rel.is_one_to_one {
            "has_one"
        } else {
            "has_many"
        };
        let rust_type = if rel.is_one_to_one {
            "HasOne"
        } else {
            "HasMany"
        };
        let field_name = unique_name(&rel.field_base, used);

        // Determine if we need relation_enum:
        // 1. Multiple FKs from same source table, OR
        // 2. Multiple relations targeting the same entity (across ALL relations including forward)
        let needs_relation_enum =
            rel.has_multiple_fks || entity_count.get(&rel.target_entity).is_some_and(|c| *c > 1);

        let attr = if needs_relation_enum {
            let preferred_relation_enum_name = if rel.is_m2m {
                // M2M: use {Target}Via{Junction} pattern directly
                // e.g., "MediaViaUserMediaRole"
                rel.base_relation_enum.clone()
            } else {
                let via_value = rel.via.as_ref().unwrap_or(&rel.source_table);
                // Direct: use via table name, fall back to FK-based on collision
                let base_enum = to_pascal_case(via_value);
                if used_relation_enums.contains(&base_enum) {
                    rel.base_relation_enum.clone()
                } else {
                    base_enum
                }
            };
            let relation_enum_name = unique_relation_enum_name(
                preferred_relation_enum_name,
                &rel.source_table,
                &rel.base_relation_enum,
                used_relation_enums,
            );
            used_relation_enums.insert(relation_enum_name.clone());

            if let Some(via_rel) = &rel.via_rel {
                format!(
                    "    #[sea_orm({relation_type}, relation_enum = \"{relation_enum_name}\", via_rel = \"{via_rel}\")]"
                )
            } else if let Some(via) = &rel.via {
                format!(
                    "    #[sea_orm({relation_type}, relation_enum = \"{relation_enum_name}\", via = \"{via}\")]"
                )
            } else {
                format!("    #[sea_orm({relation_type}, relation_enum = \"{relation_enum_name}\")]")
            }
        } else if let Some(via) = &rel.via {
            // No ambiguity - just via without relation_enum
            format!("    #[sea_orm({relation_type}, via = \"{via}\")]")
        } else {
            format!("    #[sea_orm({relation_type})]")
        };

        out.push(attr);
        let entity_path = resolve_relation_entity_module_path(
            &table.name,
            &rel.target_entity,
            module_paths,
            crate_prefix,
        );
        out.push(format!(
            "    pub {field_name}: {rust_type}<{entity_path}::Entity>,"
        ));
    }

    out
}

/// Collect many-to-many relations from a junction table.
/// Returns Some(relations) if it's a junction table that links current table to other tables,
/// or None if it's not a junction table.
pub(super) fn collect_many_to_many_relations(
    current_table: &TableDef,
    junction_table: &TableDef,
    junction_pk: &HashSet<String>,
    schema: &[TableDef],
) -> Option<Vec<ReverseRelation>> {
    // Junction table must have composite PK (2+ columns)
    if junction_pk.len() < 2 {
        return None;
    }

    // Collect all FKs from the junction table
    let fks: Vec<_> = junction_table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::ForeignKey {
                columns, ref_table, ..
            } = c
            {
                Some((columns.clone(), ref_table.clone()))
            } else {
                None
            }
        })
        .collect();

    // Must have at least 2 FKs to be a junction table
    if fks.len() < 2 {
        return None;
    }

    // Check if all FK columns are part of the PK (typical junction table pattern)
    let all_fk_cols_in_pk = fks
        .iter()
        .all(|(cols, _)| cols.iter().all(|c| junction_pk.contains(c)));

    if !all_fk_cols_in_pk {
        return None;
    }

    // Find which FK references the current table
    fks.iter()
        .find(|(_, ref_table)| ref_table == &current_table.name)?;

    let mut relations = Vec::new();

    let self_ref_fks: Vec<_> = fks
        .iter()
        .filter(|(_, ref_table)| ref_table == &current_table.name)
        .cloned()
        .collect();

    if self_ref_fks.len() == fks.len() {
        return None;
    }

    // First, add has_many to the junction table itself (direct relation, not M2M)
    let junction_base = pluralize(&sanitize_field_name(&junction_table.name));
    relations.push(ReverseRelation {
        target_entity: junction_table.name.clone(),
        is_one_to_one: false,
        field_base: junction_base,
        base_relation_enum: to_pascal_case(&junction_table.name),
        source_table: junction_table.name.clone(),
        has_multiple_fks: false,
        via: None,
        via_rel: None,
        is_m2m: false,
    });

    // Then add has_many with via for the target tables (M2M relations)
    for (_columns, ref_table) in &fks {
        // Skip the FK to the current table itself
        if ref_table == &current_table.name {
            continue;
        }

        // Find the target table in schema
        let target_exists = schema.iter().any(|t| &t.name == ref_table);
        if !target_exists {
            continue;
        }

        // M2M field name: {target}_via_{junction} to distinguish from direct relations
        // e.g., "medias_via_user_media_role" instead of "medias" (which collides with direct FK)
        let field_base = format!(
            "{}_via_{}",
            pluralize(&sanitize_field_name(ref_table)),
            sanitize_field_name(&junction_table.name)
        );
        // M2M relation_enum: {Target}Via{Junction} pattern
        // e.g., "MediaViaUserMediaRole" for media through user_media_role
        let base_relation_enum = format!(
            "{}Via{}",
            to_pascal_case(ref_table),
            to_pascal_case(&junction_table.name)
        );

        relations.push(ReverseRelation {
            target_entity: ref_table.clone(),
            is_one_to_one: false,
            field_base,
            base_relation_enum,
            source_table: junction_table.name.clone(),
            has_multiple_fks: false,
            via: Some(junction_table.name.clone()),
            via_rel: None,
            is_m2m: true,
        });
    }

    Some(relations)
}

/// Simple pluralization for field names (adds 's' suffix).
pub(super) fn pluralize(name: &str) -> String {
    if name.ends_with('s') || name.ends_with("es") {
        name.to_string()
    } else if name.ends_with('y')
        && !name.ends_with("ay")
        && !name.ends_with("ey")
        && !name.ends_with("oy")
        && !name.ends_with("uy")
    {
        // e.g., category -> categories
        format!("{}ies", name.strip_suffix('y').unwrap_or(name))
    } else {
        format!("{name}s")
    }
}

pub(super) fn fk_attr_value(cols: &[String]) -> String {
    if cols.len() == 1 {
        cols[0].clone()
    } else {
        format!("({})", cols.join(", "))
    }
}
