//! Completion context detection via tree-sitter node ancestry.

use std::ops::Range;

use vespertide_planner::{CheckToken, CheckTokenKind, lex_check_expr};

use crate::check_expr_range::expr_inner_range;
use crate::text_util::strip_quotes;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Context {
    // ------------------- VALUE positions -------------------
    /// Cursor is inside the string literal of a `type` value. Simple types
    /// insert as-is; complex types overwrite the entire `string_byte_range`
    /// (quotes included) with an object literal.
    ColumnTypeInString {
        string_byte_range: std::ops::Range<usize>,
    },
    /// Cursor is at the bare value slot of `type`: both simple strings and
    /// complex object snippets are valid.
    ColumnTypeValue,
    Nullable,
    PrimaryKey,
    Unique,
    OnDeleteAction,
    OnUpdateAction,
    RefTable,
    RefColumns {
        ref_table: String,
    },
    /// Cursor is on the value of `kind` inside a complex `type` object
    /// (`varchar` / `char` / `numeric` / `enum` / `custom`). When the
    /// cursor sits inside a `"..."` literal, the suggested label replaces
    /// the whole literal so partial typing is cleaned up.
    TypeKind {
        string_byte_range: Option<std::ops::Range<usize>>,
    },
    /// Cursor is at a column's `default` value. The candidate set depends on
    /// the sibling `type`: enum gets its `values` quoted, timestamp gets
    /// `now()`/`CURRENT_TIMESTAMP`, uuid gets `gen_random_uuid()`, etc.
    DefaultValue {
        /// Either a simple type name (`"integer"`, `"timestamp"`, ...) or the
        /// `kind` of a complex `type` object (`"varchar"`, `"enum"`, ...).
        type_kind: Option<String>,
        /// String enum members or stringified integer enum names. Empty
        /// unless the sibling `type.kind == "enum"`.
        enum_values: Vec<String>,
        /// When the cursor sits inside a `"..."` literal, this is the byte
        /// range of that string (quotes included). Completions use it as
        /// the `TextEdit` range so accepting a suggestion wipes the
        /// existing literal instead of appending to it.
        string_byte_range: Option<Range<usize>>,
    },
    /// Cursor is inside a table-level CHECK expression string
    /// (`constraints[*].expr`). The position decides whether we suggest
    /// operands (columns), operators/SQL keywords, or a partial-column edit.
    CheckExpr {
        table_columns: Vec<String>,
        position: CheckExprPos,
        replace_range_bytes: Option<Range<usize>>,
    },

    // ------------------- KEY positions ---------------------
    /// New key inside the top-level table object.
    TableTopLevelKey,
    /// New key inside a column object (`columns[N]`).
    ColumnObjectKey,
    /// New key inside a `foreign_key` object.
    ForeignKeyObjectKey,
    /// New key inside a complex `type` object (varchar/numeric/enum/...).
    TypeObjectKey,

    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CheckExprPos {
    Operand,
    Operator,
    PartialColumn { prefix: String },
}

pub(super) fn detect(tree: &tree_sitter::Tree, source: &str, byte_offset: usize) -> Context {
    let Some(node) = node_at_byte(tree, byte_offset) else {
        return Context::None;
    };

    // KEY position completions take priority over VALUE position logic so
    // that typing `"` at an object boundary offers the right key set.
    if let Some(ctx) = classify_key_context(node, source) {
        return ctx;
    }

    let path = collect_key_path(node, source);
    classify_path(&path, node, source, byte_offset)
}

fn classify_path(
    path: &[String],
    cursor_node: tree_sitter::Node<'_>,
    source: &str,
    byte_offset: usize,
) -> Context {
    let last = path.last().map(String::as_str);
    let has = |key: &str| path.iter().any(|part| part == key);

    match last {
        Some("type") if has("columns") => {
            if let Some(range) = enclosing_string_range(cursor_node) {
                Context::ColumnTypeInString {
                    string_byte_range: range,
                }
            } else {
                Context::ColumnTypeValue
            }
        }
        Some("nullable") if has("columns") => Context::Nullable,
        Some("primary_key") if has("columns") => Context::PrimaryKey,
        Some("unique") if has("columns") => Context::Unique,
        // `kind` is only meaningful inside `columns[*].type` — guard on
        // both keys so an arbitrary nested `kind` (e.g. inside someone's
        // custom JSON) does not accidentally match.
        Some("kind") if has("columns") && has("type") => Context::TypeKind {
            string_byte_range: enclosing_string_range(cursor_node),
        },
        Some("on_delete") => Context::OnDeleteAction,
        Some("on_update") => Context::OnUpdateAction,
        Some("ref_table") => Context::RefTable,
        Some("ref_columns") => Context::RefColumns {
            ref_table: sibling_ref_table(cursor_node, source).unwrap_or_default(),
        },
        Some("default") if has("columns") => {
            let (type_kind, enum_values) = analyze_sibling_type(cursor_node, source);
            Context::DefaultValue {
                type_kind,
                enum_values,
                string_byte_range: enclosing_string_range(cursor_node),
            }
        }
        Some("expr") if has("constraints") => {
            check_expr_context(cursor_node, source, byte_offset).unwrap_or(Context::None)
        }
        _ => Context::None,
    }
}

fn check_expr_context(
    cursor: tree_sitter::Node<'_>,
    source: &str,
    byte_offset: usize,
) -> Option<Context> {
    let expr_pair = enclosing_pair_with_key(cursor, source, "expr")?;
    if !is_inside_constraints(expr_pair, source) {
        return None;
    }

    let expr_value = expr_pair.named_child(1).map(unwrap_flow_node)?;
    let inner = expr_inner_range(expr_value)?;
    if byte_offset < inner.start || byte_offset > inner.end {
        return None;
    }

    let expr_text = source.get(inner.clone())?;
    let cursor_rel = clamp_to_char_boundary(
        expr_text,
        byte_offset.saturating_sub(inner.start).min(expr_text.len()),
    );
    let table_columns = current_table_columns(cursor, source);
    let (position, replace_range_bytes) =
        classify_check_expr_position(expr_text, inner.start, cursor_rel, &table_columns);

    Some(Context::CheckExpr {
        table_columns,
        position,
        replace_range_bytes,
    })
}

fn classify_check_expr_position(
    expr_text: &str,
    inner_start: usize,
    cursor_rel: usize,
    table_columns: &[String],
) -> (CheckExprPos, Option<Range<usize>>) {
    let prefix = &expr_text[..cursor_rel];
    if prefix.trim().is_empty() {
        return (CheckExprPos::Operand, None);
    }

    let tokens = lex_check_expr(prefix);
    let Some(last) = tokens.last() else {
        return (CheckExprPos::Operand, None);
    };

    if let Some((typed_prefix, replace_range)) =
        partial_column_at_cursor(prefix, last, cursor_rel, inner_start, table_columns)
    {
        return (
            CheckExprPos::PartialColumn {
                prefix: typed_prefix,
            },
            Some(replace_range),
        );
    }

    if token_expects_operand(last, prefix) {
        (CheckExprPos::Operand, None)
    } else {
        (CheckExprPos::Operator, None)
    }
}

fn partial_column_at_cursor(
    prefix: &str,
    token: &CheckToken,
    cursor_rel: usize,
    inner_start: usize,
    table_columns: &[String],
) -> Option<(String, Range<usize>)> {
    if token.kind != CheckTokenKind::Column || token.span.end != cursor_rel {
        return None;
    }

    let typed_prefix = prefix.get(token.span.clone())?;
    if typed_prefix.is_empty() || table_columns.iter().any(|column| column == typed_prefix) {
        return None;
    }

    let replace_range = (inner_start + token.span.start)..(inner_start + token.span.end);
    Some((typed_prefix.to_string(), replace_range))
}

fn token_expects_operand(token: &CheckToken, expr_prefix: &str) -> bool {
    let text = expr_prefix.get(token.span.clone()).unwrap_or_default();
    match token.kind {
        CheckTokenKind::Operator => true,
        CheckTokenKind::Punctuation => matches!(text, "(" | ","),
        CheckTokenKind::Keyword => keyword_expects_operand(text),
        CheckTokenKind::Column | CheckTokenKind::Number | CheckTokenKind::String => false,
    }
}

fn keyword_expects_operand(keyword: &str) -> bool {
    ["AND", "OR", "NOT", "IN", "BETWEEN"]
        .iter()
        .any(|expected| keyword.eq_ignore_ascii_case(expected))
}

fn is_inside_constraints(node: tree_sitter::Node<'_>, source: &str) -> bool {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if is_pair(candidate) && key_text(candidate, source) == Some("constraints") {
            return true;
        }
        current = candidate.parent();
    }
    false
}

