use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, QueryResult, Statement, TransactionTrait,
};

use crate::MigrationError;

fn database_error(message: String, source: sea_orm::DbErr) -> MigrationError {
    MigrationError::Database {
        message,
        source: Some(Box::new(source)),
    }
}

/// Compiled migration with per-backend SQL byte arrays.
/// A single migration baked into the binary at compile time by the `vespertide_migration!` macro.
///
/// Each `EmbeddedMigration` holds three pre-compiled SQL blobs (one per supported backend).
/// Individual SQL statements within a blob are separated by null bytes (`\0`); use
/// [`split_sql_blob`] to iterate over them.
///
/// You do not construct `EmbeddedMigration` values manually. The `vespertide_migration!` macro
/// generates a `const` array of these and passes it to [`run_embedded_migrations`].
#[derive(Debug, Clone, Copy)]
pub struct EmbeddedMigration {
    /// Monotonically increasing version number matching the migration file.
    pub version: u32,
    /// UUID identifying this specific migration plan, used to detect history divergence.
    pub migration_id: &'static str,
    /// Human-readable description of what this migration does.
    pub comment: &'static str,
    /// Null-byte-delimited SQL statements for `PostgreSQL`.
    pub postgres_sql_blob: &'static str,
    /// Null-byte-delimited SQL statements for `MySQL`.
    pub mysql_sql_blob: &'static str,
    /// Null-byte-delimited SQL statements for `SQLite`.
    pub sqlite_sql_blob: &'static str,
}

impl EmbeddedMigration {
    /// Construct an embedded migration; called by the `vespertide_migration!` macro expansion. Not intended for hand-written use.
    pub const fn new(
        version: u32,
        migration_id: &'static str,
        comment: &'static str,
        postgres_sql_blob: &'static str,
        mysql_sql_blob: &'static str,
        sqlite_sql_blob: &'static str,
    ) -> Self {
        Self {
            version,
            migration_id,
            comment,
            postgres_sql_blob,
            mysql_sql_blob,
            sqlite_sql_blob,
        }
    }

    pub const fn sql_blob(self, backend: DatabaseBackend) -> &'static str {
        if matches!(backend, DatabaseBackend::MySql) {
            self.mysql_sql_blob
        } else if matches!(backend, DatabaseBackend::Sqlite) {
            self.sqlite_sql_blob
        } else {
            self.postgres_sql_blob
        }
    }
}

pub fn split_sql_blob(blob: &str) -> impl Iterator<Item = &str> {
    blob.split_terminator('\0').filter(|sql| !sql.is_empty())
}

/// Runtime knobs for [`run_embedded_migrations_with_options`] (fault F94).
///
/// Both timeouts are optional and expressed in **milliseconds**. `None`
/// leaves the backend default untouched. The values are rendered to a
/// backend-appropriate statement injected at the start of the migration
/// session so a migration that blocks on a lock (or runs away) fails fast
/// instead of hanging the database.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct MigrationRuntimeOptions {
    /// Max time (ms) to wait acquiring a lock before failing.
    /// `PostgreSQL` `lock_timeout`, `MySQL` `innodb_lock_wait_timeout`
    /// (rounded up to whole seconds), `SQLite` `PRAGMA busy_timeout`.
    pub lock_timeout_ms: Option<u64>,
    /// Max time (ms) a single statement may run. `PostgreSQL`
    /// `statement_timeout`, `MySQL` `max_execution_time`. `SQLite` has no
    /// statement timeout, so this is skipped on `SQLite`.
    pub statement_timeout_ms: Option<u64>,
}

impl MigrationRuntimeOptions {
    /// Construct from optional millisecond timeouts.
    ///
    /// This is the stable constructor used by `vespertide_migration!`
    /// macro-generated code: the struct is `#[non_exhaustive]`, so user
    /// crates cannot build it with a struct literal and must call this.
    #[must_use]
    pub const fn from_millis(
        lock_timeout_ms: Option<u64>,
        statement_timeout_ms: Option<u64>,
    ) -> Self {
        Self {
            lock_timeout_ms,
            statement_timeout_ms,
        }
    }

    /// True when neither timeout is configured (no SET statements emitted).
    fn is_noop(self) -> bool {
        self.lock_timeout_ms.is_none() && self.statement_timeout_ms.is_none()
    }
}

/// Render the connection-level (pre-transaction) timeout statements for a
/// backend. Only `SQLite` needs this (`PRAGMA busy_timeout` is connection
/// scoped and cannot run inside a transaction).
fn pre_txn_timeout_sql(backend: DatabaseBackend, options: MigrationRuntimeOptions) -> Vec<String> {
    let mut out = Vec::new();
    if matches!(backend, DatabaseBackend::Sqlite)
        && let Some(ms) = options.lock_timeout_ms
    {
        out.push(format!("PRAGMA busy_timeout = {ms}"));
    }
    out
}

