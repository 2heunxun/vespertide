//! `vespertide-lsp`: Language Server for Vespertide schema files.
//!
//! Provides diagnostics, hover, cross-file go-to-definition, completion,
//! and drift detection (model <-> migration consistency) for the
//! Vespertide JSON / YAML model and migration formats.
//!
//! Wave 2 adds the data layer: tree-sitter parsing ([`ParserPool`]),
//! per-document state ([`DocumentState`]), and a concurrent
//! [`DocumentStore`]. W2-T2 wires the `did_open` / `did_change` /
//! `did_close` notification handlers and adds UTF-16 ↔ byte offset
//! conversions; W2-T3 introduces [`WorkspaceIndex`], a cross-file
//! `table_name → Uri` map maintained by walking each document's
//! tree-sitter parse.

mod backend;
mod completion;
mod definition;
mod diagnostics;
mod document;
mod drift;
mod formatting;
mod hover;
mod parser;
mod position;
mod store;
mod workspace_index;

pub use backend::Backend;
pub use completion::{CompletionItemKind, DomainCompletion, compute as compute_completion};
pub use definition::{DomainLocation, compute as compute_definition};
pub use diagnostics::{DomainDiagnostic, Severity, compute as compute_diagnostics};
pub use document::DocumentState;
pub use drift::{DomainDrift, compute as compute_drift};
pub use formatting::format_text;
pub use hover::{DomainHover, compute as compute_hover};
pub use parser::{DocumentFormat, ParserPool};
pub use position::{
    byte_to_lsp_position, ls_to_lsp_position, ls_to_lsp_range, lsp_position_to_byte,
    lsp_to_ls_position, uri_to_path,
};
pub use store::DocumentStore;
pub use workspace_index::{TableLocation, WorkspaceIndex};
