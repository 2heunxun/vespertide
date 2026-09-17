use vespertide_core::schema::column::EnumValues;
use vespertide_naming::{IdentifierStart, sanitize_identifier};

use super::render::to_pascal_case;
use crate::utils::common::string_literal;

pub(super) fn render_enum(lines: &mut Vec<String>, name: &str, values: &EnumValues) {
    // `name` is already the exported, PascalCased (and possibly struct-qualified)
    // identifier built by the caller — re-running `to_pascal_case` here would
    // fold the `_` the sanitizer substitutes for a character Go rejects
    // (`User_id` -> `UserId`) and desync the type from its field.
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
                let const_name = const_name(type_name, val);
                rendered.push(format!(
                    "    {const_name} {type_name} = {}",
                    string_literal(val)
                ));
            }
        }
        EnumValues::Integer(vals) => {
            for val in vals {
                let const_name = const_name(type_name, &val.name);
                rendered.push(format!("    {const_name} {type_name} = {}", val.value));
            }
        }
    }

    rendered.push(")".into());
    lines.extend(rendered);
}

/// A Go constant name for one enum member. The value is arbitrary text —
/// `info-level` and `1critical` are legal in the database — so it is escaped
/// the same way column names are. The `type_name` prefix already supplies a
/// leading letter, so only interior characters can need replacing.
fn const_name(type_name: &str, value: &str) -> String {
    sanitize_identifier(
        &format!("{type_name}{}", to_pascal_case(value)),
        IdentifierStart::Letter,
    )
}
