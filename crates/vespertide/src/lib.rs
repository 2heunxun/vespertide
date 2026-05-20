//! # Vespertide
//!
//! Declarative database schema management for Rust. Define schemas in JSON,
//! generate migration plans, emit SQL for PostgreSQL/MySQL/SQLite, and export
//! ORM models for SeaORM/SQLAlchemy/SQLModel/JPA.
//!
//! This is the facade crate; runtime migrations use [`vespertide_migration!`].
//! Advanced users may depend on `vespertide-core` directly for typed data structures.

pub mod runtime;

// Re-export macro for convenient usage
#[doc(inline)]
pub use vespertide_macro::vespertide_migration;

// Re-export other commonly used items
pub use vespertide_core::{MigrationError, MigrationOptions};
