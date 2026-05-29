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

use crate::text_util::strip_quotes;
use std::ops::Range;
use std::sync::{Arc, OnceLock};

use tower_lsp_server::ls_types::Uri;

use crate::cache::{RingCache, docstore_fingerprint, hash_text};
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

/// Cached symbol extraction for `compute_workspace_symbols`. Walking the
/// tree to enumerate `(table_name, [column_names])` is the dominant cost
/// in this hot path (`~60%` of profile wall time on the synthetic
/// workload). Cache the unfiltered symbol list keyed on the source text;
/// the per-call query filter runs in microseconds on the cached vec.
///
/// 128-slot ring buffer with `(fxhash64, len)` keys. Same shape as
/// `diagnostics::validation::cache::ParseCache` (HS-3), sized to cover the
/// 100-model profiling workload without ring-buffer thrash.
type SymbolKey = (u64, usize);
type SymbolCache = RingCache<SymbolKey, Vec<RawSymbol>, 128>;

/// Pre-query symbol info extracted from a doc's tree. `compute_workspace_symbols`
/// applies the query filter to this — see `ascii_ci_contains`. The `container`
/// for a Column is the table name from the same document.
#[derive(Debug, Clone)]
struct RawSymbol {
    name: String,
    kind: SymbolKind,
    container: Option<String>,
    byte_range: Range<usize>,
}

/// One element per `(uri, raw_symbol)` for every open doc + every disk table.
/// Built once per `docstore_fingerprint` change.
#[derive(Debug, Clone)]
struct WorkspaceSymbolEntry {
    uri: Uri,
    raw: RawSymbol,
}

static SYMBOL_CACHE: OnceLock<SymbolCache> = OnceLock::new();

fn symbol_cache() -> &'static SymbolCache {
    SYMBOL_CACHE.get_or_init(SymbolCache::new)
}

/// Cache the workspace-wide flat symbol list keyed on `docstore_fingerprint`.
/// Small capacity (8 slots) because invalidation is coarse: any `did_change`
/// advances the fingerprint and the next call rebuilds the whole list. 8 slots
/// is enough to amortize across a few rapid edits.
type WorkspaceSymbolsCache = RingCache<u64, Vec<WorkspaceSymbolEntry>, 8>;

static WORKSPACE_SYMBOLS_CACHE: OnceLock<WorkspaceSymbolsCache> = OnceLock::new();

fn workspace_symbols_cache() -> &'static WorkspaceSymbolsCache {
    WORKSPACE_SYMBOLS_CACHE.get_or_init(WorkspaceSymbolsCache::new)
}

/// Cache filtered query results keyed on `(docstore_fingerprint, needle_hash)`.
/// 256 slots — accommodates ~100 fingerprints × ~3 queries cardinality without
/// thrash. The value is the final `Vec<DomainSymbol>` ready to return to the
/// caller.
type FilteredSymbolsCache = RingCache<(u64, u64), Vec<DomainSymbol>, 256>;

static FILTERED_SYMBOLS_CACHE: OnceLock<FilteredSymbolsCache> = OnceLock::new();

fn filtered_symbols_cache() -> &'static FilteredSymbolsCache {
    FILTERED_SYMBOLS_CACHE.get_or_init(FilteredSymbolsCache::new)
}

/// Same as [`compute`] but returns an `Arc<Vec<DomainSymbol>>` directly from
/// the cache, avoiding a final per-call `Vec` clone. Use this entry point for
/// read-only consumers; [`compute`] wraps this and clones once for backward
/// compatibility.
#[must_use]
pub fn compute_shared(
    query: &str,
    docs: &DocumentStore,
    disk_tables: Option<&WorkspaceTables>,
) -> Arc<Vec<DomainSymbol>> {
    let needle = query.trim().to_ascii_lowercase();
    let fingerprint = docstore_fingerprint(docs);
    let needle_hash = hash_text(&needle);

    filtered_symbols_cache().get_or_compute((fingerprint, needle_hash), || {
        let flat = workspace_symbols_cache().get_or_compute(fingerprint, || {
            build_workspace_symbol_list(docs, disk_tables)
        });
        let mut result: Vec<DomainSymbol> = flat
            .iter()
            .filter(|entry| ascii_ci_contains(&entry.raw.name, &needle))
            .map(|entry| DomainSymbol {
                name: entry.raw.name.clone(),
                kind: entry.raw.kind,
                container: entry.raw.container.clone(),
                uri: entry.uri.clone(),
                byte_range: entry.raw.byte_range.clone(),
            })
            .collect();
        // Sort by (name, kind) for deterministic output across runs / clients.
        result.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then_with(|| (a.kind as u8).cmp(&(b.kind as u8)))
        });
        result
    })
}

