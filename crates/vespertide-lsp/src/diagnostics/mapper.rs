//! Map `DomainDiagnostic` → `tower_lsp_server::ls_types::Diagnostic`.

use lsp_textdocument::FullTextDocument;
use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, NumberOrString};

use super::{DomainDiagnostic, Severity};
use crate::position::byte_to_lsp_position;

#[must_use]
pub fn to_lsp(domain: &DomainDiagnostic, doc: &FullTextDocument) -> Diagnostic {
    let start = byte_to_lsp_position(doc, domain.byte_range.start);
    let end = byte_to_lsp_position(doc, domain.byte_range.end);

    Diagnostic {
        range: tower_lsp_server::ls_types::Range {
            start: tower_lsp_server::ls_types::Position {
                line: start.line,
                character: start.character,
            },
            end: tower_lsp_server::ls_types::Position {
                line: end.line,
                character: end.character,
            },
        },
        severity: Some(match domain.severity {
            Severity::Error => DiagnosticSeverity::ERROR,
            Severity::Warning => DiagnosticSeverity::WARNING,
            Severity::Information => DiagnosticSeverity::INFORMATION,
            Severity::Hint => DiagnosticSeverity::HINT,
        }),
        code: Some(NumberOrString::String(domain.code.clone())),
        code_description: None,
        source: Some("vespertide-lsp".to_string()),
        message: domain.message.clone(),
        related_information: None,
        tags: None,
        data: None,
    }
}
