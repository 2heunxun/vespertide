use std::collections::HashMap;

use rstest::rstest;
use vespertide_core::schema::column::{ColumnType, ComplexColumnType, SimpleColumnType};
use vespertide_core::{ColumnDef, TableDef};

use super::render::{column_field_names, go_relation_field_name, to_go_field_name};
use super::types::go_type_for_column_mapped;

fn col(name: &str, ty: ColumnType) -> ColumnDef {
    ColumnDef {
        name: name.into(),
        r#type: ty,
        nullable: false,
        default: None,
        comment: None,
        primary_key: None,
        unique: None,
        index: None,
        foreign_key: None,
    }
}

// -----------------------------------------------------------------------
// Type mapping unit tests
// -----------------------------------------------------------------------

#[rstest]
#[case(ColumnType::Simple(SimpleColumnType::SmallInt), false, "int16")]
#[case(ColumnType::Simple(SimpleColumnType::Integer), false, "int32")]
#[case(ColumnType::Simple(SimpleColumnType::BigInt), false, "int64")]
#[case(ColumnType::Simple(SimpleColumnType::Real), false, "float32")]
#[case(
    ColumnType::Simple(SimpleColumnType::DoublePrecision),
    false,
    "float64"
)]
#[case(ColumnType::Simple(SimpleColumnType::Text), false, "string")]
#[case(ColumnType::Simple(SimpleColumnType::Boolean), false, "bool")]
#[case(ColumnType::Simple(SimpleColumnType::Timestamp), false, "time.Time")]
#[case(ColumnType::Simple(SimpleColumnType::Timestamptz), false, "time.Time")]
#[case(ColumnType::Simple(SimpleColumnType::Date), false, "time.Time")]
#[case(ColumnType::Simple(SimpleColumnType::Time), false, "time.Time")]
#[case(ColumnType::Simple(SimpleColumnType::Uuid), false, "uuid.UUID")]
#[case(ColumnType::Simple(SimpleColumnType::Json), false, "datatypes.JSON")]
#[case(ColumnType::Simple(SimpleColumnType::Bytea), false, "[]byte")]
#[case(ColumnType::Simple(SimpleColumnType::Inet), false, "string")]
#[case(ColumnType::Complex(ComplexColumnType::Varchar { length: 255 }), false, "string")]
#[case(ColumnType::Complex(ComplexColumnType::Numeric { precision: 10, scale: 2 }), false, "decimal.Decimal")]
#[case(ColumnType::Complex(ComplexColumnType::Custom { custom_type: "JSONB".into() }), false, "datatypes.JSON")]
#[case(ColumnType::Complex(ComplexColumnType::Custom { custom_type: "jsonb".into() }), false, "datatypes.JSON")]
#[case(ColumnType::Complex(ComplexColumnType::Custom { custom_type: "TEXT".into() }), false, "string")]
#[case(ColumnType::Simple(SimpleColumnType::Integer), true, "*int32")]
#[case(ColumnType::Simple(SimpleColumnType::Text), true, "*string")]
#[case(ColumnType::Simple(SimpleColumnType::Timestamp), true, "*time.Time")]
fn test_go_type_mapping(
    #[case] col_type: ColumnType,
    #[case] nullable: bool,
    #[case] expected: &str,
) {
    assert_eq!(
        go_type_for_column_mapped(&col_type, nullable, &HashMap::new()),
        expected
    );
}

#[rstest]
#[case("user_id", "UserID")]
#[case("id", "ID")]
#[case("created_at", "CreatedAt")]
#[case("profile_image", "ProfileImage")]
#[case("media_id", "MediaID")]
#[case("identity", "Identity")]
#[case("idx", "Idx")]
#[case("1st_place", "X1stPlace")]
fn test_to_go_field_name(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(to_go_field_name(input), expected);
}

/// Two columns that map to one Go name get distinct fields, in declaration order.
#[test]
fn column_field_names_disambiguate_go_collisions() {
    let table = TableDef {
        name: "sessions".into(),
        description: None,
        columns: vec![
            col("user_id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("userId", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        constraints: vec![],
    };
    let names = column_field_names(&table);
    assert_eq!(names["user_id"], "UserID");
    assert_eq!(names["userId"], "UserID2");
}

#[rstest]
#[case("user_id", "User")]
#[case("author_id", "Author")]
#[case("parent_id", "Parent")]
#[case("node", "Node")]
fn test_go_relation_field_name(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(go_relation_field_name(input), expected);
}
