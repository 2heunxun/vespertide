use rayon::ThreadPoolBuilder;
use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType, TableDef};

#[test]
fn seaorm_export_byte_identical_across_thread_counts() {
    let schema = hundred_table_fixture();

    let single_thread = ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .expect("build single-thread rayon pool")
        .install(|| vespertide_exporter::seaorm::export(&schema))
        .expect("single-thread SeaORM export");

    let four_threads = ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .expect("build four-thread rayon pool")
        .install(|| vespertide_exporter::seaorm::export(&schema))
        .expect("four-thread SeaORM export");

    assert_eq!(single_thread, four_threads);
}

fn hundred_table_fixture() -> Vec<TableDef> {
    (0..100)
        .map(|idx| TableDef {
            name: format!("table_{idx}").into(),
            description: None,
            columns: vec![
                ColumnDef {
                    name: "id".into(),
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
                    name: "name".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: Vec::new(),
        })
        .collect()
}
