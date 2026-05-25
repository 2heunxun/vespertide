use vespertide_core::schema::primary_key::PrimaryKeySyntax;
use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType, StrOrBoolOrArray, TableDef};

#[test]
fn sqlmodel_schema_rendering_is_byte_equal_across_thread_counts() {
    let schema = large_schema();

    let one_thread = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .expect("build one-thread pool")
        .install(|| vespertide_exporter::sqlmodel::render_entities(&schema))
        .expect("render with one thread");

    let four_threads = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .expect("build four-thread pool")
        .install(|| vespertide_exporter::sqlmodel::render_entities(&schema))
        .expect("render with four threads");

    assert_eq!(one_thread.as_bytes(), four_threads.as_bytes());
}

fn large_schema() -> Vec<TableDef> {
    (0..60)
        .map(|idx| TableDef {
            name: format!("table_{idx}").into(),
            description: Some(format!("Synthetic table {idx}")),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: Some(PrimaryKeySyntax::Bool(true)),
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "created_at".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Timestamptz),
                    nullable: false,
                    default: Some("NOW()".into()),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "external_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: Some(StrOrBoolOrArray::Bool(true)),
                    foreign_key: None,
                },
            ],
            constraints: Vec::new(),
        })
        .map(|table| table.normalize())
        .collect::<Result<Vec<_>, _>>()
        .expect("normalize schema")
}
