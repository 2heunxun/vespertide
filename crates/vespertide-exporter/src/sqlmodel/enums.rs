use vespertide_core::schema::column::EnumValues;

// Naming helpers shared with the `SQLAlchemy` exporter — both Python ORMs
// produce identical PascalCase class names and identical
// SCREAMING_SNAKE_CASE enum member names, so the implementation lives in
// `crate::python_naming` and we re-export it here to keep every existing
// `super::enums::to_*` path working without churn.
pub(super) use crate::python_naming::{to_pascal_case, to_screaming_snake_case};

pub(super) fn render_enum(lines: &mut Vec<String>, name: &str, values: &EnumValues) {
    let class_name = to_pascal_case(name);

    match values {
        EnumValues::String(vals) => {
            lines.push(format!("class {class_name}(str, enum.Enum):"));
            for val in vals {
                let variant_name = to_screaming_snake_case(val);
                lines.push(format!("    {variant_name} = \"{val}\""));
            }
        }
        EnumValues::Integer(vals) => {
            lines.push(format!("class {class_name}(enum.IntEnum):"));
            for val in vals {
                lines.push(format!("    {} = {}", val.name, val.value));
            }
        }
    }
}
