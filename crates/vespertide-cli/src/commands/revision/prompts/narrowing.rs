use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{Input, Select};
use vespertide_core::{MigrationPlan, NarrowingStrategy};
use vespertide_planner::{NarrowingKind, TypeNarrowingWarning};

/// Strategies that can be safely emitted by the SQL generator for a given
/// narrowing kind. Drives the dialoguer `Select` UI so the user only ever
/// sees applicable options.
///
/// Returning an empty slice means *no automatic strategy exists* — the
/// caller must abort the revision and ask the user to pre-clean the data
/// manually (Phase 3 SQL generation returns `UnsupportedAction` for these).
pub(in crate::commands::revision) fn applicable_strategies(
    kind: &NarrowingKind,
) -> &'static [&'static str] {
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
pub(in crate::commands::revision) fn prompt_type_narrowings(
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
/// Exposed via `pub(in crate::commands::revision)` so the integration test mocks can call it after
/// stubbing the prompt.
pub(in crate::commands::revision) fn apply_narrowing_strategies_to_plan(
    plan: &mut MigrationPlan,
    warnings: &[TypeNarrowingWarning],
    strategies: &[NarrowingStrategy],
) {
    for (warning, strategy) in warnings.iter().zip(strategies) {
        if let Some(vespertide_core::MigrationAction::ModifyColumnType {
            narrowing_strategy, ..
        }) = plan.actions.get_mut(warning.action_index)
        {
            *narrowing_strategy = Some(strategy.clone());
        }
    }
}
