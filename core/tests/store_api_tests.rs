use cloudstream_core::api::Context;
use cloudstream_core::archive;
use cloudstream_core::http::MockClient;
use cloudstream_core::models::*;
use cloudstream_core::store::*;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

fn item() -> MediaItem {
    let mut i = MediaItem::new("m1", "p", "Movie");
    i.release_year = Some(1999);
    i
}

fn temp_file(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    std::env::temp_dir().join(format!("cs-{tag}-{}-{nanos}.json", std::process::id()))
}

#[test]
fn bookmark_toggle_saves_and_removes() {
    let s = Store::in_memory();
    assert!(s.with(|d| d.toggle_bookmark(&item())));
    assert_eq!(s.read(|d| d.bookmarks.len()), 1);
    assert!(!s.with(|d| d.toggle_bookmark(&item())));
    assert_eq!(s.read(|d| d.bookmarks.len()), 0);
}

#[test]
fn progress_keys_are_per_episode() {
    let i = item();
    assert_eq!(progress_key(&i, None), "p::m1");
    assert_ne!(progress_key(&i, Some("e1")), progress_key(&i, Some("e2")));
}

#[test]
fn progress_updates_and_completes() {
    let s = Store::in_memory();
    s.with(|d| d.record_progress(&item(), None, 50.0, 100.0, false));
    s.with(|d| d.record_progress(&item(), None, 60.0, 100.0, false));
    assert_eq!(s.read(|d| d.progress.len()), 1);
    assert_eq!(s.read(|d| d.progress[0].position), 60.0);
    assert!(!s.read(|d| d.progress[0].completed));
    s.with(|d| d.record_progress(&item(), None, 97.0, 100.0, false));
    assert!(s.read(|d| d.progress[0].completed));
}

#[test]
fn data_survives_relaunch() {
    let path = temp_file("relaunch");
    {
        let s = Store::open(path.clone());
        s.with(|d| d.toggle_bookmark(&item()));
        s.with(|d| d.installs.push(archive::ID.into()));
    }
    let s2 = Store::open(path.clone());
    assert_eq!(s2.read(|d| d.bookmarks.len()), 1);
    assert_eq!(s2.read(|d| d.installs.clone()), [archive::ID]);
    let _ = std::fs::remove_file(path);
}

#[test]
fn corrupt_store_is_moved_aside_not_crashed_on() {
    let path = temp_file("corrupt");
    std::fs::write(&path, b"{ not json").unwrap();
    let s = Store::open(path.clone());
    assert_eq!(s.read(|d| d.bookmarks.len()), 0);
    assert!(path.with_extension("corrupt.json").exists());
    let _ = std::fs::remove_file(path.with_extension("corrupt.json"));
}

#[test]
fn old_store_with_missing_fields_still_loads() {
    let path = temp_file("old");
    std::fs::write(&path, br#"{"bookmarks":[]}"#).unwrap();
    let s = Store::open(path.clone());
    assert_eq!(s.read(|d| d.version), 1);
    let _ = std::fs::remove_file(path);
}

fn ctx() -> Context {
    Context::new(Arc::new(MockClient::new()), Store::in_memory())
}

#[test]
fn install_and_uninstall_update_registry() {
    let c = ctx();
    assert!(c.registry.ids().is_empty());
    c.call("provider.install", &json!({"id": archive::ID})).unwrap();
    assert_eq!(c.registry.ids(), [archive::ID]);
    c.call("provider.uninstall", &json!({"id": archive::ID})).unwrap();
    assert!(c.registry.ids().is_empty());
}

#[test]
fn installing_unknown_provider_is_rejected() {
    assert!(ctx().call("provider.install", &json!({"id": "nope"})).is_err());
}

#[test]
fn search_without_providers_is_empty_and_details_need_an_enabled_provider() {
    let c = ctx();
    assert_eq!(c.call("search", &json!({"query": "x"})).unwrap(), json!([]));
    assert!(c.call("details", &json!({"providerId": archive::ID, "id": "x"})).is_err());
}

#[test]
fn importing_a_repository_does_not_install_anything() {
    let mock = Arc::new(MockClient::new());
    mock.set("https://example.com/repo.json", r#"{"name":"R","pluginLists":["https://example.com/p.json"]}"#);
    mock.set("https://example.com/p.json", r#"[{"url":"https://example.com/a.cs3","name":"A","tvTypes":["Movie"]}]"#);
    let c = Context::new(mock, Store::in_memory());
    c.call("repo.import", &json!({"url": "https://example.com/repo.json"})).unwrap();
    let snap = c.call("snapshot", &json!({})).unwrap();
    assert_eq!(snap["repositories"][0]["plugins"].as_array().unwrap().len(), 1);
    assert_eq!(snap["repositories"][0]["plugins"][0]["status"]["kind"], "metadataOnly");
    assert_eq!(snap["repositories"][0]["plugins"][0]["status"]["installable"], false);
    assert!(c.registry.ids().is_empty());
    assert_eq!(snap["providers"][0]["installed"], false);
}

#[test]
fn failed_refresh_keeps_cached_catalog() {
    let mock = Arc::new(MockClient::new());
    mock.set("https://example.com/repo.json", r#"{"name":"R","pluginLists":["https://example.com/p.json"]}"#);
    mock.set("https://example.com/p.json", r#"[{"url":"https://example.com/a.cs3","name":"A"}]"#);
    let c = Context::new(mock.clone(), Store::in_memory());
    c.call("repo.import", &json!({"url": "https://example.com/repo.json"})).unwrap();
    mock.set("https://example.com/repo.json", "garbage");
    assert!(c.call("repo.refresh", &json!({"url": "https://example.com/repo.json"})).is_err());
    let snap = c.call("snapshot", &json!({})).unwrap();
    assert_eq!(snap["repositories"][0]["plugins"].as_array().unwrap().len(), 1);
    assert!(snap["repositories"][0]["lastError"].is_string());
}

#[test]
fn unknown_method_errors() {
    assert!(ctx().call("nope", &json!({})).is_err());
}
