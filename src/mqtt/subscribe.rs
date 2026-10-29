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

/// Attach a subscription filter. Exact duplicates are ignored; overlapping
/// filters (e.g. `#` + `topic/#`) are allowed so users can add then delete in any order.
pub fn add_subscription(subs: &mut Vec<String>, filter: &str) -> Result<(), String> {
    let filter = filter.trim();
    if !is_valid_filter(filter) {
        return Err(format!("invalid topic filter: {filter}"));
    }
    if subs.iter().any(|s| s == filter) {
        return Ok(());
    }
    subs.push(filter.to_string());
    Ok(())
}

/// Remove a subscription filter from a session's list.
pub fn remove_subscription(subs: &mut Vec<String>, filter: &str) -> bool {
    let before = subs.len();
    subs.retain(|s| s != filter);
    subs.len() != before
}

/// Drop filters that are fully covered by another filter in the list.
/// Returns true if the list changed.
///
/// Not used on add/connect — overlapping filters are kept so users can stage
/// narrower filters beside `#` and remove `#` in any order. Call explicitly if
/// collapsing overlap is desired (brokers may deliver once per matching filter).
pub fn prune_redundant(subs: &mut Vec<String>) -> bool {
    let before = subs.clone();
    let mut keep: Vec<String> = Vec::new();
    for (i, f) in before.iter().enumerate() {
        let covered = before.iter().enumerate().any(|(j, g)| i != j && filter_covers(g, f));
        if !covered && !keep.iter().any(|k| k == f) {
            keep.push(f.clone());
        }
    }
    let changed = keep != *subs;
    if changed {
        *subs = keep;
    }
    changed
}

/// True if every topic that matches `inner` also matches `outer`.
pub fn filter_covers(outer: &str, inner: &str) -> bool {
    if outer == inner || outer == "#" {
        return true;
    }
    if inner == "#" {
        return false;
    }
    let o: Vec<&str> = outer.split('/').collect();
    let i: Vec<&str> = inner.split('/').collect();
    let mut oi = 0;
    let mut ii = 0;
    while oi < o.len() {
        if o[oi] == "#" {
            return oi + 1 == o.len();
        }
        if ii >= i.len() {
            return false;
        }
        if i[ii] == "#" {
            // Inner multi-level is broader from here — outer cannot cover it.
            return false;
        }
        match (o[oi], i[ii]) {
            ("+", _) => {
                oi += 1;
                ii += 1;
            }
            (a, b) if a == b => {
                oi += 1;
                ii += 1;
            }
            _ => return false,
        }
    }
    ii == i.len()
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
    fn add_allows_overlap() {
        let mut subs = Vec::new();
        add_subscription(&mut subs, "#").unwrap();
        add_subscription(&mut subs, "topic/#").unwrap();
        assert_eq!(subs, vec!["#".to_string(), "topic/#".to_string()]);
        add_subscription(&mut subs, "#").unwrap();
        assert_eq!(subs, vec!["#".to_string(), "topic/#".to_string()]);
    }

    #[test]
    fn prune_sensors_and_hash() {
        // Helper still collapses overlap when called explicitly (not used on add/connect).
        let mut subs = vec!["sensors/#".into(), "#".into()];
        assert!(prune_redundant(&mut subs));
        assert_eq!(subs, vec!["#".to_string()]);
    }

    #[test]
    fn filter_covers_cases() {
        assert!(filter_covers("#", "sensors/#"));
        assert!(filter_covers("sensors/#", "sensors/lab/temp"));
        assert!(filter_covers("sensors/#", "sensors/+/temp"));
        assert!(!filter_covers("sensors/#", "#"));
        assert!(!filter_covers("sensors/+/temp", "sensors/#"));
    }

    #[test]
    fn match_wildcards() {
        assert!(topic_matches_filter("sensors/lab/temp", "sensors/+/temp"));
        assert!(topic_matches_filter("sensors/lab/temp", "sensors/#"));
        assert!(!topic_matches_filter("sensors/lab/temp", "other/#"));
    }
}
