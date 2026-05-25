/// Simple column type names recognized as string literals. Mirrors
/// `vespertide_core::SimpleColumnType`. Kept here so we can flag unknown
/// strings BEFORE serde fails — serde's error position is unreliable inside
/// untagged enums and tends to point at the wrong byte (often the column's
/// closing brace).
pub(super) const KNOWN_SIMPLE_TYPES: &[&str] = &[
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

pub(super) struct EnumValueDescriptor {
    pub(super) name: String,
    pub(super) byte_range: std::ops::Range<usize>,
    /// Optional explicit integer value (for integer enums).
    pub(super) integer_value: Option<String>,
    pub(super) integer_value_range: std::ops::Range<usize>,
}

/// Peel YAML's `flow_node` / `block_node` wrappers (no-op on JSON).
pub(super) fn unwrap_yaml_node(node: tree_sitter::Node<'_>) -> tree_sitter::Node<'_> {
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

pub(super) fn collect_enum_value_descriptors(
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

pub(super) fn scalar_text<'a>(pair: tree_sitter::Node<'_>, source: &'a [u8]) -> Option<&'a str> {
    let value_raw = pair.named_child(1)?;
    let value = unwrap_yaml_node(value_raw);
    let text = std::str::from_utf8(&source[value.byte_range()]).ok()?;
    Some(strip_quotes_str(text))
}

pub(super) fn scalar_string(node: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(&source[node.byte_range()]).ok()?;
    Some(strip_quotes_str(text).to_string())
}

#[cfg(test)]
pub(super) fn find_value_for_key<'tree>(
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

pub(super) fn find_pair_with_key<'tree>(
    object: tree_sitter::Node<'tree>,
    source: &[u8],
    target_key: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut cursor = object.walk();
    object.children(&mut cursor).find(|&child| {
        is_pair_node(child) && pair_key_text(child, source).is_some_and(|k| k == target_key)
    })
}

pub(super) fn is_pair_node(node: tree_sitter::Node<'_>) -> bool {
    matches!(node.kind(), "pair" | "block_mapping_pair")
}

pub(super) fn pair_key_text<'a>(pair: tree_sitter::Node<'_>, source: &'a [u8]) -> Option<&'a str> {
    let key = pair.named_child(0)?;
    let text = std::str::from_utf8(&source[key.byte_range()]).ok()?;
    Some(strip_quotes_str(text))
}

pub(super) fn strip_quotes_str(s: &str) -> &str {
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
