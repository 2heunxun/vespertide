use vespertide_core::schema::column::EnumValues;

use crate::utils::python::enum_member_name;

/// Members carry only their value. Django derives the human label from the
/// member name (`PENDING` -> "Pending"); passing the raw database value as an
/// explicit label would pin a worse one ("pending").
pub(super) fn render_enum(lines: &mut Vec<String>, class_name: &str, values: &EnumValues) {
    match values {
        EnumValues::String(vals) => {
            lines.push(format!("class {class_name}(models.TextChoices):"));
            for val in vals {
                let const_name = enum_member_name(val);
                lines.push(format!("    {const_name} = \"{val}\""));
            }
        }
        EnumValues::Integer(vals) => {
            lines.push(format!("class {class_name}(models.IntegerChoices):"));
            for val in vals {
                let const_name = enum_member_name(&val.name);
                lines.push(format!("    {const_name} = {}", val.value));
            }
        }
    }
}
