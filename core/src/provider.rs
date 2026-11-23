use crate::http::NetError;
use crate::models::{Catalog, MediaDetails, MediaEpisode, MediaItem, MediaType, VideoSource};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDescriptor {
    pub id: String,
    pub name: String,
    pub description: String,
    pub language: String,
    pub supported_types: Vec<MediaType>,
    pub version: u32,
    pub author: String,
    pub source_url: Option<String>,
    /// How the content source authorizes use — shown to the user.
    pub authorization_note: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderError {
    NotFound,
    NoPlayableSource,
    InvalidResponse,
    Net(NetError),
    Message(String),
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(f, "That item could not be found."),
            Self::NoPlayableSource => write!(f, "This provider returned no playable source."),
            Self::InvalidResponse => write!(f, "The provider returned a response the app could not understand."),
            Self::Net(e) => write!(f, "{e}"),
            Self::Message(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for ProviderError {}

impl From<NetError> for ProviderError {
    fn from(e: NetError) -> Self {
        Self::Net(e)
    }
}

/// Reviewed-native provider interface. Implementations are compiled Rust.
pub trait MediaProvider: Send + Sync {
    fn descriptor(&self) -> &ProviderDescriptor;
    fn home_catalogs(&self) -> Result<Vec<Catalog>, ProviderError> {
        Ok(vec![])
    }
    fn search(&self, query: &str, page: u32) -> Result<Vec<MediaItem>, ProviderError>;
    fn load_details(&self, id: &str) -> Result<MediaDetails, ProviderError>;
    fn load_episodes(&self, media_id: &str, season: Option<i32>) -> Result<Vec<MediaEpisode>, ProviderError>;
    /// `episode` is None for single-video items.
    fn resolve_sources(&self, media_id: &str, episode: Option<&MediaEpisode>) -> Result<Vec<VideoSource>, ProviderError>;
}

pub type SearchOutcome = (ProviderDescriptor, Result<Vec<MediaItem>, ProviderError>);

/// Thread-safe registry of *enabled* providers. Home and Search only see what is registered here.
#[derive(Default)]
pub struct ProviderRegistry {
    providers: RwLock<Vec<Arc<dyn MediaProvider>>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, provider: Arc<dyn MediaProvider>) {
        let mut guard = self.providers.write().unwrap();
        let id = provider.descriptor().id.clone();
        if let Some(slot) = guard.iter_mut().find(|p| p.descriptor().id == id) {
            *slot = provider;
        } else {
            guard.push(provider);
        }
    }

    pub fn unregister(&self, id: &str) {
        self.providers.write().unwrap().retain(|p| p.descriptor().id != id);
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn MediaProvider>> {
        self.providers.read().unwrap().iter().find(|p| p.descriptor().id == id).cloned()
    }

    pub fn all(&self) -> Vec<Arc<dyn MediaProvider>> {
        self.providers.read().unwrap().clone()
    }

    pub fn ids(&self) -> Vec<String> {
        self.all().iter().map(|p| p.descriptor().id.clone()).collect()
    }

    /// Searches every registered provider concurrently; failures are reported per provider.
    pub fn search_all(&self, query: &str, page: u32) -> Vec<SearchOutcome> {
        let snapshot = self.all();
        std::thread::scope(|s| {
            let handles: Vec<_> = snapshot
                .iter()
                .map(|p| s.spawn(move || (p.descriptor().clone(), p.search(query, page))))
                .collect();
            handles
                .into_iter()
                .zip(snapshot.iter())
                .map(|(h, p)| {
                    h.join()
                        .unwrap_or_else(|_| (p.descriptor().clone(), Err(ProviderError::InvalidResponse)))
                })
                .collect()
        })
    }
}
