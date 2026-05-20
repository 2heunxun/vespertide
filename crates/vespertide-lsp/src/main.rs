//! `vespertide-lsp` binary entry point.
//!
//! Wires the [`Backend`] into a tower-lsp-server stdio transport so editors
//! (VS Code, Neovim, etc.) can spawn the language server and communicate
//! over stdin/stdout. All diagnostics go to stderr to keep stdout reserved
//! for the LSP framed JSON-RPC protocol.

use tower_lsp_server::{LspService, Server};
use tracing_subscriber::EnvFilter;
use vespertide_lsp::Backend;

#[tokio::main]
async fn main() {
    // tracing -> stderr (stdout is reserved for LSP stdio framing).
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
