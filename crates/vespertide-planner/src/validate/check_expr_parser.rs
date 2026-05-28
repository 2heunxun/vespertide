#![expect(
    clippy::doc_markdown,
    reason = "narrative prose: backend names (PostgreSQL, MySQL, SQLite) appear as plain words intentionally"
)]
//! Shared narrow-shape parser for CHECK constraint expressions.
//!
//! Built originally for fault **F86** (column default violates table
//! CHECK) and now also used by fault **F29** (CHECK expression
//! strengthening — new predicate strictly rejects values the old one
//! accepted). Both faults need to introspect a CHECK expression but
//! must remain backend-neutral: the `expr` field is a verbatim string
//! that PostgreSQL, MySQL and SQLite all accept, so the analyser can
//! only rely on syntax recognised identically by every backend.
//!
//! # Recognised grammar
//!
//! The parser intentionally covers only the dialect-neutral subset of
//! SQL boolean expressions. Anything outside this subset folds to
//! [`CheckExpr::Unparseable`] and downstream analyses must treat it
//! as "can't conclude" — never as a violation. This preserves the
//! F86 invariant of *silent pass on ambiguity* and extends it to
//! F29.
//!
//! Supported productions:
//!
//! ```text
//! expr       = or_expr
//! or_expr    = and_expr ( 'OR'  and_expr )*
//! and_expr   = not_expr ( 'AND' not_expr )*
//! not_expr   = 'NOT' atom | atom
//! atom       = '(' expr ')' | predicate
//! predicate  = column ( comparison | in_list | between | is_null )
//! comparison = ( '<' | '<=' | '>' | '>=' | '=' | '<>' | '!=' ) literal
//! in_list    = [ 'NOT' ] 'IN' '(' literal ( ',' literal )+ ')'
//! between    = [ 'NOT' ] 'BETWEEN' literal 'AND' literal
//! is_null    = 'IS' [ 'NOT' ] 'NULL'
//! literal    = integer | float | quoted-string | TRUE | FALSE | NULL
//! column     = bare-identifier (`[A-Za-z_][A-Za-z0-9_]*`)
//! ```
//!
//! Excluded by design (folds to `Unparseable`):
//!
//! - Function calls (`LOWER(col) = 'x'`) — semantics diverge across
//!   backends (`LENGTH` bytes vs characters, `LOWER` ASCII-only on
//!   SQLite, etc.)
//! - Quoted identifiers (`"col"`, `` `col` ``, `[col]`) — quoting
//!   syntax is dialect-specific
//! - PostgreSQL-only `::` cast syntax — not portable
//! - `LIKE` / `ILIKE` — case-sensitivity diverges per backend
//! - Subqueries — MySQL forbids them in CHECK
//! - Column-to-column comparison (`a > b`) — out of scope for
//!   strictness analysis
//! - `BETWEEN SYMMETRIC` — PG-only
//! - Empty `IN ()` list — only SQLite accepts; rejected at parse time
//!
//! # Conservative design
//!
//! Both F86 and F29 use this parser as a *recogniser*, not an
//! *evaluator*. When ambiguous, the parser returns `Unparseable` and
//! callers skip the analysis entirely. False positives are far worse
//! than false negatives here: rejecting a legitimate schema would
//! block users, while missing an exotic CHECK strengthening merely
//! delegates the failure to the database at apply time (which is
//! what already happens without F29).

/// Parsed shape of a CHECK boolean expression.
#[derive(Debug, Clone, PartialEq)]
pub enum CheckExpr {
    /// `<column> <op> <literal>`.
    Compare {
        column: String,
        op: Op,
        value: Literal,
    },
    /// `<column> [NOT] IN (<lit>, <lit>, ...)` — never empty.
    In {
        column: String,
        values: Vec<Literal>,
        negated: bool,
    },
    /// `<column> [NOT] BETWEEN <low> AND <high>` — always inclusive.
    Between {
        column: String,
        low: Literal,
        high: Literal,
        negated: bool,
    },
    /// `<column> IS [NOT] NULL`.
    IsNull { column: String, negated: bool },
    /// `expr AND expr [AND expr ...]`. Always >= 2 children.
    And(Vec<CheckExpr>),
    /// `expr OR expr [OR expr ...]`. Always >= 2 children.
    Or(Vec<CheckExpr>),
    /// `NOT expr`.
    Not(Box<CheckExpr>),
    /// The parser could not recognise the input. Downstream analyses
    /// must skip silently — *never* report a violation when this
    /// appears anywhere in the tree.
    Unparseable,
}

