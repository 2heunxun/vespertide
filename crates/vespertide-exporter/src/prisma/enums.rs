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
