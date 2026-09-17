//! Shared enum-column scans for the renderers that put a whole schema into
//! one scope.
//!
//! Backends that write one file per table get enum scoping for free; Prisma,
//! Drizzle, GORM and Django do not, and all start from the same per-table
//! scan. What they do with it differs — Prisma deduplicates identifiers
//! globally (see `prisma::enums`), Drizzle table-prefixes every type, GORM and
//! Django prefix only the identifiers more than one table declares.

use vespertide_core::TableDef;
use vespertide_core::schema::column::{ColumnType, ComplexColumnType, EnumValues};

use std::collections::{HashMap, HashSet};

/// Enum columns of a table, first declaration winning per name.
pub(crate) fn collect_table_enums(table: &TableDef) -> Vec<(&str, &EnumValues)> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for col in &table.columns {
        if let ColumnType::Complex(ComplexColumnType::Enum { name, values }) = &col.r#type
            && seen.insert(name.as_str())
        {
            result.push((name.as_str(), values));
        }
    }
    result
}

/// Identifiers more than one table of `schema` declares an enum under. Names
/// are compared after `identifier` has converted them, since distinct names
/// can collapse onto the same one (`doc_status` and `docStatus`).
pub(crate) fn enum_identifiers_shared_across_tables(
    schema: &[TableDef],
    identifier: impl Fn(&str) -> String,
) -> HashSet<String> {
    let mut tables_declaring: HashMap<String, usize> = HashMap::new();
    for table in schema {
        let declared: HashSet<String> = collect_table_enums(table)
            .into_iter()
            .map(|(name, _)| identifier(name))
            .collect();
        for ident in declared {
            *tables_declaring.entry(ident).or_default() += 1;
        }
    }
    tables_declaring
        .into_iter()
        .filter(|(_, tables)| *tables > 1)
        .map(|(ident, _)| ident)
        .collect()
}
