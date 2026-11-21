//! Canonical JSON, byte-identical to the core's Python:
//! `json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False)`
//! encoded as UTF-8 (`skeleton/system/packaging/primitives.py`).
//!
//! Keys are sorted explicitly (not trusting the map type, whose order a dependency feature could
//! change). Strings use serde_json's escaping, which matches Python with `ensure_ascii=False`:
//! `\"`, `\\`, `\b`, `\f`, `\n`, `\r`, `\t`, other control characters as lowercase `\u00xx`, and
//! everything else verbatim. Floats are refused: payloads only carry strings and integers.

use serde_json::Value;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("value cannot be written as canonical JSON (floats are not allowed)")]
pub struct NotCanonical;

pub fn to_canonical_bytes(value: &Value) -> Result<Vec<u8>, NotCanonical> {
    let mut out = String::new();
    write(value, &mut out)?;
    Ok(out.into_bytes())
}

fn write(value: &Value, out: &mut String) -> Result<(), NotCanonical> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Value::Number(number) => {
            if number.is_f64() {
                return Err(NotCanonical);
            }
            out.push_str(&number.to_string());
        }
        Value::String(text) => {
            out.push_str(&serde_json::to_string(text).map_err(|_| NotCanonical)?)
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write(item, out)?;
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            // Rust compares strings by UTF-8 bytes, which orders like Python's code points.
            keys.sort();
            out.push('{');
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key).map_err(|_| NotCanonical)?);
                out.push(':');
                write(&map[key], out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sorts_keys_and_drops_whitespace() {
        let value = json!({"b": 1, "a": [true, null, "x"], "C": {"z": 0, "y": -2}});
        assert_eq!(
            String::from_utf8(to_canonical_bytes(&value).unwrap()).unwrap(),
            r#"{"C":{"y":-2,"z":0},"a":[true,null,"x"],"b":1}"#
        );
    }

    #[test]
    fn keeps_unicode_and_escapes_like_python() {
        // Python: json.dumps({"k": "Katuá\n\u0001\"/"}, ensure_ascii=False, separators=(",", ":"))
        let value = json!({"k": "Katuá\n\u{1}\"/"});
        assert_eq!(
            String::from_utf8(to_canonical_bytes(&value).unwrap()).unwrap(),
            "{\"k\":\"Katuá\\n\\u0001\\\"/\"}"
        );
    }

    #[test]
    fn refuses_floats() {
        assert_eq!(to_canonical_bytes(&json!({"x": 1.5})), Err(NotCanonical));
    }
}
