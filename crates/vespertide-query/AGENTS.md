# vespertide-query

Converts `MigrationAction` enums to SQL via sea-query intermediate representation.

## STRUCTURE

```
src/
├── lib.rs              # Re-exports: build_action_queries, BuiltQuery, DatabaseBackend
├── builder.rs          # build_plan_queries() - orchestrates full plan with schema evolution
├── error.rs            # QueryError enum
└── sql/
    ├── mod.rs          # build_action_queries() dispatch - matches all 14 MigrationAction variants (1507 lines)
    ├── types.rs        # BuiltQuery (11 variants), DatabaseBackend, RawSql
    ├── helpers.rs      # Column type mapping, FK actions, enum handling, naming
    ├── create_table.rs # build_create_table(), build_create_table_for_backend()
    ├── add_column.rs   # Temp table for SQLite non-nullable/enum columns
    ├── add_constraint.rs     # Constraint SQL generation (1356 lines)
    ├── remove_constraint.rs  # Complex SQLite temp table workarounds (1465 lines)
    ├── modify_column_*.rs    # type, nullable, default, comment handlers
    ├── rename_*.rs     # Simple ALTER statements
    ├── delete_column.rs # DROP COLUMN with SQLite rebuild (1084 lines)
    ├── delete_*.rs     # Other DROP statements
    └── raw_sql.rs      # RawSql emergency escape hatch
```

## WHERE TO LOOK

| Task | File | Key Function |
|------|------|--------------|
| Add new action handler | `sql/mod.rs` | Add to `build_action_queries()` match |
| Column type mapping | `sql/helpers.rs` | `apply_column_type_with_table()` |
| SQLite workarounds | `sql/remove_constraint.rs` | `{table}_temp` pattern |
| Backend-specific emergency SQL | `sql/types.rs` | `RawSql::per_backend()` |
| Default value conversion | `sql/helpers.rs` | `convert_default_for_backend()` |
| Enum type handling | `sql/helpers.rs` | `build_create_enum_type_sql()` |

## CONVENTIONS

```rust
// BuiltQuery wraps sea-query statements - call .build(backend) for SQL string
let query = BuiltQuery::CreateTable(Box::new(stmt));
let sql = query.build(DatabaseBackend::Postgres);

// Custom SQL without bind params - use Expr::cust()
col.default(Expr::cust("CURRENT_TIMESTAMP"));

// Emergency escape hatch only; prefer typed MigrationAction handlers
BuiltQuery::Raw(RawSql::per_backend(pg_sql, mysql_sql, sqlite_sql))

// SQLite temp table pattern (for ALTER limitations):
// 1. CREATE TABLE {table}_temp (new schema)
// 2. INSERT INTO {table}_temp SELECT ... FROM {table}
// 3. DROP TABLE {table}
// 4. ALTER TABLE {table}_temp RENAME TO {table}
// 5. Recreate indexes
```

## ANTI-PATTERNS

| Pattern | Why Bad |
|---------|---------|
| Direct SQL string building | Use sea-query builders, wrap in `BuiltQuery` |
| Using bind parameters | Not supported - use `Expr::cust()` for literals |
| Ignoring SQLite for constraints | SQLite needs temp table for PK/UNIQUE/FK/CHECK changes |
| Forgetting index recreation | After SQLite temp table rename, indexes are lost |
| Skipping `current_schema` param | Required for SQLite temp table to know column list |

## NOTES

- `build_action_queries()` is exhaustive for all 14 `MigrationAction` variants: CreateTable, DeleteTable, AddColumn, RenameColumn, DeleteColumn, ModifyColumnType, ModifyColumnNullable, ModifyColumnDefault, ModifyColumnComment, AddConstraint, RemoveConstraint, ReplaceConstraint, RenameTable, RawSql.
- Prefer typed `MigrationAction` enums; `RawSql` exists as a documented emergency escape hatch, but is not recommended for normal use.
- SQLite has full feature support via temp-table-rebuild workarounds for ALTER limitations.
- YAML and JSON are both fully supported upstream input formats.
- Every `.rs` file must stay ≤ 1000 lines (CI enforced); current hotspots are `sql/mod.rs` (1507), `remove_constraint.rs` (1465), `add_constraint.rs` (1356), and `delete_column.rs` (1084).
- Workspace lints warn on unsafe code and Clippy all: `unsafe_code = "warn"`, `clippy::all = { level = "warn", priority = -1 }`.
