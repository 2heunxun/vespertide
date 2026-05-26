use anyhow::Result;
use colored::Colorize;
use vespertide_planner::{
    ConstraintDropWarning, MissingFkSupportingIndex, find_constraint_drops_without_replacement,
    find_missing_fk_supporting_indexes, plan_next_migration,
};

use crate::utils::{load_config, load_migrations, load_models};
use vespertide_core::{MigrationAction, MigrationPlan};

pub async fn cmd_diff() -> Result<()> {
    let config = load_config()?;
    let current_models = load_models(&config)?;
    let applied_plans = load_migrations(&config)?;

    let plan = plan_next_migration(&current_models, &applied_plans)
        .map_err(|e| anyhow::anyhow!("planning error: {e}"))?;

    if plan.actions.is_empty() {
        println!(
            "{} {}",
            "No differences found.".bright_green(),
            "Schema is up to date.".bright_white()
        );
    } else {
        println!(
            "{} {} {}",
            "Found".bright_cyan(),
            plan.actions.len().to_string().bright_yellow().bold(),
            "change(s) to apply:".bright_cyan()
        );
        println!();

        for (i, action) in plan.actions.iter().enumerate() {
            println!(
                "{}. {}",
                (i + 1).to_string().bright_magenta().bold(),
                format_action(action)
            );
        }
    }

    // Static safety analyses that run on the current model regardless of
    // whether there are pending actions — these are warnings, not blockers.
    emit_fk_supporting_index_warnings(&current_models);
    emit_constraint_drop_warnings(&plan);

    Ok(())
}

/// Surface `RemoveConstraint` actions that drop integrity-preserving
/// constraints (PK / UQ / FK / CHECK) with no explicit replacement.
///
/// This is fault **F50** in the data-dependent migration fault taxonomy:
/// the migration succeeds, but every subsequent write that the dropped
/// constraint would have rejected is now silently accepted.
fn emit_constraint_drop_warnings(plan: &MigrationPlan) {
    let warnings = find_constraint_drops_without_replacement(plan);
    if warnings.is_empty() {
        return;
    }

    println!();
    println!(
        "{} {}",
        "⚠".bright_yellow().bold(),
        format!(
            "{} constraint drop(s) without explicit replacement \
             (silent integrity loss risk):",
            warnings.len()
        )
        .bright_yellow()
    );
    for w in &warnings {
        println!();
        for line in format_constraint_drop_warning(w).lines() {
            println!("{line}");
        }
    }
}

/// Format a single `ConstraintDropWarning` as a multi-line indented block.
/// Extracted so its output can be unit-tested without going through stdout.
fn format_constraint_drop_warning(w: &ConstraintDropWarning) -> String {
    let kind_label = match w.kind {
        vespertide_core::ConstraintKind::PrimaryKey => "PRIMARY KEY",
        vespertide_core::ConstraintKind::Unique => "UNIQUE",
        vespertide_core::ConstraintKind::ForeignKey => "FOREIGN KEY",
        vespertide_core::ConstraintKind::Check => "CHECK",
        // Index is filtered out by the detector; this arm exists only to
        // satisfy the `#[non_exhaustive]` enum.
        _ => "(unknown)",
    };
    format!(
        "  {} {}\n  {} {}\n  {} future writes can silently violate this invariant\n  {} use `ReplaceConstraint(from, to)` to swap atomically, or keep the constraint",
        "on:".bright_white(),
        w.table.bright_cyan(),
        "drop:".bright_white(),
        format!("{} — {}", kind_label, w.label).bright_cyan().bold(),
        "why:".bright_white(),
        "fix:".bright_green(),
    )
}