/// Comparison operator (binary, infix).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// Literal value as written in the CHECK expression. Single-quoted
/// strings are preserved *with* their surrounding quotes so equality
/// with `DefaultValue::String`'s `to_sql()` output works without
/// ad-hoc unquoting (F86 invariant carried over).
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Integer(i64),
    Float(f64),
    /// As-written form including the surrounding `'`, e.g. `'pending'`.
    String(String),
    Bool(bool),
    Null,
}

// Bound recursive grouping so hostile CHECK strings cannot overflow
// planner/LSP stacks; unsupported shapes conservatively become Unparseable.
const MAX_CHECK_EXPR_DEPTH: usize = 64;

/// Semantic category of a CHECK expression lexeme, for editor
/// syntax highlighting. Mirrors the lexer's token classes but
/// collapses them into the categories an LSP semantic-token
/// legend cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckTokenKind {
    /// Bare identifier — a column reference.
    Column,
    /// Reserved word: AND, OR, NOT, IN, BETWEEN, IS, NULL, TRUE, FALSE.
    Keyword,
    /// Comparison operator: `< <= > >= = <> !=`.
    Operator,
    /// Numeric literal (integer or float).
    Number,
    /// Quoted string literal (single-quoted, quotes included in span).
    String,
    /// Structural punctuation: `(`, `)`, `,`.
    Punctuation,
}

/// A lexed CHECK token with its byte span relative to the input
/// `expr` string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckToken {
    pub kind: CheckTokenKind,
    pub span: std::ops::Range<usize>,
}

/// Lex a CHECK expression into semantic tokens with byte spans for
/// editor highlighting. Reuses the same tokenizer the parser uses,
/// so the highlighted lexemes exactly match what `parse` recognises.
///
/// Returns an empty vec when the expression fails to lex (the same
/// inputs that make [`parse`] return [`CheckExpr::Unparseable`]).
/// Spans are byte offsets into `expr`; callers translate to absolute
/// document positions by adding the offset of `expr` within the
/// source file.
#[must_use]
pub fn lex_check_expr(expr: &str) -> Vec<CheckToken> {
    let Some(spanned) = tokenize_spanned(expr) else {
        return Vec::new();
    };
    spanned
        .into_iter()
        .map(|st| CheckToken {
            kind: token_kind(&st.token),
            span: st.span,
        })
        .collect()
}

fn token_kind(token: &Token) -> CheckTokenKind {
    match token {
        Token::Ident(_) => CheckTokenKind::Column,
        Token::Integer(_) | Token::Float(_) => CheckTokenKind::Number,
        Token::QuotedString(_) => CheckTokenKind::String,
        Token::Keyword(_) => CheckTokenKind::Keyword,
        Token::Op(_) => CheckTokenKind::Operator,
        Token::LParen | Token::RParen | Token::Comma => CheckTokenKind::Punctuation,
    }
}

/// Parse a CHECK expression into its recognised shape.
///
/// Returns [`CheckExpr::Unparseable`] for any input outside the
/// supported grammar. Never panics on malformed input — bad shapes
/// fold to `Unparseable`.
#[must_use]
pub fn parse(expr: &str) -> CheckExpr {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return CheckExpr::Unparseable;
    }
    let Some(tokens) = tokenize(trimmed) else {
        return CheckExpr::Unparseable;
    };
    let mut parser = Parser {
        tokens,
        pos: 0,
        depth: 0,
    };
    let parsed = parser.parse_or();
    if !parser.at_end() {
        // Trailing garbage after a complete parse — refuse to
        // mis-classify.
        return CheckExpr::Unparseable;
    }
    parsed
}

// -- Lexer -----------------------------------------------------------------

/// Token kinds the parser distinguishes. `Keyword(_)` is folded
/// case-insensitively at lex time so the parser can match on
/// uppercase form only.
#[derive(Debug, Clone, PartialEq)]
enum Token {
    Ident(String),
    Integer(i64),
    Float(f64),
    /// Quoted string, *including* the surrounding `'`. The lexer
    /// preserves them so [`Literal::String`] equality semantics
    /// match F86.
    QuotedString(String),
    Keyword(Keyword),
    Op(Op),
    LParen,
    RParen,
    Comma,
}

