//! Body of the navigation / point-at-position LSP handlers
//! (`completion`, `hover`, `goto_definition`, `references`,
//! `code_action`, `inlay_hint`, `symbol`), lifted out of `backend::mod`
//! to keep that file under the workspace's 1000-line per-file policy.
//!
//! Each `*_impl` function takes `&Backend` so it can read shared state
//! (store, index, workspace tables) via the `pub` fields without
//! depending on private internals. The trait wrappers in `backend::mod`
//! delegate verbatim.
//!
//! `async` is preserved on every helper — the LSP trait expects an
//! `async fn`, and these wrappers must be `.await`-able from the trait
//! impl block in `mod.rs`. Several bodies (e.g. `inlay_hint_impl`,
//! `symbol_impl`) don't `.await` and so trip clippy's `unused_async`,
//! mirroring the existing `handler_rename.rs` / `handler_file_features.rs`
//! pattern.
#![expect(
    clippy::unused_async,
    reason = "tower-lsp-server LanguageServer navigation handlers must stay awaitable async fns even when bodies are synchronous"
)]

use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::{
    CodeAction, CodeActionKind as LspCodeActionKind, CodeActionOrCommand, CodeActionParams,
    CodeActionResponse, CompletionParams, CompletionResponse, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverContents, HoverParams, InlayHint,
    InlayHintKind as LspInlayHintKind, InlayHintLabel, InlayHintParams, Location, MarkupContent,
    MarkupKind, Position, Range, ReferenceParams, SymbolInformation, WorkspaceEdit,
    WorkspaceSymbolParams, WorkspaceSymbolResponse,
};

use super::Backend;
use super::helpers::{
    byte_to_ls_position, domain_edits_to_lsp, domain_reference_to_location, domain_to_lsp,
    symbol_to_lsp,
};
use crate::parser::DocumentFormat;

pub(super) async fn completion_impl(
    backend: &Backend,
    params: CompletionParams,
) -> Result<Option<CompletionResponse>> {
    let uri = &params.text_document_position.text_document.uri;
    let pos_ls = params.text_document_position.position;
    let pos_lsp = crate::position::ls_to_lsp_position(pos_ls);
    let Some(format) = DocumentFormat::from_uri(uri) else {
        tracing::debug!(
            target: "vespertide_lsp::handler",
            uri = %uri.as_str(),
            "completion: unsupported document format"
        );
        return Ok(None);
    };

    let items = backend.store.docs_iter_for_uri(uri, |state| {
        let text = state.text();
        let byte = crate::position::lsp_position_to_byte(&state.doc, pos_lsp);
        crate::completion::compute_with_workspace_tables(
            text,
            format,
            state.tree.as_ref(),
            backend.index.as_ref(),
            backend.store.as_ref(),
            backend.workspace_tables.as_ref(),
            byte,
        )
        .into_iter()
        .map(|item| domain_to_lsp(item, &state.doc))
        .collect::<Vec<_>>()
    });

    let count = items.as_ref().map_or(0, Vec::len);
    tracing::info!(
        target: "vespertide_lsp::handler",
        uri = %uri.as_str(),
        line = pos_lsp.line,
        character = pos_lsp.character,
        items = count,
        "completion"
    );

    Ok(items.map(CompletionResponse::Array))
}

pub(super) async fn hover_impl(backend: &Backend, params: HoverParams) -> Result<Option<Hover>> {
    let uri = &params.text_document_position_params.text_document.uri;
    let pos_ls = params.text_document_position_params.position;
    let pos_lsp = crate::position::ls_to_lsp_position(pos_ls);
    let Some(format) = DocumentFormat::from_uri(uri) else {
        return Ok(None);
    };

    let result = backend.store.docs_iter_for_uri(uri, |state| {
        let text = state.text();
        let byte = crate::position::lsp_position_to_byte(&state.doc, pos_lsp);
        let domain = crate::hover::compute_with_workspace_tables(
            text,
            format,
            state.tree.as_ref(),
            backend.index.as_ref(),
            backend.store.as_ref(),
            Some(backend.workspace_tables.as_ref()),
            byte,
        )?;
        let start = crate::position::byte_to_lsp_position(&state.doc, domain.byte_range.start);
        let end = crate::position::byte_to_lsp_position(&state.doc, domain.byte_range.end);
        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: domain.markdown,
            }),
            range: Some(Range {
                start: Position {
                    line: start.line,
                    character: start.character,
                },
                end: Position {
                    line: end.line,
                    character: end.character,
                },
            }),
        })
    });
    Ok(result.flatten())
}

