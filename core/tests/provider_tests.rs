use cloudstream_core::archive::{self, InternetArchiveProvider};
use cloudstream_core::compat::{evaluate, CompatibilityStatus};
use cloudstream_core::http::{HttpClient, MockClient, NetError};
use cloudstream_core::models::*;
use cloudstream_core::provider::*;
use cloudstream_core::repo::PluginEntry;
use std::collections::HashMap;
use std::sync::Arc;

struct Stub {
    d: ProviderDescriptor,
    items: Vec<MediaItem>,
    fail: bool,
}

impl Stub {
    fn new(id: &str, items: Vec<MediaItem>, fail: bool) -> Arc<Self> {
        Arc::new(Self {
            d: ProviderDescriptor {
                id: id.into(),
                name: id.into(),
                description: String::new(),
                language: "en".into(),
                supported_types: vec![MediaType::Movie],
                version: 1,
                author: "t".into(),
                source_url: None,
                authorization_note: String::new(),
            },
            items,
            fail,
        })
    }
}

impl MediaProvider for Stub {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.d
    }
    fn search(&self, q: &str, _p: u32) -> Result<Vec<MediaItem>, ProviderError> {
        if self.fail {
            return Err(ProviderError::InvalidResponse);
        }
        Ok(self.items.iter().filter(|i| i.title.to_lowercase().contains(&q.to_lowercase())).cloned().collect())
    }
    fn load_details(&self, id: &str) -> Result<MediaDetails, ProviderError> {
        let item = self.items.iter().find(|i| i.id == id).ok_or(ProviderError::NotFound)?.clone();
        Ok(MediaDetails { item, seasons: vec![1, 2], languages: vec![] })
    }
    fn load_episodes(&self, _m: &str, season: Option<i32>) -> Result<Vec<MediaEpisode>, ProviderError> {
        Ok(vec![MediaEpisode {
            id: "e1".into(),
            title: "One".into(),
            episode_number: Some(1),
            season_number: season,
            description: None,
            thumbnail_url: None,
            duration: None,
        }])
    }
    fn resolve_sources(&self, _m: &str, _e: Option<&MediaEpisode>) -> Result<Vec<VideoSource>, ProviderError> {
        Ok(vec![VideoSource {
            id: "s".into(),
            url: "https://example.com/v.m3u8".into(),
            quality: "HD".into(),
            title: "t".into(),
            container: "hls".into(),
            mime_type: None,
            headers: HashMap::new(),
            subtitles: vec![],
            provider_id: self.d.id.clone(),
        }])
    }
}

fn item(id: &str, title: &str, provider: &str) -> MediaItem {
    MediaItem::new(id, provider, title)
}

#[test]
fn register_and_remove() {
    let reg = ProviderRegistry::new();
    reg.register(Stub::new("a", vec![], false));
    reg.register(Stub::new("b", vec![], false));
    assert_eq!(reg.ids(), ["a", "b"]);
    reg.unregister("a");
    assert_eq!(reg.ids(), ["b"]);
    assert!(reg.get("a").is_none());
}

#[test]
fn re_registering_replaces_instead_of_duplicating() {
    let reg = ProviderRegistry::new();
    reg.register(Stub::new("a", vec![], false));
    reg.register(Stub::new("a", vec![], false));
    assert_eq!(reg.ids().len(), 1);
}

#[test]
fn removed_provider_is_not_searched() {
    let reg = ProviderRegistry::new();
    reg.register(Stub::new("a", vec![item("1", "Alpha", "a")], false));
    reg.register(Stub::new("b", vec![item("2", "Alpha Two", "b")], false));
    reg.unregister("a");
    let results = reg.search_all("alpha", 1);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0.id, "b");
}

#[test]
fn search_reports_per_provider_failure() {
    let reg = ProviderRegistry::new();
    reg.register(Stub::new("ok", vec![item("1", "Alpha", "ok")], false));
    reg.register(Stub::new("bad", vec![], true));
    let results = reg.search_all("alpha", 1);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].1.as_ref().unwrap().len(), 1);
    assert!(results[1].1.is_err());
}

#[test]
fn details_episodes_sources() {
    let p = Stub::new("a", vec![item("1", "Show", "a")], false);
    assert_eq!(p.load_details("1").unwrap().seasons, [1, 2]);
    let eps = p.load_episodes("1", Some(2)).unwrap();
    assert_eq!(eps[0].season_number, Some(2));
    assert_eq!(p.resolve_sources("1", eps.first()).unwrap()[0].container, "hls");
    assert_eq!(p.load_details("zzz").unwrap_err(), ProviderError::NotFound);
}

