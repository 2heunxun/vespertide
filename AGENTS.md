# VESPERTIDE KNOWLEDGE BASE

**Generated:** 2026-05-24
**Commit:** 9103bb3
**Branch:** refactor
**Targeting:** 0.2.0 (API stability + LSP hot-spot caching)

## OVERVIEW

Rust workspace for declarative database schema management. Define schemas in JSON, diff against migration history, generate typed actions and SQL.

## STRUCTURE

```
vespertide/
├── crates/
│   ├── vespertide-core/      # Data structures: TableDef, ColumnDef, MigrationAction; newtype names
│   ├── vespertide-planner/   # Schema diffing, baseline reconstruction, validation
│   ├── vespertide-query/     # SQL generation (Postgres/MySQL/SQLite)
│   ├── vespertide-cli/       # CLI commands: init, diff, sql, revision, export
│   ├── vespertide-exporter/  # ORM codegen: SeaORM, SQLAlchemy, SQLModel
│   ├── vespertide-loader/    # Filesystem loading of models/migrations
│   ├── vespertide-config/    # vespertide.json configuration
│   ├── vespertide-lsp/       # Language server: 13 LSP capabilities + HS-7~11 caching
│   ├── vespertide-macro/     # Compile-time migration macro
│   ├── vespertide-naming/    # Naming convention utilities
│   ├── vespertide-schema-gen/# JSON Schema generation
│   └── vespertide/           # Re-export crate (user-facing API)
├── examples/app/             # Example project with models/migrations (out-of-workspace)
├── tools/lsp-profile/        # LSP synthetic / realistic workload + latency profiler (out-of-workspace)
├── fuzz/                     # cargo-fuzz targets (4 targets, see FUZZING section)
├── tests/runtime-sqlite/     # vespertide-macro runtime SQLite tests (out-of-workspace)
├── schemas/                  # Generated JSON Schemas for IDE support
├── docs/                     # profiling.md, profiling-baseline.json, clippy-allow-audit.md
└── CLAUDE.md                 # Detailed implementation guidance
```

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Core types (TableDef, ColumnDef) | `vespertide-core/src/schema/` | Start with `table.rs`, `column.rs` |
| **Newtype name wrappers** | `vespertide-core/src/schema/names.rs` | `TableName` / `ColumnName` / `IndexName` with `#[serde(transparent)]` |
| Column type system | `vespertide-core/src/schema/column.rs` | `ColumnType::Simple/Complex` variants |
| Migration actions | `vespertide-core/src/action/` | **14 action variants** (incl. `RawSql` escape hatch), `MigrationPlan` struct |
| **QueryError variants** | `vespertide-query/src/error.rs` | `InvalidColumnType` / `SchemaError` / `BackendError` / `UnsupportedAction`; `Other` is `#[deprecated]` |
| Schema diffing | `vespertide-planner/src/diff/` | topological sort for FK deps |
| SQL generation | `vespertide-query/src/sql/` | One file per action type |
| CLI commands | `vespertide-cli/src/commands/` | `cmd_*` functions |
| ORM export | `vespertide-exporter/src/{seaorm,sqlalchemy,sqlmodel,jpa}/` | Backend-specific generators |
| Compile-time macro | `vespertide-macro/src/lib.rs` | `vespertide_migration!` proc macro |
| **LSP RingCache (HS-7~11)** | `vespertide-lsp/src/cache.rs` | Generic ring-buffer LRU shared across symbols/diagnostics/drift/semantic-token caches |
| **LSP drift cache** | `vespertide-lsp/src/drift/cache.rs` | HS-10 drift cache implementation |
| **LSP profiler** | `tools/lsp-profile/src/` | Synthetic + realistic workloads with p50/p95/p99 latency stats |

## DATA FLOW

```
JSON Models → load_models() → Vec<TableDef>
                                    ↓
Applied Migrations → schema_from_plans() → Baseline Schema
                                                ↓
                            diff_schemas() → Vec<MigrationAction>
                                                ↓
                            plan_next_migration() → MigrationPlan
                                                        ↓
                            build_action_queries() → Vec<BuiltQuery>
                                                        ↓
                            BuiltQuery.build(backend) → SQL String
```

## CONVENTIONS

### ColumnType Usage (CRITICAL)
```rust
// CORRECT - Always use wrapped variant
ColumnType::Simple(SimpleColumnType::Integer)
SimpleColumnType::Integer.into()

// WRONG - Old flat syntax
ColumnType::Integer  // Does not exist
```

