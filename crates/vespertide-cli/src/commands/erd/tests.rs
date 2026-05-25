use insta::assert_snapshot;
use vespertide_core::schema::foreign_key::ForeignKeySyntax;
use vespertide_core::schema::primary_key::PrimaryKeySyntax;
use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType, StrOrBoolOrArray, TableDef};

use super::dot::render_dot;
use super::mermaid::render_mermaid;
use super::svg::render_svg;
use super::{Cardinality, collect_foreign_key_relations, filter_tables_with_warnings};

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
    ColumnDef::new(name, integer(), true).foreign_key(ForeignKeySyntax::String(reference.into()))
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
