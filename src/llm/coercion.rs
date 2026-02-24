use serde_json::Value;

/// Coerce a JSON value to match a target JSON Schema type.
///
/// When LLM tool arguments arrive as strings but the schema expects a
/// different type (number, integer, boolean), this function attempts the
/// conversion. Non-string values and unrecognised target types are returned
/// unchanged.
///
/// # Supported targets
///
/// - `"number"` -- parse string as `f64`
/// - `"integer"` -- parse string as `i64`
/// - `"boolean"` -- `"true"` / `"1"` -> `true`, `"false"` / `"0"` -> `false`
/// - `"null"` -- `"null"` / `""` -> `Value::Null`
/// - anything else -- returned as-is
#[must_use]
pub fn coerce_value(value: &Value, target_type: &str) -> Value {
    let Value::String(s) = value else {
        return value.clone();
    };

    match target_type {
        "number" => coerce_number(s).unwrap_or_else(|| value.clone()),
        "integer" => coerce_integer(s).unwrap_or_else(|| value.clone()),
        "boolean" => coerce_boolean(s).unwrap_or_else(|| value.clone()),
        "null" => coerce_null(s).unwrap_or_else(|| value.clone()),
        _ => value.clone(),
    }
}

/// Coerce all string values in `args` whose keys have a matching entry in
/// `schema_properties` with a declared `"type"` field.
///
/// This is the convenience entry-point for processing an entire tool argument
/// object against its JSON Schema `properties` definition.
#[must_use]
pub fn coerce_arguments(args: &Value, schema_properties: &Value) -> Value {
    let (Some(args_obj), Some(props_obj)) = (args.as_object(), schema_properties.as_object())
    else {
        return args.clone();
    };

    let mut result = serde_json::Map::new();

    for (key, value) in args_obj {
        let coerced = if let Some(prop_schema) = props_obj.get(key.as_str()) {
            if let Some(target_type) = prop_schema.get("type").and_then(Value::as_str) {
                coerce_value(value, target_type)
            } else {
                value.clone()
            }
        } else {
            value.clone()
        };
        result.insert(key.clone(), coerced);
    }

    Value::Object(result)
}

fn coerce_number(s: &str) -> Option<Value> {
    s.parse::<f64>()
        .ok()
        .and_then(|n| serde_json::Number::from_f64(n).map(Value::Number))
}

fn coerce_integer(s: &str) -> Option<Value> {
    s.parse::<i64>().ok().map(|n| Value::Number(n.into()))
}

fn coerce_boolean(s: &str) -> Option<Value> {
    match s.trim().to_lowercase().as_str() {
        "true" | "1" | "yes" => Some(Value::Bool(true)),
        "false" | "0" | "no" => Some(Value::Bool(false)),
        _ => None,
    }
}

fn coerce_null(s: &str) -> Option<Value> {
    match s.trim().to_lowercase().as_str() {
        "null" | "" => Some(Value::Null),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{coerce_arguments, coerce_value};
    use serde_json::json;

    #[test]
    fn coerce_string_to_number() {
        let value = json!("3.14");
        let coerced = coerce_value(&value, "number");
        assert_eq!(coerced, json!(3.14));
    }

    #[test]
    fn coerce_string_to_integer() {
        let value = json!("42");
        let coerced = coerce_value(&value, "integer");
        assert_eq!(coerced, json!(42));
    }

    #[test]
    fn coerce_string_to_boolean_true() {
        for input in &["true", "1", "yes", "True", "YES"] {
            let value = json!(input);
            let coerced = coerce_value(&value, "boolean");
            assert_eq!(coerced, json!(true), "failed for input: {input}");
        }
    }

    #[test]
    fn coerce_string_to_boolean_false() {
        for input in &["false", "0", "no", "False", "NO"] {
            let value = json!(input);
            let coerced = coerce_value(&value, "boolean");
            assert_eq!(coerced, json!(false), "failed for input: {input}");
        }
    }

    #[test]
    fn coerce_string_to_null() {
        let value = json!("null");
        let coerced = coerce_value(&value, "null");
        assert_eq!(coerced, json!(null));

        let empty = json!("");
        let coerced = coerce_value(&empty, "null");
        assert_eq!(coerced, json!(null));
    }

    #[test]
    fn non_string_values_pass_through() {
        let number = json!(42);
        assert_eq!(coerce_value(&number, "number"), json!(42));

        let boolean = json!(true);
        assert_eq!(coerce_value(&boolean, "boolean"), json!(true));

        let null = json!(null);
        assert_eq!(coerce_value(&null, "null"), json!(null));
    }

    #[test]
    fn invalid_string_returns_original() {
        let value = json!("not_a_number");
        assert_eq!(coerce_value(&value, "number"), json!("not_a_number"));

        let value = json!("maybe");
        assert_eq!(coerce_value(&value, "boolean"), json!("maybe"));
    }

    #[test]
    fn unknown_target_type_returns_original() {
        let value = json!("hello");
        assert_eq!(coerce_value(&value, "array"), json!("hello"));
        assert_eq!(coerce_value(&value, "object"), json!("hello"));
        assert_eq!(coerce_value(&value, "string"), json!("hello"));
    }

    #[test]
    fn coerce_arguments_processes_object() {
        let args = json!({
            "count": "5",
            "rate": "0.75",
            "verbose": "true",
            "name": "test"
        });
        let schema_properties = json!({
            "count": {"type": "integer"},
            "rate": {"type": "number"},
            "verbose": {"type": "boolean"},
            "name": {"type": "string"}
        });

        let coerced = coerce_arguments(&args, &schema_properties);
        assert_eq!(coerced["count"], json!(5));
        assert_eq!(coerced["rate"], json!(0.75));
        assert_eq!(coerced["verbose"], json!(true));
        assert_eq!(coerced["name"], json!("test"));
    }

    #[test]
    fn coerce_arguments_non_object_returns_clone() {
        let args = json!("not an object");
        let schema = json!({"count": {"type": "integer"}});
        assert_eq!(coerce_arguments(&args, &schema), json!("not an object"));
    }

    #[test]
    fn coerce_arguments_missing_schema_key_passes_through() {
        let args = json!({"unknown_key": "42"});
        let schema = json!({"count": {"type": "integer"}});
        let coerced = coerce_arguments(&args, &schema);
        assert_eq!(coerced["unknown_key"], json!("42"));
    }

    #[test]
    fn coerce_negative_integer() {
        let value = json!("-10");
        assert_eq!(coerce_value(&value, "integer"), json!(-10));
    }

    #[test]
    fn coerce_negative_number() {
        let value = json!("-2.5");
        assert_eq!(coerce_value(&value, "number"), json!(-2.5));
    }

    #[test]
    fn coerce_scientific_notation_number() {
        let value = json!("1e10");
        let coerced = coerce_value(&value, "number");
        assert_eq!(coerced, json!(1e10));
    }
}
