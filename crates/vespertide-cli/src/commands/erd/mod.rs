pub mod dot;
pub mod mermaid;
pub mod svg;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::ValueEnum;
use vespertide_core::schema::foreign_key::ForeignKeySyntax;
use vespertide_core::schema::primary_key::PrimaryKeySyntax;
use vespertide_core::{ColumnDef, ReferenceAction, StrOrBoolOrArray, TableConstraint, TableDef};

use crate::utils::{load_config, load_models};

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum ErdFormat {
    Svg,
    Mermaid,
    Dot,
}

pub async fn cmd_erd_with_filters(
    format: ErdFormat,
    output: Option<PathBuf>,
    include: Vec<String>,
    exclude: Vec<String>,
    depth: usize,
) -> Result<()> {
    let config = load_config()?;
    let tables = filter_tables(
        normalize_tables(load_models(&config)?)?,
        &include,
        &exclude,
        depth,
    );

    let rendered = match format {
        ErdFormat::Svg => svg::render_svg(&tables).map_err(anyhow::Error::msg)?,
        ErdFormat::Mermaid => mermaid::render_mermaid(&tables),
        ErdFormat::Dot => dot::render_dot(&tables),
    };

    if let Some(path) = output {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("create ERD output directory {}", parent.display()))?;
        }

        tokio::fs::write(&path, rendered)
            .await
            .with_context(|| format!("write ERD output {}", path.display()))?;
        println!("ERD exported to {}", path.display());
    } else {
        print!("{rendered}");
    }

    Ok(())
}

#[expect(
    clippy::print_stderr,
    reason = "ERD filter warnings are user-facing diagnostics and must not mix with generated diagram stdout"
)]
pub(super) fn filter_tables(
    tables: Vec<TableDef>,
    include: &[String],
    exclude: &[String],
    depth: usize,
) -> Vec<TableDef> {
    let (tables, warnings) = filter_tables_with_warnings(tables, include, exclude, depth);
    for warning in warnings {
        eprintln!("{warning}");
    }
    tables
}

pub(super) fn filter_tables_with_warnings(
    tables: Vec<TableDef>,
    include: &[String],
    exclude: &[String],
    depth: usize,
) -> (Vec<TableDef>, Vec<String>) {
    if include.is_empty() && exclude.is_empty() {
        return (tables, Vec::new());
    }

    let include = normalized_filter_names(include);
    let exclude = normalized_filter_names(exclude);
    let all_names: BTreeSet<String> = tables.iter().map(|table| table.name.to_string()).collect();

    let mut warnings = filter_warnings(&all_names, "--include", &include);
    warnings.extend(filter_warnings(&all_names, "--exclude", &exclude));

    let mut kept: BTreeSet<String> = if include.is_empty() {
        all_names.clone()
    } else {
        include
            .iter()
            .filter(|name| all_names.contains(*name))
            .cloned()
            .collect()
    };

    let adjacency = build_fk_adjacency(&tables);
    for _ in 0..depth {
        let frontier: Vec<String> = kept.iter().cloned().collect();
        for name in frontier {
            if let Some(neighbors) = adjacency.get(&name) {
                kept.extend(
                    neighbors
                        .iter()
                        .filter(|neighbor| all_names.contains(*neighbor))
                        .cloned(),
                );
            }
        }
    }

    for name in exclude {
        kept.remove(&name);
    }

    let filtered = tables
        .into_iter()
        .filter(|table| kept.contains(table.name.as_str()))
        .collect();
    (filtered, warnings)
}

