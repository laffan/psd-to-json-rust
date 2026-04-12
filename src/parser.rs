use serde_json::Value;
use std::collections::HashMap;

/// Parsed layer name information.
#[derive(Debug, Clone)]
pub struct ParsedLayer {
    pub category: String,
    pub name: String,
    pub layer_type: Option<String>,
    pub attributes: HashMap<String, Value>,
}

/// Parse a pipe-delimited layer name into structured data.
///
/// Format: `CATEGORY | NAME | TYPE | ATTRIBUTES`
/// - 0 pipes: layer ignored (returns None)
/// - 1 pipe:  category + name
/// - 2 pipes: category + name + attributes
/// - 3 pipes: category + name + type + attributes
pub fn parse_layer_name(name: &str) -> Option<ParsedLayer> {
    let parts: Vec<&str> = name.split('|').map(str::trim).collect();

    if parts.len() < 2 || parts.len() > 4 {
        return None;
    }

    let category_char = parts[0].to_uppercase();
    let category = match category_char.as_str() {
        "Z" => "zone",
        "P" => "point",
        "S" => "sprite",
        "T" => "tileset",
        "G" => "group",
        _ => return None,
    };

    let mut result = ParsedLayer {
        category: category.to_string(),
        name: parts[1].to_string(),
        layer_type: None,
        attributes: HashMap::new(),
    };

    match parts.len() {
        3 => {
            if let Some(attrs) = parse_attribute_string(parts[2]) {
                result.attributes = attrs;
            }
        }
        4 => {
            result.layer_type = Some(parts[2].to_string());
            if let Some(attrs) = parse_attribute_string(parts[3]) {
                result.attributes = attrs;
            }
        }
        _ => {}
    }

    Some(result)
}

/// Parse a comma-separated attribute string into a map of key:value pairs.
fn parse_attribute_string(attr_str: &str) -> Option<HashMap<String, Value>> {
    let attr_str = attr_str.trim();
    if attr_str.is_empty() {
        return None;
    }

    let mut attributes = HashMap::new();

    // Split on commas that are not inside brackets
    for item in split_attributes(attr_str) {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }

        if let Some(colon_pos) = item.find(':') {
            let key = item[..colon_pos].trim().to_string();
            let value_str = item[colon_pos + 1..].trim();
            if !key.is_empty() {
                let value = parse_value(value_str);
                attributes.insert(key, value);
            }
        } else {
            // Standalone attribute -> boolean true
            attributes.insert(item.to_string(), Value::Bool(true));
        }
    }

    if attributes.is_empty() {
        None
    } else {
        Some(attributes)
    }
}

/// Split attribute string by commas, respecting brackets and quotes.
fn split_attributes(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut bracket_depth = 0;
    let mut in_quotes = false;

    for ch in s.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                current.push(ch);
            }
            '[' if !in_quotes => {
                bracket_depth += 1;
                current.push(ch);
            }
            ']' if !in_quotes => {
                bracket_depth -= 1;
                current.push(ch);
            }
            '{' if !in_quotes => {
                bracket_depth += 1;
                current.push(ch);
            }
            '}' if !in_quotes => {
                bracket_depth -= 1;
                current.push(ch);
            }
            ',' if !in_quotes && bracket_depth == 0 => {
                parts.push(current.clone());
                current.clear();
            }
            _ => {
                current.push(ch);
            }
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

/// Parse a single value string into a JSON Value.
fn parse_value(s: &str) -> Value {
    let s = s.trim();
    if s.is_empty() {
        return Value::Null;
    }

    // Quoted string
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        return Value::String(s[1..s.len() - 1].replace("\\\"", "\""));
    }

    // Array
    if s.starts_with('[') && s.ends_with(']') {
        return parse_array(s);
    }

    // Object
    if s.starts_with('{') && s.ends_with('}') {
        return parse_object(s);
    }

    // Boolean
    if s.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if s.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }

    // Integer
    if let Ok(i) = s.parse::<i64>() {
        return Value::Number(i.into());
    }

    // Float
    if let Ok(f) = s.parse::<f64>() {
        if let Some(n) = serde_json::Number::from_f64(f) {
            return Value::Number(n);
        }
    }

    // Bare string
    Value::String(s.to_string())
}

fn parse_array(s: &str) -> Value {
    let inner = &s[1..s.len() - 1];
    let elements = split_attributes(inner);
    let values: Vec<Value> = elements.iter().map(|e| parse_value(e.trim())).collect();
    Value::Array(values)
}

fn parse_object(s: &str) -> Value {
    let inner = &s[1..s.len() - 1];
    let mut map = serde_json::Map::new();
    for item in split_attributes(inner) {
        let item = item.trim();
        if let Some(colon_pos) = item.find(':') {
            let key = item[..colon_pos].trim().to_string();
            let val = parse_value(item[colon_pos + 1..].trim());
            map.insert(key, val);
        }
    }
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_pipes_ignored() {
        assert!(parse_layer_name("Background").is_none());
    }

    #[test]
    fn test_one_pipe() {
        let result = parse_layer_name("S | player").unwrap();
        assert_eq!(result.category, "sprite");
        assert_eq!(result.name, "player");
        assert!(result.layer_type.is_none());
    }

    #[test]
    fn test_two_pipes_with_attributes() {
        let result = parse_layer_name("P | spawn | level:5, active").unwrap();
        assert_eq!(result.category, "point");
        assert_eq!(result.name, "spawn");
        assert_eq!(result.attributes.get("level"), Some(&Value::from(5)));
        assert_eq!(result.attributes.get("active"), Some(&Value::Bool(true)));
    }

    #[test]
    fn test_three_pipes() {
        let result = parse_layer_name("S | hero | animation | frames:8").unwrap();
        assert_eq!(result.category, "sprite");
        assert_eq!(result.name, "hero");
        assert_eq!(result.layer_type.as_deref(), Some("animation"));
        assert_eq!(result.attributes.get("frames"), Some(&Value::from(8)));
    }

    #[test]
    fn test_invalid_category() {
        assert!(parse_layer_name("X | something").is_none());
    }

    #[test]
    fn test_array_attribute() {
        let result = parse_layer_name("S | item | tags:[\"a\",\"b\"]").unwrap();
        let tags = result.attributes.get("tags").unwrap();
        assert!(tags.is_array());
    }
}
