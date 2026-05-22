//! LSP wire-shape adapter for semantic tokens. Keeps the byte-level
//! `RawToken` work out of `backend.rs` so the latter stays under the
//! workspace's 1000-line file policy.

use tower_lsp_server::ls_types::{
    SemanticTokens, SemanticTokensParams, SemanticTokensRangeParams, SemanticTokensRangeResult,
    SemanticTokensResult,
};

use crate::parser::DocumentFormat;
use crate::store::DocumentStore;

/// Compute the full-document semantic tokens response for `uri`.
/// Returns `None` if the document isn't open or doesn't have a parsed
/// tree (e.g. plain text file the client mistakenly handed us).
pub fn compute_full(
    store: &DocumentStore,
    params: &SemanticTokensParams,
) -> Option<SemanticTokensResult> {
    let uri = &params.text_document.uri;
    let format = DocumentFormat::from_uri(uri)?;

    let data = store.docs_iter_for_uri(uri, |state| {
        let text = state.text();
        let mut raw = super::classify(text, format, state.tree.as_ref());
        super::encode(&mut raw, &state.doc)
    })?;

    tracing::info!(
        target: "vespertide_lsp::handler",
        uri = %uri.as_str(),
        tokens = data.len(),
        "semantic_tokens_full"
    );

    Some(SemanticTokensResult::Tokens(SemanticTokens {
        result_id: None,
        data,
    }))
}

/// Compute the range-scoped response. Range-based requests are cheaper
/// for clients that only need on-screen tokens; we still classify the
/// whole tree (cheap) but filter the output to the requested range.
pub fn compute_range(
    store: &DocumentStore,
    params: &SemanticTokensRangeParams,
) -> Option<SemanticTokensRangeResult> {
    let uri = &params.text_document.uri;
    let format = DocumentFormat::from_uri(uri)?;
    let lsp_range = crate::position::ls_to_lsp_range(params.range);

    let data = store.docs_iter_for_uri(uri, |state| {
        let text = state.text();
        let start = crate::position::lsp_position_to_byte(&state.doc, lsp_range.start);
        let end = crate::position::lsp_position_to_byte(&state.doc, lsp_range.end);
        let raw = super::classify(text, format, state.tree.as_ref());
        let mut filtered = super::filter_range(raw, start..end);
        super::encode(&mut filtered, &state.doc)
    })?;

    tracing::info!(
        target: "vespertide_lsp::handler",
        uri = %uri.as_str(),
        tokens = data.len(),
        "semantic_tokens_range"
    );

    Some(SemanticTokensRangeResult::Tokens(SemanticTokens {
        result_id: None,
        data,
    }))
}
