//! Validation routines: syntax → parse → planner.

use tower_lsp_server::ls_types::Uri;
use vespertide_core::TableDef;

use super::{DomainDiagnostic, Severity};

/// Simple column type names recognized as string literals. Mirrors
/// `vespertide_core::SimpleColumnType`. Kept here so we can flag unknown
/// strings BEFORE serde fails — serde's error position is unreliable inside
/// untagged enums and tends to point at the wrong byte (often the column's
/// closing brace).
const KNOWN_SIMPLE_TYPES: &[&str] = &[
    "small_int",
    "integer",
    "big_int",
    "real",
    "double_precision",
    "text",
    "boolean",
    "date",
    "time",
    "timestamp",
    "timestamptz",
    "interval",
    "bytea",
    "uuid",
    "json",
    "jsonb",
    "inet",
    "cidr",
    "macaddr",
    "xml",
];

/// Parsed table plus source context for workspace-wide validation.
pub struct WorkspaceTable {
    /// URI that owns this table definition.
    pub uri: Uri,
    /// Normalized table definition used by planner validation.
    pub table: TableDef,
    /// Raw document text used for byte-range location.
    pub source: String,
    /// Parsed tree-sitter tree for source range lookup.
    pub tree: Option<tree_sitter::Tree>,
}

pub(super) fn collect_syntax_errors(tree: &tree_sitter::Tree, out: &mut Vec<DomainDiagnostic>) {
    let root = tree.root_node();
    if root.has_error() {
        walk_for_errors(root, out);
    }
}

/// Tree-sitter-based pre-pass that flags unknown column types with a
/// precise byte range pointing at the offending `type` value. Runs before
/// serde so users see the squiggle on the right line even when serde's
/// untagged-enum error reports a misleading position.
pub(super) fn collect_unknown_column_types(
    tree: &tree_sitter::Tree,
    source: &str,
    out: &mut Vec<DomainDiagnostic>,
) {
    let source_bytes = source.as_bytes();
    let Some(columns) = find_value_for_key(tree.root_node(), source_bytes, "columns") else {
        return;
    };

    walk_column_objects(columns, source_bytes, out);
}

fn walk_column_objects(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    out: &mut Vec<DomainDiagnostic>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if matches!(child.kind(), "object" | "block_mapping") {
            inspect_column_type(child, source, out);
        }
        walk_column_objects(child, source, out);
    }
}

fn inspect_column_type(
    column: tree_sitter::Node<'_>,
    source: &[u8],
    out: &mut Vec<DomainDiagnostic>,
) {
    let Some(type_pair) = find_pair_with_key(column, source, "type") else {
        return;
    };
    let Some(type_value_raw) = type_pair.named_child(1) else {
        return;
    };
    // tree-sitter-yaml wraps every value in a `flow_node` / `block_node`.
    // Peel those wrappers so we see the real scalar or mapping underneath.
    let type_value = unwrap_yaml_node(type_value_raw);

    // Object form (`{kind: ...}`) is checked by serde + planner — skip here.
    if matches!(
        type_value.kind(),
        "object" | "block_mapping" | "flow_mapping"
    ) {
        return;
    }

    let Some(text) = source.get(type_value.byte_range()) else {
        return;
    };
    let Ok(text) = std::str::from_utf8(text) else {
        return;
    };
    let stripped = strip_quotes_str(text);

    // Skip empty placeholder while the user is typing.
    if stripped.is_empty() {
        return;
    }

    if KNOWN_SIMPLE_TYPES.contains(&stripped) {
        return;
    }

    out.push(DomainDiagnostic {
        byte_range: type_pair.byte_range(),
        severity: Severity::Error,
        message: format!(
            "Unknown column type `{stripped}`. Expected one of: {} \
             — or a complex type object such as {{\"kind\":\"varchar\",\"length\":255}}",
            KNOWN_SIMPLE_TYPES.join(", ")
        ),
        code: "unknown-type".to_string(),
    });
}

/// Peel YAML's `flow_node` / `block_node` wrappers (no-op on JSON).
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

