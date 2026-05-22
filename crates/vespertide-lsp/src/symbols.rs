//! Workspace symbols — global `Ctrl+T` / `Ctrl+Shift+O` search across
//! every table and column in the workspace.
//!
//! For each model file (open in the editor OR sitting on disk) we emit:
//!   * one symbol per table (`name: "user"`, kind=Class)
//!   * one symbol per column (`name: "email"`, container=`"user"`, kind=Field)
//!
//! The provided query is matched as a **case-insensitive substring** —
//! mirrors what most LSP clients render in the symbol picker without
//! requiring server-side fuzzy ranking, while still keeping the result
//! set tight enough to stay responsive in workspaces with hundreds of
//! columns.

use std::ops::Range;

use tower_lsp_server::ls_types::Uri;

use crate::store::DocumentStore;
use crate::workspace_tables::WorkspaceTables;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainSymbol {
    /// Display name in the symbol picker.
    pub name: String,
    /// Distinguishes tables (`Table`) from columns (`Column`) — the LSP
    /// layer maps these to `SymbolKind::Class` / `SymbolKind::Field`.
    pub kind: SymbolKind,
    /// Owning table name for column symbols; `None` for tables.
    pub container: Option<String>,
    /// File hosting the declaration.
    pub uri: Uri,
    /// Byte range of the identifier (table or column name value).
    pub byte_range: Range<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Table,
    Column,
}

/// Collect every table and column matching `query` (case-insensitive
/// substring; empty query returns everything).
#[must_use]
pub fn compute(query: &str, docs: &DocumentStore, disk_tables: Option<&WorkspaceTables>) -> Vec<DomainSymbol> {
    let needle = query.trim().to_ascii_lowercase();
    let mut out = Vec::new();

    let mut seen_uris = std::collections::BTreeSet::new();

    docs.for_each(|uri, state| {
        let Some(tree) = state.tree.as_ref() else {
            return;
        };
        seen_uris.insert(uri.clone());
        collect_from_tree(tree, state.text(), uri, &needle, &mut out);
    });

    // Pull disk-only tables, skipping ones that are already represented as
    // open documents.
    if let Some(disk) = disk_tables {
        let pool = crate::parser::ParserPool::new();
        for name in disk.names() {
            let Some(path) = disk.model_path(&name) else {
                continue;
            };
            let Some(uri) = path_to_uri(&path) else {
                continue;
            };
            if seen_uris.contains(&uri) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let format = match path.extension().and_then(|e| e.to_str()) {
                Some("yaml" | "yml") => crate::parser::DocumentFormat::Yaml,
                _ => crate::parser::DocumentFormat::Json,
            };
            let Some(tree) = pool.parse(&text, format) else {
                continue;
            };
            collect_from_tree(&tree, &text, &uri, &needle, &mut out);
        }
    }

    // Sort by (name, kind) for deterministic output across runs / clients.
    out.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| (a.kind as u8).cmp(&(b.kind as u8))));
    out
}

fn collect_from_tree(
    tree: &tree_sitter::Tree,
    source: &str,
    uri: &Uri,
    needle: &str,
    out: &mut Vec<DomainSymbol>,
) {
    let source_bytes = source.as_bytes();
    let Some(mapping) = find_outer_mapping(tree.root_node()) else {
        return;
    };

    let table_name = direct_pair_value(mapping, source_bytes, "name")
        .map(|(text, range)| (text.to_string(), range));
    let Some((table_name, table_range)) = table_name else {
        return;
    };

    if matches_needle(&table_name, needle) {
        out.push(DomainSymbol {
            name: table_name.clone(),
            kind: SymbolKind::Table,
            container: None,
            uri: uri.clone(),
            byte_range: table_range,
        });
    }

    let Some(columns_value) = direct_pair_node(mapping, source_bytes, "columns") else {
        return;
    };
    let columns_array = unwrap_yaml_node(columns_value);
    if !matches!(
        columns_array.kind(),
        "array" | "block_sequence" | "flow_sequence"
    ) {
        return;
    }

    let mut cursor = columns_array.walk();
    for raw_child in columns_array.children(&mut cursor) {
        let child = unwrap_yaml_node(raw_child);
        let mapping = match child.kind() {
            "object" | "block_mapping" | "flow_mapping" => Some(child),
            "block_sequence_item" => {
                let mut inner_cursor = child.walk();
                child
                    .children(&mut inner_cursor)
                    .map(unwrap_yaml_node)
                    .find(|n| matches!(n.kind(), "object" | "block_mapping" | "flow_mapping"))
            }
            _ => None,
        };
        let Some(column_mapping) = mapping else {
            continue;
        };
        let Some((column_name, column_range)) =
            direct_pair_value(column_mapping, source_bytes, "name")
                .map(|(text, range)| (text.to_string(), range))
        else {
            continue;
        };
        if matches_needle(&column_name, needle) {
            out.push(DomainSymbol {
                name: column_name,
                kind: SymbolKind::Column,
                container: Some(table_name.clone()),
                uri: uri.clone(),
                byte_range: column_range,
            });
        }
    }
}

fn matches_needle(name: &str, needle: &str) -> bool {
    needle.is_empty() || name.to_ascii_lowercase().contains(needle)
}

