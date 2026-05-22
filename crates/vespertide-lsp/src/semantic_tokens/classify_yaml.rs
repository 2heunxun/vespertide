//! YAML counterpart to [`super::classify_json`]. Same semantic mapping,
//! but the YAML tree-sitter grammar uses different node kinds:
//!
//! | tree-sitter-yaml          | role                                   |
//! |---------------------------|----------------------------------------|
//! | `block_mapping`           | top-level mapping (the table)          |
//! | `block_mapping_pair`      | a `key: value` pair                    |
//! | `flow_node`/`block_node`  | pure wrappers — peeled                 |
//! | `block_sequence`          | YAML list                              |
//! | `block_sequence_item`     | `- ...` entry                          |
//! | `flow_sequence`           | `[a, b, c]` inline list                |
//! | `flow_mapping`            | `{a: b, c: d}` inline mapping          |
//! | `plain_scalar`            | unquoted scalar `varchar`              |
//! | `double_quote_scalar` /   | quoted scalar — trim 1 byte each side  |
//! | `single_quote_scalar`     |                                        |
//!
//! YAML quoted scalars include the delimiters in their byte range,
//! plain scalars don't. The push helpers trim where appropriate.

#![allow(clippy::struct_excessive_bools)]

use super::{legend::ModIdx, legend::TokenIdx, RawToken};

#[must_use]
pub fn classify(source: &str, tree: &tree_sitter::Tree) -> Vec<RawToken> {
    let mut out = Vec::new();
    walk(tree.root_node(), source.as_bytes(), Ctx::default(), &mut out);
    out
}

#[derive(Debug, Clone, Copy, Default)]
struct Ctx {
    inside_columns: bool,
    inside_column: bool,
    inside_complex_type_object: bool,
    inside_enum_values_array: bool,
    inside_foreign_key: bool,
    inside_ref_columns: bool,
    mapping_depth: u32,
}

fn walk(node: tree_sitter::Node<'_>, source: &[u8], ctx: Ctx, out: &mut Vec<RawToken>) {
    match node.kind() {
        "block_mapping" | "flow_mapping" => {
            let new_ctx = Ctx {
                mapping_depth: ctx.mapping_depth + 1,
                ..ctx
            };
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                walk(child, source, new_ctx, out);
            }
        }
        "block_mapping_pair" | "flow_pair" => {
            classify_pair(node, source, ctx, out);
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                walk(child, source, ctx, out);
            }
        }
    }
}

fn classify_pair(
    pair: tree_sitter::Node<'_>,
    source: &[u8],
    ctx: Ctx,
    out: &mut Vec<RawToken>,
) {
    let Some(key_node) = pair.named_child(0) else {
        return;
    };
    let Some(value_node) = pair.named_child(1) else {
        return;
    };
    let key_node = unwrap_yaml(key_node);
    let value_node = unwrap_yaml(value_node);
    let Some(key_text) = scalar_text(key_node, source) else {
        return;
    };

    match key_text {
        "name" if ctx.mapping_depth == 1 && !ctx.inside_columns => {
            push_scalar(value_node, TokenIdx::Class, ModIdx::Declaration as u32, out);
        }
        "name" if ctx.inside_column && !ctx.inside_complex_type_object => {
            push_scalar(value_node, TokenIdx::Property, ModIdx::Declaration as u32, out);
        }
        "name" if ctx.inside_enum_values_array => {
            push_scalar(value_node, TokenIdx::EnumMember, 0, out);
        }
        "columns" => {
            let new_ctx = Ctx {
                inside_columns: true,
                ..ctx
            };
            recurse_value(value_node, source, new_ctx, out);
            return;
        }
        "type" if ctx.inside_columns => {
            classify_type_value(value_node, source, ctx, out);
            return;
        }
        "kind" if ctx.inside_complex_type_object => {
            push_scalar(value_node, TokenIdx::EnumMember, 0, out);
        }
        "values" if ctx.inside_complex_type_object => {
            let new_ctx = Ctx {
                inside_enum_values_array: true,
                ..ctx
            };
            recurse_value(value_node, source, new_ctx, out);
            return;
        }
        "foreign_key" => {
            let new_ctx = Ctx {
                inside_foreign_key: true,
                ..ctx
            };
            recurse_value(value_node, source, new_ctx, out);
            return;
        }
        "ref_table" if ctx.inside_foreign_key => {
            push_scalar(value_node, TokenIdx::Class, ModIdx::Definition as u32, out);
        }
        "ref_columns" if ctx.inside_foreign_key => {
            let new_ctx = Ctx {
                inside_ref_columns: true,
                ..ctx
            };
            recurse_value(value_node, source, new_ctx, out);
            return;
        }
        "on_delete" | "on_update" if ctx.inside_foreign_key => {
            push_scalar(value_node, TokenIdx::EnumMember, 0, out);
        }
        "default" if ctx.inside_column => {
            classify_default(value_node, out);
        }
        _ => {}
    }

    recurse_value(value_node, source, ctx, out);
}

