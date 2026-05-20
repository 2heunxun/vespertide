use std::collections::{BTreeMap, BTreeSet};

use vespertide_core::{ColumnDef, ColumnType, ComplexColumnType, MigrationAction, TableDef};

pub(super) fn diff_columns(
    actions: &mut Vec<MigrationAction>,
    table_name: &str,
    from_tbl: &TableDef,
    to_tbl: &TableDef,
) -> BTreeSet<String> {
    // Columns - use BTreeMap for consistent ordering
    let from_cols: BTreeMap<_, _> = from_tbl
        .columns
        .iter()
        .map(|c| (c.name.as_str(), c))
        .collect();
    let to_cols: BTreeMap<_, _> = to_tbl
        .columns
        .iter()
        .map(|c| (c.name.as_str(), c))
        .collect();

    let deleted_columns = diff_deleted_columns(actions, table_name, &from_cols, &to_cols);
    diff_column_types(actions, table_name, &from_cols, &to_cols);
    diff_column_nullability(actions, table_name, &from_cols, &to_cols);
    diff_column_defaults(actions, table_name, &from_cols, &to_cols);
    diff_column_comments(actions, table_name, &from_cols, &to_cols);
    diff_added_columns(actions, table_name, &from_cols, &to_cols);

    deleted_columns
}

fn diff_deleted_columns(
    actions: &mut Vec<MigrationAction>,
    table_name: &str,
    from_cols: &BTreeMap<&str, &ColumnDef>,
    to_cols: &BTreeMap<&str, &ColumnDef>,
) -> BTreeSet<String> {
    let deleted_columns: BTreeSet<String> = from_cols
        .keys()
        .filter(|col| !to_cols.contains_key(*col))
        .map(|col| (*col).to_string())
        .collect();

    for col in &deleted_columns {
        actions.push(MigrationAction::DeleteColumn {
            table: table_name.to_string(),
            column: col.clone(),
        });
    }

    deleted_columns
}

fn diff_column_types(
    actions: &mut Vec<MigrationAction>,
    table_name: &str,
    from_cols: &BTreeMap<&str, &ColumnDef>,
    to_cols: &BTreeMap<&str, &ColumnDef>,
) {
    for (col, to_def) in to_cols {
        if let Some(from_def) = from_cols.get(col) {
            let needs_type_migration = from_def.r#type.requires_migration(&to_def.r#type);
            let needs_enum_rename = !needs_type_migration
                && matches!(
                    (&from_def.r#type, &to_def.r#type),
                    (
                        ColumnType::Complex(ComplexColumnType::Enum {
                            name: old_name,
                            values: old_values,
                        }),
                        ColumnType::Complex(ComplexColumnType::Enum {
                            name: new_name,
                            values: new_values,
                        }),
                    ) if old_name != new_name
                        && !old_values.is_integer()
                        && !new_values.is_integer()
                );

            if needs_type_migration || needs_enum_rename {
                actions.push(MigrationAction::ModifyColumnType {
                    table: table_name.to_string(),
                    column: (*col).to_string(),
                    new_type: to_def.r#type.clone(),
                    fill_with: None,
                });
            }
        }
    }
}

fn diff_column_nullability(
    actions: &mut Vec<MigrationAction>,
    table_name: &str,
    from_cols: &BTreeMap<&str, &ColumnDef>,
    to_cols: &BTreeMap<&str, &ColumnDef>,
) {
    for (col, to_def) in to_cols {
        if let Some(from_def) = from_cols.get(col)
            && from_def.nullable != to_def.nullable
        {
            actions.push(MigrationAction::ModifyColumnNullable {
                table: table_name.to_string(),
                column: (*col).to_string(),
                nullable: to_def.nullable,
                fill_with: None,
                delete_null_rows: None,
            });
        }
    }
}

fn diff_column_defaults(
    actions: &mut Vec<MigrationAction>,
    table_name: &str,
    from_cols: &BTreeMap<&str, &ColumnDef>,
    to_cols: &BTreeMap<&str, &ColumnDef>,
) {
    for (col, to_def) in to_cols {
        if let Some(from_def) = from_cols.get(col) {
            let from_default = from_def
                .default
                .as_ref()
                .map(vespertide_core::DefaultValue::to_sql);
            let to_default = to_def
                .default
                .as_ref()
                .map(vespertide_core::DefaultValue::to_sql);
            if from_default != to_default {
                actions.push(MigrationAction::ModifyColumnDefault {
                    table: table_name.to_string(),
                    column: (*col).to_string(),
                    new_default: to_default,
                });
            }
        }
    }
}

fn diff_column_comments(
    actions: &mut Vec<MigrationAction>,
    table_name: &str,
    from_cols: &BTreeMap<&str, &ColumnDef>,
    to_cols: &BTreeMap<&str, &ColumnDef>,
) {
    for (col, to_def) in to_cols {
        if let Some(from_def) = from_cols.get(col)
            && from_def.comment != to_def.comment
        {
            actions.push(MigrationAction::ModifyColumnComment {
                table: table_name.to_string(),
                column: (*col).to_string(),
                new_comment: to_def.comment.clone(),
            });
        }
    }
}

fn diff_added_columns(
    actions: &mut Vec<MigrationAction>,
    table_name: &str,
    from_cols: &BTreeMap<&str, &ColumnDef>,
    to_cols: &BTreeMap<&str, &ColumnDef>,
) {
    for (col, def) in to_cols {
        if !from_cols.contains_key(col) {
            let mut col_def = (*def).clone();
            col_def.primary_key = None;
            col_def.unique = None;
            col_def.index = None;
            col_def.foreign_key = None;
            actions.push(MigrationAction::AddColumn {
                table: table_name.to_string(),
                column: Box::new(col_def),
                fill_with: None,
            });
        }
    }
}
