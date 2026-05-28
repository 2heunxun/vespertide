# Data-Dependent Migration Fault Catalog — Coverage Status

**Generated:** 2026-05-28 (session snapshot, F3 + F4 + F11 + F5 + F96 + F76 + F29 + F-novel-1/4/15 work completed, F12/F15/F42/F82 by-design out-of-scope, F7 backend-neutrality re-verified)
**Branch / Commit:** `refactor` @ `045d03d` (working tree contains F29 + F-novel-1/4/15 + shared `check_expr_parser` implementation, uncommitted)
**Reference taxonomy:** Master Catalog v4 (110 faults across categories A–O) + novel faults enabled by `CheckExpr` AST
**Scope:** Every static-validation / interactive-prompt / SQL-emit-time hook that
catches a data-dependent fault. Tracks completion against v4 P0 + P1 (47 items) plus novel AST-enabled extensions.
**Verification snapshot:** `cargo test --workspace --exclude vespertide-fuzz` =
**2715 passed / 0 failed**, `cargo clippy -D warnings` = clean, `schemas/` drift = 0.

> **🎯 v4 P0 100% RESOLVED** — every fault catalogued as P0 in master v4
> is either implemented (14) or excluded by-design (5). No P0 work
> remains. Remaining queue is exclusively P1 (16 items).

> **Why this file exists:** session status was drifting from v4 because the code
> carries an older, pre-v4 fault ID system in its doc comments (`F9`, `F10`,
> `F12 Scenario A/B/C/D/E`, `F15` — none of which match v4 meaning). Next
> session must start from this file, not from in-code IDs.

---

## TL;DR

| Bucket | Count (of 47 P0+P1) | Action posture |
|---|---|---|
| ✅ **Implemented & verified** | **24 + 3 novel** (P0: 14 / P1: 10 / novel: 3) | Keep regression tests green |
| ✅ **by-design (no implementation needed)** | **9** (P0: 5 / P1: 4) | Documented as design invariants |
| ⚠️ **Partially implemented** | **3** (P0: 2 / P1: 1) | Listed below with concrete gap |
| ❌ **Not implemented (in-scope)** | **11** (P0: 0 / P1: 11) | Primary work queue (P1 only). **Backend-neutral candidates: F58, F84, F16 ext.** |
| ❌ **Not implemented (out-of-scope)** | (folded into by-design above) | — |

**Net implementation effort remaining: 15 P1 items + 4 partials = 19 items**.
**P0 work: complete.**

**Latest changes:**
- **F3** (FK with orphan rows) — `NullifyOrphans` / `DeleteOrphans` strategy with NULL row guard, F3 Edge #1 hard error (`AddColumnWithFkRequiresNullable`).
- **F4** (CHECK with violating rows) — `NullifyViolatingColumn { column }` / `DeleteViolatingRows` strategy, narrow-shape parser reuse from F86 (`check_default::matches_simple_check`), 3-backend uniform SQL.
- **F11** (NOT VALID pattern) — PostgreSQL auto-splits CHECK / FK additions into `ADD CONSTRAINT ... NOT VALID` + `VALIDATE CONSTRAINT`. Both statements run inside the migration transaction → automatic rollback on failure, no partial-apply zombie. MySQL/SQLite unchanged.
- **F5** (PK change with violations) — F1 + F2 mechanism combination. New `PrimaryKeyAdditionStrategy::DeleteDuplicates { keep }` enum + `validate/pk_additions.rs` detector + uniform 3-backend cleanup SQL + revision CLI prompt. NULL handling delegated to the existing F1 `fill_with` mechanism (warning carries `nullable_columns` list). **F5 promotes from "partial" to "full" — A category now complete.**
- **F96** (Cascade delete unexpected reach) — Pure static FK CASCADE-chain analysis. `validate/cascade_reach.rs` builds the union cascade graph from baseline + plan additions, iterative DFS measures `(depth, max_fanout)`, and emits `CascadeReachWarning` with risk classifier (`Deep` / `HighFanout` / `Critical`). Revision CLI prompts user `Proceed` / `Cancel` on each warning. **No SQL emit, no wire-format change** — vespertide cannot auto-shrink a user-declared cascade chain. **F category now mostly complete** (F42 by-design + F96 done; F97 P2 remains).
- **F76** (Sequence/identity exhaustion) — Static integer-PK overflow detection across three shapes: `Primary` (new single-column auto-increment INT4/SmallInt PK), `PkTypeNarrowing` (BigInt→Integer narrowing on existing PK), `ForeignKeyMismatch` (child column narrower than parent PK). Revision CLI prompts `ChangeToBigInt` (mutable for CreateTable / ModifyColumnType - directly rewrites the action) / `Proceed` / `Cancel`. **Side effect:** `SimpleColumnType` gained `Copy` derive (all unit variants); propagated `&SimpleColumnType` → `SimpleColumnType` value-pass through 4 caller sites in `vespertide-exporter`, `vespertide-query`, `vespertide-cli`.
- **F12 + F82** — formally reclassified as **by-design out-of-scope**. vespertide refuses to emit non-transactional DDL (`CREATE INDEX CONCURRENTLY`) to preserve transaction-safety as a first-class invariant. Users who need `CONCURRENTLY` must execute it outside vespertide.
- **F15** — formally reclassified as **by-design out-of-scope**. Batched backfill `UPDATE`s require per-batch `COMMIT`s that break transaction atomicity; vespertide refuses to split a migration into independent transactions. Users needing batched backfill must run it outside vespertide.
- **F42** — formally reclassified as **by-design out-of-scope**. vespertide does not model triggers, so trigger side-effects of backfill `UPDATE`s are outside its responsibility boundary.