/// Render the transaction-level timeout statements for a backend
/// (`PostgreSQL` `SET LOCAL`, `MySQL` `SET SESSION`). `SQLite`'s lock timeout
/// is handled pre-transaction; `SQLite` has no statement timeout.
fn in_txn_timeout_sql(backend: DatabaseBackend, options: MigrationRuntimeOptions) -> Vec<String> {
    let mut out = Vec::new();
    match backend {
        DatabaseBackend::Postgres => {
            if let Some(ms) = options.lock_timeout_ms {
                out.push(format!("SET LOCAL lock_timeout = {ms}"));
            }
            if let Some(ms) = options.statement_timeout_ms {
                out.push(format!("SET LOCAL statement_timeout = {ms}"));
            }
        }
        DatabaseBackend::MySql => {
            if let Some(ms) = options.lock_timeout_ms {
                // MySQL innodb_lock_wait_timeout is in SECONDS; round up so a
                // sub-second config never collapses to 0 (= "no wait").
                let secs = ms.div_ceil(1000).max(1);
                out.push(format!("SET SESSION innodb_lock_wait_timeout = {secs}"));
            }
            if let Some(ms) = options.statement_timeout_ms {
                out.push(format!("SET SESSION max_execution_time = {ms}"));
            }
        }
        // SQLite lock timeout handled pre-transaction; no statement timeout.
        // sea_orm::DatabaseBackend is #[non_exhaustive] → catch-all required.
        _ => {}
    }
    out
}

pub async fn run_embedded_migrations(
    pool: &DatabaseConnection,
    version_table: &str,
    verbose: bool,
    migrations: &[EmbeddedMigration],
) -> Result<(), MigrationError> {
    run_embedded_migrations_with_options(
        pool,
        version_table,
        verbose,
        migrations,
        MigrationRuntimeOptions::default(),
    )
    .await
}

/// Like [`run_embedded_migrations`] but applies the timeout knobs in
/// [`MigrationRuntimeOptions`] (fault F94) at the start of the migration
/// session. Backend-appropriate SET / PRAGMA statements are emitted so a
/// migration cannot hang indefinitely on a lock or a runaway statement.
///
/// [`run_embedded_migrations`] delegates here with default (no-timeout)
/// options, so existing callers are unaffected.
#[expect(
    clippy::print_stderr,
    reason = "verbose runtime migrations stream progress diagnostics to stderr while leaving host stdout application-owned"
)]
#[expect(
    clippy::too_many_lines,
    reason = "linear runtime migration flow: version-table setup -> pre/in-txn timeouts -> version read -> apply loop -> commit; splitting fragments the transaction lifecycle"
)]
pub async fn run_embedded_migrations_with_options(
    pool: &DatabaseConnection,
    version_table: &str,
    verbose: bool,
    migrations: &[EmbeddedMigration],
    options: MigrationRuntimeOptions,
) -> Result<(), MigrationError> {
    let backend = pool.get_database_backend();
    let q = if matches!(backend, DatabaseBackend::MySql) {
        '`'
    } else {
        '"'
    };

    let create_table_sql = format!(
        "CREATE TABLE IF NOT EXISTS {q}{version_table}{q} (version INTEGER PRIMARY KEY, id TEXT DEFAULT '', created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP)"
    );
    let stmt = Statement::from_string(backend, create_table_sql);
    pool.execute_raw(stmt)
        .await
        .map_err(|e| database_error(format!("Failed to create version table: {e}"), e))?;

    let alter_sql = format!("ALTER TABLE {q}{version_table}{q} ADD COLUMN id TEXT DEFAULT ''");
    let stmt = Statement::from_string(backend, alter_sql);
    let _ = pool.execute_raw(stmt).await;

    // F94: connection-level timeouts that cannot run inside a transaction
    // (SQLite `PRAGMA busy_timeout`) are applied here, before `begin()`.
    if !options.is_noop() {
        for sql in pre_txn_timeout_sql(backend, options) {
            if verbose {
                eprintln!("[vespertide] {sql}");
            }
            let stmt = Statement::from_string(backend, sql.clone());
            pool.execute_raw(stmt)
                .await
                .map_err(|e| database_error(format!("Failed to apply timeout '{sql}': {e}"), e))?;
        }
    }

    let txn = pool
        .begin()
        .await
        .map_err(|e| database_error(format!("Failed to begin transaction: {e}"), e))?;

    // F94: transaction-scoped timeouts (PostgreSQL `SET LOCAL`, MySQL
    // `SET SESSION`) are applied right after BEGIN, before any lock-taking
    // statement, so they cover the whole migration.
    if !options.is_noop() {
        for sql in in_txn_timeout_sql(backend, options) {
            if verbose {
                eprintln!("[vespertide] {sql}");
            }
            let stmt = Statement::from_string(backend, sql.clone());
            txn.execute_raw(stmt)
                .await
                .map_err(|e| database_error(format!("Failed to apply timeout '{sql}': {e}"), e))?;
        }
    }

    let select_sql = format!("SELECT MAX(version) as version FROM {q}{version_table}{q}");
    let stmt = Statement::from_string(backend, select_sql);
    let version_result = txn
        .query_one_raw(stmt)
        .await
        .map_err(|e| database_error(format!("Failed to read version: {e}"), e))?;
    let version_i32 = version_result
        .and_then(|row| row.try_get::<i32>("", "version").ok())
        .unwrap_or(0);
    // Migration versions are generated by Vespertide as non-negative u32 values;
    // treat a corrupt negative database value as no applied migration.
    let version = u32::try_from(version_i32).unwrap_or(0);

    let select_ids_sql = format!("SELECT version, id FROM {q}{version_table}{q}");
    let stmt = Statement::from_string(backend, select_ids_sql);
    let id_rows = txn
        .query_all_raw(stmt)
        .await
        .map_err(|e| database_error(format!("Failed to read version ids: {e}"), e))?;
    let version_ids = collect_version_ids(&id_rows);

    if verbose {
        eprintln!("[vespertide] Current database version: {version}");
    }

    for migration in migrations {
        if version >= migration.version {
            continue;
        }

        if let Some(db_id) = version_ids.get(&migration.version)
            && !migration.migration_id.is_empty()
            && !db_id.is_empty()
            && db_id != migration.migration_id
        {
            return Err(MigrationError::IdMismatch {
                version: migration.version,
                expected: migration.migration_id.to_string(),
                found: db_id.clone(),
            });
        }

        if verbose {
            eprintln!(
                "[vespertide] Applying migration v{} ({})",
                migration.version, migration.comment
            );
        }

        let sql_blob = migration.sql_blob(backend);
        let sqls: Vec<_> = split_sql_blob(sql_blob).collect();

        for (sql_idx, sql) in sqls.iter().enumerate() {
            if verbose {
                eprintln!("[vespertide]   [{}/{}] {}", sql_idx + 1, sqls.len(), sql);
            }

            let stmt = Statement::from_string(backend, (*sql).to_owned());
            txn.execute_raw(stmt)
                .await
                .map_err(|e| database_error(format!("Failed to execute SQL '{sql}': {e}"), e))?;
        }

        let insert_sql = format!(
            "INSERT INTO {q}{}{q} (version, id) VALUES ({}, '{}')",
            version_table, migration.version, migration.migration_id
        );
        let stmt = Statement::from_string(backend, insert_sql);
        txn.execute_raw(stmt)
            .await
            .map_err(|e| database_error(format!("Failed to insert version: {e}"), e))?;

        if verbose {
            eprintln!(
                "[vespertide] Migration v{} applied successfully",
                migration.version
            );
        }
    }

    txn.commit()
        .await
        .map_err(|e| database_error(format!("Failed to commit transaction: {e}"), e))?;

    Ok(())
}