#[test]
fn stable_item_key() {
    assert_eq!(item("9", "x", "p").key(), "p::9");
}

// ---- compatibility ----

#[test]
fn phisher_style_entries_are_never_marked_compatible() {
    let mut p = PluginEntry::new("https://e.com/a.cs3", "StreamPlay");
    p.tv_types = vec!["Movie".into()];
    let s = evaluate(&p, &HashMap::new());
    assert_eq!(s, CompatibilityStatus::MetadataOnly);
    assert!(!s.is_installable());
}

#[test]
fn torrent_only_and_down_are_not_supported() {
    let mut t = PluginEntry::new("https://e.com/a.cs3", "T");
    t.tv_types = vec!["Torrent".into()];
    assert!(matches!(evaluate(&t, &HashMap::new()), CompatibilityStatus::NotSupported { .. }));
    let mut d = PluginEntry::new("https://e.com/a.cs3", "D");
    d.status = Some(0);
    assert!(matches!(evaluate(&d, &HashMap::new()), CompatibilityStatus::NotSupported { .. }));
}

#[test]
fn explicit_native_mapping_is_installable() {
    let mut m = HashMap::new();
    m.insert("Mapped".to_string(), "native.mapped".to_string());
    let s = evaluate(&PluginEntry::new("https://e.com/a.cs3", "Mapped"), &m);
    assert_eq!(s, CompatibilityStatus::NativeAvailable { provider_id: "native.mapped".into() });
    assert!(s.is_installable());
}

// ---- Internet Archive ----

#[test]
fn archive_video_file_selection() {
    let root: serde_json::Value = serde_json::json!({"files": [
        {"name": "a.mp4", "format": "h.264", "size": "900"},
        {"name": "b.mp4", "format": "512Kb MPEG4", "size": "100"},
        {"name": "c.ogv", "format": "Ogg Video", "size": "5000"},
        {"name": "d.txt", "format": "Text", "size": "1"}
    ]});
    let names: Vec<_> = archive::video_files(&root).into_iter().map(|f| f.name).collect();
    assert_eq!(names, ["a.mp4", "b.mp4"]);
}

#[test]
fn archive_source_url_encoding() {
    let c = Arc::new(MockClient::new());
    c.set(
        "https://archive.org/metadata/my-film",
        r#"{"metadata":{"title":"T"},"files":[{"name":"My Film (1950).mp4","format":"h.264","size":"10"}]}"#,
    );
    let p = InternetArchiveProvider::new(c);
    let s = p.resolve_sources("my-film", None).unwrap();
    assert_eq!(s[0].url, "https://archive.org/download/my-film/My%20Film%20(1950).mp4");
}

#[test]
fn archive_no_sources_errors() {
    let c = Arc::new(MockClient::new());
    c.set("https://archive.org/metadata/x", r#"{"metadata":{"title":"T"},"files":[]}"#);
    let p = InternetArchiveProvider::new(c);
    assert_eq!(p.resolve_sources("x", None).unwrap_err(), ProviderError::NoPlayableSource);
}

struct AnyClient;
impl HttpClient for AnyClient {
    fn get(&self, _url: &str) -> Result<Vec<u8>, NetError> {
        Ok(br#"{"response":{"docs":[{"identifier":"film1","title":"Night of the Living Dead","year":1968,"description":"<b>Classic</b> &amp; scary"}]}}"#.to_vec())
    }
}

#[test]
fn archive_search_parses_docs() {
    let p = InternetArchiveProvider::new(Arc::new(AnyClient));
    let items = p.search("night", 1).unwrap();
    assert_eq!(items[0].id, "film1");
    assert_eq!(items[0].release_year, Some(1968));
    assert_eq!(items[0].description.as_deref(), Some("Classic & scary"));
    assert_eq!(items[0].poster_url.as_deref(), Some("https://archive.org/services/img/film1"));
}

#[test]
fn archive_empty_query_returns_nothing_without_a_request() {
    let c = Arc::new(MockClient::new());
    let p = InternetArchiveProvider::new(c.clone());
    assert!(p.search("  \"  ", 1).unwrap().is_empty());
    assert!(c.requested().is_empty());
}
