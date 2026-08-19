//! Helpers shared by exporters targeting different host languages.

/// Strip one matching pair of surrounding quotes from a SQL literal.
///
/// Only an outer pair is removed, so quotes *inside* the literal survive:
/// trimming per character would turn `'say "hi"'` into `say "hi`, silently
/// dropping the closing quote. Input without a matching pair is returned
/// unchanged.
pub(crate) fn unquote(s: &str) -> &str {
    for quote in ['\'', '"'] {
        if let Some(inner) = s
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            return inner;
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::single_quoted("'draft'", "draft")]
    #[case::double_quoted("\"draft\"", "draft")]
    #[case::inner_quotes_survive("'say \"hi\"'", "say \"hi\"")]
    #[case::doubled_sql_escape("'it''s'", "it''s")]
    #[case::unquoted("draft", "draft")]
    #[case::mismatched_pair("\"draft'", "\"draft'")]
    #[case::opening_only("'draft", "'draft")]
    #[case::lone_quote("'", "'")]
    #[case::empty("", "")]
    fn unquote_removes_only_a_matching_outer_pair(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(unquote(input), expected);
    }
}