### Newtype Names (0.2.0+)
`TableName`, `ColumnName`, `IndexName` are newtypes with `#[serde(transparent)]` —
JSON wire format is **byte-identical** to plain `String`, but the Rust type system
distinguishes them.

```rust
use vespertide_core::schema::names::{TableName, ColumnName};

let table: TableName = "user".into();          // From<&str>
let col = ColumnName::new("email".to_string()); // explicit constructor
assert_eq!(table.as_str(), "user");             // explicit accessor
assert!(table == "user");                       // PartialEq<&str>
println!("{table}");                            // Display
let owned: String = table.into_inner();         // consume back to String
```

Newtypes auto-deref to `&str`, so most function-call sites work without `.into()`.
When constructing struct literals (e.g. `TableDef { name: ... }`), prefer `.into()`
from string literals over the explicit constructor for terseness.

### `#[non_exhaustive]` Structs (0.2.0+)
`VespertideConfig`, `SeaOrmConfig`, `MigrationOptions` are `#[non_exhaustive]`:
external callers MUST construct via `..Default::default()` or the provided
constructor.

```rust
// CORRECT
let opts = MigrationOptions { dry_run: true, ..Default::default() };
let opts = MigrationOptions::new();

// WRONG - exhaustive struct literal will fail E0639
let opts = MigrationOptions { dry_run: true, force: false /* ... */ };
```

### QueryError Variants (0.2.0+)
Prefer the specific variant. `Other(String)` is `#[deprecated]` and emits a warning:

```rust
// CORRECT - specific variants
return Err(QueryError::SchemaError(format!("Failed to normalize {table}: {e}")));
return Err(QueryError::InvalidColumnType { column, reason });
return Err(QueryError::BackendError { backend, reason });
return Err(QueryError::UnsupportedAction { action, backend });

// WRONG - triggers deprecation warning + uninformative match arms downstream
return Err(QueryError::Other("Failed to ...".into()));
```

### `#[expect(...)]` over `#[allow(...)]` (0.2.0+)
Workspace `[lints.clippy]` enforces `allow_attributes_without_reason = "warn"` and
`allow_attributes = "warn"`. Every suppression MUST be `#[expect(...)]` with a
domain-specific `reason = "..."` string.

```rust
// CORRECT - self-reports if the lint stops firing
#[expect(clippy::cast_possible_truncation, reason = "LSP wire format mandates u32; values bounded by file size")]
fn byte_to_lsp_position(...) -> u32 { ... }

// WRONG - silent, perpetual; will fail allow_attributes_without_reason
#[allow(clippy::cast_possible_truncation)]
fn byte_to_lsp_position(...) -> u32 { ... }
```

Test oracle code (production-public functions only called by tests) should use
`#[cfg(test)]` rather than `#[expect(dead_code)]`. See
`vespertide-lsp/src/diagnostics/validation/visitors.rs` for the canonical pattern.

See `docs/clippy-allow-audit.md` for the full audit history.

### Naming
- Indexes: `ix_{table}__{columns}` or `ix_{table}__{name}`
- Unique: `uq_{table}__{columns}`
- Foreign keys: `fk_{table}__{columns}`

## ANTI-PATTERNS

| Pattern | Why Bad |
|---------|---------|
| `ColumnType::Integer` | Use `ColumnType::Simple(SimpleColumnType::Integer)` |
| Forgetting inline fields in ColumnDef | Will cause compile errors - 4 Option fields required |
| Raw SQL in migrations | Prefer typed `MigrationAction` enums. `MigrationAction::RawSql` exists as a documented **emergency escape hatch** only — non-portable across backends, skipped by baseline replay, and not recommended for normal use |
| Skipping `normalize()` on TableDef | Inline constraints won't convert to table-level |
| `.rs` file exceeding 1000 lines | Maintainability hard limit - split into focused submodules |
| `#[allow(LINT)]` without `reason = "..."` | Workspace lint denies — use `#[expect(LINT, reason = "...")]` instead |
| `#[allow(...)]` on dead code | If the item is only used by tests, gate with `#[cfg(test)]` instead. If truly dead, delete it. |
| `QueryError::Other(...)` in new code | Emits deprecation warning. Use `SchemaError` / `InvalidColumnType` / `BackendError` / `UnsupportedAction` |
| Exhaustive struct literal for `MigrationOptions` / `VespertideConfig` | `#[non_exhaustive]` — use `..Default::default()` |
| Comparing newtype with `String::eq(&name.to_string(), "user")` | `TableName: PartialEq<&str>` — use `name == "user"` directly |