/// A `Token` paired with its byte span in the lexer input.
#[derive(Debug, Clone, PartialEq)]
struct SpannedToken {
    token: Token,
    span: std::ops::Range<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Keyword {
    And,
    Or,
    Not,
    In,
    Between,
    Is,
    Null,
    True,
    False,
}

/// Tokenise the entire expression. Returns `None` on a hard lexer
/// failure (unterminated string, illegal character) so the caller
/// folds to `Unparseable`.
#[expect(
    clippy::too_many_lines,
    reason = "single linear lexer dispatch over char classes; splitting into per-class helpers would scatter the index-arithmetic state without clarifying the flow"
)]
fn tokenize_spanned(input: &str) -> Option<Vec<SpannedToken>> {
    let bytes = input.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        match b {
            b'(' => {
                let tok_start = i;
                i += 1;
                out.push(SpannedToken {
                    token: Token::LParen,
                    span: tok_start..i,
                });
            }
            b')' => {
                let tok_start = i;
                i += 1;
                out.push(SpannedToken {
                    token: Token::RParen,
                    span: tok_start..i,
                });
            }
            b',' => {
                let tok_start = i;
                i += 1;
                out.push(SpannedToken {
                    token: Token::Comma,
                    span: tok_start..i,
                });
            }
            b'\'' => {
                // Quoted string literal. SQL doubles `''` for an
                // embedded single quote.
                let start = i;
                i += 1;
                loop {
                    if i >= bytes.len() {
                        return None; // unterminated
                    }
                    if bytes[i] == b'\'' {
                        if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                let raw = std::str::from_utf8(&bytes[start..i]).ok()?;
                out.push(SpannedToken {
                    token: Token::QuotedString(raw.to_string()),
                    span: start..i,
                });
            }
            b'<' | b'>' | b'=' | b'!' => {
                // Operator. Try two-char forms first.
                let tok_start = i;
                let two = if i + 1 < bytes.len() {
                    Some([b, bytes[i + 1]])
                } else {
                    None
                };
                let (op, len) = match two {
                    Some([b'<', b'=']) => (Op::Le, 2),
                    Some([b'>', b'=']) => (Op::Ge, 2),
                    Some([b'<', b'>'] | [b'!', b'=']) => (Op::Ne, 2),
                    _ => match b {
                        b'<' => (Op::Lt, 1),
                        b'>' => (Op::Gt, 1),
                        b'=' => (Op::Eq, 1),
                        // bare `!` not allowed
                        _ => return None,
                    },
                };
                i += len;
                out.push(SpannedToken {
                    token: Token::Op(op),
                    span: tok_start..i,
                });
            }
            b'-' | b'+' => {
                // Signed numeric literal — only valid when followed
                // by a digit and *not* preceded by an operand-like
                // token (we are at the start of a literal slot).
                if !is_literal_start_slot(&out) {
                    return None;
                }
                let start = i;
                i += 1;
                while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                    i += 1;
                }
                if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
                    i += 1;
                    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
                        i += 1;
                    }
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                let raw = std::str::from_utf8(&bytes[start..i]).ok()?;
                out.push(SpannedToken {
                    token: parse_number_token(raw)?,
                    span: start..i,
                });
            }
            b'0'..=b'9' => {
                let start = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i < bytes.len() && bytes[i] == b'.' {
                    i += 1;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
                    i += 1;
                    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
                        i += 1;
                    }
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                let raw = std::str::from_utf8(&bytes[start..i]).ok()?;
                out.push(SpannedToken {
                    token: parse_number_token(raw)?,
                    span: start..i,
                });
            }
            b'A'..=b'Z' | b'a'..=b'z' | b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let word = std::str::from_utf8(&bytes[start..i]).ok()?;
                out.push(SpannedToken {
                    token: classify_word(word),
                    span: start..i,
                });
            }
            _ => return None,
        }
    }
    Some(out)
}

fn tokenize(input: &str) -> Option<Vec<Token>> {
    tokenize_spanned(input).map(|tokens| tokens.into_iter().map(|st| st.token).collect())
}

/// True when the next token sits in a position that may legally
/// hold a literal (start of input, after an operator, after `(`,
/// after `,`, after `BETWEEN`/`AND`/`OR`/`NOT`/`IN`/`IS`). Used to
/// disambiguate `-5` (literal) from `a-5` (which we reject anyway).
fn is_literal_start_slot(tokens: &[SpannedToken]) -> bool {
    matches!(
        tokens.last().map(|st| &st.token),
        None | Some(
            Token::Op(_)
                | Token::LParen
                | Token::Comma
                | Token::Keyword(
                    Keyword::And
                        | Keyword::Or
                        | Keyword::Not
                        | Keyword::Between
                        | Keyword::In
                        | Keyword::Is,
                )
        )
    )
}

