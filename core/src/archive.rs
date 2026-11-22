use crate::http::HttpClient;
use crate::models::{Catalog, MediaDetails, MediaEpisode, MediaItem, MediaType, VideoSource};
use crate::provider::{MediaProvider, ProviderDescriptor, ProviderError};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

pub const ID: &str = "native.internetarchive";

/// Characters escaped inside a single URL path segment ('/' is escaped; parentheses are kept).
const SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'/')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}');

/// Native provider backed by the Internet Archive's documented public APIs
/// (advancedsearch + metadata + download). It streams files the Archive itself publishes and does
/// not touch any access control. Rights vary per item; each item shows its license on archive.org.
pub struct InternetArchiveProvider {
    client: Arc<dyn HttpClient>,
    descriptor: ProviderDescriptor,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoFile {
    pub name: String,
    pub format: String,
    pub size: u64,
    pub length: Option<f64>,
    pub quality: String,
}

impl InternetArchiveProvider {
    pub fn new(client: Arc<dyn HttpClient>) -> Self {
        Self {
            client,
            descriptor: ProviderDescriptor {
                id: ID.into(),
                name: "Internet Archive".into(),
                description: "Public films, classic TV, and community video from the Internet Archive's open API.".into(),
                language: "en".into(),
                supported_types: vec![MediaType::Movie, MediaType::Documentary, MediaType::Cartoon, MediaType::TvSeries],
                version: 1,
                author: "Built in".into(),
                source_url: Some("https://archive.org".into()),
                authorization_note: "Uses the Archive's documented public API and serves files the Archive itself publishes. Each item shows its own license on archive.org.".into(),
            },
        }
    }

    fn fetch(&self, query: &str, sort: &str, rows: u32, page: u32) -> Result<Vec<MediaItem>, ProviderError> {
        let rows_s = rows.to_string();
        let page_s = page.max(1).to_string();
        let mut params: Vec<(&str, &str)> = vec![("q", query)];
        for f in ["identifier", "title", "year", "description", "creator"] {
            params.push(("fl[]", f));
        }
        params.extend([("sort[]", sort), ("rows", rows_s.as_str()), ("page", page_s.as_str()), ("output", "json")]);
        let url = url::Url::parse_with_params("https://archive.org/advancedsearch.php", params)
            .map_err(|_| ProviderError::InvalidResponse)?;
        let data = self.client.get(url.as_str())?;
        let root: Value = serde_json::from_slice(&data).map_err(|_| ProviderError::InvalidResponse)?;
        let docs = root
            .get("response")
            .and_then(|r| r.get("docs"))
            .and_then(|d| d.as_array())
            .ok_or(ProviderError::InvalidResponse)?;
        Ok(docs
            .iter()
            .filter_map(|doc| {
                let id = doc.get("identifier")?.as_str()?;
                Some(make_item(id, doc, None))
            })
            .collect())
    }

    fn metadata(&self, id: &str) -> Result<Value, ProviderError> {
        let url = format!("https://archive.org/metadata/{}", encode_segment(id));
        let data = self.client.get(&url)?;
        let root: Value = serde_json::from_slice(&data).map_err(|_| ProviderError::InvalidResponse)?;
        if root.get("metadata").is_none() {
            return Err(ProviderError::NotFound);
        }
        Ok(root)
    }
}

impl MediaProvider for InternetArchiveProvider {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn home_catalogs(&self) -> Result<Vec<Catalog>, ProviderError> {
        let (features, cartoons) = std::thread::scope(|s| {
            let a = s.spawn(|| self.fetch("collection:feature_films", "downloads desc", 20, 1));
            let b = s.spawn(|| self.fetch("collection:animationandcartoons", "downloads desc", 20, 1));
            (a.join().ok().and_then(|r| r.ok()), b.join().ok().and_then(|r| r.ok()))
        });
        let mut out = vec![];
        if let Some(items) = features.filter(|i| !i.is_empty()) {
            out.push(Catalog { id: "ia.features".into(), title: "Popular Feature Films".into(), items });
        }
        if let Some(items) = cartoons.filter(|i| !i.is_empty()) {
            out.push(Catalog { id: "ia.cartoons".into(), title: "Animation & Cartoons".into(), items });
        }
        Ok(out)
    }

