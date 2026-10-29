//! Custom color rules (Pro) — fixed predicates, first-match wins; no formula engine.
//! Nested payload fields use the same dotted / `foo[0]` paths as sticky Value pins.

use crate::mqtt::paths;
use crate::mqtt::subscribe;
use crate::mqtt::value_path;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

const FILE_NAME: &str = "color_rules.json";
const MAX_RULES: usize = 32;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ColorRulesFile {
    pub version: u32,
    pub rules: Vec<ColorRule>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ColorRule {
    pub id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub order: u32,
    /// Empty / null = all profiles.
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default = "default_glob")]
    pub topic_glob: String,
    pub match_rule: MatchPred,
    pub style: RuleStyle,
}

fn default_true() -> bool {
    true
}
fn default_glob() -> String {
    "#".into()
}

/// Wire name `match` in JSON.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MatchPred {
    /// Full topic path equals (case-insensitive).
    TopicEquals { value: String },
    /// Topic path contains substring (case-insensitive).
    TopicContains { value: String },
    /// Glob / `*` / `?`, or MQTT `+` / `#` filter.
    TopicMatches { pattern: String },
    PayloadEquals { value: String },
    PayloadContains { value: String },
    /// Whole payload parsed as a number, then compared.
    PayloadNumber { op: String, value: f64 },
    /// Nested field equals (dotted path / `foo[0].bar`, optional `$.` prefix).
    JsonPathEquals { path: String, value: Value },
    /// Nested string/number field contains substring (case-insensitive).
    JsonPathContains { path: String, value: String },
    JsonPathNumber { path: String, op: String, value: f64 },
    JsonPathTruthy { path: String },
}

// Serde alias: file uses "match" key
mod match_rule_serde {
    // handled via rename on field — use #[serde(rename = "match")] on ColorRule.match_rule
}

/// RGBA style slot — JSON accepts legacy `[r,g,b]` (alpha 255) or `[r,g,b,a]`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RuleStyle {
    #[serde(default, deserialize_with = "de_opt_rgba")]
    pub topic_color: Option<[u8; 4]>,
    #[serde(default, deserialize_with = "de_opt_rgba")]
    pub value_color: Option<[u8; 4]>,
    #[serde(default, deserialize_with = "de_opt_rgba")]
    pub row_bg: Option<[u8; 4]>,
}

fn de_opt_rgba<'de, D>(deserializer: D) -> Result<Option<[u8; 4]>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<Vec<u8>> = Option::deserialize(deserializer)?;
    match opt {
        None => Ok(None),
        Some(v) if v.len() == 3 => Ok(Some([v[0], v[1], v[2], 255])),
        Some(v) if v.len() == 4 => Ok(Some([v[0], v[1], v[2], v[3]])),
        Some(_) => Err(serde::de::Error::custom(
            "color must be [r,g,b] or [r,g,b,a]",
        )),
    }
}

impl RuleStyle {
    pub fn topic_c32(&self) -> Option<eframe::egui::Color32> {
        self.topic_color.map(rgba_c32)
    }
    pub fn value_c32(&self) -> Option<eframe::egui::Color32> {
        self.value_color.map(rgba_c32)
    }
    pub fn row_bg_c32(&self) -> Option<eframe::egui::Color32> {
        self.row_bg.map(rgba_c32)
    }
}

fn rgba_c32([r, g, b, a]: [u8; 4]) -> eframe::egui::Color32 {
    eframe::egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}

#[derive(Default)]
pub struct ColorRulesStore {
    pub rules: Vec<ColorRule>,
}

impl ColorRulesStore {
    pub fn load() -> Self {
        let mut s = Self::default();
        s.reload();
        s
    }

    pub fn path() -> PathBuf {
        paths::config_dir().join("pro").join(FILE_NAME)
    }

