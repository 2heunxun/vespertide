use vespertide_core::schema::column::EnumValues;
use vespertide_naming::{to_pascal_case, to_screaming_snake_case};

pub(super) fn render_enum(name: &str, values: &EnumValues) -> String {
    let enum_name = to_pascal_case(name);
    let mut lines = Vec::new();
    lines.push(format!("enum {enum_name} {{"));
    match values {
        EnumValues::String(vals) => {
            for val in vals {
                let variant = to_screaming_snake_case(val);
                if variant == *val {
                    lines.push(format!("  {variant}"));
                } else {
                    lines.push(format!("  {variant} @map(\"{val}\")"));
                }
            }
        }
        EnumValues::Integer(vals) => {
            // Prisma doesn't support integer enums natively; emit as string variants with comment
            for val in vals {
                let variant = to_screaming_snake_case(&val.name);
                let value = val.value;
                lines.push(format!("  {variant} // = {value}"));
            }
        }
    }
    lines.push("}".into());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use vespertide_core::schema::column::NumValue;

    use super::*;

    #[rstest]
    #[case::already_screaming(
        vec!["DRAFT".into(), "PUBLISHED".into()],
        "enum DocStatus {\n  DRAFT\n  PUBLISHED\n}"
    )]
    #[case::normalized(
        vec!["draft".into(), "in progress".into()],
        "enum DocStatus {\n  DRAFT @map(\"draft\")\n  IN_PROGRESS @map(\"in progress\")\n}"
    )]
    #[case::leading_digit(
        vec!["1critical".into()],
        "enum DocStatus {\n  _1CRITICAL @map(\"1critical\")\n}"
    )]
    fn string_variants_carry_map_only_when_normalization_changes_them(
        #[case] values: Vec<String>,
        #[case] expected: &str,
    ) {
        assert_eq!(
            render_enum("doc_status", &EnumValues::String(values)),
            expected
        );
    }

    #[test]
    fn integer_variants_keep_the_declared_value_in_a_comment() {
        let values = EnumValues::Integer(vec![
            NumValue {
                name: "low".into(),
                value: 100,
            },
            NumValue {
                name: "high".into(),
                value: 200,
            },
        ]);
        assert_eq!(
            render_enum("priority", &values),
            "enum Priority {\n  LOW // = 100\n  HIGH // = 200\n}"
        );
    }
}