    fn search(&self, query: &str, page: u32) -> Result<Vec<MediaItem>, ProviderError> {
        let cleaned = query.replace('"', " ");
        let cleaned = cleaned.trim();
        if cleaned.is_empty() {
            return Ok(vec![]);
        }
        self.fetch(&format!("title:({cleaned}) AND mediatype:movies"), "downloads desc", 30, page)
    }

    fn load_details(&self, id: &str) -> Result<MediaDetails, ProviderError> {
        let root = self.metadata(id)?;
        let meta = root.get("metadata").ok_or(ProviderError::InvalidResponse)?;
        let runtime = video_files(&root).first().and_then(|f| f.length);
        Ok(MediaDetails {
            item: make_item(id, meta, runtime),
            seasons: vec![],
            languages: strings(meta.get("language")),
        })
    }

    fn load_episodes(&self, _media_id: &str, _season: Option<i32>) -> Result<Vec<MediaEpisode>, ProviderError> {
        Ok(vec![])
    }

    fn resolve_sources(&self, media_id: &str, _episode: Option<&MediaEpisode>) -> Result<Vec<VideoSource>, ProviderError> {
        let root = self.metadata(media_id)?;
        let files = video_files(&root);
        if files.is_empty() {
            return Err(ProviderError::NoPlayableSource);
        }
        Ok(files
            .into_iter()
            .map(|f| VideoSource {
                id: format!("{media_id}/{}", f.name),
                url: format!("https://archive.org/download/{}/{}", encode_segment(media_id), encode_segment(&f.name)),
                title: format!("{} \u{2022} {}", f.format, f.quality),
                quality: f.quality,
                container: "mp4".into(),
                mime_type: Some("video/mp4".into()),
                headers: HashMap::new(),
                subtitles: vec![],
                provider_id: ID.into(),
            })
            .collect())
    }
}

pub fn encode_segment(s: &str) -> String {
    utf8_percent_encode(s, SEGMENT).to_string()
}

pub fn strings(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
        _ => vec![],
    }
}

pub fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let decoded = out
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn make_item(id: &str, dict: &Value, runtime: Option<f64>) -> MediaItem {
    let year = match dict.get("year") {
        Some(Value::Number(n)) => n.as_i64().map(|y| y as i32),
        Some(Value::String(s)) => s.chars().take(4).collect::<String>().parse().ok(),
        _ => None,
    }
    .or_else(|| {
        dict.get("date")
            .and_then(|d| d.as_str())
            .and_then(|s| s.chars().take(4).collect::<String>().parse().ok())
    });
    let title = strings(dict.get("title")).into_iter().next().unwrap_or_else(|| id.to_string());
    let poster = format!("https://archive.org/services/img/{}", encode_segment(id));
    let mut item = MediaItem::new(id, ID, &title);
    item.description = strings(dict.get("description")).into_iter().next().map(|d| strip_html(&d));
    item.poster_url = Some(poster.clone());
    item.backdrop_url = Some(poster);
    item.release_year = year;
    item.genres = strings(dict.get("subject")).into_iter().take(6).collect();
    item.duration = runtime;
    item
}

/// MP4 derivatives the Archive publishes, largest (best quality) first.
pub fn video_files(root: &Value) -> Vec<VideoFile> {
    let Some(files) = root.get("files").and_then(|f| f.as_array()) else {
        return vec![];
    };
    let accepted = ["h.264", "h.264 ia", "mpeg4", "512kb mpeg4", "h.264 hd"];
    let mut out: Vec<VideoFile> = files
        .iter()
        .filter_map(|f| {
            let name = f.get("name")?.as_str()?;
            let format = f.get("format")?.as_str()?;
            if !accepted.contains(&format.to_lowercase().as_str()) || !name.to_lowercase().ends_with(".mp4") {
                return None;
            }
            let size = f.get("size").and_then(|s| s.as_str()).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            let length = f.get("length").and_then(|l| l.as_str()).and_then(|l| l.parse::<f64>().ok());
            let quality = if format.to_lowercase().contains("512") {
                "SD"
            } else if size > 1_500_000_000 {
                "HD"
            } else {
                "Standard"
            };
            Some(VideoFile { name: name.into(), format: format.into(), size, length, quality: quality.into() })
        })
        .collect();
    out.sort_by(|a, b| b.size.cmp(&a.size));
    out
}
