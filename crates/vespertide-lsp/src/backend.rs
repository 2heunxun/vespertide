//! LSP backend.
//!
//! Holds the [`Client`] handle, a shared [`DocumentStore`], and a
//! [`WorkspaceIndex`]; implements [`LanguageServer`] from tower-lsp-server.
//!
//! Wave 1 handled only the lifecycle requests (`initialize`, `initialized`,
//! `shutdown`). Wave 2 (T2 + T3) introduced the document data layer and
//! cross-file index. Wave 3 wires diagnostics publication on open/change/close.
//!
//! Note: tower-lsp-server re-exports the upstream `lsp-types` crate under
//! the name `ls_types` (NOT `lsp_types`). Using `lsp_types::` directly
//! would fail to resolve.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::{
    CompletionItem, CompletionItemKind as LspCompletionItemKind, CompletionOptions,
    CompletionParams, CompletionResponse, Diagnostic, DiagnosticSeverity,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, DocumentFormattingParams, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverContents, HoverParams, HoverProviderCapability,
    InitializeParams, InitializeResult, InitializedParams, InsertTextFormat, Location,
    MarkupContent, MarkupKind, MessageType, NumberOrString, OneOf, Position, Range,
    ServerCapabilities, ServerInfo, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions, TextDocumentSyncSaveOptions, TextEdit, Uri,
};
use tower_lsp_server::{Client, LanguageServer};

use crate::diagnostics::{self, mapper};
use crate::parser::DocumentFormat;
use crate::store::DocumentStore;
use crate::workspace_index::WorkspaceIndex;
use crate::workspace_tables::WorkspaceTables;

/// Vespertide language server backend.
///
/// Owns the [`Client`] handle used to push notifications (log messages,
/// diagnostics) back to the editor, plus a shared [`DocumentStore`] that
/// holds parsed state for every open document and a [`WorkspaceIndex`]
/// mapping table names to URIs.
#[derive(Debug)]
pub struct Backend {
    /// LSP client handle for sending notifications to the editor.
    pub client: Client,
    /// Shared document store; mutated by the notification handlers.
    pub store: Arc<DocumentStore>,
    /// Cross-file table-name → URI index; kept in sync with `store`.
    pub index: Arc<WorkspaceIndex>,
    /// Disk-discovered model tables loaded from the workspace root.
    pub workspace_tables: Arc<WorkspaceTables>,
}

