//! User-defined providers. A definition describes where a site lists its videos; playback goes
//! through the link resolver in `web.rs`. The app ships no definitions for any site: they are
//! created by the user, who is responsible for having the right to access that content.
//!
//! Kinds:
//!   rss      RSS / Atom feed (podcasts, YouTube channel feeds, video blogs)
//!   m3u      M3U / M3U8 playlist of streams
//!   json     any JSON API that returns a list of videos
//!   html     any web page that lists videos (CSS selectors)
//!   peertube any PeerTube instance

use crate::archive::strip_html;
use crate::http::{require_secure, HttpClient};
use crate::models::{Catalog, MediaDetails, MediaEpisode, MediaItem, MediaType, VideoSource};
use crate::peertube::PeerTubeProvider;
use crate::provider::{MediaProvider, ProviderDescriptor, ProviderError};
use crate::web::{self, make_source, WebResolver};
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use url::Url;

pub const KINDS: [&str; 5] = ["rss", "m3u", "json", "html", "peertube"];

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CustomDefinition {
    pub id: String,
    pub name: String,
    pub kind: String,
    /// Feed, playlist, listing page, API endpoint or PeerTube instance address.
    pub url: String,
    /// Optional search address containing {query}.
    pub search_url: String,
    /// json: dotted path to the array of videos. html: CSS selector for one video card.
    pub list: String,
    /// json: dotted path. html: CSS selector with an optional @attribute suffix.
    pub title: String,
    pub link: String,
    pub image: String,
    pub description: String,
    /// html: selector (optional @attribute) evaluated on the video's page to find its stream.
    pub stream: String,
    /// html: regular expression evaluated on the video's page; capture group 1 (or the whole match) is the stream address.
    pub stream_regex: String,
    /// Repository address this definition was installed from (empty for providers you made yourself).
    pub source: String,
    /// Version published by that repository; used to apply updates on refresh.
    pub version: i64,
}

impl CustomDefinition {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("Give the provider a name.".into());
        }
        if !KINDS.contains(&self.kind.as_str()) {
            return Err("Choose a provider type.".into());
        }
        require_secure(&self.url).map_err(|e| e.to_string())?;
        if !self.search_url.trim().is_empty() {
            if !self.search_url.contains("{query}") {
                return Err("The search address must contain {query}.".into());
            }
            require_secure(&self.search_url.replace("{query}", "x")).map_err(|e| e.to_string())?;
        }
        if self.kind == "html" && self.list.trim().is_empty() {
            return Err("HTML providers need a selector for one video card.".into());
        }
        if self.kind == "html" {
            for sel in [&self.list, &self.stream] {
                let css = sel.rsplit_once('@').map(|(c, _)| c).unwrap_or(sel).trim();
                if !css.is_empty() && Selector::parse(css).is_err() {
                    return Err(format!("\"{css}\" is not a valid CSS selector."));
                }
            }
        }
        if !self.stream_regex.is_empty() && regex::RegexBuilder::new(&self.stream_regex).size_limit(1 << 20).build().is_err() {
            return Err("The stream pattern is not a valid regular expression.".into());
        }
        Ok(())
    }
}

pub fn slug(name: &str) -> String {
    let s: String = name.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let s = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    if s.is_empty() { "provider".into() } else { s }
}

// ---------- parsing helpers (pure, unit-tested) ----------

fn json_path<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = v;
    if path.trim().is_empty() {
        return Some(cur);
    }
    for seg in path.trim().split('.') {
        cur = match cur {
            Value::Array(a) => a.get(seg.parse::<usize>().ok()?)?,
            Value::Object(o) => o.get(seg)?,
            _ => return None,
        };
    }
    Some(cur)
}

