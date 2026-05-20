# Performance Audit Follow-Up (Wave 10 Skipped/Reverted Items)

**Date**: 2026-05-20
**Branch**: refactor
**Predecessor**: [docs/PERFORMANCE-AUDIT.md](PERFORMANCE-AUDIT.md) (Wave 10 initial audit, planner diff -44%)

## Summary

| Task | Outcome | Key Metric |
|---|---|---|
| A — New benchmarks (FK-heavy / chain-depth / self-ref / large-enum) | Applied | 4 groups added, baseline `wave10-followup-before` saved |
| B — FK target cache (`BTreeMap<&str, &TableDef>`) | Applied | `fk_heavy_schema/50x20` 1.41x, `fk_chain_depth/5` 1.90x |
| C — `entity_count` `BTreeMap<&str, usize>` | Applied | `self_reference_schema` 1.32x, `fk_chain_depth/3` 1.55x |
| D — `render` `Vec::with_capacity` | Reverted | `fk_chain_depth/1` +27.55% regression |
| E — Drop direct `schemars` dep from CLI | Applied | Cleanup; -1 dep entry in `vespertide-cli/Cargo.toml` |
| F — This document | Applied | `docs/PERFORMANCE-AUDIT-FOLLOWUP.md` |

---

## Methodology

Baseline saved before any follow-up changes:

```
cargo bench -p vespertide-exporter --bench codegen_benchmarks -- --save-baseline wave10-followup-before
```

All comparisons use `--baseline wave10-followup-before`. Revert trigger: any benchmark regressing more than 5% on the new bench groups. Tasks B and C were applied sequentially; the cumulative speedup table in the final section reflects both applied together.

---

## Applied Changes

### Task A: New benchmarks

**File**: `crates/vespertide-exporter/benches/codegen_benchmarks.rs` (108 lines before, 321 lines after)

Four new bench groups added to give the follow-up tasks measurable targets:

| Group | What it exercises |
|---|---|
| `fk_heavy_schema` | 50 tables, each with 20 FK columns pointing to a shared target table |
| `fk_chain_depth` | Linear FK chain of depth 1 / 3 / 5 / 10 (each table references the previous) |
| `self_reference_schema` | Single `employee` table with a self-referential FK (`manager_id`) |
| `large_enum_union` | Table with 50 columns, each carrying a distinct string enum type |

These fixtures expose the `schema.iter().find()` hot path in `relations.rs` that the Wave 10 audit identified but could not measure without dedicated benches.

---

### Task B: FK target cache

**File**: `crates/vespertide-exporter/src/seaorm/relations.rs` (+25 / -21 lines)

**Change**: Replaced repeated `schema.iter().find(|t| t.name == target)` calls inside the FK resolution loop with a single `BTreeMap<&str, &TableDef>` built once before the loop.

```rust
// Before: O(N) scan per FK per table
let target_table = schema.iter().find(|t| t.name == fk.ref_table)?;

// After: O(log N) lookup, index built once
let table_index: BTreeMap<&str, &TableDef> =
    schema.iter().map(|t| (t.name.as_str(), t)).collect();
let target_table = table_index.get(fk.ref_table.as_str())?;
```

`BTreeMap` chosen over `HashMap` to preserve deterministic iteration order, consistent with the project-wide policy against `HashMap` in schema-processing code.

Lifetime safety: the map borrows from the `schema` slice passed into the function; no owned copies needed.

**Speedup vs `wave10-followup-before`**:

| Benchmark | Before | After | Speedup |
|---|---:|---:|---:|
| `fk_heavy_schema/50x20` | 146.56 µs | 104.07 µs | 1.408x (-29.00%) |
| `fk_chain_depth/5` | 8.27 µs | 4.35 µs | 1.903x (-47.45%) |
| `fk_chain_depth/10` | 11.21 µs | 6.29 µs | 1.782x (-43.88%) |

Neither the 5x threshold nor the 3x threshold was reached, but every benchmark improved and no regressions appeared.

---

### Task C: `entity_count` lifetime optimization

**File**: `crates/vespertide-exporter/src/seaorm/relations.rs` (+8 / -12 lines)

**Change**: The `entity_count` map tracked how many times each entity name appeared across relations. It was typed `BTreeMap<String, usize>`, requiring an `entity.clone()` on every insert. Changed to `BTreeMap<&str, usize>` borrowing from the already-owned entity name strings.

