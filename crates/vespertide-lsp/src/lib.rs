//! `vespertide-lsp`: Language Server for Vespertide schema files.
//!
//! Provides diagnostics, hover, cross-file go-to-definition, completion,
//! and drift detection (model <-> migration consistency) for the
//! Vespertide JSON / YAML model and migration formats.
//!
//! Wave 1 ships a minimal scaffold that responds to `initialize` and
//! `shutdown`. Subsequent waves layer in document state, parsing, and
//! analysis features. See the project plan for the full roadmap.

mod backend;

pub use backend::Backend;
