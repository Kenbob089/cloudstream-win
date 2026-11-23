use cloudstream_core::api::Context;
use cloudstream_core::archive;
use cloudstream_core::custom::{self, CustomDefinition, CustomProvider};
use cloudstream_core::http::{MockClient, NetError};
use cloudstream_core::models::MediaType;
use cloudstream_core::peertube;
use cloudstream_core::provider::{MediaProvider, ProviderError};
use cloudstream_core::store::Store;
use cloudstream_core::web::{self, classify, Target, WebProvider};
use cloudstream_core::youtube::{self, YouTubeProvider};
use serde_json::json;
use std::sync::{Arc, RwLock};
use url::Url;

fn u(s: &str) -> Url {
    Url::parse(s).unwrap()
}

// ---- link classification ----

#[test]
fn classifies_known_sites_and_files() {
    assert_eq!(classify(&u("https://www.youtube.com/watch?v=dQw4w9WgXcQ")), Target::YouTube("dQw4w9WgXcQ".into()));
    assert_eq!(classify(&u("https://youtu.be/dQw4w9WgXcQ?t=5")), Target::YouTube("dQw4w9WgXcQ".into()));
    assert_eq!(classify(&u("https://www.youtube.com/shorts/dQw4w9WgXcQ")), Target::YouTube("dQw4w9WgXcQ".into()));
    assert_eq!(classify(&u("https://vimeo.com/channels/staffpicks/76979871")), Target::Vimeo("76979871".into()));
    assert_eq!(classify(&u("https://www.dailymotion.com/video/x8abc12_title")), Target::Dailymotion("x8abc12".into()));
    assert_eq!(classify(&u("https://example.com/media/film.MP4")), Target::Direct);
    assert_eq!(classify(&u("https://example.com/live/index.m3u8")), Target::Direct);
    assert_eq!(
        classify(&u("https://tube.example/w/abc123")),
        Target::PeerTube { host: "https://tube.example".into(), id: "abc123".into() }
    );
    assert_eq!(classify(&u("https://example.com/some/page")), Target::Page);
    // A YouTube link that is not a single video stays a page.
    assert_eq!(classify(&u("https://www.youtube.com/playlist?list=PL123")), Target::Page);
}

#[test]
fn http_links_are_refused() {
    let p = WebProvider::new(Arc::new(MockClient::new()));
    assert!(p.search("http://example.com/v.mp4", 1).unwrap().is_empty()); // not https: not treated as a link
    assert!(p.load_details("http://example.com/v.mp4").is_err());
}

#[test]
fn youtube_link_resolves_to_embed_without_network() {
    let c = Arc::new(MockClient::new());
    let p = WebProvider::new(c.clone());
    let s = p.resolve_sources("https://youtu.be/dQw4w9WgXcQ", None).unwrap();
    assert_eq!(s.len(), 1);
    assert!(s[0].embed);
    assert_eq!(s[0].url, "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ?rel=0&autoplay=1");
    assert!(c.requested().is_empty());
}

