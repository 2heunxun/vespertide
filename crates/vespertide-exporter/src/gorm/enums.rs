use vespertide_core::schema::column::EnumValues;

use super::render::to_pascal_case;

pub(super) fn render_enum(lines: &mut Vec<String>, name: &str, values: &EnumValues) {
    // `name` is already the sanitized, PascalCased (and possibly struct-qualified)
    // identifier built by the caller — re-running `to_pascal_case` here would
    // split on the `_` a leading-digit escape (e.g. `_1users`) introduces and
    // silently drop it.
    let type_name = name;

    let mut rendered = match values {
        EnumValues::String(_) => {
            vec![
                format!("type {type_name} string"),
                String::new(),
                "const (".into(),
            ]
        }
        EnumValues::Integer(_) => {
            vec![
                format!("type {type_name} int"),
                String::new(),
                "const (".into(),
            ]
        }
    };

    match values {
        EnumValues::String(vals) => {
            for val in vals {
                let const_name = format!("{type_name}{}", to_pascal_case(val));
                rendered.push(format!("    {const_name} {type_name} = \"{val}\""));
            }
        }
        EnumValues::Integer(vals) => {
            for val in vals {
                let const_name = format!("{type_name}{}", to_pascal_case(&val.name));
                rendered.push(format!("    {const_name} {type_name} = {}", val.value));
            }
        }
    }

    rendered.push(")".into());
    lines.extend(rendered);
}
