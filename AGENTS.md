# VESPERTIDE KNOWLEDGE BASE

**Generated:** 2026-01-07T01:39:00+09:00
**Commit:** d6c2411
**Branch:** export-with-python

## OVERVIEW

Rust workspace for declarative database schema management. Define schemas in JSON, diff against migration history, generate typed actions and SQL.

## STRUCTURE

```
vespertide/
├── crates/
│   ├── vespertide-core/      # Data structures: TableDef, ColumnDef, MigrationAction
│   ├── vespertide-planner/   # Schema diffing, baseline reconstruction, validation
│   ├── vespertide-query/     # SQL generation (Postgres/MySQL/SQLite)
│   ├── vespertide-cli/       # CLI commands: init, diff, sql, revision, export
│   ├── vespertide-exporter/  # ORM codegen: SeaORM, SQLAlchemy, SQLModel
│   ├── vespertide-loader/    # Filesystem loading of models/migrations
│   ├── vespertide-config/    # vespertide.json configuration
│   ├── vespertide-macro/     # Compile-time migration macro
│   ├── vespertide-naming/    # Naming convention utilities
│   ├── vespertide-schema-gen/# JSON Schema generation
│   └── vespertide/           # Re-export crate (user-facing API)
├── examples/app/             # Example project with models/migrations
├── schemas/                  # Generated JSON Schemas for IDE support
└── CLAUDE.md                 # Detailed implementation guidance
```

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Core types (TableDef, ColumnDef) | `vespertide-core/src/schema/` | Start with `table.rs`, `column.rs` |
| Column type system | `vespertide-core/src/schema/column.rs` | `ColumnType::Simple/Complex` variants |
| Migration actions | `vespertide-core/src/action.rs` | **14 action variants** (incl. `RawSql` escape hatch), `MigrationPlan` struct |
| Schema diffing | `vespertide-planner/src/diff.rs` | topological sort for FK deps |
| SQL generation | `vespertide-query/src/sql/` | One file per action type |
| CLI commands | `vespertide-cli/src/commands/` | `cmd_*` functions |
| ORM export | `vespertide-exporter/src/{seaorm,sqlalchemy,sqlmodel}/` | Backend-specific generators |
| Compile-time macro | `vespertide-macro/src/lib.rs` | `vespertide_migration!` proc macro |

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

### ColumnDef Initialization
ALL fields required including inline constraint fields:
```rust
ColumnDef {
    name, r#type, nullable, default, comment,
    primary_key: None,   // Must include
    unique: None,        // Must include  
    index: None,         // Must include
    foreign_key: None,   // Must include
}
```

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

## COMMANDS

```bash
# Build/Test
cargo build
cargo test
cargo clippy --all-targets --all-features
cargo fmt

# CLI (always use -p vespertide-cli)
cargo run -p vespertide-cli -- init
cargo run -p vespertide-cli -- new <model>
cargo run -p vespertide-cli -- diff
cargo run -p vespertide-cli -- sql
cargo run -p vespertide-cli -- revision -m "message"
cargo run -p vespertide-cli -- export --orm seaorm

# Regenerate JSON schemas
cargo run -p vespertide-schema-gen -- --out schemas

# Snapshot testing
cargo insta test -p vespertide-exporter
cargo insta accept
```

## COMPLEXITY HOTSPOTS (subject to 1000-line split)

| File | Lines | What |
|------|-------|------|
| `planner/src/diff.rs` | 4739 | Schema diffing with topological FK sort |
| `exporter/src/seaorm/mod.rs` | 4122 | SeaORM codegen with relation inference |
| `cli/src/commands/revision.rs` | 3064 | Revision generation, prompts, action emit |
| `planner/src/validate.rs` | 2299 | Schema/migration validation |
| `planner/src/apply.rs` | 1534 | Action replay onto baseline schema |
| `core/src/schema/table.rs` | 1526 | Table normalization logic |
| `query/src/sql/mod.rs` | 1507 | Dispatch and shared builder helpers |
| `query/src/sql/remove_constraint.rs` | 1465 | SQLite temp-table workarounds |
| `exporter/src/sqlalchemy/mod.rs` | 1383 | SQLAlchemy 2.x codegen |
| `query/src/sql/add_constraint.rs` | 1356 | PK/FK/Unique/CHECK emission |
| `exporter/src/sqlmodel/mod.rs` | 1274 | SQLModel/FastAPI codegen |
| `core/src/action.rs` | 1236 | 14 `MigrationAction` variants + helpers |
| `exporter/src/jpa/mod.rs` | 1122 | JPA codegen |
| `query/src/sql/delete_column.rs` | 1084 | DROP COLUMN with SQLite rebuild |

## TESTING

- `rstest` for parameterized tests
- `serial_test::serial` for filesystem tests
- `insta` for snapshot testing (exporter crate)
- Helper functions: `col()`, `table()` reduce boilerplate
- ~1289 tests across 53 files

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
- No LSP available - use grep/AST tools
- Every `.rs` file must stay ≤ 1000 lines; CI enforces this
- Migration replay pattern: baseline always reconstructed from history (raw SQL actions are opaque to replay)

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

`cargo-fuzz` runs in a nightly CI job (`.github/workflows/fuzz.yml`).
Three targets in `fuzz/fuzz_targets/`:

- `fuzz_model_deser` — JSON deserialization of `TableDef` / `MigrationPlan`
- `fuzz_sql_identifier` — `quote_ident` safety invariants
- `fuzz_migration_apply` — `apply_action` never-panic property

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
