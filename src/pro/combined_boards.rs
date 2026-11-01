//! Combined graph boards (Pro MultiGraph) — pop-out windows own series lists.
//!
//! Session restore: `pro/combined_boards.json` (open set + contents + geometry).
//! Named library: `pro/combined_boards_library.json` (Save / Recall).

use crate::mqtt::pins::HistorySample;
use crate::mqtt::paths;
use crate::mqtt::settings::{AppSettings, COMBINED_GRAPH_H_MAX, COMBINED_GRAPH_H_MIN};
use crate::mqtt::window_geom::WinGeom;
use crate::mqtt::profile::ProfileStore;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Ring size for board display buffers (≈10 min at 1 Hz) — matches sticky Graph.
const HISTORY_CAP: usize = 600;

const SESSION_FILE: &str = "combined_boards.json";
const LIBRARY_FILE: &str = "combined_boards_library.json";

/// Per-board X-domain view filter (does not Clear or change membership).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CombinedTimeRange {
    #[default]
    Min5,
    Min1,
    Min15,
    Hour1,
    All,
}

impl CombinedTimeRange {
    pub const ALL: [Self; 5] = [
        Self::Min1,
        Self::Min5,
        Self::Min15,
        Self::Hour1,
        Self::All,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Min1 => "1 min",
            Self::Min5 => "5 min",
            Self::Min15 => "15 min",
            Self::Hour1 => "1 hour",
            Self::All => "All buffered",
        }
    }

    /// Oldest Instant to keep when filtering samples; None = no filter.
    pub fn cutoff(self, now: Instant) -> Option<Instant> {
        let secs = match self {
            Self::Min1 => 60,
            Self::Min5 => 5 * 60,
            Self::Min15 => 15 * 60,
            Self::Hour1 => 60 * 60,
            Self::All => return None,
        };
        Some(now.checked_sub(Duration::from_secs(secs)).unwrap_or(now))
    }
}

/// One series on a board — identity matches sticky / Payload Graph pin ids.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CombinedSeriesRef {
    pub profile_id: String,
    /// Topic or `topic::dotted.path` (same as sticky id).
    pub topic: String,
    #[serde(default)]
    pub profile_name: String,
    #[serde(default)]
    pub order: u32,
}

impl CombinedSeriesRef {
    pub fn series_key(&self) -> String {
        format!("{}\0{}", self.profile_id, self.topic)
    }

    pub fn key(profile_id: &str, topic: &str) -> String {
        format!("{profile_id}\0{topic}")
    }
}

/// Persisted board shape (session + library share series/timeline/height).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CombinedBoardPersist {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub series: Vec<CombinedSeriesRef>,
    #[serde(default)]
    pub time_range: CombinedTimeRange,
    #[serde(default = "default_plot_height")]
    pub plot_height: f32,
    #[serde(default)]
    pub window_geom: Option<WinGeom>,
    #[serde(default)]
    pub open: bool,
}

fn default_plot_height() -> f32 {
    120.0
}

