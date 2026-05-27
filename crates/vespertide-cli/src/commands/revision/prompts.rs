use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{Confirm, Input, Select};
use vespertide_core::{MigrationAction, MigrationPlan, NarrowingStrategy, TableDef};
#[cfg(test)]
use vespertide_planner::find_missing_fill_with;
use vespertide_planner::{
    EnumFillWithRequired, FillWithRequired, FkPolicyChangeWarning, NarrowingKind,
    TimezoneConversionWarning, TypeNarrowingWarning, find_missing_enum_fill_with,
    render_reference_action,
};

use super::timezones::{KNOWN_IANA, validate_timezone};

#[cfg(test)]
use super::emit::apply_fill_with_to_plan;
use super::emit::{RecreateReason, RecreateTableRequired, apply_enum_fill_with_to_plan};

/// Format the type info string for display.
/// Includes column type and default value hint if available.
pub(super) fn format_type_info(column_type: &str, default_value: &str) -> String {
    format!(" ({column_type}, default: {default_value})")
}

/// Format a single `fill_with` item for display.
pub(super) fn format_fill_with_item(
    table: &str,
    column: &str,
    type_info: &str,
    action_type: &str,
) -> String {
    format!(
        "  {} {}.{}{}\n    {} {}",
        "•".bright_cyan(),
        table.bright_white(),
        column.bright_green(),
        type_info.bright_black(),
        "Action:".bright_black(),
        action_type.bright_magenta()
    )
}

/// Format the prompt string for interactive input.
pub(super) fn format_fill_with_prompt(table: &str, column: &str) -> String {
    format!(
        "  Enter fill value for {}.{}",
        table.bright_white(),
        column.bright_green()
    )
}

/// Print the header for `fill_with` prompts.
pub(super) fn print_fill_with_header() {
    println!(
        "\n{} {}",
        "⚠".bright_yellow(),
        "The following columns require fill_with values:".bright_yellow()
    );
    println!("{}", "─".repeat(60).bright_black());
}

/// Print the footer for `fill_with` prompts.
pub(super) fn print_fill_with_footer() {
    println!("{}", "─".repeat(60).bright_black());
}

/// Print a `fill_with` item and return the formatted prompt.
pub(super) fn print_fill_with_item_and_get_prompt(
    table: &str,
    column: &str,
    column_type: &str,
    default_value: &str,
    action_type: &str,
) -> String {
    let type_info = format_type_info(column_type, default_value);
    let item_display = format_fill_with_item(table, column, &type_info, action_type);
    println!("{item_display}");
    format_fill_with_prompt(table, column)
}

/// Wrap a value with single quotes if it contains spaces and isn't already quoted.
pub(super) fn wrap_if_spaces(value: String) -> String {
    if value.is_empty() {
        return value;
    }
    // Already wrapped with single quotes
    if value.starts_with('\'') && value.ends_with('\'') {
        return value;
    }
    // Contains spaces: wrap with single quotes
    if value.contains(' ') {
        return format!("'{value}'");
    }
    value
}

/// Prompt the user for a `fill_with` value using dialoguer.
/// This function wraps terminal I/O and cannot be unit tested without a real terminal.
#[cfg(not(tarpaulin_include))]
pub(super) fn prompt_fill_with_value(prompt: &str, default: &str) -> Result<String> {
    let value: String = Input::new()
        .with_prompt(prompt)
        .default(default.to_string())
        .interact_text()
        .context("failed to read input")?;
    Ok(wrap_if_spaces(value))
}

/// Prompt the user to select an enum value using dialoguer Select.
/// Returns the selected value wrapped in single quotes for SQL.
#[cfg(not(tarpaulin_include))]
pub(super) fn prompt_enum_value(prompt: &str, enum_values: &[String]) -> Result<String> {
    let selection = Select::new()
        .with_prompt(prompt)
        .items(enum_values)
        .default(0)
        .interact()
        .context("failed to read selection")?;
    // Return the selected value with single quotes for SQL enum literal
    Ok(format!("'{}'", enum_values[selection]))
}

/// Prompt for enum value selection and return bare (unquoted) value.
/// Used by `cmd_revision` for enum `fill_with` collection where `BTreeMap` stores bare names.
#[cfg(not(tarpaulin_include))]
pub(super) fn prompt_enum_value_bare(prompt: &str, values: &[String]) -> Result<String> {
    let selected = prompt_enum_value(prompt, values)?;
    Ok(strip_enum_quotes(&selected))
}