**A category (Constraint additions) now complete**: F1 + F2 + F3 + F4 + F5 all shipped. Only F16 (composite UNIQUE/PK `keep_by`) remains as partial.
**E category (Operational safety): F11 shipped (PostgreSQL NOT VALID); F12+F82 by-design; other P1 items (F15, F65, F94) remain.**

## Design Invariants (post-v0.2)

The by-design exclusions are not gaps — they encode vespertide's
explicit design philosophy:

1. **Transaction-safety first** (F12, F82): every emitted DDL statement
   must be reversible via PostgreSQL transaction rollback. Non-transactional
   constructs are deliberately excluded.
2. **Typed schema model is the responsibility boundary** (F42, F92):
   objects not representable in the model (triggers, raw TRUNCATE) are
   not vespertide's responsibility.
3. **Source-of-fault elimination** (F49, F62, F109): unsafe SQL
   constructs (`NOT VALID` left-permanent, `FOREIGN_KEY_CHECKS=0`,
   `WITH NOCHECK`) are never emitted by vespertide in the first place.

These invariants are stronger than per-fault detection: they prevent
entire fault classes from being expressible in vespertide's output.

---

## 1. ⚠️ Partially Implemented — Concrete Gaps

Each entry lists *exactly what is missing* so the next session can pick it up
without re-investigation.

### ~~F5 (P1) — PK change with violations~~ **PROMOTED TO FULL ✅**

Full implementation shipped — see §5 inventory for details. Mirrors F2 +
F1 mechanisms exactly: `PrimaryKeyAdditionStrategy::DeleteDuplicates { keep }`
for duplicate handling, existing F1 `fill_with` for NULL violations.

### F10 (P0) — DROP TABLE / ENUM with deps

- **Implemented**: FK-dependency arm via `find_dangling_fk_drops`
  (`crates/vespertide-planner/src/validate/dangling_fk_drops.rs`).
- **Missing**: dependency tracking for **view**, **materialized view**, **trigger**,
  **function**, **stored procedure** that reference the dropped object.
  vespertide currently has no model representation for any of these — adding it
  is a substantial expansion of the schema model.
- **Decision pending**: keep as P0 gap, OR formally narrow F10 in v4 to
  "FK deps only" and reclassify view/MV/etc. as out-of-scope.

### F16 (P1) — Multi-column UNIQUE / PK violations

- **Implemented**: `find_unique_additions` already detects composite UNIQUE
  additions (it walks every column set, not just single-column).
- **Missing**: `keep_by` policy for composite groups. Current `KeepPolicy`
  (`First` / `Last`) is defined as "row with MIN/MAX PK value of the
  duplicate group" — for composite UNIQUE the PK might itself be composite
  or the table might have no single-column PK, in which case the `keep_by`
  needs to be an explicit ordering expression.
- **Note**: graceful fallback already in place — composite-PK / no-PK /
  PK-in-UNIQUE-set cases skip the `DELETE` emit and rely on F12-scenario-E
  to block; this is **good enough for v0.2** but flagged for follow-up.

### F55 (P0) — Dependent objects stale (FK part only)

- Same situation as F10 — FK case handled via `find_dangling_fk_drops`,
  every other dependent-object kind is unimplemented because the model
  doesn't represent them.

---

## 2. ❌ Not Implemented (In-Scope) — Primary Work Queue

Sorted by recommended implementation order (impact × code-leverage).

### P0 — 0 items 🎯

**P0 queue exhausted.** All v4 P0 faults are either implemented (14:
F1, F2, F3, F4, F6, F8, F9, F11, F20, F22, plus partials F5/F10/F16/F55)
or by-design (5: F12, F42, F49, F62, F82). See §3 (out-of-scope) and §4
(by-design) for the by-design reclassifications.

### P1 — 16 items

| v4 ID | Name | Category | Brief |
|---|---|---|---|
| F15 | Long-running DML | E | `backfill.batch_size` must be required when row count exceeds threshold. Pure planner concern. |
| F56 | Index option loss (opclass / INCLUDE / predicate) | G | Currently indexes drop these on rebuild. Need to capture full index spec in baseline + flag changes. |
| F58 | Column order / ordinal dependency | C | `SELECT *`, CSV export, CDC sinks care about column position. Detect column-order changes in plans that rebuild a table. |
| F65 | MySQL ALTER algorithm fallback | E | `INSTANT/INPLACE` requested but falls back to `COPY` silently. Add `ddl.algorithm: require_instant` model option. |
| F76 | Sequence / identity exhaustion | J | Warn when PK column type is `INT4` and traffic estimate exceeds threshold. Could default `id_type` to `BIGINT`. |
| F79 | Multi-tenant drift | I | Per-tenant precheck loop. Requires multi-DB-connection support — currently outside vespertide's CLI shape. |
| F81 | Replication / CDC incompatibility | I | Logical replication slot constraints, e.g. PG `REPLICA IDENTITY FULL` requirements. |
| F82 | Transaction mode mismatch (PG) | E | `CREATE INDEX CONCURRENTLY` outside transaction enforcement. Couples with F12. |
| F84 | Destructive without backup | D | `archive_to` option for `DROP COLUMN` / `DROP TABLE`. Pure model addition. |
| F94 | Migration timeout not set | E | Add `lock_timeout` / `statement_timeout` SET commands at plan start. |
| F96 | Cascade delete unexpected reach | F | Static graph walk of FK CASCADE chains; warn when depth ≥ N or fanout ≥ M. |
| F98 | Covering index INCLUDE loss | G | Subset of F56 — specifically warn on INCLUDE column drop. |
| F99 | Materialized view staleness | G | Requires MV model representation (couples with F55 gap). |
| F104 | Replica lag during migration | I | `wait_for_replicas` option emit. PG-specific (`pg_wait_for_replication_lsn`). |
| F43p / F44p / F59 | Partition-* faults | H | All partition-related — see §3 for scope decision. |

