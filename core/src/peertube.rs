//! PeerTube: decentralized, openly licensed video hosting with a public REST API.
//! Works with any instance; the built-in provider points at one instance, and users can add others
//! as custom providers.

use crate::archive::{encode_segment, strip_html};
use crate::http::{require_secure, secure_url, HttpClient};
use crate::models::{Catalog, MediaDetails, MediaEpisode, MediaItem, MediaType, VideoSource};
use crate::provider::{MediaProvider, ProviderDescriptor, ProviderError};
use crate::web::make_source;
use serde_json::Value;
use std::sync::Arc;
use url::Url;

pub const ID: &str = "native.peertube";
pub const DEFAULT_INSTANCE: &str = "https://framatube.org";

/// Builds an item from a PeerTube video object (list rows and the detail endpoint share this shape).
pub fn item_from_json(v: &Value, base: &str, provider_id: &str) -> Option<MediaItem> {
    let id = v.get("uuid")?.as_str()?;
    let title = v.get("name")?.as_str()?;
    let mut item = MediaItem::new(id, provider_id, title);
    item.media_type = MediaType::Video;
    item.description = v.get("description").and_then(|d| d.as_str()).map(strip_html).filter(|s| !s.is_empty());

    let thumb = v
        .get("thumbnailPath")
        .and_then(|t| t.as_str())
        .map(|p| format!("{base}{p}"))
        .or_else(|| {
            v.get("thumbnails")
                .and_then(|t| t.as_array())
                .and_then(|a| a.first())
                .and_then(|t| t.get("fileUrl"))
                .and_then(|u| u.as_str())
                .map(String::from)
        })
        .and_then(|u| secure_url(&u));
    let preview = v
        .get("previewPath")
        .and_then(|t| t.as_str())
        .map(|p| format!("{base}{p}"))
        .and_then(|u| secure_url(&u));
    item.poster_url = thumb.clone();
    item.backdrop_url = preview.or(thumb);
    item.release_year = v
        .get("publishedAt")
        .and_then(|d| d.as_str())
        .and_then(|d| d.get(0..4))
        .and_then(|y| y.parse::<i32>().ok());
    item.duration = v.get("duration").and_then(|d| d.as_f64());
    item.genres = v
        .get("tags")
        .and_then(|t| t.as_array())
        .map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).take(6).collect())
        .unwrap_or_default();
    Some(item)
}

