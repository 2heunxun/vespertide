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
    CodeAction, CodeActionKind as LspCodeActionKind, CodeActionOptions, CodeActionOrCommand,
    CodeActionParams, CodeActionProviderCapability, CodeActionResponse, CompletionOptions,
    CompletionParams, CompletionResponse, Diagnostic, DiagnosticSeverity,
    DidChangeTextDocumentParams, DidChangeWatchedFilesParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, DocumentFormattingParams,
    DocumentHighlight, DocumentHighlightParams, DocumentSymbolParams,
    DocumentSymbolResponse, FoldingRange,
    FoldingRangeParams, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents,
    HoverParams, HoverProviderCapability, InitializeParams, InitializeResult, InitializedParams,
    InlayHint, InlayHintKind as LspInlayHintKind, InlayHintLabel, InlayHintOptions,
    InlayHintParams, InlayHintServerCapabilities, Location, MarkupContent, MarkupKind,
    MessageType, NumberOrString, OneOf, Position, PrepareRenameResponse, Range, ReferenceParams,
    RenameOptions, RenameParams, SelectionRange, SelectionRangeParams,
    SelectionRangeProviderCapability, SemanticTokensFullOptions, SemanticTokensOptions,
    SemanticTokensParams, SemanticTokensRangeParams, SemanticTokensRangeResult,
    SemanticTokensResult, SemanticTokensServerCapabilities, ServerCapabilities, ServerInfo,
    SymbolInformation, TextDocumentPositionParams, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextDocumentSyncOptions, TextDocumentSyncSaveOptions, TextEdit, Uri,
    WorkDoneProgressOptions, WorkspaceEdit, WorkspaceSymbolParams, WorkspaceSymbolResponse,
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
        let counts = diagnostic_severity_counts(&diagnostics);
        tracing::info!(
            target: "vespertide_lsp::diagnostics",
            uri = %uri.as_str(),
            total = diagnostics.len(),
            errors = counts.errors,
            warnings = counts.warnings,
            "publishing diagnostics"
        );
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
        // Dedup by NORMALIZED FILESYSTEM PATH so a file that is both open
        // in the editor and present on disk is registered only once.
        //
        // URI-level dedup is not enough: Zed and our own `path_to_uri`
        // helper can emit slightly different strings (drive-letter case
        // on Windows, %20 vs space, trailing slashes) for the same file.
        // Two registrations of the same file would make the planner report
        // a spurious `DuplicateTableName`.
        let mut seen_paths: BTreeSet<PathBuf> = BTreeSet::new();

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

            if let Some(path) = crate::position::uri_to_path(uri) {
                seen_paths.insert(normalize_path(&path));
            }
            workspace.push(diagnostics::WorkspaceTable {
                uri: uri.clone(),
                table,
                source: text.to_string(),
                tree: Some(tree),
            });
        });

        for (name, table) in self.workspace_tables.all() {
            let Some(disk_path) = self.workspace_tables.model_path(&name) else {
                continue;
            };
            if !seen_paths.insert(normalize_path(&disk_path)) {
                // Same physical file is already in the workspace as an open document.
                continue;
            }

            let disk_uri = Self::path_to_uri(&disk_path)
                .unwrap_or_else(|| Self::fallback_disk_uri(&name));
            workspace.push(diagnostics::WorkspaceTable {
                uri: disk_uri,
                table,
                source: String::new(),
                tree: None,
            });
        }

        workspace
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
        let root = params
            .workspace_folders
            .as_ref()
            .and_then(|f| f.first().map(|folder| folder.uri.as_str().to_string()))
            .or_else(|| initialize_root_uri(&params).map(|uri| uri.as_str().to_string()));
        tracing::info!(
            target: "vespertide_lsp::handler",
            root = root.as_deref().unwrap_or("<none>"),
            client = ?params.client_info.as_ref().map(|c| c.name.as_str()),
            "initialize"
        );

        self.refresh_workspace_tables_from_initialize(&params);
        let discovered = self.workspace_tables.names();
        tracing::info!(
            target: "vespertide_lsp::handler",
            disk_table_count = discovered.len(),
            disk_tables = ?discovered,
            "workspace tables discovered"
        );

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
                references_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                })),
                code_action_provider: Some(CodeActionProviderCapability::Options(
                    CodeActionOptions {
                        code_action_kinds: Some(vec![LspCodeActionKind::REFACTOR]),
                        resolve_provider: Some(false),
                        work_done_progress_options: WorkDoneProgressOptions::default(),
                    },
                )),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                inlay_hint_provider: Some(
                    tower_lsp_server::ls_types::OneOf::Right(
                        InlayHintServerCapabilities::Options(InlayHintOptions {
                            work_done_progress_options: WorkDoneProgressOptions::default(),
                            resolve_provider: Some(false),
                        }),
                    ),
                ),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            work_done_progress_options: WorkDoneProgressOptions::default(),
                            legend: crate::semantic_tokens::legend(),
                            range: Some(true),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                        },
                    ),
                ),
                document_symbol_provider: Some(OneOf::Left(true)),
                document_highlight_provider: Some(OneOf::Left(true)),
                folding_range_provider: Some(
                    tower_lsp_server::ls_types::FoldingRangeProviderCapability::Simple(true),
                ),
                selection_range_provider: Some(SelectionRangeProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions {
                    // `"` triggers key/value strings, `:` value position,
                    // `,` opens a new pair, `{` and `[` open new objects.
                    trigger_characters: Some(vec![
                        "\"".to_string(),
                        ":".to_string(),
                        ",".to_string(),
                        "{".to_string(),
                        "[".to_string(),
                    ]),
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
        tracing::info!(target: "vespertide_lsp::handler", "initialized");
        let log_path = std::env::var_os("VESPERTIDE_LSP_LOG")
            .map_or_else(|| std::env::temp_dir().join("vespertide-lsp.log"), std::path::PathBuf::from);
        let message = format!(
            "vespertide-lsp v{} initialized. File log: {}",
            env!("CARGO_PKG_VERSION"),
            log_path.display()
        );
        self.client.log_message(MessageType::INFO, &message).await;

        // Ask the client to watch model + migration files. Clients that
        // don't support dynamic registration (older Zed builds, basic
        // LSP clients) simply ignore this — they'll still work via the
        // editor's own save / change notifications.
        let registration = crate::watched_files::build_registration();
        if let Err(err) = self.client.register_capability(vec![registration]).await {
            tracing::warn!(
                target: "vespertide_lsp::handler",
                error = %err,
                "client refused workspace/didChangeWatchedFiles registration; relying on editor save events"
            );
        }
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
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
            let domain = crate::hover::compute_with_workspace_tables(
                text,
                format,
                state.tree.as_ref(),
                self.index.as_ref(),
                self.store.as_ref(),
                Some(self.workspace_tables.as_ref()),
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
                crate::definition::compute_with_workspace_tables(
                    text,
                    format,
                    state.tree.as_ref(),
                    self.index.as_ref(),
                    self.store.as_ref(),
                    Some(self.workspace_tables.as_ref()),
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

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let pos_ls = params.text_document_position.position;
        let pos_lsp = crate::position::ls_to_lsp_position(pos_ls);
        let include_declaration = params.context.include_declaration;
        let Some(format) = DocumentFormat::from_uri(&uri) else {
            return Ok(None);
        };

        let domain_refs = self.store.docs_iter_for_uri(&uri, |state| {
            let text = state.text();
            let byte = crate::position::lsp_position_to_byte(&state.doc, pos_lsp);
            crate::references::compute(
                text,
                format,
                state.tree.as_ref(),
                &uri,
                self.index.as_ref(),
                self.store.as_ref(),
                Some(self.workspace_tables.as_ref()),
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
            .filter_map(|reference| domain_reference_to_location(&reference, self))
            .collect::<Vec<_>>();

        if locations.is_empty() {
            Ok(None)
        } else {
            Ok(Some(locations))
        }
    }

    async fn code_action(
        &self,
        params: CodeActionParams,
    ) -> Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri;
        let range_ls = params.range;
        let range_lsp = crate::position::ls_to_lsp_range(range_ls);
        let Some(format) = DocumentFormat::from_uri(&uri) else {
            return Ok(None);
        };

        let domain_actions = self.store.docs_iter_for_uri(&uri, |state| {
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
                let text_edits = domain_edits_to_lsp(&uri, &action.edits, self)?;
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

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let uri = params.text_document.uri;
        let range_ls = params.range;
        let range_lsp = crate::position::ls_to_lsp_range(range_ls);
        let Some(_format) = DocumentFormat::from_uri(&uri) else {
            return Ok(None);
        };

        let hints = self.store.docs_iter_for_uri(&uri, |state| {
            let text = state.text();
            let start = crate::position::lsp_position_to_byte(&state.doc, range_lsp.start);
            let end = crate::position::lsp_position_to_byte(&state.doc, range_lsp.end);
            let domain =
                crate::inlay_hints::compute(text, state.tree.as_ref(), start..end);
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

    #[allow(deprecated)]
    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<WorkspaceSymbolResponse>> {
        let query = params.query;
        let domain = crate::symbols::compute(
            &query,
            self.store.as_ref(),
            Some(self.workspace_tables.as_ref()),
        );

        tracing::info!(
            target: "vespertide_lsp::handler",
            query = %query,
            results = domain.len(),
            "workspace symbol"
        );

        let lsp_symbols: Vec<SymbolInformation> = domain
            .into_iter()
            .filter_map(|sym| symbol_to_lsp(&sym, self))
            .collect();
        if lsp_symbols.is_empty() {
            Ok(None)
        } else {
            Ok(Some(WorkspaceSymbolResponse::Flat(lsp_symbols)))
        }
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        handler_rename::prepare_rename_impl(self, params).await
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        handler_rename::rename_impl(self, params).await
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        Ok(crate::semantic_tokens::handler::compute_full(
            self.store.as_ref(),
            &params,
        ))
    }

    async fn semantic_tokens_range(
        &self,
        params: SemanticTokensRangeParams,
    ) -> Result<Option<SemanticTokensRangeResult>> {
        Ok(crate::semantic_tokens::handler::compute_range(
            self.store.as_ref(),
            &params,
        ))
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        let Some(root) = self.workspace_tables.root() else {
            return;
        };
        let models_dir = root.join("models");
        let migrations_dir = root.join("migrations");

        let mut touched = false;
        for event in &params.changes {
            let Some(path) = crate::position::uri_to_path(&event.uri) else {
                continue;
            };
            if crate::watched_files::should_refresh_for(
                &root,
                &models_dir,
                &migrations_dir,
                &path,
            ) {
                touched = true;
                break;
            }
        }
        if !touched {
            return;
        }

        self.workspace_tables.refresh(&root);
        tracing::info!(
            target: "vespertide_lsp::handler",
            changes = params.changes.len(),
            "did_change_watched_files: refreshed workspace_tables"
        );
        for uri in self.store.open_uris() {
            self.publish(uri).await;
        }
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        handler_file_features::document_symbol_impl(self, params).await
    }

    async fn folding_range(
        &self,
        params: FoldingRangeParams,
    ) -> Result<Option<Vec<FoldingRange>>> {
        handler_file_features::folding_range_impl(self, params).await
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        handler_file_features::document_highlight_impl(self, params).await
    }

    async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> Result<Option<Vec<SelectionRange>>> {
        handler_file_features::selection_range_impl(self, params).await
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
        tracing::info!(
            target: "vespertide_lsp::handler",
            uri = %uri.as_str(),
            language = %td.language_id,
            version = td.version,
            bytes = td.text.len(),
            "did_open"
        );
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
            tracing::debug!(
                target: "vespertide_lsp::handler",
                uri = %uri.as_str(),
                version = td.version,
                bytes = change.text.len(),
                "did_change"
            );
            self.store.update_full(&uri, change.text, td.version);
            self.reindex(&uri);
            self.refresh_workspace_tables_for_uri(&uri);
            self.publish(uri.clone()).await;
            self.publish_related(&uri).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        tracing::info!(
            target: "vespertide_lsp::handler",
            uri = %uri.as_str(),
            "did_save"
        );
        self.refresh_workspace_tables_for_uri(&uri);
        self.publish(uri.clone()).await;
        self.publish_related(&uri).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        tracing::info!(
            target: "vespertide_lsp::handler",
            uri = %uri.as_str(),
            "did_close"
        );
        self.store.close(&uri);
        self.index.remove(&uri);
        self.refresh_workspace_tables_for_uri(&uri);
        self.client
            .publish_diagnostics(uri.clone(), Vec::new(), None)
            .await;
        self.publish_related(&uri).await;
    }
}

mod handler_file_features;
mod handler_rename;
mod helpers;
use helpers::{
    byte_to_ls_position, diagnostic_severity_counts, domain_edits_to_lsp,
    domain_reference_to_location, domain_to_lsp, normalize_path, symbol_to_lsp,
};