#[test]
fn youtube_link_metadata_uses_oembed() {
    let c = Arc::new(MockClient::new());
    let watch = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
    let key = Url::parse_with_params("https://www.youtube.com/oembed", [("url", watch), ("format", "json")]).unwrap();
    c.set(key.as_str(), r#"{"title":"A Title","author_name":"Someone"}"#);
    let p = WebProvider::new(c);
    let found = p.search("https://www.youtube.com/watch?v=dQw4w9WgXcQ", 1).unwrap();
    assert_eq!(found[0].title, "A Title");
    assert_eq!(found[0].description.as_deref(), Some("By Someone"));
    assert_eq!(found[0].media_type, MediaType::Video);
    assert_eq!(found[0].poster_url.as_deref(), Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg"));
}

#[test]
fn page_sources_are_extracted_and_ordered() {
    let c = Arc::new(MockClient::new());
    c.set(
        "https://site.example/watch/1",
        r#"<html><head><meta property="og:title" content="Page Title"></head><body>
        <iframe src="https://player.example/embed/9"></iframe>
        <video><source src="/files/movie.mp4"></video>
        <video src="http://insecure.example/x.mp4"></video></body></html>"#,
    );
    let p = WebProvider::new(c);
    let s = p.resolve_sources("https://site.example/watch/1", None).unwrap();
    assert_eq!(s.len(), 2); // the http link is dropped
    assert_eq!(s[0].url, "https://site.example/files/movie.mp4");
    assert!(!s[0].embed);
    assert!(s[1].embed); // embedded player last
    let item = p.load_details("https://site.example/watch/1").unwrap().item;
    assert_eq!(item.title, "Page Title");
}

#[test]
fn page_without_video_reports_no_source() {
    let c = Arc::new(MockClient::new());
    c.set("https://site.example/empty", "<html><body>nothing</body></html>");
    let p = WebProvider::new(c);
    assert_eq!(p.resolve_sources("https://site.example/empty", None).unwrap_err(), ProviderError::NoPlayableSource);
}

// ---- feeds, playlists, JSON, HTML ----

#[test]
fn parses_youtube_atom_feed() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
    <feed xmlns:yt="http://www.youtube.com/xml/schemas/2015" xmlns:media="http://search.yahoo.com/mrss/" xmlns="http://www.w3.org/2005/Atom">
      <title>Channel Name</title>
      <entry>
        <id>yt:video:dQw4w9WgXcQ</id>
        <title>First video</title>
        <link rel="alternate" href="https://www.youtube.com/watch?v=dQw4w9WgXcQ"/>
        <published>2023-05-01T00:00:00+00:00</published>
        <media:group>
          <media:title>First video</media:title>
          <media:content url="https://www.youtube.com/v/dQw4w9WgXcQ?version=3" type="application/x-shockwave-flash"/>
          <media:thumbnail url="https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg"/>
          <media:description>About it</media:description>
        </media:group>
      </entry>
    </feed>"#;
    assert_eq!(custom::feed_title(xml).as_deref(), Some("Channel Name"));
    let items = custom::parse_feed(xml, &u("https://www.youtube.com/feeds/videos.xml"), "custom.x").unwrap();
    assert_eq!(items.len(), 1);
    let i = &items[0];
    assert!(!i.direct);
    assert_eq!(i.item.id, "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
    assert_eq!(i.item.title, "First video");
    assert_eq!(i.item.release_year, Some(2023));
    assert_eq!(i.item.poster_url.as_deref(), Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg"));
    assert_eq!(i.item.description.as_deref(), Some("About it"));
}

#[test]
fn rss_enclosure_is_a_direct_media_link() {
    let xml = r#"<rss version="2.0"><channel><title>Show</title>
      <item><title>Ep 1</title><link>https://show.example/ep1</link>
      <enclosure url="https://cdn.example/ep1.mp4" type="video/mp4"/>
      <pubDate>Tue, 10 Jun 2014 04:00:00 GMT</pubDate></item></channel></rss>"#;
    let items = custom::parse_feed(xml, &u("https://show.example/feed.xml"), "custom.x").unwrap();
    assert!(items[0].direct);
    assert_eq!(items[0].item.id, "https://cdn.example/ep1.mp4");
    assert_eq!(items[0].item.release_year, Some(2014));
}

#[test]
fn feed_with_dtd_is_rejected() {
    let xml = r#"<?xml version="1.0"?><!DOCTYPE x [<!ENTITY a "b">]><rss><channel><item><title>t</title><link>https://a.example/</link></item></channel></rss>"#;
    assert!(custom::parse_feed(xml, &u("https://a.example/feed"), "custom.x").is_err());
}

#[test]
fn parses_m3u_playlist() {
    let m3u = "#EXTM3U\n#EXTINF:-1 tvg-logo=\"https://img.example/a.png\" group-title=\"News\",Channel A\nhttps://stream.example/a.m3u8\n#EXTINF:-1,Channel B\nhttp://insecure.example/b.m3u8\nhttps://stream.example/c.m3u8\n";
    let rows = custom::parse_m3u(m3u, &u("https://lists.example/p.m3u"), "custom.tv");
    assert_eq!(rows.len(), 2); // the http entry is skipped
    assert_eq!(rows[0].0.title, "Channel A");
    assert_eq!(rows[0].1, "News");
    assert_eq!(rows[0].0.poster_url.as_deref(), Some("https://img.example/a.png"));
    assert_eq!(rows[1].0.title, "https://stream.example/c.m3u8"); // no name: falls back to the address
}

#[test]
fn parses_json_list_with_paths_and_defaults() {
    let root = json!({"data": {"videos": [
        {"name": "One", "url": "/v/1", "thumbnail": "https://img.example/1.jpg", "meta": {"desc": "First"}},
        {"title": "no link"}
    ]}});
    let def = CustomDefinition { list: "data.videos".into(), description: "meta.desc".into(), ..Default::default() };
    let items = custom::parse_json_list(&root, &def, &u("https://api.example/list"), "custom.j");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "https://api.example/v/1");
    assert_eq!(items[0].title, "One");
    assert_eq!(items[0].description.as_deref(), Some("First"));
}

#[test]
fn parses_html_cards_with_selectors() {
    let html = r#"<div class="grid">
      <article class="card"><a href="/watch/1" title="Alpha"><img data-src="/img/1.jpg"></a><h3>Alpha</h3></article>
      <article class="card"><a href="/watch/2"><img src="https://img.example/2.jpg"></a><h3>Beta</h3></article>
    </div>"#;
    let def = CustomDefinition { list: "article.card".into(), title: "h3".into(), ..Default::default() };
    let items = custom::parse_html_list(html, &def, &u("https://site.example/browse"), "custom.h");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].id, "https://site.example/watch/1");
    assert_eq!(items[0].title, "Alpha");
    assert_eq!(items[0].poster_url.as_deref(), Some("https://site.example/img/1.jpg"));
    assert_eq!(items[1].poster_url.as_deref(), Some("https://img.example/2.jpg"));
}