fn parse_number_token(raw: &str) -> Option<Token> {
    if let Ok(i) = raw.parse::<i64>() {
        return Some(Token::Integer(i));
    }
    if let Ok(f) = raw.parse::<f64>() {
        return Some(Token::Float(f));
    }
    None
}

fn classify_word(word: &str) -> Token {
    match word.to_ascii_uppercase().as_str() {
        "AND" => Token::Keyword(Keyword::And),
        "OR" => Token::Keyword(Keyword::Or),
        "NOT" => Token::Keyword(Keyword::Not),
        "IN" => Token::Keyword(Keyword::In),
        "BETWEEN" => Token::Keyword(Keyword::Between),
        "IS" => Token::Keyword(Keyword::Is),
        "NULL" => Token::Keyword(Keyword::Null),
        "TRUE" => Token::Keyword(Keyword::True),
        "FALSE" => Token::Keyword(Keyword::False),
        _ => Token::Ident(word.to_string()),
    }
}

// -- Parser ----------------------------------------------------------------

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    depth: usize,
}

impl Parser {
    fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn bump(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn eat_keyword(&mut self, kw: Keyword) -> bool {
        if matches!(self.peek(), Some(Token::Keyword(k)) if *k == kw) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn parse_or(&mut self) -> CheckExpr {
        let first = self.parse_and();
        if matches!(first, CheckExpr::Unparseable) {
            return CheckExpr::Unparseable;
        }
        let mut parts = vec![first];
        while self.eat_keyword(Keyword::Or) {
            let next = self.parse_and();
            if matches!(next, CheckExpr::Unparseable) {
                return CheckExpr::Unparseable;
            }
            parts.push(next);
        }
        if parts.len() == 1 {
            parts.pop().unwrap_or(CheckExpr::Unparseable)
        } else {
            CheckExpr::Or(parts)
        }
    }

    fn parse_and(&mut self) -> CheckExpr {
        let first = self.parse_not();
        if matches!(first, CheckExpr::Unparseable) {
            return CheckExpr::Unparseable;
        }
        let mut parts = vec![first];
        while self.eat_keyword(Keyword::And) {
            let next = self.parse_not();
            if matches!(next, CheckExpr::Unparseable) {
                return CheckExpr::Unparseable;
            }
            parts.push(next);
        }
        if parts.len() == 1 {
            parts.pop().unwrap_or(CheckExpr::Unparseable)
        } else {
            CheckExpr::And(parts)
        }
    }

    fn parse_not(&mut self) -> CheckExpr {
        if self.eat_keyword(Keyword::Not) {
            let inner = self.parse_atom();
            if matches!(inner, CheckExpr::Unparseable) {
                return CheckExpr::Unparseable;
            }
            CheckExpr::Not(Box::new(inner))
        } else {
            self.parse_atom()
        }
    }

    fn parse_atom(&mut self) -> CheckExpr {
        match self.peek() {
            Some(Token::LParen) => {
                self.pos += 1;
                if self.depth >= MAX_CHECK_EXPR_DEPTH {
                    return CheckExpr::Unparseable;
                }
                self.depth += 1;
                let inner = self.parse_or();
                self.depth -= 1;
                if matches!(inner, CheckExpr::Unparseable) {
                    return CheckExpr::Unparseable;
                }
                if !matches!(self.peek(), Some(Token::RParen)) {
                    return CheckExpr::Unparseable;
                }
                self.pos += 1;
                inner
            }
            Some(Token::Ident(_)) => self.parse_predicate(),
            _ => CheckExpr::Unparseable,
        }
    }

    fn parse_predicate(&mut self) -> CheckExpr {
        let Some(Token::Ident(column)) = self.bump() else {
            return CheckExpr::Unparseable;
        };
        // Look at next token to decide the predicate shape.
        let negated_for_in_or_between = self.eat_keyword(Keyword::Not);
        match self.peek().cloned() {
            Some(Token::Op(op)) => {
                if negated_for_in_or_between {
                    return CheckExpr::Unparseable; // `col NOT < 5` not legal
                }
                self.pos += 1;
                let Some(literal) = self.try_take_literal() else {
                    return CheckExpr::Unparseable;
                };
                CheckExpr::Compare {
                    column,
                    op,
                    value: literal,
                }
            }
            Some(Token::Keyword(Keyword::In)) => {
                self.pos += 1;
                if !matches!(self.peek(), Some(Token::LParen)) {
                    return CheckExpr::Unparseable;
                }
                self.pos += 1;
                let mut values = Vec::new();
                loop {
                    let Some(lit) = self.try_take_literal() else {
                        return CheckExpr::Unparseable;
                    };
                    values.push(lit);
                    match self.peek() {
                        Some(Token::Comma) => {
                            self.pos += 1;
                        }
                        Some(Token::RParen) => {
                            self.pos += 1;
                            break;
                        }
                        _ => return CheckExpr::Unparseable,
                    }
                }
                if values.is_empty() {
                    // `IN ()` is SQLite-only; refuse to analyse.
                    return CheckExpr::Unparseable;
                }
                CheckExpr::In {
                    column,
                    values,
                    negated: negated_for_in_or_between,
                }
            }
            Some(Token::Keyword(Keyword::Between)) => {
                self.pos += 1;
                let Some(low) = self.try_take_literal() else {
                    return CheckExpr::Unparseable;
                };
                if !self.eat_keyword(Keyword::And) {
                    return CheckExpr::Unparseable;
                }
                let Some(high) = self.try_take_literal() else {
                    return CheckExpr::Unparseable;
                };
                CheckExpr::Between {
                    column,
                    low,
                    high,
                    negated: negated_for_in_or_between,
                }
            }
            Some(Token::Keyword(Keyword::Is)) => {
                if negated_for_in_or_between {
                    return CheckExpr::Unparseable; // `col NOT IS NULL` is invalid
                }
                self.pos += 1;
                let negated = self.eat_keyword(Keyword::Not);
                if !self.eat_keyword(Keyword::Null) {
                    return CheckExpr::Unparseable;
                }
                CheckExpr::IsNull { column, negated }
            }
            _ => CheckExpr::Unparseable,
        }
    }

    /// Consume the next token if it is a literal.
    fn try_take_literal(&mut self) -> Option<Literal> {
        let lit = match self.peek()? {
            Token::Integer(i) => Literal::Integer(*i),
            Token::Float(f) => Literal::Float(*f),
            Token::QuotedString(s) => Literal::String(s.clone()),
            Token::Keyword(Keyword::True) => Literal::Bool(true),
            Token::Keyword(Keyword::False) => Literal::Bool(false),
            Token::Keyword(Keyword::Null) => Literal::Null,
            _ => return None,
        };
        self.pos += 1;
        Some(lit)
    }
}

// -- Test oracle: F86 backward-compat helpers ----------------------------

/// True when `expr` parses as a recognised CHECK shape that
/// references `column` *somewhere* in a way F86 can evaluate.
///
/// F86 (column default vs CHECK) used to call into a private
/// `parse_simple_check(expr, column)` that returned `Some` only for
/// `<col> <op> <lit>` or `<col> IN (...)` against the *exact* given
/// column. After the unified parser this helper preserves the same
/// boolean contract — used by F4 (`check_additions`) to identify the
/// target column of an added CHECK.
#[must_use]
pub(crate) fn matches_for_column(expr: &str, column: &str) -> bool {
    extract_simple_column_check(&parse(expr), column).is_some()
}

/// Extract the (Op, Literal) or (In, Vec<Literal>) shape used by F86
/// for a *specific* column. Returns `None` for any expression that
/// is not in the F86-recognisable single-predicate form against the
/// given column.
///
/// This is the bridge between the generic parser (which may parse
/// the same expression as part of a larger boolean tree) and F86's
/// per-column evaluation contract.
pub(crate) fn extract_simple_column_check(
    expr: &CheckExpr,
    column: &str,
) -> Option<SimpleColumnCheck> {
    match expr {
        CheckExpr::Compare {
            column: c,
            op,
            value,
        } if c == column => Some(SimpleColumnCheck::Op {
            op: *op,
            value: value.clone(),
        }),
        CheckExpr::In {
            column: c,
            values,
            negated: false,
        } if c == column => Some(SimpleColumnCheck::In(values.clone())),
        _ => None,
    }
}

/// F86-compatible projection of a recognised CHECK predicate against
/// a *specific* column.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SimpleColumnCheck {
    Op { op: Op, value: Literal },
    In(Vec<Literal>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn lex_simple_compare() {
        let expr = "age > 0";
        let tokens = lex_check_expr(expr);

        assert_eq!(
            tokens,
            vec![
                CheckToken {
                    kind: CheckTokenKind::Column,
                    span: 0..3,
                },
                CheckToken {
                    kind: CheckTokenKind::Operator,
                    span: 4..5,
                },
                CheckToken {
                    kind: CheckTokenKind::Number,
                    span: 6..7,
                },
            ]
        );
        assert_eq!(&expr[tokens[0].span.clone()], "age");
        assert_eq!(&expr[tokens[1].span.clone()], ">");
        assert_eq!(&expr[tokens[2].span.clone()], "0");
    }

    #[test]
    fn lex_duplicate_column_distinct_spans() {
        let expr = "age > 0 AND age < 150";
        let tokens = lex_check_expr(expr);
        let column_spans: Vec<_> = tokens
            .iter()
            .filter(|token| token.kind == CheckTokenKind::Column)
            .map(|token| token.span.clone())
            .collect();

        assert_eq!(tokens.len(), 7);
        assert_eq!(column_spans.as_slice(), &[0..3, 12..15]);
        assert_ne!(column_spans[0], column_spans[1]);
        assert_eq!(&expr[column_spans[0].clone()], "age");
        assert_eq!(&expr[column_spans[1].clone()], "age");
    }

    #[test]
    fn lex_string_literal() {
        let expr = "status = 'active'";
        let tokens = lex_check_expr(expr);
        let texts: Vec<_> = tokens
            .iter()
            .map(|token| &expr[token.span.clone()])
            .collect();

        assert_eq!(
            tokens,
            vec![
                CheckToken {
                    kind: CheckTokenKind::Column,
                    span: 0..6,
                },
                CheckToken {
                    kind: CheckTokenKind::Operator,
                    span: 7..8,
                },
                CheckToken {
                    kind: CheckTokenKind::String,
                    span: 9..17,
                },
            ]
        );
        assert_eq!(texts.as_slice(), &["status", "=", "'active'"]);
    }

    #[test]
    fn lex_between() {
        let expr = "age BETWEEN 0 AND 150";
        let tokens = lex_check_expr(expr);
        let kinds: Vec<_> = tokens.iter().map(|token| token.kind).collect();
        let texts: Vec<_> = tokens
            .iter()
            .map(|token| &expr[token.span.clone()])
            .collect();

        assert_eq!(
            kinds.as_slice(),
            &[
                CheckTokenKind::Column,
                CheckTokenKind::Keyword,
                CheckTokenKind::Number,
                CheckTokenKind::Keyword,
                CheckTokenKind::Number,
            ]
        );
        assert_eq!(texts.as_slice(), &["age", "BETWEEN", "0", "AND", "150"]);
    }

    #[test]
    fn lex_in_list() {
        let expr = "status IN ('a', 'b')";
        let tokens = lex_check_expr(expr);
        let kinds: Vec<_> = tokens.iter().map(|token| token.kind).collect();
        let texts: Vec<_> = tokens
            .iter()
            .map(|token| &expr[token.span.clone()])
            .collect();

        assert_eq!(
            kinds.as_slice(),
            &[
                CheckTokenKind::Column,
                CheckTokenKind::Keyword,
                CheckTokenKind::Punctuation,
                CheckTokenKind::String,
                CheckTokenKind::Punctuation,
                CheckTokenKind::String,
                CheckTokenKind::Punctuation,
            ]
        );
        assert_eq!(
            texts.as_slice(),
            &["status", "IN", "(", "'a'", ",", "'b'", ")"]
        );
    }

    #[test]
    fn lex_is_null() {
        let expr = "deleted_at IS NULL";
        let tokens = lex_check_expr(expr);
        let kinds: Vec<_> = tokens.iter().map(|token| token.kind).collect();
        let texts: Vec<_> = tokens
            .iter()
            .map(|token| &expr[token.span.clone()])
            .collect();

        assert_eq!(
            kinds.as_slice(),
            &[
                CheckTokenKind::Column,
                CheckTokenKind::Keyword,
                CheckTokenKind::Keyword,
            ]
        );
        assert_eq!(texts.as_slice(), &["deleted_at", "IS", "NULL"]);
    }

    #[test]
    fn lex_unparseable_returns_empty() {
        assert!(lex_check_expr("status = 'unterminated").is_empty());
    }

    #[test]
    fn lex_spans_are_byte_accurate() {
        let expr = "age >= 100";
        let tokens = lex_check_expr(expr);

        assert_eq!(tokens[1].span, 4..6);
        assert_eq!(&expr[tokens[1].span.clone()], ">=");
        assert_eq!(tokens[2].span, 7..10);
        assert_eq!(&expr[tokens[2].span.clone()], "100");
    }

    #[test]
    fn empty_input_is_unparseable() {
        assert!(matches!(parse(""), CheckExpr::Unparseable));
        assert!(matches!(parse("   "), CheckExpr::Unparseable));
    }

    #[rstest]
    #[case::gt("age > 0", Op::Gt, Literal::Integer(0))]
    #[case::ge("age >= 1", Op::Ge, Literal::Integer(1))]
    #[case::lt("amount < 100", Op::Lt, Literal::Integer(100))]
    #[case::le("amount <= 100", Op::Le, Literal::Integer(100))]
    #[case::eq("role = 'user'", Op::Eq, Literal::String("'user'".into()))]
    #[case::ne_iso("amount <> 0", Op::Ne, Literal::Integer(0))]
    #[case::ne_bang("amount != 0", Op::Ne, Literal::Integer(0))]
    fn simple_compare_parses(
        #[case] input: &str,
        #[case] expected_op: Op,
        #[case] expected_value: Literal,
    ) {
        let parsed = parse(input);
        let CheckExpr::Compare { column, op, value } = parsed else {
            panic!("expected Compare, got {parsed:?}");
        };
        assert_eq!(op, expected_op);
        assert_eq!(value, expected_value);
        assert!(column == "age" || column == "amount" || column == "role");
    }

    #[test]
    fn in_list_parses() {
        let parsed = parse("status IN ('active', 'inactive', 'pending')");
        let CheckExpr::In {
            column,
            values,
            negated,
        } = parsed
        else {
            panic!("expected In");
        };
        assert_eq!(column, "status");
        assert!(!negated);
        assert_eq!(values.len(), 3);
    }

    #[test]
    fn not_in_list_parses() {
        let parsed = parse("status NOT IN ('archived', 'deleted')");
        let CheckExpr::In {
            column, negated, ..
        } = parsed
        else {
            panic!("expected In with negated");
        };
        assert_eq!(column, "status");
        assert!(negated);
    }

    #[test]
    fn between_parses() {
        let parsed = parse("age BETWEEN 0 AND 150");
        let CheckExpr::Between {
            column,
            low,
            high,
            negated,
        } = parsed
        else {
            panic!("expected Between, got {parsed:?}");
        };
        assert_eq!(column, "age");
        assert_eq!(low, Literal::Integer(0));
        assert_eq!(high, Literal::Integer(150));
        assert!(!negated);
    }

    #[test]
    fn not_between_parses() {
        let parsed = parse("age NOT BETWEEN 0 AND 17");
        let CheckExpr::Between { negated, .. } = parsed else {
            panic!("expected Between");
        };
        assert!(negated);
    }

    #[test]
    fn is_null_parses() {
        assert!(matches!(
            parse("deleted_at IS NULL"),
            CheckExpr::IsNull { negated: false, .. }
        ));
        assert!(matches!(
            parse("deleted_at IS NOT NULL"),
            CheckExpr::IsNull { negated: true, .. }
        ));
    }

    #[test]
    fn and_composition_parses() {
        let parsed = parse("age > 0 AND age < 150");
        let CheckExpr::And(parts) = parsed else {
            panic!("expected And");
        };
        assert_eq!(parts.len(), 2);
    }

    #[test]
    fn or_composition_parses() {
        let parsed = parse("status = 'a' OR status = 'b'");
        let CheckExpr::Or(parts) = parsed else {
            panic!("expected Or");
        };
        assert_eq!(parts.len(), 2);
    }

    #[test]
    fn precedence_and_over_or() {
        // `a OR b AND c` should be `a OR (b AND c)`.
        let parsed = parse("x > 0 OR x < 100 AND x > 50");
        let CheckExpr::Or(or_parts) = parsed else {
            panic!("expected Or at top");
        };
        assert_eq!(or_parts.len(), 2);
        // Second OR branch is an AND.
        assert!(matches!(or_parts[1], CheckExpr::And(_)));
    }

    #[test]
    fn parentheses_override_precedence() {
        let parsed = parse("(x > 0 OR x < 100) AND x > 50");
        let CheckExpr::And(and_parts) = parsed else {
            panic!("expected And at top");
        };
        assert_eq!(and_parts.len(), 2);
        assert!(matches!(and_parts[0], CheckExpr::Or(_)));
    }

    #[test]
    fn case_insensitive_keywords() {
        assert!(matches!(
            parse("age between 0 and 10"),
            CheckExpr::Between { .. }
        ));
        assert!(matches!(
            parse("col is null"),
            CheckExpr::IsNull { negated: false, .. }
        ));
        assert!(matches!(parse("a > 0 and b < 0"), CheckExpr::And(_)));
    }

    #[test]
    fn function_call_is_unparseable() {
        assert!(matches!(parse("LOWER(name) = 'x'"), CheckExpr::Unparseable));
        assert!(matches!(parse("LENGTH(name) > 0"), CheckExpr::Unparseable));
    }

    #[test]
    fn column_to_column_is_unparseable() {
        // `a > b` — b is an identifier, not a literal.
        assert!(matches!(parse("a > b"), CheckExpr::Unparseable));
    }

    #[test]
    fn empty_in_list_is_unparseable() {
        assert!(matches!(parse("col IN ()"), CheckExpr::Unparseable));
    }

    #[test]
    fn pg_cast_is_unparseable() {
        assert!(matches!(parse("col::int > 0"), CheckExpr::Unparseable));
    }

    #[test]
    fn trailing_garbage_is_unparseable() {
        assert!(matches!(parse("age > 0 garbage"), CheckExpr::Unparseable));
    }

    #[rstest]
    #[case::null("col = NULL", Literal::Null)]
    #[case::bool_true("col = TRUE", Literal::Bool(true))]
    #[case::bool_false("col = FALSE", Literal::Bool(false))]
    fn null_and_bool_literals(#[case] input: &str, #[case] expected: Literal) {
        let CheckExpr::Compare { value, .. } = parse(input) else {
            panic!("expected Compare");
        };
        assert_eq!(value, expected);
    }

    #[test]
    fn float_literal() {
        assert!(matches!(
            parse("ratio > 0.5"),
            CheckExpr::Compare {
                value: Literal::Float(_),
                ..
            }
        ));
        assert!(matches!(
            parse("ratio > -0.5"),
            CheckExpr::Compare {
                value: Literal::Float(_),
                ..
            }
        ));
    }

    #[test]
    fn scientific_notation() {
        assert!(matches!(
            parse("big > 1.5e3"),
            CheckExpr::Compare {
                value: Literal::Float(_),
                ..
            }
        ));
    }

    #[test]
    fn nested_parens_in_and_or() {
        let parsed = parse("((a > 0 AND b > 0) OR (a < 0 AND b < 0))");
        assert!(matches!(parsed, CheckExpr::Or(_)));
    }

    #[test]
    fn deeply_nested_parens_does_not_stack_overflow() {
        let expr = format!("{}age > 0{}", "(".repeat(5000), ")".repeat(5000));

        assert!(matches!(parse(&expr), CheckExpr::Unparseable));
    }

    #[test]
    fn deeply_nested_not_does_not_overflow() {
        let expr = format!("{}age > 0", "NOT ".repeat(5000));

        assert!(matches!(parse(&expr), CheckExpr::Unparseable));
    }

    #[test]
    fn moderate_nesting_still_parses() {
        assert!(matches!(parse("((age > 0))"), CheckExpr::Compare { .. }));
    }

    #[test]
    fn unterminated_string_is_unparseable() {
        assert!(matches!(
            parse("col = 'unterminated"),
            CheckExpr::Unparseable
        ));
    }

    #[test]
    fn doubled_quote_in_string_literal() {
        let parsed = parse("col = 'it''s'");
        let CheckExpr::Compare {
            value: Literal::String(s),
            ..
        } = parsed
        else {
            panic!("expected string compare");
        };
        assert_eq!(s, "'it''s'");
    }

    // -- matches_for_column shim tests (F86 / F4 compatibility) --------

    #[test]
    fn matches_for_column_simple_op() {
        assert!(matches_for_column("age > 0", "age"));
        assert!(!matches_for_column("age > 0", "other"));
    }

    #[test]
    fn matches_for_column_in() {
        assert!(matches_for_column("status IN ('a', 'b')", "status"));
        assert!(!matches_for_column("status IN ('a', 'b')", "other"));
    }

    #[test]
    fn matches_for_column_negated_in_not_matched() {
        // F86 only handles positive IN; negated forms fall outside
        // its evaluation contract.
        assert!(!matches_for_column("status NOT IN ('a', 'b')", "status"));
    }

    #[test]
    fn matches_for_column_compound_not_matched() {
        // Compound expressions can't be projected back to a single
        // F86-shaped predicate against the given column.
        assert!(!matches_for_column("age > 0 AND age < 150", "age"));
    }
}
