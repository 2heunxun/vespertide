use vespertide_core::schema::column::{ColumnType, ComplexColumnType, EnumValues, SimpleColumnType};
use vespertide_core::DefaultValue;

#[derive(Default)]
pub(super) struct UsedImports {
    pub(super) needs_timezone: bool,
    pub(super) needs_uuid_default: bool,
}

pub(super) fn django_field_type(
    col_type: &ColumnType,
    is_pk: bool,
    auto_increment: bool,
) -> &'static str {
    match col_type {
        ColumnType::Simple(ty) => match ty {
            SimpleColumnType::SmallInt => {
                if is_pk && auto_increment { "models.SmallAutoField" } else { "models.SmallIntegerField" }
            }
            SimpleColumnType::Integer => {
                if is_pk && auto_increment { "models.AutoField" } else { "models.IntegerField" }
            }
            SimpleColumnType::BigInt => {
                if is_pk && auto_increment { "models.BigAutoField" } else { "models.BigIntegerField" }
            }
            SimpleColumnType::Real | SimpleColumnType::DoublePrecision => "models.FloatField",
            SimpleColumnType::Text | SimpleColumnType::Xml => "models.TextField",
            SimpleColumnType::Boolean => "models.BooleanField",
            SimpleColumnType::Date => "models.DateField",
            SimpleColumnType::Time => "models.TimeField",
            SimpleColumnType::Timestamp | SimpleColumnType::Timestamptz => "models.DateTimeField",
            SimpleColumnType::Interval => "models.DurationField",
            SimpleColumnType::Bytea => "models.BinaryField",
            SimpleColumnType::Uuid => "models.UUIDField",
            SimpleColumnType::Json => "models.JSONField",
            SimpleColumnType::Inet | SimpleColumnType::Cidr => "models.GenericIPAddressField",
            SimpleColumnType::Macaddr => "models.CharField",
            _ => unreachable!("SimpleColumnType is #[non_exhaustive]; all variants matched"),
        },
        ColumnType::Complex(ty) => match ty {
            ComplexColumnType::Varchar { .. } | ComplexColumnType::Char { .. } => "models.CharField",
            ComplexColumnType::Numeric { .. } => "models.DecimalField",
            ComplexColumnType::Custom { .. } => "models.TextField",
            ComplexColumnType::Enum { values, .. } => match values {
                EnumValues::String(_) => "models.CharField",
                EnumValues::Integer(_) => "models.IntegerField",
            },
            _ => unreachable!("ComplexColumnType is #[non_exhaustive]; all variants matched"),
        },
    }
}

/// Whether this type produces an AutoField that implies primary_key.
pub(super) fn is_auto_field(col_type: &ColumnType, is_pk: bool, auto_increment: bool) -> bool {
    if !(is_pk && auto_increment) {
        return false;
    }
    matches!(
        col_type,
        ColumnType::Simple(
            SimpleColumnType::SmallInt | SimpleColumnType::Integer | SimpleColumnType::BigInt
        )
    )
}

pub(super) fn build_field_kwargs(
    col_type: &ColumnType,
    is_pk: bool,
    auto_increment: bool,
    is_unique: bool,
    nullable: bool,
    default: Option<&DefaultValue>,
    enum_class_name: Option<&str>,
    used: &mut UsedImports,
) -> Vec<String> {
    let mut kwargs: Vec<String> = Vec::new();
    let auto = is_auto_field(col_type, is_pk, auto_increment);

    // Size / precision kwargs
    match col_type {
        ColumnType::Complex(ComplexColumnType::Varchar { length })
        | ColumnType::Complex(ComplexColumnType::Char { length }) => {
            kwargs.push(format!("max_length={length}"));
        }
        ColumnType::Simple(SimpleColumnType::Macaddr) => {
            kwargs.push("max_length=17".into());
        }
        ColumnType::Complex(ComplexColumnType::Numeric { precision, scale }) => {
            kwargs.push(format!("max_digits={precision}"));
            kwargs.push(format!("decimal_places={scale}"));
        }
        ColumnType::Complex(ComplexColumnType::Enum { values, .. }) => {
            if let Some(class) = enum_class_name {
                if let EnumValues::String(vals) = values {
                    let max_len = vals.iter().map(|v| v.len()).max().unwrap_or(1);
                    kwargs.push(format!("max_length={max_len}"));
                }
                kwargs.push(format!("choices={class}.choices"));
            }
        }
        _ => {}
    }

    if is_pk && !auto {
        kwargs.push("primary_key=True".into());
    }
    if nullable && !is_pk {
        kwargs.push("null=True".into());
        kwargs.push("blank=True".into());
    }
    if is_unique && !is_pk {
        kwargs.push("unique=True".into());
    }
    if let Some(dv) = default {
        if let Some(expr) = build_default(col_type, &dv.to_sql(), used) {
            kwargs.push(format!("default={expr}"));
        }
    }

    kwargs
}

pub(super) fn build_default(
    col_type: &ColumnType,
    sql: &str,
    used: &mut UsedImports,
) -> Option<String> {
    if sql.contains('(') {
        let up = sql.to_uppercase();
        return match col_type {
            ColumnType::Simple(SimpleColumnType::Timestamp | SimpleColumnType::Timestamptz)
                if up.contains("NOW") || up.contains("CURRENT_TIMESTAMP") =>
            {
                used.needs_timezone = true;
                Some("timezone.now".into())
            }
            ColumnType::Simple(SimpleColumnType::Uuid) => {
                used.needs_uuid_default = true;
                Some("uuid.uuid4".into())
            }
            _ => None,
        };
    }

    let up = sql.to_uppercase();
    if up == "TRUE" {
        return Some("True".into());
    }
    if up == "FALSE" {
        return Some("False".into());
    }

    if sql.starts_with('\'') && sql.ends_with('\'') && sql.len() >= 2 {
        let inner = &sql[1..sql.len() - 1];
        return Some(format!("\"{}\"", inner.replace('"', "\\\"")));
    }

    Some(sql.into())
}

pub(super) fn reference_action_str(action: &vespertide_core::ReferenceAction) -> &'static str {
    use vespertide_core::ReferenceAction;
    match action {
        ReferenceAction::Cascade => "models.CASCADE",
        ReferenceAction::Restrict => "models.RESTRICT",
        ReferenceAction::SetNull => "models.SET_NULL",
        ReferenceAction::SetDefault => "models.SET_DEFAULT",
        ReferenceAction::NoAction => "models.DO_NOTHING",
        _ => unreachable!("ReferenceAction is #[non_exhaustive]; all variants matched"),
    }
}