fn recurse_value(
    value: tree_sitter::Node<'_>,
    source: &[u8],
    ctx: Ctx,
    out: &mut Vec<RawToken>,
) {
    let v = unwrap_yaml(value);

    if ctx.inside_ref_columns && matches!(v.kind(), "block_sequence" | "flow_sequence") {
        let mut cursor = v.walk();
        for child in v.children(&mut cursor) {
            let element = unwrap_yaml(child);
            if is_scalar(element.kind()) {
                push_scalar(element, TokenIdx::Property, ModIdx::Definition as u32, out);
            } else {
                walk(child, source, ctx, out);
            }
        }
        return;
    }

    if ctx.inside_enum_values_array && matches!(v.kind(), "block_sequence" | "flow_sequence") {
        let mut cursor = v.walk();
        for child in v.children(&mut cursor) {
            let element = unwrap_yaml(child);
            if is_scalar(element.kind()) {
                push_scalar(element, TokenIdx::EnumMember, 0, out);
            } else {
                walk(child, source, ctx, out);
            }
        }
        return;
    }

    // The general "columns array" case must come last — nested arrays
    // (ref_columns, values) sit *inside* columns and would otherwise
    // be shadowed by this broader branch.
    if ctx.inside_columns && matches!(v.kind(), "block_sequence" | "flow_sequence") {
        let mut cursor = v.walk();
        for child in v.children(&mut cursor) {
            let element = unwrap_yaml(child);
            let item_ctx = if matches!(
                element.kind(),
                "block_mapping" | "flow_mapping" | "block_sequence_item"
            ) {
                Ctx {
                    inside_column: true,
                    ..ctx
                }
            } else {
                ctx
            };
            walk(child, source, item_ctx, out);
        }
        return;
    }

    walk(value, source, ctx, out);
}

fn classify_type_value(
    value: tree_sitter::Node<'_>,
    source: &[u8],
    ctx: Ctx,
    out: &mut Vec<RawToken>,
) {
    let v = unwrap_yaml(value);
    match v.kind() {
        k if is_scalar(k) => {
            push_scalar(v, TokenIdx::Type, 0, out);
        }
        "block_mapping" | "flow_mapping" => {
            let inner_ctx = Ctx {
                inside_complex_type_object: true,
                ..ctx
            };
            walk(value, source, inner_ctx, out);
        }
        _ => walk(value, source, ctx, out),
    }
}

fn classify_default(value: tree_sitter::Node<'_>, out: &mut Vec<RawToken>) {
    let v = unwrap_yaml(value);
    if is_scalar(v.kind()) {
        // YAML doesn't distinguish bool/null/string/number scalars by
        // tree-sitter node kind alone — treat every scalar default as a
        // string; themes can still highlight `true`/`false`/`null` via
        // their own keyword scope from the syntax grammar.
        push_scalar(v, TokenIdx::String, 0, out);
    }
}

fn push_scalar(
    node: tree_sitter::Node<'_>,
    token_type: TokenIdx,
    token_modifiers: u32,
    out: &mut Vec<RawToken>,
) {
    let range = inner_scalar_range(node);
    if range.end <= range.start {
        return;
    }
    out.push(RawToken {
        byte_range: range,
        token_type: token_type as u32,
        token_modifiers,
    });
}

fn inner_scalar_range(node: tree_sitter::Node<'_>) -> std::ops::Range<usize> {
    let raw = node.byte_range();
    match node.kind() {
        "double_quote_scalar" | "single_quote_scalar" => {
            if raw.end.saturating_sub(raw.start) >= 2 {
                (raw.start + 1)..(raw.end - 1)
            } else {
                raw
            }
        }
        _ => raw,
    }
}

fn unwrap_yaml(node: tree_sitter::Node<'_>) -> tree_sitter::Node<'_> {
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

fn is_scalar(kind: &str) -> bool {
    matches!(
        kind,
        "plain_scalar"
            | "double_quote_scalar"
            | "single_quote_scalar"
            | "string_scalar"
            | "integer_scalar"
            | "float_scalar"
            | "boolean_scalar"
            | "null_scalar"
    )
}

fn scalar_text<'a>(node: tree_sitter::Node<'_>, source: &'a [u8]) -> Option<&'a str> {
    let raw = std::str::from_utf8(source.get(node.byte_range())?).ok()?;
    Some(raw.trim().trim_matches('"').trim_matches('\''))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DocumentFormat, ParserPool};

    fn classify_yaml(src: &str) -> Vec<RawToken> {
        let tree = ParserPool::new()
            .parse(src, DocumentFormat::Yaml)
            .expect("parse");
        classify(src, &tree)
    }

    #[test]
    fn yaml_table_and_column_names_classified() {
        let src = "name: user\ncolumns:\n  - name: id\n    type: integer\n";
        let tokens = classify_yaml(src);
        let user_start = src.find("user").unwrap();
        let id_start = src.find("name: id").unwrap() + 6;

        let user_tok = tokens
            .iter()
            .find(|t| t.byte_range.start == user_start)
            .expect("user");
        let id_tok = tokens
            .iter()
            .find(|t| t.byte_range.start == id_start)
            .expect("id");
        assert_eq!(user_tok.token_type, TokenIdx::Class as u32);
        assert_eq!(id_tok.token_type, TokenIdx::Property as u32);
    }

    #[test]
    fn yaml_foreign_key_ref_table_is_class_definition() {
        let src = "name: post\ncolumns:\n  - name: a\n    type: integer\n    foreign_key:\n      ref_table: user\n      ref_columns: [id]\n";
        let tokens = classify_yaml(src);
        let user_start = src.find("ref_table: user").unwrap() + "ref_table: ".len();
        let tok = tokens
            .iter()
            .find(|t| t.byte_range.start == user_start)
            .expect("ref_table token");
        assert_eq!(tok.token_type, TokenIdx::Class as u32);
        assert_eq!(tok.token_modifiers, ModIdx::Definition as u32);
    }
}