impl Backend {
    /// Construct a new backend bound to the given LSP [`Client`].
    ///
    /// Designed to be passed directly to `LspService::new(Backend::new)`.
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self {
            client,
            store: Arc::new(DocumentStore::new()),
            index: Arc::new(WorkspaceIndex::new()),
            workspace_tables: Arc::new(WorkspaceTables::new()),
        }
    }

    /// Reindex a document after open/change. No-op if the document was just
    /// closed or never parsed (tree is `None`).
    fn reindex(&self, uri: &Uri) {
        self.store.with_doc(uri, |text, tree| {
            if let Some(tree) = tree {
                self.index.upsert(uri, text, tree);
            }
        });
    }

    /// Compute and publish diagnostics for a document.
    ///
    /// V1 publishes immediately on full-sync events. A 100ms debounce can be
    /// added later if clients report noisy updates during rapid editing.
    async fn publish(&self, uri: Uri) {
        let diagnostics = self.compute_lsp_diagnostics(&uri);
        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }

    async fn publish_related(&self, changed_uri: &Uri) {
        let other_uris: Vec<Uri> = self
            .store
            .open_uris()
            .into_iter()
            .filter(|uri| uri != changed_uri)
            .collect();

        for uri in other_uris {
            self.publish(uri).await;
        }
    }

    fn collect_workspace_tables(&self) -> Vec<diagnostics::WorkspaceTable> {
        let mut workspace = Vec::new();
        let mut seen_names = BTreeSet::new();

        self.store.for_each(|uri, state| {
            let text = state.text();
            let Some(tree) = state.tree.clone() else {
                return;
            };
            let parsed = match state.format {
                DocumentFormat::Json => {
                    serde_json::from_str::<vespertide_core::TableDef>(text).ok()
                }
                DocumentFormat::Yaml => {
                    serde_yaml::from_str::<vespertide_core::TableDef>(text).ok()
                }
            };
            let Some(table) = parsed else {
                return;
            };
            let Ok(table) = table.normalize() else {
                return;
            };

            seen_names.insert(table.name.clone());
            workspace.push(diagnostics::WorkspaceTable {
                uri: uri.clone(),
                table,
                source: text.to_string(),
                tree: Some(tree),
            });
        });

        for (name, table) in self.workspace_tables.all() {
            if !seen_names.insert(name.clone()) {
                continue;
            }

            workspace.push(diagnostics::WorkspaceTable {
                uri: self.disk_table_uri(&name),
                table,
                source: String::new(),
                tree: None,
            });
        }

        workspace
    }

    fn disk_table_uri(&self, table_name: &str) -> Uri {
        self.workspace_tables
            .model_path(table_name)
            .and_then(|path| Self::path_to_uri(&path))
            .unwrap_or_else(|| Self::fallback_disk_uri(table_name))
    }

    fn path_to_uri(path: &Path) -> Option<Uri> {
        let mut path_text = path.to_string_lossy().replace('\\', "/");
        if !path_text.starts_with('/') {
            path_text = format!("/{path_text}");
        }
        Uri::from_str(&format!("file://{path_text}")).ok()
    }

    fn fallback_disk_uri(table_name: &str) -> Uri {
        Uri::from_str(&format!("file:///__disk__/{table_name}.json"))
            .expect("synthetic disk model URI should parse")
    }

    fn compute_lsp_diagnostics(&self, uri: &Uri) -> Vec<Diagnostic> {
        let Some(format) = DocumentFormat::from_uri(uri) else {
            return Vec::new();
        };

        let workspace = self.collect_workspace_tables();

        let mut diagnostics: Vec<Diagnostic> = self
            .store
            .docs_iter_for_uri(uri, |state| {
                let domain = diagnostics::compute_workspace(
                    state.text(),
                    format,
                    state.tree.as_ref(),
                    &workspace,
                    uri,
                );
                domain
                    .iter()
                    .map(|diag| mapper::to_lsp(diag, &state.doc))
                    .collect()
            })
            .unwrap_or_default();

        if let Some(root) = Self::workspace_root_for(uri) {
            for drift in crate::drift::compute(&root, self.index.as_ref(), self.store.as_ref()) {
                if drift.uri == *uri {
                    diagnostics.push(Self::drift_diagnostic(&drift.summary));
                }
            }
        }

        diagnostics
    }

    fn drift_diagnostic(summary: &str) -> Diagnostic {
        Diagnostic {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 0,
                },
            },
            severity: Some(DiagnosticSeverity::INFORMATION),
            code: Some(NumberOrString::String("drift".to_string())),
            code_description: None,
            source: Some("vespertide-lsp".to_string()),
            message: format!("Model drift detected — {summary}"),
            related_information: None,
            tags: None,
            data: None,
        }
    }

    fn workspace_root_for(uri: &Uri) -> Option<PathBuf> {
        let path = crate::position::uri_to_path(uri)?;
        let mut current = path.parent();
        while let Some(dir) = current {
            if dir.join("vespertide.json").exists() {
                return Some(dir.to_path_buf());
            }
            current = dir.parent();
        }
        None
    }

    fn refresh_workspace_tables_for_uri(&self, uri: &Uri) {
        if let Some(root) = Self::workspace_root_for(uri) {
            self.workspace_tables.refresh(&root);
        }
    }

    fn refresh_workspace_tables_from_initialize(&self, params: &InitializeParams) {
        if let Some(root_uri) = initialize_root_uri(params) {
            if let Some(root) = crate::position::uri_to_path(root_uri) {
                self.workspace_tables.refresh(&root);
            }
            return;
        }

        let Some(folders) = params.workspace_folders.as_ref() else {
            return;
        };
        for folder in folders {
            if let Some(root) = crate::position::uri_to_path(&folder.uri)
                && self.workspace_tables.refresh(&root)
            {
                break;
            }
        }
    }
}

#[allow(deprecated)]
fn initialize_root_uri(params: &InitializeParams) -> Option<&Uri> {
    // `root_uri` is deprecated in newer LSP versions, but several editors still
    // send it without `workspace_folders`. Keep it as the first fallback for
    // workspace discovery while isolating the compatibility warning here.
    params.root_uri.as_ref()
}

impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        self.refresh_workspace_tables_from_initialize(&params);

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        will_save: None,
                        will_save_wait_until: None,
                        save: Some(TextDocumentSyncSaveOptions::Supported(true)),
                    },
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec!["\"".to_string(), ":".to_string()]),
                    ..CompletionOptions::default()
                }),
                document_formatting_provider: Some(OneOf::Left(true)),
                ..ServerCapabilities::default()
            },
            server_info: Some(ServerInfo {
                name: "vespertide-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
            // tower-lsp-server 0.23 exposes an explicit offset_encoding field;
            // leaving it `None` keeps the default (UTF-16) negotiated by the client.
            offset_encoding: None,
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "vespertide-lsp initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let pos_ls = params.text_document_position.position;
        let pos_lsp = crate::position::ls_to_lsp_position(pos_ls);
        let Some(format) = DocumentFormat::from_uri(uri) else {
            return Ok(None);
        };

        let items = self.store.docs_iter_for_uri(uri, |state| {
            let text = state.text();
            let byte = crate::position::lsp_position_to_byte(&state.doc, pos_lsp);
            crate::completion::compute_with_workspace_tables(
                text,
                format,
                state.tree.as_ref(),
                self.index.as_ref(),
                self.store.as_ref(),
                self.workspace_tables.as_ref(),
                byte,
            )
            .into_iter()
            .map(|item| CompletionItem {
                label: item.label.clone(),
                kind: Some(match item.kind {
                    crate::completion::CompletionItemKind::Value => LspCompletionItemKind::VALUE,
                    crate::completion::CompletionItemKind::Property => {
                        LspCompletionItemKind::PROPERTY
                    }
                    crate::completion::CompletionItemKind::Reference => {
                        LspCompletionItemKind::REFERENCE
                    }
                    crate::completion::CompletionItemKind::Snippet => {
                        LspCompletionItemKind::SNIPPET
                    }
                }),
                detail: item.detail,
                insert_text_format: item.insert_text.as_ref().map(|_| InsertTextFormat::SNIPPET),
                insert_text: item.insert_text,
                sort_text: Some(format!("{:03}{}", item.sort_priority, item.label)),
                ..CompletionItem::default()
            })
            .collect::<Vec<_>>()
        });

        Ok(items.map(CompletionResponse::Array))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos_ls = params.text_document_position_params.position;
        let pos_lsp = crate::position::ls_to_lsp_position(pos_ls);
        let Some(format) = DocumentFormat::from_uri(uri) else {
            return Ok(None);
        };

        let result = self.store.docs_iter_for_uri(uri, |state| {
            let text = state.text();
            let byte = crate::position::lsp_position_to_byte(&state.doc, pos_lsp);
            let domain = crate::hover::compute(
                text,
                format,
                state.tree.as_ref(),
                self.index.as_ref(),
                self.store.as_ref(),
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

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos_ls = params.text_document_position_params.position;
        let pos_lsp = crate::position::ls_to_lsp_position(pos_ls);
        let Some(format) = DocumentFormat::from_uri(&uri) else {
            return Ok(None);
        };

        let domain = self
            .store
            .docs_iter_for_uri(&uri, |state| {
                let text = state.text();
                let byte = crate::position::lsp_position_to_byte(&state.doc, pos_lsp);
                crate::definition::compute(
                    text,
                    format,
                    state.tree.as_ref(),
                    self.index.as_ref(),
                    self.store.as_ref(),
                    byte,
                )
            })
            .flatten();

        let Some(domain) = domain else {
            return Ok(None);
        };

        let target_range = self
            .store
            .docs_iter_for_uri(&domain.uri, |state| {
                let start =
                    crate::position::byte_to_lsp_position(&state.doc, domain.byte_range.start);
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

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = &params.text_document.uri;
        let Some(format) = DocumentFormat::from_uri(uri) else {
            return Ok(None);
        };

        let result = self.store.docs_iter_for_uri(uri, |state| {
            let original = state.text();
            let formatted = crate::formatting::format_text(original, format)?;
            if formatted == original {
                return Some(Vec::new());
            }

            let end = crate::position::byte_to_lsp_position(&state.doc, original.len());
            Some(vec![TextEdit {
                range: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: end.line,
                        character: end.character,
                    },
                },
                new_text: formatted,
            }])
        });

        Ok(result.flatten())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let td = params.text_document;
        let uri = td.uri.clone();
        self.store
            .open(uri.clone(), td.language_id, td.version, td.text);
        self.reindex(&uri);
        self.refresh_workspace_tables_for_uri(&uri);
        self.publish(uri.clone()).await;
        self.publish_related(&uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let td = params.text_document;
        // V1 = FULL sync: changes[0].text is the entire new content.
        if let Some(change) = params.content_changes.into_iter().next() {
            let uri = td.uri;
            self.store.update_full(&uri, change.text, td.version);
            self.reindex(&uri);
            self.refresh_workspace_tables_for_uri(&uri);
            self.publish(uri.clone()).await;
            self.publish_related(&uri).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        self.refresh_workspace_tables_for_uri(&uri);
        self.publish(uri.clone()).await;
        self.publish_related(&uri).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.store.close(&uri);
        self.index.remove(&uri);
        self.refresh_workspace_tables_for_uri(&uri);
        self.client
            .publish_diagnostics(uri.clone(), Vec::new(), None)
            .await;
        self.publish_related(&uri).await;
    }
}
