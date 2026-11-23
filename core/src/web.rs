//! Link resolver: turns a pasted web address into a playable item.
//!
//! Known video sites (YouTube, Vimeo, Dailymotion, PeerTube) are handled through their public
//! embed / oEmbed / API endpoints. Direct media files play as-is. For any other page the resolver
//! looks for the video the page itself exposes (`<video>`, `og:video`, embedded players).
//! Nothing here bypasses DRM or any access control: if a page does not expose a plain playable
//! link, the resolver reports that instead of guessing.

use crate::archive::strip_html;
use crate::http::{require_secure, HttpClient};
use crate::models::{Catalog, MediaDetails, MediaEpisode, MediaItem, MediaType, VideoSource};
use crate::provider::{MediaProvider, ProviderDescriptor, ProviderError};
use scraper::{Html, Selector};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use url::Url;

pub const ID: &str = "native.web";

const MEDIA_EXT: [&str; 14] = [
    ".mp4", ".m4v", ".webm", ".mov", ".mkv", ".m3u8", ".mp3", ".m4a", ".aac", ".ogg", ".oga", ".ogv", ".flac", ".wav",
];

pub fn is_media_url(u: &Url) -> bool {
    let p = u.path().to_lowercase();
    MEDIA_EXT.iter().any(|e| p.ends_with(e))
}

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    YouTube(String),
    Vimeo(String),
    Dailymotion(String),
    PeerTube { host: String, id: String },
    Direct,
    Page,
}

fn valid_youtube_id(s: &str) -> bool {
    s.len() == 11 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub fn classify(u: &Url) -> Target {
    let host_lower = u.host_str().unwrap_or("").to_lowercase();
    let host = host_lower.trim_start_matches("www.");
    let segs: Vec<&str> = u.path_segments().map(|s| s.filter(|x| !x.is_empty()).collect()).unwrap_or_default();

    match host {
        "youtube.com" | "m.youtube.com" | "music.youtube.com" | "youtube-nocookie.com" => {
            if segs.first() == Some(&"watch") {
                if let Some((_, v)) = u.query_pairs().find(|(k, _)| k == "v") {
                    if valid_youtube_id(&v) {
                        return Target::YouTube(v.to_string());
                    }
                }
            }
            if matches!(segs.first(), Some(&"shorts") | Some(&"embed") | Some(&"live") | Some(&"v")) {
                if let Some(id) = segs.get(1) {
                    if valid_youtube_id(id) {
                        return Target::YouTube((*id).to_string());
                    }
                }
            }
        }
        "youtu.be" => {
            if let Some(id) = segs.first() {
                if valid_youtube_id(id) {
                    return Target::YouTube((*id).to_string());
                }
            }
        }
        "vimeo.com" | "player.vimeo.com" => {
            if let Some(id) = segs.iter().rev().find(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())) {
                return Target::Vimeo((*id).to_string());
            }
        }
        "dailymotion.com" | "dai.ly" => {
            let raw = if host == "dai.ly" { segs.first() } else if segs.first() == Some(&"video") { segs.get(1) } else { None };
            if let Some(id) = raw {
                let id = id.split('_').next().unwrap_or(id);
                if !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric()) {
                    return Target::Dailymotion(id.to_string());
                }
            }
        }
        _ => {}
    }

    if is_media_url(u) {
        return Target::Direct;
    }
    let origin = u.origin().ascii_serialization();
    if segs.len() >= 2 && segs[0] == "w" {
        return Target::PeerTube { host: origin, id: segs[1].to_string() };
    }
    if segs.len() >= 3 && segs[0] == "videos" && segs[1] == "watch" {
        return Target::PeerTube { host: origin, id: segs[2].to_string() };
    }
    Target::Page
}

pub fn host_label(url: &str) -> String {
    Url::parse(url).ok().and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_string())).unwrap_or_else(|| "Link".into())
}

