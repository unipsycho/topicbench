//! Topic subscribe helpers, including MQTT wildcards (`+`, `#`).

/// True if `filter` is a valid MQTT topic filter (single-level `+`, multi-level `#`).
pub fn is_valid_filter(filter: &str) -> bool {
    if filter.is_empty() || filter.contains("//") {
        return false;
    }
    let parts: Vec<&str> = filter.split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() && !(i == 0 || i == parts.len() - 1) {
            return false;
        }
        if *part == "#" {
            return i == parts.len() - 1;
        }
        if part.contains('#') || (part.contains('+') && *part != "+") {
            return false;
        }
    }
    true
}

/// Attach a subscription filter to an open session (placeholder — no broker call yet).
pub fn add_subscription(subs: &mut Vec<String>, filter: &str) -> Result<(), String> {
    let filter = filter.trim();
    if !is_valid_filter(filter) {
        return Err(format!("invalid topic filter: {filter}"));
    }
    if !subs.iter().any(|s| s == filter) {
        subs.push(filter.to_string());
    }
    Ok(())
}

/// Remove a subscription filter from a session's list.
pub fn remove_subscription(subs: &mut Vec<String>, filter: &str) -> bool {
    let before = subs.len();
    subs.retain(|s| s != filter);
    subs.len() != before
}

/// True if `topic` matches MQTT `filter` (`+` / `#`).
pub fn topic_matches_filter(topic: &str, filter: &str) -> bool {
    if filter == "#" {
        return true;
    }
    let t: Vec<&str> = topic.split('/').collect();
    let f: Vec<&str> = filter.split('/').collect();
    let mut ti = 0;
    for (fi, part) in f.iter().enumerate() {
        if *part == "#" {
            return fi == f.len() - 1;
        }
        if ti >= t.len() {
            return false;
        }
        if *part != "+" && *part != t[ti] {
            return false;
        }
        ti += 1;
    }
    ti == t.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards_ok() {
        assert!(is_valid_filter("sensors/+/temp"));
        assert!(is_valid_filter("building/#"));
        assert!(!is_valid_filter("a/#/b"));
        assert!(!is_valid_filter("a+b"));
    }

    #[test]
    fn add_remove_subscription() {
        let mut subs = Vec::new();
        add_subscription(&mut subs, "sensors/+/temp").unwrap();
        add_subscription(&mut subs, "sensors/+/temp").unwrap();
        assert_eq!(subs.len(), 1);
        assert!(remove_subscription(&mut subs, "sensors/+/temp"));
        assert!(subs.is_empty());
    }

    #[test]
    fn match_wildcards() {
        assert!(topic_matches_filter("sensors/lab/temp", "sensors/+/temp"));
        assert!(topic_matches_filter("sensors/lab/temp", "sensors/#"));
        assert!(!topic_matches_filter("sensors/lab/temp", "other/#"));
    }
}
