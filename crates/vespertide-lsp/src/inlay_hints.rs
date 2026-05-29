//! Inlay hints — small inline annotations next to each column showing its
//! key semantics at a glance.
//!
//! For every column object in `columns` we emit (at most one) hint placed
//! at the closing `}` of that column:
//!
//! | Column shape | Hint label |
//! |---|---|
//! | `primary_key: true`                          | `PK`            |
//! | `foreign_key: { ref_table: T, ref_columns: [c] }` | `→ T.c`     |
//! | `unique: true`                               | `UQ`            |
//! | `index: true`                                | `IX`            |
//!
//! Multiple flags compose: a PK column with `unique` becomes `PK · UQ`.
//! The hint is intentionally terse — inlay hints share screen space with
//! the actual code, and noisy annotations are worse than none.

use std::collections::HashMap;
use std::ops::Range;

use vespertide_planner::{CheckTokenKind, lex_check_expr};

use crate::check_expr_range::expr_inner_range;

/// A single inline annotation. The LSP layer maps `byte_offset` to an LSP
/// `Position` and uses `InlayHintKind::TYPE` for these (matching how
/// rust-analyzer surfaces type info — the closest semantic match).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainInlayHint {
    /// Byte offset where the hint is anchored (we use the column's closing
    /// brace position so the annotation reads after the column literal).
    pub byte_offset: usize,
    /// Display text (e.g. `" ⟶ user.id"`).
    pub label: String,
}

/// Compute inlay hints for the visible byte range of a document.
///
/// `visible_range` mirrors the LSP `inlayHint.range`, letting clients
/// request hints incrementally for the on-screen area. An empty range is
/// allowed and yields no hints.
#[must_use]
pub fn compute(
    source: &str,
    tree: Option<&tree_sitter::Tree>,
    visible_range: Range<usize>,
) -> Vec<DomainInlayHint> {
    let Some(tree) = tree else {
        return Vec::new();
    };
    let source_bytes = source.as_bytes();
    let Some(columns_value) = find_value_for_key(tree.root_node(), source_bytes, "columns") else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let column_objects = direct_column_objects(columns_value);
    for column in &column_objects {
        if !ranges_overlap(&column.byte_range(), &visible_range) {
            continue;
        }
        if let Some(hint) = column_to_hint(*column, source_bytes) {
            out.push(hint);
        }
    }

    // CHECK-expr column-type hints. Resolve `column_name -> type_label`
    // from the SAME in-doc columns pass — never from disk, so unsaved
    // edits never race.
    let type_map = build_column_type_map(&column_objects, source_bytes);
    if !type_map.is_empty()
        && let Some(constraints_value) =
            find_value_for_key(tree.root_node(), source_bytes, "constraints")
    {
        emit_check_expr_type_hints(
            constraints_value,
            source_bytes,
            &type_map,
            &visible_range,
            &mut out,
        );
    }

    out
}

/// Walk every column object and pull its `name` + `type` into a flat
/// `name -> type_label` map. Simple string types render as-is
/// (`"integer"` → `integer`); complex object types render their `kind`
/// (`{"kind":"varchar",...}` → `varchar`). Columns with malformed or
/// missing `type` are skipped — the inlay just won't show for them.
fn build_column_type_map(
    columns: &[tree_sitter::Node<'_>],
    source: &[u8],
) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for column in columns {
        let Some(name) = pair_string_value(*column, source, "name") else {
            continue;
        };
        let Some(type_label) = column_type_label(*column, source) else {
            continue;
        };
        map.insert(name, type_label);
    }
    map
}

fn pair_string_value(object: tree_sitter::Node<'_>, source: &[u8], key: &str) -> Option<String> {
    let pair = find_pair_with_key(object, source, key)?;
    let value_raw = pair.named_child(1)?;
    let value = unwrap_yaml_node(value_raw);
    let text = std::str::from_utf8(source.get(value.byte_range())?).ok()?;
    Some(strip_quotes(text.trim()).to_string())
}

