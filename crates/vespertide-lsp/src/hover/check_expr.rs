//! CHECK-expression hover: hovering inside a `constraints[*].expr`
//! string shows a markdown popup describing the parsed structure.
//!
//! Dispatched **first** in `hover::mod.rs` so a bare column identifier
//! that happens to sit inside a CHECK expression is interpreted as
//! check-expr context, not as a column-declaration object hover.

use crate::text_util::strip_quotes;
use std::fmt::Write as _;
use vespertide_planner::{CheckExprAst, CheckExprLiteral, CheckExprOp, parse_check_expr};

use super::DomainHover;
use crate::check_expr_range::expr_inner_range;

pub(super) fn try_hover(
    node: tree_sitter::Node<'_>,
    source: &str,
    byte_offset: usize,
) -> Option<DomainHover> {
    let pair = expr_pair_ancestor(node, source)?;
    if !is_inside_constraints(pair, source) {
        return None;
    }

    let value = pair.named_child(1)?;
    let inner = expr_inner_range(value)?;
    // The cursor must actually fall inside the expr value (not the key
    // or whitespace before `:`); otherwise let other handlers run.
    if !inner.contains(&byte_offset) && byte_offset != inner.end {
        return None;
    }

    let expr_text = source.get(inner.clone())?;
    let ast = parse_check_expr(expr_text);
    Some(DomainHover {
        markdown: render_markdown(&ast, expr_text),
        byte_range: inner,
    })
}

fn render_markdown(ast: &CheckExprAst, expr_text: &str) -> String {
    let mut md = String::new();
    if matches!(ast, CheckExprAst::Unparseable) {
        let _ = write!(
            md,
            "**CHECK expression** _(could not parse structure)_\n\n`{}`",
            expr_text.trim()
        );
    } else {
        let header = header_for(ast);
        let _ = write!(md, "**{header}**\n\n`{}`", expr_text.trim());
        let bullets = bullets_for(ast);
        if !bullets.is_empty() {
            md.push_str("\n\n");
            for line in bullets {
                let _ = writeln!(md, "- {line}");
            }
        }
    }
    md
}

fn header_for(ast: &CheckExprAst) -> String {
    match ast {
        CheckExprAst::And(parts) => format!("Logical AND of {} conditions", parts.len()),
        CheckExprAst::Or(parts) => format!("Logical OR of {} conditions", parts.len()),
        CheckExprAst::Not(_) => "Logical NOT (negated condition)".to_string(),
        CheckExprAst::Compare { .. } => "Comparison predicate".to_string(),
        CheckExprAst::In { negated, .. } => {
            if *negated {
                "NOT IN list predicate".to_string()
            } else {
                "IN list predicate".to_string()
            }
        }
        CheckExprAst::Between { negated, .. } => {
            if *negated {
                "NOT BETWEEN range predicate".to_string()
            } else {
                "BETWEEN range predicate".to_string()
            }
        }
        CheckExprAst::IsNull { negated, .. } => {
            if *negated {
                "IS NOT NULL predicate".to_string()
            } else {
                "IS NULL predicate".to_string()
            }
        }
        CheckExprAst::Unparseable => "CHECK expression".to_string(),
    }
}

fn bullets_for(ast: &CheckExprAst) -> Vec<String> {
    match ast {
        CheckExprAst::And(parts) | CheckExprAst::Or(parts) => {
            parts.iter().map(render_inline).collect()
        }
        CheckExprAst::Not(inner) => vec![render_inline(inner)],
        CheckExprAst::Compare { column, op, value } => vec![format!(
            "column `{column}` {} {}",
            render_op(*op),
            render_literal(value)
        )],
        CheckExprAst::In {
            column,
            values,
            negated,
        } => {
            let mut lines = vec![format!(
                "column `{column}` {}IN list of {} value{}",
                if *negated { "NOT " } else { "" },
                values.len(),
                if values.len() == 1 { "" } else { "s" }
            )];
            for v in values {
                lines.push(format!("value {}", render_literal(v)));
            }
            lines
        }
        CheckExprAst::Between {
            column,
            low,
            high,
            negated,
        } => vec![format!(
            "column `{column}` {}BETWEEN {} AND {}",
            if *negated { "NOT " } else { "" },
            render_literal(low),
            render_literal(high)
        )],
        CheckExprAst::IsNull { column, negated } => vec![format!(
            "column `{column}` IS {}NULL",
            if *negated { "NOT " } else { "" }
        )],
        CheckExprAst::Unparseable => Vec::new(),
    }
}

fn render_inline(ast: &CheckExprAst) -> String {
    match ast {
        CheckExprAst::Compare { column, op, value } => format!(
            "condition `{column} {} {}`",
            render_op(*op),
            render_literal(value)
        ),
        CheckExprAst::In {
            column,
            values,
            negated,
        } => format!(
            "condition `{column} {}IN ({})`",
            if *negated { "NOT " } else { "" },
            values
                .iter()
                .map(render_literal)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        CheckExprAst::Between {
            column,
            low,
            high,
            negated,
        } => format!(
            "condition `{column} {}BETWEEN {} AND {}`",
            if *negated { "NOT " } else { "" },
            render_literal(low),
            render_literal(high)
        ),
        CheckExprAst::IsNull { column, negated } => format!(
            "condition `{column} IS {}NULL`",
            if *negated { "NOT " } else { "" }
        ),
        CheckExprAst::And(parts) => {
            format!("nested AND of {} conditions", parts.len())
        }
        CheckExprAst::Or(parts) => {
            format!("nested OR of {} conditions", parts.len())
        }
        CheckExprAst::Not(_) => "nested NOT condition".to_string(),
        CheckExprAst::Unparseable => "unparseable sub-expression".to_string(),
    }
}

fn render_op(op: CheckExprOp) -> &'static str {
    match op {
        CheckExprOp::Eq => "=",
        CheckExprOp::Ne => "<>",
        CheckExprOp::Lt => "<",
        CheckExprOp::Le => "<=",
        CheckExprOp::Gt => ">",
        CheckExprOp::Ge => ">=",
    }
}

