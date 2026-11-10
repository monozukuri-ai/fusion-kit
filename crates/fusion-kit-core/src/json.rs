//! Strict sidecar JSON: reject duplicate keys at every depth, including unknown fields.
use std::collections::BTreeMap;
use std::fmt;

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::Value;

use crate::{Error, ErrorKind};

struct Unique(Value);
impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Unique;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("JSON value with unique object keys")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Unique, E> {
                serde_json::Number::from_f64(v)
                    .map(|v| Unique(Value::Number(v)))
                    .ok_or_else(|| E::custom("non-finite JSON number"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Unique, E> {
                Ok(Unique(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Unique, A::Error> {
                let mut values = Vec::new();
                while let Some(Unique(v)) = a.next_element()? {
                    values.push(v);
                }
                Ok(Unique(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Unique, A::Error> {
                let mut values = BTreeMap::new();
                while let Some(key) = a.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(de::Error::custom(format!("duplicate JSON key: {key}")));
                    }
                    let Unique(v) = a.next_value()?;
                    values.insert(key, v);
                }
                Ok(Unique(Value::Object(values.into_iter().collect())))
            }
        }
        d.deserialize_any(V)
    }
}

pub(crate) fn parse(data: &[u8], member: &str) -> Result<Value, Error> {
    // Bound values and keys before allocating a serde tree. Punctuation inside
    // strings is ignored; serde performs all grammar and UTF-8 checks below.
    let (mut quoted, mut escaped, mut scalar, mut tokens) = (false, false, false, 0_u32);
    for &b in data {
        if quoted {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                quoted = false;
            }
        } else {
            match b {
                b'"' => {
                    quoted = true;
                    scalar = false;
                    tokens += 1;
                }
                b'{' | b'[' => {
                    scalar = false;
                    tokens += 1;
                }
                b'}' | b']' | b',' | b':' | b' ' | b'\t' | b'\r' | b'\n' => {
                    scalar = false;
                }
                _ if !scalar => {
                    tokens += 1;
                    scalar = true;
                }
                _ => {}
            }
        }
        if tokens > 100_000 {
            return Err(Error::new(
                ErrorKind::ResourceLimit,
                "limit.json_values",
                "package_json",
                "JSON exceeds 100000 values and keys",
            )
            .member(member));
        }
    }
    let mut decoder = serde_json::Deserializer::from_slice(data);
    let parsed = Unique::deserialize(&mut decoder).and_then(|value| {
        decoder.end()?;
        Ok(value.0)
    });
    let value = parsed.map_err(|e| {
        Error::new(
            ErrorKind::Format,
            "json.invalid",
            "package_json",
            e.to_string(),
        )
        .member(member)
    })?;
    // The byte cap bounds allocations during decoding, serde's depth cap remains on.
    let mut pending = vec![&value];
    let mut count = 0;
    while let Some(v) = pending.pop() {
        count += 1;
        if count > 100_000 {
            return Err(Error::new(
                ErrorKind::ResourceLimit,
                "limit.json_values",
                "package_json",
                "JSON exceeds 100000 values",
            )
            .member(member));
        }
        match v {
            Value::Array(a) => pending.extend(a),
            Value::Object(o) => pending.extend(o.values()),
            _ => {}
        }
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_unknown_fields_and_trailing_bytes_fail() {
        for data in [
            br#"{"unknown":{"a":1,"a":2}}"#.as_slice(),
            b"{} {}",
            b"NaN",
            b"[true",
        ] {
            assert_eq!(
                parse(data, "sidecar").unwrap_err().diagnostic.code,
                "json.invalid"
            );
        }
    }
    #[test]
    fn bool_is_not_an_integer_and_unicode_survives() {
        let v = parse("{\"id\":true,\"name\":\"モデル\"}".as_bytes(), "sidecar").unwrap();
        assert_eq!(v["id"].as_i64(), None);
        assert_eq!(v["name"], "モデル");
    }
}
