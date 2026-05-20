use std::collections::BTreeSet;

use proptest::{collection, prelude::*};

use crate::{
    MigrationAction,
    schema::{
        ColumnDef, ColumnType, ComplexColumnType, DefaultValue, EnumValues, NumValue,
        ReferenceAction, SimpleColumnType, StrOrBoolOrArray, StringOrBool, TableConstraint,
        TableDef,
        foreign_key::{ForeignKeyDef, ForeignKeySyntax, ReferenceSyntaxDef},
        primary_key::{PrimaryKeyDef, PrimaryKeySyntax},
    },
};

/// Generates `snake_case` SQL-safe identifiers matching `[a-z][a-z0-9_]{0,20}`.
pub fn arb_safe_ident() -> impl Strategy<Value = String> {
    (
        prop::char::range('a', 'z'),
        collection::vec(
            prop_oneof![
                prop::char::range('a', 'z'),
                prop::char::range('0', '9'),
                Just('_'),
            ],
            0..=20,
        ),
    )
        .prop_map(|(first, rest)| {
            let mut ident = String::with_capacity(rest.len() + 1);
            ident.push(first);
            ident.extend(rest);
            ident
        })
}

pub fn arb_simple_column_type() -> impl Strategy<Value = SimpleColumnType> {
    prop_oneof![
        Just(SimpleColumnType::SmallInt),
        Just(SimpleColumnType::Integer),
        Just(SimpleColumnType::BigInt),
        Just(SimpleColumnType::Real),
        Just(SimpleColumnType::DoublePrecision),
        Just(SimpleColumnType::Text),
        Just(SimpleColumnType::Boolean),
        Just(SimpleColumnType::Date),
        Just(SimpleColumnType::Time),
        Just(SimpleColumnType::Timestamp),
        Just(SimpleColumnType::Timestamptz),
        Just(SimpleColumnType::Interval),
        Just(SimpleColumnType::Bytea),
        Just(SimpleColumnType::Uuid),
        Just(SimpleColumnType::Json),
        Just(SimpleColumnType::Inet),
        Just(SimpleColumnType::Cidr),
        Just(SimpleColumnType::Macaddr),
        Just(SimpleColumnType::Xml),
    ]
}

pub fn arb_complex_column_type() -> impl Strategy<Value = ComplexColumnType> {
    prop_oneof![
        (1_u32..=512).prop_map(|length| ComplexColumnType::Varchar { length }),
        (1_u32..=64, 0_u32..=16)
            .prop_filter("scale must be <= precision", |(precision, scale)| {
                scale <= precision
            })
            .prop_map(|(precision, scale)| ComplexColumnType::Numeric { precision, scale }),
        (1_u32..=64).prop_map(|length| ComplexColumnType::Char { length }),
        arb_safe_ident().prop_map(|custom_type| ComplexColumnType::Custom { custom_type }),
        (arb_safe_ident(), arb_enum_values())
            .prop_map(|(name, values)| { ComplexColumnType::Enum { name, values } }),
    ]
}

pub fn arb_column_type() -> impl Strategy<Value = ColumnType> {
    prop_oneof![
        arb_simple_column_type().prop_map(ColumnType::Simple),
        arb_complex_column_type().prop_map(ColumnType::Complex),
    ]
}

pub fn arb_reference_action() -> impl Strategy<Value = ReferenceAction> {
    prop_oneof![
        Just(ReferenceAction::Cascade),
        Just(ReferenceAction::Restrict),
        Just(ReferenceAction::SetNull),
        Just(ReferenceAction::SetDefault),
        Just(ReferenceAction::NoAction),
    ]
}

pub fn arb_default_value() -> impl Strategy<Value = DefaultValue> {
    prop_oneof![
        any::<bool>().prop_map(DefaultValue::Bool),
        (-10_000_i64..=10_000).prop_map(DefaultValue::Integer),
        (-10_000_i32..=10_000).prop_map(|n| DefaultValue::Float(f64::from(n) / 10.0)),
        arb_default_string().prop_map(DefaultValue::String),
    ]
}

pub fn arb_str_or_bool() -> impl Strategy<Value = StringOrBool> {
    arb_default_value()
}

pub fn arb_str_or_bool_or_array() -> impl Strategy<Value = StrOrBoolOrArray> {
    prop_oneof![
        arb_safe_ident().prop_map(StrOrBoolOrArray::Str),
        unique_idents(1..=4).prop_map(StrOrBoolOrArray::Array),
        any::<bool>().prop_map(StrOrBoolOrArray::Bool),
    ]
}