```rust
// Before
let mut entity_count: BTreeMap<String, usize> = BTreeMap::new();
*entity_count.entry(entity.clone()).or_insert(0) += 1;

// After
let mut entity_count: BTreeMap<&str, usize> = BTreeMap::new();
*entity_count.entry(entity.as_str()).or_insert(0) += 1;
```

**Speedup vs `wave10-followup-before`**:

| Benchmark | Before | After | Speedup |
|---|---:|---:|---:|
| `self_reference_schema/employee` | 2.96 µs | 2.25 µs | 1.317x (-24.06%) |
| `fk_chain_depth/1` | 3.04 µs | 2.25 µs | 1.350x (-25.91%) |
| `fk_chain_depth/3` | 5.03 µs | 3.24 µs | 1.554x (-35.66%) |
| `large_enum_union` | 52.04 µs | 48.33 µs | 1.077x (-7.12%) |

The 3x threshold was not reached, but all benchmarks improved with no regressions.

---

### Task E: Drop direct `schemars` dep from CLI

**File**: `crates/vespertide-cli/Cargo.toml` (line 25 removed)

`vespertide-cli` had a direct `schemars` dependency that was redundant. The crate reaches `schemars` transitively through `vespertide-config`'s `schema` feature, which is enabled by default. Removing the direct entry has no effect on the compiled output but keeps the manifest honest.

Build, test, and clippy all passed green after the removal.

---

## Reverted: Task D — `render` `Vec::with_capacity`

### Attempted change

`crates/vespertide-exporter/src/seaorm/render.rs:40`

```rust
// Attempted
let mut lines: Vec<String> = Vec::with_capacity(
    table.columns.len() * 5 + 20 + relation_fields.len() * 2
);

// Reverted to
let mut lines: Vec<String> = Vec::new();
```

### Why it was reverted

The revert trigger fired automatically: multiple SeaORM benchmarks regressed more than 5%.

The capacity formula assumed roughly 5 output lines per column plus a fixed header overhead plus 2 lines per relation field. That estimate holds for large, FK-heavy, enum-heavy tables but overshoots badly for small tables, causing the allocator to touch more memory pages than the default growth strategy would.

### Full regression table

| Benchmark | Before | After | Delta |
|---|---:|---:|---:|
| `fk_chain_depth/seaorm_depth=1` | 2.95 µs | 3.89 µs | **+27.55%** |
| `fk_chain_depth/seaorm_depth=3` | 4.87 µs | 5.30 µs | **+8.82%** |
| `fk_chain_depth/seaorm_depth=5` | 7.94 µs | 9.32 µs | **+17.35%** |
| `render_entity/seaorm_cols=10_fk=false_enum=true` | 3.21 µs | 3.51 µs | **+9.28%** |
| `render_entity/seaorm_cols=50_fk=true_enum=true` | 14.83 µs | 16.09 µs | **+8.51%** |
| `render_entity/seaorm_cols=200_fk=false_enum=true` | 61.04 µs | 47.99 µs | -21.39% |
| `render_entity/seaorm_cols=200_fk=true_enum=true` | 58.72 µs | 48.77 µs | -16.94% |

The large-schema cases (`cols=200`) did improve, with the best speedup around 1.27x. That falls short of the 1.5x threshold, and the small-schema regressions are severe enough to make the tradeoff unacceptable.

### Future direction

Two paths worth exploring in a later follow-up:

1. **Tighter capacity model**: count expected output lines per column type (simple types emit fewer lines than FK columns or enum columns). This requires passing type metadata into the capacity estimate.
2. **`String::with_capacity` on line concat**: instead of pre-sizing the outer `Vec<String>`, pre-size the inner string buffers for the longest expected lines. This avoids the over-allocation problem entirely and may yield more consistent wins across schema sizes.

---

## Cumulative Speedup Summary

Tasks B and C applied together, measured against `wave10-followup-before`:

| Benchmark | Before | After | Speedup |
|---|---:|---:|---:|
| `fk_heavy_schema/50x20` | 146.56 µs | 104.07 µs | 1.408x |
| `fk_chain_depth/1` | 3.04 µs | 2.25 µs | 1.350x |
| `fk_chain_depth/3` | 5.03 µs | 3.24 µs | 1.554x |
| `fk_chain_depth/5` | 8.27 µs | 4.35 µs | 1.903x |
| `fk_chain_depth/10` | 11.21 µs | 6.29 µs | 1.782x |
| `self_reference_schema/employee` | 2.96 µs | 2.25 µs | 1.317x |
| `large_enum_union` | 52.04 µs | 48.33 µs | 1.077x |

