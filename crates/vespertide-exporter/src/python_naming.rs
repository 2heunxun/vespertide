//! Shared naming helpers for the Python-targeted ORM exporters (SQLAlchemy,
//! SQLModel). Both backends share an identical, snake-case-aware
//! `to_pascal_case` and an identical `to_screaming_snake_case` that doubles
//! as a Python `enum.Enum` member-name sanitiser.
//!
//! `seaorm` deliberately keeps its own `to_pascal_case` in
//! `seaorm/imports.rs` — that variant carries reserved-keyword guards and a
//! different allocation pattern and is NOT in scope for this consolidation.

/// Convert snake_case (or single-word) input to PascalCase. Splits on
/// underscores, upper-cases the first character of each segment, and
/// preserves the remainder verbatim.
///
/// Public so the `vespertide-cli` `export` command can reuse the exact same
/// PascalCase semantics for JPA filename derivation without keeping a
/// duplicate private implementation.
pub fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect()
}

/// Convert any input to SCREAMING_SNAKE_CASE: inserts `_` before interior
/// uppercase characters, upper-cases everything, then replaces any
/// non-alphanumeric character with `_` so the result is safe as a Python
/// `enum.Enum` member name.
pub(crate) fn to_screaming_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.push(ch.to_ascii_uppercase());
    }
    // Replace any non-alphanumeric with underscore
    result
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
