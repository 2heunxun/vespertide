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
| `exporter/src/seaorm/relations.rs` | 996 | SeaORM FK relation resolution + sequential aggregation |
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
| `exporter/src/seaorm/tests.rs` | 990 | SeaORM codegen snapshots |
| `core/src/schema/table/tests.rs` | 986 | Table normalization tests |
| `exporter/src/sqlalchemy/tests.rs` | 988 | SQLAlchemy snapshots |
| `query/src/sql/delete_column/tests.rs` | 954 | DROP COLUMN tests |
| `planner/src/validate/tests/plan_validation.rs` | 954 | Plan validation tests |

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

Verify line policy: `python -c "import os, glob; files = []; [files.extend(glob.glob(os.path.join(r,'*.rs'))) for r,_,_ in os.walk('crates')]; over = [(sum(1 for _ in open(f, encoding='utf-8', errors='ignore')), f) for f in files]; result = sorted([x for x in over if x[0] > 1000], reverse=True); print('\n'.join(f'{l:5} {p}' for l, p in result) if result else 'OK: zero files >1000 lines')"`

## TESTING

- `rstest` for parameterized tests
- `serial_test::serial` for filesystem tests
- `insta` for snapshot testing (exporter crate)
- `proptest` for property-based testing (`vespertide-planner` diff + `vespertide-query` SQL)
- Helper functions: `col()`, `table()` reduce boilerplate
- **2135 tests across ~276 `.rs` files, 0 failed, 3 documented `#[ignore]`** (offline trybuild + 2 `///` doctest blocks)

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