## COMMANDS

```bash
# Build/Test
cargo build --workspace --exclude vespertide-fuzz
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all --check

# CLI (always use -p vespertide-cli)
cargo run -p vespertide-cli -- init
cargo run -p vespertide-cli -- new <model>
cargo run -p vespertide-cli -- diff
cargo run -p vespertide-cli -- sql
cargo run -p vespertide-cli -- revision -m "message"
cargo run -p vespertide-cli -- export --orm seaorm

# Regenerate JSON schemas (must produce zero diff vs `schemas/`)
cargo run -p vespertide-schema-gen -- --out schemas

# Schema drift verification (CI gate)
cargo run -p vespertide-schema-gen -- --out _tmp_schemas
git diff --no-index schemas _tmp_schemas    # must be empty

# Snapshot testing
cargo insta test -p vespertide-exporter
cargo insta accept

# LSP performance profiler (out-of-workspace tool — uses its own Cargo.lock)
cargo run --release --manifest-path tools/lsp-profile/Cargo.toml -- \
    --workload synthetic --baseline docs/profiling-baseline.json
cargo run --release --manifest-path tools/lsp-profile/Cargo.toml -- \
    --workload realistic --baseline docs/profiling-realistic.json

# Verify zero unjustified clippy `allow`s
cargo clippy --workspace --all-targets --all-features 2>&1 | grep -c allow_attributes_without_reason
# Expected: 0
```

## COMPLEXITY HOTSPOTS (≤ 1000-line policy enforced)

**Policy**: Every `.rs` file must stay ≤ 1000 lines. CI enforces; current state: ✅ zero violations.

Largest production files (margin 임박 = next split candidates):

| File | Lines | What |
|------|-------|------|
| `cli/src/commands/export.rs` | 991 | CLI export command for 4 ORMs |
| `query/src/sql/create_table.rs` | 750 | CREATE TABLE statement generation |
| `query/src/sql/add_column.rs` | 732 | ADD COLUMN with SQLite temp-table for non-nullable/enum |
| `query/src/sql/helpers.rs` | 706 | Column type mapping, FK actions, enum/naming helpers |
| `cli/src/commands/diff.rs` | 659 | Diff CLI command |
| `loader/src/models.rs` | 641 | Model file loading with rayon parallelization |
| `naming/src/lib.rs` | 630 | Naming convention utilities |
| `query/src/sql/modify_column_default.rs` | 604 | ALTER COLUMN SET/DROP DEFAULT |

Largest test files (snapshot-locked; split costs snapshot rename):

| File | Lines | What |
|------|-------|------|
| `core/src/schema/table/tests/mod.rs` | 986 | Table normalization tests |
| `query/src/sql/delete_column/tests/mod.rs` | 847 | DROP COLUMN tests |
| `planner/src/validate/tests/plan_validation.rs` | 954 | Plan validation tests |
| `core/src/action/tests/mod.rs` | 894 | MigrationAction unit tests |
| `planner/src/apply/tests/mod.rs` | 806 | apply_action unit tests |

