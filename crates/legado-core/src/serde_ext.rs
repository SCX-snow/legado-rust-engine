use std::collections::HashMap;

use serde::de::{Deserializer, Error as _};
use serde::ser::{Error as _, Serializer};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub fn default_true() -> bool {
    true
}

pub fn is_empty<K, V>(map: &HashMap<K, V>) -> bool {
    map.is_empty()
}

fn string_value(value: Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => Some(text),
        other => Some(other.to_string()),
    }
}

fn boolean_value(value: Value) -> Option<bool> {
    Some(match value {
        Value::Null => return None,
        Value::Bool(value) => value,
        Value::Number(number) => number.as_f64().unwrap_or_default() != 0.0,
        Value::String(text) => matches!(
            text.trim().to_ascii_lowercase().as_str(),
            "true" | "1" | "yes"
        ),
        _ => false,
    })
}

macro_rules! integer {
    ($name:ident, $type:ty) => {
        pub mod $name {
            use super::*;

            pub fn deserialize<'de, D: Deserializer<'de>>(
                deserializer: D,
            ) -> Result<$type, D::Error> {
                Ok(match Value::deserialize(deserializer)? {
                    Value::Number(number) => number
                        .as_i64()
                        .or_else(|| number.as_f64().map(|value| value as i64))
                        .unwrap_or_default() as $type,
                    Value::String(text) => text.trim().parse().unwrap_or_default(),
                    Value::Bool(value) => value as $type,
                    _ => 0,
                })
            }

            pub fn serialize<S: Serializer>(
                value: &$type,
                serializer: S,
            ) -> Result<S::Ok, S::Error> {
                serializer.serialize_i64(*value as i64)
            }
        }
    };
}

integer!(i32, i32);
integer!(i64, i64);

pub mod text {
    use super::*;

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
        Ok(string_value(Value::deserialize(deserializer)?).unwrap_or_default())
    }

    pub fn serialize<S: Serializer>(value: &str, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(value)
    }
}

pub mod opt_text {
    use super::*;

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<String>, D::Error> {
        Ok(string_value(Value::deserialize(deserializer)?))
    }

    pub fn serialize<S: Serializer>(
        value: &Option<String>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(text) => serializer.serialize_str(text),
            None => serializer.serialize_none(),
        }
    }
}

pub mod boolean {
    use super::*;

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
        Ok(boolean_value(Value::deserialize(deserializer)?).unwrap_or_default())
    }

    pub fn serialize<S: Serializer>(value: &bool, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bool(*value)
    }
}

pub mod opt_boolean {
    use super::*;

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<bool>, D::Error> {
        Ok(boolean_value(Value::deserialize(deserializer)?))
    }

    pub fn serialize<S: Serializer>(
        value: &Option<bool>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(value) => serializer.serialize_bool(*value),
            None => serializer.serialize_none(),
        }
    }
}

pub mod rule {
    use super::*;

    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: serde::de::DeserializeOwned,
    {
        match Value::deserialize(deserializer)? {
            Value::Null => Ok(None),
            Value::String(text) if text.trim().is_empty() => Ok(None),
            Value::String(text) => serde_json::from_str(&text)
                .map(Some)
                .map_err(D::Error::custom),
            value @ Value::Object(_) => serde_json::from_value(value)
                .map(Some)
                .map_err(D::Error::custom),
            _ => Ok(None),
        }
    }

    pub fn serialize<S, T>(value: &Option<T>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: Serialize,
    {
        match value {
            Some(value) => value.serialize(serializer),
            None => serializer.serialize_none(),
        }
    }
}

pub mod variables {
    use super::*;

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<HashMap<String, String>, D::Error> {
        Ok(match Value::deserialize(deserializer)? {
            Value::String(text) => serde_json::from_str(&text).unwrap_or_default(),
            value @ Value::Object(_) => serde_json::from_value(value).unwrap_or_default(),
            _ => HashMap::new(),
        })
    }

    pub fn serialize<S: Serializer>(
        variables: &HashMap<String, String>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let text = serde_json::to_string(variables).map_err(S::Error::custom)?;
        serializer.serialize_str(&text)
    }
}