fn json_text(v: &Value, path: &str) -> Option<String> {
    match json_path(v, path)? {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn first_text(v: &Value, spec: &str, defaults: &[&str]) -> Option<String> {
    if !spec.trim().is_empty() {
        return json_text(v, spec);
    }
    defaults.iter().find_map(|d| json_text(v, d))
}

fn https_join(base: &Url, raw: &str) -> Option<String> {
    let u = base.join(raw.trim()).ok()?;
    (u.scheme() == "https" && u.host_str().is_some()).then(|| u.to_string())
}

fn video_item(id: &str, provider_id: &str, title: &str) -> MediaItem {
    let mut i = MediaItem::new(id, provider_id, title);
    i.media_type = MediaType::Video;
    i
}

pub fn parse_json_list(root: &Value, def: &CustomDefinition, base: &Url, provider_id: &str) -> Vec<MediaItem> {
    let Some(rows) = json_path(root, &def.list).and_then(|v| v.as_array()) else { return vec![] };
    rows.iter()
        .filter_map(|row| {
            let link = https_join(base, &first_text(row, &def.link, &["url", "link", "href", "id"])?)?;
            let title = first_text(row, &def.title, &["title", "name", "label"]).unwrap_or_else(|| link.clone());
            let mut item = video_item(&link, provider_id, &strip_html(&title));
            item.poster_url = first_text(row, &def.image, &["image", "thumbnail", "thumbnail_url", "poster", "cover", "thumb"])
                .and_then(|u| https_join(base, &u));
            item.backdrop_url = item.poster_url.clone();
            item.description = first_text(row, &def.description, &["description", "summary", "overview"]).map(|d| strip_html(&d));
            Some(item)
        })
        .collect()
}

/// `a.title@href` -> (css, Some(attr)); no `@` -> text content. Empty css means the card itself.
fn extract(el: ElementRef, spec: &str) -> Option<String> {
    let spec = spec.trim();
    if spec.is_empty() {
        return None;
    }
    let (css, attr) = match spec.rsplit_once('@') {
        Some((c, a)) => (c.trim(), Some(a.trim())),
        None => (spec, None),
    };
    let target = if css.is_empty() {
        el
    } else {
        let sel = Selector::parse(css).ok()?;
        el.select(&sel).next()?
    };
    let raw = match attr {
        Some(a) => target.value().attr(a)?.to_string(),
        None => target.text().collect::<Vec<_>>().join(" "),
    };
    let cleaned = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    (!cleaned.is_empty()).then_some(cleaned)
}

fn extract_all(root: ElementRef, spec: &str) -> Vec<String> {
    let spec = spec.trim();
    if spec.is_empty() {
        return vec![];
    }
    let (css, attr) = match spec.rsplit_once('@') {
        Some((c, a)) => (c.trim(), Some(a.trim())),
        None => (spec, None),
    };
    let Ok(sel) = Selector::parse(if css.is_empty() { "*" } else { css }) else { return vec![] };
    root.select(&sel)
        .filter_map(|el| match attr {
            Some(a) => el.value().attr(a).map(|v| v.trim().to_string()),
            None => Some(el.text().collect::<Vec<_>>().join(" ").trim().to_string()),
        })
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn parse_html_list(html: &str, def: &CustomDefinition, base: &Url, provider_id: &str) -> Vec<MediaItem> {
    let Ok(card_sel) = Selector::parse(def.list.trim()) else { return vec![] };
    let doc = Html::parse_document(html);
    doc.select(&card_sel)
        .filter_map(|card| {
            let link_spec = if !def.link.trim().is_empty() {
                def.link.clone()
            } else if card.value().name() == "a" {
                "@href".to_string()
            } else {
                "a@href".to_string()
            };
            let link = https_join(base, &extract(card, &link_spec)?)?;
            let title = extract(card, &def.title)
                .or_else(|| extract(card, "a@title"))
                .or_else(|| extract(card, "img@alt"))
                .or_else(|| extract(card, ""))
                .or_else(|| Some(card.text().collect::<Vec<_>>().join(" ")))
                .map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "))
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| link.clone());
            let title: String = title.chars().take(160).collect();
            let mut item = video_item(&link, provider_id, &title);
            let img_spec = if def.image.trim().is_empty() { "img@src".to_string() } else { def.image.clone() };
            item.poster_url = extract(card, &img_spec)
                .or_else(|| extract(card, "img@data-src"))
                .and_then(|u| https_join(base, &u));
            item.backdrop_url = item.poster_url.clone();
            item.description = extract(card, &def.description).map(|d| d.chars().take(400).collect());
            Some(item)
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct FeedItem {
    pub item: MediaItem,
    /// The id is a direct media file (enclosure) rather than a page.
    pub direct: bool,
}

fn children<'a, 'b>(n: roxmltree::Node<'a, 'b>, name: &str) -> std::vec::IntoIter<roxmltree::Node<'a, 'b>> {
    n.children().filter(|c| c.is_element() && c.tag_name().name() == name).collect::<Vec<_>>().into_iter()
}

fn deep<'a, 'b>(n: roxmltree::Node<'a, 'b>, name: &str) -> Option<roxmltree::Node<'a, 'b>> {
    n.descendants().find(|c| c.is_element() && c.tag_name().name() == name)
}

pub fn feed_title(text: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(text).ok()?;
    let root = doc.root_element();
    children(root, "title")
        .next()
        .or_else(|| children(root, "channel").next().and_then(|c| children(c, "title").next()))
        .and_then(|t| t.text())
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

pub fn parse_feed(text: &str, base: &Url, provider_id: &str) -> Result<Vec<FeedItem>, ProviderError> {
    let doc = roxmltree::Document::parse(text).map_err(|_| ProviderError::InvalidResponse)?;
    let mut out = vec![];
    for node in doc.descendants().filter(|n| n.is_element() && matches!(n.tag_name().name(), "item" | "entry")) {
        let title = children(node, "title").next().and_then(|t| t.text()).map(strip_html).unwrap_or_default();

        let page_link = children(node, "link")
            .find(|l| l.attribute("href").is_some() && matches!(l.attribute("rel"), None | Some("alternate")))
            .and_then(|l| l.attribute("href").map(String::from))
            .or_else(|| children(node, "link").find_map(|l| l.text().map(|t| t.trim().to_string()).filter(|t| !t.is_empty())));

        // Media file: <enclosure> or <media:content> with a video/audio type.
        let media = children(node, "enclosure")
            .chain(node.descendants().filter(|d| d.is_element() && d.tag_name().name() == "content" && d.attribute("url").is_some()))
            .find(|e| {
                let ty = e.attribute("type").unwrap_or("");
                let medium = e.attribute("medium").unwrap_or("");
                ty.starts_with("video/") || ty.starts_with("audio/") || medium == "video" || medium == "audio"
            })
            .and_then(|e| e.attribute("url").map(String::from));

        let (raw_id, direct) = match (&media, &page_link) {
            (Some(m), _) => (m.clone(), true),
            (None, Some(l)) => (l.clone(), false),
            _ => continue,
        };
        let Some(id) = https_join(base, &raw_id) else { continue };

        let mut item = video_item(&id, provider_id, if title.is_empty() { &id } else { &title });
        item.poster_url = node
            .descendants()
            .filter(|d| d.is_element() && matches!(d.tag_name().name(), "thumbnail" | "image"))
            .find_map(|d| d.attribute("url").or_else(|| d.attribute("href")).map(String::from))
            .or_else(|| {
                children(node, "enclosure")
                    .find(|e| e.attribute("type").unwrap_or("").starts_with("image/"))
                    .and_then(|e| e.attribute("url").map(String::from))
            })
            .and_then(|u| https_join(base, &u));
        item.backdrop_url = item.poster_url.clone();
        item.description = ["description", "summary", "content"]
            .iter()
            .find_map(|n| children(node, n).next().and_then(|d| d.text()))
            .or_else(|| deep(node, "description").and_then(|d| d.text()))
            .map(|d| strip_html(d).chars().take(600).collect::<String>())
            .filter(|d| !d.is_empty());
        item.release_year = ["pubDate", "published", "updated"]
            .iter()
            .find_map(|n| children(node, n).next().and_then(|d| d.text()))
            .and_then(|d| d.split(|c: char| !c.is_ascii_digit()).find(|p| p.len() == 4).and_then(|p| p.parse::<i32>().ok()))
            .filter(|y| (1900..=2100).contains(y));
        out.push(FeedItem { item, direct });
    }
    if out.is_empty() {
        return Err(ProviderError::InvalidResponse);
    }
    Ok(out)
}

fn m3u_attr(line: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=\"");
    let start = line.find(&needle)? + needle.len();
    let end = line[start..].find('"')? + start;
    let v = line[start..end].trim();
    (!v.is_empty()).then(|| v.to_string())
}

/// (item, group)
pub fn parse_m3u(text: &str, base: &Url, provider_id: &str) -> Vec<(MediaItem, String)> {
    let mut out = vec![];
    let mut pending: Option<(String, Option<String>, String)> = None; // name, logo, group
    for raw in text.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            let tail_start = rest.rfind('"').map(|i| i + 1).unwrap_or(0);
            let name = rest[tail_start..].split_once(',').map(|(_, n)| n.trim().to_string()).unwrap_or_else(|| {
                rest.split_once(',').map(|(_, n)| n.trim().to_string()).unwrap_or_default()
            });
            pending = Some((name, m3u_attr(rest, "tvg-logo"), m3u_attr(rest, "group-title").unwrap_or_else(|| "Channels".into())));
        } else if !line.is_empty() && !line.starts_with('#') {
            if let Some(url) = https_join(base, line) {
                let (name, logo, group) = pending.take().unwrap_or_else(|| (String::new(), None, "Channels".into()));
                let title = if name.is_empty() { url.clone() } else { name };
                let mut item = video_item(&url, provider_id, &title);
                item.media_type = MediaType::Live;
                item.poster_url = logo.and_then(|l| https_join(base, &l));
                out.push((item, group));
            } else {
                pending = None;
            }
        }
    }
    out
}

// ---------- provider ----------

pub struct CustomProvider {
    def: CustomDefinition,
    client: Arc<dyn HttpClient>,
    web: WebResolver,
    descriptor: ProviderDescriptor,
    peertube: Option<PeerTubeProvider>,
    cache: Mutex<HashMap<String, MediaItem>>,
    direct: Mutex<HashSet<String>>,
}

impl CustomProvider {
    pub fn new(mut def: CustomDefinition, client: Arc<dyn HttpClient>) -> Result<Self, ProviderError> {
        def.validate().map_err(ProviderError::Message)?;
        if def.id.trim().is_empty() {
            def.id = format!("custom.{}", slug(&def.name));
        }
        let host = Url::parse(&def.url).ok().and_then(|u| u.host_str().map(String::from)).unwrap_or_default();
        let descriptor = ProviderDescriptor {
            id: def.id.clone(),
            name: def.name.trim().to_string(),
            description: format!("Custom {} provider for {host}.", def.kind),
            language: "".into(),
            supported_types: vec![MediaType::Video],
            version: 1,
            author: "You".into(),
            source_url: Some(def.url.clone()),
            authorization_note: "Created by you. You are responsible for having the right to access this content. Protected (DRM) streams are not supported.".into(),
        };
        let peertube = if def.kind == "peertube" {
            Some(PeerTubeProvider::new(client.clone(), &def.id, def.name.trim(), &def.url)?)
        } else {
            None
        };
        Ok(Self {
            web: WebResolver::new(client.clone()),
            def,
            client,
            descriptor,
            peertube,
            cache: Mutex::new(HashMap::new()),
            direct: Mutex::new(HashSet::new()),
        })
    }

    pub fn definition(&self) -> &CustomDefinition {
        &self.def
    }

    fn fetch_text(&self, url: &str) -> Result<(String, Url), ProviderError> {
        let checked = require_secure(url)?;
        let base = Url::parse(&checked).map_err(|_| ProviderError::InvalidResponse)?;
        let bytes = self.client.get(&checked)?;
        Ok((String::from_utf8_lossy(&bytes).into_owned(), base))
    }

    /// Items from one listing address, parsed according to the provider kind.
    fn listing(&self, url: &str) -> Result<Vec<(MediaItem, String)>, ProviderError> {
        let (text, base) = self.fetch_text(url)?;
        let pid = self.def.id.as_str();
        let rows: Vec<(MediaItem, String)> = match self.def.kind.as_str() {
            "rss" => {
                let feed = parse_feed(&text, &base, pid)?;
                let mut direct = self.direct.lock().unwrap();
                feed.into_iter()
                    .map(|f| {
                        if f.direct {
                            direct.insert(f.item.id.clone());
                        }
                        (f.item, "Latest".to_string())
                    })
                    .collect()
            }
            "m3u" => {
                let rows = parse_m3u(&text, &base, pid);
                let mut direct = self.direct.lock().unwrap();
                for (i, _) in &rows {
                    direct.insert(i.id.clone());
                }
                rows
            }
            "json" => {
                let root: Value = serde_json::from_str(&text).map_err(|_| ProviderError::InvalidResponse)?;
                parse_json_list(&root, &self.def, &base, pid).into_iter().map(|i| (i, "Browse".to_string())).collect()
            }
            _ => parse_html_list(&text, &self.def, &base, pid).into_iter().map(|i| (i, "Browse".to_string())).collect(),
        };
        let mut cache = self.cache.lock().unwrap();
        for (item, _) in &rows {
            cache.insert(item.id.clone(), item.clone());
        }
        Ok(rows)
    }

    fn search_address(&self, query: &str) -> Option<String> {
        let t = self.def.search_url.trim();
        (!t.is_empty()).then(|| t.replace("{query}", &utf8_percent_encode(query, NON_ALPHANUMERIC).to_string()))
    }

    fn stream_from_page(&self, page_url: &str) -> Result<Vec<VideoSource>, ProviderError> {
        let (text, base) = self.fetch_text(page_url)?;
        let mut found: Vec<String> = vec![];
        if !self.def.stream.trim().is_empty() {
            let doc = Html::parse_document(&text);
            found.extend(extract_all(doc.root_element(), &self.def.stream));
        }
        if !self.def.stream_regex.is_empty() {
            if let Ok(re) = regex::RegexBuilder::new(&self.def.stream_regex).size_limit(1 << 20).build() {
                for caps in re.captures_iter(&text).take(8) {
                    if let Some(m) = caps.get(1).or_else(|| caps.get(0)) {
                        found.push(m.as_str().replace("\\/", "/"));
                    }
                }
            }
        }
        let mut out: Vec<VideoSource> = vec![];
        for raw in found {
            let Some(link) = https_join(&base, &raw) else { continue };
            let Ok(u) = Url::parse(&link) else { continue };
            if web::is_media_url(&u) || link.contains(".m3u8") {
                out.push(make_source(&self.def.id, &web::host_label(&link), "Default", &link, false));
            } else {
                // Not a media file: let the resolver treat it as a player page or known site.
                out.extend(self.web.sources_from_html(&base, &format!("<iframe src=\"{link}\"></iframe>")));
            }
        }
        Ok(out)
    }
}

impl MediaProvider for CustomProvider {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn home_catalogs(&self) -> Result<Vec<Catalog>, ProviderError> {
        if let Some(p) = &self.peertube {
            return p.home_catalogs();
        }
        let rows = self.listing(&self.def.url)?;
        if rows.is_empty() {
            return Err(ProviderError::Message("The page was loaded but no videos were found. Check the selectors or fields.".into()));
        }
        if self.def.kind == "m3u" {
            let mut groups: Vec<(String, Vec<MediaItem>)> = vec![];
            for (item, group) in rows {
                match groups.iter_mut().find(|(g, _)| *g == group) {
                    Some((_, list)) => list.push(item),
                    None => groups.push((group, vec![item])),
                }
            }
            return Ok(groups
                .into_iter()
                .take(12)
                .enumerate()
                .map(|(i, (title, items))| Catalog { id: format!("{}.{i}", self.def.id), title, items: items.into_iter().take(60).collect() })
                .collect());
        }
        Ok(vec![Catalog {
            id: format!("{}.all", self.def.id),
            title: self.def.name.trim().to_string(),
            items: rows.into_iter().map(|(i, _)| i).take(80).collect(),
        }])
    }

    fn search(&self, query: &str, page: u32) -> Result<Vec<MediaItem>, ProviderError> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(vec![]);
        }
        if let Some(p) = &self.peertube {
            return p.search(q, page);
        }
        if page > 1 {
            return Ok(vec![]);
        }
        match self.search_address(q) {
            Some(url) => Ok(self.listing(&url)?.into_iter().map(|(i, _)| i).collect()),
            None => {
                let needle = q.to_lowercase();
                Ok(self.listing(&self.def.url)?.into_iter().map(|(i, _)| i).filter(|i| i.title.to_lowercase().contains(&needle)).collect())
            }
        }
    }

    fn load_details(&self, id: &str) -> Result<MediaDetails, ProviderError> {
        if let Some(p) = &self.peertube {
            return p.load_details(id);
        }
        if !self.cache.lock().unwrap().contains_key(id) {
            let _ = self.listing(&self.def.url); // repopulate after a restart
        }
        let cached = self.cache.lock().unwrap().get(id).cloned();
        let item = match cached {
            Some(i) => i,
            None => {
                let mut i = self.web.item(id)?;
                i.provider_id = self.def.id.clone();
                i
            }
        };
        Ok(MediaDetails { item, seasons: vec![], languages: vec![] })
    }

    fn load_episodes(&self, _media_id: &str, _season: Option<i32>) -> Result<Vec<MediaEpisode>, ProviderError> {
        Ok(vec![])
    }

    fn resolve_sources(&self, media_id: &str, _episode: Option<&MediaEpisode>) -> Result<Vec<VideoSource>, ProviderError> {
        if let Some(p) = &self.peertube {
            return p.resolve_sources(media_id, None);
        }
        if !self.direct.lock().unwrap().contains(media_id) && matches!(self.def.kind.as_str(), "rss" | "m3u") {
            let _ = self.listing(&self.def.url);
        }
        if self.direct.lock().unwrap().contains(media_id) {
            return Ok(vec![make_source(&self.def.id, &web::host_label(media_id), "Default", media_id, false)]);
        }
        let mut out = vec![];
        if self.def.kind == "html" && (!self.def.stream.trim().is_empty() || !self.def.stream_regex.is_empty()) {
            out = self.stream_from_page(media_id)?;
        }
        if out.is_empty() {
            out = self.web.sources(media_id)?;
        }
        if out.is_empty() {
            return Err(ProviderError::NoPlayableSource);
        }
        for s in &mut out {
            s.provider_id = self.def.id.clone();
        }
        Ok(out)
    }
}

// ---------- "paste any address" detection ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detected {
    pub definition: CustomDefinition,
    pub note: String,
}