// ---- custom provider ----

fn rss_def() -> CustomDefinition {
    CustomDefinition {
        name: "My Feed".into(),
        kind: "rss".into(),
        url: "https://feed.example/rss.xml".into(),
        search_url: "https://feed.example/search?q={query}".into(),
        ..Default::default()
    }
}

#[test]
fn definition_validation() {
    assert!(rss_def().validate().is_ok());
    let mut d = rss_def();
    d.url = "http://feed.example/rss.xml".into();
    assert!(d.validate().is_err());
    let mut d = rss_def();
    d.search_url = "https://feed.example/search".into();
    assert!(d.validate().is_err()); // missing {query}
    let mut d = rss_def();
    d.kind = "html".into();
    assert!(d.validate().is_err()); // no card selector
    d.list = "div[".into();
    assert!(d.validate().is_err()); // bad selector
    d.list = "div.card".into();
    d.stream_regex = "(".into();
    assert!(d.validate().is_err()); // bad regex
}

#[test]
fn custom_rss_provider_searches_with_encoded_query() {
    let c = Arc::new(MockClient::new());
    c.set(
        "https://feed.example/search?q=cat%20dogs",
        r#"<rss><channel><item><title>Cats and dogs</title><link>https://feed.example/p/1</link><enclosure url="https://cdn.example/1.mp4" type="video/mp4"/></item></channel></rss>"#,
    );
    let p = CustomProvider::new(rss_def(), c).unwrap();
    assert_eq!(p.descriptor().id, "custom.my-feed");
    let found = p.search("cat dogs", 1).unwrap();
    assert_eq!(found.len(), 1);
    // enclosure files are known to be direct, so no page is fetched for playback
    let s = p.resolve_sources("https://cdn.example/1.mp4", None).unwrap();
    assert_eq!(s[0].url, "https://cdn.example/1.mp4");
    assert_eq!(s[0].provider_id, "custom.my-feed");
}

