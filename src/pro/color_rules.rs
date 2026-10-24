//! Custom color rules (Pro) — fixed predicates, first-match wins; no formula engine.

use crate::mqtt::paths;
use crate::mqtt::subscribe;
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
    PayloadEquals { value: String },
    PayloadContains { value: String },
    JsonPathEquals { path: String, value: Value },
    JsonPathNumber { path: String, op: String, value: f64 },
    JsonPathTruthy { path: String },
}

// Serde alias: file uses "match" key
mod match_rule_serde {
    // handled via rename on field — use #[serde(rename = "match")] on ColorRule.match_rule
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RuleStyle {
    #[serde(default)]
    pub topic_color: Option<[u8; 3]>,
    #[serde(default)]
    pub value_color: Option<[u8; 3]>,
    #[serde(default)]
    pub row_bg: Option<[u8; 3]>,
}

impl RuleStyle {
    pub fn topic_c32(&self) -> Option<eframe::egui::Color32> {
        self.topic_color
            .map(|[r, g, b]| eframe::egui::Color32::from_rgb(r, g, b))
    }
    pub fn value_c32(&self) -> Option<eframe::egui::Color32> {
        self.value_color
            .map(|[r, g, b]| eframe::egui::Color32::from_rgb(r, g, b))
    }
    pub fn row_bg_c32(&self) -> Option<eframe::egui::Color32> {
        self.row_bg
            .map(|[r, g, b]| eframe::egui::Color32::from_rgb(r, g, b))
    }
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
                topic_color: Some([220, 70, 70]),
                value_color: Some([220, 70, 70]),
                row_bg: Some([60, 28, 28]),
            },
        });
        self.save()
    }

    pub fn remove(&mut self, id: &str) {
        self.rules.retain(|r| r.id != id);
        let _ = self.save();
    }

    pub fn reorder(&mut self, from: usize, to: usize) {
        if from >= self.rules.len() || to >= self.rules.len() || from == to {
            return;
        }
        let item = self.rules.remove(from);
        self.rules.insert(to, item);
        let _ = self.save();
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
            if pred_matches(&r.match_rule, payload) {
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

fn pred_matches(pred: &MatchPred, payload: &str) -> bool {
    let trimmed = payload.trim();
    match pred {
        MatchPred::PayloadEquals { value } => trimmed == value.trim(),
        MatchPred::PayloadContains { value } => {
            !value.is_empty() && trimmed.to_lowercase().contains(&value.to_lowercase())
        }
        MatchPred::JsonPathEquals { path, value } => {
            json_at(payload, path).is_some_and(|v| json_eq(&v, value))
        }
        MatchPred::JsonPathNumber { path, op, value } => {
            let Some(n) = json_at(payload, path).and_then(|v| v.as_f64()) else {
                return false;
            };
            match op.as_str() {
                "<" => n < *value,
                "<=" => n <= *value,
                ">" => n > *value,
                ">=" => n >= *value,
                "==" | "=" => (n - *value).abs() < 1e-9,
                _ => false,
            }
        }
        MatchPred::JsonPathTruthy { path } => {
            json_at(payload, path).is_some_and(|v| json_truthy(&v))
        }
    }
}

fn json_at(payload: &str, path: &str) -> Option<Value> {
    let v: Value = serde_json::from_str(payload).ok()?;
    if path.is_empty() {
        return Some(v);
    }
    let mut cur = &v;
    for seg in path.split('.').filter(|s| !s.is_empty()) {
        cur = cur.get(seg)?;
    }
    Some(cur.clone())
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