fn current_table_columns(node: tree_sitter::Node<'_>, source: &str) -> Vec<String> {
    let root = document_value(node);
    let Some(columns_pair) = find_pair_with_key(root, source, "columns") else {
        return Vec::new();
    };
    let Some(columns_value_raw) = columns_pair.named_child(1) else {
        return Vec::new();
    };

    collect_column_names(unwrap_flow_node(columns_value_raw), source)
}

fn document_value(mut node: tree_sitter::Node<'_>) -> tree_sitter::Node<'_> {
    while let Some(parent) = node.parent() {
        node = parent;
    }

    node.named_child(0).map_or(node, unwrap_flow_node)
}

fn collect_column_names(columns_value: tree_sitter::Node<'_>, source: &str) -> Vec<String> {
    if !matches!(
        columns_value.kind(),
        "array" | "block_sequence" | "flow_sequence"
    ) {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut cursor = columns_value.walk();
    for raw_child in columns_value.children(&mut cursor) {
        let child = unwrap_flow_node(raw_child);
        let Some(column_object) = column_object_from_sequence_child(child) else {
            continue;
        };
        if let Some(name) = string_value_for_key(column_object, source, "name")
            && !name.is_empty()
        {
            out.push(name.to_string());
        }
    }
    out
}

fn column_object_from_sequence_child(
    child: tree_sitter::Node<'_>,
) -> Option<tree_sitter::Node<'_>> {
    match child.kind() {
        "object" | "block_mapping" | "flow_mapping" => Some(child),
        "block_sequence_item" => {
            let mut cursor = child.walk();
            child.children(&mut cursor).find_map(|raw_inner| {
                let inner = unwrap_flow_node(raw_inner);
                matches!(inner.kind(), "object" | "block_mapping" | "flow_mapping").then_some(inner)
            })
        }
        _ => None,
    }
}

fn string_value_for_key<'source>(
    object: tree_sitter::Node<'_>,
    source: &'source str,
    key: &str,
) -> Option<&'source str> {
    let pair = find_pair_with_key(object, source, key)?;
    let value = pair.named_child(1).map(unwrap_flow_node)?;
    source.get(value.byte_range()).map(strip_quotes)
}

