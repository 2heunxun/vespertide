use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use vespertide_core::schema::primary_key::PrimaryKeySyntax;
use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType, TableConstraint, TableDef};
use vespertide_planner::diff_schemas;

fn simple_type(ty: SimpleColumnType) -> ColumnType {
    ColumnType::Simple(ty)
}

fn build_schema(n: usize) -> Vec<TableDef> {
    (0..n)
        .map(|i| TableDef {
            name: format!("table_{i}"),
            description: None,
            columns: vec![
                ColumnDef::new("id", simple_type(SimpleColumnType::Integer), false)
                    .primary_key(PrimaryKeySyntax::Bool(true)),
                ColumnDef::new("name", simple_type(SimpleColumnType::Text), false),
                ColumnDef::new(
                    "created_at",
                    simple_type(SimpleColumnType::Timestamp),
                    false,
                ),
            ],
            constraints: vec![],
        })
        .collect()
}

fn build_table_with_constraints(n: usize) -> Vec<TableDef> {
    let columns = std::iter::once(
        ColumnDef::new("id", simple_type(SimpleColumnType::Integer), false)
            .primary_key(PrimaryKeySyntax::Bool(true)),
    )
    .chain((0..n).map(|i| {
        ColumnDef::new(
            format!("constrained_col_{i}"),
            simple_type(SimpleColumnType::Integer),
            false,
        )
    }))
    .collect();

    let constraints = (0..n)
        .map(|i| TableConstraint::Unique {
            name: Some(format!("uq_constraints__old_{i}")),
            columns: vec![format!("constrained_col_{i}")],
        })
        .collect();

    vec![TableDef {
        name: "constraint_heavy".into(),
        description: None,
        columns,
        constraints,
    }]
}

fn rename_all_constraints(from: &[TableDef]) -> Vec<TableDef> {
    let mut to = from.to_vec();
    for (i, constraint) in to[0].constraints.iter_mut().enumerate() {
        if let TableConstraint::Unique { name, .. } = constraint {
            *name = Some(format!("uq_constraints__new_{i}"));
        }
    }
    to
}

fn bench_diff_identity(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_identity");
    for size in [10, 100, 1000] {
        let schema = build_schema(size);
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| diff_schemas(black_box(&schema), black_box(&schema)));
        });
    }
    group.finish();
}

fn bench_diff_add_column(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_add_column");
    for size in [10, 100, 1000] {
        let from = build_schema(size);
        let mut to = from.clone();
        for table in &mut to {
            table.columns.push(ColumnDef::new(
                "new_col",
                simple_type(SimpleColumnType::Text),
                true,
            ));
        }
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| diff_schemas(black_box(&from), black_box(&to)));
        });
    }
    group.finish();
}

fn bench_diff_constraint_replacement(c: &mut Criterion) {
    let from = build_table_with_constraints(100);
    let to = rename_all_constraints(&from);
    c.bench_function("diff_constraint_replacement_100", |b| {
        b.iter(|| diff_schemas(black_box(&from), black_box(&to)));
    });
}

criterion_group!(
    benches,
    bench_diff_identity,
    bench_diff_add_column,
    bench_diff_constraint_replacement
);
criterion_main!(benches);