/// Strip SQL single-quotes from an enum value string.
/// `BTreeMap` stores bare enum names; the SQL layer handles quoting via `Expr::val()`.
pub(super) fn strip_enum_quotes(value: &str) -> String {
    value
        .trim_start_matches('\'')
        .trim_end_matches('\'')
        .to_string()
}

/// Collect `fill_with` values interactively for missing columns.
/// The `prompt_fn` parameter allows injecting a mock for testing.
/// The `enum_prompt_fn` parameter handles enum type columns with selection UI.
pub(super) fn collect_fill_with_values<F, E>(
    missing: &[vespertide_planner::FillWithRequired],
    fill_values: &mut HashMap<(String, String), String>,
    prompt_fn: F,
    enum_prompt_fn: E,
) -> Result<()>
where
    F: Fn(&str, &str) -> Result<String>,
    E: Fn(&str, &[String]) -> Result<String>,
{
    print_fill_with_header();

    for item in missing {
        let prompt = print_fill_with_item_and_get_prompt(
            &item.table,
            &item.column,
            &item.column_type,
            &item.default_value,
            item.action_type,
        );

        let value = if let Some(enum_values) = &item.enum_values {
            // Use selection UI for enum types
            enum_prompt_fn(&prompt, enum_values)?
        } else {
            // Use text input with default pre-filled
            prompt_fn(&prompt, &item.default_value)?
        };
        fill_values.insert((item.table.clone(), item.column.clone()), value);
    }

    print_fill_with_footer();
    Ok(())
}
/// Handle interactive `fill_with` collection if there are missing values.
/// Returns the updated `fill_values` map after collecting from user.
#[cfg(test)]
pub(super) fn handle_missing_fill_with<F, E>(
    plan: &mut MigrationPlan,
    fill_values: &mut HashMap<(String, String), String>,
    current_schema: &[TableDef],
    prompt_fn: F,
    enum_prompt_fn: E,
) -> Result<()>
where
    F: Fn(&str, &str) -> Result<String>,
    E: Fn(&str, &[String]) -> Result<String>,
{
    let missing = find_missing_fill_with(plan, current_schema);

    if !missing.is_empty() {
        collect_fill_with_values(&missing, fill_values, prompt_fn, enum_prompt_fn)?;

        // Apply the collected fill_with values
        apply_fill_with_to_plan(plan, fill_values);
    }

    Ok(())
}

#[cfg(not(tarpaulin_include))]
pub(super) fn prompt_delete_null_rows(table: &str, column: &str) -> Result<bool> {
    let confirmed = Confirm::new()
        .with_prompt(format!("  Delete rows where {table}.{column} IS NULL?"))
        .default(false)
        .interact()
        .context("failed to read confirmation")?;
    Ok(confirmed)
}

pub(super) fn handle_delete_null_rows<F>(
    plan: &mut MigrationPlan,
    missing: &mut Vec<FillWithRequired>,
    delete_set: &HashSet<(String, String)>,
    prompt_fn: F,
) -> Result<()>
where
    F: Fn(&str, &str) -> Result<bool>,
{
    let mut to_delete = Vec::new();
    let mut remaining = Vec::new();

    for item in missing.drain(..) {
        if item.has_foreign_key && !delete_set.contains(&(item.table.clone(), item.column.clone()))
        {
            // FK column without CLI arg — prompt user
            println!(
                "  {} {}.{} has a foreign key constraint — fill_with may not work.",
                "\u{2022}".bright_cyan(),
                item.table.bright_white(),
                item.column.bright_green()
            );
            if prompt_fn(&item.table, &item.column)? {
                to_delete.push((item.table.clone(), item.column.clone()));
            } else {
                remaining.push(item);
            }
        } else if delete_set.contains(&(item.table.clone(), item.column.clone())) {
            to_delete.push((item.table.clone(), item.column.clone()));
        } else {
            remaining.push(item);
        }
    }

    // Apply delete_null_rows to plan
    for (table, column) in &to_delete {
        for action in &mut plan.actions {
            if let MigrationAction::ModifyColumnNullable {
                table: t,
                column: c,
                delete_null_rows,
                ..
            } = action
                && t == table
                && c == column
            {
                *delete_null_rows = Some(true);
            }
        }
    }

    *missing = remaining;
    Ok(())
}
/// Collect enum `fill_with` values interactively for removed enum values.
/// The `enum_prompt_fn` parameter handles enum type columns with selection UI.
pub(super) fn collect_enum_fill_with_values<E>(
    missing: &[EnumFillWithRequired],
    enum_prompt_fn: E,
) -> Result<Vec<(usize, BTreeMap<String, String>)>>
where
    E: Fn(&str, &[String]) -> Result<String>,
{
    let mut results = Vec::new();

    println!(
        "\n{} {}",
        "\u{26a0}".bright_yellow(),
        "The following enum value removals require replacement mappings:".bright_yellow()
    );
    println!("{}", "\u{2500}".repeat(60).bright_black());

    for item in missing {
        println!(
            "  {} {}.{}: removing enum values",
            "\u{2022}".bright_cyan(),
            item.table.bright_white(),
            item.column.bright_green()
        );

        let mut mappings = BTreeMap::new();
        for removed in &item.removed_values {
            let prompt = format!(
                "  Replace '{}' in {}.{} with",
                removed.bright_red(),
                item.table.bright_white(),
                item.column.bright_green()
            );
            let value = enum_prompt_fn(&prompt, &item.remaining_values)?;
            mappings.insert(removed.clone(), value);
        }
        results.push((item.action_index, mappings));
    }

    println!("{}", "\u{2500}".repeat(60).bright_black());
    Ok(results)
}