pub fn arb_column_def() -> impl Strategy<Value = ColumnDef> {
    (
        arb_safe_ident(),
        arb_column_type(),
        any::<bool>(),
        prop::option::of(arb_str_or_bool()),
        prop::option::of(arb_comment()),
        prop::option::of(arb_primary_key_syntax()),
        prop::option::of(arb_str_or_bool_or_array()),
        prop::option::of(arb_str_or_bool_or_array()),
        prop::option::of(arb_foreign_key_syntax()),
    )
        .prop_map(
            |(name, ty, nullable, default, comment, primary_key, unique, index, foreign_key)| {
                let mut column = ColumnDef::new(name, ty, nullable);
                if let Some(default) = default {
                    column = column.default(default);
                }
                if let Some(comment) = comment {
                    column = column.comment(comment);
                }
                if let Some(primary_key) = primary_key {
                    column = column.primary_key(primary_key);
                }
                if let Some(unique) = unique {
                    column = column.unique(unique);
                }
                if let Some(index) = index {
                    column = column.index(index);
                }
                if let Some(foreign_key) = foreign_key {
                    column = column.foreign_key(foreign_key);
                }
                column
            },
        )
}

pub fn arb_table_def() -> impl Strategy<Value = TableDef> {
    (
        arb_safe_ident(),
        prop::option::of(arb_comment()),
        collection::vec(arb_column_def(), 0..=8).prop_filter("unique column names", |columns| {
            names_are_unique(columns.iter().map(|column| column.name.as_str()))
        }),
        collection::vec(arb_table_constraint(), 0..=4),
    )
        .prop_map(|(name, description, columns, constraints)| TableDef {
            name,
            description,
            columns,
            constraints,
        })
}

pub fn arb_table_constraint() -> impl Strategy<Value = TableConstraint> {
    prop_oneof![
        (any::<bool>(), unique_idents(1..=4)).prop_map(|(auto_increment, columns)| {
            TableConstraint::PrimaryKey {
                auto_increment,
                columns,
            }
        }),
        (prop::option::of(arb_safe_ident()), unique_idents(1..=4))
            .prop_map(|(name, columns)| TableConstraint::Unique { name, columns },),
        (
            prop::option::of(arb_safe_ident()),
            unique_idents(1..=4),
            arb_safe_ident(),
            unique_idents(1..=4),
            prop::option::of(arb_reference_action()),
            prop::option::of(arb_reference_action()),
        )
            .prop_map(
                |(name, columns, ref_table, ref_columns, on_delete, on_update)| {
                    TableConstraint::ForeignKey {
                        name,
                        columns,
                        ref_table,
                        ref_columns,
                        on_delete,
                        on_update,
                    }
                },
            ),
        (arb_safe_ident(), arb_check_expr())
            .prop_map(|(name, expr)| { TableConstraint::Check { name, expr } }),
        (prop::option::of(arb_safe_ident()), unique_idents(1..=4))
            .prop_map(|(name, columns)| TableConstraint::Index { name, columns }),
    ]
}

pub fn arb_migration_action() -> impl Strategy<Value = MigrationAction> {
    prop_oneof![
        arb_create_table_action(),
        arb_safe_ident().prop_map(|table| MigrationAction::DeleteTable { table }),
        arb_add_column_action(),
        (arb_safe_ident(), arb_safe_ident(), arb_safe_ident())
            .prop_map(|(table, from, to)| { MigrationAction::RenameColumn { table, from, to } }),
        (arb_safe_ident(), arb_safe_ident())
            .prop_map(|(table, column)| { MigrationAction::DeleteColumn { table, column } }),
        arb_modify_column_type_action(),
        arb_modify_column_nullable_action(),
        (
            arb_safe_ident(),
            arb_safe_ident(),
            prop::option::of(arb_default_string())
        )
            .prop_map(|(table, column, new_default)| {
                MigrationAction::ModifyColumnDefault {
                    table,
                    column,
                    new_default,
                }
            }),
        (
            arb_safe_ident(),
            arb_safe_ident(),
            prop::option::of(arb_comment())
        )
            .prop_map(|(table, column, new_comment)| {
                MigrationAction::ModifyColumnComment {
                    table,
                    column,
                    new_comment,
                }
            }),
        (arb_safe_ident(), arb_table_constraint()).prop_map(|(table, constraint)| {
            MigrationAction::AddConstraint { table, constraint }
        }),
        (arb_safe_ident(), arb_table_constraint()).prop_map(|(table, constraint)| {
            MigrationAction::RemoveConstraint { table, constraint }
        }),
        (
            arb_safe_ident(),
            arb_table_constraint(),
            arb_table_constraint()
        )
            .prop_map(|(table, from, to)| MigrationAction::ReplaceConstraint {
                table,
                from,
                to
            },),
        (arb_safe_ident(), arb_safe_ident())
            .prop_map(|(from, to)| { MigrationAction::RenameTable { from, to } }),
        arb_sql().prop_map(|sql| MigrationAction::RawSql { sql }),
    ]
}

fn arb_create_table_action() -> impl Strategy<Value = MigrationAction> {
    (
        arb_safe_ident(),
        collection::vec(arb_column_def(), 0..=8),
        collection::vec(arb_table_constraint(), 0..=4),
    )
        .prop_map(
            |(table, columns, constraints)| MigrationAction::CreateTable {
                table,
                columns,
                constraints,
            },
        )
}

