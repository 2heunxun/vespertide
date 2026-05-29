//! Scan open documents (and on-disk models) for references to a symbol.

use crate::text_util::strip_quotes;
use std::path::PathBuf;

use tower_lsp_server::ls_types::Uri;

use crate::store::DocumentStore;
use crate::workspace_index::WorkspaceIndex;
use crate::workspace_tables::WorkspaceTables;

use super::{DomainReference, ReferenceSymbol};

#[expect(
    clippy::too_many_arguments,
    reason = "reference search needs target symbol, current document, open/disk workspace stores, and declaration policy; ReferenceSearchContext is deferred"
)]
pub(super) fn find_all(
    symbol: &ReferenceSymbol,
    current_uri: &Uri,
    current_source: &str,
    current_tree: Option<&tree_sitter::Tree>,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
    disk_tables: Option<&WorkspaceTables>,
    include_declaration: bool,
) -> Vec<DomainReference> {
    let mut out = Vec::new();

    // Always scan the document the cursor is in (it might contain self-refs).
    if let Some(tree) = current_tree {
        collect_in_document(
            symbol,
            current_uri,
            current_source,
            tree,
            include_declaration,
            &mut out,
        );
    }

    // Every OTHER open document.
    let other_uris: Vec<Uri> = docs
        .open_uris()
        .into_iter()
        .filter(|uri| uri != current_uri)
        .collect();
    for uri in other_uris {
        docs.with_doc(&uri, |text, tree| {
            if let Some(tree) = tree {
                collect_in_document(symbol, &uri, text, tree, include_declaration, &mut out);
            }
        });
    }

    // Disk-only models that the editor has not opened.
    if let Some(disk) = disk_tables {
        let open_paths: std::collections::BTreeSet<PathBuf> = docs
            .open_uris()
            .into_iter()
            .filter_map(|uri| crate::position::uri_to_path(&uri))
            .collect();
        for name in disk.names() {
            let Some(path) = disk.model_path(&name) else {
                continue;
            };
            if open_paths.contains(&path) {
                // Already scanned via the open document above.
                continue;
            }
            scan_disk_file(symbol, &path, include_declaration, &mut out);
        }
    }

    // Deterministic ordering — uri first, then byte range.
    out.sort_by(|a, b| {
        a.uri
            .as_str()
            .cmp(b.uri.as_str())
            .then(a.byte_range.start.cmp(&b.byte_range.start))
    });
    out.dedup();

    // Resolved declarations are valuable to surface even without
    // include_declaration via cross-file lookups (some clients ignore the
    // flag and rely on us to be authoritative). Keep callsite explicit.
    let _ = index;
    out
}

fn scan_disk_file(
    symbol: &ReferenceSymbol,
    path: &std::path::Path,
    include_declaration: bool,
    out: &mut Vec<DomainReference>,
) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let format = match path.extension().and_then(|ext| ext.to_str()) {
        Some("json") => crate::parser::DocumentFormat::Json,
        Some("yaml" | "yml") => crate::parser::DocumentFormat::Yaml,
        _ => return,
    };
    let pool = crate::parser::ParserPool::new();
    let Some(tree) = pool.parse(&text, format) else {
        return;
    };
    let uri = path_to_uri(path);
    collect_in_document(symbol, &uri, &text, &tree, include_declaration, out);
}

fn path_to_uri(path: &std::path::Path) -> Uri {
    let mut text = path.to_string_lossy().replace('\\', "/");
    if !text.starts_with('/') {
        text = format!("/{text}");
    }
    std::str::FromStr::from_str(&format!("file://{text}"))
        .unwrap_or_else(|_| std::str::FromStr::from_str("file:///").unwrap())
}

fn collect_in_document(
    symbol: &ReferenceSymbol,
    uri: &Uri,
    source: &str,
    tree: &tree_sitter::Tree,
    include_declaration: bool,
    out: &mut Vec<DomainReference>,
) {
    let source_bytes = source.as_bytes();
    let root = tree.root_node();
    walk_for_symbol(symbol, uri, source_bytes, root, include_declaration, out);
}

