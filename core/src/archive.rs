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

    fn fetch(&self, query: &str, sort: Option<&str>, rows: u32, page: u32) -> Result<Vec<MediaItem>, ProviderError> {
        let rows_s = rows.to_string();
        let page_s = page.max(1).to_string();
        let mut params: Vec<(&str, &str)> = vec![("q", query)];
        for f in ["identifier", "title", "year", "description", "creator"] {
            params.push(("fl[]", f));
        }
        if let Some(sort) = sort {
            params.push(("sort[]", sort));
        }
        params.extend([("rows", rows_s.as_str()), ("page", page_s.as_str()), ("output", "json")]);
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
        // Every query is restricted to mediatype:movies so shelves hold playable videos, not sub-collections.
        const SHELVES: [(&str, &str, &str); 6] = [
            ("ia.features", "Feature Films", "feature_films"),
            ("ia.cartoons", "Animation and Cartoons", "animationandcartoons"),
            ("ia.classictv", "Classic TV", "classic_tv"),
            ("ia.noir", "Film Noir", "film_noir"),
            ("ia.silent", "Silent Films", "silent_films"),
            ("ia.scifi", "Sci-Fi and Horror", "SciFi_Horror"),
        ];
        let results: Vec<Option<Vec<MediaItem>>> = std::thread::scope(|s| {
            let handles: Vec<_> = SHELVES
                .iter()
                .map(|(_, _, collection)| {
                    s.spawn(move || {
                        self.fetch(&format!("collection:{collection} AND mediatype:movies"), Some("downloads desc"), 24, 1).ok()
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().ok().flatten()).collect()
        });
        let mut out = vec![];
        for ((id, title, _), items) in SHELVES.iter().zip(results) {
            if let Some(items) = items.filter(|i| !i.is_empty()) {
                out.push(Catalog { id: (*id).into(), title: (*title).into(), items });
            }
        }
        if out.is_empty() {
            return Err(ProviderError::InvalidResponse);
        }
        Ok(out)
    }

    fn search(&self, query: &str, page: u32) -> Result<Vec<MediaItem>, ProviderError> {
        let terms = search_terms(query);
        if terms.is_empty() {
            return Ok(vec![]);
        }
        let q = format!("({}) AND mediatype:movies", terms.join(" AND "));
        self.fetch(&q, None, 30, page)
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

    fn load_episodes(&self, media_id: &str, _season: Option<i32>) -> Result<Vec<MediaEpisode>, ProviderError> {
        let root = self.metadata(media_id)?;
        let groups = grouped_files(&video_files(&root));
        if groups.len() < 2 {
            return Ok(vec![]); // a single video: no episode list
        }
        Ok(groups
            .iter()
            .enumerate()
            .map(|(i, (key, files))| MediaEpisode {
                id: key.clone(),
                title: pretty_name(key),
                episode_number: Some(i as i32 + 1),
                season_number: None,
                description: None,
                thumbnail_url: None,
                duration: files.first().and_then(|f| f.length),
            })
            .collect())
    }

    fn resolve_sources(&self, media_id: &str, episode: Option<&MediaEpisode>) -> Result<Vec<VideoSource>, ProviderError> {
        let root = self.metadata(media_id)?;
        let groups = grouped_files(&video_files(&root));
        // With an episode, use that file group; without one, a single-video item uses its only group.
        let files = match episode {
            Some(ep) => groups.into_iter().find(|(k, _)| *k == ep.id).map(|(_, f)| f),
            None => groups.into_iter().next().map(|(_, f)| f),
        }
        .unwrap_or_default();
        if files.is_empty() {
            return Err(ProviderError::NoPlayableSource);
        }
        Ok(files
            .into_iter()
            .map(|f| VideoSource {
                id: format!("{media_id}/{}", f.name),
                url: format!("https://archive.org/download/{}/{}", encode_segment(media_id), encode_segment(&f.name)),
                title: format!("{} - {}", f.format, f.quality),
                quality: f.quality,
                container: "mp4".into(),
                mime_type: Some("video/mp4".into()),
                headers: HashMap::new(),
                subtitles: vec![],
                provider_id: ID.into(),
                embed: false,
            })
            .collect())
    }
}

/// Splits user text into safe search terms (Lucene operators and punctuation are removed).
pub fn search_terms(query: &str) -> Vec<String> {
    query
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '\'' { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .filter(|t| !matches!(t.to_uppercase().as_str(), "AND" | "OR" | "NOT" | "TO"))
        .map(|t| t.to_string())
        .collect()
}

/// Strips quality suffixes so the derivatives of one video share a key.
pub fn group_key(name: &str) -> String {
    let mut base = name.rsplit_once('.').map(|(b, _)| b).unwrap_or(name).to_string();
    loop {
        let lower = base.to_lowercase();
        let stripped = [".ia", "_512kb", ".512kb", "_hd"].iter().find(|s| lower.ends_with(**s));
        match stripped {
            Some(s) => base.truncate(base.len() - s.len()),
            None => break,
        }
    }
    base
}

fn pretty_name(key: &str) -> String {
    key.rsplit('/').next().unwrap_or(key).replace(['_', '.'], " ").trim().to_string()
}

/// Groups files by video, best file first inside each group; group order follows file name.
pub fn grouped_files(files: &[VideoFile]) -> Vec<(String, Vec<VideoFile>)> {
    let mut groups: Vec<(String, Vec<VideoFile>)> = vec![];
    for f in files {
        let key = group_key(&f.name);
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, list)) => list.push(f.clone()),
            None => groups.push((key, vec![f.clone()])),
        }
    }
    groups.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    groups
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
    let is_video = |n: &str| n.ends_with(".mp4") || n.ends_with(".m4v");
    let mut out: Vec<VideoFile> = files
        .iter()
        .filter_map(|f| {
            let name = f.get("name")?.as_str()?;
            let format = f.get("format")?.as_str()?;
            if !accepted.contains(&format.to_lowercase().as_str()) || !is_video(&name.to_lowercase()) {
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
