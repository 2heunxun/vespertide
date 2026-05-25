use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType, TableDef};

#[test]
fn jpa_parallel_render_matches_sequential_order_and_content() {
    let schema = wide_schema(64);

    let sequential = schema
        .iter()
        .map(vespertide_exporter::jpa::render_entity)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    let parallel = vespertide_exporter::jpa::render_entities(&schema).unwrap();

    assert_eq!(parallel, sequential);
}

fn wide_schema(count: usize) -> Vec<TableDef> {
    (0..count)
        .map(|idx| TableDef {
            name: format!("parallel_jpa_table_{idx}").into(),
            description: None,
            columns: vec![
                column("id", ColumnType::Simple(SimpleColumnType::Integer), false),
                column(
                    "created_on",
                    ColumnType::Simple(SimpleColumnType::Date),
                    false,
                ),
                column(
                    "created_at",
                    ColumnType::Simple(SimpleColumnType::Timestamptz),
                    false,
                ),
                column(
                    "external_id",
                    ColumnType::Simple(SimpleColumnType::Uuid),
                    false,
                ),
            ],
            constraints: Vec::new(),
        })
        .collect()
}

fn column(name: &str, r#type: ColumnType, nullable: bool) -> ColumnDef {
    ColumnDef {
        name: name.into(),
        r#type,
        nullable,
        default: None,
        comment: None,
        primary_key: None,
        unique: None,
        index: None,
        foreign_key: None,
    }
}
