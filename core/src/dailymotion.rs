//! Dailymotion via its public read-only Data API. Playback uses Dailymotion's own embed player.

use crate::archive::strip_html;
use crate::http::{secure_url, HttpClient};
use crate::models::{Catalog, MediaDetails, MediaEpisode, MediaItem, MediaType, VideoSource};
use crate::provider::{MediaProvider, ProviderDescriptor, ProviderError};
use crate::web::make_source;
use serde_json::Value;
use std::sync::Arc;
use url::Url;

pub const ID: &str = "native.dailymotion";
const FIELDS: &str = "id,title,description,thumbnail_480_url,duration,created_time,tags";

pub fn item_from_json(v: &Value) -> Option<MediaItem> {
    let id = v.get("id")?.as_str()?;
    let title = v.get("title")?.as_str()?;
    let mut item = MediaItem::new(id, ID, &strip_html(title));
    item.media_type = MediaType::Video;
    item.description = v.get("description").and_then(|d| d.as_str()).map(strip_html).filter(|s| !s.is_empty());
    item.poster_url = v.get("thumbnail_480_url").and_then(|u| u.as_str()).and_then(secure_url);
    item.backdrop_url = item.poster_url.clone();
    item.duration = v.get("duration").and_then(|d| d.as_f64());
    item.release_year = v
        .get("created_time")
        .and_then(|t| t.as_i64())
        .map(|secs| 1970 + (secs / 31_556_952) as i32);
    item.genres = v
        .get("tags")
        .and_then(|t| t.as_array())
        .map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).take(6).collect())
        .unwrap_or_default();
    Some(item)
}

pub struct DailymotionProvider {
    client: Arc<dyn HttpClient>,
    descriptor: ProviderDescriptor,
}

impl DailymotionProvider {
    pub fn new(client: Arc<dyn HttpClient>) -> Self {
        Self {
            client,
            descriptor: ProviderDescriptor {
                id: ID.into(),
                name: "Dailymotion".into(),
                description: "Browse and search public Dailymotion videos.".into(),
                language: "en".into(),
                supported_types: vec![MediaType::Video],
                version: 1,
                author: "Built in".into(),
                source_url: Some("https://www.dailymotion.com".into()),
                authorization_note: "Uses Dailymotion's public API and its official embed player, so playback follows the uploader's own availability rules.".into(),
            },
        }
    }

    fn list(&self, sort: Option<&str>, search: Option<&str>, page: u32) -> Result<Vec<MediaItem>, ProviderError> {
        let page_s = page.max(1).to_string();
        let mut params: Vec<(&str, &str)> = vec![("fields", FIELDS), ("limit", "30"), ("page", page_s.as_str())];
        if let Some(s) = sort {
            params.push(("sort", s));
        }
        if let Some(q) = search {
            params.push(("search", q));
        }
        let url = Url::parse_with_params("https://api.dailymotion.com/videos", params).map_err(|_| ProviderError::InvalidResponse)?;
        let v: Value = serde_json::from_slice(&self.client.get(url.as_str())?).map_err(|_| ProviderError::InvalidResponse)?;
        let rows = v.get("list").and_then(|l| l.as_array()).ok_or(ProviderError::InvalidResponse)?;
        Ok(rows.iter().filter_map(item_from_json).collect())
    }
}

impl MediaProvider for DailymotionProvider {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn home_catalogs(&self) -> Result<Vec<Catalog>, ProviderError> {
        let (a, b) = std::thread::scope(|s| {
            let t = s.spawn(|| self.list(Some("trending"), None, 1).ok());
            let w = s.spawn(|| self.list(Some("visited-week"), None, 1).ok());
            (t.join().ok().flatten(), w.join().ok().flatten())
        });
        let mut out = vec![];
        if let Some(items) = a.filter(|i| !i.is_empty()) {
            out.push(Catalog { id: "dm.trending".into(), title: "Trending".into(), items });
        }
        if let Some(items) = b.filter(|i| !i.is_empty()) {
            out.push(Catalog { id: "dm.week".into(), title: "Most watched this week".into(), items });
        }
        if out.is_empty() {
            return Err(ProviderError::InvalidResponse);
        }
        Ok(out)
    }

    fn search(&self, query: &str, page: u32) -> Result<Vec<MediaItem>, ProviderError> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(vec![]);
        }
        self.list(Some("relevance"), Some(q), page)
    }

    fn load_details(&self, id: &str) -> Result<MediaDetails, ProviderError> {
        if !id.chars().all(|c| c.is_ascii_alphanumeric()) || id.is_empty() {
            return Err(ProviderError::NotFound);
        }
        let url = format!("https://api.dailymotion.com/video/{id}?fields={FIELDS}");
        let v: Value = serde_json::from_slice(&self.client.get(&url)?).map_err(|_| ProviderError::NotFound)?;
        Ok(MediaDetails { item: item_from_json(&v).ok_or(ProviderError::NotFound)?, seasons: vec![], languages: vec![] })
    }

    fn load_episodes(&self, _media_id: &str, _season: Option<i32>) -> Result<Vec<MediaEpisode>, ProviderError> {
        Ok(vec![])
    }

    fn resolve_sources(&self, media_id: &str, _episode: Option<&MediaEpisode>) -> Result<Vec<VideoSource>, ProviderError> {
        if media_id.is_empty() || !media_id.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(ProviderError::NotFound);
        }
        Ok(vec![make_source(
            ID,
            "Dailymotion",
            "Embedded",
            &format!("https://www.dailymotion.com/embed/video/{media_id}?autoplay=1"),
            true,
        )])
    }
}
