//! MQTT topic + JSON field path helpers for graphable Payload → sticky pins.
//!
//! Sticky identity: `topic` or `topic::dotted.path` (arrays use numeric segments).

use serde_json::Value;

/// Join MQTT topic with optional JSON field path for sticky identity.
pub fn sticky_id(topic: &str, path: Option<&str>) -> String {
    match path.map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => format!("{topic}::{p}"),
        None => topic.to_string(),
    }
}

/// Split sticky identity into MQTT topic + optional field path.
pub fn split_sticky_id(id: &str) -> (&str, Option<&str>) {
    match id.split_once("::") {
        Some((topic, path)) if !path.is_empty() => (topic, Some(path)),
        _ => (id, None),
    }
}

/// Extract a scalar at `dotted.path` from a JSON payload (objects + array indices).
pub fn extract_path_value(payload: &str, path: &str) -> Option<String> {
    let root: Value = serde_json::from_str(payload).ok()?;
    let mut cur = &root;
    for seg in path.split('.').filter(|s| !s.is_empty()) {
        cur = if let Ok(i) = seg.parse::<usize>() {
            cur.get(i)?
        } else {
            cur.get(seg)?
        };
    }
    match cur {
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(s.clone()),
        Value::Null => Some("null".into()),
        _ => None,
    }
}

/// History / Graph numeric: numbers as-is; bools → 1.0 / 0.0.
pub fn sample_numeric(payload: &str) -> Option<f64> {
    let t = payload.trim();
    if t.eq_ignore_ascii_case("true") || t == "1" || t.eq_ignore_ascii_case("on") {
        return Some(1.0);
    }
    if t.eq_ignore_ascii_case("false") || t == "0" || t.eq_ignore_ascii_case("off") {
        return Some(0.0);
    }
    t.parse::<f64>().ok()
}

/// True when a JSON/text literal can be graphed (number or boolean).
pub fn is_graphable_literal(s: &str) -> bool {
    let t = s.trim().trim_end_matches(',');
    if t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("false") {
        return true;
    }
    t.parse::<f64>().is_ok()
}

/// For each pretty-JSON line: dotted path when that line is a graphable leaf.
pub fn graphable_paths_per_line(pretty: &str) -> Vec<Option<String>> {
    let Ok(root) = serde_json::from_str::<Value>(pretty) else {
        return pretty.lines().map(|_| None).collect();
    };
    let mut leaves = Vec::new();
    collect_leaves(&root, String::new(), &mut leaves);

    let mut out = Vec::new();
    for line in pretty.lines() {
        let trimmed = line.trim().trim_end_matches(',');
        let path = match parse_kv(trimmed) {
            Some((key, val)) if is_graphable_literal(val) => {
                take_leaf(&mut leaves, |p, v| {
                    values_eq(v, val) && (p == key || p.ends_with(&format!(".{key}")))
                })
            }
            None if is_graphable_literal(trimmed) => {
                // Array / root scalar leaf on its own line.
                take_leaf(&mut leaves, |_, v| values_eq(v, trimmed))
            }
            _ => None,
        };
        out.push(path);
    }
    out
}

fn collect_leaves(v: &Value, path: String, out: &mut Vec<(String, String)>) {
    match v {
        Value::Bool(b) => out.push((path, b.to_string())),
        Value::Number(n) => out.push((path, n.to_string())),
        Value::Object(map) => {
            for (k, child) in map {
                let p = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                collect_leaves(child, p, out);
            }
        }
        Value::Array(arr) => {
            for (i, child) in arr.iter().enumerate() {
                let p = if path.is_empty() {
                    format!("{i}")
                } else {
                    format!("{path}.{i}")
                };
                collect_leaves(child, p, out);
            }
        }
        _ => {}
    }
}

fn take_leaf(
    leaves: &mut Vec<(String, String)>,
    pred: impl Fn(&str, &str) -> bool,
) -> Option<String> {
    let i = leaves.iter().position(|(p, v)| pred(p, v))?;
    Some(leaves.remove(i).0)
}

fn parse_kv(trimmed: &str) -> Option<(&str, &str)> {
    let t = trimmed.strip_prefix('"')?;
    let end = t.find('"')?;
    let key = &t[..end];
    let rest = t[end + 1..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    Some((key, rest))
}

fn values_eq(stored: &str, line_val: &str) -> bool {
    let a = stored.trim();
    let b = line_val.trim().trim_end_matches(',');
    if a.eq_ignore_ascii_case(b) {
        return true;
    }
    match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(x), Ok(y)) => (x - y).abs() < 1e-12,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sticky_id_roundtrip() {
        assert_eq!(sticky_id("a/b", None), "a/b");
        assert_eq!(sticky_id("a/b", Some("temp")), "a/b::temp");
        assert_eq!(split_sticky_id("a/b::temp.x"), ("a/b", Some("temp.x")));
        assert_eq!(split_sticky_id("a/b"), ("a/b", None));
    }

    #[test]
    fn extracts_nested() {
        let j = r#"{"nested":{"humidity":40},"on":true}"#;
        assert_eq!(
            extract_path_value(j, "nested.humidity").as_deref(),
            Some("40")
        );
        assert_eq!(extract_path_value(j, "on").as_deref(), Some("true"));
    }

    #[test]
    fn annotates_pretty_lines() {
        let pretty = "{\n  \"temp\": 21.5,\n  \"on\": true,\n  \"name\": \"lab\"\n}";
        let paths = graphable_paths_per_line(pretty);
        assert_eq!(paths.len(), 5);
        assert_eq!(paths[1].as_deref(), Some("temp"));
        assert_eq!(paths[2].as_deref(), Some("on"));
        assert!(paths[3].is_none());
    }
}
