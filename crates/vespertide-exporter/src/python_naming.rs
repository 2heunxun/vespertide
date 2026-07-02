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
    let mut result = String::with_capacity(s.len());
    for word in s.split('_') {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            result.extend(first.to_uppercase());
            result.push_str(chars.as_str());
        }
    }
    result
}

/// Convert any input to SCREAMING_SNAKE_CASE: inserts `_` before interior
/// uppercase characters, upper-cases everything, then replaces any
/// non-alphanumeric character with `_` so the result is safe as a Python
/// `enum.Enum` member name.
pub(crate) fn to_screaming_snake_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            result.push('_');
        }
        let upper = ch.to_ascii_uppercase();
        // Sanitise in the same pass: any non-alphanumeric becomes `_` so the
        // result is safe as a Python `enum.Enum` member name.
        if upper.is_alphanumeric() || upper == '_' {
            result.push(upper);
        } else {
            result.push('_');
        }
    }
    result
}
