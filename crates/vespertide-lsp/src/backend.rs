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
    CompletionParams, CompletionResponse, CompletionTextEdit, Diagnostic, DiagnosticSeverity,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, DocumentFormattingParams, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverContents, HoverParams, HoverProviderCapability,
    InitializeParams, InitializeResult, InitializedParams, InsertTextFormat, Location,
    MarkupContent, MarkupKind, MessageType, NumberOrString, OneOf, Position, Range,
    CodeAction, CodeActionKind as LspCodeActionKind, CodeActionOptions, CodeActionOrCommand,
    CodeActionParams, CodeActionProviderCapability, CodeActionResponse, ReferenceParams,
    RenameParams, ServerCapabilities, ServerInfo, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextDocumentSyncOptions, TextDocumentSyncSaveOptions, TextEdit, Uri,
    WorkDoneProgressOptions, WorkspaceEdit,
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
                rename_provider: Some(OneOf::Left(true)),
                code_action_provider: Some(CodeActionProviderCapability::Options(
                    CodeActionOptions {
                        code_action_kinds: Some(vec![LspCodeActionKind::REFACTOR]),
                        resolve_provider: Some(false),
                        work_done_progress_options: WorkDoneProgressOptions::default(),
                    },
                )),
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

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri;
        let pos_ls = params.text_document_position.position;
        let pos_lsp = crate::position::ls_to_lsp_position(pos_ls);
        let new_name = params.new_name;
        let Some(format) = DocumentFormat::from_uri(&uri) else {
            return Ok(None);
        };

        let domain = self.store.docs_iter_for_uri(&uri, |state| {
            let text = state.text();
            let byte = crate::position::lsp_position_to_byte(&state.doc, pos_lsp);
            crate::rename::compute(
                text,
                format,
                state.tree.as_ref(),
                &uri,
                self.index.as_ref(),
                self.store.as_ref(),
                Some(self.workspace_tables.as_ref()),
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

        let mut changes = std::collections::HashMap::new();
        for (target_uri, domain_edits) in domain.edits {
            let Some(text_edits) = domain_edits_to_lsp(&target_uri, &domain_edits, self) else {
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

#[derive(Default)]
struct DiagnosticSeverityCounts {
    errors: usize,
    warnings: usize,
}

fn diagnostic_severity_counts(diagnostics: &[Diagnostic]) -> DiagnosticSeverityCounts {
    let mut counts = DiagnosticSeverityCounts::default();
    for diag in diagnostics {
        match diag.severity {
            Some(DiagnosticSeverity::ERROR) => counts.errors += 1,
            Some(DiagnosticSeverity::WARNING) => counts.warnings += 1,
            _ => {}
        }
    }
    counts
}

/// Translate a [`crate::completion::DomainCompletion`] into the LSP wire
/// shape. When the domain layer supplies a byte range to replace, we lower
/// it to a `TextEdit` so the client wipes the existing string (quotes and
/// all) before inserting the snippet — that is what makes typing `varchar`
/// inside `""` collapse the quotes and unfold into a `{...}` object literal.
fn domain_to_lsp(
    item: crate::completion::DomainCompletion,
    doc: &lsp_textdocument::FullTextDocument,
) -> CompletionItem {
    let kind = Some(match item.kind {
        crate::completion::CompletionItemKind::Value => LspCompletionItemKind::VALUE,
        crate::completion::CompletionItemKind::Property => LspCompletionItemKind::PROPERTY,
        crate::completion::CompletionItemKind::Reference => LspCompletionItemKind::REFERENCE,
        crate::completion::CompletionItemKind::Snippet => LspCompletionItemKind::SNIPPET,
    });

    let text_edit = item.replace_range_bytes.as_ref().map(|range| {
        let start = byte_to_ls_position(doc, range.start);
        let end = byte_to_ls_position(doc, range.end);
        let new_text = item
            .insert_text
            .clone()
            .unwrap_or_else(|| item.label.clone());
        CompletionTextEdit::Edit(TextEdit {
            range: Range { start, end },
            new_text,
        })
    });

    let insert_text_format = item.insert_text.as_ref().map(|_| InsertTextFormat::SNIPPET);
    // Per LSP spec: when text_edit is set, the client ignores insert_text.
    // Suppress it so the two never disagree.
    let insert_text = if text_edit.is_some() {
        None
    } else {
        item.insert_text
    };
    let sort_text = Some(format!("{:03}{}", item.sort_priority, item.label));

    CompletionItem {
        label: item.label,
        kind,
        detail: item.detail,
        text_edit,
        insert_text_format,
        insert_text,
        sort_text,
        ..CompletionItem::default()
    }
}

fn byte_to_ls_position(
    doc: &lsp_textdocument::FullTextDocument,
    byte_offset: usize,
) -> tower_lsp_server::ls_types::Position {
    crate::position::lsp_to_ls_position(crate::position::byte_to_lsp_position(doc, byte_offset))
}

/// Best-effort filesystem path normalization for workspace dedup.
///
/// 1. `std::fs::canonicalize` when the file exists — that is the most
///    reliable cross-tool match (resolves symlinks + UNC + casing).
/// 2. Fallback to forward-slash + lowercase rewrite so Windows files that
///    differ only in drive-letter case still compare equal.
fn normalize_path(path: &std::path::Path) -> std::path::PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }
    let lossy = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        std::path::PathBuf::from(lossy.to_lowercase())
    } else {
        std::path::PathBuf::from(lossy)
    }
}

/// Convert a list of [`crate::rename::DomainTextEdit`] into LSP `TextEdit`s
/// for `target_uri`. Mirrors the open-vs-disk fallback used by references.
fn domain_edits_to_lsp(
    target_uri: &Uri,
    domain_edits: &[crate::rename::DomainTextEdit],
    backend: &Backend,
) -> Option<Vec<TextEdit>> {
    let to_lsp = |doc: &lsp_textdocument::FullTextDocument| {
        domain_edits
            .iter()
            .map(|edit| TextEdit {
                range: Range {
                    start: byte_to_ls_position(doc, edit.byte_range.start),
                    end: byte_to_ls_position(doc, edit.byte_range.end),
                },
                new_text: edit.new_text.clone(),
            })
            .collect()
    };

    if let Some(edits) = backend
        .store
        .docs_iter_for_uri(target_uri, |state| to_lsp(&state.doc))
    {
        return Some(edits);
    }

    let path = crate::position::uri_to_path(target_uri)?;
    let text = std::fs::read_to_string(&path).ok()?;
    let language_id = match path.extension().and_then(|e| e.to_str()) {
        Some("yaml" | "yml") => "yaml",
        _ => "json",
    };
    let doc = lsp_textdocument::FullTextDocument::new(language_id.to_string(), 1, text);
    Some(to_lsp(&doc))
}

/// Convert a [`crate::references::DomainReference`] into an LSP [`Location`].
///
/// When the target URI is an open document we use its `FullTextDocument`
/// for accurate UTF-16 offset conversion. For disk-only files we read the
/// source and build a transient document. Returns `None` only when both
/// fail.
fn domain_reference_to_location(
    reference: &crate::references::DomainReference,
    backend: &Backend,
) -> Option<Location> {
    if let Some(range) = backend
        .store
        .docs_iter_for_uri(&reference.uri, |state| {
            Range {
                start: byte_to_ls_position(&state.doc, reference.byte_range.start),
                end: byte_to_ls_position(&state.doc, reference.byte_range.end),
            }
        })
    {
        return Some(Location {
            uri: reference.uri.clone(),
            range,
        });
    }

    // Disk-only file — read source on demand.
    let path = crate::position::uri_to_path(&reference.uri)?;
    let text = std::fs::read_to_string(&path).ok()?;
    let language_id = match path.extension().and_then(|e| e.to_str()) {
        Some("yaml" | "yml") => "yaml",
        _ => "json",
    };
    let doc = lsp_textdocument::FullTextDocument::new(language_id.to_string(), 1, text);
    Some(Location {
        uri: reference.uri.clone(),
        range: Range {
            start: byte_to_ls_position(&doc, reference.byte_range.start),
            end: byte_to_ls_position(&doc, reference.byte_range.end),
        },
    })
}
