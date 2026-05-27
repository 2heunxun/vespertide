use super::MigrationAction;
use crate::schema::TableConstraint;
use std::fmt;

impl fmt::Display for MigrationAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_migration_action(f, self)
    }
}

fn write_migration_action(f: &mut fmt::Formatter<'_>, action: &MigrationAction) -> fmt::Result {
    match action {
        MigrationAction::CreateTable { table, .. } => write!(f, "CreateTable: {table}"),
        MigrationAction::DeleteTable { table } => write!(f, "DeleteTable: {table}"),
        MigrationAction::AddColumn { table, column, .. } => {
            write!(f, "AddColumn: {}.{}", table, column.name)
        }
        MigrationAction::RenameColumn { table, from, to } => {
            write!(f, "RenameColumn: {table}.{from} -> {to}")
        }
        MigrationAction::DeleteColumn { table, column } => {
            write!(f, "DeleteColumn: {table}.{column}")
        }
        MigrationAction::ModifyColumnType { table, column, .. } => {
            write!(f, "ModifyColumnType: {table}.{column}")
        }
        MigrationAction::ModifyColumnNullable {
            table,
            column,
            nullable,
            ..
        } => write_nullable_action(f, table, column, *nullable),
        MigrationAction::ModifyColumnDefault {
            table,
            column,
            new_default,
        } => write_default_action(f, table, column, new_default.as_deref()),
        MigrationAction::ModifyColumnComment {
            table,
            column,
            new_comment,
        } => write_comment_action(f, table, column, new_comment.as_deref()),
        MigrationAction::AddConstraint { table, constraint } => {
            write_constraint_action(f, "AddConstraint", table, constraint)
        }
        MigrationAction::RemoveConstraint { table, constraint } => {
            write_constraint_action(f, "RemoveConstraint", table, constraint)
        }
        MigrationAction::ReplaceConstraint { table, to, .. } => {
            write_constraint_action(f, "ReplaceConstraint", table, to)
        }
        MigrationAction::RenameTable { from, to } => write!(f, "RenameTable: {from} -> {to}"),
        MigrationAction::RawSql { sql } => write_raw_sql_action(f, sql),
        MigrationAction::RemapEnumValues {
            table,
            column,
            mapping,
        } => {
            let summary = mapping
                .iter()
                .map(|(old, new)| format!("{old}->{new}"))
                .collect::<Vec<_>>()
                .join(", ");
            write!(f, "RemapEnumValues: {table}.{column} [{summary}]")
        }
    }
}

fn write_nullable_action(
    f: &mut fmt::Formatter<'_>,
    table: &str,
    column: &str,
    nullable: bool,
) -> fmt::Result {
    let nullability = if nullable { "NULL" } else { "NOT NULL" };
    write!(f, "ModifyColumnNullable: {table}.{column} -> {nullability}")
}

fn write_default_action(
    f: &mut fmt::Formatter<'_>,
    table: &str,
    column: &str,
    default: Option<&str>,
) -> fmt::Result {
    if let Some(default) = default {
        write!(f, "ModifyColumnDefault: {table}.{column} -> {default}")
    } else {
        write!(f, "ModifyColumnDefault: {table}.{column} -> (none)")
    }
}

fn write_comment_action(
    f: &mut fmt::Formatter<'_>,
    table: &str,
    column: &str,
    comment: Option<&str>,
) -> fmt::Result {
    if let Some(comment) = comment {
        let display = truncate_comment(comment);
        write!(f, "ModifyColumnComment: {table}.{column} -> '{display}'")
    } else {
        write!(f, "ModifyColumnComment: {table}.{column} -> (none)")
    }
}

fn truncate_comment(comment: &str) -> String {
    if comment.chars().count() > 30 {
        format!("{}...", truncate_chars(comment, 27))
    } else {
        comment.to_string()
    }
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

fn write_raw_sql_action(f: &mut fmt::Formatter<'_>, sql: &str) -> fmt::Result {
    let display_sql = if sql.chars().count() > 50 {
        format!("{}...", truncate_chars(sql, 47))
    } else {
        sql.to_string()
    };
    write!(f, "RawSql: {display_sql}")
}

fn write_constraint_action(
    f: &mut fmt::Formatter<'_>,
    action: &str,
    table: &str,
    constraint: &TableConstraint,
) -> fmt::Result {
    match constraint {
        TableConstraint::PrimaryKey { .. } => write!(f, "{action}: {table}.PRIMARY KEY"),
        TableConstraint::Unique { name, .. } => {
            write_named_constraint(f, action, table, name.as_ref(), "UNIQUE")
        }
        TableConstraint::ForeignKey { name, .. } => {
            write_named_constraint(f, action, table, name.as_ref(), "FOREIGN KEY")
        }
        TableConstraint::Check { name, .. } => write!(f, "{action}: {table}.{name} (CHECK)"),
        TableConstraint::Index { name, .. } => {
            write_named_constraint(f, action, table, name.as_ref(), "INDEX")
        }
    }
}

fn write_named_constraint(
    f: &mut fmt::Formatter<'_>,
    action: &str,
    table: &str,
    name: Option<&String>,
    fallback: &str,
) -> fmt::Result {
    if let Some(name) = name {
        write!(f, "{action}: {table}.{name} ({fallback})")
    } else {
        write!(f, "{action}: {table}.{fallback}")
    }
}
