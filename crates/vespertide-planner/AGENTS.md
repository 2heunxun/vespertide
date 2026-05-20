# vespertide-planner

Schema diffing engine - compares baseline vs target schema to emit typed migration actions.

## STRUCTURE

```
src/
├── diff.rs      # 4739 lines, scheduled for split - Schema comparison, topological sort
├── validate.rs  # 2299 lines, scheduled for split - Schema/plan validation
├── apply.rs     # 1617 lines, scheduled for split - Apply actions to in-memory schema
├── schema.rs    # Replay migrations → baseline schema
├── plan.rs      # High-level planning API
└── error.rs     # PlannerError enum
```

## WHERE TO LOOK

| Task | File | Key Functions |
|------|------|---------------|
| Compare schemas | `diff.rs` | `diff_schemas()` |
| Replay migrations | `schema.rs` | `schema_from_plans()` |
| One-shot planning | `plan.rs` | `plan_next_migration()` |
| Apply single action | `apply.rs` | `apply_action()` |
| Validate schema | `validate.rs` | `validate_schema()`, `validate_migration_plan()` |
| FK dependency sort | `diff.rs` | `topological_sort_tables()`, `sort_delete_tables()` |

## ALGORITHM NOTES

**Diffing Flow:**
1. Normalize both schemas (inline constraints → table-level)
2. Use BTreeMaps for deterministic iteration order
3. Detect: deleted tables, modified columns, added columns, constraint changes
4. Topologically sort CreateTable by FK dependencies (Kahn's algorithm)
5. Reverse-sort DeleteTable (dependents deleted first)

**Topological Sort (Kahn's):**
- Build adjacency list from FK references
- Track in-degree (dependency count) per table
- Process zero-dependency tables first
- Detect cycles via incomplete result

**Normalization Critical:** Both schemas normalized before comparison so inline `unique: true` equals table-level `Unique { columns: [...] }`.

## ANTI-PATTERNS

| Pattern | Problem |
|---------|---------|
| Comparing without normalize | Inline vs table-level constraints won't match |
| Using HashMap in diff | Non-deterministic action ordering |
| Ignoring topological sort | FK constraint violations on CREATE/DELETE |
| Forgetting `fill_with` validation | NOT NULL columns without defaults fail |

## NOTES

- YAML and JSON are both fully supported for models and migrations.
- Prefer typed `MigrationAction` enums; `RawSql` exists as a documented emergency escape hatch, but is opaque to baseline replay and not recommended for normal use.
- Every `.rs` file must stay ≤ 1000 lines (CI enforced); current planner hotspots are `diff.rs` (4739), `validate.rs` (2299), and `apply.rs` (1617).
- Workspace lints warn on unsafe code and Clippy all: `unsafe_code = "warn"`, `clippy::all = { level = "warn", priority = -1 }`.