fn column_type_label(column: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    let pair = find_pair_with_key(column, source, "type")?;
    let value_raw = pair.named_child(1)?;
    let value = unwrap_yaml_node(value_raw);
    match value.kind() {
        "object" | "block_mapping" | "flow_mapping" => {
            // Complex type — render the `kind` discriminant.
            pair_string_value(value, source, "kind")
        }
        _ => {
            let text = std::str::from_utf8(source.get(value.byte_range())?).ok()?;
            let stripped = strip_quotes(text.trim());
            if stripped.is_empty() {
                None
            } else {
                Some(stripped.to_string())
            }
        }
    }
}

/// Walk `constraints[*].expr` scalars within `visible_range`. For each
/// expression, lex it, and emit a `: <type>` hint anchored at the END
/// of every Column token whose name resolves in `type_map`.
fn emit_check_expr_type_hints(
    constraints_value: tree_sitter::Node<'_>,
    source: &[u8],
    type_map: &HashMap<String, String>,
    visible_range: &Range<usize>,
    out: &mut Vec<DomainInlayHint>,
) {
    let array = unwrap_yaml_node(constraints_value);
    if !matches!(array.kind(), "array" | "block_sequence" | "flow_sequence") {
        return;
    }

    let mut cursor = array.walk();
    for raw_child in array.children(&mut cursor) {
        let item = unwrap_yaml_node(raw_child);
        let object = match item.kind() {
            "object" | "block_mapping" | "flow_mapping" => item,
            "block_sequence_item" => {
                let mut inner_cursor = item.walk();
                let Some(found) = item.children(&mut inner_cursor).find_map(|c| {
                    let inner = unwrap_yaml_node(c);
                    matches!(inner.kind(), "object" | "block_mapping" | "flow_mapping")
                        .then_some(inner)
                }) else {
                    continue;
                };
                found
            }
            _ => continue,
        };

        let Some(expr_pair) = find_pair_with_key(object, source, "expr") else {
            continue;
        };
        let Some(expr_value_raw) = expr_pair.named_child(1) else {
            continue;
        };
        let expr_value = unwrap_yaml_node(expr_value_raw);

        let Some(inner) = expr_inner_range(expr_value) else {
            continue;
        };
        if !ranges_overlap(&inner, visible_range) {
            continue;
        }
        let Some(expr_bytes) = source.get(inner.clone()) else {
            continue;
        };
        let Ok(expr_text) = std::str::from_utf8(expr_bytes) else {
            continue;
        };

        for token in lex_check_expr(expr_text) {
            if token.kind != CheckTokenKind::Column {
                continue;
            }
            let Some(name_bytes) = expr_text.as_bytes().get(token.span.clone()) else {
                continue;
            };
            let Ok(name) = std::str::from_utf8(name_bytes) else {
                continue;
            };
            let Some(type_label) = type_map.get(name) else {
                continue;
            };
            out.push(DomainInlayHint {
                byte_offset: inner.start + token.span.end,
                label: format!(": {type_label}"),
            });
        }
    }
}

fn column_to_hint(column: tree_sitter::Node<'_>, source: &[u8]) -> Option<DomainInlayHint> {
    let mut tags: Vec<String> = Vec::new();

    if pair_is_true(column, source, "primary_key") {
        tags.push("PK".to_string());
    }
    if let Some(fk) = foreign_key_target(column, source) {
        tags.push(format!("⟶ {fk}"));
    }
    if pair_is_true(column, source, "unique") {
        tags.push("UQ".to_string());
    }
    if pair_is_true(column, source, "index") {
        tags.push("IX".to_string());
    }

    if tags.is_empty() {
        return None;
    }

    // Anchor right AFTER the opening brace so the hint sits on the brace
    // line of a pretty-printed column object — never colliding with the
    // first pair underneath:
    //
    //     { ⟪ PK · ⟶ user.id ⟫    ← hint here, on the `{` line
    //       "name": "id",
    //       "type": "integer",
    //       ...
    //     }
    //
    // Single-line column objects (`{"name":"id"}`) place the hint between
    // `{` and the first pair, which still keeps the closing brace clean.
    let column_start = column.byte_range().start;
    let anchor = column_start.saturating_add(1);

    Some(DomainInlayHint {
        byte_offset: anchor,
        label: format!(" ⟪ {} ⟫", tags.join(" · ")),
    })
}