pub(super) async fn goto_definition_impl(
    backend: &Backend,
    params: GotoDefinitionParams,
) -> Result<Option<GotoDefinitionResponse>> {
    let uri = params.text_document_position_params.text_document.uri;
    let pos_ls = params.text_document_position_params.position;
    let pos_lsp = crate::position::ls_to_lsp_position(pos_ls);
    let Some(format) = DocumentFormat::from_uri(&uri) else {
        return Ok(None);
    };

    let domain = backend
        .store
        .docs_iter_for_uri(&uri, |state| {
            let text = state.text();
            let byte = crate::position::lsp_position_to_byte(&state.doc, pos_lsp);
            crate::definition::compute_with_workspace_tables(
                text,
                format,
                state.tree.as_ref(),
                backend.index.as_ref(),
                backend.store.as_ref(),
                Some(backend.workspace_tables.as_ref()),
                byte,
            )
        })
        .flatten();

    let Some(domain) = domain else {
        tracing::info!(
            target: "vespertide_lsp::handler",
            uri = %uri.as_str(),
            line = pos_lsp.line,
            character = pos_lsp.character,
            "goto_definition: no target"
        );
        return Ok(None);
    };
    tracing::info!(
        target: "vespertide_lsp::handler",
        from_uri = %uri.as_str(),
        target_uri = %domain.uri.as_str(),
        line = pos_lsp.line,
        character = pos_lsp.character,
        "goto_definition: resolved"
    );

    let target_range = backend
        .store
        .docs_iter_for_uri(&domain.uri, |state| {
            let start = crate::position::byte_to_lsp_position(&state.doc, domain.byte_range.start);
            let end = crate::position::byte_to_lsp_position(&state.doc, domain.byte_range.end);
            Range {
                start: Position {
                    line: start.line,
                    character: start.character,
                },
                end: Position {
                    line: end.line,
                    character: end.character,
                },
            }
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

    Ok(Some(GotoDefinitionResponse::Scalar(Location {
        uri: domain.uri,
        range: target_range,
    })))
}

pub(super) async fn references_impl(
    backend: &Backend,
    params: ReferenceParams,
) -> Result<Option<Vec<Location>>> {
    let uri = params.text_document_position.text_document.uri;
    let pos_ls = params.text_document_position.position;
    let pos_lsp = crate::position::ls_to_lsp_position(pos_ls);
    let include_declaration = params.context.include_declaration;
    let Some(format) = DocumentFormat::from_uri(&uri) else {
        return Ok(None);
    };

    let domain_refs = backend.store.docs_iter_for_uri(&uri, |state| {
        let text = state.text();
        let byte = crate::position::lsp_position_to_byte(&state.doc, pos_lsp);
        crate::references::compute(
            text,
            format,
            state.tree.as_ref(),
            &uri,
            backend.index.as_ref(),
            backend.store.as_ref(),
            Some(backend.workspace_tables.as_ref()),
            byte,
            include_declaration,
        )
    });
    let Some(domain_refs) = domain_refs else {
        return Ok(None);
    };

    tracing::info!(
        target: "vespertide_lsp::handler",
        uri = %uri.as_str(),
        line = pos_lsp.line,
        character = pos_lsp.character,
        include_declaration,
        count = domain_refs.len(),
        "references"
    );

    let locations = domain_refs
        .into_iter()
        .filter_map(|reference| domain_reference_to_location(&reference, backend))
        .collect::<Vec<_>>();

    if locations.is_empty() {
        Ok(None)
    } else {
        Ok(Some(locations))
    }
}

pub(super) async fn code_action_impl(
    backend: &Backend,
    params: CodeActionParams,
) -> Result<Option<CodeActionResponse>> {
    let uri = params.text_document.uri;
    let range_ls = params.range;
    let range_lsp = crate::position::ls_to_lsp_range(range_ls);
    let Some(format) = DocumentFormat::from_uri(&uri) else {
        return Ok(None);
    };

    let domain_actions = backend.store.docs_iter_for_uri(&uri, |state| {
        let text = state.text();
        let start = crate::position::lsp_position_to_byte(&state.doc, range_lsp.start);
        let end = crate::position::lsp_position_to_byte(&state.doc, range_lsp.end);
        crate::code_actions::compute(text, format, state.tree.as_ref(), start..end)
    });
    let Some(domain_actions) = domain_actions else {
        return Ok(None);
    };

    tracing::info!(
        target: "vespertide_lsp::handler",
        uri = %uri.as_str(),
        actions = domain_actions.len(),
        "code_action"
    );

    let actions: Vec<CodeActionOrCommand> = domain_actions
        .into_iter()
        .filter_map(|action| {
            let text_edits = domain_edits_to_lsp(&uri, &action.edits, backend)?;
            let mut changes = std::collections::HashMap::new();
            changes.insert(uri.clone(), text_edits);
            Some(CodeActionOrCommand::CodeAction(CodeAction {
                title: action.title,
                kind: Some(LspCodeActionKind::REFACTOR),
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    ..WorkspaceEdit::default()
                }),
                ..CodeAction::default()
            }))
        })
        .collect();

    if actions.is_empty() {
        Ok(None)
    } else {
        Ok(Some(actions))
    }
}