fn walk_for_symbol(
    symbol: &ReferenceSymbol,
    uri: &Uri,
    source: &[u8],
    node: tree_sitter::Node<'_>,
    include_declaration: bool,
    out: &mut Vec<DomainReference>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if matches!(child.kind(), "pair" | "block_mapping_pair") {
            inspect_pair(symbol, uri, source, child, include_declaration, out);
        }
        walk_for_symbol(symbol, uri, source, child, include_declaration, out);
    }
}

fn inspect_pair(
    symbol: &ReferenceSymbol,
    uri: &Uri,
    source: &[u8],
    pair: tree_sitter::Node<'_>,
    include_declaration: bool,
    out: &mut Vec<DomainReference>,
) {
    let Some(key) = pair.named_child(0) else {
        return;
    };
    let Some(key_text) = std::str::from_utf8(&source[key.byte_range()]).ok() else {
        return;
    };
    let key_text = strip_quotes(key_text);
    let Some(value) = pair.named_child(1) else {
        return;
    };

    match (symbol, key_text) {
        (ReferenceSymbol::Table { name }, "ref_table") if value_matches(source, value, name) => {
            out.push(DomainReference {
                uri: uri.clone(),
                byte_range: scalar_range(value),
            });
        }
        // Emit the top-level declaration only when explicitly asked.
        (ReferenceSymbol::Table { name }, "name")
            if include_declaration && value_matches(source, value, name) && is_top_level(pair) =>
        {
            out.push(DomainReference {
                uri: uri.clone(),
                byte_range: scalar_range(value),
            });
        }
        // ref_columns is an array — push every matching element, scoped to
        // the FK whose sibling `ref_table` equals `table`.
        (ReferenceSymbol::Column { table, column }, "ref_columns")
            if sibling_ref_table_matches(source, pair, table) =>
        {
            push_array_matches(value, source, column, uri, out);
        }
        // Column declaration inside its owning table.
        (ReferenceSymbol::Column { table, column }, "name")
            if include_declaration
                && value_matches(source, value, column)
                && is_column_pair(pair, source, table) =>
        {
            out.push(DomainReference {
                uri: uri.clone(),
                byte_range: scalar_range(value),
            });
        }
        // Column reference inside a table-level CHECK `expr` string. Each
        // bare identifier in the expression that names this column (scoped
        // to the CHECK's owning table) is a reference.
        (ReferenceSymbol::Column { table, column }, "expr")
            if is_check_constraint_pair(source, pair)
                && check_owning_table_matches(source, pair, table) =>
        {
            push_check_expr_matches(value, source, column, uri, out);
        }
        _ => {}
    }
}

/// True when this `expr` pair sits next to a sibling `type: "check"` pair
/// inside a constraint object.
fn is_check_constraint_pair(source: &[u8], expr_pair: tree_sitter::Node<'_>) -> bool {
    sibling_value(source, expr_pair, "type").is_some_and(|v| v == "check")
}

/// True when the CHECK constraint's owning table (the document's outermost
/// `name`) equals `expected_table`.
fn check_owning_table_matches(
    source: &[u8],
    expr_pair: tree_sitter::Node<'_>,
    expected_table: &str,
) -> bool {
    outer_table_name(source, expr_pair).is_some_and(|name| name == expected_table)
}

/// Look up a sibling pair's scalar value within the same constraint object.
fn sibling_value(source: &[u8], pair: tree_sitter::Node<'_>, target_key: &str) -> Option<String> {
    let object_raw = pair.parent()?;
    let object = match object_raw.kind() {
        "flow_node" | "block_node" => object_raw.named_child(0)?,
        _ => object_raw,
    };
    let mut cursor = object.walk();
    for child in object.children(&mut cursor) {
        if !matches!(child.kind(), "pair" | "block_mapping_pair") {
            continue;
        }
        let Some(key) = child.named_child(0) else {
            continue;
        };
        let Some(key_text) = std::str::from_utf8(&source[key.byte_range()]).ok() else {
            continue;
        };
        if strip_quotes(key_text) != target_key {
            continue;
        }
        let value = child.named_child(1)?;
        let actual = match value.kind() {
            "flow_node" | "block_node" => value.named_child(0).unwrap_or(value),
            _ => value,
        };
        let text = std::str::from_utf8(&source[actual.byte_range()]).ok()?;
        return Some(strip_quotes(text).to_string());
    }
    None
}