fn pair_is_true(object: tree_sitter::Node<'_>, source: &[u8], key: &str) -> bool {
    let Some(pair) = find_pair_with_key(object, source, key) else {
        return false;
    };
    let Some(value_raw) = pair.named_child(1) else {
        return false;
    };
    let value = unwrap_yaml_node(value_raw);
    matches!(
        source
            .get(value.byte_range())
            .and_then(|b| std::str::from_utf8(b).ok())
            .map(str::trim),
        Some("true")
    )
}

/// Extract `"target_table.target_column"` from a column's `foreign_key`
/// object. Returns `None` when the FK is malformed (missing fields).
fn foreign_key_target(column: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    let fk_pair = find_pair_with_key(column, source, "foreign_key")?;
    let fk_object_raw = fk_pair.named_child(1)?;
    let fk_object = unwrap_yaml_node(fk_object_raw);
    if !matches!(
        fk_object.kind(),
        "object" | "block_mapping" | "flow_mapping"
    ) {
        return None;
    }

    let table_pair = find_pair_with_key(fk_object, source, "ref_table")?;
    let table_value = unwrap_yaml_node(table_pair.named_child(1)?);
    let table = strip_quotes(std::str::from_utf8(source.get(table_value.byte_range())?).ok()?);

    let columns_pair = find_pair_with_key(fk_object, source, "ref_columns")?;
    let columns_array = unwrap_yaml_node(columns_pair.named_child(1)?);
    if !matches!(
        columns_array.kind(),
        "array" | "block_sequence" | "flow_sequence"
    ) {
        return None;
    }
    // Use the first element so the hint stays compact; composite FKs are
    // rare and the user can see the rest by hovering on `ref_columns`.
    let first_column_text = first_array_string(columns_array, source)?;
    Some(format!("{table}.{first_column_text}"))
}

fn first_array_string(array: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    let mut cursor = array.walk();
    for raw in array.children(&mut cursor) {
        let node = unwrap_yaml_node(raw);
        match node.kind() {
            "string"
            | "double_quote_scalar"
            | "single_quote_scalar"
            | "string_scalar"
            | "plain_scalar" => {
                let text = std::str::from_utf8(source.get(node.byte_range())?).ok()?;
                return Some(strip_quotes(text).to_string());
            }
            "block_sequence_item" => {
                let mut inner = node.walk();
                for inner_child in node.children(&mut inner) {
                    let inner_node = unwrap_yaml_node(inner_child);
                    if matches!(
                        inner_node.kind(),
                        "string"
                            | "double_quote_scalar"
                            | "single_quote_scalar"
                            | "string_scalar"
                            | "plain_scalar"
                    ) {
                        let text =
                            std::str::from_utf8(source.get(inner_node.byte_range())?).ok()?;
                        return Some(strip_quotes(text).to_string());
                    }
                }
            }
            _ => {}
        }
    }
    None
}

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

fn find_value_for_key<'tree>(
    node: tree_sitter::Node<'tree>,
    source: &[u8],
    target_key: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if matches!(child.kind(), "pair" | "block_mapping_pair")
            && child
                .named_child(0)
                .and_then(|key| std::str::from_utf8(&source[key.byte_range()]).ok())
                .map(strip_quotes)
                == Some(target_key)
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
        matches!(child.kind(), "pair" | "block_mapping_pair")
            && child
                .named_child(0)
                .and_then(|key| std::str::from_utf8(&source[key.byte_range()]).ok())
                .map(strip_quotes)
                == Some(target_key)
    })
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

