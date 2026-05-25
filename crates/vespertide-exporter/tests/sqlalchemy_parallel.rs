use vespertide_core::schema::primary_key::PrimaryKeySyntax;
use vespertide_core::{ColumnDef, ColumnType, ComplexColumnType, SimpleColumnType, TableDef};

#[test]
fn sqlalchemy_export_byte_identical() {
    let schema = make_100_table_schema();

    let a = vespertide_exporter::sqlalchemy::export(&schema).expect("first export succeeds");
    let b = vespertide_exporter::sqlalchemy::export(&schema).expect("second export succeeds");

    assert_eq!(a, b);
}

fn make_100_table_schema() -> Vec<TableDef> {
    (0..100)
        .map(|i| {
            TableDef {
                name: format!("table_{i}").into(),
                description: None,
                columns: vec![
                    ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false)
                        .primary_key(PrimaryKeySyntax::Bool(true)),
                    ColumnDef::new(
                        "name",
                        ColumnType::Complex(ComplexColumnType::Varchar { length: 191 }),
                        false,
                    ),
                    ColumnDef::new(
                        "created_at",
                        ColumnType::Simple(SimpleColumnType::Timestamptz),
                        false,
                    )
                    .default("NOW()".into()),
                    ColumnDef::new("metadata", ColumnType::Simple(SimpleColumnType::Json), true),
                ],
                constraints: Vec::new(),
            }
            .normalize()
            .expect("generated table normalizes")
        })
        .collect()
}