pub fn make_source(provider_id: &str, title: &str, quality: &str, url: &str, embed: bool) -> VideoSource {
    let lower = url.to_lowercase();
    let container = if embed {
        "embed"
    } else if lower.contains(".m3u8") {
        "hls"
    } else if lower.contains(".webm") {
        "webm"
    } else if lower.contains(".mp3") || lower.contains(".m4a") || lower.contains(".aac") || lower.contains(".flac") {
        "audio"
    } else {
        "mp4"
    };
    VideoSource {
        id: url.to_string(),
        url: url.to_string(),
        quality: quality.into(),
        title: title.into(),
        container: container.into(),
        mime_type: None,
        headers: HashMap::new(),
        subtitles: vec![],
        provider_id: provider_id.into(),
        embed,
    }
}

/// Collects playable links a page exposes: (url, is_embed).
pub fn extract_page_sources(base: &Url, html: &str) -> Vec<(String, bool)> {
    let doc = Html::parse_document(html);
    let mut out: Vec<(String, bool)> = vec![];
    let mut push = |raw: &str, mode: u8| {
        // mode: 0 = direct media, 1 = embedded page, 2 = decide from the extension
        if let Ok(u) = base.join(raw.trim()) {
            if u.scheme() == "https" && u.host_str().is_some() {
                let embed = match mode {
                    0 => false,
                    1 => true,
                    _ => !is_media_url(&u),
                };
                let s = u.to_string();
                if !out.iter().any(|(x, _)| *x == s) {
                    out.push((s, embed));
                }
            }
        }
    };
    let rules: [(&str, &str, u8); 6] = [
        ("meta[property=\"og:video:secure_url\"]", "content", 2),
        ("meta[property=\"og:video:url\"]", "content", 2),
        ("meta[property=\"og:video\"]", "content", 2),
        ("video[src]", "src", 0),
        ("video source[src]", "src", 0),
        ("iframe[src]", "src", 1),
    ];
    for (css, attr, mode) in rules {
        if let Ok(sel) = Selector::parse(css) {
            for el in doc.select(&sel) {
                if let Some(v) = el.value().attr(attr) {
                    push(v, mode);
                }
            }
        }
    }
    out
}

