use anyhow::Result;
use chrono::Utc;
use colored::Colorize;
use vespertide_core::NarrowingStrategy;
use vespertide_planner::{
    FkPolicyChangeWarning, TypeNarrowingWarning, find_fk_policy_changes, find_missing_fill_with,
    find_type_narrowings, plan_next_migration, schema_from_plans,
};

use crate::utils::{load_config, load_migrations, load_models};

mod emit;
mod parse;
mod prompts;
mod write;

#[cfg(test)]
mod tests;

#[cfg(test)]
use emit::*;
#[cfg(test)]
use parse::*;
#[cfg(test)]
use prompts::*;

use emit::RecreateTableRequired;

pub async fn cmd_revision(
    message: String,
    fill_with_args: Vec<String>,
    delete_null_rows_args: Vec<String>,
) -> Result<()> {
    cmd_revision_core(
        message,
        fill_with_args,
        delete_null_rows_args,
        RevisionPromptFns {
            recreate: prompts::prompt_recreate_tables,
            delete_null_rows: prompts::prompt_delete_null_rows,
            fill_with: prompts::prompt_fill_with_value,
            enum_quoted: prompts::prompt_enum_value,
            enum_bare: prompts::prompt_enum_value_bare,
            fk_policy_change: prompts::prompt_fk_policy_changes,
            type_narrowing: prompts::prompt_type_narrowings,
        },
    )
    .await
}

struct RevisionPromptFns<R, D, F, E, EB, P, N> {
    recreate: R,
    delete_null_rows: D,
    fill_with: F,
    enum_quoted: E,
    enum_bare: EB,
    fk_policy_change: P,
    type_narrowing: N,
}

async fn cmd_revision_core<R, D, F, E, EB, P, N>(
    message: String,
    fill_with_args: Vec<String>,
    delete_null_rows_args: Vec<String>,
    prompt_fns: RevisionPromptFns<R, D, F, E, EB, P, N>,
) -> Result<()>
where
    R: Fn(&[RecreateTableRequired]) -> Result<bool>,
    D: Fn(&str, &str) -> Result<bool>,
    F: Fn(&str, &str) -> Result<String>,
    E: Fn(&str, &[String]) -> Result<String>,
    EB: Fn(&str, &[String]) -> Result<String>,
    P: Fn(&[FkPolicyChangeWarning]) -> Result<bool>,
    N: Fn(&[TypeNarrowingWarning]) -> Result<Option<Vec<NarrowingStrategy>>>,
{
    let RevisionPromptFns {
        recreate: recreate_prompt_fn,
        delete_null_rows: delete_null_rows_prompt_fn,
        fill_with: fill_with_prompt_fn,
        enum_quoted: enum_prompt_fn,
        enum_bare: enum_bare_prompt_fn,
        fk_policy_change: fk_policy_change_prompt_fn,
        type_narrowing: type_narrowing_prompt_fn,
    } = prompt_fns;

    let config = load_config()?;
    let current_models = load_models(&config)?;
    let applied_plans = load_migrations(&config)?;

    let mut plan = plan_next_migration(&current_models, &applied_plans)
        .map_err(|e| anyhow::anyhow!("planning error: {e}"))?;

    // Check for non-nullable FK changes that require table recreation.
    emit::handle_recreate_requirements(&mut plan, &current_models, recreate_prompt_fn)?;

    if plan.actions.is_empty() {
        println!(
            "{} {}",
            "No changes detected.".bright_yellow(),
            "Nothing to migrate.".bright_white()
        );
        return Ok(());
    }

    // Reconstruct baseline schema for column type lookups
    let baseline_schema = schema_from_plans(&applied_plans)
        .map_err(|e| anyhow::anyhow!("schema reconstruction error: {e}"))?;

    // Parse CLI fill_with arguments
    let mut fill_values = parse::parse_fill_with_args(&fill_with_args);
    let delete_set = parse::parse_delete_null_rows_args(&delete_null_rows_args);

    // Apply any CLI-provided fill_with values first
    emit::apply_fill_with_to_plan(&mut plan, &fill_values);
    emit::apply_delete_null_rows_to_plan(&mut plan, &delete_set);

    // Find all missing fill_with values
    let mut missing = find_missing_fill_with(&plan, &baseline_schema);

    // Handle FK columns with delete_null_rows option first
    if !missing.is_empty() {
        prompts::handle_delete_null_rows(
            &mut plan,
            &mut missing,
            &delete_set,
            delete_null_rows_prompt_fn,
        )?;
    }

    // Handle remaining missing fill_with values interactively
    if !missing.is_empty() {
        prompts::collect_fill_with_values(
            &missing,
            &mut fill_values,
            fill_with_prompt_fn,
            enum_prompt_fn,
        )?;
        emit::apply_fill_with_to_plan(&mut plan, &fill_values);
    }

    // Handle any missing enum fill_with values (for removed enum values) interactively
    prompts::handle_missing_enum_fill_with(&mut plan, &baseline_schema, enum_bare_prompt_fn)?;

    // F30 — FK referential-action policy changes silently alter application
    // behavior. Surface them and require explicit double-confirmation before
    // the migration file is written.
    let fk_policy_warnings = find_fk_policy_changes(&plan);
    if !fk_policy_warnings.is_empty() && !fk_policy_change_prompt_fn(&fk_policy_warnings)? {
        println!(
            "{} {}",
            "Cancelled.".bright_yellow().bold(),
            "Review backend code before retrying revision.".bright_white()
        );
        return Ok(());
    }

    // F6/F19/F33/F87 — type narrowings can truncate, reject, or silently
    // corrupt existing rows depending on backend. Surface every narrowing
    // via per-narrowing Select UI; the chosen strategy is stamped onto the
    // plan so the SQL generator can emit safe pre-processing alongside
    // the ALTER. Returns None when the user declines or when the kind has
    // no automatic strategy.
    let narrowing_warnings = find_type_narrowings(&plan, &baseline_schema);
    if !narrowing_warnings.is_empty() {
        let Some(strategies) = type_narrowing_prompt_fn(&narrowing_warnings)? else {
            println!(
                "{} {}",
                "Cancelled.".bright_yellow().bold(),
                "Pre-clean the data manually before retrying revision.".bright_white()
            );
            return Ok(());
        };
        prompts::apply_narrowing_strategies_to_plan(
            &mut plan,
            &narrowing_warnings,
            &strategies,
        );
    }

    plan.id = uuid::Uuid::new_v4().to_string();
    plan.comment = Some(message);
    if plan.created_at.is_none() {
        // Record creation time in RFC3339 (UTC).
        plan.created_at = Some(Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
    }

    let path = write::write_migration_file(&config, &plan).await?;

    println!(
        "{} {}",
        "Created migration:".bright_green().bold(),
        format!("{}", path.display()).bright_white()
    );
    println!(
        "  {} {}",
        "Version:".bright_cyan(),
        plan.version.to_string().bright_magenta().bold()
    );
    println!(
        "  {} {}",
        "Actions:".bright_cyan(),
        plan.actions.len().to_string().bright_yellow()
    );
    if let Some(comment) = &plan.comment {
        println!("  {} {}", "Comment:".bright_cyan(), comment.bright_white());
    }

    Ok(())
}
