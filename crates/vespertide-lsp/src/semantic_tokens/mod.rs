//! Semantic tokens — LSP `textDocument/semanticTokens/{full,range}`.
//!
//! Vespertide ships its own `TextMate` / tree-sitter highlight queries
//! (`apps/vscode-extension/syntaxes/`, `apps/zed-extension/languages/`)
//! which colour the document by *syntax*. Semantic tokens layer on top:
//! they classify nodes by *meaning* — a `column.name` value vs a
//! `ref_table` value vs an enum value — so themes can paint them
//! distinctly even though all three are JSON strings at the syntax level.
//!
//! Architecture:
//!   * [`legend`] defines the ordered set of token types and modifiers
//!     reported on `initialize` (LSP requires the indices to be stable
//!     for the lifetime of the connection).
//!   * `classify_*` modules tree-sitter-walk a document and emit a
//!     [`RawToken`] for each significant span.
//!   * [`encode`] sorts and delta-encodes the raw tokens into the
//!     `Vec<SemanticToken>` wire shape.
//!   * The backend pumps a document through `classify_* → encode` for
//!     both `semantic_tokens_full` and `semantic_tokens_range` (range
//!     is a strict subset, computed by pre-filtering on `RawToken`).

mod classify_json;
mod classify_yaml;
mod encode;
pub mod handler;
pub mod legend;

use std::ops::Range;

pub use encode::encode;
pub use legend::legend;

use crate::parser::DocumentFormat;

/// A single token emitted by a classifier. Byte ranges are over the
/// document's UTF-8 source — [`encode`] resolves them to UTF-16
/// line/character positions using `lsp-textdocument`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawToken {
    /// UTF-8 byte range of the token in the source document.
    pub byte_range: Range<usize>,
    /// Index into [`TOKEN_TYPE_NAMES`].
    pub token_type: u32,
    /// Bitmask over [`TOKEN_MODIFIER_NAMES`].
    pub token_modifiers: u32,
}

/// Classify the entire document. The classifier dispatches on format —
/// JSON and YAML use different tree-sitter grammars with different node
/// kinds.
#[must_use]
pub fn classify(
    source: &str,
    format: DocumentFormat,
    tree: Option<&tree_sitter::Tree>,
) -> Vec<RawToken> {
    let Some(tree) = tree else {
        return Vec::new();
    };
    match format {
        DocumentFormat::Json => classify_json::classify(source, tree),
        DocumentFormat::Yaml => classify_yaml::classify(source, tree),
    }
}

/// Filter a raw token list to those whose byte range overlaps `range`.
/// Used by `semantic_tokens_range` to satisfy the LSP range request.
#[must_use]
pub fn filter_range(tokens: Vec<RawToken>, range: Range<usize>) -> Vec<RawToken> {
    tokens
        .into_iter()
        .filter(|t| t.byte_range.start < range.end && range.start < t.byte_range.end)
        .collect()
}