    pub fn reload(&mut self) {
        let path = Self::path();
        let Ok(text) = fs::read_to_string(&path) else {
            self.rules.clear();
            return;
        };
        // Accept either `match` or `match_rule` in file via a helper.
        match parse_file(&text) {
            Ok(mut rules) => {
                rules.sort_by_key(|r| r.order);
                self.rules = rules;
            }
            Err(_) => self.rules.clear(),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut rules = self.rules.clone();
        for (i, r) in rules.iter_mut().enumerate() {
            r.order = i as u32;
        }
        // Serialize with "match" key for the product file shape.
        let arr: Vec<Value> = rules
            .iter()
            .filter_map(|r| rule_to_json(r).ok())
            .collect();
        let body = serde_json::json!({ "version": 1, "rules": arr });
        let text = serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?;
        fs::write(&path, text).map_err(|e| e.to_string())
    }

    pub fn add_blank(&mut self) -> Result<(), String> {
        if self.rules.len() >= MAX_RULES {
            return Err(format!("at most {MAX_RULES} color rules"));
        }
        self.rules.push(ColorRule {
            id: new_id(),
            name: "New rule".into(),
            enabled: true,
            order: self.rules.len() as u32,
            profile_id: None,
            topic_glob: "#".into(),
            match_rule: MatchPred::PayloadContains {
                value: String::new(),
            },
            style: RuleStyle {
                topic_color: Some([220, 70, 70, 255]),
                value_color: Some([220, 70, 70, 255]),
                // Default bg slightly faded so rows stay readable on charcoal.
                row_bg: Some([60, 28, 28, 140]),
            },
        });
        self.save()
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.rules.len();
        self.rules.retain(|r| r.id != id);
        if self.rules.len() != before {
            let _ = self.save();
            true
        } else {
            false
        }
    }

    pub fn reorder(&mut self, from: usize, to: usize) {
        if from >= self.rules.len() || to >= self.rules.len() || from == to {
            return;
        }
        let item = self.rules.remove(from);
        self.rules.insert(to, item);
        let _ = self.save();
    }

    /// Reorder by id (insert before/after target) — first-match eval order.
    pub fn reorder_ids(&mut self, dragged: &str, target: &str, after: bool) -> bool {
        if dragged == target {
            return false;
        }
        let Some(from) = self.rules.iter().position(|r| r.id == dragged) else {
            return false;
        };
        let Some(mut to) = self.rules.iter().position(|r| r.id == target) else {
            return false;
        };
        let item = self.rules.remove(from);
        if from < to {
            to -= 1;
        }
        if after {
            to += 1;
        }
        to = to.min(self.rules.len());
        self.rules.insert(to, item);
        let _ = self.save();
        true
    }

    /// Create or replace a rule from product JSON (`match` key). Empty/missing id → new.
    pub fn upsert_json(&mut self, v: &Value) -> Result<ColorRule, String> {
        let mut rule = rule_from_json(v)?;
        if rule.id.trim().is_empty() {
            rule.id = new_id();
        }
        if let Some(existing) = self.rules.iter_mut().find(|r| r.id == rule.id) {
            let order = existing.order;
            *existing = rule.clone();
            existing.order = order;
        } else {
            if self.rules.len() >= MAX_RULES {
                return Err(format!("at most {MAX_RULES} color rules"));
            }
            rule.order = self.rules.len() as u32;
            self.rules.push(rule.clone());
        }
        self.save()?;
        Ok(rule)
    }

    /// Import `{version, rules:[…]}`. `mode`: `"replace"` or `"merge"` (by id).
    pub fn import_json(&mut self, text: &str, mode: &str) -> Result<usize, String> {
        let incoming = parse_file(text)?;
        let n = incoming.len();
        match mode {
            "replace" => {
                if n > MAX_RULES {
                    return Err(format!("at most {MAX_RULES} color rules"));
                }
                self.rules = incoming;
            }
            "merge" => {
                for r in incoming {
                    if let Some(existing) = self.rules.iter_mut().find(|x| x.id == r.id) {
                        let order = existing.order;
                        *existing = r;
                        existing.order = order;
                    } else {
                        if self.rules.len() >= MAX_RULES {
                            return Err(format!("at most {MAX_RULES} color rules"));
                        }
                        self.rules.push(r);
                    }
                }
            }
            _ => return Err(format!("unknown import mode: {mode}")),
        }
        self.rules.sort_by_key(|r| r.order);
        self.save()?;
        Ok(n)
    }

    /// Export product file shape (`match` key on each rule).
    pub fn export_json(&self) -> Result<Value, String> {
        let mut rules = self.rules.clone();
        for (i, r) in rules.iter_mut().enumerate() {
            r.order = i as u32;
        }
        let arr: Vec<Value> = rules
            .iter()
            .filter_map(|r| rule_to_json(r).ok())
            .collect();
        Ok(serde_json::json!({ "version": 1, "rules": arr }))
    }

    /// First matching enabled rule for this leaf topic + payload.
    pub fn style_for(
        &self,
        profile_id: &str,
        topic: &str,
        payload: &str,
    ) -> Option<&RuleStyle> {
        for r in &self.rules {
            if !r.enabled {
                continue;
            }
            if let Some(pid) = r.profile_id.as_deref() {
                if !pid.is_empty() && pid != profile_id {
                    continue;
                }
            }
            if !topic_glob_match(&r.topic_glob, topic) {
                continue;
            }
            if pred_matches(&r.match_rule, topic, payload) {
                return Some(&r.style);
            }
        }
        None
    }
}

fn topic_glob_match(glob: &str, topic: &str) -> bool {
    let g = glob.trim();
    if g.is_empty() || g == "#" || g == "*" {
        return true;
    }
    // MQTT-style via existing matcher when possible; also allow trailing *
    if g.ends_with('*') && !g.contains('+') && !g.contains('#') {
        let prefix = g.trim_end_matches('*');
        return topic.starts_with(prefix);
    }
    subscribe::topic_matches_filter(topic, g)
}

fn pred_matches(pred: &MatchPred, topic: &str, payload: &str) -> bool {
    let trimmed = payload.trim();
    match pred {
        MatchPred::TopicEquals { value } => {
            !value.trim().is_empty() && topic.eq_ignore_ascii_case(value.trim())
        }
        MatchPred::TopicContains { value } => {
            !value.is_empty()
                && topic
                    .to_lowercase()
                    .contains(&value.trim().to_lowercase())
        }
        MatchPred::TopicMatches { pattern } => topic_pattern_match(topic, pattern.trim()),
        MatchPred::PayloadEquals { value } => trimmed == value.trim(),
        MatchPred::PayloadContains { value } => {
            !value.is_empty() && trimmed.to_lowercase().contains(&value.to_lowercase())
        }
        MatchPred::PayloadNumber { op, value } => {
            let Some(n) = parse_payload_number(trimmed) else {
                return false;
            };
            cmp_f64(op, n, *value)
        }
        MatchPred::JsonPathEquals { path, value } => {
            json_at(payload, path).is_some_and(|v| json_eq(&v, value))
        }
        MatchPred::JsonPathContains { path, value } => {
            if value.is_empty() || path.trim().is_empty() {
                return false;
            }
            let Some(v) = json_at(payload, path) else {
                return false;
            };
            let hay = json_as_text(&v);
            !hay.is_empty() && hay.to_lowercase().contains(&value.to_lowercase())
        }
        MatchPred::JsonPathNumber { path, op, value } => {
            let Some(n) = json_at(payload, path).and_then(json_as_f64) else {
                return false;
            };
            cmp_f64(op, n, *value)
        }
        MatchPred::JsonPathTruthy { path } => {
            json_at(payload, path).is_some_and(|v| json_truthy(&v))
        }
    }
}

fn cmp_f64(op: &str, n: f64, value: f64) -> bool {
    match op {
        "<" => n < value,
        "<=" => n <= value,
        ">" => n > value,
        ">=" => n >= value,
        "==" | "=" => (n - value).abs() < 1e-9,
        _ => false,
    }
}

fn parse_payload_number(trimmed: &str) -> Option<f64> {
    if trimmed.is_empty() {
        return None;
    }
    // Bare number, or a JSON number literal.
    if let Ok(n) = trimmed.parse::<f64>() {
        return Some(n);
    }
    let v: Value = serde_json::from_str(trimmed).ok()?;
    json_as_f64(v)
}

fn json_as_f64(v: Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        Value::Bool(b) => Some(if b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

/// Topic name pattern: MQTT `+`/`#` when present, else `*` / `?` glob (case-insensitive).
fn topic_pattern_match(topic: &str, pattern: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }
    if pattern.contains('+') || pattern.contains('#') {
        return subscribe::topic_matches_filter(topic, pattern);
    }
    glob_match_ci(topic, pattern)
}

/// Simple case-insensitive glob: `*` any run, `?` one char.
fn glob_match_ci(text: &str, pattern: &str) -> bool {
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    glob_match_chars(&t, &p)
}

fn glob_match_chars(text: &[char], pattern: &[char]) -> bool {
    let (mut ti, mut pi) = (0usize, 0usize);
    let mut star_pi = None;
    let mut star_ti = 0usize;
    while ti < text.len() {
        if pi < pattern.len() && (pattern[pi] == '?' || pattern[pi] == text[ti]) {
            ti += 1;
            pi += 1;
        } else if pi < pattern.len() && pattern[pi] == '*' {
            star_pi = Some(pi);
            star_ti = ti;
            pi += 1;
        } else if let Some(sp) = star_pi {
            pi = sp + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }
    while pi < pattern.len() && pattern[pi] == '*' {
        pi += 1;
    }
    pi == pattern.len()
}

fn json_at(payload: &str, path: &str) -> Option<Value> {
    // Shared walker: nested objects, array indices, `$.` / `foo[0]` sugar.
    value_path::value_at_path(payload, path)
}

fn json_as_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn json_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Null, Value::Null) => true,
        // Allow string compare against number/bool literals in rules.
        (Value::String(x), other) => x == &other.to_string().trim_matches('"').to_string()
            || (other.as_str().is_some_and(|y| x == y)),
        (other, Value::String(y)) => other.as_str() == Some(y.as_str())
            || other.to_string().trim_matches('"') == y.as_str(),
        _ => a == b,
    }
}

