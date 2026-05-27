use super::{MigrationAction, MigrationPlan};
use crate::schema::TableName;

impl MigrationPlan {
    /// Apply a prefix to all table names in the migration plan.
    /// This modifies all table references in all actions.
    pub fn with_prefix(self, prefix: &str) -> Self {
        if prefix.is_empty() {
            return self;
        }
        Self {
            actions: self
                .actions
                .into_iter()
                .map(|action| action.with_prefix(prefix))
                .collect(),
            ..self
        }
    }
}

impl MigrationAction {
    /// Apply a prefix to all table names in this action.
    pub fn with_prefix(self, prefix: &str) -> Self {
        if prefix.is_empty() {
            return self;
        }

        prefix_migration_action(self, prefix)
    }
}

fn prefix_migration_action(action: MigrationAction, prefix: &str) -> MigrationAction {
    match action {
        MigrationAction::CreateTable {
            table,
            columns,
            constraints,
        } => MigrationAction::CreateTable {
            table: add_prefix(table, prefix),
            columns,
            constraints: constraints
                .into_iter()
                .map(|c| c.with_prefix(prefix))
                .collect(),
        },
        MigrationAction::DeleteTable { table } => MigrationAction::DeleteTable {
            table: add_prefix(table, prefix),
        },
        MigrationAction::RenameTable { from, to } => MigrationAction::RenameTable {
            from: add_prefix(from, prefix),
            to: add_prefix(to, prefix),
        },
        MigrationAction::RawSql { sql } => MigrationAction::RawSql { sql },
        action => prefix_column_or_constraint_action(action, prefix),
    }
}

fn prefix_column_or_constraint_action(action: MigrationAction, prefix: &str) -> MigrationAction {
    match action {
        MigrationAction::AddColumn {
            table,
            column,
            fill_with,
        } => MigrationAction::AddColumn {
            table: add_prefix(table, prefix),
            column,
            fill_with,
        },
        MigrationAction::RenameColumn { table, from, to } => MigrationAction::RenameColumn {
            table: add_prefix(table, prefix),
            from,
            to,
        },
        MigrationAction::DeleteColumn { table, column } => MigrationAction::DeleteColumn {
            table: add_prefix(table, prefix),
            column,
        },
        MigrationAction::ModifyColumnType {
            table,
            column,
            new_type,
            fill_with,
            narrowing_strategy,
            timezone,
        } => MigrationAction::ModifyColumnType {
            table: add_prefix(table, prefix),
            column,
            new_type,
            fill_with,
            narrowing_strategy,
            timezone,
        },
        MigrationAction::ModifyColumnNullable {
            table,
            column,
            nullable,
            fill_with,
            delete_null_rows,
        } => MigrationAction::ModifyColumnNullable {
            table: add_prefix(table, prefix),
            column,
            nullable,
            fill_with,
            delete_null_rows,
        },
        action => prefix_remaining_action(action, prefix),
    }
}

fn prefix_remaining_action(action: MigrationAction, prefix: &str) -> MigrationAction {
    match action {
        MigrationAction::ModifyColumnDefault {
            table,
            column,
            new_default,
            backfill,
        } => MigrationAction::ModifyColumnDefault {
            table: add_prefix(table, prefix),
            column,
            new_default,
            backfill,
        },
        MigrationAction::ModifyColumnComment {
            table,
            column,
            new_comment,
        } => MigrationAction::ModifyColumnComment {
            table: add_prefix(table, prefix),
            column,
            new_comment,
        },
        MigrationAction::AddConstraint { table, constraint } => MigrationAction::AddConstraint {
            table: format!("{prefix}{table}").into(),
            constraint: constraint.with_prefix(prefix),
        },
        MigrationAction::RemoveConstraint { table, constraint } => {
            MigrationAction::RemoveConstraint {
                table: add_prefix(table, prefix),
                constraint: constraint.with_prefix(prefix),
            }
        }
        MigrationAction::ReplaceConstraint { table, from, to } => {
            MigrationAction::ReplaceConstraint {
                table: add_prefix(table, prefix),
                from: from.with_prefix(prefix),
                to: to.with_prefix(prefix),
            }
        }
        other => other,
    }
}

fn add_prefix(table: TableName, prefix: &str) -> TableName {
    let mut table = table.into_inner();
    table.insert_str(0, prefix);
    table.into()
}
