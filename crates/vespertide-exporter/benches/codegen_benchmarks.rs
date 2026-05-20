use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use vespertide_core::schema::foreign_key::{ForeignKeyDef, ForeignKeySyntax};
use vespertide_core::schema::primary_key::PrimaryKeySyntax;
use vespertide_core::{
    ColumnDef, ColumnType, ComplexColumnType, EnumValues, ReferenceAction, SimpleColumnType,
    TableDef,
};
use vespertide_exporter::{Orm, render_entity_with_schema};

fn simple_type(ty: SimpleColumnType) -> ColumnType {
    ColumnType::Simple(ty)
}

fn enum_type() -> ColumnType {
    ColumnType::Complex(ComplexColumnType::Enum {
        name: "record_status".to_string(),
        values: EnumValues::from(vec!["draft", "active", "archived"]),
    })
}

fn user_table() -> TableDef {
    TableDef {
        name: "user".into(),
        description: None,
        columns: vec![
            ColumnDef::new("id", simple_type(SimpleColumnType::Integer), false)
                .primary_key(PrimaryKeySyntax::Bool(true)),
            ColumnDef::new("email", simple_type(SimpleColumnType::Text), false),
        ],
        constraints: vec![],
    }
    .normalize()
    .expect("valid user table")
}

fn build_table(n_columns: usize, with_fk: bool, with_enum: bool) -> TableDef {
    let mut columns = vec![
        ColumnDef::new("id", simple_type(SimpleColumnType::Integer), false)
            .primary_key(PrimaryKeySyntax::Bool(true)),
    ];
    if with_fk {
        columns.push(
            ColumnDef::new("user_id", simple_type(SimpleColumnType::Integer), false).foreign_key(
                ForeignKeySyntax::Object(ForeignKeyDef {
                    ref_table: "user".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: Some(ReferenceAction::Cascade),
                    on_update: None,
                }),
            ),
        );
    }
    if with_enum {
        columns.push(ColumnDef::new("status", enum_type(), false));
    }
    while columns.len() < n_columns {
        let i = columns.len();
        let ty = match i % 5 {
            0 => simple_type(SimpleColumnType::Integer),
            1 => simple_type(SimpleColumnType::Text),
            2 => simple_type(SimpleColumnType::Boolean),
            3 => simple_type(SimpleColumnType::Timestamptz),
            _ => ColumnType::Complex(ComplexColumnType::Varchar { length: 191 }),
        };
        columns.push(ColumnDef::new(format!("field_{i}"), ty, i % 7 == 0));
    }

    TableDef {
        name: format!("entity_{n_columns}_{with_fk}_{with_enum}"),
        description: None,
        columns,
        constraints: vec![],
    }
    .normalize()
    .expect("valid generated benchmark table")
}

fn bench_render_entity(c: &mut Criterion) {
    let mut group = c.benchmark_group("render_entity");
    let parent = user_table();

    for orm in [Orm::SeaOrm, Orm::SqlAlchemy, Orm::SqlModel, Orm::Jpa] {
        for n_columns in [10, 50, 200] {
            for (with_fk, with_enum) in [(false, false), (true, false), (false, true), (true, true)]
            {
                let table = build_table(n_columns, with_fk, with_enum);
                let schema = vec![parent.clone(), table.clone()];
                let case = format!("{orm:?}/cols={n_columns}/fk={with_fk}/enum={with_enum}");
                group.bench_with_input(BenchmarkId::from_parameter(case), &orm, |b, orm| {
                    b.iter(|| {
                        black_box(
                            render_entity_with_schema(
                                black_box(*orm),
                                black_box(&table),
                                black_box(&schema),
                            )
                            .expect("code generation should succeed"),
                        )
                    });
                });
            }
        }
    }
    group.finish();
}

criterion_group!(benches, bench_render_entity);
criterion_main!(benches);