/// Normalise the current model set and surface FK constraints that lack a
/// supporting index on the child table. Each FK is reported individually
/// with a concrete suggested index name.
///
/// This is fault **F51** in the data-dependent migration fault taxonomy:
/// it never produces a SQL error, but degrades cascade/lookup performance
/// silently as the child table grows.
fn emit_fk_supporting_index_warnings(current_models: &[vespertide_core::TableDef]) {
    // Normalise per-table; skip tables that fail to normalise (they will
    // have surfaced as planner errors above).
    let normalized: Vec<vespertide_core::TableDef> = current_models
        .iter()
        .filter_map(|t| t.normalize().ok())
        .collect();
    let missing = find_missing_fk_supporting_indexes(&normalized);
    if missing.is_empty() {
        return;
    }

    println!();
    println!(
        "{} {}",
        "⚠".bright_yellow().bold(),
        format!(
            "{} foreign key(s) lack a supporting index \
             (silent performance regression risk):",
            missing.len()
        )
        .bright_yellow()
    );
    for m in &missing {
        println!();
        for line in format_missing_fk_warning(m).lines() {
            println!("{line}");
        }
    }
}

/// Format a single `MissingFkSupportingIndex` as a multi-line indented block.
/// Extracted from `emit_fk_supporting_index_warnings` so its output can be
/// unit-tested without going through stdout.
fn format_missing_fk_warning(m: &MissingFkSupportingIndex) -> String {
    let fk_label = m.constraint_name.as_deref().unwrap_or("(unnamed)");
    let from = format!("{}({})", m.table, m.columns.join(", "));
    let to = format!("{}({})", m.ref_table, m.ref_columns.join(", "));
    format!(
        "  {} {}\n  {} {} {} {}\n  {} cascade/lookup scans the entire `{}` table\n  {} add index `{}`",
        "fk:".bright_white(),
        fk_label.bright_cyan(),
        "ref:".bright_white(),
        from.bright_cyan().bold(),
        "->".bright_white(),
        to.bright_cyan(),
        "why:".bright_white(),
        m.table,
        "fix:".bright_green(),
        m.suggested_index_name.bright_green().bold(),
    )
}

#[expect(
    clippy::too_many_lines,
    reason = "one display arm per migration action keeps output obvious"
)]
fn format_action(action: &MigrationAction) -> String {
    let table = action.table_name().map(Colorize::bright_cyan);
    match action {
        MigrationAction::CreateTable { .. } => {
            format!(
                "{} {}",
                "Create table:".bright_green(),
                table.expect("CreateTable has a table").bold()
            )
        }
        MigrationAction::DeleteTable { .. } => {
            format!(
                "{} {}",
                "Delete table:".bright_red(),
                table.expect("DeleteTable has a table").bold()
            )
        }
        MigrationAction::AddColumn { column, .. } => {
            format!(
                "{} {}.{}",
                "Add column:".bright_green(),
                table.expect("AddColumn has a table"),
                column.name.bright_cyan().bold()
            )
        }
        MigrationAction::RenameColumn { from, to, .. } => {
            format!(
                "{} {}.{} {} {}",
                "Rename column:".bright_yellow(),
                table.expect("RenameColumn has a table"),
                from.bright_white(),
                "->".bright_white(),
                to.bright_cyan().bold()
            )
        }
        MigrationAction::DeleteColumn { column, .. } => {
            format!(
                "{} {}.{}",
                "Delete column:".bright_red(),
                table.expect("DeleteColumn has a table"),
                column.bright_cyan().bold()
            )
        }
        MigrationAction::ModifyColumnType {
            column, new_type, ..
        } => {
            format!(
                "{} {}.{} {} {}",
                "Modify column type:".bright_yellow(),
                table.expect("ModifyColumnType has a table"),
                column.bright_cyan().bold(),
                "->".bright_white(),
                new_type.to_display_string().bright_cyan().bold()
            )
        }
        MigrationAction::ModifyColumnNullable {
            column, nullable, ..
        } => {
            let nullability = if *nullable { "NULL" } else { "NOT NULL" };
            format!(
                "{} {}.{} {} {}",
                "Modify column nullability:".bright_yellow(),
                table.expect("ModifyColumnNullable has a table"),
                column.bright_cyan().bold(),
                "->".bright_white(),
                nullability.bright_cyan().bold()
            )
        }
        MigrationAction::ModifyColumnDefault {
            column,
            new_default,
            ..
        } => {
            let default_display = new_default.as_deref().unwrap_or("(none)");
            format!(
                "{} {}.{} {} {}",
                "Modify column default:".bright_yellow(),
                table.expect("ModifyColumnDefault has a table"),
                column.bright_cyan().bold(),
                "->".bright_white(),
                default_display.bright_cyan().bold()
            )
        }
        MigrationAction::ModifyColumnComment {
            column,
            new_comment,
            ..
        } => {
            let comment_display = new_comment.as_deref().unwrap_or("(none)");
            let truncated = if comment_display.chars().count() > 30 {
                format!(
                    "{}...",
                    comment_display.chars().take(27).collect::<String>()
                )
            } else {
                comment_display.to_string()
            };
            format!(
                "{} {}.{} {} '{}'",
                "Modify column comment:".bright_yellow(),
                table.expect("ModifyColumnComment has a table"),
                column.bright_cyan().bold(),
                "->".bright_white(),
                truncated.bright_cyan().bold()
            )
        }
        MigrationAction::RenameTable { from, to } => {
            format!(
                "{} {} {} {}",
                "Rename table:".bright_yellow(),
                from.bright_cyan(),
                "->".bright_white(),
                to.bright_cyan().bold()
            )
        }
        MigrationAction::RawSql { sql } => {
            format!(
                "{} {}",
                "Execute raw SQL:".bright_yellow(),
                sql.bright_cyan()
            )
        }
        MigrationAction::AddConstraint { constraint, .. } => {
            format!(
                "{} {} {} {}",
                "Add constraint:".bright_green(),
                format_constraint_type(constraint).bright_cyan().bold(),
                "on".bright_white(),
                table.expect("AddConstraint has a table")
            )
        }
        MigrationAction::RemoveConstraint { constraint, .. } => {
            format!(
                "{} {} {} {}",
                "Remove constraint:".bright_red(),
                format_constraint_type(constraint).bright_cyan().bold(),
                "from".bright_white(),
                table.expect("RemoveConstraint has a table")
            )
        }
        MigrationAction::ReplaceConstraint { from, to, .. } => {
            format!(
                "{} {} {} {} {} {}",
                "Replace constraint:".bright_yellow(),
                format_constraint_type(from).bright_cyan().bold(),
                "->".bright_white(),
                format_constraint_type(to).bright_cyan().bold(),
                "on".bright_white(),
                table.expect("ReplaceConstraint has a table")
            )
        }
        _ => unreachable!("MigrationAction is #[non_exhaustive]; all variants are matched above"),
    }
}