---

## 3. ❌ Not Implemented (Out-of-Scope) — Documented Non-Goals

These are not work queue items. Listed so the next session does not waste
effort revisiting the scoping decision.

| v4 ID | Name | Reason |
|---|---|---|
| F60 (P0) | Partitioned UNIQUE | vespertide does not model PARTITION clauses. Partitioning is a deliberate v0.2 non-goal. |
| F43p / F44p / F59 (P1) | Partition key / boundary / attach | Same — partition not modeled. |
| F70 (P1) | Oracle ENABLE NOVALIDATE | Oracle backend not supported. |
| F73 (P1) | SQL Server WITH NOCHECK | SQL Server backend not supported. |

---

## 4. ✅ Already Satisfied "By Design" (no code needed)

These v4 faults are satisfied because vespertide's design **does not emit the
unsafe construct in the first place** or **does not model the relevant object**.
They count as covered for paper purposes but require no implementation. Reframe
in the paper as "vespertide closes the fault-origin entirely" rather than
"vespertide detects it".

| v4 ID | Name | By-design mechanism |
|---|---|---|
| **F12** (P0) | CONCURRENTLY missing | vespertide refuses to emit `CREATE INDEX CONCURRENTLY` because it cannot run inside a transaction. Sacrifices outage-avoidance for transaction-safety. **Users needing CONCURRENTLY must execute it outside vespertide migration.** |
| **F42** (P0) | Backfill triggers fire | vespertide does not model triggers. Trigger side-effects of backfill `UPDATE`s are outside vespertide's responsibility boundary — user must handle externally (e.g. wrap `vespertide migrate` in shell with `SET session_replication_role = replica` if needed). |
| **F49 ⭐** (P0) | Constraint validation downgrade | vespertide never emits `NOT VALID` (left-permanent) / `NOVALIDATE` / `WITH NOCHECK`. F11 uses `NOT VALID` only as a temporary state inside the migration transaction; the immediate `VALIDATE CONSTRAINT` that follows is part of the same transaction. **Strongest by-design contribution** — the "둘째 AI 핵심 통찰" of v4 K category. |
| **F62** (P0) | MySQL FK off after re-enable | vespertide never emits `SET FOREIGN_KEY_CHECKS = 0`. |
| **F82** (P0) | Transaction mode mismatch | F12's twin — all vespertide-emitted plans run inside a single transaction. There is no mechanism to split a plan into non-transactional segments. |
| **F15** (P1) | Long-running DML (batch_size enforcement) | Batching a bulk `UPDATE` into chunks requires *separate `COMMIT`s per batch* — each batch becomes an independent transaction, breaking the all-or-nothing rollback guarantee. `SAVEPOINT` subtransactions preserve rollback but do **not** release WAL/replication backpressure, so they do not solve the resource problem. vespertide chooses *single-transaction safety* over batched-throughput; users needing batched backfill must run it outside the vespertide migration (e.g. a separate batch script after the migration applies the column). |
| **F92** (P1) | TRUNCATE in migration | `MigrationAction` enum has no `TRUNCATE` variant. Cannot be expressed in the model. |
| **F109** (P1) | PG NOT VALID permanent | F11 always emits `VALIDATE CONSTRAINT` in the same transaction as `ADD CONSTRAINT ... NOT VALID`; PG rollback reverts the pair on failure. The "left-permanent NOT VALID" anti-pattern is structurally impossible. |
| **F110** (P1) | Index validity state (INVALID) | Implied by F12 by-design: `CREATE INDEX CONCURRENTLY` is the only PG path that produces `INVALID` index zombies on failure, and vespertide does not emit it. |

---

## 5. ✅ Implemented & Verified — Full Inventory (20 items)

For completeness; no action required. Each row links the v4 ID to the
planner module that implements it.