**Historical splits** (Waves 1-9 of optimization work):
- `planner/src/diff.rs` (4739) → `diff/{mod,columns,constraints,ordering,tables}.rs`
- `exporter/src/seaorm/mod.rs` (4122) → split into `mod.rs` + `relations.rs` + `helper_tests.rs`
- `cli/src/commands/revision.rs` (3064) → `revision/{mod,prompts,recreate,tests}.rs`
- `planner/src/validate.rs` (2299) → `validate/{plan,schema,foreign_keys,tests}.rs`
- `planner/src/apply.rs` (1534) → `apply/{mod,tests}.rs`
- `core/src/schema/table.rs` (1526) → `table/{mod,tests}.rs`
- `query/src/sql/mod.rs` (1507) → `sql/{mod,tests}.rs`
- `query/src/sql/remove_constraint.rs` (1465) → `remove_constraint/{mod,sqlite,...}.rs`
- `exporter/src/sqlalchemy/mod.rs` (1383) → `sqlalchemy/{mod,render,types,tests}.rs`
- `query/src/sql/add_constraint.rs` (1356) → `add_constraint/{mod,tests}.rs`
- `exporter/src/sqlmodel/mod.rs` (1274) → `sqlmodel/{mod,render,types,tests}.rs`
- `core/src/action.rs` (1236) → `action/{mod,tests}.rs`
- `exporter/src/jpa/mod.rs` (1122) → `jpa/{mod,render,types}.rs`
- `query/src/sql/delete_column.rs` (1084) → `delete_column/{mod,tests}.rs`
- `query/src/sql/modify_column_type.rs` (1056, Wave 9) → `modify_column_type/{mod,direct,sqlite_rebuild,tests}.rs`
- `query/src/builder.rs` (995, Wave 9 preventive) → `builder/{mod,sequential,transaction,parallel,tests}.rs`
- `lsp/src/backend/mod.rs` (970, preventive) → extracted 7 navigation/feature handler bodies (`completion`, `hover`, `goto_definition`, `references`, `code_action`, `inlay_hint`, `symbol`) into `backend/handler_navigation.rs`. Trait methods in `mod.rs` are now one-line delegations to `handler_navigation::*_impl(self, params).await`. Final: `mod.rs` 599 lines, `handler_navigation.rs` 358 lines. Mirrors the pre-existing `handler_file_features.rs` / `handler_rename.rs` pattern.
- `lsp/src/drift/mod.rs` (715 production-only, preventive) → `drift/{types,compute,sources,actions}.rs` (with pre-existing `cache.rs` unchanged). Carved by responsibility: `types.rs` (118) holds `DriftKind` + `DomainDrift` + internal `DriftRecord` tuple alias; `compute.rs` (240) holds `compute` / `compute_with_cache` / `loaded_state_with_cache` + path resolution helpers (`find_config_and_mtime`, `resolve_models_dir`, `guess_uri`, `path_to_uri`); `sources.rs` (31) holds `source_and_tree` + `source_and_tree_from_disk`; `actions.rs` (356) holds the `action_to_drift` dispatcher, per-action drift builders, render helpers (`render_column_type` / `render_default` / `render_nullable` / `render_comment`), `lookup_baseline_column`, and tree-sitter range helpers. Public API surface unchanged (`pub use {DriftKind, DomainDrift, DriftCache, compute, compute_with_cache}`). Cross-module helpers narrowed from `pub(crate)` to `pub(super)` since callers all live under `drift::`. With production now ~22 lines, the previously out-of-line `drift/tests/mod.rs` (484 lines) was inlined into `drift/mod.rs` as a `#[cfg(test)] mod tests { ... }` block — final `drift/mod.rs` 528 lines (well under the 1200 combined ceiling); `tests/mod.rs` count 10 → 9.
- `exporter/src/seaorm/relations.rs` (1000 production-only, at workspace cap) → `seaorm/relations/{mod,fk_resolve,naming,self_ref,reverse}.rs`. Carved by responsibility: `fk_resolve.rs` (118) holds `as_fk` (private) + the `resolve_fk_target` / `resolve_fk_target_inner` chain walker + the `ForwardRelationResolution` struct emitted by `resolve_table_fks_pure` (sequential/parallel split on `SEAORM_RELATION_PAR_FK_THRESHOLD`); `naming.rs` (134) holds the pure naming helpers `generate_relation_enum_name` / `unique_relation_enum_name` / `infer_field_name_from_fk_column` / `pluralize` / `fk_attr_value`; `self_ref.rs` (233) holds `SelfRefJunction` + `collect_self_ref_junction` / `self_ref_link_name` / `resolve_self_ref_link_module_path` / `render_self_ref_link_helpers` / `render_self_ref_query_helpers`; `reverse.rs` (467) holds the private `ReverseRelation` struct + `collect_reverse_relation_targets` / `collect_many_to_many_targets` / `reverse_relation_field_defs` (+ private `ReverseRelationFieldCtx` and `collect_many_to_many_relations`); `mod.rs` (140) owns the forward (`belongs_to`) `relation_field_defs_with_schema` entry point and the `pub(in crate::seaorm) use` re-exports that satisfy the existing `use super::relations::{...}` import in `seaorm/render.rs` and the `#[cfg(test)] use relations::*;` glob in `seaorm/mod.rs`. Visibility envelope unchanged: items previously `pub(super)` of `seaorm::relations` (i.e. visible throughout `seaorm`) are now `pub(in crate::seaorm)` on items hosted in submodules — same scope, just spelled differently to survive the extra module hop. SeaORM codegen output is byte-identical (0 `.snap.new` files across the 232 cross-ORM snapshots + per-ORM seaorm snapshots). Largest sub-file (`reverse.rs`, 467) is well under the 1000-line policy; aggregate relations-tree = 1092 lines.
- `cli/src/commands/erd/svg.rs` (995 production-only, preventive) → `erd/svg/{mod,style,model,layout,edges,render,util}.rs`. Carved by responsibility: `style.rs` (55) holds every palette / sizing / typography constant as `pub(super)`; `model.rs` (189) holds `TableBox` / `RowSpec` / `EdgeSpec` plus `build_boxes` / `build_edges` / `measure_table_width` / `badge_block_width`; `layout.rs` (116) holds `compute_ranks` / `layout_grid` / `rebalance_groups` / `view_size`; `edges.rs` (247) holds the private `Side` enum, `edge_geometry`, `render_edge_path` / `render_edge_label`, `pick_anchors`, `bezier_path` / `bezier_at` / `control_point`, and the `parallel_curvature_offset` / `label_t_for_parallel` helpers; `render.rs` (297) holds `render_doc` + `render_defs` + `render_table` / `render_row` / `render_badge` + the `rounded_top_path` / `rounded_bottom_path` SVG-path emitters; `util.rs` (32) holds `render_empty` and `escape_xml`; `mod.rs` (49) keeps the single public entry `pub fn render_svg(...)` orchestrating the pipeline. Public API surface unchanged — `erd::svg::render_svg` resolves identically. The original 9-lint file-level `#![expect(clippy::...)]` block was distributed per-file to exactly the lints each module triggers: `cast_precision_loss` (every coord-math file), `cast_lossless` (model only, for `u32`→`f64` badge counts), `cast_possible_truncation`+`cast_sign_loss` (layout only, for `sqrt().ceil() as usize`), `range_plus_one` (layout only, for `0..(n+1)` rank fixed-point), `uninlined_format_args` (edges/render/util, for `writeln!("{x}", x = …)` SVG templates), `too_many_arguments`+`similar_names` (edges only, for Bézier helpers), `unnecessary_wraps` (mod.rs only, for `render_svg -> Result`). `style.rs` carries zero lint exemptions. Largest sub-file (`render.rs`, 297) is well under the 1000-line policy; aggregate svg-tree = 985 lines.

