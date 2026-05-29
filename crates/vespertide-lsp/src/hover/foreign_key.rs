//! Foreign-key hover: preview target table columns for `ref_table` values.

use crate::store::DocumentStore;
use crate::text_util::strip_quotes;
use crate::workspace_index::WorkspaceIndex;
use crate::workspace_tables::WorkspaceTables;

use super::DomainHover;

pub(super) fn try_hover(
    node: tree_sitter::Node<'_>,
    source: &str,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
    disk_tables: Option<&WorkspaceTables>,
) -> Option<DomainHover> {
    let pair = ref_table_pair(node, source)?;
    let value = pair.named_child(1)?;
    let target_name = strip_quotes(&source[value.byte_range()]).to_string();

    // Prefer an OPEN document (carries the user's current unsaved edits).
    if let Some(loc) = index.lookup(&target_name) {
        let preview = docs
            .with_doc(&loc.uri, |text, _tree| extract_column_summary(text))
            .unwrap_or_default();
        let detail = if preview.is_empty() {
            "_columns unavailable_".to_string()
        } else {
            preview
        };
        return Some(DomainHover {
            markdown: format!("**Target table**: `{target_name}`\n\n{detail}"),
            byte_range: value.byte_range(),
        });
    }

    // Fall back to on-disk discovery so closed model files still preview.
    if let Some(disk) = disk_tables
        && let Some(table) = disk.get(&target_name)
    {
        let preview = column_summary(&table);
        return Some(DomainHover {
            markdown: format!("**Target table**: `{target_name}` _(on disk)_\n\n{preview}"),
            byte_range: value.byte_range(),
        });
    }

    Some(DomainHover {
        markdown: format!("**Target table**: `{target_name}`\n\n⚠ table not found in workspace"),
        byte_range: value.byte_range(),
    })
}

fn ref_table_pair<'tree>(
    node: tree_sitter::Node<'tree>,
    source: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut cur = Some(node);
    while let Some(candidate) = cur {
        if is_pair(candidate)
            && let Some(key) = candidate.named_child(0)
            && strip_quotes(&source[key.byte_range()]) == "ref_table"
        {
            return Some(candidate);
        }
        cur = candidate.parent();
    }
    None
}

fn extract_column_summary(text: &str) -> String {
    match serde_json::from_str::<vespertide_core::TableDef>(text) {
        Ok(table) => column_summary(&table),
        Err(_) => match serde_yaml::from_str::<vespertide_core::TableDef>(text) {
            Ok(table) => column_summary(&table),
            Err(_) => String::new(),
        },
    }
}

fn column_summary(table: &vespertide_core::TableDef) -> String {
    let columns = table
        .columns
        .iter()
        .map(|column| column.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!("columns: {columns}")
}

fn is_pair(node: tree_sitter::Node<'_>) -> bool {
    matches!(node.kind(), "pair" | "block_mapping_pair")
}
