//! Code actions — quick refactors for the column under the cursor.
//!
//! V1 surface area:
//! * Toggle `primary_key`, `unique`, `index` on a column (add when absent,
//!   remove when present).
//! * Toggle nullability (`nullable: true` ↔ `nullable: false`).
//!
//! All edits are produced as JSON-aware byte-level [`DomainTextEdit`]s. JSON
//! is the format we drive most often and where the structural rules are
//! tightest — YAML support can grow from the same data model later.
//!
//! Returned [`DomainCodeAction`]s carry the title (what the editor lists)
//! and a small set of edits scoped to the file the cursor is in. Workspace
//! edits are out of scope here; rename / references handle those.

use std::ops::Range;

use crate::parser::DocumentFormat;
use crate::rename::DomainTextEdit;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainCodeAction {
    pub title: String,
    pub kind: CodeActionKind,
    pub edits: Vec<DomainTextEdit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeActionKind {
    /// `refactor.rewrite` family — for boolean flag toggles.
    Refactor,
}

/// Compute available code actions for the byte range under the cursor.
#[must_use]
pub fn compute(
    source: &str,
    format: DocumentFormat,
    tree: Option<&tree_sitter::Tree>,
    byte_range: Range<usize>,
) -> Vec<DomainCodeAction> {
    // YAML structural edits are non-trivial (indent-aware). V1 only emits
    // actions for JSON — YAML can grow from the same data model later.
    if format != DocumentFormat::Json {
        return Vec::new();
    }
    let Some(tree) = tree else {
        return Vec::new();
    };
    let source_bytes = source.as_bytes();
    let Some(column) = enclosing_column_object(tree.root_node(), source_bytes, byte_range.start)
    else {
        return Vec::new();
    };
    let mut actions = Vec::new();
    for (flag, on_title, off_title) in [
        ("primary_key", "Mark column as primary key", "Unmark primary key"),
        ("unique", "Mark column as unique", "Remove unique"),
        ("index", "Add index to column", "Remove index"),
    ] {
        actions.extend(toggle_bool_flag(
            column,
            source_bytes,
            flag,
            on_title,
            off_title,
        ));
    }
    actions.extend(toggle_nullable(column, source_bytes));
    actions
}

fn toggle_bool_flag(
    column: tree_sitter::Node<'_>,
    source: &[u8],
    flag: &str,
    title_when_adding: &str,
    title_when_removing: &str,
) -> Option<DomainCodeAction> {
    if let Some(pair) = find_pair(column, source, flag) {
        let value = pair.named_child(1)?;
        let value_text = std::str::from_utf8(&source[value.byte_range()]).ok()?;
        if value_text.trim() == "true" {
            return Some(DomainCodeAction {
                title: title_when_removing.to_string(),
                kind: CodeActionKind::Refactor,
                edits: vec![remove_pair_edit(column, pair, source)?],
            });
        }
        // Field exists but is `false` / something else — flip to true.
        return Some(DomainCodeAction {
            title: title_when_adding.to_string(),
            kind: CodeActionKind::Refactor,
            edits: vec![DomainTextEdit {
                byte_range: value.byte_range(),
                new_text: "true".to_string(),
            }],
        });
    }

    // Flag absent → insert it.
    Some(DomainCodeAction {
        title: title_when_adding.to_string(),
        kind: CodeActionKind::Refactor,
        edits: vec![insert_pair_edit(column, source, flag, "true")?],
    })
}

fn toggle_nullable(column: tree_sitter::Node<'_>, source: &[u8]) -> Option<DomainCodeAction> {
    let pair = find_pair(column, source, "nullable");
    if let Some(pair) = pair {
        let value = pair.named_child(1)?;
        let value_text = std::str::from_utf8(&source[value.byte_range()]).ok()?;
        let (next_value, title) = match value_text.trim() {
            "true" => ("false", "Make column NOT NULL"),
            "false" => ("true", "Allow NULL"),
            _ => return None,
        };
        return Some(DomainCodeAction {
            title: title.to_string(),
            kind: CodeActionKind::Refactor,
            edits: vec![DomainTextEdit {
                byte_range: value.byte_range(),
                new_text: next_value.to_string(),
            }],
        });
    }

    // Absent → assume the caller wants a non-nullable column (most common).
    Some(DomainCodeAction {
        title: "Make column NOT NULL".to_string(),
        kind: CodeActionKind::Refactor,
        edits: vec![insert_pair_edit(column, source, "nullable", "false")?],
    })
}

fn enclosing_column_object<'tree>(
    root: tree_sitter::Node<'tree>,
    source: &[u8],
    byte_offset: usize,
) -> Option<tree_sitter::Node<'tree>> {
    let mut current = root;
    'outer: loop {
        let mut cursor = current.walk();
        for child in current.children(&mut cursor) {
            if child.byte_range().contains(&byte_offset) {
                current = child;
                continue 'outer;
            }
        }
        break;
    }
    // Walk back up to the smallest enclosing JSON object whose ancestor is
    // a `columns` array.
    let mut node = Some(current);
    while let Some(candidate) = node {
        if candidate.kind() == "object" && is_inside_columns_array(candidate, source) {
            return Some(candidate);
        }
        node = candidate.parent();
    }
    None
}

fn is_inside_columns_array(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if candidate.kind() == "pair"
            && let Some(key) = candidate.named_child(0)
            && let Ok(text) = std::str::from_utf8(&source[key.byte_range()])
            && strip_quotes(text) == "columns"
        {
            return true;
        }
        current = candidate.parent();
    }
    false
}