fn json_truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0),
        Value::String(s) => {
            let t = s.trim();
            !(t.is_empty()
                || t == "0"
                || t.eq_ignore_ascii_case("false")
                || t.eq_ignore_ascii_case("off")
                || t.eq_ignore_ascii_case("null"))
        }
        Value::Null => false,
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn parse_file(text: &str) -> Result<Vec<ColorRule>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let arr = v
        .get("rules")
        .and_then(|a| a.as_array())
        .ok_or_else(|| "missing rules".to_string())?;
    let mut out = Vec::new();
    for item in arr {
        out.push(rule_from_json(item)?);
    }
    Ok(out)
}

fn rule_from_json(v: &Value) -> Result<ColorRule, String> {
    let match_v = v
        .get("match")
        .or_else(|| v.get("match_rule"))
        .cloned()
        .ok_or_else(|| "missing match".to_string())?;
    let match_rule: MatchPred =
        serde_json::from_value(match_v).map_err(|e| format!("match: {e}"))?;
    let style: RuleStyle = serde_json::from_value(
        v.get("style").cloned().unwrap_or(serde_json::json!({})),
    )
    .map_err(|e| format!("style: {e}"))?;
    Ok(ColorRule {
        id: v
            .get("id")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        name: v
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("Rule")
            .to_string(),
        enabled: v
            .get("enabled")
            .and_then(|x| x.as_bool())
            .unwrap_or(true),
        order: v.get("order").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
        profile_id: v
            .get("profile_id")
            .and_then(|x| {
                if x.is_null() {
                    None
                } else {
                    x.as_str().map(|s| s.to_string())
                }
            }),
        topic_glob: v
            .get("topic_glob")
            .and_then(|x| x.as_str())
            .unwrap_or("#")
            .to_string(),
        match_rule,
        style,
    })
}

