use serde::{Deserialize, Serialize};

/// Prisma `relationMode` datasource setting.
///
/// Controls whether Prisma relies on real database-level foreign key
/// constraints or emulates referential integrity in Prisma Client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub enum RelationMode {
    /// Use real database-level foreign key constraints. This is Prisma's
    /// default and is correct for any backend that supports FKs (e.g.
    /// PostgreSQL, MySQL, SQLite).
    ForeignKeys,
    /// Emulate referential integrity in Prisma Client instead of relying on
    /// database-level foreign keys. Needed for datastores that don't support
    /// FK constraints (e.g. PlanetScale, MongoDB).
    Prisma,
}

impl RelationMode {
    /// The literal string Prisma expects for `relationMode = "..."`.
    pub fn as_str(self) -> &'static str {
        match self {
            RelationMode::ForeignKeys => "foreignKeys",
            RelationMode::Prisma => "prisma",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_str_matches_prisma_literals() {
        assert_eq!(RelationMode::ForeignKeys.as_str(), "foreignKeys");
        assert_eq!(RelationMode::Prisma.as_str(), "prisma");
    }

    #[test]
    fn serde_round_trip() {
        assert_eq!(
            serde_json::to_string(&RelationMode::ForeignKeys).unwrap(),
            "\"foreignKeys\""
        );
        assert_eq!(
            serde_json::to_string(&RelationMode::Prisma).unwrap(),
            "\"prisma\""
        );
        assert_eq!(
            serde_json::from_str::<RelationMode>("\"foreignKeys\"").unwrap(),
            RelationMode::ForeignKeys
        );
        assert_eq!(
            serde_json::from_str::<RelationMode>("\"prisma\"").unwrap(),
            RelationMode::Prisma
        );
    }
}
