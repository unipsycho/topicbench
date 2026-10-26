//! Bridge handlers for Pro bookmarks + color rules (MCP → live GUI stores).

use crate::mqtt::profile::ProfileStore;
use crate::pro::bookmarks::{BookmarkJump, BookmarksStore};
use crate::pro::bridge::BridgeProState;
use crate::pro::color_rules::ColorRulesStore;
use serde_json::{json, Value};

/// Dispatch a bookmark_* / color_rule_* method, or `None` if not a Pro-store tool.
pub fn try_dispatch(
    method: &str,
    params: &Value,
    profiles: &ProfileStore,
    pro: &mut BridgeProState<'_>,
) -> Option<Result<Value, String>> {
    Some(match method {
        "bookmark_list" => Ok(json!({ "bookmarks": pro.bookmarks.items })),
        "bookmark_add" => bookmark_add(params, profiles, pro.bookmarks),
        "bookmark_update" => bookmark_update(params, pro.bookmarks),
        "bookmark_remove" => bookmark_remove(params, pro.bookmarks),
        "bookmark_toggle" => bookmark_toggle(params, profiles, pro.bookmarks),
        "bookmark_set_expand" => bookmark_set_expand(params, pro.bookmarks),
        "bookmark_reorder" => bookmark_reorder(params, pro.bookmarks),
        "bookmark_import" => bookmark_import(params, pro.bookmarks),
        "bookmark_export" => pro.bookmarks.export_json(),
        "bookmark_jump" => bookmark_jump(params, pro),
        "color_rule_list" => color_rule_list(pro.color_rules),
        "color_rule_upsert" => color_rule_upsert(params, pro.color_rules),
        "color_rule_delete" => color_rule_delete(params, pro.color_rules),
        "color_rule_reorder" => color_rule_reorder(params, pro.color_rules),
        "color_rule_import" => color_rule_import(params, pro.color_rules),
        "color_rule_export" => pro.color_rules.export_json(),
        _ => return None,
    })
}

fn resolve_profile_id(profiles: &ProfileStore, key: &str) -> Result<String, String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("profile required".into());
    }
    if profiles.profiles.contains_key(key) {
        return Ok(key.to_string());
    }
    let lower = key.to_ascii_lowercase();
    let mut hits: Vec<String> = profiles
        .profiles
        .values()
        .filter(|p| p.name.to_ascii_lowercase() == lower)
        .map(|p| p.id.clone())
        .collect();
    if hits.len() == 1 {
        Ok(hits.remove(0))
    } else if hits.is_empty() {
        Err(format!("profile not found: {key}"))
    } else {
        Err(format!("ambiguous profile name: {key}"))
    }
}

fn bookmark_add(
    params: &Value,
    profiles: &ProfileStore,
    store: &mut BookmarksStore,
) -> Result<Value, String> {
    let key = params
        .get("profile")
        .or_else(|| params.get("profile_id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "profile required".to_string())?;
    let profile_id = resolve_profile_id(profiles, key)?;
    let topic = params
        .get("topic")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "topic required".to_string())?;
    let label = params.get("label").and_then(|v| v.as_str()).unwrap_or("");
    let expand = params
        .get("expand_subtree")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    store.add(&profile_id, topic, label, expand)?;
    let b = store
        .items
        .iter()
        .rev()
        .find(|b| b.profile_id == profile_id && b.topic == topic)
        .cloned()
        .ok_or_else(|| "bookmark add failed".to_string())?;
    Ok(json!({ "ok": true, "bookmark": b }))
}

fn bookmark_update(params: &Value, store: &mut BookmarksStore) -> Result<Value, String> {
    let id = params
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "id required".to_string())?;
    let profile_id = params
        .get("profile")
        .or_else(|| params.get("profile_id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "profile required".to_string())?;
    let topic = params
        .get("topic")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "topic required".to_string())?;
    let label = params.get("label").and_then(|v| v.as_str()).unwrap_or("");
    let expand = params
        .get("expand_subtree")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    store.update(id, profile_id, topic, label, expand)?;
    let b = store
        .items
        .iter()
        .find(|b| b.id == id)
        .cloned()
        .ok_or_else(|| "bookmark not found after update".to_string())?;
    Ok(json!({ "ok": true, "bookmark": b }))
}

fn bookmark_remove(params: &Value, store: &mut BookmarksStore) -> Result<Value, String> {
    let id = params
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "id required".to_string())?;
    if !store.items.iter().any(|b| b.id == id) {
        return Err(format!("bookmark not found: {id}"));
    }
    store.remove_id(id);
    Ok(json!({ "ok": true, "id": id }))
}

fn bookmark_toggle(
    params: &Value,
    profiles: &ProfileStore,
    store: &mut BookmarksStore,
) -> Result<Value, String> {
    let key = params
        .get("profile")
        .or_else(|| params.get("profile_id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "profile required".to_string())?;
    let profile_id = resolve_profile_id(profiles, key)?;
    let topic = params
        .get("topic")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "topic required".to_string())?;
    let bookmarked = store.toggle(&profile_id, topic);
    Ok(json!({
        "ok": true,
        "profile_id": profile_id,
        "topic": topic,
        "bookmarked": bookmarked,
    }))
}

fn bookmark_set_expand(params: &Value, store: &mut BookmarksStore) -> Result<Value, String> {
    let id = params
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "id required".to_string())?;
    let expand = params
        .get("expand_subtree")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| "expand_subtree required".to_string())?;
    if !store.items.iter().any(|b| b.id == id) {
        return Err(format!("bookmark not found: {id}"));
    }
    store.set_expand(id, expand);
    Ok(json!({ "ok": true, "id": id, "expand_subtree": expand }))
}

