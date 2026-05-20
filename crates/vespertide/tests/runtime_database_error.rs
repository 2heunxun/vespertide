use std::error::Error;

use sea_orm::Database;
use vespertide::{
    MigrationError,
    runtime::{EmbeddedMigration, run_embedded_migrations},
};

#[tokio::test]
async fn run_embedded_migrations_preserves_db_error_source() {
    let db = Database::connect("sqlite::memory:").await.unwrap();

    let err = run_embedded_migrations(
        &db,
        "vespertide_migrations",
        false,
        &[EmbeddedMigration::new(
            1,
            "broken",
            "invalid sql",
            "THIS IS NOT SQL;\0",
            "THIS IS NOT SQL;\0",
            "THIS IS NOT SQL;\0",
        )],
    )
    .await
    .unwrap_err();

    assert!(err.source().is_some());
    let source = err.source().unwrap();
    assert!(source.downcast_ref::<sea_orm::DbErr>().is_some());
    assert!(matches!(
        err,
        MigrationError::Database { message, source: Some(_) }
            if message.contains("Failed to execute SQL 'THIS IS NOT SQL;'")
    ));
}