Verify line policy: `python -c "import os, glob; files = []; [files.extend(glob.glob(os.path.join(r,'*.rs'))) for r,_,_ in os.walk('crates')]; over = [(sum(1 for _ in open(f, encoding='utf-8', errors='ignore')), f) for f in files]; result = sorted([x for x in over if x[0] > 1000], reverse=True); print('\n'.join(f'{l:5} {p}' for l, p in result) if result else 'OK: zero files >1000 lines')"`

## TESTING

- `rstest` for parameterized tests — **default choice for any test with ≥ 2 input variants** (multi-backend, multi-ORM, multi-format). Plain `#[test]` is reserved for single-case unit tests.
- `serial_test::serial` for filesystem tests
- `insta` for snapshot testing (exporter crate)
- `proptest` for property-based testing (`vespertide-planner` diff + `vespertide-query` SQL)
- Helper functions: `col()`, `table()` reduce boilerplate
- **2267 tests across ~276 `.rs` files, 0 failed, 3 documented `#[ignore]`** (offline trybuild + 2 `///` doctest blocks)

### Test-file placement policy (avoid confusion with production code)

| Pattern | Verdict | Rationale |
|---|---|---|
| **`#[cfg(test)] mod tests { ... }` inline at the bottom of a production `.rs`** | ✅ **Preferred — default choice** | Closest to the code under test; zero confusion; no `mod tests;` declaration needed; private items reachable via `use super::*;` without widening visibility. Allowed when **`production_lines + inline_test_lines + 5 wrapper` ≤ 1200**. |
| `src/<module>/tests/mod.rs` (entry file inside a `tests/` directory) | ✅ Acceptable fallback | Use **only** when (a) inlining would push the parent `.rs` over the **1200-line combined ceiling** (production + inline tests), or (b) the test entry needs to declare sub-modules (`mod foo; mod bar;`) for `src/<module>/tests/<name>.rs` siblings |
| `src/<module>/tests/<name>.rs` (anything under a `tests/` directory) | ✅ Acceptable | Directory name marks it as test-only |
| `crates/<crate>/tests/<name>.rs` (cargo integration tests) | ✅ Acceptable | Standard cargo layout |
| **`src/<module>/tests.rs`** (bare sibling file named `tests.rs`) | ❌ Forbidden | Owner directive: test files must live inside a `tests/` directory only. Convert to `src/<module>/tests/mod.rs` (parent `mod tests;` declaration is unchanged — cargo resolves the directory entry automatically), **or** preferably inline into the parent `.rs` |
| **`src/<module>/<name>_tests.rs`** (e.g. `helper_tests.rs` next to `helper.rs`) | ❌ Forbidden | Confused with production helpers; move under `src/<module>/tests/<name>.rs` |
| `src/<module>/test_<name>.rs` (e.g. `test_fixtures.rs`) | ⚠️ Discouraged | Prefer `src/<module>/tests/fixtures.rs` for new code; existing exceptions documented per-crate |