#[test]
fn custom_html_provider_uses_stream_selector_and_regex() {
    let c = Arc::new(MockClient::new());
    c.set(
        "https://site.example/watch/9",
        r#"<html><body><video><source src="https://cdn.example/a.mp4"></video>
        <script>var cfg = {"hls":"https:\/\/cdn.example\/live\/master.m3u8"};</script></body></html>"#,
    );
    let def = CustomDefinition {
        name: "Site".into(),
        kind: "html".into(),
        url: "https://site.example/browse".into(),
        list: "article".into(),
        stream: "video source@src".into(),
        stream_regex: r#""hls":"([^"]+)""#.into(),
        ..Default::default()
    };
    let p = CustomProvider::new(def, c).unwrap();
    let s = p.resolve_sources("https://site.example/watch/9", None).unwrap();
    let urls: Vec<_> = s.iter().map(|x| x.url.as_str()).collect();
    assert_eq!(urls, ["https://cdn.example/a.mp4", "https://cdn.example/live/master.m3u8"]);
}

#[test]
fn detect_recognises_feeds_playlists_and_json() {
    let c = Arc::new(MockClient::new());
    c.set("https://a.example/feed", r#"<?xml version="1.0"?><rss><channel><title>Podcast</title></channel></rss>"#);
    let d = custom::detect(c.clone(), "https://a.example/feed").unwrap();
    assert_eq!((d.definition.kind.as_str(), d.definition.name.as_str()), ("rss", "Podcast"));

    c.set("https://a.example/p.m3u", "#EXTM3U\n#EXTINF:-1,x\nhttps://s.example/x.m3u8\n");
    assert_eq!(custom::detect(c.clone(), "https://a.example/p.m3u").unwrap().definition.kind, "m3u");

    c.set("https://a.example/api", r#"{"result":{"items":[{"name":"A","url":"https://v.example/1","poster":"https://i.example/1.jpg"}]}}"#);
    let d = custom::detect(c.clone(), "https://a.example/api").unwrap().definition;
    assert_eq!((d.kind.as_str(), d.list.as_str(), d.title.as_str(), d.link.as_str(), d.image.as_str()), ("json", "result.items", "name", "url", "poster"));

    let yt = custom::detect(c.clone(), "https://www.youtube.com/channel/UC1234567890").unwrap().definition;
    assert_eq!(yt.kind, "rss");
    assert_eq!(yt.url, "https://www.youtube.com/feeds/videos.xml?channel_id=UC1234567890");
}

#[test]
fn detect_finds_advertised_feed_and_peertube() {
    let c = Arc::new(MockClient::new());
    c.set(
        "https://blog.example/",
        r#"<html><head><title>Blog</title><link rel="alternate" type="application/rss+xml" href="/rss.xml"></head></html>"#,
    );
    let d = custom::detect(c.clone(), "https://blog.example/").unwrap().definition;
    assert_eq!((d.kind.as_str(), d.url.as_str()), ("rss", "https://blog.example/rss.xml"));

    c.set("https://pt.example/", "<html><head><title>PT</title></head></html>");
    c.set("https://pt.example/api/v1/config", r#"{"instance":{"name":"PT"}}"#);
    assert_eq!(custom::detect(c, "https://pt.example/").unwrap().definition.kind, "peertube");
}

// ---- PeerTube / YouTube ----

#[test]
fn peertube_item_and_sources() {
    let v = json!({"uuid":"u1","name":"Film","description":"<b>Hi</b>","thumbnailPath":"/lazy/t.jpg","publishedAt":"2024-02-03T00:00:00Z","duration":90,"tags":["a","b"]});
    let item = peertube::item_from_json(&v, "https://pt.example", "p").unwrap();
    assert_eq!(item.id, "u1");
    assert_eq!(item.poster_url.as_deref(), Some("https://pt.example/lazy/t.jpg"));
    assert_eq!(item.release_year, Some(2024));
    assert_eq!(item.description.as_deref(), Some("Hi"));

    let c = MockClient::new();
    c.set(
        "https://pt.example/api/v1/videos/u1",
        r#"{"files":[{"fileUrl":"https://pt.example/a-480.mp4","resolution":{"id":480,"label":"480p"}},
                     {"fileUrl":"https://pt.example/a-1080.mp4","resolution":{"id":1080,"label":"1080p"}},
                     {"fileUrl":"http://pt.example/insecure.mp4","resolution":{"id":720,"label":"720p"}}],
            "streamingPlaylists":[{"playlistUrl":"https://pt.example/master.m3u8"}]}"#,
    );
    let s = peertube::sources_for(&c, "https://pt.example", "u1", "p").unwrap();
    let q: Vec<_> = s.iter().map(|x| x.quality.as_str()).collect();
    assert_eq!(q, ["1080p", "480p", "Auto"]);
    assert_eq!(s[2].container, "hls");
}

#[test]
fn youtube_requires_a_key_and_uses_embed() {
    let key = Arc::new(RwLock::new(String::new()));
    let c = Arc::new(MockClient::new());
    let p = YouTubeProvider::new(c.clone(), key.clone());
    match p.search("cats", 1) {
        Err(ProviderError::Message(m)) => assert!(m.contains("API key")),
        other => panic!("expected key message, got {other:?}"),
    }
    assert!(c.requested().is_empty());
    let s = p.resolve_sources("dQw4w9WgXcQ", None).unwrap();
    assert!(s[0].embed);
    assert_eq!(p.resolve_sources("bad id", None).unwrap_err(), ProviderError::NotFound);

    *key.write().unwrap() = "KEY".into();
    let url = Url::parse_with_params(
        "https://www.googleapis.com/youtube/v3/search",
        [("part", "snippet"), ("type", "video"), ("maxResults", "30"), ("q", "cats"), ("safeSearch", "moderate"), ("key", "KEY")],
    )
    .unwrap();
    c.set(
        url.as_str(),
        r#"{"items":[{"id":{"videoId":"dQw4w9WgXcQ"},"snippet":{"title":"Cats &amp; more","channelTitle":"Ch","publishedAt":"2020-01-01T00:00:00Z","thumbnails":{"high":{"url":"https://i.ytimg.com/vi/x/hq.jpg"}}}}]}"#,
    );
    let found = p.search("cats", 1).unwrap();
    assert_eq!(found[0].title, "Cats & more");
    assert_eq!(found[0].release_year, Some(2020));
}

#[test]
fn iso_durations() {
    assert_eq!(youtube::iso_duration("PT1H2M3S"), Some(3723.0));
    assert_eq!(youtube::iso_duration("PT45S"), Some(45.0));
    assert_eq!(youtube::iso_duration("bad"), None);
}

#[test]
fn youtube_quota_errors_are_explained() {
    let key = Arc::new(RwLock::new("KEY".to_string()));
    let c = Arc::new(MockClient::new());
    let url = Url::parse_with_params(
        "https://www.googleapis.com/youtube/v3/search",
        [("part", "snippet"), ("type", "video"), ("maxResults", "30"), ("q", "x"), ("safeSearch", "moderate"), ("key", "KEY")],
    )
    .unwrap();
    c.set_err(url.as_str(), NetError::BadStatus(403));
    let p = YouTubeProvider::new(c, key);
    assert!(p.search("x", 1).unwrap_err().to_string().contains("quota"));
}

// ---- Internet Archive helpers ----

#[test]
fn archive_groups_derivatives_into_one_video() {
    assert_eq!(archive::group_key("Show_Ep1.mp4"), "Show_Ep1");
    assert_eq!(archive::group_key("Show_Ep1_512kb.mp4"), "Show_Ep1");
    assert_eq!(archive::group_key("Show_Ep1.ia.mp4"), "Show_Ep1");
    let root = json!({"files": [
        {"name": "b.mp4", "format": "h.264", "size": "900"},
        {"name": "b_512kb.mp4", "format": "512Kb MPEG4", "size": "100"},
        {"name": "a.mp4", "format": "h.264", "size": "500"}
    ]});
    let groups = archive::grouped_files(&archive::video_files(&root));
    let keys: Vec<_> = groups.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["a", "b"]);
    assert_eq!(groups[1].1.len(), 2);
}