/// Walk up to the document's outermost mapping and return its `name` value.
fn outer_table_name(source: &[u8], node: tree_sitter::Node<'_>) -> Option<String> {
    let mut current = node.parent();
    let mut outer = None;
    while let Some(candidate) = current {
        if matches!(
            candidate.kind(),
            "object" | "block_mapping" | "flow_mapping"
        ) {
            outer = Some(candidate);
        }
        current = candidate.parent();
    }
    let outer = outer?;
    let mut cursor = outer.walk();
    for child in outer.children(&mut cursor) {
        if !matches!(child.kind(), "pair" | "block_mapping_pair") {
            continue;
        }
        let Some(key) = child.named_child(0) else {
            continue;
        };
        let Some(key_text) = std::str::from_utf8(&source[key.byte_range()]).ok() else {
            continue;
        };
        if strip_quotes(key_text) != "name" {
            continue;
        }
        let value = child.named_child(1)?;
        let actual = match value.kind() {
            "flow_node" | "block_node" => value.named_child(0).unwrap_or(value),
            _ => value,
        };
        let text = std::str::from_utf8(&source[actual.byte_range()]).ok()?;
        return Some(strip_quotes(text).to_string());
    }
    None
}

/// Lex the CHECK expression in `value` and push a reference for every bare
/// identifier matching `column`, with byte ranges absolute to the document.
fn push_check_expr_matches(
    value: tree_sitter::Node<'_>,
    source: &[u8],
    column: &str,
    uri: &Uri,
    out: &mut Vec<DomainReference>,
) {
    let Some(inner) = crate::check_expr_range::expr_inner_range(value) else {
        return;
    };
    let Some(expr_text) = std::str::from_utf8(&source[inner.clone()]).ok() else {
        return;
    };
    for token in vespertide_planner::lex_check_expr(expr_text) {
        if token.kind != vespertide_planner::CheckTokenKind::Column {
            continue;
        }
        let Some(ident) = expr_text.get(token.span.clone()) else {
            continue;
        };
        if ident != column {
            continue;
        }
        out.push(DomainReference {
            uri: uri.clone(),
            byte_range: (inner.start + token.span.start)..(inner.start + token.span.end),
        });
    }
}

fn sibling_ref_table_matches(
    source: &[u8],
    ref_columns_pair: tree_sitter::Node<'_>,
    table_name: &str,
) -> bool {
    let Some(fk_object_raw) = ref_columns_pair.parent() else {
        return false;
    };
    let fk_object = match fk_object_raw.kind() {
        "flow_node" | "block_node" => match fk_object_raw.named_child(0) {
            Some(inner) => inner,
            None => return false,
        },
        _ => fk_object_raw,
    };
    let mut cursor = fk_object.walk();
    for child in fk_object.children(&mut cursor) {
        if !matches!(child.kind(), "pair" | "block_mapping_pair") {
            continue;
        }
        let Some(key) = child.named_child(0) else {
            continue;
        };
        let Some(key_text) = std::str::from_utf8(&source[key.byte_range()]).ok() else {
            continue;
        };
        if strip_quotes(key_text) != "ref_table" {
            continue;
        }
        let Some(value) = child.named_child(1) else {
            return false;
        };
        return value_matches(source, value, table_name);
    }
    false
}

fn push_array_matches(
    array_node: tree_sitter::Node<'_>,
    source: &[u8],
    column: &str,
    uri: &Uri,
    out: &mut Vec<DomainReference>,
) {
    // For both `array` (JSON) and any YAML wrapper, walk descendants and
    // check every scalar.
    let mut cursor = array_node.walk();
    for child in array_node.children(&mut cursor) {
        // Skip punctuation, comments, etc. — only scalar values matter.
        if is_scalar_kind(child.kind()) && value_matches(source, child, column) {
            out.push(DomainReference {
                uri: uri.clone(),
                byte_range: scalar_range(child),
            });
            continue;
        }
        // YAML wraps each element in `flow_node`; recurse one level.
        push_array_matches(child, source, column, uri, out);
    }
}