fn rule_to_json(r: &ColorRule) -> Result<Value, String> {
    let mut v = serde_json::to_value(r).map_err(|e| e.to_string())?;
    if let Some(obj) = v.as_object_mut() {
        if let Some(m) = obj.remove("match_rule") {
            obj.insert("match".into(), m);
        }
    }
    Ok(v)
}

fn new_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("cr-{ns:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_channel_equals() {
        let payload = r#"{"meta":{"channel":"Line-A"},"ok":true}"#;
        assert!(pred_matches(
            &MatchPred::JsonPathEquals {
                path: "meta.channel".into(),
                value: Value::String("Line-A".into()),
            },
            "plant/status",
            payload,
        ));
        assert!(!pred_matches(
            &MatchPred::JsonPathEquals {
                path: "meta.channel".into(),
                value: Value::String("Line-B".into()),
            },
            "plant/status",
            payload,
        ));
    }

    #[test]
    fn nested_channel_contains_and_array() {
        let payload = r#"{"channels":[{"name":"CH-North"},{"name":"CH-South"}]}"#;
        assert!(pred_matches(
            &MatchPred::JsonPathContains {
                path: "channels[1].name".into(),
                value: "south".into(),
            },
            "plant/io",
            payload,
        ));
        assert!(pred_matches(
            &MatchPred::JsonPathEquals {
                path: "$.channels.0.name".into(),
                value: Value::String("CH-North".into()),
            },
            "plant/io",
            payload,
        ));
    }

    #[test]
    fn whole_payload_rules_still_work() {
        assert!(pred_matches(
            &MatchPred::PayloadContains {
                value: "fault".into(),
            },
            "a/b",
            "device fault code",
        ));
        assert!(pred_matches(
            &MatchPred::PayloadNumber {
                op: ">".into(),
                value: 10.0,
            },
            "a/b",
            "12.5",
        ));
    }
}