/// Playable files of one video: MP4 files (best first), then the adaptive HLS playlist.
pub fn sources_for(client: &dyn HttpClient, base: &str, id: &str, provider_id: &str) -> Result<Vec<VideoSource>, ProviderError> {
    let data = client.get(&format!("{base}/api/v1/videos/{}", encode_segment(id)))?;
    let v: Value = serde_json::from_slice(&data).map_err(|_| ProviderError::InvalidResponse)?;
    let host = Url::parse(base).ok().and_then(|u| u.host_str().map(String::from)).unwrap_or_else(|| "PeerTube".into());

    let mut files: Vec<(i64, String, String)> = v
        .get("files")
        .and_then(|f| f.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|f| {
                    let url = secure_url(f.get("fileUrl")?.as_str()?)?;
                    let res = f.get("resolution")?;
                    let rank = res.get("id").and_then(|i| i.as_i64()).unwrap_or(0);
                    let label = res.get("label").and_then(|l| l.as_str()).unwrap_or("Default").to_string();
                    Some((rank, label, url))
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort_by(|a, b| b.0.cmp(&a.0));

    let mut out: Vec<VideoSource> = files
        .into_iter()
        .map(|(_, label, url)| make_source(provider_id, &host, &label, &url, false))
        .collect();

    if let Some(playlist) = v
        .get("streamingPlaylists")
        .and_then(|p| p.as_array())
        .and_then(|a| a.first())
        .and_then(|p| p.get("playlistUrl"))
        .and_then(|u| u.as_str())
        .and_then(secure_url)
    {
        out.push(make_source(provider_id, &format!("{host} (adaptive)"), "Auto", &playlist, false));
    }
    Ok(out)
}

pub struct PeerTubeProvider {
    client: Arc<dyn HttpClient>,
    base: String,
    descriptor: ProviderDescriptor,
}

impl PeerTubeProvider {
    pub fn new(client: Arc<dyn HttpClient>, id: &str, name: &str, instance: &str) -> Result<Self, ProviderError> {
        let checked = require_secure(instance)?;
        let base = Url::parse(&checked).map_err(|_| ProviderError::InvalidResponse)?.origin().ascii_serialization();
        let descriptor = ProviderDescriptor {
            id: id.into(),
            name: name.into(),
            description: format!("Open, community-hosted video from the PeerTube instance {base}."),
            language: "en".into(),
            supported_types: vec![MediaType::Video],
            version: 1,
            author: "Built in".into(),
            source_url: Some(base.clone()),
            authorization_note: "Uses the instance's public REST API and plays files the instance itself publishes. Licenses are set by each uploader.".into(),
        };
        Ok(Self { client, base, descriptor })
    }

    fn list(&self, path_and_query: &str) -> Result<Vec<MediaItem>, ProviderError> {
        let data = self.client.get(&format!("{}{}", self.base, path_and_query))?;
        let v: Value = serde_json::from_slice(&data).map_err(|_| ProviderError::InvalidResponse)?;
        let rows = v.get("data").and_then(|d| d.as_array()).ok_or(ProviderError::InvalidResponse)?;
        Ok(rows.iter().filter_map(|r| item_from_json(r, &self.base, &self.descriptor.id)).collect())
    }
}

impl MediaProvider for PeerTubeProvider {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn home_catalogs(&self) -> Result<Vec<Catalog>, ProviderError> {
        const SHELVES: [(&str, &str, &str); 3] = [
            ("pt.trending", "Trending", "/api/v1/videos?sort=-trending&count=24&nsfw=false"),
            ("pt.recent", "Recently added", "/api/v1/videos?sort=-publishedAt&count=24&nsfw=false"),
            ("pt.popular", "Most viewed", "/api/v1/videos?sort=-views&count=24&nsfw=false"),
        ];
        let results: Vec<Option<Vec<MediaItem>>> = std::thread::scope(|s| {
            let handles: Vec<_> = SHELVES.iter().map(|(_, _, q)| s.spawn(move || self.list(q).ok())).collect();
            handles.into_iter().map(|h| h.join().ok().flatten()).collect()
        });
        let out: Vec<Catalog> = SHELVES
            .iter()
            .zip(results)
            .filter_map(|((id, title, _), items)| {
                items.filter(|i| !i.is_empty()).map(|items| Catalog { id: (*id).into(), title: (*title).into(), items })
            })
            .collect();
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
        let start = ((page.max(1) - 1) * 30).to_string();
        let url = Url::parse_with_params(
            &format!("{}/api/v1/search/videos", self.base),
            [("search", q), ("count", "30"), ("nsfw", "false"), ("start", start.as_str())],
        )
        .map_err(|_| ProviderError::InvalidResponse)?;
        let path = format!("{}?{}", url.path(), url.query().unwrap_or(""));
        self.list(&path)
    }

    fn load_details(&self, id: &str) -> Result<MediaDetails, ProviderError> {
        let data = self.client.get(&format!("{}/api/v1/videos/{}", self.base, encode_segment(id)))?;
        let v: Value = serde_json::from_slice(&data).map_err(|_| ProviderError::NotFound)?;
        let item = item_from_json(&v, &self.base, &self.descriptor.id).ok_or(ProviderError::NotFound)?;
        Ok(MediaDetails { item, seasons: vec![], languages: vec![] })
    }

    fn load_episodes(&self, _media_id: &str, _season: Option<i32>) -> Result<Vec<MediaEpisode>, ProviderError> {
        Ok(vec![])
    }

    fn resolve_sources(&self, media_id: &str, _episode: Option<&MediaEpisode>) -> Result<Vec<VideoSource>, ProviderError> {
        let found = sources_for(&*self.client, &self.base, media_id, &self.descriptor.id)?;
        if found.is_empty() {
            return Err(ProviderError::NoPlayableSource);
        }
        Ok(found)
    }
}
