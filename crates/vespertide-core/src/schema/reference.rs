use serde::{Deserialize, Serialize};

/// The referential action taken on child rows when the referenced parent row changes.
///
/// Used in `ForeignKeyDef::on_delete` and `ForeignKeyDef::on_update` to control cascading
/// behaviour. In JSON model files these are written in `snake_case`
/// (e.g. `"on_delete": "cascade"`).
///
/// This enum is `#[non_exhaustive]`: new variants may be added in future releases.
/// Downstream `match` expressions should include a wildcard arm.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ReferenceAction {
    /// Automatically delete or update child rows when the parent row is deleted or updated (`CASCADE`).
    Cascade,
    /// Prevent the parent row from being deleted or updated if child rows exist (`RESTRICT`).
    Restrict,
    /// Set the foreign key column(s) in child rows to `NULL` when the parent changes (`SET NULL`).
    /// The column must be nullable.
    SetNull,
    /// Set the foreign key column(s) in child rows to their column default when the parent changes (`SET DEFAULT`).
    SetDefault,
    /// Do nothing to child rows; the database defers enforcement or raises an error (`NO ACTION`).
    NoAction,
}

impl ReferenceAction {
    /// SQL keyword representation as written in `ALTER TABLE ... ADD
    /// CONSTRAINT ... FOREIGN KEY ... ON DELETE <keyword>` etc. Used by
    /// `vespertide-query` when emitting raw SQL (e.g. the F11
    /// `NOT VALID` + `VALIDATE` PG path, which bypasses the sea-query
    /// `ForeignKey` builder).
    #[must_use]
    pub fn to_sql_keyword(&self) -> &'static str {
        match self {
            Self::Cascade => "CASCADE",
            Self::Restrict => "RESTRICT",
            Self::SetNull => "SET NULL",
            Self::SetDefault => "SET DEFAULT",
            Self::NoAction => "NO ACTION",
        }
    }
}
