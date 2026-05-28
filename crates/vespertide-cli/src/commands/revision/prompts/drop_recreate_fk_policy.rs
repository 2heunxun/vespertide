use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{Confirm, Select};
use vespertide_planner::{
    DropChoice, DropResolution, DropTarget, FkPolicyChangeWarning, Match, render_reference_action,
};

use super::super::emit::{RecreateReason, RecreateTableRequired};

/// Render a one-line summary of a single FK policy change. The result is
/// shared between the interactive prompt and the unit tests so the wording
/// can be locked in without going through stdout.
pub(in crate::commands::revision) fn format_fk_policy_change_line(
    w: &FkPolicyChangeWarning,
) -> String {
    let fk_label = w.constraint_name.as_deref().unwrap_or("(unnamed)");
    let from = format!("{}({})", w.table, w.columns.join(", "));
    let to = format!("{}({})", w.ref_table, w.ref_columns.join(", "));
    let mut deltas: Vec<String> = Vec::with_capacity(2);
    if let Some(d) = &w.on_delete_change {
        deltas.push(format!(
            "ON DELETE {} -> {}",
            render_reference_action(d.before.as_ref()),
            render_reference_action(d.after.as_ref()),
        ));
    }
    if let Some(d) = &w.on_update_change {
        deltas.push(format!(
            "ON UPDATE {} -> {}",
            render_reference_action(d.before.as_ref()),
            render_reference_action(d.after.as_ref()),
        ));
    }
    format!("{fk_label} {from} -> {to} :: {}", deltas.join(" / "))
}

/// Prompt the user to confirm all FK referential-action policy changes
/// queued in the current migration plan. Reaches the user as a single
/// batch confirmation, matching the existing `prompt_recreate_tables`
/// pattern: showing every change first, then a single decision point.
///
/// Returns `Ok(true)` when the user confirms, `Ok(false)` when they
/// decline (which the caller turns into a `revision` abort).
#[cfg(not(tarpaulin_include))]
pub(in crate::commands::revision) fn prompt_fk_policy_changes(
    warnings: &[FkPolicyChangeWarning],
) -> Result<bool> {
    println!(
        "\n{} {}",
        "\u{26a0}".bright_yellow(),
        "The following FK referential-action policies will change \
         — backend behavior will SILENTLY differ:"
            .bright_yellow()
    );
    println!("{}", "\u{2500}".repeat(60).bright_black());
    for w in warnings {
        println!(
            "  {} {}",
            "\u{2022}".bright_cyan(),
            format_fk_policy_change_line(w).bright_white()
        );
    }
    println!("{}", "\u{2500}".repeat(60).bright_black());
    println!(
        "  {} {}",
        "\u{26a0}".bright_red(),
        "Review backend code that depends on these policies BEFORE proceeding.".bright_red()
    );

    let confirmed = Confirm::new()
        .with_prompt("  I have reviewed the backend code. Apply policy changes?")
        .default(false)
        .interact()
        .context("failed to read confirmation")?;
    Ok(confirmed)
}

/// Prompt the user to confirm table recreation.
/// Returns true if the user confirms, false otherwise.
#[cfg(not(tarpaulin_include))]
pub(in crate::commands::revision) fn prompt_recreate_tables(
    tables: &[RecreateTableRequired],
) -> Result<bool> {
    println!(
        "\n{} {}",
        "\u{26a0}".bright_yellow(),
        "The following tables need to be RECREATED:".bright_yellow()
    );
    println!("{}", "\u{2500}".repeat(60).bright_black());

    for item in tables {
        let reason_msg = match item.reason {
            RecreateReason::AddColumnWithFk => "adding required FK column",
            RecreateReason::AddFkToExistingColumn => "adding FK to existing required column",
        };
        println!(
            "  {} Table {} \u{2014} {} {}",
            "\u{2022}".bright_cyan(),
            item.table.bright_white(),
            reason_msg,
            item.column.bright_green()
        );
    }

    println!("{}", "\u{2500}".repeat(60).bright_black());
    println!(
        "  {} {}",
        "\u{26a0}".bright_red(),
        "ALL DATA in these tables will be DELETED.".bright_red()
    );

    let confirmed = Confirm::new()
        .with_prompt("  Proceed with table recreation?")
        .default(false)
        .interact()
        .context("failed to read confirmation")?;

    Ok(confirmed)
}