/// Handle interactive enum `fill_with` collection if there are missing values.
pub(super) fn handle_missing_enum_fill_with<E>(
    plan: &mut MigrationPlan,
    current_schema: &[TableDef],
    enum_prompt_fn: E,
) -> Result<()>
where
    E: Fn(&str, &[String]) -> Result<String>,
{
    let missing = find_missing_enum_fill_with(plan, current_schema);

    if !missing.is_empty() {
        let collected = collect_enum_fill_with_values(&missing, enum_prompt_fn)?;
        apply_enum_fill_with_to_plan(plan, &collected);
    }

    Ok(())
}
/// Render a one-line summary of a single FK policy change. The result is
/// shared between the interactive prompt and the unit tests so the wording
/// can be locked in without going through stdout.
pub(super) fn format_fk_policy_change_line(w: &FkPolicyChangeWarning) -> String {
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

/// Strategies that can be safely emitted by the SQL generator for a given
/// narrowing kind. Drives the dialoguer `Select` UI so the user only ever
/// sees applicable options.
///
/// Returning an empty slice means *no automatic strategy exists* — the
/// caller must abort the revision and ask the user to pre-clean the data
/// manually (Phase 3 SQL generation returns `UnsupportedAction` for these).
pub(super) fn applicable_strategies(kind: &NarrowingKind) -> &'static [&'static str] {
    match kind {
        NarrowingKind::VarcharLength { .. }
        | NarrowingKind::CharLength { .. }
        | NarrowingKind::VarcharToCharShorter { .. }
        | NarrowingKind::CharToVarcharShorter { .. }
        | NarrowingKind::TextToVarchar { .. }
        | NarrowingKind::TextToChar { .. }
        | NarrowingKind::NumericScale { .. } => &["truncate", "delete", "set_to_value"],
        NarrowingKind::NumericIntegerDigits { .. } | NarrowingKind::IntegerSize { .. } => {
            &["delete", "set_to_value"]
        }
        NarrowingKind::FloatSize { .. } | NarrowingKind::TimestamptzToTimestamp => &[],
    }
}

/// Whether the new type is string-shaped (`set_to_value` input should be
/// auto-quoted with single quotes when the user types a bare literal).
fn is_string_target(kind: &NarrowingKind) -> bool {
    matches!(
        kind,
        NarrowingKind::VarcharLength { .. }
            | NarrowingKind::CharLength { .. }
            | NarrowingKind::VarcharToCharShorter { .. }
            | NarrowingKind::CharToVarcharShorter { .. }
            | NarrowingKind::TextToVarchar { .. }
            | NarrowingKind::TextToChar { .. }
    )
}