**Line-policy ceilings (two-tier)**:

- **Production-only `.rs` files** (without inline tests): bound by the workspace
  **≤ 1000-line** policy. This is the long-standing maintainability cap and is
  unchanged.
- **Production `.rs` files carrying inline `#[cfg(test)] mod tests { ... }`**:
  bound by **≤ 1200 lines** combined (`production_lines + inline_test_lines + 5
  wrapper`). The additional 200 lines is the budget for tests — it exists
  **only** when a production file carries inline test code, and it must not be
  used to grow production logic.
- Canonical inline-with-tests examples at the new 1200 ceiling:
  `commands/erd/mod.rs` (1065 lines), `vespertide-macro/src/lib.rs` (1177
  lines), `vespertide-core/src/action/mod.rs` (1101 lines).

**Decision flow for a new test module**:

1. **Default**: append `#[cfg(test)] mod tests { use super::*; ... }` at the bottom of the production file. No `mod tests;` declaration; the inline block defines the module.
2. **If `parent.rs + tests > 1200 lines`** (the combined ceiling for files carrying inline tests): use `src/<module>/tests/mod.rs` instead. (Production-only files remain bound by the ≤ 1000-line workspace cap.)
3. **If the test needs to split into sub-files** (`mod foo; mod bar;`): use `src/<module>/tests/mod.rs` as the entry and put siblings under `src/<module>/tests/<name>.rs`.
4. **Never** widen visibility (`pub`, `pub(crate)`, `pub(super)`) of a production item just to make an out-of-line test reach it. Inline placement makes this unnecessary, since `super::*` from inside an inline `mod tests` already sees every private item of the parent module.

**Snapshot-path implication for migrations**: insta's default snapshot directory is resolved relative to the test file's location. Inlining a test from `parent/tests/mod.rs` into `parent.rs` shifts the default from `parent/tests/snapshots/` to `parent/snapshots/`. If the test uses explicit `with_settings!({ snapshot_path => "../../snapshots" })` from `parent/tests/mod.rs`, change it to `"../snapshots"` after inlining so the same physical `snapshots/` directory keeps resolving. Module-path naming inside snapshot filenames is unchanged (the inline module is still named `tests`, so `<crate>__<module>__tests__<name>.snap` stays byte-identical).

**Migration rule**: When you split a test file or extract fixtures, the new files live under `src/<module>/tests/` — never as `*_tests.rs` siblings of production code, and never as a bare `src/<module>/tests.rs` file.

#### Wiring a `tests/<name>.rs` file into the module tree (mod-based only)

**Policy (as of the magic-elimination wave, commit on `refactor`):** the **only**
sanctioned wiring pattern is plain `mod <name>;` declarations. `#[path = "..."]`
on test modules and `include!("tests/<name>.rs")` inside test entry files are
both **forbidden** — owner directive: "no magic test wiring."

- `tests/mod.rs` declares `mod <name>;` for each sibling test file.
- Sub-test files access shared imports via `use super::*;` (which inherits the
  imports the entry `tests/mod.rs` brings into scope).
- When a test file needs **private items** of a production sibling module, do
  **not** re-root it via `#[path]`. Instead, raise the production item to
  `pub(super)` (narrowest scope that works) and import it explicitly:
  `use super::super::<module>::{item1, item2};`. `pub(super)` keeps the item
  invisible to other crates and to sibling production modules — only the
  parent module's subtree (which includes `tests::<name>`) can reach it.