pub(super) async fn inlay_hint_impl(
    backend: &Backend,
    params: InlayHintParams,
) -> Result<Option<Vec<InlayHint>>> {
    let uri = params.text_document.uri;
    let range_ls = params.range;
    let range_lsp = crate::position::ls_to_lsp_range(range_ls);
    let Some(_format) = DocumentFormat::from_uri(&uri) else {
        return Ok(None);
    };

    let hints = backend.store.docs_iter_for_uri(&uri, |state| {
        let text = state.text();
        let start = crate::position::lsp_position_to_byte(&state.doc, range_lsp.start);
        let end = crate::position::lsp_position_to_byte(&state.doc, range_lsp.end);
        let domain = crate::inlay_hints::compute(text, state.tree.as_ref(), start..end);
        domain
            .into_iter()
            .map(|hint| InlayHint {
                position: byte_to_ls_position(&state.doc, hint.byte_offset),
                label: InlayHintLabel::String(hint.label),
                kind: Some(LspInlayHintKind::TYPE),
                text_edits: None,
                tooltip: None,
                padding_left: Some(false),
                padding_right: Some(false),
                data: None,
            })
            .collect::<Vec<_>>()
    });

    let Some(hints) = hints else {
        return Ok(None);
    };

    tracing::debug!(
        target: "vespertide_lsp::handler",
        uri = %uri.as_str(),
        count = hints.len(),
        "inlay_hint"
    );

    if hints.is_empty() {
        Ok(None)
    } else {
        Ok(Some(hints))
    }
}

pub(super) async fn symbol_impl(
    backend: &Backend,
    params: WorkspaceSymbolParams,
) -> Result<Option<WorkspaceSymbolResponse>> {
    let query = params.query;
    let domain = crate::symbols::compute_shared(
        &query,
        backend.store.as_ref(),
        Some(backend.workspace_tables.as_ref()),
    );

    tracing::info!(
        target: "vespertide_lsp::handler",
        query = %query,
        results = domain.len(),
        "workspace symbol"
    );

    let lsp_symbols: Vec<SymbolInformation> = domain
        .iter()
        .filter_map(|sym| symbol_to_lsp(sym, backend))
        .collect();
    if lsp_symbols.is_empty() {
        Ok(None)
    } else {
        Ok(Some(WorkspaceSymbolResponse::Flat(lsp_symbols)))
    }
}