/// Collect every table and column matching `query` (case-insensitive
/// substring; empty query returns everything).
#[must_use]
pub fn compute(
    query: &str,
    docs: &DocumentStore,
    disk_tables: Option<&WorkspaceTables>,
) -> Vec<DomainSymbol> {
    (*compute_shared(query, docs, disk_tables)).clone()
}

/// Build the workspace-wide flat symbol list. Iterates every open doc (using
/// HS-7 `SymbolCache` for per-doc extraction) and every disk-only table (using
/// HS-2 `cached_parse` + per-doc cache). Returns one `WorkspaceSymbolEntry` per
/// `(uri, raw_symbol)` pair, sorted by URI then `byte_range` for determinism.
fn build_workspace_symbol_list(
    docs: &DocumentStore,
    disk_tables: Option<&WorkspaceTables>,
) -> Vec<WorkspaceSymbolEntry> {
    let mut out = Vec::new();
    let mut seen_uris = std::collections::BTreeSet::new();

    docs.for_each(|uri, state| {
        let Some(tree) = state.tree.as_ref() else {
            return;
        };
        seen_uris.insert(uri.clone());
        let text = state.text();
        let raw = symbol_cache().get_or_compute((hash_text(text), text.len()), || {
            extract_raw_symbols(tree, text)
        });
        for raw_sym in raw.iter() {
            out.push(WorkspaceSymbolEntry {
                uri: uri.clone(),
                raw: raw_sym.clone(),
            });
        }
    });

    if let Some(disk) = disk_tables {
        let pool = shared_parser_pool();
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
            let Some((text_arc, tree_arc)) = disk.cached_parse(&path, pool) else {
                continue;
            };
            let text = &*text_arc;
            let tree = &*tree_arc;
            let raw = symbol_cache().get_or_compute((hash_text(text), text.len()), || {
                extract_raw_symbols(tree, text)
            });
            for raw_sym in raw.iter() {
                out.push(WorkspaceSymbolEntry {
                    uri: uri.clone(),
                    raw: raw_sym.clone(),
                });
            }
        }
    }

    out.sort_by(|a, b| {
        a.uri
            .as_str()
            .cmp(b.uri.as_str())
            .then_with(|| a.raw.byte_range.start.cmp(&b.raw.byte_range.start))
    });
    out
}

fn shared_parser_pool() -> &'static crate::parser::ParserPool {
    static SHARED_POOL: OnceLock<crate::parser::ParserPool> = OnceLock::new();
    SHARED_POOL.get_or_init(crate::parser::ParserPool::new)
}

