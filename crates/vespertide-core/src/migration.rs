/// Runtime options controlling how Vespertide tracks applied migrations.
///
/// Pass this to the migration runner to configure the version-tracking table name.
/// The default table name used by the `vespertide_migration!` macro is `"vespertide_migrations"`.
#[derive(Debug, Clone)]
pub struct MigrationOptions {
    /// Name of the table used to record which migration versions have been applied.
    ///
    /// Defaults to `"vespertide_migrations"`. Override this when multiple Vespertide-managed
    /// schemas share the same database and need separate version tables.
    pub version_table: String,
}

#[derive(thiserror::Error, Debug)]
pub enum MigrationError {
    #[error("migration execution is not yet implemented")]
    NotImplemented,
    #[error("database error: {0}")]
    #[deprecated(
        since = "0.1.62",
        note = "Use Database { message, source } for proper error source chains"
    )]
    DatabaseError(String),
    #[error("database error: {message}")]
    Database {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
    #[error(
        "migration id mismatch for version {version}: expected '{expected}', found '{found}' in database"
    )]
    IdMismatch {
        version: u32,
        expected: String,
        found: String,
    },
}

impl From<sea_orm::DbErr> for MigrationError {
    fn from(err: sea_orm::DbErr) -> Self {
        Self::Database {
            message: err.to_string(),
            source: Some(Box::new(err)),
        }
    }
}
