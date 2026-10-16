//! Config schema migrations (NFR-3).
//!
//! Spec: `.local-doc/Workspace.md` §FR-2 / NFR-3.

use serde_json::{json, Map, Value};

/// Current persisted schema version.
pub const CURRENT_SCHEMA_VERSION: u32 = 2;

/// Apply migrations in order until `CURRENT_SCHEMA_VERSION` (idempotent).
///
/// Returns `(value, migrated)` where `migrated` is true when any step ran or
/// the on-disk schema version differed from [`CURRENT_SCHEMA_VERSION`].
pub fn migrate_config_value(mut value: Value) -> (Value, bool) {
    let version = value
        .as_object()
        .and_then(|o| o.get("schema_version"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let mut migrated = version != CURRENT_SCHEMA_VERSION;

    if version < 1 {
        value = migrate_v0_to_v1(value);
        migrated = true;
    }
    if version < 2 {
        value = migrate_v1_to_v2(value);
        migrated = true;
    }

    if let Some(obj) = value.as_object_mut() {
        obj.insert("schema_version".into(), Value::from(CURRENT_SCHEMA_VERSION));
    }
    (value, migrated)
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

/// v1 → v2: ship GPU dither families as a normal selectable option.
///
/// Prior builds defaulted `experimental.gpu_dither_families` to false and wrote
/// that into `config.json`, so the Error Diffusion — GPU family stayed
/// unavailable with no in-app toggle. Enable it on upgrade (no Preferences UI
/// ever turned it off).
pub fn migrate_v1_to_v2(mut value: Value) -> Value {
    let obj = match value.as_object_mut() {
        Some(o) => o,
        None => return value,
    };
    match obj.get_mut("experimental") {
        Some(exp) => {
            if let Some(e) = exp.as_object_mut() {
                e.insert("gpu_dither_families".into(), json!(true));
            }
        }
        None => {
            obj.insert(
                "experimental".into(),
                json!({ "gpu_dither_families": true }),
            );
        }
    }
    obj.insert("schema_version".into(), json!(2));
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_v0_to_v1_idempotent() {
        let v = json!({"foo": 1});
        let (once, migrated) = migrate_config_value(v);
        assert!(migrated);
        let (twice, migrated2) = migrate_config_value(once.clone());
        assert!(!migrated2);
        assert_eq!(once, twice);
        assert_eq!(once["schema_version"], 2);
        assert_eq!(once["workspace"], "general");
        assert_eq!(once["advanced"]["tiling_advanced"], false);
        assert_eq!(once["experimental"]["gpu_dither_families"], true);
        assert_eq!(once["foo"], 1);
    }

    #[test]
    fn migrate_v1_enables_gpu_dither_families() {
        let v = json!({
            "schema_version": 1,
            "workspace": "general",
            "experimental": { "gpu_dither_families": false }
        });
        let (out, migrated) = migrate_config_value(v);
        assert!(migrated);
        assert_eq!(out["schema_version"], 2);
        assert_eq!(out["experimental"]["gpu_dither_families"], true);
    }
}