fn normalize_tables(tables: Vec<TableDef>) -> Result<Vec<TableDef>> {
    tables
        .into_iter()
        .map(|table| {
            table
                .normalize()
                .with_context(|| format!("normalize table '{}'", table.name))
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct ForeignKeyRelation {
    pub child_table: String,
    pub child_columns: Vec<String>,
    pub parent_table: String,
    pub parent_columns: Vec<String>,
    pub on_delete: Option<ReferenceAction>,
    pub on_update: Option<ReferenceAction>,
    pub cardinality: Cardinality,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Cardinality {
    OneToOne,
    OneToMany,
    ZeroOrOneToMany,
    ManyToMany,
}

impl Cardinality {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::OneToOne => "1:1",
            Self::OneToMany => "1:N",
            Self::ZeroOrOneToMany => "0..1:N",
            Self::ManyToMany => "M:N",
        }
    }
}

pub(super) fn collect_foreign_key_relations(tables: &[TableDef]) -> BTreeSet<ForeignKeyRelation> {
    let mut relations = BTreeSet::new();
    let table_lookup: BTreeMap<&str, &TableDef> = tables
        .iter()
        .map(|table| (table.name.as_str(), table))
        .collect();

    for table in tables {
        for constraint in &table.constraints {
            if let TableConstraint::ForeignKey {
                columns,
                ref_table,
                ref_columns,
                on_delete,
                on_update,
                ..
            } = constraint
            {
                let Some(parent_table) = table_lookup.get(ref_table.as_str()).copied() else {
                    continue;
                };
                relations.insert(build_foreign_key_relation(
                    table,
                    column_names_to_strings(columns),
                    ref_table.to_string(),
                    column_names_to_strings(ref_columns),
                    on_delete.clone(),
                    on_update.clone(),
                    parent_table,
                ));
            }
        }

        for column in &table.columns {
            if let Some(foreign_key) = &column.foreign_key
                && let Some(relation) =
                    inline_foreign_key_relation(table, column, foreign_key, &table_lookup)
            {
                relations.insert(relation);
            }
        }
    }

    relations
}

pub(super) fn is_primary_key_column(table: &TableDef, column_name: &str) -> bool {
    table
        .columns
        .iter()
        .any(|column| column.name == column_name && is_inline_primary_key(column))
        || table.constraints.iter().any(|constraint| {
            matches!(
                constraint,
                TableConstraint::PrimaryKey { columns, .. }
                    if columns.iter().any(|column| column == column_name)
            )
        })
}

pub(super) fn is_foreign_key_column(table: &TableDef, column_name: &str) -> bool {
    table
        .columns
        .iter()
        .any(|column| column.name == column_name && column.foreign_key.is_some())
        || table.constraints.iter().any(|constraint| {
            matches!(
                constraint,
                TableConstraint::ForeignKey { columns, .. }
                    if columns.iter().any(|column| column == column_name)
            )
        })
}

pub(super) fn column_markers(table: &TableDef, column: &ColumnDef) -> String {
    let mut markers = Vec::new();
    if is_primary_key_column(table, &column.name) {
        markers.push("PK");
    }
    if is_foreign_key_column(table, &column.name) {
        markers.push("FK");
    }

    if markers.is_empty() {
        String::new()
    } else {
        format!(" ({})", markers.join(", "))
    }
}

pub(super) fn sanitize_identifier(input: &str) -> String {
    let mut identifier = String::new();

    for (index, ch) in input.chars().enumerate() {
        if ch == '_' || ch.is_ascii_alphanumeric() {
            if index == 0 && ch.is_ascii_digit() {
                identifier.push('_');
            }
            identifier.push(ch);
        } else {
            identifier.push('_');
        }
    }

    if identifier.is_empty() {
        "_".to_string()
    } else {
        identifier
    }
}

fn inline_foreign_key_relation(
    table: &TableDef,
    column: &ColumnDef,
    foreign_key: &ForeignKeySyntax,
    table_lookup: &BTreeMap<&str, &TableDef>,
) -> Option<ForeignKeyRelation> {
    let (parent_table, parent_columns, on_delete, on_update) = match foreign_key {
        ForeignKeySyntax::String(reference) => {
            let (table, columns) = parse_reference(reference)?;
            (table, columns, None, None)
        }
        ForeignKeySyntax::Reference(reference) => {
            let (table, columns) = parse_reference(&reference.references)?;
            (
                table,
                columns,
                reference.on_delete.clone(),
                reference.on_update.clone(),
            )
        }
        ForeignKeySyntax::Object(definition) => (
            definition.ref_table.to_string(),
            column_names_to_strings(&definition.ref_columns),
            definition.on_delete.clone(),
            definition.on_update.clone(),
        ),
    };

    let parent_table_def = table_lookup.get(parent_table.as_str()).copied()?;
    Some(build_foreign_key_relation(
        table,
        vec![column.name.to_string()],
        parent_table,
        parent_columns,
        on_delete,
        on_update,
        parent_table_def,
    ))
}

fn parse_reference(reference: &str) -> Option<(String, Vec<String>)> {
    let mut parts = reference.split('.');
    let table = parts.next()?;
    let column = parts.next()?;

    if parts.next().is_some() || table.is_empty() || column.is_empty() {
        return None;
    }

    Some((table.to_string(), vec![column.to_string()]))
}

fn build_foreign_key_relation(
    child_table: &TableDef,
    child_columns: Vec<String>,
    parent_table: String,
    parent_columns: Vec<String>,
    on_delete: Option<ReferenceAction>,
    on_update: Option<ReferenceAction>,
    parent_table_def: &TableDef,
) -> ForeignKeyRelation {
    let cardinality = detect_cardinality(child_table, &child_columns, parent_table_def);
    ForeignKeyRelation {
        child_table: child_table.name.to_string(),
        child_columns,
        parent_table,
        parent_columns,
        on_delete,
        on_update,
        cardinality,
    }
}

fn detect_cardinality(
    child_table: &TableDef,
    child_columns: &[String],
    _parent_table: &TableDef,
) -> Cardinality {
    if is_junction_table(child_table) {
        return Cardinality::ManyToMany;
    }

    if are_columns_unique(child_table, child_columns) {
        return Cardinality::OneToOne;
    }

    if child_columns
        .iter()
        .any(|column| is_nullable_column(child_table, column))
    {
        return Cardinality::ZeroOrOneToMany;
    }

    Cardinality::OneToMany
}

fn is_junction_table(table: &TableDef) -> bool {
    let primary_key_columns = primary_key_columns(table);
    if primary_key_columns.len() < 2 {
        return false;
    }

    let foreign_key_groups = foreign_key_column_groups(table);
    if foreign_key_groups.len() < 2 {
        return false;
    }

    let foreign_key_columns: BTreeSet<&str> = foreign_key_groups
        .iter()
        .flat_map(|group| group.iter().map(String::as_str))
        .collect();

    primary_key_columns
        .iter()
        .all(|column| foreign_key_columns.contains(column.as_str()))
}

fn are_columns_unique(table: &TableDef, columns: &[String]) -> bool {
    if columns.is_empty() {
        return false;
    }

    let primary_key_columns = primary_key_columns(table);
    if !primary_key_columns.is_empty() && same_column_set(columns, &primary_key_columns) {
        return true;
    }

    table.constraints.iter().any(|constraint| {
        matches!(
            constraint,
            TableConstraint::Unique { columns: unique_columns, .. }
                if same_column_set(columns, unique_columns)
        )
    }) || inline_unique_column_groups(table)
        .iter()
        .any(|unique_columns| same_column_set(columns, unique_columns))
}

fn primary_key_columns(table: &TableDef) -> Vec<String> {
    if let Some(columns) = table.constraints.iter().find_map(|constraint| {
        if let TableConstraint::PrimaryKey { columns, .. } = constraint {
            Some(columns.clone())
        } else {
            None
        }
    }) {
        return column_names_to_strings(&columns);
    }

    table
        .columns
        .iter()
        .filter(|column| is_inline_primary_key(column))
        .map(|column| column.name.to_string())
        .collect()
}

fn foreign_key_column_groups(table: &TableDef) -> Vec<Vec<String>> {
    let mut groups: Vec<Vec<String>> = Vec::new();
    for constraint in &table.constraints {
        if let TableConstraint::ForeignKey { columns, .. } = constraint
            && !groups.iter().any(|group| same_column_set(group, columns))
        {
            groups.push(column_names_to_strings(columns));
        }
    }

    for column in &table.columns {
        if column.foreign_key.is_some() {
            let group = vec![column.name.to_string()];
            if !groups
                .iter()
                .any(|existing| same_column_set(existing, &group))
            {
                groups.push(group);
            }
        }
    }

    groups
}

fn inline_unique_column_groups(table: &TableDef) -> Vec<Vec<String>> {
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for column in &table.columns {
        let Some(unique) = &column.unique else {
            continue;
        };

        match unique {
            StrOrBoolOrArray::Str(name) => {
                groups
                    .entry(name.clone())
                    .or_default()
                    .push(column.name.to_string());
            }
            StrOrBoolOrArray::Array(names) => {
                for name in names {
                    groups
                        .entry(name.clone())
                        .or_default()
                        .push(column.name.to_string());
                }
            }
            StrOrBoolOrArray::Bool(true) => {
                groups.insert(
                    format!("__auto_{}", column.name),
                    vec![column.name.to_string()],
                );
            }
            _ => {}
        }
    }
    groups.into_values().collect()
}

fn is_inline_primary_key(column: &ColumnDef) -> bool {
    matches!(
        &column.primary_key,
        Some(PrimaryKeySyntax::Bool(true) | PrimaryKeySyntax::Object(_))
    )
}

fn is_nullable_column(table: &TableDef, column_name: &str) -> bool {
    table
        .columns
        .iter()
        .any(|column| column.name == column_name && column.nullable)
}

fn same_column_set<T: AsRef<str>, U: AsRef<str>>(left: &[T], right: &[U]) -> bool {
    let left: BTreeSet<&str> = left.iter().map(AsRef::as_ref).collect();
    let right: BTreeSet<&str> = right.iter().map(AsRef::as_ref).collect();
    left == right
}

fn column_names_to_strings<T: AsRef<str>>(columns: &[T]) -> Vec<String> {
    columns
        .iter()
        .map(|column| column.as_ref().to_string())
        .collect()
}

fn normalized_filter_names(names: &[String]) -> Vec<String> {
    names
        .iter()
        .filter_map(|name| {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .collect()
}

fn filter_warnings(
    all_names: &BTreeSet<String>,
    flag: &str,
    filter_names: &[String],
) -> Vec<String> {
    let unknown: BTreeSet<&str> = filter_names
        .iter()
        .map(String::as_str)
        .filter(|name| !all_names.contains(*name))
        .collect();

    unknown
        .into_iter()
        .map(|name| format!("warning: ERD {flag} references unknown table '{name}'"))
        .collect()
}

fn build_fk_adjacency(tables: &[TableDef]) -> BTreeMap<String, BTreeSet<String>> {
    let mut adjacency: BTreeMap<String, BTreeSet<String>> = tables
        .iter()
        .map(|table| (table.name.to_string(), BTreeSet::new()))
        .collect();
    let mut junction_parents: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for relation in collect_foreign_key_relations(tables) {
        if let Some(neighbors) = adjacency.get_mut(&relation.child_table) {
            neighbors.insert(relation.parent_table.clone());
        }
        if let Some(neighbors) = adjacency.get_mut(&relation.parent_table) {
            neighbors.insert(relation.child_table.clone());
        }
        if relation.cardinality == Cardinality::ManyToMany {
            junction_parents
                .entry(relation.child_table)
                .or_default()
                .insert(relation.parent_table);
        }
    }

    for parents in junction_parents.values() {
        for parent in parents {
            for peer in parents {
                if parent != peer
                    && let Some(neighbors) = adjacency.get_mut(parent)
                {
                    neighbors.insert(peer.clone());
                }
            }
        }
    }

    adjacency
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use vespertide_core::schema::foreign_key::ForeignKeySyntax;
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;
    use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType, StrOrBoolOrArray, TableDef};

    use super::dot::render_dot;
    use super::mermaid::render_mermaid;
    use super::svg::render_svg;

    fn integer() -> ColumnType {
        ColumnType::Simple(SimpleColumnType::Integer)
    }

    fn text() -> ColumnType {
        ColumnType::Simple(SimpleColumnType::Text)
    }

    fn column(name: &str, column_type: ColumnType) -> ColumnDef {
        ColumnDef::new(name, column_type, false)
    }

    fn primary_key(name: &str, column_type: ColumnType) -> ColumnDef {
        column(name, column_type).primary_key(PrimaryKeySyntax::Bool(true))
    }

    fn foreign_key(name: &str, reference: &str) -> ColumnDef {
        column(name, integer()).foreign_key(ForeignKeySyntax::String(reference.to_string()))
    }

    fn nullable_foreign_key(name: &str, reference: &str) -> ColumnDef {
        ColumnDef::new(name, integer(), true)
            .foreign_key(ForeignKeySyntax::String(reference.into()))
    }

    fn unique_foreign_key(name: &str, reference: &str) -> ColumnDef {
        foreign_key(name, reference).unique(StrOrBoolOrArray::Bool(true))
    }

    fn table(name: &str, columns: Vec<ColumnDef>) -> TableDef {
        TableDef {
            name: name.into(),
            description: None,
            columns,
            constraints: Vec::new(),
        }
    }

    fn normalize(table: &TableDef) -> TableDef {
        table.normalize().unwrap()
    }

    fn simple_schema() -> Vec<TableDef> {
        vec![
            normalize(&table(
                "user",
                vec![
                    primary_key("id", integer()),
                    column("email", text()),
                    column("name", text()),
                ],
            )),
            normalize(&table(
                "article",
                vec![
                    primary_key("id", integer()),
                    foreign_key("author_id", "user.id"),
                    column("title", text()),
                ],
            )),
        ]
    }

    fn cardinality_schema() -> Vec<TableDef> {
        vec![
            normalize(&table("user", vec![primary_key("id", integer())])),
            normalize(&table("tag", vec![primary_key("id", integer())])),
            normalize(&table(
                "article",
                vec![
                    primary_key("id", integer()),
                    foreign_key("author_id", "user.id"),
                ],
            )),
            normalize(&table(
                "profile",
                vec![
                    primary_key("id", integer()),
                    unique_foreign_key("user_id", "user.id"),
                ],
            )),
            normalize(&table(
                "photo",
                vec![
                    primary_key("id", integer()),
                    nullable_foreign_key("user_id", "user.id"),
                ],
            )),
            normalize(&table(
                "user_tag",
                vec![
                    primary_key("user_id", integer())
                        .foreign_key(ForeignKeySyntax::String("user.id".into())),
                    primary_key("tag_id", integer())
                        .foreign_key(ForeignKeySyntax::String("tag.id".into())),
                ],
            )),
        ]
    }

    fn filter_schema() -> Vec<TableDef> {
        vec![
            normalize(&table("user", vec![primary_key("id", integer())])),
            normalize(&table(
                "media",
                vec![
                    primary_key("id", integer()),
                    foreign_key("owner_id", "user.id"),
                ],
            )),
            normalize(&table(
                "article",
                vec![
                    primary_key("id", integer()),
                    foreign_key("media_id", "media.id"),
                ],
            )),
            normalize(&table(
                "article_user",
                vec![
                    primary_key("article_id", integer())
                        .foreign_key(ForeignKeySyntax::String("article.id".into())),
                    primary_key("user_id", integer())
                        .foreign_key(ForeignKeySyntax::String("user.id".into())),
                ],
            )),
            normalize(&table(
                "comment",
                vec![
                    primary_key("id", integer()),
                    foreign_key("article_id", "article.id"),
                ],
            )),
        ]
    }

    fn table_names(tables: &[TableDef]) -> Vec<&str> {
        tables.iter().map(|table| table.name.as_str()).collect()
    }

    fn only_include(names: &[&str]) -> Vec<String> {
        names.iter().map(ToString::to_string).collect()
    }

    fn relation_cardinality(schema: &[TableDef], child_table: &str) -> Cardinality {
        collect_foreign_key_relations(schema)
            .into_iter()
            .find(|relation| relation.child_table == child_table)
            .expect("relation exists")
            .cardinality
    }

    #[test]
    fn test_render_mermaid_simple_two_tables() {
        assert_snapshot!(render_mermaid(&simple_schema()), @r###"
    erDiagram
      user {
        int id PK
        string email
        string name
      }
      article {
        int id PK
        int author_id FK
        string title
      }
      user ||--o{ article : "author_id"
    "###);
    }

    #[test]
    fn test_render_dot_simple_two_tables() {
        assert_snapshot!(render_dot(&simple_schema()), @r###"
    digraph {
      rankdir=LR;
      bgcolor="transparent";
      node [shape=record, fontname="Helvetica"];
      edge [fontname="Helvetica"];
      user [shape=record, label="{user|id: integer (PK)|email: text|name: text}"];
      article [shape=record, label="{article|id: integer (PK)|author_id: integer (FK)|title: text}"];
      article -> user [label="1:N: author_id -> id"];
    }
    "###);
    }

    #[test]
    fn test_render_svg_produces_valid_svg() {
        let svg = render_svg(&simple_schema()).unwrap();

        assert!(svg.contains("<svg"));
        assert!(svg.contains("user"));
        assert!(svg.contains("article"));
    }

    #[test]
    fn test_render_mermaid_with_inline_fk_without_normalize() {
        let schema = vec![
            table("user", vec![primary_key("id", integer())]),
            table(
                "article",
                vec![
                    primary_key("id", integer()),
                    foreign_key("author_id", "user.id"),
                ],
            ),
        ];

        assert_snapshot!(render_mermaid(&schema), @r###"
    erDiagram
      user {
        int id PK
      }
      article {
        int id PK
        int author_id FK
      }
      user ||--o{ article : "author_id"
    "###);
    }

    #[test]
    fn test_cardinality_one_to_many_default() {
        assert_eq!(
            relation_cardinality(&simple_schema(), "article"),
            Cardinality::OneToMany
        );
    }

    #[test]
    fn test_cardinality_one_to_one_unique_fk() {
        assert_eq!(
            relation_cardinality(&cardinality_schema(), "profile"),
            Cardinality::OneToOne
        );
    }

    #[test]
    fn test_cardinality_zero_or_one_to_many_nullable() {
        assert_eq!(
            relation_cardinality(&cardinality_schema(), "photo"),
            Cardinality::ZeroOrOneToMany
        );
    }

    #[test]
    fn test_cardinality_many_to_many_junction() {
        assert_eq!(
            relation_cardinality(&cardinality_schema(), "user_tag"),
            Cardinality::ManyToMany
        );
    }

    #[test]
    fn test_filter_include_only() {
        let (filtered, warnings) =
            filter_tables_with_warnings(filter_schema(), &only_include(&["user"]), &[], 0);

        assert!(warnings.is_empty());
        assert_eq!(table_names(&filtered), vec!["user"]);
    }

    #[test]
    fn test_filter_include_with_depth_1() {
        let (filtered, warnings) =
            filter_tables_with_warnings(filter_schema(), &only_include(&["user"]), &[], 1);

        assert!(warnings.is_empty());
        assert_eq!(
            table_names(&filtered),
            vec!["user", "media", "article", "article_user"]
        );
    }

    #[test]
    fn test_filter_include_with_depth_2() {
        let (filtered, warnings) =
            filter_tables_with_warnings(filter_schema(), &only_include(&["user"]), &[], 2);

        assert!(warnings.is_empty());
        assert_eq!(
            table_names(&filtered),
            vec!["user", "media", "article", "article_user", "comment"]
        );
    }

    #[test]
    fn test_filter_exclude() {
        let (filtered, warnings) =
            filter_tables_with_warnings(filter_schema(), &[], &only_include(&["article_user"]), 0);

        assert!(warnings.is_empty());
        assert_eq!(
            table_names(&filtered),
            vec!["user", "media", "article", "comment"]
        );
    }

    #[test]
    fn test_filter_include_exclude_combined() {
        let (filtered, warnings) = filter_tables_with_warnings(
            filter_schema(),
            &only_include(&["user"]),
            &only_include(&["article"]),
            1,
        );

        assert!(warnings.is_empty());
        assert_eq!(
            table_names(&filtered),
            vec!["user", "media", "article_user"]
        );
    }

    #[test]
    fn test_filter_unknown_table_warns() {
        let (filtered, warnings) =
            filter_tables_with_warnings(filter_schema(), &only_include(&["ghost"]), &[], 0);

        assert!(filtered.is_empty());
        assert_eq!(
            warnings,
            vec!["warning: ERD --include references unknown table 'ghost'"]
        );
    }

    #[test]
    fn test_render_mermaid_cardinality_snapshot() {
        assert_snapshot!(render_mermaid(&cardinality_schema()), @r###"
    erDiagram
      user {
        int id PK
      }
      tag {
        int id PK
      }
      article {
        int id PK
        int author_id FK
      }
      profile {
        int id PK
        int user_id FK
      }
      photo {
        int id PK
        int user_id FK
      }
      user_tag {
        int user_id PK FK
        int tag_id PK FK
      }
      user ||--o{ article : "author_id"
      user |o--o{ photo : "user_id"
      user ||--|| profile : "user_id"
      user_tag }o--|| tag : "tag_id"
      user_tag }o--|| user : "user_id"
    "###);
    }

    #[test]
    fn test_render_dot_cardinality_snapshot() {
        assert_snapshot!(render_dot(&cardinality_schema()), @r###"
    digraph {
      rankdir=LR;
      bgcolor="transparent";
      node [shape=record, fontname="Helvetica"];
      edge [fontname="Helvetica"];
      user [shape=record, label="{user|id: integer (PK)}"];
      tag [shape=record, label="{tag|id: integer (PK)}"];
      article [shape=record, label="{article|id: integer (PK)|author_id: integer (FK)}"];
      profile [shape=record, label="{profile|id: integer (PK)|user_id: integer (FK)}"];
      photo [shape=record, label="{photo|id: integer (PK)|user_id: integer (FK)}"];
      user_tag [shape=record, label="{user_tag|user_id: integer (PK, FK)|tag_id: integer (PK, FK)}"];
      article -> user [label="1:N: author_id -> id"];
      photo -> user [label="0..1:N: user_id -> id"];
      profile -> user [label="1:1: user_id -> id"];
      user_tag -> tag [label="M:N: tag_id -> id"];
      user_tag -> user [label="M:N: user_id -> id"];
    }
    "###);
    }

    #[test]
    fn test_render_svg_cardinality_snapshot() {
        assert_snapshot!(svg_cardinality_labels(&cardinality_schema()), @r###"
    1:N
    0..1:N
    1:1
    M:N
    M:N"###);
    }

    #[test]
    fn test_render_empty_schema() {
        assert_snapshot!(render_mermaid(&[]), @r###"
    erDiagram
    "###);

        assert_snapshot!(render_dot(&[]), @r###"
    digraph {
      rankdir=LR;
      bgcolor="transparent";
      node [shape=record, fontname="Helvetica"];
      edge [fontname="Helvetica"];
    }
    "###);
    }

    #[test]
    fn test_render_with_composite_pk() {
        let schema = vec![
            normalize(&table("user", vec![primary_key("id", integer())])),
            normalize(&table("role", vec![primary_key("id", integer())])),
            normalize(&table(
                "user_role",
                vec![
                    primary_key("user_id", integer())
                        .foreign_key(ForeignKeySyntax::String("user.id".into())),
                    primary_key("role_id", integer())
                        .foreign_key(ForeignKeySyntax::String("role.id".into())),
                ],
            )),
        ];

        assert_snapshot!(render_mermaid(&schema), @r###"
    erDiagram
      user {
        int id PK
      }
      role {
        int id PK
      }
      user_role {
        int user_id PK FK
        int role_id PK FK
      }
      user_role }o--|| role : "role_id"
      user_role }o--|| user : "user_id"
    "###);
    }

    fn svg_cardinality_labels(schema: &[TableDef]) -> String {
        render_svg(schema)
            .unwrap()
            .lines()
            .filter_map(|line| {
                if !line.contains("edge-cardinality") {
                    return None;
                }
                let start = line.find('>')? + 1;
                let end = line.find("</text>")?;
                Some(line[start..end].to_string())
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