fn ranges_overlap(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DocumentFormat, ParserPool};

    fn parse(src: &str) -> tree_sitter::Tree {
        ParserPool::new().parse(src, DocumentFormat::Json).unwrap()
    }

    #[test]
    fn primary_key_column_gets_pk_hint() {
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer","primary_key":true}]}"#;
        let tree = parse(src);
        let hints = compute(src, Some(&tree), 0..src.len());
        assert_eq!(hints.len(), 1);
        assert!(hints[0].label.contains("PK"));
    }

    /// Regression — the hint must anchor JUST AFTER the opening brace
    /// of the column object. On a multi-line column this puts the hint
    /// on the `{` line, leaving the first pair clean on the next line.
    #[test]
    fn hint_is_anchored_immediately_after_opening_brace() {
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer","primary_key":true}]}"#;
        let tree = parse(src);
        let hints = compute(src, Some(&tree), 0..src.len());
        assert_eq!(hints.len(), 1);

        let column_start = src.find(r#"{"name":"id""#).unwrap();
        assert_eq!(
            hints[0].byte_offset,
            column_start + 1,
            "hint should anchor at the byte directly after `{{`"
        );
        // Must NOT anchor on the closing brace.
        assert_ne!(
            hints[0].byte_offset,
            column_start + r#"{"name":"id","type":"integer","primary_key":true}"#.len() - 1,
        );
    }

    #[test]
    fn foreign_key_column_gets_arrow_hint() {
        let src = r#"{"name":"p","columns":[{"name":"author_id","type":"integer","foreign_key":{"ref_table":"user","ref_columns":["id"]}}]}"#;
        let tree = parse(src);
        let hints = compute(src, Some(&tree), 0..src.len());
        assert_eq!(hints.len(), 1);
        assert!(
            hints[0].label.contains("user.id"),
            "expected `user.id` in hint, got: {}",
            hints[0].label
        );
    }

    #[test]
    fn multiple_flags_compose() {
        let src = r#"{"name":"u","columns":[{"name":"id","type":"uuid","primary_key":true,"unique":true,"index":true}]}"#;
        let tree = parse(src);
        let hints = compute(src, Some(&tree), 0..src.len());
        assert_eq!(hints.len(), 1);
        let label = &hints[0].label;
        assert!(
            label.contains("PK") && label.contains("UQ") && label.contains("IX"),
            "got: {label}"
        );
    }

    #[test]
    fn plain_column_without_flags_emits_no_hint() {
        let src = r#"{"name":"u","columns":[{"name":"name","type":"text","nullable":true}]}"#;
        let tree = parse(src);
        let hints = compute(src, Some(&tree), 0..src.len());
        assert!(hints.is_empty(), "got: {hints:?}");
    }

    #[test]
    fn visible_range_filters_hints_to_on_screen_columns() {
        let src = r#"{"name":"u","columns":[{"name":"a","type":"integer","primary_key":true},{"name":"b","type":"text","unique":true}]}"#;
        let tree = parse(src);
        // Only the FIRST column is in the visible range — the user has
        // scrolled past the second one.
        let first_end =
            src.find(r#""primary_key":true"#).unwrap() + r#""primary_key":true"#.len() + 2;
        let hints = compute(src, Some(&tree), 0..first_end);
        assert_eq!(hints.len(), 1, "expected only the visible column's hint");
        assert!(hints[0].label.contains("PK"));
    }

    #[test]
    fn yaml_inlay_hints() {
        let pool = ParserPool::new();
        let src = "name: post\ncolumns:\n  - name: author_id\n    type: integer\n    foreign_key:\n      ref_table: user\n      ref_columns: [id]\n";
        let tree = pool.parse(src, DocumentFormat::Yaml).unwrap();
        let hints = compute(src, Some(&tree), 0..src.len());
        assert_eq!(hints.len(), 1);
        assert!(hints[0].label.contains("user.id"));
    }

    // ----------------------------------------------------------------------
    // CHECK-expr inlay hints — type annotation for column refs inside CHECK
    // expressions. The hint is anchored at the END of the column identifier
    // INSIDE the `expr` string (so it reads as `age|: integer | > 0`).
    // ----------------------------------------------------------------------

    /// I-S1: a CHECK expression that references a declared column gets a
    /// `": integer"` type hint anchored at the end of the `age` token
    /// INSIDE the `expr` string value.
    #[test]
    fn i_s1_check_expr_column_gets_type_hint() {
        let src = r#"{"name":"u","columns":[{"name":"age","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"chk","expr":"age > 0"}]}"#;
        let tree = parse(src);
        let hints = compute(src, Some(&tree), 0..src.len());

        // Compute expected anchor: end of `age` INSIDE the expr string.
        // The expr value is `"age > 0"`. The inner content starts right
        // after the opening quote of that string.
        let expr_field = r#""expr":"age > 0""#;
        let expr_field_start = src.find(expr_field).expect("expr field present");
        // After `"expr":"` the inner content begins.
        let inner_start = expr_field_start + r#""expr":""#.len();
        let age_offset_in_expr = "age > 0".find("age").unwrap();
        let expected_anchor = inner_start + age_offset_in_expr + "age".len();

        let check_hint = hints
            .iter()
            .find(|h| h.byte_offset == expected_anchor)
            .unwrap_or_else(|| {
                panic!(
                    "expected a CHECK-expr type hint at byte_offset {expected_anchor}; got: {hints:?}"
                )
            });
        assert!(
            check_hint.label.contains("integer"),
            "expected label to contain `integer`, got: {:?}",
            check_hint.label,
        );
        assert!(
            check_hint.label.contains(':'),
            "expected `: integer`-style label, got: {:?}",
            check_hint.label,
        );
    }

    /// I-S2: a CHECK expression referencing an UNDECLARED column produces
    /// NO type hint at that column's position. We pin this by computing
    /// the position where the hint *would* go and asserting no hint sits
    /// there.
    #[test]
    fn i_s2_check_expr_unknown_column_no_hint() {
        let src = r#"{"name":"u","columns":[{"name":"age","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"chk","expr":"unknownCol > 0"}]}"#;
        let tree = parse(src);
        let hints = compute(src, Some(&tree), 0..src.len());

        let expr_field = r#""expr":"unknownCol > 0""#;
        let expr_field_start = src.find(expr_field).expect("expr field present");
        let inner_start = expr_field_start + r#""expr":""#.len();
        let unknown_end = inner_start + "unknownCol".len();

        assert!(
            !hints.iter().any(|h| h.byte_offset == unknown_end),
            "no hint should be emitted for an undeclared column; got: {hints:?}"
        );
        // Also: no hint label should mention `unknownCol`.
        assert!(
            !hints.iter().any(|h| h.label.contains("unknownCol")),
            "no hint label should reference unknownCol; got: {hints:?}"
        );
    }

    /// I-S3 regression: adding CHECK-expr inlays must NOT disturb the
    /// existing column-flag inlay (PK at the column's `{`).
    #[test]
    fn i_s3_existing_column_flag_inlays_unchanged() {
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"age","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"chk","expr":"age > 0"}]}"#;
        let tree = parse(src);
        let hints = compute(src, Some(&tree), 0..src.len());

        // The PK column inlay must still appear at the byte AFTER `{` of
        // the `id` column object — same anchor rule as the legacy pass.
        let id_col_start = src
            .find(r#"{"name":"id""#)
            .expect("id column object present");
        let pk_anchor = id_col_start + 1;
        let pk_hint = hints
            .iter()
            .find(|h| h.byte_offset == pk_anchor)
            .unwrap_or_else(|| panic!("PK flag inlay missing at {pk_anchor}; got: {hints:?}"));
        assert!(
            pk_hint.label.contains("PK"),
            "expected PK in legacy flag inlay, got: {:?}",
            pk_hint.label,
        );
    }
}