#[test]
fn archive_multi_video_item_becomes_episodes() {
    let c = Arc::new(MockClient::new());
    c.set(
        "https://archive.org/metadata/series",
        r#"{"metadata":{"title":"S"},"files":[
          {"name":"ep1.mp4","format":"h.264","size":"10"},{"name":"ep2.mp4","format":"h.264","size":"10"}]}"#,
    );
    let p = archive::InternetArchiveProvider::new(c);
    let eps = p.load_episodes("series", None).unwrap();
    assert_eq!(eps.len(), 2);
    let s = p.resolve_sources("series", Some(&eps[1])).unwrap();
    assert_eq!(s[0].url, "https://archive.org/download/series/ep2.mp4");
}

#[test]
fn archive_search_terms_strip_operators() {
    assert_eq!(archive::search_terms("night: (of) the \"living\" dead AND OR"), ["night", "of", "the", "living", "dead"]);
    assert!(archive::search_terms(" \" ").is_empty());
}

// ---- API integration ----

#[test]
fn api_saves_tests_and_deletes_custom_providers() {
    let mock = Arc::new(MockClient::new());
    mock.set(
        "https://feed.example/rss.xml",
        r#"<rss><channel><item><title>One</title><link>https://feed.example/1</link></item></channel></rss>"#,
    );
    let ctx = Context::new(mock, Store::in_memory());
    let def = json!({"name":"My Feed","kind":"rss","url":"https://feed.example/rss.xml"});

    let t = ctx.call("provider.test", &json!({"definition": def})).unwrap();
    assert_eq!(t["count"], 1);
    assert_eq!(t["sample"][0], "One");

    let id = ctx.call("provider.save", &json!({"definition": def})).unwrap();
    assert_eq!(id, "custom.my-feed");
    assert!(ctx.registry.ids().contains(&"custom.my-feed".to_string()));
    // a second provider with the same name gets its own id
    let id2 = ctx.call("provider.save", &json!({"definition": def})).unwrap();
    assert_eq!(id2, "custom.my-feed-2");

    let snap = ctx.call("snapshot", &json!({})).unwrap();
    let customs = snap["providers"].as_array().unwrap().iter().filter(|p| p["custom"] == true).count();
    assert_eq!(customs, 2);

    ctx.call("provider.delete", &json!({"id": "custom.my-feed"})).unwrap();
    assert!(!ctx.registry.ids().contains(&"custom.my-feed".to_string()));
    assert!(ctx.registry.ids().contains(&"custom.my-feed-2".to_string()));
}