fn find_pair<'tree>(
    object: tree_sitter::Node<'tree>,
    source: &[u8],
    target_key: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut cursor = object.walk();
    object.children(&mut cursor).find(|&child| {
        child.kind() == "pair"
            && child
                .named_child(0)
                .and_then(|key| std::str::from_utf8(&source[key.byte_range()]).ok())
                .map(strip_quotes)
                == Some(target_key)
    })
}

fn insert_pair_edit(
    column: tree_sitter::Node<'_>,
    source: &[u8],
    new_key: &str,
    new_value: &str,
) -> Option<DomainTextEdit> {
    let object_text = std::str::from_utf8(&source[column.byte_range()]).ok()?;
    let close_idx = object_text.rfind('}')?;
    let absolute_close = column.start_byte() + close_idx;

    // Inspect what comes immediately before the closing brace so we know
    // whether to emit a leading comma.
    let trimmed = object_text[..close_idx].trim_end();
    let needs_comma = trimmed.ends_with(|c: char| c != '{' && c != ',');
    let insertion = if needs_comma {
        format!(",\"{new_key}\":{new_value}")
    } else {
        format!("\"{new_key}\":{new_value}")
    };

    Some(DomainTextEdit {
        byte_range: absolute_close..absolute_close,
        new_text: insertion,
    })
}

fn remove_pair_edit(
    column: tree_sitter::Node<'_>,
    pair: tree_sitter::Node<'_>,
    source: &[u8],
) -> Option<DomainTextEdit> {
    // Decide which neighbouring comma to consume so the resulting JSON has
    // no `,,` or trailing `,}`.
    let object_text = std::str::from_utf8(&source[column.byte_range()]).ok()?;
    let object_start = column.start_byte();
    let pair_start = pair.start_byte() - object_start;
    let pair_end = pair.end_byte() - object_start;

    let before = &object_text[..pair_start];
    let after = &object_text[pair_end..];

    let trim_before = before.trim_end_matches(|c: char| c.is_whitespace());
    let trim_after = after.trim_start_matches(|c: char| c.is_whitespace());

    let removed_start;
    let removed_end;
    if trim_before.ends_with(',') {
        // Eat the leading comma + any whitespace between it and the pair.
        let comma_offset = trim_before.len() - 1;
        removed_start = object_start + comma_offset;
        removed_end = object_start + pair_end;
    } else if trim_after.starts_with(',') {
        // The pair is at the front; eat the comma that follows it.
        let comma_offset = pair_end + (after.len() - trim_after.len()) + 1;
        removed_start = object_start + pair_start;
        removed_end = object_start + comma_offset;
    } else {
        // Single pair object — just drop it.
        removed_start = object_start + pair_start;
        removed_end = object_start + pair_end;
    }

    Some(DomainTextEdit {
        byte_range: removed_start..removed_end,
        new_text: String::new(),
    })
}

fn strip_quotes(text: &str) -> &str {
    text.trim()
        .trim_start_matches('"')
        .trim_end_matches('"')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DocumentFormat, ParserPool};

    fn parse(src: &str) -> tree_sitter::Tree {
        ParserPool::new().parse(src, DocumentFormat::Json).unwrap()
    }

    #[test]
    fn add_primary_key_when_absent() {
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer"}]}"#;
        let tree = parse(src);
        let cursor = src.find(r#""name":"id""#).unwrap() + 2;
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);

        let action = actions
            .iter()
            .find(|a| a.title == "Mark column as primary key")
            .expect("primary_key add action missing");
        assert_eq!(action.edits.len(), 1);
        let edit = &action.edits[0];
        assert_eq!(edit.new_text, r#","primary_key":true"#);
    }

    #[test]
    fn remove_primary_key_when_present() {
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer","primary_key":true}]}"#;
        let tree = parse(src);
        let cursor = src.find(r#""name":"id""#).unwrap() + 2;
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);

        let action = actions
            .iter()
            .find(|a| a.title == "Unmark primary key")
            .expect("primary_key remove action missing");
        let edit = &action.edits[0];
        // Confirm the edit produces a valid object when applied.
        let mut after = String::from(&src[..edit.byte_range.start]);
        after.push_str(&edit.new_text);
        after.push_str(&src[edit.byte_range.end..]);
        assert!(serde_json::from_str::<serde_json::Value>(&after).is_ok());
        assert!(!after.contains("primary_key"));
    }

    #[test]
    fn toggle_nullable_flips_value() {
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer","nullable":false}]}"#;
        let tree = parse(src);
        let cursor = src.find(r#""name":"id""#).unwrap() + 2;
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);

        let action = actions
            .iter()
            .find(|a| a.title == "Allow NULL")
            .expect("nullable toggle missing");
        let edit = &action.edits[0];
        assert_eq!(edit.new_text, "true");
        // Sanity check the byte_range covers `false` only.
        assert_eq!(&src[edit.byte_range.clone()], "false");
    }

    #[test]
    fn cursor_outside_column_returns_no_actions() {
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer"}]}"#;
        let tree = parse(src);
        // Position cursor inside the table-level `name` value — NOT a column.
        let cursor = src.find(r#""name":"u""#).unwrap() + 9;
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);
        assert!(
            actions.is_empty(),
            "no actions expected outside a column, got: {actions:?}"
        );
    }

    #[test]
    fn yaml_returns_no_actions_in_v1() {
        let src = "name: u\ncolumns:\n  - name: id\n    type: integer\n";
        let pool = ParserPool::new();
        let tree = pool.parse(src, DocumentFormat::Yaml).unwrap();
        let cursor = src.find("name: id").unwrap();
        let actions = compute(src, DocumentFormat::Yaml, Some(&tree), cursor..cursor);
        assert!(actions.is_empty(), "YAML actions are out of scope in V1");
    }
}