- Example: `vespertide-query/src/sql/tests/helpers.rs` is a child of
  `sql::tests`. It accesses three `pub(super)` helpers in
  `sql/helpers.rs` via `use super::super::helpers::{parse_pg_type_cast,
  is_enum_type, needs_quoting};`.

**Rationale:** `#[path]` and `include!` hide the module tree from `cargo
modules`, `rustdoc`, and any tooling that walks `mod` declarations. The
`pub(super)` + explicit `use` pattern is fully transparent and self-documenting.

### `rstest` is the default for parametric tests
For backend / ORM / format / configuration matrices, use `rstest` with explicit case names so each case appears as its own `cargo test` row and produces its own snapshot.

```rust
use rstest::rstest;
use insta::{assert_snapshot, with_settings};

#[rstest]
#[case::postgres(DatabaseBackend::Postgres)]
#[case::mysql(DatabaseBackend::MySql)]
#[case::sqlite(DatabaseBackend::Sqlite)]
fn create_table_snapshot(#[case] backend: DatabaseBackend) {
    let sql = build_create_table(/* ... */).build(backend);
    with_settings!(
        { snapshot_suffix => format!("create_table_{backend:?}") },
        { assert_snapshot!(sql); }
    );
}
```

This is the same pattern used by `vespertide-query` (3 backends, 357 snapshots) and `vespertide-exporter` (4 ORMs via `Orm` enum, 232 cross-ORM snapshots). When adding a new backend / ORM / format, the change is **one `#[case::name(Value)]` line**.

### `#[cfg(test)]` test-oracle pattern
When a function exists solely as an oracle for a regression test (e.g. comparing
a fused/optimized pipeline against the equivalent unfused implementation), gate
it with `#[cfg(test)]` rather than `#[allow(dead_code)]`. Canonical example:
`vespertide-lsp/src/diagnostics/validation/visitors.rs` keeps
`collect_syntax_errors`/`collect_unknown_column_types`/etc. as `#[cfg(test)]`
oracles for the `fused_walk_matches_unfused_pipeline` test.

### NO TEST DELETION (policy)
Never delete or `#[ignore]` a failing test to make CI green. Fix the code, not
the test. Documented `#[ignore]` tests must include a concrete reason in a
`#[ignore = "..."]` attribute or adjacent comment.

## DATABASE BACKENDS

| Backend | Identifier Quoting | Notes |
|---------|-------------------|-------|
| PostgreSQL | `"identifier"` | Full feature support |
| MySQL | `` `identifier` `` | Full feature support |
| SQLite | `"identifier"` | Full feature support (ALTER limitations implemented via canonical temp-table-rebuild pattern in `query/src/sql/remove_constraint.rs` etc.) |

## MODEL FORMATS

Both JSON and YAML are supported for model and migration files. Loaders accept `.json`, `.yaml`, and `.yml` extensions. JSON is preferred (canonical schema URLs reference JSON) but YAML loading is a first-class, tested feature — see `vespertide-loader/src/models.rs` and `vespertide-config/src/file_format.rs`.

## NOTES

- Edition 2024 (bleeding edge)
- rust-analyzer is unreliable on this workspace (large macro expansions in `vespertide-macro` + cargo-flamegraph profile in `tools/lsp-profile` cause indexer churn); prefer `cargo check`, `cargo clippy`, ast-grep, and ripgrep over LSP-based navigation when iterating
- Every `.rs` file must stay ≤ 1000 lines; CI enforces this
- Migration replay pattern: baseline always reconstructed from history (raw SQL actions are opaque to replay)
- Wire format stability: JSON output of every newtype, action, and config struct must remain byte-identical to 0.1.x. Verify via the schema-drift command in COMMANDS section.
- `tools/lsp-profile`, `examples/app`, and `tests/runtime-sqlite` are out-of-workspace crates (separate `Cargo.lock`); see root `Cargo.toml` comment for the rationale

## RELEASE PROCESS

All release artefacts (crates.io publishes, LSP binaries, VSCode VSIX) ship
through a **single unified `changepacks` pipeline** in `.github/workflows/CI.yml`.
There is no separate `lsp-release.yml` or `vscode-release.yml`.

