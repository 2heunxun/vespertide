# Performance Audit (post Wave 0-9)

## Methodology

- Baseline Criterion was saved with crate-level bench targets because `cargo bench --workspace -- --save-baseline pre-perf-audit` forwards Criterion flags to non-Criterion binary benches and fails on the example app.
- Commands used:
  - `cargo bench -p vespertide-core --bench normalize_benchmarks -- --save-baseline pre-perf-audit`
  - `cargo bench -p vespertide-planner --bench diff_benchmarks -- --save-baseline pre-perf-audit`
  - `cargo bench -p vespertide-query --bench sql_benchmarks -- --save-baseline pre-perf-audit`
  - `cargo bench -p vespertide-exporter --bench codegen_benchmarks -- --save-baseline pre-perf-audit`
  - `cargo bench -p vespertide-naming --bench naming_benchmarks -- --save-baseline pre-perf-audit`
  - `cargo build --workspace --all-features --exclude vespertide-fuzz --timings`
- Audit covered allocation, algorithmic complexity, cache/lazy computation, I/O/parallelism, and compile-time categories.
- Note: the requested `mcp_Ast_grep_search` tool was not available in this environment, so code-pattern audit used repository reads/searches and parallel exploration agents.

## Candidates Identified

| Category | File:Line | Pattern | Impact (estimated) | Risk | Status |
|---|---|---|---|---|---|
| Allocation | `crates/vespertide-planner/src/diff/mod.rs:27` | `actions` used `Vec::new()` despite schema-size bounded output | Medium on large diffs | Low | Applied |
| Allocation | `crates/vespertide-planner/src/diff/mod.rs:93` | per-table local action vector grew from zero capacity | Low/medium in large per-table diff loops | Low | Applied |
| Algorithm | `crates/vespertide-planner/src/diff/ordering.rs:26,153` | dependency vectors used linear `.contains()` in Kahn inner loops | Medium for FK-heavy create/delete ordering | Low | Applied |
| Algorithm | `crates/vespertide-planner/src/diff/ordering.rs:221` | sort comparator repeatedly scanned sorted table positions | High for many deleted tables | Low | Applied |
| Allocation | `crates/vespertide-exporter/src/seaorm/render.rs:40` | SeaORM line vector capacity hint possible | Medium | Low | Reverted: mixed regressions in codegen bench |
| Allocation | `crates/vespertide-exporter/src/seaorm/relations.rs:135` | clone-heavy `BTreeMap<String, usize>` entity count | Medium in relation-heavy exports | Low/medium | Reverted: mixed regressions in codegen bench |
| Algorithm/cache | `crates/vespertide-exporter/src/seaorm/relations.rs:55` | repeated `schema.iter().find()` per FK chain | Medium for FK-heavy SeaORM exports | Medium | Skipped: needs API-internal refactor and dedicated relation bench |
| Algorithm | `crates/vespertide-exporter/src/seaorm/relations.rs:356,413` | self-reference helper nested loops | Low/medium, niche schemas | Low | Skipped: uncommon path, no existing targeted bench |
| I/O/parallelism | `crates/vespertide-loader/src/models.rs:60` | recursive directory traversal is sequential | Medium only for deep model trees | Medium | Skipped: I/O error-ordering risk |
| I/O/parallelism | `crates/vespertide-cli/src/commands/export.rs:249` | async cleanup removes files one by one | Low, cleanup-only | Low | Skipped: no existing cleanup benchmark |
| Compile-time | `crates/vespertide-core/Cargo.toml` | `schema` feature pulls `schemars` by default | Low/medium compile-time potential | Medium ecosystem/API risk | Skipped: feature default change is breaking-adjacent |
| Compile-time | `crates/vespertide-exporter/src/seaorm/relations.rs` | large SeaORM relation module near 1000 lines | Maintainability/compile locality | Low | Skipped: refactor only, not measurable runtime improvement |

## Applied Changes

1. `diff_schemas` now preallocates the top-level action vector from schema sizes.
2. `diff_existing_table` now preallocates a small local action vector for common column/constraint changes.
3. `topological_sort_tables` and `sort_delete_tables` store dependency adjacency as `BTreeSet<&str>` instead of `Vec<&str>`, reducing inner-loop membership checks from linear scans to ordered-set lookups while preserving deterministic ordering.
4. `sort_delete_tables` builds one `BTreeMap<&str, usize>` position index before sorting delete actions, removing repeated sorted-order scans inside the comparator.

## Criterion Results (before / after / delta)

Planner results from `cargo bench -p vespertide-planner --bench diff_benchmarks -- --baseline pre-perf-audit`:

| Benchmark | Before | After | Delta | Status |
|---|---:|---:|---:|---|
| `diff_identity/10` | 17.786 µs | 16.492 µs | -3.67% | within noise |
| `diff_identity/100` | 187.49 µs | 157.63 µs | -18.12% | improved |
| `diff_identity/1000` | 3.2749 ms | 1.8347 ms | -43.98% | improved |
| `diff_add_column/10` | 22.501 µs | 16.345 µs | -26.48% | improved |
| `diff_add_column/100` | 174.72 µs | 178.11 µs | +5.03% | small regression; kept because large-N path improved and 100-table CI-size variance was isolated |
| `diff_add_column/1000` | 2.3502 ms | 1.9543 ms | -16.85% | improved |
| `diff_constraint_replacement_100` | 232.98 µs | 227.50 µs | +2.19% | no change detected |

Exporter experiment results were mixed, so exporter changes were reverted. Representative reverted measurements:

| Benchmark | Delta | Decision |
|---|---:|---|
| `render_entity/SeaOrm/cols=50/fk=false/enum=false` | -21.28% | promising but not retained alone |
| `render_entity/SeaOrm/cols=200/fk=true/enum=true` | +42.37% | regression, reverted |
| `render_entity/SeaOrm/cols=50/fk=false/enum=true` | +29.46% | regression, reverted |

## Skipped / Reverted Rationale

- SeaORM relation/render allocation tweaks were low risk structurally but not measurement-safe: several representative SeaORM cases regressed, so they were reverted.
- FK target table lookup caching in SeaORM likely needs a dedicated table-name index passed through recursive resolution. This is measurable but broader than a safe 60-minute low-risk change.
- Query builder `evolving_schema.clone()` is a real large-plan memory candidate, but replacing with `Arc`/`Cow` changes internal ownership and requires targeted builder benchmarks; skipped as medium risk.
- Loader parallel directory traversal and export cleanup batching are I/O-bound and need synthetic filesystem benchmarks to avoid optimizing noise.
- Compile-time feature splitting around `schemars` may improve build times, but changing default feature surfaces is compatibility-sensitive and was not applied.

## Compile-Time Findings

- `cargo build --workspace --all-features --exclude vespertide-fuzz --timings` completed in 36.89s after dependencies were warm.
- The timing report was written to `target/cargo-timings/cargo-timing-20260520T141713508Z-23a3a9a5856bf856.html`.
- Main compile-time follow-ups remain dependency/feature driven (`schemars`, `sea-orm`, `sqlx`) rather than obvious local generic hot spots.

## Next Recommendations

1. Add targeted Criterion benches for delete-table ordering and FK-heavy create ordering; current diff benches show large-N wins but do not isolate `sort_delete_tables` directly.
2. Add a SeaORM FK-chain benchmark before refactoring table lookup caching.
3. Add filesystem synthetic benches for loader directory traversal and export cleanup before changing task granularity.
4. Re-evaluate `schemars` default feature impact in a semver-planned release.
