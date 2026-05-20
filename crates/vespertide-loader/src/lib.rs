//! Filesystem loading of vespertide models and migrations.
//!
//! Supports both JSON and YAML model/migration files. Used by both the CLI
//! at runtime and the `vespertide_migration!` macro at compile time.

// Test-only env mutation in `migrations.rs` / `models.rs` uses
// `std::env::set_var` / `remove_var`, which became `unsafe` in Rust 2024.
// All sites are serialized via `serial_test::serial`, so allow unsafe in tests only.
#![cfg_attr(test, allow(unsafe_code))]

pub mod config;
pub mod migrations;
pub mod models;

pub use config::{load_config, load_config_from_path, load_config_or_default};
pub use migrations::{load_migrations, load_migrations_at_compile_time, load_migrations_from_dir};
pub use models::{load_models, load_models_at_compile_time, load_models_from_dir};
