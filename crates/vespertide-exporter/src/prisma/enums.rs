use vespertide_core::schema::column::EnumValues;
use vespertide_naming::to_pascal_case;

pub(super) fn render_enum(name: &str, values: &EnumValues) -> String {
    let enum_name = to_pascal_case(name);
    let mut lines = Vec::new();
    lines.push(format!("enum {enum_name} {{"));
    match values {
        EnumValues::String(vals) => {
            for val in vals {
                let variant = to_screaming_snake(val);
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
                let variant = to_screaming_snake(&val.name);
                let value = val.value;
                lines.push(format!("  {variant} // = {value}"));
            }
        }
    }
    lines.push("}".into());
    lines.join("\n")
}

pub(super) fn to_screaming_snake(s: &str) -> String {
    let mut result = String::new();
    let mut prev_lower = false;
    for ch in s.chars() {
        if ch.is_uppercase() && prev_lower {
            result.push('_');
        }
        if ch.is_alphanumeric() {
            result.push(ch.to_ascii_uppercase());
            prev_lower = ch.is_lowercase();
        } else {
            result.push('_');
            prev_lower = false;
        }
    }
    let result = result.trim_end_matches('_').to_string();
    // Prisma identifiers cannot start with a digit; prefix with '_' to keep it valid.
    if result.starts_with(|c: char| c.is_ascii_digit()) {
        format!("_{result}")
    } else {
        result
    }
}