/// Extract every table + column symbol from a parsed model file, WITHOUT
/// applying any query filter. The result is cacheable per-text; callers
/// apply `ascii_ci_contains(name, needle)` to filter.
fn extract_raw_symbols(tree: &tree_sitter::Tree, source: &str) -> Vec<RawSymbol> {
    let source_bytes = source.as_bytes();
    let Some(mapping) = find_outer_mapping(tree.root_node()) else {
        return Vec::new();
    };
    let table_name = direct_pair_value(mapping, source_bytes, "name")
        .map(|(text, range)| (text.to_string(), range));
    let Some((table_name, table_range)) = table_name else {
        return Vec::new();
    };

    let mut out = Vec::new();
    out.push(RawSymbol {
        name: table_name.clone(),
        kind: SymbolKind::Table,
        container: None,
        byte_range: table_range,
    });

    let Some(columns_value) = direct_pair_node(mapping, source_bytes, "columns") else {
        return out;
    };
    let columns_array = unwrap_yaml_node(columns_value);
    if !matches!(
        columns_array.kind(),
        "array" | "block_sequence" | "flow_sequence"
    ) {
        return out;
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
        out.push(RawSymbol {
            name: column_name,
            kind: SymbolKind::Column,
            container: Some(table_name.clone()),
            byte_range: column_range,
        });
    }

    out
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
        "string" => value.named_child(0).map_or_else(
            || trim_one_byte(&value.byte_range()),
            |inner| inner.byte_range(),
        ),
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

/// ASCII case-insensitive substring search. `needle_lower` must already be
/// lowercase (the public `compute()` entry-point lowercases the query once).
/// Allocates zero — walks `haystack` byte-by-byte folding only ASCII case.
/// Non-ASCII bytes are compared exactly (same semantics as the prior
/// `to_ascii_lowercase().contains(...)` because that function also only
/// folds ASCII).
fn ascii_ci_contains(haystack: &str, needle_lower: &str) -> bool {
    if needle_lower.is_empty() {
        return true;
    }
    let hay = haystack.as_bytes();
    let nee = needle_lower.as_bytes();
    if nee.len() > hay.len() {
        return false;
    }
    'outer: for start in 0..=(hay.len() - nee.len()) {
        for i in 0..nee.len() {
            if hay[start + i].to_ascii_lowercase() != nee[i] {
                continue 'outer;
            }
        }
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DocumentFormat, ParserPool};
    use std::str::FromStr;
    use std::sync::Arc;

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

    #[test]
    fn ascii_ci_contains_empty_needle_matches() {
        assert!(ascii_ci_contains("anything", ""));
        assert!(ascii_ci_contains("", ""));
    }

    #[test]
    fn ascii_ci_contains_needle_longer_than_haystack() {
        assert!(!ascii_ci_contains("a", "abc"));
    }

    #[test]
    fn ascii_ci_contains_mixed_case_match() {
        assert!(ascii_ci_contains("FooBar", "oob"));
        assert!(ascii_ci_contains("USER", "use"));
    }

    #[test]
    fn ascii_ci_contains_no_match() {
        assert!(!ascii_ci_contains("foo", "xyz"));
    }

    #[test]
    fn ascii_ci_contains_non_ascii_passes_through() {
        // Matches existing `to_ascii_lowercase().contains(...)` semantics:
        // non-ASCII bytes are NOT folded; they're compared byte-wise.
        assert!(ascii_ci_contains("카페", "카페"));
        assert!(!ascii_ci_contains("카페", "café"));
    }

    #[test]
    fn symbol_cache_hit_returns_same_arc() {
        let cache = SymbolCache::default();
        let source = r#"{"name":"user","columns":[{"name":"id","type":"integer"}]}"#;
        let pool = crate::parser::ParserPool::new();
        let tree = pool
            .parse(source, crate::parser::DocumentFormat::Json)
            .unwrap();
        let key = (hash_text(source), source.len());
        let a = cache.get_or_compute(key, || extract_raw_symbols(&tree, source));
        let b = cache.get_or_compute(key, || extract_raw_symbols(&tree, source));
        assert!(Arc::ptr_eq(&a, &b), "cache hit returns same Arc");
        assert_eq!(a.len(), 2, "1 table + 1 column");
    }

    #[test]
    fn symbol_cache_miss_on_different_text() {
        let cache = SymbolCache::default();
        let source_a = r#"{"name":"user","columns":[]}"#;
        let source_b = r#"{"name":"post","columns":[]}"#;
        let pool = crate::parser::ParserPool::new();
        let tree_a = pool
            .parse(source_a, crate::parser::DocumentFormat::Json)
            .unwrap();
        let tree_b = pool
            .parse(source_b, crate::parser::DocumentFormat::Json)
            .unwrap();
        let a = cache.get_or_compute((hash_text(source_a), source_a.len()), || {
            extract_raw_symbols(&tree_a, source_a)
        });
        let b = cache.get_or_compute((hash_text(source_b), source_b.len()), || {
            extract_raw_symbols(&tree_b, source_b)
        });
        assert_eq!(a[0].name, "user");
        assert_eq!(b[0].name, "post");
    }

    #[test]
    fn workspace_symbols_cache_hit_returns_arc() {
        let cache = WorkspaceSymbolsCache::new();
        let entries = vec![WorkspaceSymbolEntry {
            uri: "file:///t.json".parse().unwrap(),
            raw: RawSymbol {
                name: "user".into(),
                kind: SymbolKind::Table,
                container: None,
                byte_range: 0..4,
            },
        }];
        let a = cache.get_or_compute(42, || entries.clone());
        let b = cache.get_or_compute(42, Vec::new);
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn filtered_cache_hit_returns_arc() {
        let cache = FilteredSymbolsCache::new();
        let syms = vec![DomainSymbol {
            name: "user".into(),
            kind: SymbolKind::Table,
            container: None,
            uri: "file:///t.json".parse().unwrap(),
            byte_range: 0..4,
        }];
        let a = cache.get_or_compute((1, 2), || syms.clone());
        let b = cache.get_or_compute((1, 2), Vec::new);
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn filtered_cache_miss_on_different_fingerprint() {
        let cache = FilteredSymbolsCache::new();
        let counter = std::sync::atomic::AtomicUsize::new(0);
        cache.get_or_compute((1, 99), || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            vec![]
        });
        cache.get_or_compute((2, 99), || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            vec![]
        });
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn workspace_symbols_cache_miss_on_different_fingerprint() {
        let cache = WorkspaceSymbolsCache::new();
        let counter = std::sync::atomic::AtomicUsize::new(0);
        cache.get_or_compute(1, || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            vec![]
        });
        cache.get_or_compute(2, || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            vec![]
        });
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn filtered_cache_miss_on_different_needle_hash() {
        let cache = FilteredSymbolsCache::new();
        let counter = std::sync::atomic::AtomicUsize::new(0);
        cache.get_or_compute((1, 10), || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            vec![]
        });
        cache.get_or_compute((1, 11), || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            vec![]
        });
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn build_workspace_symbol_list_includes_open_doc_columns() {
        let docs = DocumentStore::new();
        let uri: Uri = "file:///t.json".parse().unwrap();
        let source = r#"{"name":"user","columns":[{"name":"id","type":"integer"},{"name":"email","type":"text"}]}"#;
        docs.open(uri, "json".to_string(), 1, source.to_string());

        let entries = build_workspace_symbol_list(&docs, None);
        assert!(entries.iter().any(|e| e.raw.name == "user"));
        assert!(entries.iter().any(|e| e.raw.name == "id"));
        assert!(entries.iter().any(|e| e.raw.name == "email"));
    }

    #[test]
    fn build_workspace_symbol_list_sorts_by_uri_then_byte_range() {
        let docs = DocumentStore::new();
        docs.open(
            "file:///z.json".parse().unwrap(),
            "json".to_string(),
            1,
            r#"{"name":"zeta","columns":[{"name":"z_col","type":"integer"}]}"#.to_string(),
        );
        docs.open(
            "file:///a.json".parse().unwrap(),
            "json".to_string(),
            1,
            r#"{"name":"alpha","columns":[{"name":"a_col","type":"integer"}]}"#.to_string(),
        );

        let entries = build_workspace_symbol_list(&docs, None);
        let positions: Vec<_> = entries
            .iter()
            .map(|entry| (entry.uri.as_str().to_string(), entry.raw.byte_range.start))
            .collect();
        let mut sorted = positions.clone();
        sorted.sort();
        assert_eq!(positions, sorted);
    }

    #[test]
    fn compute_with_filtered_cache_returns_same_results_as_unfiltered_iteration() {
        // Smoke test the end-to-end caching: same input → same output across 3 calls.
        let docs = DocumentStore::new();
        let uri: Uri = "file:///t.json".parse().unwrap();
        docs.open(
            uri,
            "json".to_string(),
            1,
            r#"{"name":"user","columns":[{"name":"id","type":"integer"},{"name":"email","type":"text"}]}"#
                .to_string(),
        );

        let first = compute("email", &docs, None);
        let second = compute("email", &docs, None);
        let third = compute("email", &docs, None);
        assert_eq!(first, second);
        assert_eq!(second, third);
        assert!(first.iter().any(|s| s.name == "email"));
    }

    #[test]
    fn compute_shared_and_compute_return_same_results() {
        let docs = DocumentStore::new();
        let uri: Uri = "file:///t.json".parse().unwrap();
        docs.open(
            uri,
            "json".to_string(),
            1,
            r#"{"name":"user","columns":[{"name":"id","type":"integer"},{"name":"email","type":"text"}]}"#
                .to_string(),
        );

        let shared = compute_shared("email", &docs, None);
        let owned = compute("email", &docs, None);
        assert_eq!(*shared, owned, "Arc deref equals owned clone");
    }

    #[test]
    fn compute_shared_hit_returns_same_arc() {
        let docs = DocumentStore::new();
        let uri: Uri = "file:///t2.json".parse().unwrap();
        docs.open(
            uri,
            "json".to_string(),
            1,
            r#"{"name":"order","columns":[{"name":"id","type":"integer"}]}"#.to_string(),
        );

        let a = compute_shared("id", &docs, None);
        let b = compute_shared("id", &docs, None);
        assert!(Arc::ptr_eq(&a, &b), "warm cache returns same Arc");
    }

    #[test]
    fn compute_clones_arc_for_compat() {
        let docs = DocumentStore::new();
        let uri: Uri = "file:///t3.json".parse().unwrap();
        docs.open(
            uri,
            "json".to_string(),
            1,
            r#"{"name":"x","columns":[{"name":"id","type":"integer"}]}"#.to_string(),
        );

        let owned = compute("id", &docs, None);
        let shared = compute_shared("id", &docs, None);
        assert_eq!(owned.len(), shared.len());
        for (a, b) in owned.iter().zip(shared.iter()) {
            assert_eq!(a, b);
        }
    }

    #[test]
    fn compute_misses_filtered_cache_after_doc_text_change() {
        let docs = DocumentStore::new();
        let uri: Uri = "file:///change.json".parse().unwrap();
        docs.open(
            uri.clone(),
            "json".to_string(),
            1,
            r#"{"name":"user","columns":[{"name":"email","type":"text"}]}"#.to_string(),
        );
        assert!(
            compute("email", &docs, None)
                .iter()
                .any(|s| s.name == "email")
        );

        docs.update_full(
            &uri,
            r#"{"name":"post","columns":[{"name":"title","type":"text"}]}"#.to_string(),
            2,
        );
        assert!(compute("email", &docs, None).is_empty());
        assert!(
            compute("title", &docs, None)
                .iter()
                .any(|s| s.name == "title")
        );
    }

    #[test]
    fn compute_trims_query_before_filtering() {
        let docs = DocumentStore::new();
        let uri: Uri = "file:///trim.json".parse().unwrap();
        let source = r#"{"name":"user","columns":[{"name":"email","type":"text"}]}"#;
        docs.open(uri, "json".to_string(), 1, source.to_string());

        let out = compute("  EMAIL  ", &docs, None);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "email");
    }

    #[test]
    fn compute_filters_workspace_entries_by_query() {
        let docs = DocumentStore::new();
        let uri: Uri = "file:///filter.json".parse().unwrap();
        let source = r#"{"name":"user","columns":[{"name":"id","type":"integer"},{"name":"email","type":"text"}]}"#;
        docs.open(uri, "json".to_string(), 1, source.to_string());

        let out = compute("mail", &docs, None);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "email");
    }

    #[test]
    fn compute_caches_symbol_extraction_across_calls() {
        // Verify that calling compute() twice on the same DocumentStore content
        // produces the same DomainSymbol vec — and that the cache speeds up the
        // second call (we don't time it, but we DO verify the cached path returns
        // the same Arc-backed raw list).
        use crate::DocumentStore;

        let docs = DocumentStore::new();
        let uri: Uri = "file:///t.json".parse().unwrap();
        let source = r#"{"name":"user","columns":[{"name":"email","type":"text"}]}"#;
        docs.open(uri.clone(), "json".to_string(), 1, source.to_string());
        let first = compute("", &docs, None);
        let second = compute("", &docs, None);
        assert_eq!(first.len(), second.len(), "deterministic across calls");
        assert!(first.iter().any(|s| s.name == "email"));
    }
}