fn collect_version_ids(rows: &[QueryResult]) -> std::collections::HashMap<u32, String> {
    let mut version_ids = std::collections::HashMap::new();
    for row in rows {
        if let Ok(found_version) = row.try_get::<i32>("", "version") {
            let id = row.try_get::<String>("", "id").unwrap_or_default();
            if let Ok(found_version) = u32::try_from(found_version) {
                version_ids.insert(found_version, id);
            }
        }
    }
    version_ids
}

// NOTE: tests that exercise `Database::connect("sqlite::memory:")` are kept
// in the workspace-excluded crate `tests/runtime-sqlite/`. Those need the
// sea-orm `sqlx-sqlite` feature, which links libsqlite3-sys 0.30 and
// conflicts on `links = "sqlite3"` with vespertide-query's rusqlite 0.39
// (libsqlite3-sys 0.37). Keeping them out of the workspace lets every crate
// stay on latest pinned versions.
//
// Run via: cargo test --manifest-path tests/runtime-sqlite/Cargo.toml
#[cfg(test)]
mod tests {
    use std::error::Error;

    use sea_orm::DatabaseBackend;

    use crate::MigrationError;

    use super::{EmbeddedMigration, split_sql_blob};

    #[test]
    fn split_sql_blob_ignores_empty_segments() {
        let sqls: Vec<_> =
            split_sql_blob("CREATE TABLE users ();\0\0ALTER TABLE users;\0").collect();

        assert_eq!(sqls, vec!["CREATE TABLE users ();", "ALTER TABLE users;"]);
    }

    #[test]
    fn embedded_migration_selects_backend_blob() {
        let migration = EmbeddedMigration::new(1, "id", "comment", "pg\0", "mysql\0", "sqlite\0");

        assert_eq!(migration.sql_blob(DatabaseBackend::Postgres), "pg\0");
        assert_eq!(migration.sql_blob(DatabaseBackend::MySql), "mysql\0");
        assert_eq!(migration.sql_blob(DatabaseBackend::Sqlite), "sqlite\0");
    }

    #[test]
    fn db_err_conversion_preserves_source_chain() {
        let db_err = sea_orm::DbErr::Custom("connection refused".to_owned());
        let err = MigrationError::from(db_err);

        assert_eq!(
            err.to_string(),
            "database error: Custom Error: connection refused"
        );
        assert!(err.source().is_some());
        assert!(matches!(
            &err,
            MigrationError::Database {
                source: Some(_),
                ..
            }
        ));
        assert!(
            err.source()
                .is_some_and(|source| source.to_string() == "Custom Error: connection refused")
        );
    }
}