fn guess_json_list(v: &Value) -> Option<(String, &Vec<Value>)> {
    fn walk<'a>(v: &'a Value, path: String, depth: u8) -> Option<(String, &'a Vec<Value>)> {
        match v {
            Value::Array(a) if a.first().map(|x| x.is_object()).unwrap_or(false) => Some((path, a)),
            Value::Object(o) if depth < 3 => o.iter().find_map(|(k, child)| {
                let next = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
                walk(child, next, depth + 1)
            }),
            _ => None,
        }
    }
    walk(v, String::new(), 0)
}

pub fn detect(client: Arc<dyn HttpClient>, raw: &str) -> Result<Detected, ProviderError> {
    let checked = require_secure(raw)?;
    let url = Url::parse(&checked).map_err(|_| ProviderError::InvalidResponse)?;
    let host = url.host_str().unwrap_or("").trim_start_matches("www.").to_string();
    let mut def = CustomDefinition { name: host.clone(), url: checked.clone(), ..Default::default() };

    // YouTube channel / playlist links map to YouTube's official RSS feeds.
    if host == "youtube.com" || host == "m.youtube.com" {
        let segs: Vec<&str> = url.path_segments().map(|s| s.filter(|x| !x.is_empty()).collect()).unwrap_or_default();
        let list = url.query_pairs().find(|(k, _)| k == "list").map(|(_, v)| v.to_string());
        let feed = if segs.first() == Some(&"channel") && segs.len() >= 2 {
            Some(format!("https://www.youtube.com/feeds/videos.xml?channel_id={}", segs[1]))
        } else if let Some(l) = list.filter(|_| segs.first() == Some(&"playlist")) {
            Some(format!("https://www.youtube.com/feeds/videos.xml?playlist_id={l}"))
        } else {
            None
        };
        if let Some(f) = feed {
            def.kind = "rss".into();
            def.url = f;
            def.name = "YouTube feed".into();
            return Ok(Detected { definition: def, note: "YouTube's official RSS feed. Videos play in the embedded YouTube player.".into() });
        }
    }

    let bytes = client.get(&checked)?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let head = text.trim_start();

    if head.starts_with("#EXTM3U") {
        def.kind = "m3u".into();
        return Ok(Detected { definition: def, note: "Detected an M3U playlist. Only HTTPS streams are used.".into() });
    }
    if head.starts_with('{') || head.starts_with('[') {
        let root: Value = serde_json::from_str(&text).map_err(|_| ProviderError::InvalidResponse)?;
        let (path, rows) = guess_json_list(&root)
            .ok_or_else(|| ProviderError::Message("That JSON has no list of videos the app can recognise.".into()))?;
        def.kind = "json".into();
        def.list = path;
        if let Some(first) = rows.first() {
            let has = |k: &str| first.get(k).is_some();
            for (field, candidates) in [
                ("title", ["title", "name", "label"]),
                ("link", ["url", "link", "href"]),
                ("image", ["image", "thumbnail", "poster"]),
            ] {
                if let Some(c) = candidates.iter().find(|c| has(c)) {
                    match field {
                        "title" => def.title = (*c).to_string(),
                        "link" => def.link = (*c).to_string(),
                        _ => def.image = (*c).to_string(),
                    }
                }
            }
        }
        return Ok(Detected { definition: def, note: "Detected a JSON API. Check the field names, then press Test.".into() });
    }
    if head.starts_with("<?xml") || head.starts_with("<rss") || head.starts_with("<feed") {
        if let Some(t) = feed_title(&text) {
            def.name = t;
        }
        def.kind = "rss".into();
        return Ok(Detected { definition: def, note: "Detected an RSS/Atom feed.".into() });
    }

    // HTML page: look for an advertised feed, then a PeerTube instance, then fall back to selectors.
    let doc = Html::parse_document(&text);
    if let Ok(sel) = Selector::parse("title") {
        if let Some(t) = doc.select(&sel).next() {
            let t = strip_html(&t.text().collect::<String>());
            if !t.is_empty() {
                def.name = t.chars().take(60).collect();
            }
        }
    }
    if let Ok(sel) = Selector::parse("link[rel=\"alternate\"][href]") {
        for l in doc.select(&sel) {
            let ty = l.value().attr("type").unwrap_or("").to_lowercase();
            if ty.contains("rss") || ty.contains("atom") {
                if let Some(feed) = l.value().attr("href").and_then(|h| https_join(&url, h)) {
                    def.kind = "rss".into();
                    def.url = feed;
                    return Ok(Detected { definition: def, note: "This page advertises an RSS/Atom feed, so that feed is used.".into() });
                }
            }
        }
    }
    let origin = url.origin().ascii_serialization();
    if let Ok(cfg) = client.get(&format!("{origin}/api/v1/config")) {
        if serde_json::from_slice::<Value>(&cfg).ok().map(|v| v.get("instance").is_some()).unwrap_or(false) {
            def.kind = "peertube".into();
            def.url = origin;
            return Ok(Detected { definition: def, note: "Detected a PeerTube instance.".into() });
        }
    }
    def.kind = "html".into();
    Ok(Detected {
        definition: def,
        note: "A normal web page. Enter a CSS selector for one video card (for example article or .video-item), then press Test.".into(),
    })
}
