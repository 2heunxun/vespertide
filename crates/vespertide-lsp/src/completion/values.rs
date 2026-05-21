//! Concrete completion values: schema literals and cross-file table lookups.

use std::collections::BTreeSet;

use super::{CompletionItemKind, DomainCompletion};
use crate::store::DocumentStore;
use crate::workspace_index::WorkspaceIndex;
use crate::workspace_tables::WorkspaceTables;

const COLUMN_TYPES: &[&str] = &[
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
    "inet",
    "cidr",
    "macaddr",
    "xml",
];

const REFERENCE_ACTIONS: &[&str] = &[
    "cascade",
    "restrict",
    "set_null",
    "set_default",
    "no_action",
];

pub(super) fn column_types() -> Vec<DomainCompletion> {
    let mut completions = COLUMN_TYPES
        .iter()
        .map(|column_type| value(column_type, format!("Column type: {column_type}")))
        .collect::<Vec<_>>();

    completions.extend([
        snippet(
            "varchar(N)",
            "Variable-length string",
            r#"{"kind":"varchar","length":${1:255}}"#,
        ),
        snippet(
            "char(N)",
            "Fixed-length string",
            r#"{"kind":"char","length":${1:2}}"#,
        ),
        snippet(
            "numeric(P,S)",
            "Fixed-precision decimal",
            r#"{"kind":"numeric","precision":${1:10},"scale":${2:2}}"#,
        ),
        snippet(
            "enum",
            "Native string enum",
            r#"{"kind":"enum","name":"${1:status}","values":["${2:active}","${3:inactive}"]}"#,
        ),
    ]);

    completions
}

pub(super) fn reference_actions() -> Vec<DomainCompletion> {
    REFERENCE_ACTIONS
        .iter()
        .map(|action| value(action, format!("Reference action: {action}")))
        .collect()
}

pub(super) fn booleans() -> Vec<DomainCompletion> {
    ["true", "false"]
        .into_iter()
        .map(|label| DomainCompletion {
            label: label.to_string(),
            kind: CompletionItemKind::Value,
            detail: None,
            insert_text: None,
            sort_priority: 1,
        })
        .collect()
}

pub(super) fn tables_in_workspace(
    index: &WorkspaceIndex,
    disk_tables: Option<&WorkspaceTables>,
) -> Vec<DomainCompletion> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();

    for name in index.tables() {
        seen.insert(name.clone());
        out.push(table_completion(name));
    }

    if let Some(disk_tables) = disk_tables {
        for name in disk_tables.names() {
            if seen.insert(name.clone()) {
                out.push(table_completion(name));
            }
        }
    }

    out
}

pub(super) fn columns_of(
    table_name: &str,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
    disk_tables: Option<&WorkspaceTables>,
) -> Vec<DomainCompletion> {
    if let Some(loc) = index.lookup(table_name) {
        let open_columns = docs
            .with_doc(&loc.uri, |text, _tree| {
                parse_table(text)
                    .map_or_else(Vec::new, |table| column_completions(table_name, &table))
            })
            .unwrap_or_default();

        if !open_columns.is_empty() {
            return open_columns;
        }
    }

    disk_tables
        .and_then(|tables| tables.get(table_name))
        .map_or_else(Vec::new, |table| column_completions(table_name, &table))
}

fn table_completion(name: String) -> DomainCompletion {
    DomainCompletion {
        detail: Some(format!("Table: {name}")),
        label: name,
        kind: CompletionItemKind::Reference,
        insert_text: None,
        sort_priority: 1,
    }
}

fn column_completions(
    table_name: &str,
    table: &vespertide_core::TableDef,
) -> Vec<DomainCompletion> {
    table
        .columns
        .iter()
        .map(|column| DomainCompletion {
            label: column.name.as_str().to_string(),
            kind: CompletionItemKind::Reference,
            detail: Some(format!("Column in {table_name}")),
            insert_text: None,
            sort_priority: 1,
        })
        .collect()
}

fn value(label: &str, detail: String) -> DomainCompletion {
    DomainCompletion {
        label: label.to_string(),
        kind: CompletionItemKind::Value,
        detail: Some(detail),
        insert_text: None,
        sort_priority: 1,
    }
}

fn snippet(label: &str, detail: &str, insert_text: &str) -> DomainCompletion {
    DomainCompletion {
        label: label.to_string(),
        kind: CompletionItemKind::Snippet,
        detail: Some(detail.to_string()),
        insert_text: Some(insert_text.to_string()),
        sort_priority: 2,
    }
}

fn parse_table(text: &str) -> Option<vespertide_core::TableDef> {
    serde_json::from_str(text)
        .ok()
        .or_else(|| serde_yaml::from_str(text).ok())
}
