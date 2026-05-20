use insta::assert_snapshot;
use vespertide_core::schema::foreign_key::ForeignKeySyntax;
use vespertide_core::schema::primary_key::PrimaryKeySyntax;
use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType, TableDef};

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

fn table(name: &str, columns: Vec<ColumnDef>) -> TableDef {
    TableDef {
        name: name.to_string(),
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
  article -> user [label="author_id -> id"];
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
  role ||--o{ user_role : "role_id"
  user ||--o{ user_role : "user_id"
"###);
}