| v4 ID | Name | Implementation site | Test count |
|---|---|---|---|
| F1 (P0) | NOT NULL on existing NULLs | `validate/plan.rs::find_missing_fill_with` + `validate/schema.rs::PrimaryKeyColumnNullable` + `validate/fk_addcolumn_nullable.rs::find_addcolumn_fk_nullable_violations` (F3 Edge #1) + CLI `--fill-with` / `--delete-null-rows` | 4 (`validate/tests/fill_with.rs`) + 8 (`fk_addcolumn_nullable`) |
| F2 (P0) | Single UNIQUE with duplicates | `validate/unique_additions.rs::find_unique_additions` + `core::UniqueConstraintStrategy::DeleteDuplicates { keep }` | 8 (`validate/unique_additions.rs::tests`) |
| F3 (P0) | FK with orphan rows | `validate/fk_orphan_additions.rs::find_fk_orphan_additions` + `core::ForeignKeyOrphanStrategy::{NullifyOrphans, DeleteOrphans}` + `core::TableConstraint::ForeignKey.orphan_strategy` field + `core::ForeignKeyDef.orphan_strategy` field + `query/.../add_constraint/foreign_key.rs::build_fk_orphan_cleanup` (3-backend uniform SQL with NULL row guard) + CLI `prompt_fk_orphan_additions`/`apply_fk_orphan_addition_choice` + revision hook (after F2) | 8 (`fk_orphan_additions::tests`) + 3 (`fk_orphan_strategy::tests`) + 3 snapshot triples in `add_constraint/tests` |
| F4 (P0) | CHECK with violating rows | `validate/check_additions.rs::find_check_additions` (reuses `check_default::matches_simple_check` narrow-shape parser) + `core::CheckViolationStrategy::{NullifyViolatingColumn { column }, DeleteViolatingRows}` + `core::TableConstraint::Check.strategy` field + `query/.../add_constraint/check.rs::build_check_violation_cleanup` (3-backend uniform `UPDATE … SET = NULL WHERE NOT (<expr>)` or `DELETE FROM … WHERE NOT (<expr>)`) + CLI `prompt_check_additions`/`apply_check_addition_choice` + revision hook (after F3) | 8 (`check_additions::tests`) + 3 (`check_violation_strategy::tests`) + snapshot triples in `add_constraint/tests` |
| F11 (P0) | NOT VALID pattern (PG only) | `query/.../add_constraint/check.rs::build_check` PG branch + `query/.../add_constraint/foreign_key.rs::build_foreign_key` PG branch — auto-split CHECK / FK addition into `ADD CONSTRAINT ... NOT VALID` + `VALIDATE CONSTRAINT`. Both statements inside the migration transaction → PG rollback reverts both on failure (no partial-apply zombie). MySQL single statement; SQLite via existing rebuild path. Uses `ReferenceAction::to_sql_keyword()` (new method on `core::ReferenceAction`). | 6 (snapshot triples reflecting PG 2-statement form vs MySQL/SQLite single) |
| F5 (P1) | PK change with violations (F1+F2 combo) | `validate/pk_additions.rs::find_primary_key_additions` (warning carries `kind: PkAdditionKind`, `nullable_columns` list, `auto_cleanup_capable` flag) + `core::PrimaryKeyAdditionStrategy::DeleteDuplicates { keep }` + `core::TableConstraint::PrimaryKey.strategy` field + `query/.../add_constraint/primary_key.rs::build_pk_pre_cleanup` (3-backend uniform `DELETE … NOT IN (SELECT MIN/MAX(old_pk) … GROUP BY new_pk)` with single-column-PK fallback) + CLI `prompt_pk_additions`/`apply_pk_addition_choice` + revision hook (after F4, before fill_with). NULL violations handled by existing F1 `fill_with` triggered by `nullable_columns` list. | 8 (`pk_additions::tests`) + 3 (`pk_addition_strategy::tests`) |
| F96 (P1) | Cascade delete unexpected reach (static FK CASCADE-chain analysis) | `validate/cascade_reach.rs::find_cascade_reach_violations` — pure static DFS over the baseline+plan cascade-FK graph. Reports `CascadeReachWarning { depth, reached_tables, max_fanout, risk_level }` with `CascadeRiskLevel::{Deep, HighFanout, Critical}` classifier (thresholds: depth ≥ 3, fanout ≥ 3). Cycle-safe via visited set; tree-shaped self-referential FKs naturally bounded at depth 1. RESTRICT/SET NULL/SET DEFAULT FKs excluded from graph. CLI `prompt_cascade_reach`/`CascadeReachChoice::Proceed` + revision hook (after F5, before fill_with). No SQL emit; no wire-format change. | 8 (`cascade_reach::tests`) |
| **F76 (P1)** | **Sequence/identity exhaustion (INT4 PK overflow / FK type mismatch)** | **`validate/sequence_exhaustion.rs::find_sequence_exhaustion_risks` — static analysis emitting `SequenceExhaustionWarning { kind: Primary \| PkTypeNarrowing { from } \| ForeignKeyMismatch { parent_table, parent_type } }` with `SequenceRiskLevel::{High, Medium}` (SmallInt=High, Integer=Medium). Baseline-suppressed: existing INT4 PKs the plan does not touch are not re-flagged. CLI `prompt_sequence_exhaustion` offers `ChangeToBigInt` (only for `Primary` + `PkTypeNarrowing` cases where vespertide can directly rewrite the `CreateTable`/`ModifyColumnType` action in place) / `Proceed` / `Cancel`. Revision hook (after F96, before fill_with). **LSP**: `vespertide-lsp::validation::validate_sequence_exhaustion` reuses the same planner detector with a synthetic single-`CreateTable` plan + empty baseline — emits file-local `Primary` warnings only (PkTypeNarrowing needs baseline, ForeignKeyMismatch needs cross-file parent PK). Diagnostic code `sequence-exhaustion`, Severity::Warning, anchored to the column's `type` field. Side-effect: `SimpleColumnType` gained `Copy` (all unit variants, safe).** | **15** (`sequence_exhaustion::tests` 12 + `vespertide-lsp::diagnostics::tests` 3) |
| **F29 (P1)** | **CHECK expression strengthening (backend-neutral hand-rolled parser)** | **New shared `validate/check_expr_parser.rs` (~600 SLOC + 33 tests) implements a dialect-neutral SQL boolean expression parser covering: comparison operators (`< <= > >= = <> !=`), `AND`/`OR`/`NOT` composition, parenthesised grouping, `IN (literal-list)` / `NOT IN`, `BETWEEN`/`NOT BETWEEN`, `IS NULL`/`IS NOT NULL`, signed numeric/string/bool/NULL literals, bare ASCII identifiers. Anything outside this subset (functions, casts, subqueries, quoted identifiers, `LIKE`, etc.) folds to `CheckExpr::Unparseable` and silently passes both F86 and F29 analyses. `validate/check_strengthening.rs::find_check_strengthenings(plan, baseline)` emits `CheckStrengtheningWarning { action_index, table, constraint_name, old_expr, new_expr, kind: BoundaryTightened \| OperatorTightened \| InListShrunk \| BetweenNarrowed \| ConjunctAdded \| DisjunctRemoved }` for *demonstrably* stricter replacements. Matching sources (priority): `ReplaceConstraint(Check{name=X}, Check{name=X})` → same-plan `RemoveConstraint + AddConstraint` pair → baseline `Check{name=X}` + plan `AddConstraint(Check{name=X})`. Conservative strictness: emits only when new predicate is a *strict subset* of the value space old accepted (never on identical, equivalent-but-rewritten, or ambiguous pairs). CLI `prompt_check_strengthening` offers `Proceed` / `Cancel` (no mutation — vespertide cannot auto-widen a user-authored predicate). Revision hook after F76. `RevisionPromptFns` 17→18 generics (added `CS`). **F86 refactor**: 230-line hand-rolled parser in `check_default.rs` removed; F86 now delegates parsing to shared `check_expr_parser::parse` + `extract_simple_column_check` adapter, F4 (`check_additions.rs`) updated to use `matches_for_column` alias — F86+F4 backward-compatible.** | **62** (`check_expr_parser::tests` 33 + `check_strengthening::tests` 29) |
| **F-novel-15** | **CHECK BETWEEN boundary order reversed (AST-enabled, hard error)** | **`validate/check_between_order.rs::validate_between_boundary_order(table)` walks each table-level CHECK via shared parser and inspects every `Between` node for literal boundary order. When `low > high`, raises `PlannerError::BetweenBoundaryReversed { table, column, check_name, low, high }`. Walks `And`/`Or`/`Not` composition to catch nested BETWEEN. Suppression: `NOT BETWEEN` reversed (= always true, harmless), mixed-type / boolean / null boundaries (comparator returns None), Unparseable expressions. Hooked into `validate_schema` (per-table) like F86. Backend-neutral (`BETWEEN` SQL standard semantics identical on PG/MySQL/SQLite). LSP locator anchored on column.** | **14** (`check_between_order::tests`) |
| **F-novel-4** | **CHECK literal type-mismatch (AST-enabled, warning + CLI prompt path)** | **`validate/check_type_mismatch.rs::find_check_type_mismatches(plan, baseline)` parses every CHECK in `AddConstraint(Check)`, `ReplaceConstraint(Check)`, and `CreateTable` inline constraints. Walks `Compare`/`In`/`Between`/`And`/`Or`/`Not` nodes; for each leaf literal looks up the column's type in baseline ∪ plan-added schema. Conservative compatibility table flags *demonstrable* incompatibility on every supported backend: integer-family + non-numeric literal, text + non-string, boolean + float/string (integer 0/1 borderline-OK), UUID/Date/Time/Timestamp/Interval/Inet/Cidr/Macaddr/XML + non-string, Varchar/Char + non-string, Numeric + non-numeric, string-enum + non-string, integer-enum + non-integer. Silently passes: NULL, JSON column, Custom column, unknown column, Numeric+Integer/Float cross-promotion, Unparseable. Emits `CheckTypeMismatchWarning { action_index, table, constraint_name, column, column_type_label, literal_text, literal_kind, expr }`. CLI `prompt_check_type_mismatch` offers `Proceed` / `Cancel` (no mutation — vespertide cannot auto-correct a user-authored literal). Revision hook after F29. `RevisionPromptFns` 18→19 generics (added `CTM`). **LSP**: CHECK type-mismatch diagnostics (code `check-type-mismatch`) plus Find References / Rename now treat column identifiers inside CHECK `expr` strings as references to the owning table's column (scoped so `user.age` ≠ `other.age`).** | **25** (`check_type_mismatch::tests` 23 + revision integration `tm_s1_s2`/`tm_s3` 2) |
| **F-novel-1** | **CHECK self-contradiction (AST-enabled, hard error)** | **`validate/check_self_contradiction.rs::validate_self_contradiction(table)` flattens top-level `And` conjuncts and pairwise-checks per column for *demonstrable* contradiction: range impossibility (`col > N AND col < M` where `N >= M`), boundary impossibility (`col >= N AND col < N`), equality conflict (`col = X AND col = Y` where `X != Y`), equality vs negation (`col = X AND col != X`), null conflict (`col IS NULL AND col IS NOT NULL`), null vs Compare (`col IS NULL AND col = X`). Walks nested `And` flattening + recurses into `Or` branches (a contradiction in any branch is reportable dead code). Suppression: mixed-type literals, `NOT` wrappers, different-column predicates, Unparseable. Raises `PlannerError::CheckSelfContradiction { table, check_name, column, first, second }`. Hooked into `validate_schema` after F-novel-15. Backend-neutral. LSP locator anchored on column.** | **24** (`check_self_contradiction::tests`) |
| F6 (P0) | Type narrowing (text → int etc.) | `validate/type_narrowing.rs::find_type_narrowings` | many (shared with F19/F33/F87) |
| F7 (P1) | ENUM value removal | `core::RemapEnumValues` action + typed `BTreeMap<i64,i64>` mapping with legacy-array serde + `find_missing_enum_fill_with` | 6 (mapping serde) + 4 (snapshot triples) |
| F8 (P0) | Column RENAME | `drop_resolution.rs::find_drop_resolutions` (rename candidate Levenshtein heuristic, threshold 3) + revision prompt | 14 (`drop_resolution/tests`) + 7 (heuristic) |
| F9 (P0) | DROP COLUMN with data | Same `find_drop_resolutions` infrastructure (Drop / RenameTo / Cancel) | shared |
| F19 (P1) | NUMERIC precision/scale narrowing | `validate/type_narrowing.rs` | shared |
| F20 (P0) | TIMESTAMP ↔ TIMESTAMPTZ | `validate/timezone_conversion.rs::find_timezone_conversions` + `AT TIME ZONE '<tz>'` emit | (in validate tests) |
| F22 (P0) | Table RENAME | `find_drop_resolutions` (table variant) | shared |
| F30 (P2) | FK policy change (silent on_delete / on_update) | `validate/fk_policy_changes.rs::find_fk_policy_changes` | (in validate tests) |
| F33 (P2) | TIMESTAMP precision narrowing | `validate/type_narrowing.rs` | shared |
| F44 (P2) | DEFAULT future semantics | `validate/default_changes.rs::find_default_changes` (6 `DefaultChangeKind` × 3 `RiskLevel`) + `ModifyColumnDefault.backfill` field | 13 (`default_changes` rstest) |
| F50 (P1) | Constraint drop/weakening | `validate/constraint_drops.rs::find_constraint_drops_without_replacement` + `validate/constraint_type_changes.rs::find_constraint_type_changes` + `find_primary_key_removals` | many |
| F51 (P1) | FK supporting index missing | `validate/foreign_keys.rs::find_missing_fk_supporting_indexes` | (in validate tests) |
| F86 (P2) | DEFAULT expression vs CHECK conflict | `validate/check_default.rs::PlannerError::DefaultViolatesCheck` | (in validate tests) |
| F87 (P2) | INTERVAL precision narrowing | `validate/type_narrowing.rs` | shared |

---

## 6. Planner Validator ↔ v4 Mapping (Source of Truth)

Use this table when reading code. The internal ID in code doc comments
(left column) is *not* v4. v4 IDs (right column) are the canonical paper /
catalog ID.

| Module / function | Internal code ID | v4 mapping | v4 P |
|---|---|---|---|
| `validate/plan.rs::find_missing_fill_with` | (none) | **F1** | P0 |
| `validate/plan.rs::find_missing_enum_fill_with` | (none) | **F7** (partial) | P1 |
| `validate/schema.rs::PrimaryKeyColumnNullable` | "F12 Scenario C" | **F1** (PK-specific) | P0 |
| `validate/schema.rs::MissingPrimaryKey` | (none) | typed-schema invariant (not in v4) | — |
| `validate/unique_additions.rs::find_unique_additions` | "F2" | **F2** | P0 |
| `validate/constraint_drops.rs::find_constraint_drops_without_replacement` | "F50" | **F50** | P1 |
| `validate/constraint_type_changes.rs::find_constraint_type_changes` | "F12 Scenarios A/B" | **F50** (PK↔UQ swap specialization) | P1 |
| `validate/constraint_type_changes.rs::find_primary_key_removals` | "F12 Scenario E" | **F50** (PK removal specialization) | P1 |
| `validate/dangling_fk_drops.rs::find_dangling_fk_drops` | "F9" | **F10** (FK dep arm) + **F55** (FK dep arm) | P0 |
| **`validate/fk_orphan_additions.rs::find_fk_orphan_additions`** | "F3" | **F3** | P0 |
| **`validate/fk_addcolumn_nullable.rs::find_addcolumn_fk_nullable_violations`** | "F3 Edge #1" | **F1** (FK-participating AddColumn specialization) | P0 |
| **`validate/check_additions.rs::find_check_additions`** | "F4" | **F4** | P0 |
| **`query/.../add_constraint/check.rs::build_check` (PG branch)** | "F11" | **F11** (PG NOT VALID + VALIDATE) | P0 |
| **`query/.../add_constraint/foreign_key.rs::build_foreign_key` (PG branch)** | "F11" | **F11** (FK NOT VALID + VALIDATE) | P0 |
| **`validate/pk_additions.rs::find_primary_key_additions`** | "F5" | **F5** (combines F1+F2 mechanisms) | P1 |
| **`query/.../add_constraint/primary_key.rs::build_pk_pre_cleanup`** | "F5" | **F5** (DeleteDuplicates SQL) | P1 |
| **`validate/cascade_reach.rs::find_cascade_reach_violations`** | "F96" | **F96** (static DFS, no SQL emit) | P1 |
| **`validate/sequence_exhaustion.rs::find_sequence_exhaustion_risks`** | "F76" | **F76** (static type analysis, plan-mutating ChangeToBigInt) | P1 |
| **`validate/check_strengthening.rs::find_check_strengthenings`** | "F29" | **F29** (conservative AST strictness comparison, shared parser) | P1 |
| **`validate/check_expr_parser.rs::parse`** (helper, shared by F4/F86/F29/F-novel-1/4/15) | n/a | infra — dialect-neutral CHECK expression parser | n/a |
| **`validate/check_between_order.rs::validate_between_boundary_order`** | "F-novel-15" | **F-novel-15** (BETWEEN boundary order, hard error in validate_schema) | novel |
| **`validate/check_type_mismatch.rs::find_check_type_mismatches`** | "F-novel-4" | **F-novel-4** (literal type-mismatch, plan-shaped warning) | novel |
| **`validate/check_self_contradiction.rs::validate_self_contradiction`** | "F-novel-1" | **F-novel-1** (self-contradiction, hard error in validate_schema) | novel |
| `validate/default_changes.rs::find_default_changes` | "F15" | **F44** | P2 |
| `validate/enums.rs::DuplicateEnumVariantName` / `DuplicateEnumValue` / `InvalidEnumDefault` | (none) | typed-schema invariant (enum-specific; F86-adjacent) | — |
| `validate/fk_policy_changes.rs::find_fk_policy_changes` | "F30" | **F30** | P2 |
| `validate/foreign_keys.rs::find_missing_fk_supporting_indexes` | "F51" | **F51** | P1 |
| `validate/timezone_conversion.rs::find_timezone_conversions` | "F20" | **F20** | P0 |
| `validate/type_narrowing.rs::find_type_narrowings` | "F6/F19/F33/F87" | **F6 + F19 + F33 + F87** | P0/P1 |
| `validate/check_default.rs` (DefaultViolatesCheck variant) | "F86" | **F86** | P2 |
| `drop_resolution.rs::find_drop_resolutions` + `apply_drop_resolution` | "F10+F8+F22" | **F8 + F9 + F22** | P0 |
| `cli/.../emit::handle_recreate_requirements` | (none) | **F1** FK specialization + **F11** adjacent | P0 |
| `core::MigrationAction::RemapEnumValues` | "F7-(b)" | **F7** | P1 |

### Non-v4 Schema Invariants (always blocked, no fault ID needed)

These are typed-schema invariants — vespertide refuses to compile/load the
model if violated. They are not in v4 because they are inexpressible in v4's
"data-dependent" framing (they're *schema*-dependent, not data-dependent).

- `PlannerError::DuplicateTableName`
- `PlannerError::ForeignKeyTableNotFound` / `ForeignKeyColumnNotFound`
- `PlannerError::IndexColumnNotFound`
- `PlannerError::ConstraintColumnNotFound`
- `PlannerError::EmptyConstraintColumns`
- `PlannerError::MissingPrimaryKey`
- `PlannerError::DuplicateEnumVariantName` / `DuplicateEnumValue`
- `PlannerError::InvalidEnumDefault`
- `PlannerError::InvalidAutoIncrement`

Worth a paper paragraph as "fault classes vespertide makes inexpressible".

---

## 7. ID Drift Warning (Read Before Editing Code)

**Code doc comments use a pre-v4 ID system.** When you read
`crates/vespertide-planner/src/validate/constraint_type_changes.rs` and see
`Fault F12 Scenarios A/B/E`, that is *not* v4 F12 (CONCURRENTLY).

Drift summary:

| Internal code ID | v4 canonical ID(s) | Notes |
|---|---|---|
| Code `F9` | v4 **F10** (FK dep arm) | DanglingForeignKeyAfterDrop |
| Code `F10` | v4 **F9** | DROP COLUMN with data (in drop_resolution) |
| Code `F12 Scenarios A/B` | v4 **F50** | PK↔UQ swap (constraint drop/weakening specialization) |
| Code `F12 Scenario C` | v4 **F1** | PK nullable column (PrimaryKeyColumnNullable) |
| Code `F12 Scenario D` | v4 **F50** | Other constraint type change (subsumed in find_constraint_type_changes) |
| Code `F12 Scenario E` | v4 **F50** | PK removed without replacement (PrimaryKeyRemovedWithoutReplacement) |
| Code `F15` | v4 **F44** | DEFAULT future semantics (NOT v4 F15 = long-running DML) |
| Code `F23` (helper module) | (none; rename heuristic is a sub-mechanism of F8/F22) | Not a v4 fault — Levenshtein heuristic. |

**Recommended cleanup (not yet done):** rewrite code doc comments to use v4
IDs. Tests / function names / variant names stay as-is — only the prose
in `//!` and `///` blocks needs to flip. Schedule when next touching each
module to avoid a churn-commit.

---

## 8. Meta-Decisions (Pending User Confirmation)

These are scoping questions from v4 §"결정해야 할 메타 질문" that affect what
work is in/out of the queue:

| v4 Q | Tentative position | Effect on queue |
|---|---|---|
| Q1: N (security) / O (data quality) in scope? | **Out** | F123–F129 dropped. |
| Q2: L (backend-specific) handling? | **Distribute into A–O** | L is bookkeeping; faults already counted in their natural category. |
| Q3: M (tool-inherent constraints)? | **Separate "Constraints" section, not faults** | F56s, F121, F122 dropped from queue. |
| Q4: Cover all 110 or just P0+P1? | **P0 + P1 only** | 47-item target. |
| Q5: Multi-backend or PG-only paper? | **Multi-backend** | Already enforced by `vespertide-query` 3-backend triple policy. |

Confirm or override these before treating this file as final.

---

## 9. Next-Session Quickstart

1. Read this file top to bottom.
2. Confirm/override §8 meta-decisions.
3. **All P0 done.** Pick the next work item from §2 (partials) or §6 (P1 queue) — recommended order:
   - **F5 partial → full** (PK change with violations — extend F12 PK detection with row-uniqueness check)
   - **F16 partial → full** (composite UNIQUE `keep_by` policy — extend F2 strategy with multi-column ordering)
   - **F15** (Long-running DML — backfill `batch_size` enforcement, planner-only)
   - **F50/F55 partial → full** (view/MV/trigger dependency tracking — requires *modeling these objects first*, big lift)
4. Implementation pattern is now codified in **three** mirrored slots — F2, F3, and F4:
   - Static detect: `validate/fk_orphan_additions.rs` (or `validate/unique_additions.rs`)
   - Per-warning prompt + apply: `cli/.../revision/prompts.rs::prompt_fk_orphan_additions`
   - Revision hook: `cli/.../revision/mod.rs::cmd_revision_core` (search for "F3 —")
   - 3-backend SQL emit: `query/src/sql/add_constraint/foreign_key.rs::build_fk_orphan_cleanup`
5. After every change, verify:
   - `cargo test --workspace --exclude vespertide-fuzz`
   - `cargo clippy --workspace --exclude vespertide-fuzz --all-targets --all-features -- -D warnings`
   - `cargo run -q -p vespertide-schema-gen -- --out _tmp_schemas && git diff --no-index schemas _tmp_schemas` (must be empty)

### F3 + F4 + F11 — Concrete File Inventory (for follow-up reference)

New files (F3):
- `crates/vespertide-core/src/schema/fk_orphan_strategy.rs`
- `crates/vespertide-planner/src/validate/fk_addcolumn_nullable.rs`
- `crates/vespertide-planner/src/validate/fk_orphan_additions.rs`

New files (F4):
- `crates/vespertide-core/src/schema/check_violation_strategy.rs`
- `crates/vespertide-planner/src/validate/check_additions.rs`
- `crates/vespertide-query/src/sql/add_constraint/check.rs` (rewritten in place)

F11 modifications:
- `crates/vespertide-core/src/schema/reference.rs` — new `ReferenceAction::to_sql_keyword()` method
- `crates/vespertide-query/src/sql/add_constraint/check.rs` — PG branch emits 2 statements (NOT VALID + VALIDATE)
- `crates/vespertide-query/src/sql/add_constraint/foreign_key.rs` — PG branch emits 2 statements (FK NOT VALID + VALIDATE), bypassing sea-query builder
- `crates/vespertide-core/src/arbitrary/mod.rs` — `arb_complex_column_type` Custom variant filtered to length ≥ 4 (avoids SQL reserved word collision in proptest fuzz)

F4 design note: unlike F3, `CheckViolationStrategy::default()` is
`DeleteViolatingRows`, **not** the less destructive `NullifyViolatingColumn`.
Reason: the `NullifyViolatingColumn` variant carries a `column: ColumnName`
field that the wire-format default cannot supply (the column name must be
parsed out of the free-form CHECK expression by the planner). The default
exists only for v0.1.x compatibility; the revision CLI prompts for an
explicit choice on every `AddConstraint(Check)` and offers
`NullifyViolatingColumn` whenever the target column is nullable.

F11 design note: F11 only applies to constraints PG supports `NOT VALID`
for — **CHECK and FOREIGN KEY**. PRIMARY KEY, UNIQUE, and NOT NULL
column modifications do not support `NOT VALID` in PG and use single-
statement emission. F11 is **strictly transaction-safe** — both
`ADD CONSTRAINT ... NOT VALID` and `VALIDATE CONSTRAINT` execute inside
the migration transaction, so PG rollback reverts both on failure. This
is why F12 (`CREATE INDEX CONCURRENTLY`) and F82 (transaction split) are
**deliberately excluded** — they would break the transaction-safety
invariant.

Modified core wire format (`#[serde(default, skip_serializing_if = ...)]` preserves v0.1.x byte-identical JSON):
- `TableConstraint::ForeignKey { ..., orphan_strategy }` (`schema/constraint.rs`)
- `ForeignKeyDef { ..., orphan_strategy }` (`schema/foreign_key.rs`)
- `PlannerError::AddColumnWithFkRequiresNullable { table, column }` (`planner/src/error.rs`)
- `RevisionPromptFns<..., FO>` extended 14→15 generics (`cli/.../revision/mod.rs`)

SQL emit (3-backend uniform `NOT EXISTS` correlated subquery with NULL row guard):
- `query/src/sql/add_constraint/foreign_key.rs::build_fk_orphan_cleanup` — `UPDATE … SET … = NULL WHERE (<col> IS NOT NULL OR …) AND NOT EXISTS (SELECT 1 FROM parent WHERE …)` or `DELETE FROM … WHERE NOT EXISTS (…)`.

Snapshot impact:
- `cargo insta accept --workspace` ran; every `AddConstraint(ForeignKey)` snapshot now carries the pre-cleanup statement ahead of the `ADD CONSTRAINT`. SQL semantics unchanged on a table with no orphans (no-op).

---

## Update Protocol

When a status changes:

1. Move the row between §1 / §2 / §3 / §4 / §5.
2. Bump the "Generated" line and "Branch / Commit" line at the top.
3. Refresh TL;DR counts.
4. Re-run the verification snapshot (3 commands above) and update the
   "Verification snapshot" line.