fn bookmark_reorder(params: &Value, store: &mut BookmarksStore) -> Result<Value, String> {
    let dragged = params
        .get("dragged")
        .or_else(|| params.get("id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "dragged id required".to_string())?;
    let target = params
        .get("target")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "target id required".to_string())?;
    let after = params
        .get("after")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !store.reorder_ids(dragged, target, after) {
        return Err("reorder failed (unknown ids?)".into());
    }
    Ok(json!({ "ok": true, "bookmarks": store.items }))
}

fn bookmark_import(params: &Value, store: &mut BookmarksStore) -> Result<Value, String> {
    let mode = params
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("replace");
    let text = if let Some(s) = params.get("json").and_then(|j| j.as_str()) {
        s.to_string()
    } else if params.get("bookmarks").is_some() {
        let body = if params.get("version").is_some() {
            params.clone()
        } else {
            json!({
                "version": 1,
                "bookmarks": params.get("bookmarks").cloned().unwrap_or(json!([]))
            })
        };
        serde_json::to_string(&body).map_err(|e| e.to_string())?
    } else {
        return Err("bookmark_import requires json string or bookmarks array".into());
    };
    let n = store.import_json(&text, mode)?;
    Ok(json!({ "ok": true, "mode": mode, "imported": n, "bookmarks": store.items }))
}

fn bookmark_jump(params: &Value, pro: &mut BridgeProState<'_>) -> Result<Value, String> {
    let (profile_id, topic, expand) = if let Some(id) = params.get("id").and_then(|v| v.as_str()) {
        let b = pro
            .bookmarks
            .items
            .iter()
            .find(|b| b.id == id)
            .ok_or_else(|| format!("bookmark not found: {id}"))?;
        (b.profile_id.clone(), b.topic.clone(), b.expand_subtree)
    } else {
        let profile_id = params
            .get("profile")
            .or_else(|| params.get("profile_id"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| "id or profile+topic required".to_string())?
            .to_string();
        let topic = params
            .get("topic")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "topic required".to_string())?
            .to_string();
        let expand = params
            .get("expand_subtree")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        (profile_id, topic, expand)
    };
    *pro.pending_bookmark_jump = Some(BookmarkJump {
        profile_id: profile_id.clone(),
        topic: topic.clone(),
        expand_subtree: expand,
    });
    Ok(json!({
        "ok": true,
        "queued": true,
        "profile_id": profile_id,
        "topic": topic,
        "expand_subtree": expand,
    }))
}

fn color_rule_list(store: &ColorRulesStore) -> Result<Value, String> {
    store.export_json()
}

fn color_rule_upsert(params: &Value, store: &mut ColorRulesStore) -> Result<Value, String> {
    let rule_v = params.get("rule").unwrap_or(params);
    let rule = store.upsert_json(rule_v)?;
    let rules = store
        .export_json()?
        .get("rules")
        .cloned()
        .unwrap_or(json!([]));
    let one = rules
        .as_array()
        .and_then(|a| {
            a.iter()
                .find(|r| r.get("id").and_then(|i| i.as_str()) == Some(rule.id.as_str()))
                .cloned()
        })
        .unwrap_or(json!({ "id": rule.id }));
    Ok(json!({ "ok": true, "rule": one }))
}

fn color_rule_delete(params: &Value, store: &mut ColorRulesStore) -> Result<Value, String> {
    let id = params
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "id required".to_string())?;
    if !store.remove(id) {
        return Err(format!("color rule not found: {id}"));
    }
    Ok(json!({ "ok": true, "id": id }))
}

fn color_rule_reorder(params: &Value, store: &mut ColorRulesStore) -> Result<Value, String> {
    if let (Some(from), Some(to)) = (
        params.get("from").and_then(|v| v.as_u64()),
        params.get("to").and_then(|v| v.as_u64()),
    ) {
        store.reorder(from as usize, to as usize);
        return Ok(json!({
            "ok": true,
            "rules": store.export_json()?.get("rules").cloned().unwrap_or(json!([]))
        }));
    }
    let dragged = params
        .get("dragged")
        .or_else(|| params.get("id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "from/to or dragged+target required".to_string())?;
    let target = params
        .get("target")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "target id required".to_string())?;
    let after = params
        .get("after")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !store.reorder_ids(dragged, target, after) {
        return Err("reorder failed (unknown ids?)".into());
    }
    Ok(json!({
        "ok": true,
        "rules": store.export_json()?.get("rules").cloned().unwrap_or(json!([]))
    }))
}

fn color_rule_import(params: &Value, store: &mut ColorRulesStore) -> Result<Value, String> {
    let mode = params
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("replace");
    let text = if let Some(s) = params.get("json").and_then(|j| j.as_str()) {
        s.to_string()
    } else if params.get("rules").is_some() {
        let body = if params.get("version").is_some() {
            params.clone()
        } else {
            json!({
                "version": 1,
                "rules": params.get("rules").cloned().unwrap_or(json!([]))
            })
        };
        serde_json::to_string(&body).map_err(|e| e.to_string())?
    } else {
        return Err("color_rule_import requires json string or rules array".into());
    };
    let n = store.import_json(&text, mode)?;
    Ok(json!({
        "ok": true,
        "mode": mode,
        "imported": n,
        "rules": store.export_json()?.get("rules").cloned().unwrap_or(json!([]))
    }))
}
