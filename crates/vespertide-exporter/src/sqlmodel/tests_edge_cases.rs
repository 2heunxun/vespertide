use super::render_entity;
use insta::assert_snapshot;
use vespertide_core::schema::foreign_key::{ForeignKeyDef, ForeignKeySyntax};
use vespertide_core::schema::primary_key::PrimaryKeySyntax;
use vespertide_core::{
    ColumnDef, ColumnType, ComplexColumnType, EnumValues, NumValue, ReferenceAction,
    SimpleColumnType, StrOrBoolOrArray, TableDef,
};

fn simple_type(name: &str, ty: SimpleColumnType, nullable: bool) -> ColumnDef {
    ColumnDef::new(name, ColumnType::Simple(ty), nullable)
}

fn integer_enum_column() -> ColumnDef {
    ColumnDef::new(
        "state",
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "edge_state".into(),
            values: EnumValues::Integer(vec![
                NumValue {
                    name: "unknown".into(),
                    value: -1,
                },
                NumValue {
                    name: "not_started".into(),
                    value: 0,
                },
                NumValue {
                    name: "InProgress".into(),
                    value: 10,
                },
                NumValue {
                    name: "HTTP_500".into(),
                    value: 500,
                },
            ]),
        }),
        false,
    )
}

fn render_snapshot(table: &TableDef) -> String {
    let rendered = render_entity(&table.normalize().unwrap()).unwrap();
    normalize_datetime_import(&rendered)
}

fn normalize_datetime_import(rendered: &str) -> String {
    rendered
        .lines()
        .map(|line| {
            if line.starts_with("from datetime import ") {
                "from datetime import date, datetime, time".to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn snapshot_self_referencing_fk() {
    let table = TableDef {
        name: "employees".into(),
        description: None,
        columns: vec![
            simple_type("id", SimpleColumnType::Integer, false)
                .primary_key(PrimaryKeySyntax::Bool(true)),
            simple_type("manager_id", SimpleColumnType::Integer, true).foreign_key(
                ForeignKeySyntax::Object(ForeignKeyDef {
                    ref_table: "employees".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: Some(ReferenceAction::SetNull),
                    on_update: None,
                }),
            ),
        ],
        constraints: vec![],
    };

    let rendered = render_snapshot(&table);
    assert_snapshot!(rendered);
}

#[test]
fn snapshot_all_simple_types() {
    let table = TableDef {
        name: "type_matrix".into(),
        description: None,
        columns: vec![
            simple_type("c_int", SimpleColumnType::Integer, false),
            simple_type("c_bigint", SimpleColumnType::BigInt, false),
            simple_type("c_smallint", SimpleColumnType::SmallInt, false),
            simple_type("c_text", SimpleColumnType::Text, true),
            simple_type("c_bool", SimpleColumnType::Boolean, false),
            simple_type("c_real", SimpleColumnType::Real, false),
            simple_type("c_double", SimpleColumnType::DoublePrecision, false),
            simple_type("c_date", SimpleColumnType::Date, true),
            simple_type("c_time", SimpleColumnType::Time, true),
            simple_type("c_ts", SimpleColumnType::Timestamp, true),
            simple_type("c_tstz", SimpleColumnType::Timestamptz, true),
            simple_type("c_interval", SimpleColumnType::Interval, true),
            simple_type("c_uuid", SimpleColumnType::Uuid, false),
            simple_type("c_json", SimpleColumnType::Json, true),
            simple_type("c_bytea", SimpleColumnType::Bytea, true),
            simple_type("c_inet", SimpleColumnType::Inet, true),
            simple_type("c_cidr", SimpleColumnType::Cidr, true),
            simple_type("c_macaddr", SimpleColumnType::Macaddr, true),
            simple_type("c_xml", SimpleColumnType::Xml, true),
        ],
        constraints: vec![],
    };

    let rendered = render_snapshot(&table);
    assert_snapshot!(rendered);
}

#[test]
fn snapshot_reserved_word_identifiers() {
    let table = TableDef {
        name: "order".into(),
        description: None,
        columns: vec![
            simple_type("id", SimpleColumnType::Integer, false)
                .primary_key(PrimaryKeySyntax::Bool(true)),
            simple_type("user", SimpleColumnType::Text, false),
            simple_type("select", SimpleColumnType::Integer, false),
        ],
        constraints: vec![],
    };

    let rendered = render_snapshot(&table);
    assert_snapshot!(rendered);
}

#[test]
fn snapshot_composite_primary_key() {
    let table = TableDef {
        name: "membership".into(),
        description: None,
        columns: vec![
            simple_type("tenant_id", SimpleColumnType::Integer, false)
                .primary_key(PrimaryKeySyntax::Bool(true)),
            simple_type("user_id", SimpleColumnType::Integer, false)
                .primary_key(PrimaryKeySyntax::Bool(true)),
            simple_type("role", SimpleColumnType::Text, false),
        ],
        constraints: vec![],
    };

    let rendered = render_snapshot(&table);
    assert_snapshot!(rendered);
}

#[test]
fn snapshot_composite_unique_constraint() {
    let table = TableDef {
        name: "account_aliases".into(),
        description: None,
        columns: vec![
            simple_type("id", SimpleColumnType::Integer, false)
                .primary_key(PrimaryKeySyntax::Bool(true)),
            simple_type("tenant_id", SimpleColumnType::Integer, false).unique(
                StrOrBoolOrArray::Array(vec!["uq_account_aliases__tenant_slug".into()]),
            ),
            simple_type("slug", SimpleColumnType::Text, false).unique(StrOrBoolOrArray::Array(
                vec!["uq_account_aliases__tenant_slug".into()],
            )),
        ],
        constraints: vec![],
    };

    let rendered = render_snapshot(&table);
    assert_snapshot!(rendered);
}

#[test]
fn snapshot_integer_enum_all_variant_types() {
    let table = TableDef {
        name: "workflow_runs".into(),
        description: None,
        columns: vec![
            simple_type("id", SimpleColumnType::Integer, false)
                .primary_key(PrimaryKeySyntax::Bool(true)),
            integer_enum_column(),
        ],
        constraints: vec![],
    };

    let rendered = render_snapshot(&table);
    assert_snapshot!(rendered);
}
