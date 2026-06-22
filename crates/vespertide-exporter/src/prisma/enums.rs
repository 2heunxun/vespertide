use vespertide_core::schema::column::EnumValues;

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

pub(super) fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect()
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
    result.trim_end_matches('_').to_string()
}