/// Interactive resolution for a single `DropResolution`.
///
/// Renders a `Select` menu listing every rename candidate (sorted by match
/// quality), then `Drop permanently`, then `Cancel migration`. Returns:
/// - `Ok(None)` → user picked Cancel, the whole revision should abort.
/// - `Ok(Some(DropChoice::Drop))` → user accepted the permanent drop.
/// - `Ok(Some(DropChoice::RenameTo(target)))` → user selected a rename target.
///
/// When the user picks `Drop permanently` a second `Confirm` is shown with
/// a backup-recommendation hint (F10 strong confirm); declining the confirm
/// falls back to `Ok(None)` so the user can pick a different option.
pub(in crate::commands::revision) fn prompt_drop_resolution(
    resolution: &DropResolution,
) -> Result<Option<DropChoice>> {
    let header = format_drop_header(&resolution.target);
    println!();
    println!("{}", "\u{2500}".repeat(60).bright_black());
    println!("{header}");
    if !resolution.candidates.is_empty() {
        println!(
            "  {}",
            "Same-plan add actions detected as possible rename targets.".bright_white()
        );
    }
    println!("{}", "\u{2500}".repeat(60).bright_black());

    let mut labels: Vec<String> = Vec::new();
    for c in &resolution.candidates {
        labels.push(format_candidate_label(c));
    }
    let drop_index = labels.len();
    labels.push("Drop permanently (data lost, irreversible)".to_string());
    let cancel_index = labels.len();
    labels.push("Cancel migration".to_string());

    let selection = Select::new()
        .with_prompt("  Pick one")
        .items(&labels)
        .default(0)
        .interact()
        .context("failed to read drop resolution choice")?;

    if selection == cancel_index {
        return Ok(None);
    }
    if selection == drop_index {
        return confirm_permanent_drop(&resolution.target);
    }
    let target = resolution.candidates[selection].target_name.clone();
    Ok(Some(DropChoice::RenameTo(target)))
}

fn format_drop_header(target: &DropTarget) -> String {
    match target {
        DropTarget::Column {
            table,
            column,
            column_type,
        } => format!(
            "  {} Resolve drop: column `{}.{}` ({})",
            "\u{26a0}".bright_yellow(),
            table.bright_white().bold(),
            column.bright_white().bold(),
            column_type
        ),
        DropTarget::Table { name } => format!(
            "  {} Resolve drop: table `{}`",
            "\u{26a0}".bright_yellow(),
            name.bright_white().bold(),
        ),
    }
}

fn format_candidate_label(c: &vespertide_planner::RenameCandidate) -> String {
    let marker = match c.match_quality {
        Match::Exact => "\u{2728} ",
        Match::SameType | Match::Different => "",
    };
    let diff = if c.differences.is_empty() {
        String::new()
    } else {
        format!(" — {}", c.differences.join(", "))
    };
    format!("{marker}Rename \u{2192} {}{}", c.target_name, diff)
}

fn confirm_permanent_drop(target: &DropTarget) -> Result<Option<DropChoice>> {
    println!();
    let (what, backup_hint) = match target {
        DropTarget::Column { table, column, .. } => (
            format!("column `{table}.{column}`"),
            "Recommended: pg_dump / mysqldump the table before applying.".to_string(),
        ),
        DropTarget::Table { name } => (
            format!("table `{name}`"),
            format!(
                "Recommended backup commands before applying:\n     pg_dump -t {name} \u{2026}\n     mysqldump db {name} \u{2026}\n     cp app.db app.db.backup"
            ),
        ),
    };
    println!(
        "  {} About to permanently drop {what}.",
        "\u{26a0}".bright_red()
    );
    println!("  {}", backup_hint.bright_white());

    let confirmed = Confirm::new()
        .with_prompt("  Really drop permanently?")
        .default(false)
        .interact()
        .context("failed to read drop confirmation")?;

    if confirmed {
        Ok(Some(DropChoice::Drop))
    } else {
        Ok(None)
    }
}