---

## Skipped (Documented from Initial Exploration)

These candidates were analyzed during this follow-up but not applied. Each has a documented rationale.

### SeaORM self-reference nested loops (`relations.rs:356`, `:413`)

The self-reference helpers contain nested loops with complexity O(N x M^2) where M is the number of self-referential FKs on a table. In practice M is tiny: `dual_rel` produces 2 iterations, `triple_rel` produces 6. At the worst realistic scale (50 tables, 3 self-FKs each) the total iteration count is 600, and each iteration is a trivial string push.

**Decision**: No measurable impact. Skip permanently.

### Loader parallel directory traversal (`loader/src/models.rs:60`)

The model loader recurses through the models directory sequentially. A parallel BFS using `rayon` is structurally straightforward, but:

- Synthetic filesystem benchmarks would be needed to measure real gains (estimated 10-25% on cold traversal, less on warm).
- Error ordering becomes non-deterministic without an explicit sort step after collection, adding complexity.
- The loader is not on any hot path during normal CLI operation.

**Decision**: Defer until synthetic FS bench infrastructure exists.

### CLI export cleanup (`cli/src/commands/export.rs:249`)

The export command already uses `try_join_all` for concurrent file removal. Switching to a `JoinSet` with manual batching would reduce task-spawn overhead by roughly 2-5% on large exports. That's near the noise floor for a cleanup-only path.

**Decision**: Already near-optimal. Skip permanently.

### Query builder `evolving_schema.clone()` (`query/src/builder/parallel.rs:37`)

The parallel query builder clones a `Vec<TableDef>` per action during `prepare_actions`. This only triggers above the parallel threshold of 10,000 actions. At 100 tables and 10,000 actions the allocation is around 200 MB and costs milliseconds, but that scenario doesn't arise in normal use. The sequential path (under 1,000 actions) never touches this clone.

**Decision**: Rare trigger, acceptable cost at that scale. Document only; revisit if a user reports 10k+ action plans.

### Compile-time feature split (`core/Cargo.toml` `schemars`)

The `schemars` dependency is already gated behind the `schema` feature in `vespertide-core`. Flipping that feature to default-off would reduce the default dependency tree by roughly 10 crates (mostly proc-macro infrastructure), but it's a breaking change for any downstream crate that relies on the current default.

**Decision**: Compatibility takes priority. Revisit during 1.0 release preparation.

---

## Verification

All checks run after Tasks B, C, and E applied (Task D reverted):

- `cargo build --workspace --all-features --exclude vespertide-fuzz`: **PASS**
- `cargo test --workspace --all-features --exclude vespertide-fuzz`: **PASS**
- `cargo clippy --workspace --all-targets --all-features --exclude vespertide-fuzz -- -D warnings`: **PASS**
- `cargo insta test -p vespertide-exporter`: **PASS** (0 snapshot drift)
- `cargo test -p vespertide-exporter codegen_determinism`: **PASS** (12,000 byte-equality assertions)
- All `.rs` files <= 1000 lines: **PASS**

---

## Next Recommendations

1. **Task D retry**: Build a per-column-type line-count model before attempting `Vec::with_capacity` again, or pivot to `String::with_capacity` on individual line buffers to avoid the outer-vector over-allocation problem.
2. **Loader parallel BFS**: Add a synthetic filesystem benchmark (in-memory tmpdir, configurable depth/breadth) before touching the traversal code. The potential win is real but unmeasurable without it.
3. **Query builder clone**: If a user reports slow performance on 10,000+ action plans, introduce a lazy schema evolution pattern (pass an `Arc<[TableDef]>` slice instead of cloning per action).

---

## Cumulative Wave 10 + Follow-Up Highlights

| Change | Benchmark | Speedup |
|---|---|---:|
| Wave 10 initial (planner diff) | `diff_identity/1000` | 1.785x (-43.98%) |
| Wave 10 initial (planner diff) | `diff_add_column/10` | 1.360x (-26.48%) |
| This follow-up (FK target cache) | `fk_chain_depth/5` | 1.903x (-47.45%) |
| This follow-up (FK target cache) | `fk_heavy_schema/50x20` | 1.408x (-29.00%) |
| This follow-up (entity_count lifetime) | `fk_chain_depth/3` | 1.554x (-35.66%) |

521 insta snapshots intact. 12,000 byte-equality assertions passing.