/// Tree-sitter-based pre-pass that flags two columns sharing a `name`.
/// Pinpoints the SECOND (and later) occurrence so the user sees the
/// squiggle on the offending column, not on the table.
///
/// Critically, we visit ONLY the direct elements of the `columns` array.
/// A naive recursive walk would dive into nested objects (e.g. integer
/// enum members like `{"name":"low","value":0}` inside `type.values`) and
/// mistakenly compare their `name` against the column names.
pub(super) fn collect_duplicate_column_names(
    tree: &tree_sitter::Tree,
    source: &str,
    out: &mut Vec<DomainDiagnostic>,
) {
    let source_bytes = source.as_bytes();
    let Some(columns_raw) = find_value_for_key(tree.root_node(), source_bytes, "columns") else {
        return;
    };

    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for column in direct_column_objects(columns_raw) {
        inspect_column_name(column, source_bytes, &mut seen, out);
    }
}

/// Resolve `columns: [...]` value to the direct list of column mapping
/// nodes — peeling through tree-sitter-yaml's wrappers and skipping
/// punctuation / comments.
fn direct_column_objects(columns_value: tree_sitter::Node<'_>) -> Vec<tree_sitter::Node<'_>> {
    let array = unwrap_yaml_node(columns_value);
    if !matches!(array.kind(), "array" | "block_sequence" | "flow_sequence") {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut cursor = array.walk();
    for raw_child in array.children(&mut cursor) {
        let child = unwrap_yaml_node(raw_child);
        match child.kind() {
            "object" | "block_mapping" | "flow_mapping" => out.push(child),
            // YAML block sequence items wrap each element in
            // `block_sequence_item` → mapping. Recurse exactly one level.
            "block_sequence_item" => {
                let mut inner_cursor = child.walk();
                for inner in child.children(&mut inner_cursor) {
                    let inner = unwrap_yaml_node(inner);
                    if matches!(inner.kind(), "object" | "block_mapping" | "flow_mapping") {
                        out.push(inner);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn inspect_column_name(
    column: tree_sitter::Node<'_>,
    source: &[u8],
    seen: &mut std::collections::BTreeSet<String>,
    out: &mut Vec<DomainDiagnostic>,
) {
    let Some(name_pair) = find_pair_with_key(column, source, "name") else {
        return;
    };
    let Some(name_value_raw) = name_pair.named_child(1) else {
        return;
    };
    let name_value = unwrap_yaml_node(name_value_raw);
    let Some(text) = source.get(name_value.byte_range()) else {
        return;
    };
    let Ok(text) = std::str::from_utf8(text) else {
        return;
    };
    let name = strip_quotes_str(text).to_string();
    if name.is_empty() {
        return;
    }
    if !seen.insert(name.clone()) {
        out.push(DomainDiagnostic {
            byte_range: name_value.byte_range(),
            severity: Severity::Error,
            message: format!("Duplicate column name `{name}` in this table"),
            code: "duplicate-column".to_string(),
        });
    }
}

/// Tree-sitter-based pre-pass for COMPLEX (object-form) column types.
///
/// Catches things serde either silently allows or reports at a misleading
/// byte position:
///   * `kind` is missing / empty / unknown.
///   * `varchar` / `char` without `length`.
///   * `numeric` without `precision` or `scale`.
///   * `enum` without `name`, without `values`, with an empty `values`, or
///     with duplicate string variants / duplicate integer variant names.
///   * `custom` without `custom_type`.
///
/// Each diagnostic gets a precise byte range covering the offending pair so
/// the squiggle lands on the right line.
pub(super) fn collect_complex_type_violations(
    tree: &tree_sitter::Tree,
    source: &str,
    out: &mut Vec<DomainDiagnostic>,
) {
    let source_bytes = source.as_bytes();
    let Some(columns) = find_value_for_key(tree.root_node(), source_bytes, "columns") else {
        return;
    };
    walk_columns_for_complex_type(columns, source_bytes, out);
}

fn walk_columns_for_complex_type(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    out: &mut Vec<DomainDiagnostic>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if matches!(child.kind(), "object" | "block_mapping") {
            inspect_complex_type(child, source, out);
        }
        walk_columns_for_complex_type(child, source, out);
    }
}

fn inspect_complex_type(
    column: tree_sitter::Node<'_>,
    source: &[u8],
    out: &mut Vec<DomainDiagnostic>,
) {
    let Some(type_pair) = find_pair_with_key(column, source, "type") else {
        return;
    };
    let Some(type_value_raw) = type_pair.named_child(1) else {
        return;
    };
    let type_value = unwrap_yaml_node(type_value_raw);
    if !matches!(
        type_value.kind(),
        "object" | "block_mapping" | "flow_mapping"
    ) {
        return;
    }

    // `kind` is mandatory.
    let Some(kind_pair) = find_pair_with_key(type_value, source, "kind") else {
        push_complex(
            out,
            type_pair.byte_range(),
            "Type object requires a `kind` field (varchar, char, numeric, enum, custom)",
        );
        return;
    };
    let kind = match scalar_text(kind_pair, source) {
        Some(text) if !text.is_empty() => text.to_string(),
        _ => {
            push_complex(
                out,
                kind_pair.byte_range(),
                "`kind` must be a non-empty string",
            );
            return;
        }
    };

    match kind.as_str() {
        "varchar" | "char" => check_length_required(type_value, type_pair, &kind, source, out),
        "numeric" => check_numeric_precision_scale(type_value, type_pair, source, out),
        "enum" => check_enum_shape(type_value, type_pair, source, out),
        "custom" => check_custom_type(type_value, type_pair, source, out),
        other => {
            push_complex(
                out,
                kind_pair.byte_range(),
                &format!(
                    "Unknown type kind `{other}`. Expected: varchar, char, numeric, enum, custom"
                ),
            );
        }
    }
}

fn check_length_required(
    type_value: tree_sitter::Node<'_>,
    type_pair: tree_sitter::Node<'_>,
    kind: &str,
    source: &[u8],
    out: &mut Vec<DomainDiagnostic>,
) {
    if find_pair_with_key(type_value, source, "length").is_none() {
        push_complex(
            out,
            type_pair.byte_range(),
            &format!("`{kind}` type requires a `length` field"),
        );
    }
}

fn check_numeric_precision_scale(
    type_value: tree_sitter::Node<'_>,
    type_pair: tree_sitter::Node<'_>,
    source: &[u8],
    out: &mut Vec<DomainDiagnostic>,
) {
    let mut missing = Vec::new();
    if find_pair_with_key(type_value, source, "precision").is_none() {
        missing.push("precision");
    }
    if find_pair_with_key(type_value, source, "scale").is_none() {
        missing.push("scale");
    }
    if !missing.is_empty() {
        push_complex(
            out,
            type_pair.byte_range(),
            &format!(
                "`numeric` type requires {} field{}",
                missing.join(" and "),
                if missing.len() > 1 { "s" } else { "" }
            ),
        );
    }
}

fn check_enum_shape(
    type_value: tree_sitter::Node<'_>,
    type_pair: tree_sitter::Node<'_>,
    source: &[u8],
    out: &mut Vec<DomainDiagnostic>,
) {
    let name_pair = find_pair_with_key(type_value, source, "name");
    let values_pair = find_pair_with_key(type_value, source, "values");

    let mut missing = Vec::new();
    if name_pair.is_none() {
        missing.push("name");
    }
    if values_pair.is_none() {
        missing.push("values");
    }
    if !missing.is_empty() {
        push_complex(
            out,
            type_pair.byte_range(),
            &format!(
                "`enum` type requires field{}: {}",
                if missing.len() > 1 { "s" } else { "" },
                missing.join(", ")
            ),
        );
        return;
    }

    let values_pair = values_pair.unwrap();
    let Some(values_value_raw) = values_pair.named_child(1) else {
        return;
    };
    let values_value = unwrap_yaml_node(values_value_raw);
    if !matches!(
        values_value.kind(),
        "array" | "block_sequence" | "flow_sequence"
    ) {
        push_complex(
            out,
            values_pair.byte_range(),
            "`values` must be a non-empty array",
        );
        return;
    }

    let elements = collect_enum_value_descriptors(values_value, source);
    if elements.is_empty() {
        push_complex(
            out,
            values_pair.byte_range(),
            "`enum` requires a non-empty `values` array",
        );
        return;
    }

    check_duplicate_enum_values(&elements, out);
}

fn check_custom_type(
    type_value: tree_sitter::Node<'_>,
    type_pair: tree_sitter::Node<'_>,
    source: &[u8],
    out: &mut Vec<DomainDiagnostic>,
) {
    if find_pair_with_key(type_value, source, "custom_type").is_none() {
        push_complex(
            out,
            type_pair.byte_range(),
            "`custom` type requires a `custom_type` SQL string",
        );
    }
}

struct EnumValueDescriptor {
    name: String,
    byte_range: std::ops::Range<usize>,
    /// Optional explicit integer value (for integer enums).
    integer_value: Option<String>,
    integer_value_range: std::ops::Range<usize>,
}

fn collect_enum_value_descriptors(
    array: tree_sitter::Node<'_>,
    source: &[u8],
) -> Vec<EnumValueDescriptor> {
    let mut out = Vec::new();
    let mut cursor = array.walk();
    for raw_child in array.children(&mut cursor) {
        let child = unwrap_yaml_node(raw_child);
        match child.kind() {
            "string"
            | "double_quote_scalar"
            | "single_quote_scalar"
            | "string_scalar"
            | "plain_scalar" => {
                if let Some(name) = scalar_string(child, source) {
                    out.push(EnumValueDescriptor {
                        name,
                        byte_range: child.byte_range(),
                        integer_value: None,
                        integer_value_range: 0..0,
                    });
                }
            }
            "object" | "block_mapping" | "flow_mapping" => {
                let name_pair = find_pair_with_key(child, source, "name");
                let value_pair = find_pair_with_key(child, source, "value");
                let Some(name_pair) = name_pair else {
                    continue;
                };
                let Some(name_value_raw) = name_pair.named_child(1) else {
                    continue;
                };
                let name_value = unwrap_yaml_node(name_value_raw);
                let Some(name) = scalar_string(name_value, source) else {
                    continue;
                };
                let (integer_value, integer_range) = match value_pair {
                    Some(pair) => {
                        let v = pair.named_child(1).map(unwrap_yaml_node);
                        match v {
                            Some(node) => (scalar_string(node, source), node.byte_range()),
                            None => (None, 0..0),
                        }
                    }
                    None => (None, 0..0),
                };
                out.push(EnumValueDescriptor {
                    name,
                    byte_range: child.byte_range(),
                    integer_value,
                    integer_value_range: integer_range,
                });
            }
            // YAML block_sequence_item wraps the actual element.
            "block_sequence_item" => {
                let mut inner_cursor = child.walk();
                for inner in child.children(&mut inner_cursor) {
                    let inner = unwrap_yaml_node(inner);
                    match inner.kind() {
                        "string"
                        | "double_quote_scalar"
                        | "single_quote_scalar"
                        | "string_scalar"
                        | "plain_scalar" => {
                            if let Some(name) = scalar_string(inner, source) {
                                out.push(EnumValueDescriptor {
                                    name,
                                    byte_range: inner.byte_range(),
                                    integer_value: None,
                                    integer_value_range: 0..0,
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn check_duplicate_enum_values(
    descriptors: &[EnumValueDescriptor],
    out: &mut Vec<DomainDiagnostic>,
) {
    let mut seen_names: std::collections::BTreeMap<&str, std::ops::Range<usize>> =
        std::collections::BTreeMap::new();
    for descriptor in descriptors {
        if let Some(_prev) =
            seen_names.insert(descriptor.name.as_str(), descriptor.byte_range.clone())
        {
            push_complex(
                out,
                descriptor.byte_range.clone(),
                &format!("Duplicate enum value `{}`", descriptor.name),
            );
        }
    }

    // Integer enums: also catch duplicate numeric values.
    let mut seen_values: std::collections::BTreeMap<String, std::ops::Range<usize>> =
        std::collections::BTreeMap::new();
    for descriptor in descriptors {
        let Some(value) = &descriptor.integer_value else {
            continue;
        };
        if seen_values
            .insert(value.clone(), descriptor.integer_value_range.clone())
            .is_some()
        {
            push_complex(
                out,
                descriptor.integer_value_range.clone(),
                &format!("Duplicate enum numeric value `{value}`"),
            );
        }
    }
}

fn push_complex(
    out: &mut Vec<DomainDiagnostic>,
    byte_range: std::ops::Range<usize>,
    message: &str,
) {
    out.push(DomainDiagnostic {
        byte_range,
        severity: Severity::Error,
        message: message.to_string(),
        code: "complex-type".to_string(),
    });
}

fn scalar_text<'a>(pair: tree_sitter::Node<'_>, source: &'a [u8]) -> Option<&'a str> {
    let value_raw = pair.named_child(1)?;
    let value = unwrap_yaml_node(value_raw);
    let text = std::str::from_utf8(&source[value.byte_range()]).ok()?;
    Some(strip_quotes_str(text))
}

fn scalar_string(node: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(&source[node.byte_range()]).ok()?;
    Some(strip_quotes_str(text).to_string())
}

fn find_value_for_key<'tree>(
    node: tree_sitter::Node<'tree>,
    source: &[u8],
    target_key: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_pair_node(child)
            && pair_key_text(child, source).is_some_and(|k| k == target_key)
            && let Some(value) = child.named_child(1)
        {
            return Some(value);
        }
        if let Some(found) = find_value_for_key(child, source, target_key) {
            return Some(found);
        }
    }
    None
}

fn find_pair_with_key<'tree>(
    object: tree_sitter::Node<'tree>,
    source: &[u8],
    target_key: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut cursor = object.walk();
    object.children(&mut cursor).find(|&child| {
        is_pair_node(child) && pair_key_text(child, source).is_some_and(|k| k == target_key)
    })
}

fn is_pair_node(node: tree_sitter::Node<'_>) -> bool {
    matches!(node.kind(), "pair" | "block_mapping_pair")
}

fn pair_key_text<'a>(pair: tree_sitter::Node<'_>, source: &'a [u8]) -> Option<&'a str> {
    let key = pair.named_child(0)?;
    let text = std::str::from_utf8(&source[key.byte_range()]).ok()?;
    Some(strip_quotes_str(text))
}

fn strip_quotes_str(s: &str) -> &str {
    let trimmed = s.trim();
    trimmed
        .strip_prefix('"')
        .and_then(|w| w.strip_suffix('"'))
        .or_else(|| {
            trimmed
                .strip_prefix('\'')
                .and_then(|w| w.strip_suffix('\''))
        })
        .unwrap_or(trimmed)
}

fn walk_for_errors(node: tree_sitter::Node<'_>, out: &mut Vec<DomainDiagnostic>) {
    if node.is_error() || node.is_missing() {
        out.push(DomainDiagnostic {
            byte_range: node.byte_range(),
            severity: Severity::Error,
            message: if node.is_missing() {
                format!("Missing {}", node.kind())
            } else {
                "Syntax error".to_string()
            },
            code: "syntax-error".to_string(),
        });
        return;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_for_errors(child, out);
    }
}

pub(super) fn try_parse_json(text: &str, out: &mut Vec<DomainDiagnostic>) -> Option<TableDef> {
    match serde_json::from_str::<TableDef>(text) {
        Ok(table) => normalize_table(&table, out),
        Err(e) => {
            let byte = byte_offset_for_line_col(text, e.line(), e.column());
            out.push(DomainDiagnostic {
                byte_range: byte..(byte + 1).min(text.len()),
                severity: Severity::Error,
                message: format!("JSON parse error: {e}"),
                code: "parse-error".to_string(),
            });
            None
        }
    }
}

pub(super) fn try_parse_yaml(text: &str, out: &mut Vec<DomainDiagnostic>) -> Option<TableDef> {
    match serde_yaml::from_str::<TableDef>(text) {
        Ok(table) => normalize_table(&table, out),
        Err(e) => {
            let byte = e.location().map_or(0, |loc| loc.index().min(text.len()));
            out.push(DomainDiagnostic {
                byte_range: byte..(byte + 1).min(text.len()),
                severity: Severity::Error,
                message: format!("YAML parse error: {e}"),
                code: "parse-error".to_string(),
            });
            None
        }
    }
}

/// Run `TableDef::normalize()` so inline constraints participate in planner validation.
fn normalize_table(table: &TableDef, out: &mut Vec<DomainDiagnostic>) -> Option<TableDef> {
    match table.normalize() {
        Ok(table) => Some(table),
        Err(e) => {
            out.push(DomainDiagnostic {
                byte_range: 0..1,
                severity: Severity::Error,
                message: e.to_string(),
                code: "validate-schema".to_string(),
            });
            None
        }
    }
}

pub(super) fn validate_table(table: &TableDef, out: &mut Vec<DomainDiagnostic>) {
    // Single-table validation. `vespertide_planner::validate_schema` expects
    // `&[TableDef]`; for LSP per-file diagnostics, run on a singleton slice.
    if let Err(e) = vespertide_planner::validate_schema(std::slice::from_ref(table)) {
        out.push(DomainDiagnostic {
            byte_range: 0..1,
            severity: Severity::Error,
            message: e.to_string(),
            code: "validate-schema".to_string(),
        });
    }
}

/// Compare the file's basename to its declared table `name` and surface a
/// warning when they diverge. This catches accidental renames where the
/// user changes `"name"` but forgets to rename the file (or vice versa).
///
/// Path → basename rules (longest extension wins):
///   `foo.vespertide.json` → `foo`
///   `foo.vespertide.yaml` → `foo`
///   `foo.vespertide.yml`  → `foo`
///   `foo.json` / `foo.yaml` / `foo.yml` → `foo`
pub(super) fn check_filename_table_name_mismatch(
    text: &str,
    uri: &Uri,
    tree: Option<&tree_sitter::Tree>,
    table_name: &str,
    out: &mut Vec<DomainDiagnostic>,
) {
    let Some(file_basename) = file_basename_of(uri) else {
        return;
    };
    if file_basename == table_name {
        return;
    }
    let byte_range = super::locator::locate_top_name(tree, text).unwrap_or(0..1);
    out.push(DomainDiagnostic {
        byte_range,
        severity: Severity::Warning,
        message: format!(
            "Table name `{table_name}` does not match file basename `{file_basename}`. \
             Rename one to keep them in sync."
        ),
        code: "filename-mismatch".to_string(),
    });
}

fn file_basename_of(uri: &Uri) -> Option<String> {
    let path = crate::position::uri_to_path(uri)?;
    let file_name = path.file_name()?.to_str()?;
    let stripped = file_name
        .strip_suffix(".vespertide.json")
        .or_else(|| file_name.strip_suffix(".vespertide.yaml"))
        .or_else(|| file_name.strip_suffix(".vespertide.yml"))
        .or_else(|| file_name.strip_suffix(".json"))
        .or_else(|| file_name.strip_suffix(".yaml"))
        .or_else(|| file_name.strip_suffix(".yml"))
        .unwrap_or(file_name);
    Some(stripped.to_string())
}

pub(super) fn validate_workspace(
    workspace: &[WorkspaceTable],
    current_uri: &Uri,
    out: &mut Vec<DomainDiagnostic>,
) {
    let tables: Vec<TableDef> = workspace.iter().map(|entry| entry.table.clone()).collect();
    let Err(err) = vespertide_planner::validate_schema(&tables) else {
        return;
    };

    let Some(location) = super::locator::ErrorLocation::from_planner_error(&err) else {
        push_validate_error(out, 0..1, err.to_string());
        return;
    };

    let Some(target) = workspace
        .iter()
        .find(|entry| entry.table.name.as_str() == location.table.as_str())
    else {
        push_validate_error(out, 0..1, err.to_string());
        return;
    };

    if target.uri != *current_uri {
        return;
    }

    let byte_range = if let Some(column) = &location.column {
        if let Some(field) = location.field {
            super::locator::locate_column_field(target.tree.as_ref(), &target.source, column, field)
        } else {
            super::locator::locate_column(target.tree.as_ref(), &target.source, column)
        }
    } else if let Some(constraint) = &location.constraint {
        super::locator::locate_constraint(target.tree.as_ref(), &target.source, constraint)
    } else {
        super::locator::locate_top_name(target.tree.as_ref(), &target.source).unwrap_or(0..1)
    };

    push_validate_error(out, byte_range, err.to_string());
}

fn push_validate_error(
    out: &mut Vec<DomainDiagnostic>,
    byte_range: std::ops::Range<usize>,
    message: String,
) {
    out.push(DomainDiagnostic {
        byte_range,
        severity: Severity::Error,
        message,
        code: "validate-schema".to_string(),
    });
}

fn byte_offset_for_line_col(text: &str, line: usize, col: usize) -> usize {
    // serde_json line/column values are 1-indexed.
    let line_zero = line.saturating_sub(1);
    let col_zero = col.saturating_sub(1);
    let mut byte = 0;

    for (idx, line_text) in text.split_inclusive('\n').enumerate() {
        if idx == line_zero {
            return byte + col_zero.min(line_text.len().saturating_sub(1));
        }
        byte += line_text.len();
    }

    byte.min(text.len())
}
