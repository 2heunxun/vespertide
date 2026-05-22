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
mod code_actions;
mod completion;
mod definition;
pub mod diagnostics;
mod document;
mod drift;
mod formatting;
mod hover;
mod inlay_hints;
pub mod logging;
mod parser;
mod position;
mod references;
mod rename;
mod semantic_tokens;
mod store;
mod symbols;
mod watched_files;
mod workspace_index;
pub mod workspace_tables;

pub use backend::Backend;
pub use completion::{
    CompletionItemKind, DomainCompletion, compute as compute_completion,
    compute_with_workspace_tables as compute_completion_with_workspace_tables,
};
pub use definition::{DomainLocation, compute as compute_definition};
pub use diagnostics::{
    DomainDiagnostic, Severity, compute as compute_diagnostics,
    compute_workspace as compute_workspace_diagnostics,
};
pub use document::DocumentState;
pub use drift::{DomainDrift, compute as compute_drift};
pub use formatting::format_text;
pub use hover::{DomainHover, compute as compute_hover};
pub use inlay_hints::{DomainInlayHint, compute as compute_inlay_hints};
pub use parser::{DocumentFormat, ParserPool};
pub use references::{
    DomainReference, ReferenceSymbol, compute as compute_references,
    resolve_symbol as resolve_reference_symbol,
};
pub use code_actions::{
    CodeActionKind as DomainCodeActionKind, DomainCodeAction, compute as compute_code_actions,
};
pub use rename::{
    DomainPrepareRename, DomainRename, DomainTextEdit, compute as compute_rename,
    prepare as prepare_rename,
};
pub use symbols::{
    DomainSymbol, SymbolKind as DomainSymbolKind, compute as compute_workspace_symbols,
};
pub use position::{
    byte_to_lsp_position, ls_to_lsp_position, ls_to_lsp_range, lsp_position_to_byte,
    lsp_to_ls_position, uri_to_path,
};
pub use store::DocumentStore;
pub use workspace_index::{TableLocation, WorkspaceIndex};
pub use workspace_tables::WorkspaceTables;