fn arb_add_column_action() -> impl Strategy<Value = MigrationAction> {
    (
        arb_safe_ident(),
        arb_column_def(),
        prop::option::of(arb_default_string()),
    )
        .prop_map(|(table, column, fill_with)| MigrationAction::AddColumn {
            table,
            column: Box::new(column),
            fill_with,
        })
}

fn arb_modify_column_type_action() -> impl Strategy<Value = MigrationAction> {
    (
        arb_safe_ident(),
        arb_safe_ident(),
        arb_column_type(),
        prop::option::of(collection::btree_map(
            arb_safe_ident(),
            arb_default_string(),
            0..=4,
        )),
    )
        .prop_map(
            |(table, column, new_type, fill_with)| MigrationAction::ModifyColumnType {
                table,
                column,
                new_type,
                fill_with,
            },
        )
}

fn arb_modify_column_nullable_action() -> impl Strategy<Value = MigrationAction> {
    (
        arb_safe_ident(),
        arb_safe_ident(),
        any::<bool>(),
        prop::option::of(arb_default_string()),
        prop::option::of(any::<bool>()),
    )
        .prop_map(|(table, column, nullable, fill_with, delete_null_rows)| {
            MigrationAction::ModifyColumnNullable {
                table,
                column,
                nullable,
                fill_with,
                delete_null_rows,
            }
        })
}

fn arb_enum_values() -> impl Strategy<Value = EnumValues> {
    prop_oneof![
        unique_idents(1..=6).prop_map(EnumValues::String),
        collection::vec((arb_safe_ident(), -1_000_i32..=1_000), 1..=6)
            .prop_filter("unique enum variant names", |values| {
                names_are_unique(values.iter().map(|(name, _)| name.as_str()))
            })
            .prop_map(|values| {
                EnumValues::Integer(
                    values
                        .into_iter()
                        .map(|(name, value)| NumValue {
                            name,
                            value: i64::from(value),
                        })
                        .collect(),
                )
            }),
    ]
}

fn arb_primary_key_syntax() -> impl Strategy<Value = PrimaryKeySyntax> {
    prop_oneof![
        any::<bool>().prop_map(PrimaryKeySyntax::Bool),
        any::<bool>()
            .prop_map(|auto_increment| PrimaryKeySyntax::Object(PrimaryKeyDef { auto_increment })),
    ]
}

fn arb_foreign_key_syntax() -> impl Strategy<Value = ForeignKeySyntax> {
    prop_oneof![
        (arb_safe_ident(), arb_safe_ident())
            .prop_map(|(table, column)| ForeignKeySyntax::String(format!("{table}.{column}"))),
        (
            arb_safe_ident(),
            arb_safe_ident(),
            prop::option::of(arb_reference_action()),
            prop::option::of(arb_reference_action())
        )
            .prop_map(|(table, column, on_delete, on_update)| {
                ForeignKeySyntax::Reference(ReferenceSyntaxDef {
                    references: format!("{table}.{column}"),
                    on_delete,
                    on_update,
                })
            }),
        (
            arb_safe_ident(),
            unique_idents(1..=4),
            prop::option::of(arb_reference_action()),
            prop::option::of(arb_reference_action())
        )
            .prop_map(|(ref_table, ref_columns, on_delete, on_update)| {
                ForeignKeySyntax::Object(ForeignKeyDef {
                    ref_table,
                    ref_columns,
                    on_delete,
                    on_update,
                })
            }),
    ]
}

fn unique_idents(size: impl Into<collection::SizeRange>) -> impl Strategy<Value = Vec<String>> {
    collection::vec(arb_safe_ident(), size).prop_filter("unique identifiers", |values| {
        names_are_unique(values.iter().map(String::as_str))
    })
}

fn names_are_unique<'a>(names: impl Iterator<Item = &'a str>) -> bool {
    let mut seen = BTreeSet::new();
    names.into_iter().all(|name| seen.insert(name))
}

fn arb_default_string() -> impl Strategy<Value = String> {
    prop_oneof![
        arb_safe_ident(),
        arb_safe_ident().prop_map(|ident| format!("'{ident}'")),
        Just("NOW()".to_string()),
        Just("CURRENT_TIMESTAMP".to_string()),
    ]
}

fn arb_comment() -> impl Strategy<Value = String> {
    collection::vec(prop::char::range('a', 'z'), 0..=80)
        .prop_map(|chars| chars.into_iter().collect())
}

fn arb_check_expr() -> impl Strategy<Value = String> {
    (
        arb_safe_ident(),
        prop_oneof![Just(">"), Just(">="), Just("<"), Just("<=")],
        0_i32..=100,
    )
        .prop_map(|(column, op, value)| format!("{column} {op} {value}"))
}

fn arb_sql() -> impl Strategy<Value = String> {
    arb_safe_ident().prop_map(|name| format!("SELECT 1 AS {name}"))
}
