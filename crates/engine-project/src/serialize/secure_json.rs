//! Strict JSON parse for archive payloads (SPEC §7.1).
//!
//! - Max nesting depth (default 64)
//! - Duplicate object keys rejected (serde_json keeps the last by default)
//! - No filesystem / network side effects

use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use std::collections::HashSet;
use std::fmt;
use thiserror::Error;

/// Default max nesting depth for archive JSON (SPEC §7.1).
pub const MAX_JSON_DEPTH: usize = 64;

/// Errors from the strict JSON loader.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SecureJsonError {
    #[error("JSON nesting exceeds max depth {0}")]
    DepthExceeded(usize),

    #[error("duplicate JSON key: {0}")]
    DuplicateKey(String),

    #[error("invalid JSON: {0}")]
    Invalid(String),
}

/// Parse JSON bytes into a [`Value`] with depth + duplicate-key checks.
pub fn parse_value(data: &[u8], max_depth: usize) -> Result<Value, SecureJsonError> {
    let mut de = serde_json::Deserializer::from_slice(data);
    let value = DepthSeed {
        max_depth,
        depth: 0,
    }
    .deserialize(&mut de)
    .map_err(map_de_error)?;
    de.end().map_err(map_de_error)?;
    Ok(value)
}

/// Convenience: parse then `serde_json::from_value`.
pub fn parse_as<T: serde::de::DeserializeOwned>(
    data: &[u8],
    max_depth: usize,
) -> Result<T, SecureJsonError> {
    let value = parse_value(data, max_depth)?;
    serde_json::from_value(value).map_err(|e| SecureJsonError::Invalid(e.to_string()))
}

fn map_de_error(err: serde_json::Error) -> SecureJsonError {
    let msg = err.to_string();
    let core = msg.split(" at line ").next().unwrap_or(&msg);
    if let Some(rest) = core.strip_prefix("duplicate key: ") {
        return SecureJsonError::DuplicateKey(rest.to_string());
    }
    if let Some(rest) = core.strip_prefix("depth exceeded: ") {
        if let Ok(d) = rest.parse::<usize>() {
            return SecureJsonError::DepthExceeded(d);
        }
    }
    SecureJsonError::Invalid(msg)
}

struct DepthSeed {
    max_depth: usize,
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for DepthSeed {
    type Value = Value;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(DepthVisitor {
            max_depth: self.max_depth,
            depth: self.depth,
        })
    }
}

struct DepthVisitor {
    max_depth: usize,
    depth: usize,
}

impl DepthVisitor {
    fn enter_child(&self) -> Result<(), String> {
        let next = self.depth.saturating_add(1);
        if next > self.max_depth {
            return Err(format!("depth exceeded: {}", self.max_depth));
        }
        Ok(())
    }

    fn child_seed(&self) -> DepthSeed {
        DepthSeed {
            max_depth: self.max_depth,
            depth: self.depth + 1,
        }
    }
}

impl<'de> Visitor<'de> for DepthVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "any JSON value")
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
        Ok(Value::Number(v.into()))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
        Ok(Value::Number(v.into()))
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
        serde_json::Number::from_f64(v)
            .map(Value::Number)
            .ok_or_else(|| de::Error::custom("non-finite JSON number"))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        Ok(Value::String(v.to_owned()))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
        Ok(Value::String(v))
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        self.enter_child().map_err(de::Error::custom)?;
        let child = self.child_seed();
        let mut out = Vec::new();
        while let Some(v) = seq.next_element_seed(DepthSeed {
            max_depth: child.max_depth,
            depth: child.depth,
        })? {
            out.push(v);
        }
        Ok(Value::Array(out))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        self.enter_child().map_err(de::Error::custom)?;
        let child = self.child_seed();
        let mut out = serde_json::Map::new();
        let mut seen = HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !seen.insert(key.clone()) {
                return Err(de::Error::custom(format!("duplicate key: {key}")));
            }
            let value = map.next_value_seed(DepthSeed {
                max_depth: child.max_depth,
                depth: child.depth,
            })?;
            out.insert(key, value);
        }
        Ok(Value::Object(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_normal_object() {
        let v = parse_value(br#"{"a":1,"b":[true,null]}"#, MAX_JSON_DEPTH).unwrap();
        assert_eq!(v["a"], 1);
        assert_eq!(v["b"][0], true);
    }

    #[test]
    fn rejects_duplicate_keys() {
        let err = parse_value(br#"{"a":1,"a":2}"#, MAX_JSON_DEPTH).unwrap_err();
        assert!(
            matches!(err, SecureJsonError::DuplicateKey(ref k) if k == "a"),
            "{err:?}"
        );
    }

    #[test]
    fn rejects_deep_nesting() {
        let mut s = String::new();
        for _ in 0..70 {
            s.push('[');
        }
        s.push('1');
        for _ in 0..70 {
            s.push(']');
        }
        let err = parse_value(s.as_bytes(), MAX_JSON_DEPTH).unwrap_err();
        assert!(matches!(err, SecureJsonError::DepthExceeded(_)), "{err:?}");
    }

    #[test]
    fn depth_limit_is_configurable() {
        let err = parse_value(br#"[[[[1]]]]"#, 2).unwrap_err();
        assert!(matches!(err, SecureJsonError::DepthExceeded(2)));
    }
}
