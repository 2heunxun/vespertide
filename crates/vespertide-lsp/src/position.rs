//! UTF-16 (LSP) ↔ byte offset (tree-sitter/Rust) position conversions.
//!
//! Uses `lsp-textdocument`'s [`FullTextDocument`] which handles this correctly
//! (verified by rust-analyzer + nushell switching to it after ropey UTF-8 bugs).
//!
//! Also provides small bridges between `tower_lsp_server::ls_types` and the
//! upstream `lsp_types` crate. The two are structurally identical at the
//! position/range level but are distinct types because tower-lsp-server
//! maintains a fork; conversion happens at the I/O seam, never inside the
//! analysis engine.

use lsp_textdocument::FullTextDocument;
use tower_lsp_server::ls_types::Uri;

/// Convert an LSP `lsp_types::Position` to a UTF-8 byte offset.
#[must_use]
pub fn lsp_position_to_byte(doc: &FullTextDocument, pos: lsp_types::Position) -> usize {
    doc.offset_at(pos) as usize
}

/// Convert a UTF-8 byte offset to an LSP `lsp_types::Position`.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn byte_to_lsp_position(doc: &FullTextDocument, byte_offset: usize) -> lsp_types::Position {
    doc.position_at(byte_offset as u32)
}

/// Bridge from tower-lsp-server's `ls_types::Position` to `lsp_types::Position`.
/// They're structurally identical but type-distinct (different crates).
#[must_use]
pub fn ls_to_lsp_position(p: tower_lsp_server::ls_types::Position) -> lsp_types::Position {
    lsp_types::Position {
        line: p.line,
        character: p.character,
    }
}

/// Bridge from `lsp_types::Position` to `ls_types::Position`.
#[must_use]
pub fn lsp_to_ls_position(p: lsp_types::Position) -> tower_lsp_server::ls_types::Position {
    tower_lsp_server::ls_types::Position {
        line: p.line,
        character: p.character,
    }
}

/// Bridge from tower-lsp-server's `ls_types::Range` to `lsp_types::Range`.
#[must_use]
pub fn ls_to_lsp_range(r: tower_lsp_server::ls_types::Range) -> lsp_types::Range {
    lsp_types::Range {
        start: ls_to_lsp_position(r.start),
        end: ls_to_lsp_position(r.end),
    }
}

/// Convert a `file://` URI into a local filesystem path.
#[must_use]
pub fn uri_to_path(uri: &Uri) -> Option<std::path::PathBuf> {
    let uri_text = uri.to_string();
    let path = uri_text.strip_prefix("file://")?;

    let path = if cfg!(windows) {
        path.strip_prefix('/')
            .filter(|without_slash| has_windows_drive_prefix(without_slash))
            .unwrap_or(path)
            .replace('/', std::path::MAIN_SEPARATOR_STR)
    } else {
        path.to_string()
    };

    Some(std::path::PathBuf::from(path))
}

fn has_windows_drive_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_textdocument::FullTextDocument;

    fn doc(text: &str) -> FullTextDocument {
        FullTextDocument::new("json".to_string(), 1, text.to_string())
    }

    #[test]
    fn ascii_round_trip() {
        let d = doc("hello world");
        let pos = lsp_types::Position {
            line: 0,
            character: 6,
        };
        let byte = lsp_position_to_byte(&d, pos);
        assert_eq!(byte, 6);
        let pos2 = byte_to_lsp_position(&d, byte);
        assert_eq!(pos2, pos);
    }

    #[test]
    fn cjk_round_trip() {
        // "도서" = 2 chars, 6 bytes UTF-8, 2 UTF-16 code units (BMP).
        let d = doc("도서 test");
        let pos = lsp_types::Position {
            line: 0,
            character: 2,
        }; // after "도서"
        let byte = lsp_position_to_byte(&d, pos);
        assert_eq!(byte, 6); // 2 chars × 3 bytes each
    }

    #[test]
    fn emoji_round_trip() {
        // "🚀" = 1 char, 4 bytes UTF-8, 2 UTF-16 code units (surrogate pair).
        let d = doc("🚀test");
        let pos_after_emoji = lsp_types::Position {
            line: 0,
            character: 2,
        }; // 2 UTF-16 units
        let byte = lsp_position_to_byte(&d, pos_after_emoji);
        assert_eq!(byte, 4); // 4 UTF-8 bytes
    }

    #[test]
    fn multiline_position() {
        let d = doc("line one\nline two\nline three");
        let pos = lsp_types::Position {
            line: 1,
            character: 5,
        }; // "line " on line 2
        let byte = lsp_position_to_byte(&d, pos);
        assert_eq!(byte, "line one\nline ".len());
    }

    #[test]
    fn position_bridge_round_trip() {
        let p = lsp_types::Position {
            line: 5,
            character: 10,
        };
        let ls = lsp_to_ls_position(p);
        let back = ls_to_lsp_position(ls);
        assert_eq!(back, p);
    }

    #[test]
    fn range_bridge() {
        let ls = tower_lsp_server::ls_types::Range {
            start: tower_lsp_server::ls_types::Position {
                line: 1,
                character: 2,
            },
            end: tower_lsp_server::ls_types::Position {
                line: 3,
                character: 4,
            },
        };
        let r = ls_to_lsp_range(ls);
        assert_eq!(r.start.line, 1);
        assert_eq!(r.start.character, 2);
        assert_eq!(r.end.line, 3);
        assert_eq!(r.end.character, 4);
    }
}
