//! `vespertide-lsp`: Language Server for Vespertide schema files.
//!
//! Provides diagnostics, hover, cross-file go-to-definition, completion,
//! and drift detection (model <-> migration consistency) for the
//! Vespertide JSON / YAML model and migration formats.
//!
//! Wave 2 adds the data layer: tree-sitter parsing ([`ParserPool`]),
//! per-document state ([`DocumentState`]), and a concurrent
//! [`DocumentStore`]. LSP notification handlers (`did_open`,
//! `did_change`, `did_close`) land in W2-T2.

mod backend;
mod document;
mod parser;
mod store;

pub use backend::Backend;
pub use document::DocumentState;
pub use parser::{DocumentFormat, ParserPool};
pub use store::DocumentStore;
