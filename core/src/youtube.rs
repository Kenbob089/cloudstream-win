//! YouTube through the official YouTube Data API v3 (needs a free API key from Google Cloud,
//! entered in Settings) and YouTube's own embedded player. Pasted YouTube links work without a key
//! through the "Web links" provider.

use crate::archive::strip_html;
use crate::http::HttpClient;
use crate::models::{Catalog, MediaDetails, MediaEpisode, MediaItem, MediaType, VideoSource};
use crate::provider::{MediaProvider, ProviderDescriptor, ProviderError};
use crate::web::make_source;
use serde_json::Value;
use std::sync::{Arc, RwLock};
use url::Url;

pub const ID: &str = "native.youtube";
const API: &str = "https://www.googleapis.com/youtube/v3";

/// Shared, updatable API key (changed from Settings without restarting).
pub type KeyHandle = Arc<RwLock<String>>;

pub fn iso_duration(s: &str) -> Option<f64> {
    let rest = s.strip_prefix("PT")?;
    let mut total = 0.0;
    let mut num = String::new();
    for c in rest.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else {
            let n: f64 = num.parse().ok()?;
            num.clear();
            total += match c {
                'H' => n * 3600.0,
                'M' => n * 60.0,
                'S' => n,
                _ => return None,
            };
        }
    }
    Some(total)
}

fn best_thumb(snippet: &Value) -> Option<String> {
    let thumbs = snippet.get("thumbnails")?;
    for k in ["maxres", "standard", "high", "medium", "default"] {
        if let Some(u) = thumbs.get(k).and_then(|t| t.get("url")).and_then(|u| u.as_str()) {
            return crate::http::secure_url(u);
        }
    }
    None
}

pub fn item_from_json(v: &Value) -> Option<MediaItem> {
    // search results: id.videoId; videos endpoint: id is a string.
    let id = v.get("id").and_then(|i| i.as_str().or_else(|| i.get("videoId").and_then(|x| x.as_str())))?;
    let snippet = v.get("snippet")?;
    let title = strip_html(snippet.get("title")?.as_str()?);
    let mut item = MediaItem::new(id, ID, &title);
    item.media_type = MediaType::Video;
    let channel = snippet.get("channelTitle").and_then(|c| c.as_str()).map(strip_html).unwrap_or_default();
    let desc = snippet.get("description").and_then(|d| d.as_str()).map(strip_html).unwrap_or_default();
    let combined = match (channel.is_empty(), desc.is_empty()) {
        (true, true) => None,
        (false, true) => Some(format!("By {channel}")),
        (true, false) => Some(desc),
        (false, false) => Some(format!("By {channel}. {desc}")),
    };
    item.description = combined;
    item.poster_url = best_thumb(snippet);
    item.backdrop_url = item.poster_url.clone();
    item.release_year = snippet
        .get("publishedAt")
        .and_then(|d| d.as_str())
        .and_then(|d| d.get(0..4))
        .and_then(|y| y.parse::<i32>().ok());
    item.duration = v.get("contentDetails").and_then(|c| c.get("duration")).and_then(|d| d.as_str()).and_then(iso_duration);
    Some(item)
}

pub struct YouTubeProvider {
    client: Arc<dyn HttpClient>,
    key: KeyHandle,
    descriptor: ProviderDescriptor,
}

impl YouTubeProvider {
    pub fn new(client: Arc<dyn HttpClient>, key: KeyHandle) -> Self {
        Self {
            client,
            key,
            descriptor: ProviderDescriptor {
                id: ID.into(),
                name: "YouTube".into(),
                description: "Search and browse YouTube with your own free Data API key (Settings). Plays in YouTube's official embedded player.".into(),
                language: "en".into(),
                supported_types: vec![MediaType::Video],
                version: 1,
                author: "Built in".into(),
                source_url: Some("https://www.youtube.com".into()),
                authorization_note: "Uses the official YouTube Data API and the official embedded player, as YouTube's terms require. No streams are extracted.".into(),
            },
        }
    }

    fn call(&self, endpoint: &str, params: &[(&str, &str)]) -> Result<Value, ProviderError> {
        let key = self.key.read().unwrap().trim().to_string();
        if key.is_empty() {
            return Err(ProviderError::Message(
                "Add a YouTube API key in Settings to browse and search YouTube. Pasted YouTube links work without one.".into(),
            ));
        }
        let mut all: Vec<(&str, &str)> = params.to_vec();
        all.push(("key", key.as_str()));
        let url = Url::parse_with_params(&format!("{API}/{endpoint}"), all).map_err(|_| ProviderError::InvalidResponse)?;
        let data = self.client.get(url.as_str()).map_err(|e| match e {
            crate::http::NetError::BadStatus(400) | crate::http::NetError::BadStatus(403) => {
                ProviderError::Message("YouTube rejected the API key or the daily quota is used up. Check the key in Settings.".into())
            }
            other => ProviderError::Net(other),
        })?;
        serde_json::from_slice(&data).map_err(|_| ProviderError::InvalidResponse)
    }

    fn rows(v: &Value) -> Vec<MediaItem> {
        v.get("items").and_then(|i| i.as_array()).map(|a| a.iter().filter_map(item_from_json).collect()).unwrap_or_default()
    }
}

impl MediaProvider for YouTubeProvider {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn home_catalogs(&self) -> Result<Vec<Catalog>, ProviderError> {
        let v = self.call(
            "videos",
            &[("part", "snippet,contentDetails"), ("chart", "mostPopular"), ("maxResults", "30"), ("regionCode", "US")],
        )?;
        let items = Self::rows(&v);
        if items.is_empty() {
            return Err(ProviderError::InvalidResponse);
        }
        Ok(vec![Catalog { id: "yt.popular".into(), title: "Popular on YouTube".into(), items }])
    }

    fn search(&self, query: &str, _page: u32) -> Result<Vec<MediaItem>, ProviderError> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(vec![]);
        }
        let v = self.call("search", &[("part", "snippet"), ("type", "video"), ("maxResults", "30"), ("q", q), ("safeSearch", "moderate")])?;
        Ok(Self::rows(&v))
    }

    fn load_details(&self, id: &str) -> Result<MediaDetails, ProviderError> {
        if id.len() != 11 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return Err(ProviderError::NotFound);
        }
        let v = self.call("videos", &[("part", "snippet,contentDetails"), ("id", id)])?;
        let item = Self::rows(&v).into_iter().next().ok_or(ProviderError::NotFound)?;
        Ok(MediaDetails { item, seasons: vec![], languages: vec![] })
    }

    fn load_episodes(&self, _media_id: &str, _season: Option<i32>) -> Result<Vec<MediaEpisode>, ProviderError> {
        Ok(vec![])
    }

    fn resolve_sources(&self, media_id: &str, _episode: Option<&MediaEpisode>) -> Result<Vec<VideoSource>, ProviderError> {
        if media_id.len() != 11 || !media_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return Err(ProviderError::NotFound);
        }
        Ok(vec![make_source(
            ID,
            "YouTube",
            "Embedded",
            &format!("https://www.youtube-nocookie.com/embed/{media_id}?rel=0&autoplay=1"),
            true,
        )])
    }
}
