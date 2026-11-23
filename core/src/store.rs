use crate::models::{now_secs, MediaEpisode, MediaItem};
use crate::repo::{PluginEntry, RepositoryImport};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::PathBuf;
use std::sync::Mutex;

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bookmark {
    pub item: MediaItem,
    pub is_favorite: bool,
    pub added_at: i64,
}

/// One row per playable unit (a movie, or one episode). Drives Continue Watching and History.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEntry {
    pub key: String,
    pub media_key: String,
    pub item: MediaItem,
    pub episode_id: Option<String>,
    pub episode_title: Option<String>,
    pub season_number: Option<i32>,
    pub episode_number: Option<i32>,
    pub position: f64,
    pub duration: f64,
    pub completed: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedRepo {
    pub url: String,
    pub name: String,
    pub description: Option<String>,
    pub icon_url: Option<String>,
    pub last_refreshed: Option<i64>,
    pub last_error: Option<String>,
    #[serde(default)]
    pub plugins: Vec<PluginEntry>,
    #[serde(default)]
    pub warnings: Vec<String>,
    pub added_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryQuery {
    pub query: String,
    pub date: i64,
}

/// Everything persisted to disk. Unknown/missing fields are tolerated so updates never wipe data.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StoreData {
    pub version: u32,
    pub bookmarks: Vec<Bookmark>,
    pub progress: Vec<ProgressEntry>,
    pub search_history: Vec<HistoryQuery>,
    pub repositories: Vec<SavedRepo>,
    pub installs: Vec<String>,
    pub custom_providers: Vec<crate::custom::CustomDefinition>,
    pub settings: Map<String, Value>,
}

pub fn progress_key(item: &MediaItem, episode_id: Option<&str>) -> String {
    match episode_id {
        Some(e) => format!("{}::{}", item.key(), e),
        None => item.key(),
    }
}

impl StoreData {
    /// Returns true when the item is bookmarked afterwards.
    pub fn toggle_bookmark(&mut self, item: &MediaItem) -> bool {
        let key = item.key();
        if let Some(pos) = self.bookmarks.iter().position(|b| b.item.key() == key) {
            self.bookmarks.remove(pos);
            false
        } else {
            self.bookmarks.insert(0, Bookmark { item: item.clone(), is_favorite: false, added_at: now_secs() });
            true
        }
    }

    pub fn toggle_favorite(&mut self, item: &MediaItem) {
        let key = item.key();
        if let Some(b) = self.bookmarks.iter_mut().find(|b| b.item.key() == key) {
            b.is_favorite = !b.is_favorite;
        } else {
            self.bookmarks.insert(0, Bookmark { item: item.clone(), is_favorite: true, added_at: now_secs() });
        }
    }

    pub fn record_progress(
        &mut self,
        item: &MediaItem,
        episode: Option<&MediaEpisode>,
        position: f64,
        duration: f64,
        completed: bool,
    ) {
        let key = progress_key(item, episode.map(|e| e.id.as_str()));
        let done = completed || (duration > 0.0 && position / duration > 0.95);
        let entry = ProgressEntry {
            key: key.clone(),
            media_key: item.key(),
            item: item.clone(),
            episode_id: episode.map(|e| e.id.clone()),
            episode_title: episode.map(|e| e.title.clone()),
            season_number: episode.and_then(|e| e.season_number),
            episode_number: episode.and_then(|e| e.episode_number),
            position,
            duration,
            completed: done,
            updated_at: now_secs(),
        };
        match self.progress.iter_mut().find(|p| p.key == key) {
            Some(existing) => *existing = entry,
            None => self.progress.push(entry),
        }
    }

    pub fn add_search(&mut self, query: &str) {
        self.search_history.retain(|h| h.query != query);
        self.search_history.insert(0, HistoryQuery { query: query.into(), date: now_secs() });
        self.search_history.truncate(50);
    }

    pub fn upsert_repo(&mut self, import: &RepositoryImport) {
        let url = import.repository_url.clone();
        let icon = import.manifest.icon_url.as_deref().and_then(crate::http::secure_url);
        let saved = SavedRepo {
            url: url.clone(),
            name: import.manifest.name.clone(),
            description: import.manifest.description.clone(),
            icon_url: icon,
            last_refreshed: Some(import.fetched_at),
            last_error: None,
            plugins: import.plugins.clone(),
            warnings: import.warnings.clone(),
            added_at: now_secs(),
        };
        match self.repositories.iter_mut().find(|r| r.url == url) {
            Some(existing) => {
                let added = existing.added_at;
                *existing = SavedRepo { added_at: added, ..saved };
            }
            None => self.repositories.push(saved),
        }
    }
}

/// JSON-file store with atomic writes. Mutations save immediately.
pub struct Store {
    path: Option<PathBuf>,
    data: Mutex<StoreData>,
}

impl Store {
    pub fn in_memory() -> Self {
        Self { path: None, data: Mutex::new(StoreData { version: SCHEMA_VERSION, ..Default::default() }) }
    }

    /// Opens (or creates) the store. A corrupt file is moved aside rather than overwritten.
    pub fn open(path: PathBuf) -> Self {
        let data = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<StoreData>(&bytes) {
                Ok(mut d) => {
                    d.version = SCHEMA_VERSION; // add migrations here when the schema changes
                    d
                }
                Err(_) => {
                    let _ = std::fs::rename(&path, path.with_extension("corrupt.json"));
                    StoreData { version: SCHEMA_VERSION, ..Default::default() }
                }
            },
            Err(_) => StoreData { version: SCHEMA_VERSION, ..Default::default() },
        };
        Self { path: Some(path), data: Mutex::new(data) }
    }

    pub fn read<R>(&self, f: impl FnOnce(&StoreData) -> R) -> R {
        f(&self.data.lock().unwrap())
    }

    /// Mutates under the lock, then persists.
    pub fn with<R>(&self, f: impl FnOnce(&mut StoreData) -> R) -> R {
        let mut guard = self.data.lock().unwrap();
        let result = f(&mut guard);
        self.save(&guard);
        result
    }

    fn save(&self, data: &StoreData) {
        let Some(path) = &self.path else { return };
        let Ok(bytes) = serde_json::to_vec_pretty(data) else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}