#[test]
fn api_rejects_insecure_or_invalid_definitions() {
    let ctx = Context::new(Arc::new(MockClient::new()), Store::in_memory());
    let bad = json!({"name":"X","kind":"rss","url":"http://x.example/feed"});
    assert!(ctx.call("provider.save", &json!({"definition": bad})).is_err());
    let bad_id = json!({"id":"native.evil","name":"X","kind":"rss","url":"https://x.example/feed"});
    assert!(ctx.call("provider.save", &json!({"definition": bad_id})).is_err());
}

#[test]
fn settings_key_reaches_the_youtube_provider() {
    let ctx = Context::new(Arc::new(MockClient::new()), Store::in_memory());
    ctx.call("settings.set", &json!({"key":"youtubeApiKey","value":"  abc  "})).unwrap();
    assert_eq!(*ctx.youtube_key.read().unwrap(), "abc");
    ctx.call("data.erase", &json!({})).unwrap();
    assert_eq!(*ctx.youtube_key.read().unwrap(), "");
}

#[test]
fn built_in_provider_list_includes_new_sources() {
    let ctx = Context::new(Arc::new(MockClient::new()), Store::in_memory());
    let ids: Vec<String> = ctx.native.iter().map(|p| p.descriptor().id.clone()).collect();
    for want in [web::ID, archive::ID, peertube::ID, youtube::ID, "native.dailymotion"] {
        assert!(ids.iter().any(|i| i == want), "missing {want}");
    }
}

// ---- declarative repositories: import a link, install everything, update on refresh ----

