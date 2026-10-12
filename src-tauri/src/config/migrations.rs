//! Config schema migrations (NFR-3).
//!
//! Spec: `.local-doc/Workspace.md` §FR-2 / NFR-3.

use serde_json::{json, Map, Value};

/// Current persisted schema version.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Apply migrations in order until `CURRENT_SCHEMA_VERSION` (idempotent).
pub fn migrate_config_value(mut value: Value) -> Value {
    let version = value
        .as_object()
        .and_then(|o| o.get("schema_version"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    if version == 0 {
        value = migrate_v0_to_v1(value);
    }

    if let Some(obj) = value.as_object_mut() {
        obj.insert("schema_version".into(), Value::from(CURRENT_SCHEMA_VERSION));
    }
    value
}

/// v0 → v1: add `workspace = General`, `advanced = default` (NFR-3).
pub fn migrate_v0_to_v1(mut value: Value) -> Value {
    let obj = match value.as_object_mut() {
        Some(o) => o,
        None => {
            let mut m = Map::new();
            m.insert("schema_version".into(), json!(1));
            m.insert("workspace".into(), json!("general"));
            m.insert("advanced".into(), json!({ "tiling_advanced": false }));
            return Value::Object(m);
        }
    };

    if !obj.contains_key("workspace") {
        obj.insert("workspace".into(), json!("general"));
    }
    if !obj.contains_key("advanced") {
        obj.insert("advanced".into(), json!({ "tiling_advanced": false }));
    } else if let Some(adv) = obj.get_mut("advanced") {
        if let Some(a) = adv.as_object_mut() {
            a.entry("tiling_advanced".to_string())
                .or_insert(json!(false));
        }
    }
    obj.insert("schema_version".into(), json!(1));
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_v0_to_v1_idempotent() {
        let v = json!({"foo": 1});
        let once = migrate_config_value(v);
        let twice = migrate_config_value(once.clone());
        assert_eq!(once, twice);
        assert_eq!(once["schema_version"], 1);
        assert_eq!(once["workspace"], "general");
        assert_eq!(once["advanced"]["tiling_advanced"], false);
        assert_eq!(once["foo"], 1);
    }
}