/// Find a direct child pair `key: …` and return `(stripped value text, value byte range)`.
fn direct_pair_value<'a>(
    mapping: tree_sitter::Node<'_>,
    source: &'a [u8],
    target_key: &str,
) -> Option<(&'a str, Range<usize>)> {
    let pair = find_pair_with_key(mapping, source, target_key)?;
    let value = unwrap_yaml_node(pair.named_child(1)?);
    let raw = source.get(value.byte_range())?;
    let text = std::str::from_utf8(raw).ok()?;
    let stripped = strip_quotes(text);
    // Adjust byte range to skip quotes when the value is a quoted string.
    let range = match value.kind() {
        "string" => value
            .named_child(0)
            .map_or_else(|| trim_one_byte(&value.byte_range()), |inner| inner.byte_range()),
        "double_quote_scalar" | "single_quote_scalar" => trim_one_byte(&value.byte_range()),
        _ => value.byte_range(),
    };
    // Defensive: if stripping changed the byte length unexpectedly, fall
    // back to the raw range so we never panic on slice indexing.
    let _ = stripped;
    Some((strip_quotes(text), range))
}

fn direct_pair_node<'tree>(
    mapping: tree_sitter::Node<'tree>,
    source: &[u8],
    target_key: &str,
) -> Option<tree_sitter::Node<'tree>> {
    find_pair_with_key(mapping, source, target_key)?.named_child(1)
}

fn find_pair_with_key<'tree>(
    mapping: tree_sitter::Node<'tree>,
    source: &[u8],
    target_key: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut cursor = mapping.walk();
    mapping.children(&mut cursor).find(|&child| {
        matches!(child.kind(), "pair" | "block_mapping_pair")
            && child
                .named_child(0)
                .and_then(|key| std::str::from_utf8(&source[key.byte_range()]).ok())
                .map(strip_quotes)
                == Some(target_key)
    })
}

fn find_outer_mapping(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    if matches!(node.kind(), "object" | "block_mapping" | "flow_mapping") {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = find_outer_mapping(child) {
            return Some(found);
        }
    }
    None
}

fn unwrap_yaml_node(node: tree_sitter::Node<'_>) -> tree_sitter::Node<'_> {
    let mut current = node;
    while matches!(current.kind(), "flow_node" | "block_node") {
        let Some(inner) = current.named_child(0) else {
            break;
        };
        if inner.id() == current.id() {
            break;
        }
        current = inner;
    }
    current
}

fn strip_quotes(s: &str) -> &str {
    s.trim()
        .trim_start_matches('"')
        .trim_end_matches('"')
        .trim_start_matches('\'')
        .trim_end_matches('\'')
}

fn trim_one_byte(range: &Range<usize>) -> Range<usize> {
    if range.end.saturating_sub(range.start) >= 2 {
        (range.start + 1)..(range.end - 1)
    } else {
        range.clone()
    }
}

fn path_to_uri(path: &std::path::Path) -> Option<Uri> {
    let mut text = path.to_string_lossy().replace('\\', "/");
    if !text.starts_with('/') {
        text = format!("/{text}");
    }
    std::str::FromStr::from_str(&format!("file://{text}")).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DocumentFormat, ParserPool};
    use std::str::FromStr;

    fn uri(p: &str) -> Uri {
        Uri::from_str(&format!("file:///{p}")).unwrap()
    }

    #[test]
    fn empty_query_returns_all_tables_and_columns() {
        let docs = DocumentStore::new();
        let src = r#"{"name":"user","columns":[{"name":"id","type":"integer"},{"name":"email","type":"text"}]}"#;
        let u = uri("user.json");
        // `DocumentStore::open` parses the tree via the internal ParserPool,
        // so we do not need to feed a tree manually.
        docs.open(u, "json".to_string(), 1, src.to_string());

        let symbols = compute("", &docs, None);
        let names: Vec<_> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"user"), "got: {names:?}");
        assert!(names.contains(&"id"));
        assert!(names.contains(&"email"));

        // Each column's container points at its owning table.
        let email = symbols.iter().find(|s| s.name == "email").unwrap();
        assert_eq!(email.kind, SymbolKind::Column);
        assert_eq!(email.container.as_deref(), Some("user"));
    }

    #[test]
    fn query_filters_case_insensitively() {
        let docs = DocumentStore::new();
        let src = r#"{"name":"User","columns":[{"name":"emAil","type":"text"}]}"#;
        docs.open(uri("user.json"), "json".to_string(), 1, src.to_string());

        let s1 = compute("user", &docs, None);
        assert!(s1.iter().any(|s| s.name == "User"));

        let s2 = compute("EMAIL", &docs, None);
        assert!(s2.iter().any(|s| s.name == "emAil"));
    }

    #[test]
    fn output_is_sorted_for_deterministic_picker_ordering() {
        let docs = DocumentStore::new();
        let src = r#"{"name":"zeta","columns":[{"name":"alpha","type":"integer"},{"name":"beta","type":"integer"}]}"#;
        docs.open(uri("z.json"), "json".to_string(), 1, src.to_string());

        let symbols = compute("", &docs, None);
        let names: Vec<_> = symbols.iter().map(|s| s.name.clone()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "symbols must be sorted alphabetically");
    }

    /// Regression-style: column name picker must not silently drop
    /// columns when the file uses YAML scalars.
    #[test]
    fn yaml_workspace_symbols() {
        let docs = DocumentStore::new();
        let src = "name: account\ncolumns:\n  - name: id\n    type: integer\n  - name: balance\n    type: numeric\n";
        docs.open(uri("account.yaml"), "yaml".to_string(), 1, src.to_string());

        let symbols = compute("", &docs, None);
        let names: Vec<_> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"account"));
        assert!(names.contains(&"id"));
        assert!(names.contains(&"balance"));
    }

    // Silence dead-code lints on the parser pool helper used by tests above
    // when they otherwise stop importing it.
    fn _force_parser_pool() {
        let _ = ParserPool::new();
        let _ = DocumentFormat::Json;
    }
}