/// Print the multi-line strategy explainer block. Shared between Select UI
/// and unit tests so wording is canonical.
fn print_strategy_descriptions(applicable: &[&'static str]) {
    let header = "  Choose how to handle existing rows that would violate the new type:";
    println!("{}", header.bright_white());
    println!();
    for option in applicable {
        match *option {
            "truncate" => println!(
                "    {} - Trim violating values to fit the new size ({}).\n      \
                 Row preserved; tail content lost.",
                "truncate".bright_cyan().bold(),
                "LEFT(col, N) / ROUND(col, scale)".bright_black()
            ),
            "delete" => println!(
                "    {} - Delete entire rows whose value violates.\n      \
                 ⚠ Other columns of those rows are lost. Watch FK CASCADE.",
                "delete".bright_cyan().bold()
            ),
            "set_to_value" => println!(
                "    {} - Replace violating values with a sentinel you provide.\n      \
                 Rows preserved; you will be asked for the value next.",
                "set_to_value".bright_cyan().bold()
            ),
            _ => {}
        }
    }
    println!();
}

/// Prompt the user to pick a [`NarrowingStrategy`] for every type
/// narrowing queued in the current migration plan. Replaces the Phase 1
/// strong-confirm with a per-narrowing `Select` UI driven by
/// [`applicable_strategies`].
///
/// Returns `Ok(Some(strategies))` with one strategy per warning (in the
/// input order) on successful completion. Returns `Ok(None)` when:
///   * any narrowing kind has no automatic strategy (caller aborts revision);
///   * the user explicitly declines via the trailing confirm.
#[cfg(not(tarpaulin_include))]
pub(super) fn prompt_type_narrowings(
    warnings: &[TypeNarrowingWarning],
) -> Result<Option<Vec<NarrowingStrategy>>> {
    println!(
        "\n{} {}",
        "\u{26a0}".bright_yellow(),
        format!(
            "{} type narrowing(s) detected — each requires a strategy:",
            warnings.len()
        )
        .bright_yellow()
    );

    let mut strategies = Vec::with_capacity(warnings.len());
    for (idx, w) in warnings.iter().enumerate() {
        println!("{}", "\u{2500}".repeat(60).bright_black());
        println!(
            "  {} {}/{}: {} ({} {} {})",
            "\u{25b6}".bright_cyan(),
            idx + 1,
            warnings.len(),
            format!("{}.{}", w.table, w.column).bright_white().bold(),
            w.from_display.bright_red(),
            "->".bright_white(),
            w.to_display.bright_yellow().bold(),
        );
        println!(
            "    postgres: {}\n    mysql:    {}\n    sqlite:   {}",
            w.kind.postgres_impact().bright_black(),
            w.kind.mysql_impact().bright_black(),
            w.kind.sqlite_impact().bright_black(),
        );
        println!();

        let applicable = applicable_strategies(&w.kind);
        if applicable.is_empty() {
            println!(
                "  {} {}",
                "\u{26a0}".bright_red(),
                "No automatic strategy is available for this narrowing kind. \
                 You must pre-clean the data manually before retrying."
                    .bright_red()
            );
            return Ok(None);
        }

        print_strategy_descriptions(applicable);

        let selection = Select::new()
            .with_prompt("  Select strategy")
            .items(applicable)
            .default(0)
            .interact()
            .context("failed to read selection")?;
        let chosen = applicable[selection];
        let strategy = match chosen {
            "truncate" => NarrowingStrategy::Truncate,
            "delete" => NarrowingStrategy::Delete,
            "set_to_value" => {
                let raw: String = Input::new()
                    .with_prompt(format!(
                        "    Replacement value for {}.{} (must fit {})",
                        w.table, w.column, w.to_display
                    ))
                    .interact_text()
                    .context("failed to read replacement value")?;
                NarrowingStrategy::SetToValue {
                    value: quote_value_for_target(&raw, &w.kind),
                }
            }
            _ => unreachable!("applicable_strategies returns only the three known labels"),
        };
        strategies.push(strategy);
    }
    println!("{}", "\u{2500}".repeat(60).bright_black());
    Ok(Some(strategies))
}

/// Wrap a raw `set_to_value` input in single quotes when the new column
/// type is string-shaped, leave numeric/boolean literals as-is. Mirrors
/// the existing `wrap_if_spaces` helper used by `fill_with` collection so
/// users do not have to remember the SQL quoting rules.
fn quote_value_for_target(raw: &str, kind: &NarrowingKind) -> String {
    if !is_string_target(kind) {
        return raw.to_string();
    }
    if raw.starts_with('\'') && raw.ends_with('\'') {
        return raw.to_string();
    }
    format!("'{}'", raw.replace('\'', "''"))
}

/// Apply user-selected strategies onto the plan in place. Each warning's
/// `action_index` points at the `ModifyColumnType` action it came from.
///
/// Exposed via `pub(super)` so the integration test mocks can call it after
/// stubbing the prompt.
pub(super) fn apply_narrowing_strategies_to_plan(
    plan: &mut MigrationPlan,
    warnings: &[TypeNarrowingWarning],
    strategies: &[NarrowingStrategy],
) {
    for (warning, strategy) in warnings.iter().zip(strategies) {
        if let Some(MigrationAction::ModifyColumnType {
            narrowing_strategy, ..
        }) = plan.actions.get_mut(warning.action_index)
        {
            *narrowing_strategy = Some(strategy.clone());
        }
    }
}

// ---------------------------------------------------------------------------
// F20 — timezone conversion prompt
// ---------------------------------------------------------------------------

/// Sentinel labels appended after the IANA whitelist in the Select UI.
const CUSTOM_IANA_LABEL: &str = "Custom IANA name (validated against whitelist)";
const CUSTOM_OFFSET_LABEL: &str = "Custom UTC offset (±HH:MM)";

/// Prompt the user to pick a timezone for every `timestamp ⇄ timestamptz`
/// conversion queued in the current migration plan.
///
/// Returns `Ok(Some(choices))` with one timezone string per warning (in the
/// input order) on successful completion. Returns `Ok(None)` when the user
/// explicitly declines via the trailing Confirm or the validation loop fails
/// repeatedly (after 3 attempts).
#[cfg(not(tarpaulin_include))]
pub(super) fn prompt_timezone_conversions(
    warnings: &[TimezoneConversionWarning],
) -> Result<Option<Vec<String>>> {
    println!(
        "\n{} {}",
        "\u{26a0}".bright_yellow(),
        format!(
            "{} timestamp \u{21c4} timestamptz conversion(s) detected \
             \u{2014} a timezone is required for safe migration:",
            warnings.len()
        )
        .bright_yellow()
    );

    // Build the Select item list once: 30 IANA entries plus 2 custom slots.
    let mut items: Vec<String> = KNOWN_IANA.iter().map(|s| (*s).to_string()).collect();
    items.push(CUSTOM_IANA_LABEL.to_string());
    items.push(CUSTOM_OFFSET_LABEL.to_string());

    let mut choices = Vec::with_capacity(warnings.len());
    for (idx, w) in warnings.iter().enumerate() {
        println!("{}", "\u{2500}".repeat(60).bright_black());
        println!(
            "  {} {}/{}: {} ({})",
            "\u{25b6}".bright_cyan(),
            idx + 1,
            warnings.len(),
            format!("{}.{}", w.table, w.column).bright_white().bold(),
            w.direction.label().bright_yellow().bold(),
        );
        match w.direction {
            vespertide_planner::TimezoneConversionDirection::NaiveToAware => println!(
                "    {} {}",
                "interpretation:".bright_white(),
                "existing naive values will be read AS IF they are in this timezone."
                    .bright_black()
            ),
            vespertide_planner::TimezoneConversionDirection::AwareToNaive => println!(
                "    {} {}",
                "projection:    ".bright_white(),
                "existing aware values will be projected INTO this timezone, then dropped."
                    .bright_black()
            ),
        }
        if let Some(prev) = &w.current_timezone {
            println!(
                "    {} {} {}",
                "currently:".bright_white(),
                prev.bright_cyan(),
                "(picking again will overwrite this)".bright_black()
            );
        }
        println!();

        let selection = Select::new()
            .with_prompt("  Select timezone")
            .items(&items)
            .default(0)
            .interact()
            .context("failed to read timezone selection")?;

        let tz = if selection < KNOWN_IANA.len() {
            KNOWN_IANA[selection].to_string()
        } else {
            // Custom path: ask for free-text and run validate_timezone with
            // up to 3 retries. After 3 failures the prompt cancels — the user
            // can re-run with `--timezone` later (future flag).
            let label = items[selection].as_str();
            match prompt_custom_timezone_with_retry(label, 3)? {
                Some(custom) => custom,
                None => return Ok(None),
            }
        };
        println!("  {} {}", "selected:".bright_white(), tz.bright_green().bold());
        choices.push(tz);
    }
    println!("{}", "\u{2500}".repeat(60).bright_black());
    Ok(Some(choices))
}

#[cfg(not(tarpaulin_include))]
fn prompt_custom_timezone_with_retry(label: &str, max_attempts: u8) -> Result<Option<String>> {
    for attempt in 1..=max_attempts {
        let raw: String = Input::new()
            .with_prompt(format!("  {label}"))
            .interact_text()
            .context("failed to read custom timezone")?;
        match validate_timezone(&raw) {
            Ok(tz) => return Ok(Some(tz)),
            Err(why) => {
                println!("  {} {}", "\u{2717}".bright_red(), why);
                if attempt < max_attempts {
                    println!(
                        "  {} {} attempts left",
                        "\u{21bb}".bright_yellow(),
                        max_attempts - attempt
                    );
                }
            }
        }
    }
    Ok(None)
}

/// F7-(b) — surface every `RemapEnumValues` action that the planner emit
/// and force the user to acknowledge the *automatic data rewrite*. We do
/// not provide an "edit" option here because the mapping is fully
/// determined by the model diff; the user's only choice is proceed /
/// cancel. Cancelling lets them revisit the model (e.g. revert the value
/// change, or coordinate with downstream consumers first).
#[cfg(not(tarpaulin_include))]
pub(super) fn prompt_remap_enum_values(plan: &MigrationPlan) -> Result<bool> {
    let remaps: Vec<&MigrationAction> = plan
        .actions
        .iter()
        .filter(|a| matches!(a, MigrationAction::RemapEnumValues { .. }))
        .collect();
    if remaps.is_empty() {
        return Ok(true);
    }

    println!(
        "\n{} {}",
        "\u{26a0}".bright_yellow(),
        format!(
            "{} integer enum value remap(s) detected \u{2014} existing rows will be \
             AUTOMATICALLY rewritten by UPDATE ... CASE WHEN:",
            remaps.len()
        )
        .bright_yellow()
    );
    println!("{}", "\u{2500}".repeat(60).bright_black());
    for action in &remaps {
        if let MigrationAction::RemapEnumValues {
            table,
            column,
            mapping,
        } = action
        {
            let summary = mapping
                .iter()
                .map(|(old, new)| format!("{old}\u{2192}{new}"))
                .collect::<Vec<_>>()
                .join(", ");
            println!(
                "  {} {}.{} [{}]",
                "\u{2022}".bright_cyan(),
                table.as_str().bright_white(),
                column.as_str().bright_green(),
                summary.bright_white()
            );
        }
    }
    println!("{}", "\u{2500}".repeat(60).bright_black());
    println!(
        "  {} {}",
        "\u{26a0}".bright_red(),
        "This rewrite runs the moment the migration is applied. \
         Coordinate with all running ORM consumers BEFORE proceeding."
            .bright_red()
    );

    let confirmed = Confirm::new()
        .with_prompt("  I have coordinated downstream consumers. Apply remap?")
        .default(false)
        .interact()
        .context("failed to read confirmation")?;
    Ok(confirmed)
}

/// Apply user-supplied timezones onto the plan in place. Each warning's
/// `action_index` points at the `ModifyColumnType` action it came from.
pub(super) fn apply_timezone_choices_to_plan(
    plan: &mut MigrationPlan,
    warnings: &[TimezoneConversionWarning],
    choices: &[String],
) {
    for (warning, choice) in warnings.iter().zip(choices) {
        if let Some(MigrationAction::ModifyColumnType { timezone, .. }) =
            plan.actions.get_mut(warning.action_index)
        {
            *timezone = Some(choice.clone());
        }
    }
}

/// Prompt the user to confirm all FK referential-action policy changes
/// queued in the current migration plan. Reaches the user as a single
/// batch confirmation, matching the existing `prompt_recreate_tables`
/// pattern: showing every change first, then a single decision point.
///
/// Returns `Ok(true)` when the user confirms, `Ok(false)` when they
/// decline (which the caller turns into a `revision` abort).
#[cfg(not(tarpaulin_include))]
pub(super) fn prompt_fk_policy_changes(warnings: &[FkPolicyChangeWarning]) -> Result<bool> {
    println!(
        "\n{} {}",
        "\u{26a0}".bright_yellow(),
        "The following FK referential-action policies will change \
         — backend behavior will SILENTLY differ:".bright_yellow()
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
pub(super) fn prompt_recreate_tables(tables: &[RecreateTableRequired]) -> Result<bool> {
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
