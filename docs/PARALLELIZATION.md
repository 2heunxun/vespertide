# Vespertide Parallelization Report

Date: 2026-05-20
Branch: refactor
Commit: 9730c3ad5216d4c0220cc5bbf356dbc9f3c0c432
Wave: 6 adaptive threshold retuning complete

## Summary

- 12 hot paths parallelized and retuned with per-path thresholds
- Determinism: 521 insta snapshots + 12000+ `codegen_determinism` assertions per run, all green
- Wave 6 thread matrix: `RAYON_NUM_THREADS={1,4}` all green
- SQLModel `try_reduce` ordering risk removed: entity rendering now parallel-collects in Rayon indexed order, then sequentially accumulates output/import state

## Tasks Completed

| Wave | Task | Crate | Function | Threshold | with_min_len |
|------|------|-------|----------|-----------|--------------|
| 1 | T2 | vespertide-cli | export | 50 | 32 |
| 1 | T3 | vespertide-loader | 4 functions | 20 | 4 |
| 1 | T4 | vespertide-planner | validate_migration_plan | 10000 | 32 |
| 2 | T5 | vespertide-planner | diff_schemas (per-table) | 10000 | 16 |
| 2 | T6 | vespertide-planner | validate_schema | 10000 | 16 |
| 3 | T7 | vespertide-exporter | seaorm::export | 50 | 8 |
| 3 | T8 | vespertide-exporter | sqlalchemy::export | 50 | 8 |
| 3 | T9 | vespertide-exporter | sqlmodel::render_entities | 50 | 8 |
| 3 | T10 | vespertide-exporter | jpa::render_entities | 50 | 8 |
| 4 | T11 | vespertide-exporter | seaorm::relations | 50 | 8 |
| 4 | T12 | vespertide-query | build_plan_queries | 10000 | 8 |

## Wave 6 Retuned Thresholds

| Hot path | Threshold | Break-even (measured) | Large-N result |
|---|---:|---:|---:|
| planner `diff_schemas` | 10000 tables | identity ~5600 tables; add-column ~3700 tables | 1.76x @ identity/10000; 1.62x @ add-column/10000 |
| planner `validate_schema` | 10000 tables | not reached through 1000 tables | no win observed in sweep; small/medium schemas now sequential |
| planner `validate_migration_plan` | 10000 actions | not reached through 1000 actions | no win observed in sweep; ordinary revisions now sequential |
| query `build_plan_queries` | 10000 actions | not reached through 1000 actions | no win observed in sweep; ordinary plans now sequential |
| exporter `seaorm::export` | 50 tables | <50 tables | 9.8x @ 200 tables × 50 cols |
| exporter `sqlalchemy::export` | 50 tables | <50 tables | 4.1x @ 200 tables × 50 cols |
| exporter `sqlmodel::render_entities` | 50 tables | <50 tables | 4.3x @ 200 tables × 50 cols |
| exporter `jpa::render_entities` | 50 tables | <50 tables | 7.7x @ 200 tables × 50 cols |
| exporter SeaORM relation resolution | 50 schema tables | covered by SeaORM export sweep | retained at 50 |
| loader model/migration loading | 20 files | IO-bound; not retuned | retained at 20 |
| CLI export render fan-out | 50 tables | only 4 ORM variants; not retuned | retained at 50 |

## Methodology

1. Measured current parallel thresholds against a forced-sequential build using the same generated schemas/actions.
2. Swept planner `diff_schemas` at N={50,100,200,500,1000,2000,5000,10000}; validation and query at N={50,100,500,1000}; exporter schema renders at table counts {50,100,200,500} with 50 columns/table; SQLAlchemy single-entity render at column counts {10,50,100,200,500}.
3. Chose threshold = break-even × safety buffer where break-even was in range. When break-even was above the measured range, used the Wave 6 rule: threshold 10000, preserving the parallel path only for very large workloads.
4. Re-ran targeted Criterion checks for the five originally regressed benchmarks after retuning.

