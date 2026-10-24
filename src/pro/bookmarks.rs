//! Topic bookmarks store (Pro) — jump list persisted under `pro/bookmarks.json`.

use crate::mqtt::paths;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const FILE_NAME: &str = "bookmarks.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: String,
    pub profile_id: String,
    pub topic: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub expand_subtree: bool,
    #[serde(default)]
    pub order: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct FileShape {
    version: u32,
    bookmarks: Vec<Bookmark>,
}

#[derive(Default)]
pub struct BookmarksStore {
    pub items: Vec<Bookmark>,
    loaded: bool,
}

impl BookmarksStore {
    pub fn load() -> Self {
        let mut s = Self::default();
        s.reload();
        s
    }

    pub fn path() -> PathBuf {
        paths::config_dir().join("pro").join(FILE_NAME)
    }

    pub fn reload(&mut self) {
        self.loaded = true;
        let path = Self::path();
        let Ok(text) = fs::read_to_string(&path) else {
            self.items.clear();
            return;
        };
        match serde_json::from_str::<FileShape>(&text) {
            Ok(f) => {
                self.items = f.bookmarks;
                self.items.sort_by_key(|b| b.order);
            }
            Err(_) => self.items.clear(),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut bookmarks = self.items.clone();
        for (i, b) in bookmarks.iter_mut().enumerate() {
            b.order = i as u32;
        }
        let body = FileShape {
            version: 1,
            bookmarks,
        };
        let text = serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?;
        fs::write(&path, text).map_err(|e| e.to_string())
    }

    pub fn export_to(&self, dest: &std::path::Path) -> Result<(), String> {
        let path = Self::path();
        if path.is_file() {
            fs::copy(&path, dest).map_err(|e| e.to_string())?;
        } else {
            self.save()?;
            fs::copy(&path, dest).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn is_bookmarked(&self, profile_id: &str, topic: &str) -> bool {
        self.items
            .iter()
            .any(|b| b.profile_id == profile_id && b.topic == topic)
    }

    /// Toggle bookmark for a tree path. Returns true if now bookmarked.
    pub fn toggle(&mut self, profile_id: &str, topic: &str) -> bool {
        if let Some(i) = self
            .items
            .iter()
            .position(|b| b.profile_id == profile_id && b.topic == topic)
        {
            self.items.remove(i);
            let _ = self.save();
            false
        } else {
            let label = topic
                .rsplit('/')
                .next()
                .unwrap_or(topic)
                .to_string();
            self.items.push(Bookmark {
                id: new_id(),
                profile_id: profile_id.to_string(),
                topic: topic.to_string(),
                label,
                expand_subtree: false,
                order: self.items.len() as u32,
            });
            let _ = self.save();
            true
        }
    }

    pub fn remove_id(&mut self, id: &str) {
        self.items.retain(|b| b.id != id);
        let _ = self.save();
    }

    pub fn set_expand(&mut self, id: &str, expand: bool) {
        if let Some(b) = self.items.iter_mut().find(|b| b.id == id) {
            b.expand_subtree = expand;
            let _ = self.save();
        }
    }

    pub fn reorder(&mut self, from: usize, to: usize) {
        if from >= self.items.len() || to >= self.items.len() || from == to {
            return;
        }
        let item = self.items.remove(from);
        self.items.insert(to, item);
        let _ = self.save();
    }
}

fn new_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("bm-{ns:x}")
}
