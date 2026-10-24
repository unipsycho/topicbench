//! Live MQTT record → CSV tape (same shape as Simulation Playback).
//!
//! Controls live on topic-tree broker headers (Pro). Playback stays on Simulate.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// One captured publish on the tape (relative ms from recording start).
#[derive(Clone, Debug)]
pub struct RecEvent {
    pub t_ms: u64,
    pub topic: String,
    pub payload: String,
    pub qos: u8,
    pub retain: bool,
}

struct ActiveRec {
    profile_id: String,
    started: Instant,
    started_unix_ms: u64,
    events: Vec<RecEvent>,
}

/// Per-profile recording sessions (at most one active tape per profile).
#[derive(Default)]
pub struct RecordingStore {
    active: HashMap<String, ActiveRec>,
}

impl RecordingStore {
    pub fn is_recording(&self, profile_id: &str) -> bool {
        self.active.contains_key(profile_id)
    }

    pub fn event_count(&self, profile_id: &str) -> usize {
        self.active
            .get(profile_id)
            .map(|r| r.events.len())
            .unwrap_or(0)
    }

    pub fn start(&mut self, profile_id: &str) {
        let started_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        self.active.insert(
            profile_id.to_string(),
            ActiveRec {
                profile_id: profile_id.to_string(),
                started: Instant::now(),
                started_unix_ms,
                events: Vec::new(),
            },
        );
    }

    /// Append a message if this profile is recording.
    pub fn on_message(&mut self, profile_id: &str, topic: &str, payload: &str, retain: bool) {
        let Some(rec) = self.active.get_mut(profile_id) else {
            return;
        };
        let t_ms = rec.started.elapsed().as_millis() as u64;
        rec.events.push(RecEvent {
            t_ms,
            topic: topic.to_string(),
            payload: payload.to_string(),
            qos: 0,
            retain,
        });
    }

    /// Stop and write CSV. Returns (path, event_count) or error. Clears active entry.
    pub fn stop_to_file(&mut self, profile_id: &str, path: &Path) -> Result<(PathBuf, usize), String> {
        let rec = self
            .active
            .remove(profile_id)
            .ok_or_else(|| "Not recording".to_string())?;
        let n = rec.events.len();
        let text = format_csv(&rec);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        fs::write(path, text).map_err(|e| e.to_string())?;
        Ok((path.to_path_buf(), n))
    }

    /// Stop without saving (discard).
    pub fn cancel(&mut self, profile_id: &str) {
        self.active.remove(profile_id);
    }

    pub fn default_filename(profile_id: &str) -> String {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let safe: String = profile_id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        format!("topicbench-recording-{safe}-{secs}.csv")
    }
}

fn format_csv(rec: &ActiveRec) -> String {
    let mut out = String::new();
    out.push_str("# topicbench-recording v1\n");
    out.push_str("# profile_id,started_at_unix_ms\n");
    out.push_str(&format!("{},{}\n", rec.profile_id, rec.started_unix_ms));
    out.push_str("timestamp_ms,topic,payload,qos,retain\n");
    for e in &rec.events {
        out.push_str(&format!(
            "{},{},{},{},{}\n",
            e.t_ms,
            csv_escape(&e.topic),
            csv_escape(&e.payload),
            e.qos,
            e.retain
        ));
    }
    out
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
