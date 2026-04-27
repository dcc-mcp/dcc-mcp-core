//! Rust-powered YAML <-> JSON conversion helpers.
//!
//! `yaml_loads` / `yaml_dumps` `#[pyfunction]` exports live in
//! `crate::python::py_yaml`. The conversion helpers below remain here so
//! other crates can call them directly.

/// Convert a `serde_yaml_ng::Value` to a `serde_json::Value`.
pub(crate) fn yaml_value_to_json(val: serde_yaml_ng::Value) -> serde_json::Value {
    match val {
        serde_yaml_ng::Value::Null => serde_json::Value::Null,
        serde_yaml_ng::Value::Bool(b) => serde_json::Value::Bool(b),
        serde_yaml_ng::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                serde_json::Value::Number(i.into())
            } else if let Some(f) = n.as_f64() {
                serde_json::json!(f)
            } else {
                serde_json::Value::Null
            }
        }
        serde_yaml_ng::Value::String(s) => serde_json::Value::String(s),
        serde_yaml_ng::Value::Sequence(seq) => {
            serde_json::Value::Array(seq.into_iter().map(yaml_value_to_json).collect())
        }
        serde_yaml_ng::Value::Mapping(map) => {
            let mut obj = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                let key = match k {
                    serde_yaml_ng::Value::String(s) => s,
                    serde_yaml_ng::Value::Number(n) => n.to_string(),
                    serde_yaml_ng::Value::Bool(b) => b.to_string(),
                    _ => continue,
                };
                obj.insert(key, yaml_value_to_json(v));
            }
            serde_json::Value::Object(obj)
        }
        serde_yaml_ng::Value::Tagged(tagged) => yaml_value_to_json(tagged.value),
    }
}

/// Convert a `serde_json::Value` to a `serde_yaml_ng::Value`.
pub(crate) fn json_value_to_yaml(val: &serde_json::Value) -> serde_yaml_ng::Value {
    match val {
        serde_json::Value::Null => serde_yaml_ng::Value::Null,
        serde_json::Value::Bool(b) => serde_yaml_ng::Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                serde_yaml_ng::Value::Number(i.into())
            } else if let Some(f) = n.as_f64() {
                serde_yaml_ng::Value::Number(f.into())
            } else {
                serde_yaml_ng::Value::Null
            }
        }
        serde_json::Value::String(s) => serde_yaml_ng::Value::String(s.clone()),
        serde_json::Value::Array(arr) => {
            serde_yaml_ng::Value::Sequence(arr.iter().map(json_value_to_yaml).collect())
        }
        serde_json::Value::Object(obj) => {
            let mut map = serde_yaml_ng::Mapping::with_capacity(obj.len());
            for (k, v) in obj {
                map.insert(
                    serde_yaml_ng::Value::String(k.clone()),
                    json_value_to_yaml(v),
                );
            }
            serde_yaml_ng::Value::Mapping(map)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_yaml_loads_basic() {
        let py_json = yaml_value_to_json(serde_yaml_ng::from_str("name: test\nvalue: 42").unwrap());
        assert_eq!(py_json["name"], "test");
        assert_eq!(py_json["value"], 42);
    }

    #[test]
    fn test_yaml_loads_nested() {
        let py_json = yaml_value_to_json(
            serde_yaml_ng::from_str("config:\n  dcc: maya\n  port: 8080").unwrap(),
        );
        assert_eq!(py_json["config"]["dcc"], "maya");
        assert_eq!(py_json["config"]["port"], 8080);
    }

    #[test]
    fn test_yaml_loads_list() {
        let py_json = yaml_value_to_json(
            serde_yaml_ng::from_str("items:\n  - one\n  - two\n  - three").unwrap(),
        );
        assert_eq!(py_json["items"][0], "one");
        assert_eq!(py_json["items"][2], "three");
    }

    #[test]
    fn test_round_trip() {
        let yaml_str = "name: test\nvalue: 42\n";
        let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(yaml_str).unwrap();
        let json_val = yaml_value_to_json(value);
        let yaml_val = json_value_to_yaml(&json_val);
        let output = serde_yaml_ng::to_string(&yaml_val).unwrap();
        let reparsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&output).unwrap();
        let json_reparsed = yaml_value_to_json(reparsed);
        assert_eq!(json_reparsed["name"], "test");
        assert_eq!(json_reparsed["value"], 42);
    }
}