fn meta(doc: &Html, key: &str) -> Option<String> {
    for attr in ["property", "name"] {
        if let Ok(sel) = Selector::parse(&format!("meta[{attr}=\"{key}\"]")) {
            if let Some(v) = doc.select(&sel).next().and_then(|e| e.value().attr("content")) {
                let v = strip_html(v);
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }
    }
    None
}

#[derive(Clone)]
pub struct WebResolver {
    client: Arc<dyn HttpClient>,
}

impl WebResolver {
    pub fn new(client: Arc<dyn HttpClient>) -> Self {
        Self { client }
    }

    fn json(&self, url: &str) -> Option<Value> {
        serde_json::from_slice(&self.client.get(url).ok()?).ok()
    }

    fn oembed(&self, endpoint: &str, page: &str) -> Option<Value> {
        let u = Url::parse_with_params(endpoint, [("url", page), ("format", "json")]).ok()?;
        self.json(u.as_str())
    }

    fn page_html(&self, url: &str) -> Result<String, ProviderError> {
        let bytes = self.client.get(url)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Metadata for a link. The item id is the (https) link itself.
    pub fn item(&self, raw: &str) -> Result<MediaItem, ProviderError> {
        let url = require_secure(raw)?;
        let parsed = Url::parse(&url).map_err(|_| ProviderError::InvalidResponse)?;
        let mut item = MediaItem::new(&url, ID, &host_label(&url));
        item.media_type = MediaType::Video;

        let str_of = |j: &Value, k: &str| j.get(k).and_then(|v| v.as_str()).map(strip_html).filter(|s| !s.is_empty());

        match classify(&parsed) {
            Target::YouTube(id) => {
                let watch = format!("https://www.youtube.com/watch?v={id}");
                item.title = "YouTube video".into();
                if let Some(j) = self.oembed("https://www.youtube.com/oembed", &watch) {
                    if let Some(t) = str_of(&j, "title") {
                        item.title = t;
                    }
                    item.description = str_of(&j, "author_name").map(|a| format!("By {a}"));
                }
                let thumb = format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg");
                item.poster_url = Some(thumb.clone());
                item.backdrop_url = Some(thumb);
            }
            Target::Vimeo(id) => {
                item.title = "Vimeo video".into();
                if let Some(j) = self.oembed("https://vimeo.com/api/oembed.json", &format!("https://vimeo.com/{id}")) {
                    item.title = str_of(&j, "title").unwrap_or(item.title);
                    item.description = str_of(&j, "description").or_else(|| str_of(&j, "author_name").map(|a| format!("By {a}")));
                    item.poster_url = str_of(&j, "thumbnail_url");
                    item.duration = j.get("duration").and_then(|d| d.as_f64());
                }
            }
            Target::Dailymotion(id) => {
                item.title = "Dailymotion video".into();
                if let Some(j) =
                    self.oembed("https://www.dailymotion.com/services/oembed", &format!("https://www.dailymotion.com/video/{id}"))
                {
                    item.title = str_of(&j, "title").unwrap_or(item.title);
                    item.description = str_of(&j, "author_name").map(|a| format!("By {a}"));
                    item.poster_url = str_of(&j, "thumbnail_url");
                }
            }
            Target::PeerTube { host, id } => {
                let api = format!("{host}/api/v1/videos/{id}");
                match self.json(&api).and_then(|j| crate::peertube::item_from_json(&j, &host, ID)) {
                    Some(found) => {
                        item = found;
                        item.id = url.clone();
                    }
                    None => self.fill_from_page(&mut item, &url),
                }
            }
            Target::Direct => {
                let name = parsed.path_segments().and_then(|s| s.last()).unwrap_or("Media file");
                let decoded = percent_encoding::percent_decode_str(name).decode_utf8_lossy().into_owned();
                item.title = decoded.rsplit_once('.').map(|(b, _)| b.to_string()).unwrap_or(decoded).replace(['_', '-'], " ");
                item.description = Some(format!("Direct media file from {}", host_label(&url)));
            }
            Target::Page => self.fill_from_page(&mut item, &url),
        }
        Ok(item)
    }

    fn fill_from_page(&self, item: &mut MediaItem, url: &str) {
        let Ok(html) = self.page_html(url) else { return };
        let doc = Html::parse_document(&html);
        let title_tag = Selector::parse("title").ok().and_then(|s| doc.select(&s).next().map(|e| e.text().collect::<String>()));
        if let Some(t) = meta(&doc, "og:title").or(title_tag.map(|t| strip_html(&t))).filter(|t| !t.is_empty()) {
            item.title = t;
        }
        item.description = meta(&doc, "og:description").or_else(|| meta(&doc, "description"));
        if let Some(img) = meta(&doc, "og:image") {
            if let Ok(base) = Url::parse(url) {
                item.poster_url = base.join(&img).ok().filter(|u| u.scheme() == "https").map(|u| u.to_string());
                item.backdrop_url = item.poster_url.clone();
            }
        }
    }

    /// Every playable source the link exposes, best first.
    pub fn sources(&self, raw: &str) -> Result<Vec<VideoSource>, ProviderError> {
        let url = require_secure(raw)?;
        let parsed = Url::parse(&url).map_err(|_| ProviderError::InvalidResponse)?;
        match classify(&parsed) {
            Target::YouTube(id) => {
                return Ok(vec![make_source(
                    ID,
                    "YouTube",
                    "Embedded",
                    &format!("https://www.youtube-nocookie.com/embed/{id}?rel=0&autoplay=1"),
                    true,
                )])
            }
            Target::Vimeo(id) => {
                return Ok(vec![make_source(ID, "Vimeo", "Embedded", &format!("https://player.vimeo.com/video/{id}?autoplay=1"), true)])
            }
            Target::Dailymotion(id) => {
                return Ok(vec![make_source(
                    ID,
                    "Dailymotion",
                    "Embedded",
                    &format!("https://www.dailymotion.com/embed/video/{id}?autoplay=1"),
                    true,
                )])
            }
            Target::PeerTube { host, id } => {
                if let Ok(found) = crate::peertube::sources_for(&*self.client, &host, &id, ID) {
                    if !found.is_empty() {
                        return Ok(found);
                    }
                }
            }
            Target::Direct => return Ok(vec![make_source(ID, &host_label(&url), "Default", &url, false)]),
            Target::Page => {}
        }
        let html = self.page_html(&url)?;
        Ok(self.sources_from_html(&parsed, &html))
    }

    pub fn sources_from_html(&self, base: &Url, html: &str) -> Vec<VideoSource> {
        let mut out = vec![];
        for (link, embed) in extract_page_sources(base, html) {
            let Ok(u) = Url::parse(&link) else { continue };
            match classify(&u) {
                Target::YouTube(id) => out.push(make_source(
                    ID,
                    "YouTube",
                    "Embedded",
                    &format!("https://www.youtube-nocookie.com/embed/{id}?rel=0&autoplay=1"),
                    true,
                )),
                Target::Vimeo(id) => out.push(make_source(ID, "Vimeo", "Embedded", &format!("https://player.vimeo.com/video/{id}?autoplay=1"), true)),
                Target::Dailymotion(id) => out.push(make_source(
                    ID,
                    "Dailymotion",
                    "Embedded",
                    &format!("https://www.dailymotion.com/embed/video/{id}?autoplay=1"),
                    true,
                )),
                _ => {
                    let quality = if embed { "Embedded" } else if link.contains(".m3u8") { "Auto" } else { "Default" };
                    out.push(make_source(ID, &host_label(&link), quality, &link, embed));
                }
            }
        }
        // Direct files first, embedded players last.
        out.sort_by_key(|s| s.embed);
        out
    }
}

/// Provider wrapper: paste a link into Search and it appears as a result.
pub struct WebProvider {
    resolver: WebResolver,
    descriptor: ProviderDescriptor,
}

impl WebProvider {
    pub fn new(client: Arc<dyn HttpClient>) -> Self {
        Self {
            resolver: WebResolver::new(client),
            descriptor: ProviderDescriptor {
                id: ID.into(),
                name: "Web links".into(),
                description: "Paste a link in Search: YouTube, Vimeo, Dailymotion, PeerTube, direct video files, or any page that exposes a video.".into(),
                language: "en".into(),
                supported_types: vec![MediaType::Video],
                version: 1,
                author: "Built in".into(),
                source_url: None,
                authorization_note: "Plays only what the linked site publishes for viewing. Protected streams are not supported. You are responsible for having the right to watch what you open.".into(),
            },
        }
    }
}

impl MediaProvider for WebProvider {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }
    fn home_catalogs(&self) -> Result<Vec<Catalog>, ProviderError> {
        Ok(vec![])
    }
    fn search(&self, query: &str, _page: u32) -> Result<Vec<MediaItem>, ProviderError> {
        let q = query.trim();
        if !q.to_lowercase().starts_with("https://") {
            return Ok(vec![]);
        }
        Ok(vec![self.resolver.item(q)?])
    }
    fn load_details(&self, id: &str) -> Result<MediaDetails, ProviderError> {
        Ok(MediaDetails { item: self.resolver.item(id)?, seasons: vec![], languages: vec![] })
    }
    fn load_episodes(&self, _media_id: &str, _season: Option<i32>) -> Result<Vec<MediaEpisode>, ProviderError> {
        Ok(vec![])
    }
    fn resolve_sources(&self, media_id: &str, _episode: Option<&MediaEpisode>) -> Result<Vec<VideoSource>, ProviderError> {
        let found = self.resolver.sources(media_id)?;
        if found.is_empty() {
            return Err(ProviderError::NoPlayableSource);
        }
        Ok(found)
    }
}
