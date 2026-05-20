//! Column hover: markdown showing name, type, nullable, default, constraints.

use std::fmt::Write as _;
use std::ops::Range;

use super::DomainHover;

pub(super) fn try_hover(node: tree_sitter::Node<'_>, source: &str) -> Option<DomainHover> {
    let mut cur = Some(node);
    while let Some(candidate) = cur {
        if is_mapping(candidate)
            && is_inside_columns(candidate, source)
            && let Some(markdown) = column_object_markdown(candidate, source)
        {
            return Some(DomainHover {
                markdown,
                byte_range: highlight_range(node, candidate),
            });
        }
        cur = candidate.parent();
    }
    None
}

fn column_object_markdown(obj: tree_sitter::Node<'_>, source: &str) -> Option<String> {
    let mut name: Option<String> = None;
    let mut type_str: Option<String> = None;
    let mut nullable: Option<bool> = None;
    let mut default: Option<String> = None;
    let mut constraints = Vec::new();

    let mut cursor = obj.walk();
    for child in obj.children(&mut cursor) {
        if !is_pair(child) {
            continue;
        }
        let Some(key) = child.named_child(0) else {
            continue;
        };
        let Some(value) = child.named_child(1) else {
            continue;
        };

        let key_text = strip_quotes(&source[key.byte_range()]);
        let value_text = source[value.byte_range()].trim();
        match key_text {
            "name" => name = Some(strip_quotes(value_text).to_string()),
            "type" => type_str = Some(display_value(value_text).to_string()),
            "nullable" => nullable = Some(value_text == "true"),
            "default" => default = Some(display_value(value_text).to_string()),
            "primary_key" if constraint_is_enabled(value_text) => constraints.push("PK"),
            "unique" if constraint_is_enabled(value_text) => constraints.push("UNIQUE"),
            "index" if constraint_is_enabled(value_text) => constraints.push("INDEX"),
            "foreign_key" if constraint_is_enabled(value_text) => constraints.push("FK"),
            _ => {}
        }
    }

    let name = name?;
    let type_str = type_str?;
    let mut markdown = format!("**{name}**: `{}`", type_str.trim());
    if let Some(nullable) = nullable {
        let _ = write!(markdown, "  \nnullable: `{nullable}`");
    }
    if let Some(default) = default {
        let _ = write!(markdown, "  \ndefault: `{}`", default.trim());
    }
    if !constraints.is_empty() {
        let _ = write!(markdown, "  \nconstraints: {}", constraints.join(", "));
    }
    Some(markdown)
}

fn is_inside_columns(node: tree_sitter::Node<'_>, source: &str) -> bool {
    let mut cur = node.parent();
    while let Some(candidate) = cur {
        if is_pair(candidate)
            && let Some(key) = candidate.named_child(0)
            && strip_quotes(&source[key.byte_range()]) == "columns"
        {
            return true;
        }
        cur = candidate.parent();
    }
    false
}

fn highlight_range(node: tree_sitter::Node<'_>, fallback: tree_sitter::Node<'_>) -> Range<usize> {
    let range = node.byte_range();
    if range.is_empty() {
        fallback.byte_range()
    } else {
        range
    }
}

fn is_mapping(node: tree_sitter::Node<'_>) -> bool {
    matches!(node.kind(), "object" | "block_mapping")
}

fn is_pair(node: tree_sitter::Node<'_>) -> bool {
    matches!(node.kind(), "pair" | "block_mapping_pair")
}

fn constraint_is_enabled(value: &str) -> bool {
    !matches!(value.trim(), "false" | "null" | "[]" | "{}")
}

fn display_value(value: &str) -> &str {
    strip_quotes(value.trim())
}

fn strip_quotes(s: &str) -> &str {
    s.trim()
        .trim_start_matches('"')
        .trim_end_matches('"')
        .trim_start_matches('\'')
        .trim_end_matches('\'')
}
