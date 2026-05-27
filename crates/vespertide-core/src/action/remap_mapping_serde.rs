//! Custom (de)serialization for [`MigrationAction::RemapEnumValues::mapping`].
//!
//! The mapping is stored in memory as `BTreeMap<i64, i64>` so the type
//! system guarantees uniqueness of the source value — `{5: 10, 5: 20}` is
//! representable only by collapsing into a single entry. On the wire two
//! formats are accepted to preserve backward compatibility with v0.1.x
//! migration files:
//!
//! - **Map form** (canonical, current): `{"5": 10, "100": 20}` — JSON
//!   object with integer keys (`serde_json` stringifies on emit, parses
//!   back on read). YAML accepts integer keys verbatim. This is what
//!   [`serialize`] emits.
//! - **Array form** (legacy, accepted on read only):
//!   `[[5, 10], [100, 20]]` — `Vec<(i64, i64)>` pair list. Older
//!   migration JSON files written by vespertide 0.1.x use this shape;
//!   loading them keeps working without rewrites.
//!
//! Both shapes deserialize through the same [`MappingVisitor`] and arrive
//! at the same `BTreeMap`. Duplicate keys are rejected on the array path
//! (the only path where they are syntactically possible) so silently
//! shadowed mappings cannot slip in via hand-edited migration files.

use std::collections::BTreeMap;
use std::fmt;

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserializer, Serialize, Serializer};

/// Serialize a `BTreeMap<i64, i64>` as a JSON / YAML map. Always emits the
/// canonical map form — readers that only understand v0.1.x array form
/// must be upgraded.
pub fn serialize<S>(map: &BTreeMap<i64, i64>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    map.serialize(serializer)
}

/// Deserialize either a map or an array-of-pairs into `BTreeMap<i64, i64>`.
/// Dispatches on the actual JSON / YAML token via `deserialize_any` so the
/// caller does not need to declare which form they're sending.
pub fn deserialize<'de, D>(deserializer: D) -> Result<BTreeMap<i64, i64>, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_any(MappingVisitor)
}

struct MappingVisitor;

impl<'de> Visitor<'de> for MappingVisitor {
    type Value = BTreeMap<i64, i64>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a map of integer→integer entries or an array of [old, new] integer pairs")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        // Legacy v0.1.x array form: [[5, 10], [100, 20]].
        let mut map = BTreeMap::new();
        while let Some((old, new)) = seq.next_element::<(i64, i64)>()? {
            if map.insert(old, new).is_some() {
                return Err(de::Error::custom(format!(
                    "duplicate enum remap key: {old} appears more than once"
                )));
            }
        }
        Ok(map)
    }

    fn visit_map<A>(self, mut access: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        // Canonical map form: {"5": 10, "100": 20} (JSON) or `5: 10` (YAML).
        // serde_json transparently parses string keys back to i64; YAML
        // accepts integer keys verbatim. `BTreeMap` is naturally
        // duplicate-free, but we still surface the error so malformed
        // hand-edited files don't silently drop entries.
        let mut map = BTreeMap::new();
        while let Some((key, value)) = access.next_entry::<i64, i64>()? {
            if map.insert(key, value).is_some() {
                return Err(de::Error::custom(format!(
                    "duplicate enum remap key: {key} appears more than once"
                )));
            }
        }
        Ok(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Wrap {
        #[serde(with = "super")]
        mapping: BTreeMap<i64, i64>,
    }

    fn fixture() -> Wrap {
        let mut m = BTreeMap::new();
        m.insert(5, 100);
        m.insert(100, 101);
        Wrap { mapping: m }
    }

    // ── canonical (map) form ────────────────────────────────────────────

    #[test]
    fn serializes_to_map_form_in_json() {
        let json = serde_json::to_string(&fixture()).unwrap();
        // Order is BTreeMap-defined: 5 before 100.
        assert_eq!(json, r#"{"mapping":{"5":100,"100":101}}"#);
    }

    #[test]
    fn deserializes_map_form_from_json() {
        let json = r#"{"mapping":{"5":100,"100":101}}"#;
        let parsed: Wrap = serde_json::from_str(json).unwrap();
        assert_eq!(parsed, fixture());
    }

    // ── legacy (array) form on read ─────────────────────────────────────

    #[test]
    fn deserializes_legacy_array_form_from_json() {
        // Shape v0.1.x migrations were written with.
        let json = r#"{"mapping":[[5,100],[100,101]]}"#;
        let parsed: Wrap = serde_json::from_str(json).unwrap();
        assert_eq!(parsed, fixture());
    }

    // ── round-trip ──────────────────────────────────────────────────────

    #[test]
    fn map_form_round_trip_is_stable_in_json() {
        let json = serde_json::to_string(&fixture()).unwrap();
        let parsed: Wrap = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&parsed).unwrap();
        assert_eq!(json, json2);
    }

    // ── duplicate detection ─────────────────────────────────────────────

    #[test]
    fn rejects_duplicate_keys_in_legacy_array_form() {
        // Two entries for `5` — possible in array form, impossible in map form.
        let json = r#"{"mapping":[[5,100],[5,200]]}"#;
        let err = serde_json::from_str::<Wrap>(json).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("duplicate enum remap key"),
            "expected duplicate-key error, got: {msg}"
        );
    }

    #[test]
    fn empty_mapping_round_trips() {
        let empty = Wrap {
            mapping: BTreeMap::new(),
        };
        let json = serde_json::to_string(&empty).unwrap();
        assert_eq!(json, r#"{"mapping":{}}"#);
        let parsed: Wrap = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, empty);
    }
}
