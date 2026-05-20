//! Fuzz LSP pure-domain request helpers. Arbitrary input should either produce
//! best-effort results or no result, but never panic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use vespertide_lsp::{
    DocumentFormat, DocumentStore, ParserPool, WorkspaceIndex, compute_completion,
    compute_definition, compute_diagnostics, compute_hover, format_text,
};

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);

    for format in [DocumentFormat::Json, DocumentFormat::Yaml] {
        let pool = ParserPool::new();
        let tree = pool.parse(&text, format);
        let index = WorkspaceIndex::new();
        let docs = DocumentStore::new();

        let _ = compute_diagnostics(&text, format, tree.as_ref(), &index);
        let _ = format_text(&text, format);

        for byte_offset in [0, text.len() / 2, text.len()] {
            let _ = compute_hover(&text, format, tree.as_ref(), &index, &docs, byte_offset);
            let _ = compute_definition(&text, format, tree.as_ref(), &index, &docs, byte_offset);
            let _ = compute_completion(&text, format, tree.as_ref(), &index, &docs, byte_offset);
        }
    }
});