#[test]
fn declarative_repository_installs_all_and_updates_on_refresh() {
    let mock = Arc::new(MockClient::new());
    let repo = "https://repos.example/r/repo.json";
    // the list is referenced relative to the repo file
    mock.set(repo, r#"{"name":"R","manifestVersion":1,"pluginLists":["providers.json"]}"#);
    mock.set(
        "https://repos.example/r/providers.json",
        r#"[
          {"url":"https://feed.example/a.xml","name":"A","internalName":"A","version":1,"status":1,
           "definition":{"name":"A","kind":"rss","url":"https://feed.example/a.xml"}},
          {"url":"https://x.example/b.cs3","name":"B","internalName":"B","version":3,"status":1,"tvTypes":["Movie"]},
          {"url":"https://c.example/","name":"C","internalName":"C","version":1,
           "definition":{"name":"C","kind":"html","url":"https://c.example/"}}
        ]"#,
    );
    let ctx = Context::new(mock.clone(), Store::in_memory());
    ctx.call("repo.import", &json!({"url": repo})).unwrap();

    let snap = ctx.call("snapshot", &json!({})).unwrap();
    let plugins = &snap["repositories"][0]["plugins"];
    assert_eq!(plugins[0]["status"]["kind"], "declarative");
    assert_eq!(plugins[1]["status"]["kind"], "metadataOnly"); // an Android .cs3 stays metadata only
    assert_eq!(plugins[2]["status"]["kind"], "notSupported"); // invalid definition
    // importing installs nothing
    assert!(!ctx.registry.ids().iter().any(|i| i.starts_with("custom.")));

    let r = ctx.call("plugin.installAll", &json!({"repoUrl": repo})).unwrap();
    assert_eq!(r["installed"], 1);
    assert_eq!(r["failed"].as_array().unwrap().len(), 1);
    assert!(ctx.registry.ids().contains(&"custom.a".to_string()));
    let snap = ctx.call("snapshot", &json!({})).unwrap();
    assert_eq!(snap["repositories"][0]["plugins"][0]["installed"], true);
    assert_eq!(snap["repositories"][0]["plugins"][0]["updateAvailable"], false);

    // the repository publishes version 2 with a new address: refreshing updates the installed provider
    mock.set(
        "https://repos.example/r/providers.json",
        r#"[{"url":"https://feed.example/a2.xml","name":"A","internalName":"A","version":2,"status":1,
             "definition":{"name":"A","kind":"rss","url":"https://feed.example/a2.xml"}}]"#,
    );
    ctx.call("repo.refresh", &json!({"url": repo})).unwrap();
    let snap = ctx.call("snapshot", &json!({})).unwrap();
    let provider = snap["providers"].as_array().unwrap().iter().find(|p| p["descriptor"]["id"] == "custom.a").unwrap();
    assert_eq!(provider["definition"]["url"], "https://feed.example/a2.xml");
    assert_eq!(provider["definition"]["version"], 2);
}

#[test]
fn exported_repository_round_trips() {
    let mock = Arc::new(MockClient::new());
    let ctx = Context::new(mock.clone(), Store::in_memory());
    ctx.call("provider.save", &json!({"definition": {"name":"Mine","kind":"rss","url":"https://feed.example/rss.xml"}})).unwrap();
    let dir = std::env::temp_dir().join(format!("cs_export_test_{}", std::process::id()));
    ctx.call("repo.export", &json!({"dir": dir.to_string_lossy()})).unwrap();
    let repo = std::fs::read_to_string(dir.join("repo.json")).unwrap();
    let list = std::fs::read_to_string(dir.join("providers.json")).unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    // host the two files together and import the repo into a fresh app
    mock.set("https://host.example/mine/repo.json", &repo);
    mock.set("https://host.example/mine/providers.json", &list);
    let fresh = Context::new(mock, Store::in_memory());
    fresh.call("repo.import", &json!({"url": "https://host.example/mine/repo.json"})).unwrap();
    let r = fresh.call("plugin.installAll", &json!({"repoUrl": "https://host.example/mine/repo.json"})).unwrap();
    assert_eq!(r["installed"], 1);
    assert!(fresh.registry.ids().contains(&"custom.mine".to_string()));
}

#[test]
fn export_requires_something_to_export() {
    let ctx = Context::new(Arc::new(MockClient::new()), Store::in_memory());
    assert!(ctx.call("repo.export", &json!({"dir": std::env::temp_dir().to_string_lossy()})).is_err());
}
