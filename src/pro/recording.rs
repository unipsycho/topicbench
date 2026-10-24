//! Live MQTT record → CSV tape (same shape as Simulation Playback).
//!
//! Controls live on topic-tree broker headers (Pro). Playback stays on Simulate.
//! Soft max duration (default 3 min) auto-stops with a toast + Suggest offer.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Default soft cap for a recording session (~3 minutes).
pub const DEFAULT_MAX_SECS: u64 = 180;

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
    profile_name: String,
    started: Instant,
    started_unix_ms: u64,
    events: Vec<RecEvent>,
    max_secs: u64,
}

/// Per-profile recording sessions (at most one active tape per profile).
pub struct RecordingStore {
    active: HashMap<String, ActiveRec>,
    /// Soft max duration for new recordings.
    pub max_secs: u64,
}

impl Default for RecordingStore {
    fn default() -> Self {
        Self {
            active: HashMap::new(),
            max_secs: DEFAULT_MAX_SECS,
        }
    }
}

/// Result of an auto-stop (caller should toast + offer Suggest).
#[derive(Clone, Debug)]
pub struct AutoStopped {
    pub profile_id: String,
    pub profile_name: String,
    pub path: PathBuf,
    pub events: usize,
    pub csv: String,
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

    /// Elapsed seconds for an active recording.
    pub fn elapsed_secs(&self, profile_id: &str) -> Option<u64> {
        self.active
            .get(profile_id)
            .map(|r| r.started.elapsed().as_secs())
    }

    /// Remaining seconds until soft auto-stop (0 when at/over cap).
    pub fn remaining_secs(&self, profile_id: &str) -> Option<u64> {
        let r = self.active.get(profile_id)?;
        let elapsed = r.started.elapsed().as_secs();
        Some(r.max_secs.saturating_sub(elapsed))
    }

    pub fn max_secs_for(&self, profile_id: &str) -> u64 {
        self.active
            .get(profile_id)
            .map(|r| r.max_secs)
            .unwrap_or(self.max_secs)
    }

    pub fn start(&mut self, profile_id: &str, profile_name: &str) {
        let started_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        self.active.insert(
            profile_id.to_string(),
            ActiveRec {
                profile_id: profile_id.to_string(),
                profile_name: profile_name.to_string(),
                started: Instant::now(),
                started_unix_ms,
                events: Vec::new(),
                max_secs: self.max_secs,
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

    /// Stop and write CSV. Returns (path, event_count, csv_text) or error. Clears active entry.
    pub fn stop_to_file(
        &mut self,
        profile_id: &str,
        path: &Path,
    ) -> Result<(PathBuf, usize, String), String> {
        let rec = self
            .active
            .remove(profile_id)
            .ok_or_else(|| "Not recording".to_string())?;
        let n = rec.events.len();
        let text = format_csv(&rec);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        fs::write(path, &text).map_err(|e| e.to_string())?;
        Ok((path.to_path_buf(), n, text))
    }

    /// Auto-stop profiles that hit the soft max; save under default recordings dir.
    pub fn auto_stop_expired(&mut self) -> Vec<AutoStopped> {
        let now_ids: Vec<String> = self
            .active
            .iter()
            .filter(|(_, r)| r.started.elapsed() >= Duration::from_secs(r.max_secs))
            .map(|(id, _)| id.clone())
            .collect();
        let mut out = Vec::new();
        for id in now_ids {
            let name = self
                .active
                .get(&id)
                .map(|r| r.profile_name.clone())
                .unwrap_or_else(|| id.clone());
            let dir = default_recordings_dir();
            let _ = fs::create_dir_all(&dir);
            let path = dir.join(Self::default_filename(&id));
            match self.stop_to_file(&id, &path) {
                Ok((path, n, csv)) => out.push(AutoStopped {
                    profile_id: id,
                    profile_name: name,
                    path,
                    events: n,
                    csv,
                }),
                Err(_) => {
                    self.active.remove(&id);
                }
            }
        }
        out
    }

    /// Stop without saving (discard).
    pub fn cancel(&mut self, profile_id: &str) {
        self.active.remove(profile_id);
    }

    /// Any active recording (for idle repaint / timer).
    pub fn any_active(&self) -> bool {
        !self.active.is_empty()
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

fn default_recordings_dir() -> PathBuf {
    crate::mqtt::paths::config_dir().join("recordings")
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