/// Named Save/Recall entry (no window geom / open flag).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CombinedLibraryEntry {
    pub name: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub series: Vec<CombinedSeriesRef>,
    #[serde(default)]
    pub time_range: CombinedTimeRange,
    #[serde(default = "default_plot_height")]
    pub plot_height: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SessionFile {
    version: u32,
    boards: Vec<CombinedBoardPersist>,
    /// Most recently focused open board (add target).
    #[serde(default)]
    last_focused_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct LibraryFile {
    version: u32,
    boards: Vec<CombinedLibraryEntry>,
}

/// Runtime board: persist fields + in-memory display buffers.
#[derive(Clone, Debug)]
pub struct CombinedBoard {
    pub id: String,
    pub title: String,
    pub series: Vec<CombinedSeriesRef>,
    pub time_range: CombinedTimeRange,
    pub plot_height: f32,
    pub window_geom: Option<WinGeom>,
    pub open: bool,
    /// Per-series display samples (Clear empties these only).
    pub buffers: HashMap<String, VecDeque<HistorySample>>,
}

impl CombinedBoard {
    fn from_persist(p: CombinedBoardPersist) -> Self {
        let mut buffers = HashMap::new();
        for s in &p.series {
            buffers.insert(s.series_key(), VecDeque::new());
        }
        Self {
            id: p.id,
            title: p.title,
            series: p.series,
            time_range: p.time_range,
            plot_height: clamp_height(p.plot_height),
            window_geom: p.window_geom,
            open: p.open,
            buffers,
        }
    }

    fn to_persist(&self) -> CombinedBoardPersist {
        CombinedBoardPersist {
            id: self.id.clone(),
            title: self.title.clone(),
            series: self.series.clone(),
            time_range: self.time_range,
            plot_height: self.plot_height,
            window_geom: self.window_geom,
            open: self.open,
        }
    }

    pub fn display_title(&self) -> String {
        let t = self.title.trim();
        if t.is_empty() {
            "Combined".into()
        } else {
            t.to_string()
        }
    }
}

/// Session boards + named library.
pub struct CombinedBoardsStore {
    pub boards: Vec<CombinedBoard>,
    pub last_focused_id: Option<String>,
    pub library: Vec<CombinedLibraryEntry>,
    dirty: bool,
}

impl Default for CombinedBoardsStore {
    fn default() -> Self {
        Self {
            boards: Vec::new(),
            last_focused_id: None,
            library: Vec::new(),
            dirty: false,
        }
    }
}

impl CombinedBoardsStore {
    pub fn load() -> Self {
        let mut s = Self::default();
        s.reload_session();
        s.reload_library();
        s
    }

    pub fn session_path() -> PathBuf {
        paths::config_dir().join("pro").join(SESSION_FILE)
    }

    pub fn library_path() -> PathBuf {
        paths::config_dir().join("pro").join(LIBRARY_FILE)
    }

    fn reload_session(&mut self) {
        let path = Self::session_path();
        let Ok(text) = fs::read_to_string(&path) else {
            return;
        };
        let Ok(file) = serde_json::from_str::<SessionFile>(&text) else {
            return;
        };
        self.boards = file
            .boards
            .into_iter()
            .map(CombinedBoard::from_persist)
            .collect();
        self.last_focused_id = file.last_focused_id;
        self.dirty = false;
    }

    fn reload_library(&mut self) {
        let path = Self::library_path();
        let Ok(text) = fs::read_to_string(&path) else {
            self.library.clear();
            return;
        };
        match serde_json::from_str::<LibraryFile>(&text) {
            Ok(f) => self.library = f.boards,
            Err(_) => self.library.clear(),
        }
    }

    pub fn save_session(&mut self) -> Result<(), String> {
        let path = Self::session_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let body = SessionFile {
            version: 1,
            boards: self.boards.iter().map(CombinedBoard::to_persist).collect(),
            last_focused_id: self.last_focused_id.clone(),
        };
        let text = serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?;
        fs::write(&path, text).map_err(|e| e.to_string())?;
        self.dirty = false;
        Ok(())
    }

    pub fn save_library(&self) -> Result<(), String> {
        let path = Self::library_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let body = LibraryFile {
            version: 1,
            boards: self.library.clone(),
        };
        let text = serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?;
        fs::write(&path, text).map_err(|e| e.to_string())
    }

    /// Persist if membership / open / timeline / height / geom changed.
    pub fn save_session_if_dirty(&mut self) {
        if self.dirty {
            let _ = self.save_session();
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn any_open(&self) -> bool {
        self.boards.iter().any(|b| b.open)
    }

    /// One-shot: stickies with `in_combined` + legacy `combined_graph_open` → default board.
    pub fn migrate_from_legacy(
        &mut self,
        settings: &mut AppSettings,
        profiles: &ProfileStore,
    ) {
        if Self::session_path().is_file() {
            // Still clear the legacy flag so docked Combined stays retired.
            if settings.combined_graph_open {
                settings.combined_graph_open = false;
                let _ = settings.save();
            }
            return;
        }
        let mut series: Vec<CombinedSeriesRef> = Vec::new();
        let mut seen = HashSet::new();
        for p in profiles.profiles.values() {
            for s in &p.stickies {
                if !s.in_combined {
                    continue;
                }
                let key = CombinedSeriesRef::key(&p.id, &s.topic);
                if !seen.insert(key) {
                    continue;
                }
                series.push(CombinedSeriesRef {
                    profile_id: p.id.clone(),
                    topic: s.topic.clone(),
                    profile_name: p.name.clone(),
                    order: series.len() as u32,
                });
            }
        }
        let open = settings.combined_graph_open || !series.is_empty();
        if !open && series.is_empty() {
            return;
        }
        let id = new_id();
        let height = clamp_height(settings.combined_graph_height);
        self.boards.push(CombinedBoard {
            id: id.clone(),
            title: "Combined".into(),
            series,
            time_range: CombinedTimeRange::default(),
            plot_height: height,
            window_geom: None,
            open,
            buffers: HashMap::new(),
        });
        if open {
            self.last_focused_id = Some(id);
        }
        settings.combined_graph_open = false;
        let _ = settings.save();
        let _ = self.save_session();
    }

    pub fn mark_focused(&mut self, board_id: &str) {
        if self.last_focused_id.as_deref() != Some(board_id) {
            self.last_focused_id = Some(board_id.to_string());
            self.dirty = true;
        }
    }

    /// Open boards in stable order (for drawing viewports).
    pub fn open_board_ids(&self) -> Vec<String> {
        self.boards
            .iter()
            .filter(|b| b.open)
            .map(|b| b.id.clone())
            .collect()
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut CombinedBoard> {
        self.boards.iter_mut().find(|b| b.id == id)
    }

    pub fn get(&self, id: &str) -> Option<&CombinedBoard> {
        self.boards.iter().find(|b| b.id == id)
    }

    /// Target for Add: most recently focused open board, else sole open, else new.
    pub fn ensure_add_target(&mut self) -> String {
        let open: Vec<String> = self
            .boards
            .iter()
            .filter(|b| b.open)
            .map(|b| b.id.clone())
            .collect();
        if open.len() == 1 {
            let id = open[0].clone();
            self.last_focused_id = Some(id.clone());
            return id;
        }
        if let Some(fid) = self.last_focused_id.clone() {
            if open.iter().any(|id| id == &fid) {
                return fid;
            }
        }
        if let Some(id) = open.first() {
            self.last_focused_id = Some(id.clone());
            return id.clone();
        }
        self.create_board(true)
    }

    pub fn create_board(&mut self, open: bool) -> String {
        let n = self.boards.len() + 1;
        let title = if n <= 1 {
            "Combined".into()
        } else {
            format!("Combined {n}")
        };
        let id = new_id();
        self.boards.push(CombinedBoard {
            id: id.clone(),
            title,
            series: Vec::new(),
            time_range: CombinedTimeRange::default(),
            plot_height: default_plot_height(),
            window_geom: None,
            open,
            buffers: HashMap::new(),
        });
        if open {
            self.last_focused_id = Some(id.clone());
        }
        self.dirty = true;
        id
    }

    /// Add series to focused/new board. Returns (board_id, newly_added).
    pub fn add_series(
        &mut self,
        profile_id: &str,
        profile_name: &str,
        topic: &str,
        seed: Option<&VecDeque<HistorySample>>,
    ) -> (String, bool) {
        let board_id = self.ensure_add_target();
        let added = self.add_series_to(&board_id, profile_id, profile_name, topic, seed);
        (board_id, added)
    }

    pub fn add_series_to(
        &mut self,
        board_id: &str,
        profile_id: &str,
        profile_name: &str,
        topic: &str,
        seed: Option<&VecDeque<HistorySample>>,
    ) -> bool {
        let Some(board) = self.boards.iter_mut().find(|b| b.id == board_id) else {
            return false;
        };
        board.open = true;
        let key = CombinedSeriesRef::key(profile_id, topic);
        if board.series.iter().any(|s| s.series_key() == key) {
            // Refresh name; keep existing buffer.
            if let Some(s) = board.series.iter_mut().find(|s| s.series_key() == key) {
                if !profile_name.is_empty() {
                    s.profile_name = profile_name.to_string();
                }
            }
            return false;
        }
        let order = board.series.len() as u32;
        board.series.push(CombinedSeriesRef {
            profile_id: profile_id.to_string(),
            topic: topic.to_string(),
            profile_name: profile_name.to_string(),
            order,
        });
        let mut buf = VecDeque::new();
        if let Some(hist) = seed {
            for s in hist {
                buf.push_back(s.clone());
            }
            while buf.len() > HISTORY_CAP {
                buf.pop_front();
            }
        }
        board.buffers.insert(key, buf);
        self.last_focused_id = Some(board_id.to_string());
        self.dirty = true;
        true
    }

    pub fn remove_series(&mut self, board_id: &str, profile_id: &str, topic: &str) {
        let Some(board) = self.boards.iter_mut().find(|b| b.id == board_id) else {
            return;
        };
        let key = CombinedSeriesRef::key(profile_id, topic);
        board.series.retain(|s| s.series_key() != key);
        for (i, s) in board.series.iter_mut().enumerate() {
            s.order = i as u32;
        }
        board.buffers.remove(&key);
        self.dirty = true;
    }

    /// Clear = empty this board's display buffers only (membership untouched).
    pub fn clear_display_buffer(&mut self, board_id: &str) {
        let Some(board) = self.boards.iter_mut().find(|b| b.id == board_id) else {
            return;
        };
        for buf in board.buffers.values_mut() {
            buf.clear();
        }
    }

    pub fn close_board(&mut self, board_id: &str) {
        if let Some(board) = self.boards.iter_mut().find(|b| b.id == board_id) {
            board.open = false;
            self.dirty = true;
        }
        if self.last_focused_id.as_deref() == Some(board_id) {
            self.last_focused_id = self
                .boards
                .iter()
                .find(|b| b.open)
                .map(|b| b.id.clone());
        }
    }

    /// Save current board into the named library (replace same name).
    pub fn save_board_as(&mut self, board_id: &str, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Name required".into());
        }
        let Some(board) = self.boards.iter().find(|b| b.id == board_id) else {
            return Err("Board not found".into());
        };
        let entry = CombinedLibraryEntry {
            name: name.to_string(),
            title: board.title.clone(),
            series: board.series.clone(),
            time_range: board.time_range,
            plot_height: board.plot_height,
        };
        if let Some(existing) = self.library.iter_mut().find(|e| e.name == name) {
            *existing = entry;
        } else {
            self.library.push(entry);
        }
        self.save_library()
    }

    /// Recall a named library board into a new open window.
    pub fn recall_library(&mut self, name: &str) -> Option<String> {
        let entry = self.library.iter().find(|e| e.name == name)?.clone();
        let id = new_id();
        let mut buffers = HashMap::new();
        for s in &entry.series {
            buffers.insert(s.series_key(), VecDeque::new());
        }
        let title = if entry.title.trim().is_empty() {
            entry.name.clone()
        } else {
            entry.title.clone()
        };
        self.boards.push(CombinedBoard {
            id: id.clone(),
            title,
            series: entry.series,
            time_range: entry.time_range,
            plot_height: clamp_height(entry.plot_height),
            window_geom: None,
            open: true,
            buffers,
        });
        self.last_focused_id = Some(id.clone());
        self.dirty = true;
        Some(id)
    }

    pub fn delete_library_entry(&mut self, name: &str) -> Result<(), String> {
        self.library.retain(|e| e.name != name);
        self.save_library()
    }

    /// Live feed — update every board that lists a matching series (sticky optional).
    pub fn on_message_samples(
        &mut self,
        profile_id: &str,
        profile_name: &str,
        topic: &str,
        samples: &[(Instant, String)],
    ) {
        if samples.is_empty() || self.boards.is_empty() {
            return;
        }
        for board in &mut self.boards {
            if board.series.is_empty() {
                continue;
            }
            // Collect matching series indices first (avoid borrow issues).
            let matches: Vec<usize> = board
                .series
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    if s.profile_id != profile_id {
                        return false;
                    }
                    let (base, _) = crate::mqtt::value_path::split_sticky_id(&s.topic);
                    base == topic
                        || crate::mqtt::subscribe::topic_matches_filter(topic, base)
                })
                .map(|(i, _)| i)
                .collect();
            for idx in matches {
                let series = &mut board.series[idx];
                if !profile_name.is_empty() {
                    series.profile_name = profile_name.to_string();
                }
                let (base, path) = crate::mqtt::value_path::split_sticky_id(&series.topic);
                let _ = base; // matched above
                let key = series.series_key();
                let buf = board
                    .buffers
                    .entry(key)
                    .or_insert_with(VecDeque::new);
                for (at, payload) in samples {
                    let extracted = match path {
                        Some(p) => crate::mqtt::value_path::extract_path_value(payload, p)
                            .unwrap_or_else(|| payload.to_string()),
                        None => payload.clone(),
                    };
                    let numeric = crate::mqtt::value_path::sample_numeric(&extracted);
                    buf.push_back(HistorySample {
                        at: *at,
                        payload: extracted,
                        numeric,
                    });
                    while buf.len() > HISTORY_CAP {
                        buf.pop_front();
                    }
                }
            }
        }
    }
}

pub fn clamp_height(h: f32) -> f32 {
    if !h.is_finite() {
        return default_plot_height();
    }
    h.clamp(COMBINED_GRAPH_H_MIN, COMBINED_GRAPH_H_MAX)
}

fn new_id() -> String {
    format!("cb-{}", uuid::Uuid::new_v4())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_keeps_membership() {
        let mut store = CombinedBoardsStore::default();
        let id = store.create_board(true);
        store.add_series_to(&id, "p1", "Broker", "sensors/temp", None);
        {
            let board = store.get_mut(&id).unwrap();
            let key = CombinedSeriesRef::key("p1", "sensors/temp");
            board.buffers.get_mut(&key).unwrap().push_back(HistorySample {
                at: Instant::now(),
                payload: "1".into(),
                numeric: Some(1.0),
            });
        }
        store.clear_display_buffer(&id);
        let board = store.get(&id).unwrap();
        assert_eq!(board.series.len(), 1);
        let key = CombinedSeriesRef::key("p1", "sensors/temp");
        assert!(board.buffers.get(&key).unwrap().is_empty());
    }

    #[test]
    fn add_targets_focused_open_board() {
        let mut store = CombinedBoardsStore::default();
        let a = store.create_board(true);
        let b = store.create_board(true);
        store.mark_focused(&b);
        let (target, added) = store.add_series("p", "N", "t/x", None);
        assert_eq!(target, b);
        assert!(added);
        assert_eq!(store.get(&a).unwrap().series.len(), 0);
        assert_eq!(store.get(&b).unwrap().series.len(), 1);
    }
}