fn is_scalar_kind(kind: &str) -> bool {
    matches!(
        kind,
        "string" | "double_quote_scalar" | "single_quote_scalar" | "string_scalar" | "plain_scalar"
    )
}

fn value_matches(source: &[u8], value: tree_sitter::Node<'_>, expected: &str) -> bool {
    // YAML wraps scalars in flow_node — peel.
    let actual = match value.kind() {
        "flow_node" | "block_node" => match value.named_child(0) {
            Some(inner) => inner,
            None => value,
        },
        _ => value,
    };
    let Some(text) = std::str::from_utf8(&source[actual.byte_range()]).ok() else {
        return false;
    };
    strip_quotes(text) == expected
}

fn scalar_range(node: tree_sitter::Node<'_>) -> std::ops::Range<usize> {
    let actual = match node.kind() {
        "flow_node" | "block_node" => node.named_child(0).unwrap_or(node),
        _ => node,
    };
    inner_content_range(actual)
}

/// Byte range of the scalar's TEXT CONTENT, with surrounding quotes
/// excluded when present. This is what we want for highlighting and for
/// rename — `"id"` → `a` should leave the quotes intact and replace only
/// the two-byte interior, not blow them away.
fn inner_content_range(node: tree_sitter::Node<'_>) -> std::ops::Range<usize> {
    match node.kind() {
        // tree-sitter-json: `string` is `"…"`. Its first named child is
        // `string_content` (absent when the literal is empty).
        "string" => node.named_child(0).map_or_else(
            || trim_one_byte_each_side(node.byte_range()),
            |inner| inner.byte_range(),
        ),
        // tree-sitter-yaml quoted scalars include their delimiters; trim
        // one byte on each side.
        "double_quote_scalar" | "single_quote_scalar" => trim_one_byte_each_side(node.byte_range()),
        // Unquoted scalars (YAML plain / string_scalar, or anything else)
        // have no delimiters — the full range is the identifier.
        _ => node.byte_range(),
    }
}

fn trim_one_byte_each_side(range: std::ops::Range<usize>) -> std::ops::Range<usize> {
    if range.end.saturating_sub(range.start) >= 2 {
        (range.start + 1)..(range.end - 1)
    } else {
        range
    }
}

fn is_top_level(pair: tree_sitter::Node<'_>) -> bool {
    let Some(parent) = pair.parent() else {
        return false;
    };
    if !matches!(parent.kind(), "object" | "block_mapping" | "flow_mapping") {
        return false;
    }
    let mut current = parent.parent();
    while let Some(candidate) = current {
        if matches!(
            candidate.kind(),
            "object" | "block_mapping" | "flow_mapping"
        ) {
            return false;
        }
        current = candidate.parent();
    }
    true
}

/// Check that this `name` pair lives directly inside a column object whose
/// owning table is `expected_table`.
fn is_column_pair(name_pair: tree_sitter::Node<'_>, source: &[u8], expected_table: &str) -> bool {
    // The pair's grandparent (mapping) is the column object; we walk above
    // the column object to the outer mapping and check its `name`.
    let Some(column_object) = name_pair.parent() else {
        return false;
    };
    if !matches!(
        column_object.kind(),
        "object" | "block_mapping" | "flow_mapping"
    ) {
        return false;
    }
    // The column object is not allowed to be the outermost mapping — that's
    // the table itself.
    let mut current = column_object.parent();
    let mut outer = None;
    while let Some(candidate) = current {
        if matches!(
            candidate.kind(),
            "object" | "block_mapping" | "flow_mapping"
        ) {
            outer = Some(candidate);
        }
        current = candidate.parent();
    }
    let Some(outer) = outer else {
        return false;
    };
    if outer.id() == column_object.id() {
        return false;
    }

    let mut cursor = outer.walk();
    for child in outer.children(&mut cursor) {
        if !matches!(child.kind(), "pair" | "block_mapping_pair") {
            continue;
        }
        let Some(key) = child.named_child(0) else {
            continue;
        };
        let Some(key_text) = std::str::from_utf8(&source[key.byte_range()]).ok() else {
            continue;
        };
        if strip_quotes(key_text) != "name" {
            continue;
        }
        let Some(value) = child.named_child(1) else {
            return false;
        };
        return value_matches(source, value, expected_table);
    }
    false
}