### How it works
1. **Author a changepack** locally before merging the PR:
   ```bash
   bunx @changepacks/cli      # → writes a markdown descriptor under .changepacks/
   ```
2. **Merge the PR.** CI runs the full quality gate (`fmt`, `clippy`, `test`,
   `coverage`, `deny`, `semver-checks`, etc.), then the `changepacks` job:
   - Bumps versions in every Cargo.toml / package.json listed in the descriptor
   - Creates a GitHub Release with the new tag
   - Runs `cargo publish` for every changed Rust crate (in dependency order)
   - Emits two outputs: `changepacks` (list of changed package files) and
     `release_assets_urls` (per-package upload URL into the new release)
3. **Conditional follow-up jobs** consume those outputs:
   - **`lsp-release`** (matrix × 5 platforms) — fires only when
     `crates/vespertide-lsp/Cargo.toml` is in the wave. Builds the
     `vespertide-lsp` binary natively + cross + windows, packages `tar.gz`/`zip`
     with `sha256`, uploads to the changepacks release.
   - **`vscode-release`** (matrix × 5 vsce targets) — fires only when
     `apps/vscode-extension/package.json` is in the wave. Pulls the matching
     LSP binary (just-released if LSP is also in the wave, otherwise the latest
     prior release), packages VSIX, uploads to the release, and publishes to
     **VS Code Marketplace** (`VSCE_PAT`) + **Open VSX** (`OVSX_PAT`).

### Configuration
- `.changepacks/config.json` — tracks `crates/**/Cargo.toml` (except
  `vespertide-schema-gen` which is `publish=false`) and
  `apps/vscode-extension/package.json`. `apps/landing`, `apps/zed-extension`,
  `tools/`, and `tests/` are intentionally not tracked.
- Required secrets: `CARGO_REGISTRY_TOKEN`, `VSCE_PAT`, `OVSX_PAT`.
- `.changepacks/changepack_log_*.json` runtime state is gitignored.

### Zed extension
Zed publishes happen out-of-band against the external `zed-industries/extensions`
repo and are not in this pipeline. Update `apps/zed-extension/extension.toml`
manually and open a PR against that repo when the LSP binary version moves.

## MUTATION TESTING

`cargo-mutants` runs in CI on every PR for changed lines only. Locally:

```bash
# Full pass on the planner crate (slow, ~30 min)
cargo install --locked cargo-mutants
cargo mutants -p vespertide-planner --in-place --timeout-multiplier 3.0

# Only mutations introduced by current changes
cargo mutants --in-diff <(git diff main..) --in-place
```

Survived mutants indicate test gaps. Fix by adding assertions, not by suppressing the mutant.

## FUZZING

`cargo-fuzz` runs on every `main` push via `.github/workflows/fuzz.yml`
(no cron schedule — `actions/cache` is immutable per SHA, so cron runs
on unchanged code can't persist their discovered corpus). For deep-fuzz
sessions, use `workflow_dispatch` with a larger `duration_seconds`.
Four targets in `fuzz/fuzz_targets/`:

- `fuzz_model_deser` — JSON deserialization of `TableDef` / `MigrationPlan`
- `fuzz_sql_identifier` — `quote_ident` safety invariants
- `fuzz_migration_apply` — `apply_action` never-panic property
- `fuzz_lsp_request` — LSP request handler sweep (9 capabilities) over random `model.json` bodies

Local run (requires nightly):

```bash
rustup install nightly
cargo install cargo-fuzz
cd fuzz
cargo +nightly fuzz run fuzz_model_deser -- -max_total_time=60
```

Corpus and artifacts are gitignored except the `.gitkeep` markers.
Discovered crashes appear under `fuzz/artifacts/<target>/` and should be
committed to a regression test before fixing.

## BENCHMARKS

`criterion` benchmarks in `crates/*/benches/`. Run locally:

```bash
# All benchmarks
cargo bench --workspace

# Single crate
cargo bench -p vespertide-planner

# Single benchmark with statistical comparison
cargo bench -p vespertide-planner --bench diff_benchmarks -- diff_identity/100
```

HTML reports at `target/criterion/<bench>/report/index.html`.

Save baseline for comparison:

```bash
cargo bench -- --save-baseline main
git checkout feature/foo
cargo bench -- --baseline main
```

CI workflow in `.github/workflows/bench.yml` runs on PR for informational
trend tracking (not currently blocking).