## Retuned Criterion Comparison

Original before/after numbers are the Wave 5 Criterion estimates. Retuned numbers are Wave 6 targeted Criterion estimates after central thresholds and the allocation-free sequential `diff_schemas` path.

| Benchmark | Before Rayon | Wave 5 After | Wave 6 Retuned | Retuned vs Before |
|-----------|-------------:|-------------:|----------------:|------------------:|
| planner `diff_identity/100` | 134.54 µs | 206.04 µs | 138.35 µs | 0.97x |
| planner `diff_identity/1000` | 2.058 ms | 2.252 ms | 1.583 ms | 1.30x |
| planner `diff_add_column/100` | 205.26 µs | 321.02 µs | 157.06 µs | 1.31x |
| planner `diff_add_column/1000` | 2.498 ms | 2.900 ms | 1.970 ms | 1.27x |
| exporter SQLAlchemy `render_entity/cols=50/fk=true/enum=false` | 15.89 µs | 20.37 µs | 15.68 µs | 1.01x |

### Benchmark Coverage Notes

- Planner Criterion benchmarks include table counts 10, 100, and 1000; Wave 6 used a temporary threshold probe for 50/200/500/2000/5000/10000 break-even sweeps.
- Exporter Criterion benchmarks cover single-entity rendering by ORM/column count. Wave 6 used the threshold probe for schema-level export break-even sweeps.
- SQLModel and JPA render benchmarks do not have `before-rayon` baselines in `target/criterion`, so no before/after speedup can be reported from the saved baseline.
- Query benchmarks cover create-table and representative single-action SQL emission. Wave 6 used the threshold probe for `build_plan_queries` action-count sweeps.
- No existing loader criterion benchmark was present, so loader speedup could not be measured in this run.
- Several parallelized paths are intentionally thresholded; current microbenchmarks often remain below or adjacent to threshold/overhead boundaries, so not every Criterion case shows a speedup.

## Determinism Verification

- `codegen_determinism` proptest: 1000 cases × 4 ORMs × 3 repeats = 12000 byte-equality assertions, all PASS.
- SQLModel 200-table regression: 100 repeated byte-equality checks comparing dedicated Rayon pools with 1 and 8 threads, all PASS.
- Wave 6 thread matrix `{1,4}`: all PASS.
- Insta snapshots (521): 0 drift.

## Thread Matrix Results

| RAYON_NUM_THREADS | Command | Result |
|-------------------|---------|--------|
| 1 | `cargo test --workspace --all-features --exclude vespertide-fuzz` | PASS |
| 4 | `cargo test --workspace --all-features --exclude vespertide-fuzz` | PASS |

## Sequential-By-Nature (NOT parallelized)

- `diff/ordering.rs::topological_sort_tables` — Kahn algorithm requires in-degree evolution
- `validate/duplicate_table_detection` — earliest-error ordering
- `vespertide-macro` — proc-macro context forbids rayon (use `std::thread::scope` if needed later)
- SQLite temp-table rebuild — sequential within each action; per-action parallel ok
- `seaorm::relations` aggregation phase — `entity_count`, `fk_by_table`, used relation enum tracking

## Files Changed

- `crates/vespertide-exporter/src/sqlmodel/render.rs` — removed order-sensitive `try_reduce` entity accumulation
- `crates/vespertide-exporter/tests/codegen_determinism.rs` — raised proptest cases to 1000 and added SQLModel thread-pool regression
- `.github/workflows/CI.yml` — added `test-parallelism` matrix for Rayon thread counts 1 and 4
- `crates/*/src/parallel_config.rs` — centralized empirically tuned thresholds
- `crates/vespertide-planner/src/diff/mod.rs` — restored allocation-free sequential path below threshold
- `docs/PARALLELIZATION.md` — Wave 6 retuning methodology and benchmark report