fn render_literal(lit: &CheckExprLiteral) -> String {
    match lit {
        CheckExprLiteral::Integer(i) => i.to_string(),
        CheckExprLiteral::Float(f) => f.to_string(),
        CheckExprLiteral::String(s) => s.clone(),
        CheckExprLiteral::Bool(b) => {
            if *b {
                "TRUE".to_string()
            } else {
                "FALSE".to_string()
            }
        }
        CheckExprLiteral::Null => "NULL".to_string(),
    }
}

/// Walk up from `node` looking for a pair `"expr": <scalar>` (JSON
/// `pair` or YAML `block_mapping_pair`).
fn expr_pair_ancestor<'tree>(
    node: tree_sitter::Node<'tree>,
    source: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut cur = Some(node);
    while let Some(candidate) = cur {
        if is_pair(candidate)
            && let Some(key) = candidate.named_child(0)
            && source
                .get(key.byte_range())
                .is_some_and(|text| strip_quotes(text) == "expr")
        {
            return Some(candidate);
        }
        cur = candidate.parent();
    }
    None
}

/// True when any ancestor pair has key `"constraints"`. Mirrors
/// `column::is_inside_columns` from the column-hover handler.
fn is_inside_constraints(node: tree_sitter::Node<'_>, source: &str) -> bool {
    let mut cur = node.parent();
    while let Some(candidate) = cur {
        if is_pair(candidate)
            && let Some(key) = candidate.named_child(0)
            && source
                .get(key.byte_range())
                .is_some_and(|text| strip_quotes(text) == "constraints")
        {
            return true;
        }
        cur = candidate.parent();
    }
    false
}

fn is_pair(node: tree_sitter::Node<'_>) -> bool {
    matches!(node.kind(), "pair" | "block_mapping_pair")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DocumentFormat, ParserPool};

    fn hover_at(src: &str, format: DocumentFormat, byte_offset: usize) -> Option<DomainHover> {
        let pool = ParserPool::new();
        let tree = pool.parse(src, format).expect("source should parse");
        let node = tree
            .root_node()
            .descendant_for_byte_range(byte_offset, byte_offset)
            .expect("cursor should resolve to a node");
        try_hover(node, src, byte_offset)
    }

    #[test]
    fn hover_json_and_structure() {
        let src = r#"{"name":"users","columns":[{"name":"age","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"chk_age","expr":"age > 0 AND age < 150"}]}"#;
        let offset = src.find("AND").expect("AND present") + 1;

        let hover = hover_at(src, DocumentFormat::Json, offset)
            .expect("hover inside JSON CHECK expr should return Some");

        assert!(
            hover.markdown.contains("AND") && hover.markdown.contains("age < 150"),
            "markdown should describe AND structure, got: {}",
            hover.markdown
        );
    }

    #[test]
    fn hover_yaml_block_scalar() {
        let src = r"name: users
columns:
  - name: age
    type: integer
    nullable: false
constraints:
  - type: check
    name: chk_age
    expr: |
      age > 0 AND age < 150
";
        let offset = src.find("age > 0").expect("expr present") + 2;

        let hover = hover_at(src, DocumentFormat::Yaml, offset)
            .expect("hover inside YAML block CHECK expr should return Some");

        assert!(
            hover.markdown.contains("age > 0") && hover.markdown.contains("AND"),
            "markdown should reflect YAML block expr, got: {}",
            hover.markdown
        );
    }

    #[test]
    fn hover_cursor_at_expr_end_boundary() {
        let src = r#"{"name":"users","columns":[{"name":"age","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"chk_age","expr":"age > 0"}]}"#;
        let expr_start = src.find("age > 0").expect("expr present");
        let expr_end = expr_start + "age > 0".len();

        assert!(
            hover_at(src, DocumentFormat::Json, expr_end - 1).is_some(),
            "hover should work on the last byte inside the CHECK expr"
        );
        assert!(
            hover_at(src, DocumentFormat::Json, expr_end).is_some(),
            "hover should work at the expr inner.end exclusive boundary"
        );
        assert_eq!(
            hover_at(src, DocumentFormat::Json, expr_end + 1),
            None,
            "hover should not work one byte past the expr boundary"
        );
    }

    #[test]
    fn hover_outside_constraints_returns_none() {
        let src = r#"{"name":"users","columns":[{"name":"age","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"chk_age","expr":"age > 0"}]}"#;
        let offset = src.find("integer").expect("column type present") + 2;

        assert_eq!(hover_at(src, DocumentFormat::Json, offset), None);
    }
}