fn clamp_to_char_boundary(text: &str, mut byte_offset: usize) -> usize {
    while byte_offset > 0 && !text.is_char_boundary(byte_offset) {
        byte_offset -= 1;
    }
    byte_offset
}

/// Walk to the enclosing column object and inspect its `type` sibling.
/// Returns `(type_kind, enum_values)` where `type_kind` is either the simple
/// type name (`"integer"`) or the complex object's `kind` (`"varchar"` /
/// `"enum"`), and `enum_values` is the value list when `kind == "enum"`.
fn analyze_sibling_type(
    cursor: tree_sitter::Node<'_>,
    source: &str,
) -> (Option<String>, Vec<String>) {
    let Some(column_object) = enclosing_column_object(cursor) else {
        return (None, Vec::new());
    };
    let Some(type_pair) = find_pair_with_key(column_object, source, "type") else {
        return (None, Vec::new());
    };
    let Some(type_value) = type_pair.named_child(1) else {
        return (None, Vec::new());
    };
    let effective = unwrap_flow_node(type_value);

    match effective.kind() {
        "string"
        | "double_quote_scalar"
        | "single_quote_scalar"
        | "string_scalar"
        | "plain_scalar" => {
            let raw = source.get(effective.byte_range()).unwrap_or("");
            (Some(strip_quotes(raw).to_string()), Vec::new())
        }
        "object" | "block_mapping" | "flow_mapping" => {
            let kind = find_pair_with_key(effective, source, "kind")
                .and_then(|pair| pair.named_child(1))
                .map(unwrap_flow_node)
                .and_then(|node| source.get(node.byte_range()))
                .map(|raw| strip_quotes(raw).to_string());

            let enum_values = if kind.as_deref() == Some("enum") {
                collect_enum_values(effective, source)
            } else {
                Vec::new()
            };
            (kind, enum_values)
        }
        _ => (None, Vec::new()),
    }
}

