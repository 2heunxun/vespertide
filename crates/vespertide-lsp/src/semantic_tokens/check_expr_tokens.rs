//! Tokenise CHECK constraint expressions for semantic highlighting.
//!
//! Bridges the planner's span-aware CHECK lexer
//! (`vespertide_planner::lex_check_expr`) to LSP `RawToken`s. Each
//! lexeme's span (relative to the expression text) is translated to
//! an absolute document byte range by adding `inner_start` — the byte
//! offset of the first character of the expression *inside* the
//! enclosing JSON/YAML string value (i.e. after the opening quote).

use vespertide_planner::{CheckTokenKind, lex_check_expr};

use super::RawToken;
use super::legend::TokenIdx;

/// Emit one `RawToken` per highlightable CHECK lexeme, with byte
/// ranges absolute to the source document. Punctuation (`( ) ,`) is
/// skipped. Malformed expressions (lexer returns empty) emit nothing.
pub(super) fn emit_check_expr_tokens(expr_text: &str, inner_start: usize, out: &mut Vec<RawToken>) {
    for token in lex_check_expr(expr_text) {
        let Some(token_type) = token_kind_to_idx(token.kind) else {
            continue;
        };
        let abs = (inner_start + token.span.start)..(inner_start + token.span.end);
        out.push(RawToken {
            byte_range: abs,
            token_type: token_type as u32,
            token_modifiers: 0,
        });
    }
}

fn token_kind_to_idx(kind: CheckTokenKind) -> Option<TokenIdx> {
    match kind {
        CheckTokenKind::Column => Some(TokenIdx::Property),
        CheckTokenKind::Keyword | CheckTokenKind::Operator => Some(TokenIdx::Keyword),
        CheckTokenKind::Number => Some(TokenIdx::Number),
        CheckTokenKind::String => Some(TokenIdx::String),
        CheckTokenKind::Punctuation => None,
    }
}