fn format_constraint_type(constraint: &vespertide_core::TableConstraint) -> String {
    match constraint {
        vespertide_core::TableConstraint::PrimaryKey { columns, .. } => {
            format!("PRIMARY KEY ({})", columns.join(", "))
        }
        vespertide_core::TableConstraint::Unique { name, columns } => {
            if let Some(n) = name {
                format!("{} UNIQUE ({})", n, columns.join(", "))
            } else {
                format!("UNIQUE ({})", columns.join(", "))
            }
        }
        vespertide_core::TableConstraint::ForeignKey {
            name,
            columns,
            ref_table,
            ..
        } => {
            if let Some(n) = name {
                format!("{} FK ({}) -> {}", n, columns.join(", "), ref_table)
            } else {
                format!("FK ({}) -> {}", columns.join(", "), ref_table)
            }
        }
        vespertide_core::TableConstraint::Check { name, expr } => {
            format!("{name} CHECK ({expr})")
        }
        vespertide_core::TableConstraint::Index { name, columns } => {
            if let Some(n) = name {
                format!("{} INDEX ({})", n, columns.join(", "))
            } else {
                format!("INDEX ({})", columns.join(", "))
            }
        }
        _ => unreachable!("TableConstraint is #[non_exhaustive]; all variants are matched above"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use colored::Colorize;
    use rstest::rstest;
    use serial_test::serial;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;
    use vespertide_config::VespertideConfig;
    use vespertide_core::{
        ColumnDef, ColumnType, ReferenceAction, SimpleColumnType, TableConstraint, TableDef,
    };

    struct CwdGuard {
        original: PathBuf,
    }

    impl CwdGuard {
        fn new(dir: &PathBuf) -> Self {
            let original = std::env::current_dir().unwrap();
            std::env::set_current_dir(dir).unwrap();
            Self { original }
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.original);
        }
    }

    fn write_config() {
        let cfg = VespertideConfig::default();
        let text = serde_json::to_string_pretty(&cfg).unwrap();
        fs::write("vespertide.json", text).unwrap();
    }

    fn write_model(name: &str) {
        let models_dir = PathBuf::from("models");
        fs::create_dir_all(&models_dir).unwrap();
        let table = TableDef {
            name: name.into(),
            description: None,
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            }],
        };
        let path = models_dir.join(format!("{name}.json"));
        fs::write(path, serde_json::to_string_pretty(&table).unwrap()).unwrap();
    }

    #[rstest]
    #[case(
        MigrationAction::CreateTable { table: "users".into(), columns: vec![], constraints: vec![] },
        format!("{} {}", "Create table:".bright_green(), "users".bright_cyan().bold())
    )]
    #[case(
        MigrationAction::DeleteTable { table: "users".into() },
        format!("{} {}", "Delete table:".bright_red(), "users".bright_cyan().bold())
    )]
    #[case(
        MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(ColumnDef {
                name: "name".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }),
            fill_with: None,
        },
        format!("{} {}.{}", "Add column:".bright_green(), "users".bright_cyan(), "name".bright_cyan().bold())
    )]
    #[case(
        MigrationAction::RenameColumn {
            table: "users".into(),
            from: "old".into(),
            to: "new".into(),
        },
        format!("{} {}.{} {} {}", "Rename column:".bright_yellow(), "users".bright_cyan(), "old".bright_white(), "->".bright_white(), "new".bright_cyan().bold())
    )]
    #[case(
        MigrationAction::DeleteColumn { table: "users".into(), column: "name".into() },
        format!("{} {}.{}", "Delete column:".bright_red(), "users".bright_cyan(), "name".bright_cyan().bold())
    )]
    #[case(
        MigrationAction::ModifyColumnType {
            table: "users".into(),
            column: "id".into(),
            new_type: ColumnType::Simple(SimpleColumnType::Integer),
            fill_with: None,
        },
        format!("{} {}.{} {} {}", "Modify column type:".bright_yellow(), "users".bright_cyan(), "id".bright_cyan().bold(), "->".bright_white(), "integer".bright_cyan().bold())
    )]
    #[case(
        MigrationAction::AddConstraint {
            table: "users".into(),
            constraint: vespertide_core::TableConstraint::Index {
                name: Some("idx".into()),
                columns: vec!["id".into()],
            },
        },
        format!("{} {} {} {}", "Add constraint:".bright_green(), "idx INDEX (id)".bright_cyan().bold(), "on".bright_white(), "users".bright_cyan())
    )]
    #[case(
        MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: vespertide_core::TableConstraint::Index {
                name: Some("idx".into()),
                columns: vec!["id".into()],
            },
        },
        format!("{} {} {} {}", "Remove constraint:".bright_red(), "idx INDEX (id)".bright_cyan().bold(), "from".bright_white(), "users".bright_cyan())
    )]
    #[case(
        MigrationAction::RenameTable { from: "users".into(), to: "accounts".into() },
        format!("{} {} {} {}", "Rename table:".bright_yellow(), "users".bright_cyan(), "->".bright_white(), "accounts".bright_cyan().bold())
    )]
    #[case(
        MigrationAction::RawSql { sql: "SELECT 1".into() },
        format!("{} {}", "Execute raw SQL:".bright_yellow(), "SELECT 1".bright_cyan())
    )]
    #[case(
        MigrationAction::AddConstraint {
            table: "users".into(),
            constraint: vespertide_core::TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            },
        },
        format!("{} {} {} {}", "Add constraint:".bright_green(), "PRIMARY KEY (id)".bright_cyan().bold(), "on".bright_white(), "users".bright_cyan())
    )]
    #[case(
        MigrationAction::AddConstraint {
            table: "users".into(),
            constraint: vespertide_core::TableConstraint::Unique {
                name: Some("unique_email".into()),
                columns: vec!["email".into()],
            },
        },
        format!("{} {} {} {}", "Add constraint:".bright_green(), "unique_email UNIQUE (email)".bright_cyan().bold(), "on".bright_white(), "users".bright_cyan())
    )]
    #[case(
        MigrationAction::AddConstraint {
            table: "posts".into(),
            constraint: vespertide_core::TableConstraint::ForeignKey {
                name: Some("fk_user".into()),
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            },
        },
        format!("{} {} {} {}", "Add constraint:".bright_green(), "fk_user FK (user_id) -> users".bright_cyan().bold(), "on".bright_white(), "posts".bright_cyan())
    )]
    #[case(
        MigrationAction::AddConstraint {
            table: "users".into(),
            constraint: vespertide_core::TableConstraint::Check {
                name: "check_age".into(),
                expr: "age > 0".into(),
            },
        },
        format!("{} {} {} {}", "Add constraint:".bright_green(), "check_age CHECK (age > 0)".bright_cyan().bold(), "on".bright_white(), "users".bright_cyan())
    )]
    #[case(
        MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: vespertide_core::TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            },
        },
        format!("{} {} {} {}", "Remove constraint:".bright_red(), "PRIMARY KEY (id)".bright_cyan().bold(), "from".bright_white(), "users".bright_cyan())
    )]
    #[case(
        MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: vespertide_core::TableConstraint::Unique {
                name: None,
                columns: vec!["email".into()],
            },
        },
        format!("{} {} {} {}", "Remove constraint:".bright_red(), "UNIQUE (email)".bright_cyan().bold(), "from".bright_white(), "users".bright_cyan())
    )]
    #[case(
        MigrationAction::RemoveConstraint {
            table: "posts".into(),
            constraint: vespertide_core::TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            },
        },
        format!("{} {} {} {}", "Remove constraint:".bright_red(), "FK (user_id) -> users".bright_cyan().bold(), "from".bright_white(), "posts".bright_cyan())
    )]
    #[case(
        MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: vespertide_core::TableConstraint::Check {
                name: "check_age".into(),
                expr: "age > 0".into(),
            },
        },
        format!(
            "{} {} {} {}",
            "Remove constraint:".bright_red(),
            "check_age CHECK (age > 0)".bright_cyan().bold(),
            "from".bright_white(),
            "users".bright_cyan()
        )
    )]
    #[case(
        MigrationAction::ModifyColumnNullable {
            table: "users".into(),
            column: "email".into(),
            nullable: false,
            fill_with: None,
            delete_null_rows: None,
        },
        format!(
            "{} {}.{} {} {}",
            "Modify column nullability:".bright_yellow(),
            "users".bright_cyan(),
            "email".bright_cyan().bold(),
            "->".bright_white(),
            "NOT NULL".bright_cyan().bold()
        )
    )]
    #[case(
        MigrationAction::ModifyColumnNullable {
            table: "users".into(),
            column: "email".into(),
            nullable: true,
            fill_with: None,
            delete_null_rows: None,
        },
        format!(
            "{} {}.{} {} {}",
            "Modify column nullability:".bright_yellow(),
            "users".bright_cyan(),
            "email".bright_cyan().bold(),
            "->".bright_white(),
            "NULL".bright_cyan().bold()
        )
    )]
    #[case(
        MigrationAction::ModifyColumnDefault {
            table: "users".into(),
            column: "status".into(),
            new_default: Some("'active'".into()),
        },
        format!(
            "{} {}.{} {} {}",
            "Modify column default:".bright_yellow(),
            "users".bright_cyan(),
            "status".bright_cyan().bold(),
            "->".bright_white(),
            "'active'".bright_cyan().bold()
        )
    )]
    #[case(
        MigrationAction::ModifyColumnDefault {
            table: "users".into(),
            column: "status".into(),
            new_default: None,
        },
        format!(
            "{} {}.{} {} {}",
            "Modify column default:".bright_yellow(),
            "users".bright_cyan(),
            "status".bright_cyan().bold(),
            "->".bright_white(),
            "(none)".bright_cyan().bold()
        )
    )]
    #[case(
        MigrationAction::ModifyColumnComment {
            table: "users".into(),
            column: "email".into(),
            new_comment: Some("User email address".into()),
        },
        format!(
            "{} {}.{} {} '{}'",
            "Modify column comment:".bright_yellow(),
            "users".bright_cyan(),
            "email".bright_cyan().bold(),
            "->".bright_white(),
            "User email address".bright_cyan().bold()
        )
    )]
    #[case(
        MigrationAction::ModifyColumnComment {
            table: "users".into(),
            column: "email".into(),
            new_comment: None,
        },
        format!(
            "{} {}.{} {} '{}'",
            "Modify column comment:".bright_yellow(),
            "users".bright_cyan(),
            "email".bright_cyan().bold(),
            "->".bright_white(),
            "(none)".bright_cyan().bold()
        )
    )]
    #[case(
        MigrationAction::ModifyColumnComment {
            table: "users".into(),
            column: "email".into(),
            new_comment: Some("This is a very long comment that exceeds thirty characters and should be truncated".into()),
        },
        format!(
            "{} {}.{} {} '{}'",
            "Modify column comment:".bright_yellow(),
            "users".bright_cyan(),
            "email".bright_cyan().bold(),
            "->".bright_white(),
            "This is a very long comment...".bright_cyan().bold()
        )
    )]
    #[case(
        MigrationAction::ReplaceConstraint {
            table: "posts".into(),
            from: vespertide_core::TableConstraint::ForeignKey {
                name: Some("fk_user".into()),
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            },
            to: vespertide_core::TableConstraint::ForeignKey {
                name: Some("fk_user".into()),
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some(ReferenceAction::Cascade),
                on_update: None,
            },
        },
        format!("{} {} {} {} {} {}", "Replace constraint:".bright_yellow(), "fk_user FK (user_id) -> users".bright_cyan().bold(), "->".bright_white(), "fk_user FK (user_id) -> users".bright_cyan().bold(), "on".bright_white(), "posts".bright_cyan())
    )]
    #[serial]
    fn format_action_cases(#[case] action: MigrationAction, #[case] expected: String) {
        assert_eq!(format_action(&action), expected);
    }

    #[rstest]
    #[serial]
    #[tokio::test]
    async fn cmd_diff_with_model_and_no_migrations() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        write_config();
        write_model("users");
        fs::create_dir_all("migrations").unwrap();

        let result = cmd_diff().await;
        assert!(result.is_ok());
    }

    #[rstest]
    #[serial]
    #[tokio::test]
    async fn cmd_diff_when_no_changes() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        write_config();
        // No models, no migrations -> planner should report no actions.
        fs::create_dir_all("models").unwrap();
        fs::create_dir_all("migrations").unwrap();

        let result = cmd_diff().await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_constraint_display_unnamed_index() {
        let constraint = TableConstraint::Index {
            name: None,
            columns: vec!["email".into(), "username".into()],
        };
        let display = format_constraint_type(&constraint);
        assert_eq!(display, "INDEX (email, username)");
    }

    #[test]
    fn test_constraint_display_named_index() {
        let constraint = TableConstraint::Index {
            name: Some("ix_users_email".into()),
            columns: vec!["email".into()],
        };
        let display = format_constraint_type(&constraint);
        assert_eq!(display, "ix_users_email INDEX (email)");
    }

    #[test]
    fn format_missing_fk_warning_named_fk_produces_4_lines() {
        let m = MissingFkSupportingIndex {
            table: "orders".to_string(),
            constraint_name: Some("fk_orders__user".to_string()),
            columns: vec!["user_id".to_string()],
            ref_table: "users".to_string(),
            ref_columns: vec!["id".to_string()],
            suggested_index_name: "ix_orders__user_id".to_string(),
        };
        let out = format_missing_fk_warning(&m);

        assert_eq!(out.lines().count(), 4, "4 indented lines: fk / ref / why / fix");
        // The four labels must each appear exactly once.
        for label in ["fk:", "ref:", "why:", "fix:"] {
            assert_eq!(
                out.matches(label).count(),
                1,
                "label `{label}` should appear exactly once in:\n{out}"
            );
        }
        // The user-facing identifiers must surface unescaped.
        assert!(out.contains("fk_orders__user"));
        assert!(out.contains("orders(user_id)"));
        assert!(out.contains("users(id)"));
        assert!(out.contains("ix_orders__user_id"));
    }

    #[test]
    fn format_missing_fk_warning_unnamed_fk_falls_back_to_placeholder() {
        let m = MissingFkSupportingIndex {
            table: "orders".to_string(),
            constraint_name: None,
            columns: vec!["user_id".to_string()],
            ref_table: "users".to_string(),
            ref_columns: vec!["id".to_string()],
            suggested_index_name: "ix_orders__user_id".to_string(),
        };
        let out = format_missing_fk_warning(&m);
        assert!(out.contains("(unnamed)"));
        assert!(out.contains("ix_orders__user_id"));
    }

    #[test]
    fn format_missing_fk_warning_composite_fk_lists_all_columns() {
        let m = MissingFkSupportingIndex {
            table: "audit".to_string(),
            constraint_name: Some("fk_audit__tenant_user".to_string()),
            columns: vec!["tenant_id".to_string(), "user_id".to_string()],
            ref_table: "membership".to_string(),
            ref_columns: vec!["tenant_id".to_string(), "user_id".to_string()],
            suggested_index_name: "ix_audit__tenant_id_user_id".to_string(),
        };
        let out = format_missing_fk_warning(&m);
        assert!(out.contains("audit(tenant_id, user_id)"));
        assert!(out.contains("membership(tenant_id, user_id)"));
        assert!(out.contains("ix_audit__tenant_id_user_id"));
    }

    // -----------------------------------------------------------------------
    // F50: constraint-drop warnings
    // -----------------------------------------------------------------------

    fn drop_warning(
        kind: vespertide_core::ConstraintKind,
        label: &str,
        table: &str,
        columns: Vec<&str>,
    ) -> ConstraintDropWarning {
        ConstraintDropWarning {
            action_index: 0,
            table: table.to_string(),
            kind,
            label: label.to_string(),
            columns: columns.into_iter().map(ToString::to_string).collect(),
        }
    }

    #[test]
    fn format_constraint_drop_warning_primary_key_produces_4_lines() {
        let w = drop_warning(
            vespertide_core::ConstraintKind::PrimaryKey,
            "PRIMARY KEY (id)",
            "users",
            vec!["id"],
        );
        let out = format_constraint_drop_warning(&w);

        assert_eq!(
            out.lines().count(),
            4,
            "4 indented lines: on / drop / why / fix"
        );
        for label in ["on:", "drop:", "why:", "fix:"] {
            assert_eq!(
                out.matches(label).count(),
                1,
                "label `{label}` should appear exactly once in:\n{out}"
            );
        }
        assert!(out.contains("users"));
        assert!(out.contains("PRIMARY KEY"));
        assert!(out.contains("PRIMARY KEY (id)"));
    }

    #[test]
    fn format_constraint_drop_warning_unique_uses_unique_kind_label() {
        let w = drop_warning(
            vespertide_core::ConstraintKind::Unique,
            "uq_users__email UNIQUE (email)",
            "users",
            vec!["email"],
        );
        let out = format_constraint_drop_warning(&w);
        assert!(out.contains("UNIQUE"));
        assert!(out.contains("uq_users__email"));
    }

    #[test]
    fn format_constraint_drop_warning_foreign_key_uses_fk_kind_label() {
        let w = drop_warning(
            vespertide_core::ConstraintKind::ForeignKey,
            "fk_orders__user FK (user_id) -> users",
            "orders",
            vec!["user_id"],
        );
        let out = format_constraint_drop_warning(&w);
        assert!(out.contains("FOREIGN KEY"));
        assert!(out.contains("fk_orders__user"));
        assert!(out.contains("-> users"));
    }

    #[test]
    fn format_constraint_drop_warning_check_uses_check_kind_label() {
        let w = drop_warning(
            vespertide_core::ConstraintKind::Check,
            "chk_positive_total CHECK (total > 0)",
            "orders",
            vec![],
        );
        let out = format_constraint_drop_warning(&w);
        assert!(out.contains("CHECK"));
        assert!(out.contains("total > 0"));
    }
}