/// tree-sitter-yaml wraps scalars in `flow_node` (inline values) and
/// multi-line mappings/sequences in `block_node`. Both are pure wrappers
/// over their first named child — peel them so downstream `match`es see
/// the real kind. We loop to handle the (rare) double-wrapping case.
/// JSON's grammar has no such wrapper, so this is a no-op there.
fn unwrap_flow_node(node: tree_sitter::Node<'_>) -> tree_sitter::Node<'_> {
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

/// Walk up to the smallest enclosing object that lives inside a `columns`
/// array — i.e. the column object the cursor belongs to.
fn enclosing_column_object(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    let mut current = Some(node);
    while let Some(candidate) = current {
        if matches!(
            candidate.kind(),
            "object" | "block_mapping" | "flow_mapping"
        ) {
            return Some(candidate);
        }
        current = candidate.parent();
    }
    None
}

fn find_pair_with_key<'tree>(
    object: tree_sitter::Node<'tree>,
    source: &str,
    target_key: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut cursor = object.walk();
    object
        .children(&mut cursor)
        .find(|&child| is_pair(child) && key_text(child, source) == Some(target_key))
}

fn collect_enum_values(type_object: tree_sitter::Node<'_>, source: &str) -> Vec<String> {
    let Some(values_pair) = find_pair_with_key(type_object, source, "values") else {
        return Vec::new();
    };
    let Some(values_array_raw) = values_pair.named_child(1) else {
        return Vec::new();
    };
    let values_array = unwrap_flow_node(values_array_raw);
    if !matches!(
        values_array.kind(),
        "array" | "block_sequence" | "flow_sequence"
    ) {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut cursor = values_array.walk();
    for raw_child in values_array.children(&mut cursor) {
        let child = unwrap_flow_node(raw_child);
        match child.kind() {
            "string"
            | "double_quote_scalar"
            | "single_quote_scalar"
            | "string_scalar"
            | "plain_scalar" => {
                if let Some(raw) = source.get(child.byte_range()) {
                    out.push(strip_quotes(raw).to_string());
                }
            }
            // Integer-enum members are objects of the form `{name: "...", value: N}`.
            "object" | "block_mapping" | "flow_mapping" => {
                if let Some(name_pair) = find_pair_with_key(child, source, "name")
                    && let Some(name_value_raw) = name_pair.named_child(1)
                {
                    let name_value = unwrap_flow_node(name_value_raw);
                    if let Some(raw) = source.get(name_value.byte_range()) {
                        out.push(strip_quotes(raw).to_string());
                    }
                }
            }
            // YAML block sequence items show up as `block_sequence_item` →
            // `flow_node` or `block_mapping`; recurse one level so they are
            // not silently skipped.
            "block_sequence_item" => {
                let mut inner_cursor = child.walk();
                for inner in child.children(&mut inner_cursor) {
                    let inner = unwrap_flow_node(inner);
                    if let Some(raw) = source.get(inner.byte_range())
                        && matches!(
                            inner.kind(),
                            "string"
                                | "double_quote_scalar"
                                | "single_quote_scalar"
                                | "string_scalar"
                                | "plain_scalar"
                        )
                    {
                        out.push(strip_quotes(raw).to_string());
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Decide whether the cursor sits at a place where a new object key would
/// be typed, and if so which set of keys is appropriate.
fn classify_key_context(cursor: tree_sitter::Node<'_>, source: &str) -> Option<Context> {
    if !is_at_pair_key_position(cursor) {
        return None;
    }

    let path = enclosing_object_parent_path(cursor, source);
    Some(match path.last().map(String::as_str) {
        Some("foreign_key") => Context::ForeignKeyObjectKey,
        Some("type") => Context::TypeObjectKey,
        Some("columns") => Context::ColumnObjectKey,
        None => Context::TableTopLevelKey,
        // Unknown nested object (e.g. inside enum values, table-level
        // constraints) — fall through to value-based classification.
        _ => return None,
    })
}

/// True when the cursor sits inside a pair's KEY string, or directly inside
/// an object body between pairs (where a new key would be typed).
fn is_at_pair_key_position(cursor: tree_sitter::Node<'_>) -> bool {
    let cursor_start = cursor.start_byte();
    let cursor_end = cursor.end_byte();

    let mut current = Some(cursor);
    while let Some(candidate) = current {
        if is_pair(candidate) {
            if let Some(key) = candidate.named_child(0) {
                let range = key.byte_range();
                return cursor_start >= range.start && cursor_end <= range.end;
            }
            return false;
        }
        if matches!(
            candidate.kind(),
            "object" | "block_mapping" | "flow_mapping"
        ) {
            return true;
        }
        current = candidate.parent();
    }
    false
}

/// Walk upward from the cursor, find the smallest enclosing object, and
/// return the ancestor pair-key path ABOVE that object (excluding any pair
/// that the cursor itself is the key of).
fn enclosing_object_parent_path(cursor: tree_sitter::Node<'_>, source: &str) -> Vec<String> {
    let object = {
        let mut current = Some(cursor);
        loop {
            match current {
                Some(node)
                    if matches!(node.kind(), "object" | "block_mapping" | "flow_mapping") =>
                {
                    break Some(node);
                }
                Some(node) => current = node.parent(),
                None => break None,
            }
        }
    };
    let Some(object) = object else {
        return Vec::new();
    };

    let mut path = Vec::new();
    let mut current = object.parent();
    while let Some(candidate) = current {
        if is_pair(candidate)
            && let Some(key) = key_text(candidate, source)
        {
            path.push(key.to_string());
        }
        current = candidate.parent();
    }
    path.reverse();
    path
}

/// Walk upward from the cursor and return the byte range of the enclosing
/// JSON/YAML string literal (quotes included for quoted variants), or `None`
/// if the cursor is not inside a string. Stops at the first container
/// boundary so nested object values never count as "in string".
fn enclosing_string_range(node: tree_sitter::Node<'_>) -> Option<std::ops::Range<usize>> {
    let mut current = Some(node);
    while let Some(candidate) = current {
        match candidate.kind() {
            // `string_content` is the inner span without quotes — climb one
            // more level so we capture the surrounding quotes too.
            "string_content" => {
                if let Some(parent) = candidate.parent()
                    && parent.kind() == "string"
                {
                    return Some(parent.byte_range());
                }
                return Some(candidate.byte_range());
            }
            // All other scalar variants are returned as-is. Quoted JSON/YAML
            // scalars (`string`, `double_quote_scalar`, `single_quote_scalar`)
            // already include their delimiters; unquoted YAML scalars
            // (`string_scalar`, `plain_scalar`) have no quotes to begin with
            // but should still be replaced wholesale when expanding into an
            // object literal.
            "string"
            | "double_quote_scalar"
            | "single_quote_scalar"
            | "string_scalar"
            | "plain_scalar" => {
                return Some(candidate.byte_range());
            }
            "pair" | "block_mapping_pair" | "object" | "array" | "block_mapping"
            | "block_sequence" | "flow_mapping" | "flow_sequence" => {
                return None;
            }
            _ => {}
        }
        current = candidate.parent();
    }
    None
}

fn collect_key_path(node: tree_sitter::Node<'_>, source: &str) -> Vec<String> {
    let mut path = Vec::new();
    let mut current = Some(node);

    while let Some(candidate) = current {
        if is_pair(candidate)
            && let Some(key) = key_text(candidate, source)
        {
            path.push(key.to_string());
        }
        current = candidate.parent();
    }

    path.reverse();
    path
}

fn sibling_ref_table(node: tree_sitter::Node<'_>, source: &str) -> Option<String> {
    let ref_columns_pair = enclosing_pair_with_key(node, source, "ref_columns")?;
    let parent = ref_columns_pair.parent()?;
    direct_child_value(parent, source, "ref_table").map(ToString::to_string)
}

fn enclosing_pair_with_key<'tree>(
    node: tree_sitter::Node<'tree>,
    source: &str,
    expected: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut current = Some(node);

    while let Some(candidate) = current {
        if is_pair(candidate) && key_text(candidate, source) == Some(expected) {
            return Some(candidate);
        }
        current = candidate.parent();
    }

    None
}

fn direct_child_value<'source>(
    node: tree_sitter::Node<'_>,
    source: &'source str,
    expected_key: &str,
) -> Option<&'source str> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_pair(child) && key_text(child, source) == Some(expected_key) {
            return value_text(child, source);
        }
    }

    None
}

fn key_text<'source>(
    pair_node: tree_sitter::Node<'_>,
    source: &'source str,
) -> Option<&'source str> {
    let key = pair_node.named_child(0)?;
    source.get(key.byte_range()).map(strip_quotes)
}

fn value_text<'source>(
    pair_node: tree_sitter::Node<'_>,
    source: &'source str,
) -> Option<&'source str> {
    let value = pair_node.named_child(1)?;
    source.get(value.byte_range()).map(strip_quotes)
}

fn node_at_byte(tree: &tree_sitter::Tree, byte_offset: usize) -> Option<tree_sitter::Node<'_>> {
    let root = tree.root_node();
    if root.end_byte() == 0 {
        return Some(root);
    }

    let start = byte_offset
        .saturating_sub(1)
        .min(root.end_byte().saturating_sub(1));
    let end = byte_offset.min(root.end_byte());
    root.descendant_for_byte_range(start, end).or(Some(root))
}

fn is_pair(node: tree_sitter::Node<'_>) -> bool {
    matches!(node.kind(), "pair" | "block_mapping_pair")
}
