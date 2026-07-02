# vespertide-exporter

ORM code generation from `TableDef` schemas → SeaORM (Rust), SQLAlchemy (Python), SQLModel (Python), JPA (Java).

## STRUCTURE

```
src/
├── lib.rs              # Re-exports all backends
├── orm.rs              # OrmExporter trait, Orm enum (SeaOrm/SqlAlchemy/SqlModel/Jpa), dispatch
├── constraint_scan.rs  # Shared constraint scanning helpers
├── parallel_config.rs  # Rayon parallelism thresholds
├── python_naming.rs    # Shared Python identifier naming
├── seaorm/             # mod.rs, render.rs, types.rs, enums.rs, imports.rs,
│                       #   relations/ (fk_resolve, naming, self_ref, reverse), tests/
├── sqlalchemy/         # mod.rs, render.rs, types.rs, enums.rs — declarative_base models
├── sqlmodel/           # mod.rs, render.rs, types.rs, enums.rs — SQLModel + Pydantic models
├── jpa/                # mod.rs, render.rs, types.rs — JPA/Hibernate entities
├── utils/              # common.rs, python.rs
└── tests/              # Shared orm_cases! cross-ORM snapshot suite + fixtures/ + snapshots/
```

## WHERE TO LOOK

| Task | Location |
|------|----------|
| Add new ORM backend | Implement `OrmExporter` trait in new module |
| Type mapping (Rust) | `ColumnType::to_rust_type(nullable)` in `vespertide-core` |
| Type mapping (Python) | `UsedTypes` struct in each Python backend |
| Relation inference | `relation_field_defs_with_schema()`, `infer_field_name_from_fk_column()` |
| FK chain resolution | `resolve_fk_target()` follows FKs through intermediate tables |
| Enum generation | `render_enum()` in each backend |

## BACKEND NOTES

### SeaORM (Rust)
- **Relation inference**: `creator_user_id` → field name `creator_user`, relation enum `CreatorUser`
- **FK chains**: Follows FK→FK chains to find ultimate target table
- **Multiple FKs**: Generates `relation_enum` attribute when table has multiple FKs to same target
- **Output**: Entity, Model, ActiveModel, Column enum, Relation enum
- **Config**: `SeaOrmExporterWithConfig` for `extra_model_derives`

### SQLAlchemy (Python)
- Uses `declarative_base()` pattern
- `UsedTypes` tracks imports: `sa_types`, `datetime_types`, `needs_uuid`, etc.
- Generates `relationship()` for FKs, `__table_args__` for composite constraints

### SQLModel (Python)
- SQLAlchemy + Pydantic integration (`SQLModel` base class)
- Uses `Field()` instead of `Column()` with Pydantic-style defaults
- Lighter import tracking (no `sa_types` - uses native Python types)
- `sa_column_kwargs` for SQLAlchemy-specific options

### JPA (Java)
- Jakarta Persistence (`jakarta.persistence.*`) entity classes with `@Entity`/`@Table`/`@Column`
- Enum types render as Java `enum` + `@Enumerated`
- FK columns render as `@ManyToOne`/`@JoinColumn` relations

## TESTING

```bash
# Run all exporter tests
cargo test -p vespertide-exporter

# Update snapshots after changes
cargo insta test -p vespertide-exporter
cargo insta accept
```

- Snapshot testing with `insta` crate (YAML format)
- `rstest` for parameterized tests across all ORM backends
- 240 snapshot files across exporter snapshot directories; every export scenario goes through the shared `orm_cases!` macro in `src/tests/mod.rs`, producing one snapshot per ORM (all four) in `src/tests/snapshots/`

## NOTES

- YAML and JSON are both fully supported input formats; exporter tests also use YAML-formatted insta snapshots.
- Generated ORM files are outputs only; edit Vespertide models, then regenerate.
- Two-tier line policy (CI-enforced via `scripts/check-line-budget.sh`): production-only `.rs` ≤ 1000 lines; files carrying test code (`tests/` dir or inline `#[cfg(test)] mod tests`) ≤ 1200 lines.
- Workspace lints warn on unsafe code and Clippy all: `unsafe_code = "warn"`, `clippy::all = { level = "warn", priority = -1 }`.
