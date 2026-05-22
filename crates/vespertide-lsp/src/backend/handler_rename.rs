//! Body of the `prepare_rename` / `rename` LSP handlers, lifted out of
//! `backend::mod` to keep the latter under the 1000-line per-file
//! policy. Each function takes `&Backend` so it can read the shared
//! state (store, index, workspace tables) without exposing fields.
//!
//! `async` is preserved on both helpers — the LSP trait expects an
//! `async fn`, and these wrappers must be `.await`-able from the trait
//! impl block in `mod.rs`. Clippy's `unused_async` lint fires here
//! because the bodies themselves don't `.await`, but removing `async`
//! would break the trait signature mirror. Allow the lint locally.
#![allow(clippy::unused_async)]

use std::collections::HashMap;

use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::{
    Position, PrepareRenameResponse, Range, RenameParams, TextDocumentPositionParams, WorkspaceEdit,
};

use super::Backend;
use super::helpers::{byte_to_ls_position, domain_edits_to_lsp};
use crate::parser::DocumentFormat;

pub(super) async fn prepare_rename_impl(
    backend: &Backend,
    params: TextDocumentPositionParams,
) -> Result<Option<PrepareRenameResponse>> {
    let uri = params.text_document.uri;
    let pos_ls = params.position;
    let pos_lsp = crate::position::ls_to_lsp_position(pos_ls);
    let Some(format) = DocumentFormat::from_uri(&uri) else {
        return Ok(None);
    };

    let domain = backend.store.docs_iter_for_uri(&uri, |state| {
        let text = state.text();
        let byte = crate::position::lsp_position_to_byte(&state.doc, pos_lsp);
        crate::rename::prepare(text, format, state.tree.as_ref(), &uri, byte)
    });
    let Some(Some(domain)) = domain else {
        tracing::debug!(
            target: "vespertide_lsp::handler",
            uri = %uri.as_str(),
            "prepare_rename: position is not renameable"
        );
        return Ok(None);
    };

    let range = backend
        .store
        .docs_iter_for_uri(&uri, |state| Range {
            start: byte_to_ls_position(&state.doc, domain.byte_range.start),
            end: byte_to_ls_position(&state.doc, domain.byte_range.end),
        })
        .unwrap_or(Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 0,
            },
        });

    tracing::info!(
        target: "vespertide_lsp::handler",
        uri = %uri.as_str(),
        placeholder = %domain.placeholder,
        "prepare_rename"
    );

    Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
        range,
        placeholder: domain.placeholder,
    }))
}

pub(super) async fn rename_impl(
    backend: &Backend,
    params: RenameParams,
) -> Result<Option<WorkspaceEdit>> {
    let uri = params.text_document_position.text_document.uri;
    let pos_ls = params.text_document_position.position;
    let pos_lsp = crate::position::ls_to_lsp_position(pos_ls);
    let new_name = params.new_name;
    let Some(format) = DocumentFormat::from_uri(&uri) else {
        return Ok(None);
    };

    let domain = backend.store.docs_iter_for_uri(&uri, |state| {
        let text = state.text();
        let byte = crate::position::lsp_position_to_byte(&state.doc, pos_lsp);
        crate::rename::compute(
            text,
            format,
            state.tree.as_ref(),
            &uri,
            backend.index.as_ref(),
            backend.store.as_ref(),
            Some(backend.workspace_tables.as_ref()),
            byte,
            &new_name,
        )
    });
    let Some(Some(domain)) = domain else {
        return Ok(None);
    };

    tracing::info!(
        target: "vespertide_lsp::handler",
        uri = %uri.as_str(),
        new_name = %new_name,
        files = domain.edits.len(),
        total_edits = domain.edits.values().map(Vec::len).sum::<usize>(),
        "rename"
    );

    let mut changes = HashMap::new();
    for (target_uri, domain_edits) in domain.edits {
        let Some(text_edits) = domain_edits_to_lsp(&target_uri, &domain_edits, backend) else {
            continue;
        };
        changes.insert(target_uri, text_edits);
    }

    if changes.is_empty() {
        return Ok(None);
    }
    Ok(Some(WorkspaceEdit {
        changes: Some(changes),
        ..WorkspaceEdit::default()
    }))
}
