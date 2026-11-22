use cloudstream_core::http::{MockClient, NetError};
use cloudstream_core::repo::*;

const REPO: &str = "https://example.com/repo.json";
const LIST1: &str = "https://example.com/plugins.json";
const LIST2: &str = "https://example.com/plugins2.json";

fn manifest(lists: &[&str]) -> String {
    let items: Vec<String> = lists.iter().map(|l| format!("\"{l}\"")).collect();
    format!(r#"{{"name":"Test Repo","description":"d","manifestVersion":1,"pluginLists":[{}]}}"#, items.join(","))
}

fn entry(name: &str, version: i64) -> String {
    format!(
        r#"{{"url":"https://example.com/{name}.cs3","status":1,"version":{version},"name":"{name}","internalName":"{name}","authors":["A"],"language":"en","tvTypes":["Movie","TvSeries"],"apiVersion":1,"fileHash":"sha256-x"}}"#
    )
}

#[test]
fn valid_manifest_follows_plugin_list() {
    let c = MockClient::new();
    c.set(REPO, &manifest(&[LIST1]));
    c.set(LIST1, &format!("[{},{}]", entry("One", 1), entry("Two", 1)));
    let r = import_repository(&c, REPO).unwrap();
    assert_eq!(r.manifest.name, "Test Repo");
    let names: Vec<_> = r.plugins.iter().map(|p| p.internal_name.as_str()).collect();
    assert_eq!(names, ["One", "Two"]);
    assert_eq!(r.plugins[0].file_hash.as_deref(), Some("sha256-x"));
    assert_eq!(c.requested()[0], REPO);
}

#[test]
fn multiple_plugin_lists_are_merged() {
    let c = MockClient::new();
    c.set(REPO, &manifest(&[LIST1, LIST2]));
    c.set(LIST1, &format!("[{}]", entry("One", 1)));
    c.set(LIST2, &format!("[{}]", entry("Two", 1)));
    let r = import_repository(&c, REPO).unwrap();
    assert_eq!(r.plugins.len(), 2);
}

#[test]
fn missing_optional_fields_decode() {
    let (entries, skipped) = decode_plugin_list(br#"[{"url":"https://e.com/a.cs3","name":"Bare"}]"#).unwrap();
    assert_eq!(skipped, 0);
    assert_eq!(entries[0].internal_name, "Bare");
    assert!(entries[0].description.is_none());
    assert!(entries[0].authors.is_empty());
}

#[test]
fn malformed_entry_does_not_fail_list() {
    let json = format!("[{}, {{\"nope\": true}}, 42]", entry("Good", 1));
    let (entries, skipped) = decode_plugin_list(json.as_bytes()).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(skipped, 2);
}

#[test]
fn invalid_json_manifest() {
    let c = MockClient::new();
    c.set(REPO, "not json");
    assert_eq!(import_repository(&c, REPO).unwrap_err(), RepoError::InvalidManifest);
}

#[test]
fn plugin_list_as_manifest_is_rejected() {
    let c = MockClient::new();
    c.set(REPO, &format!("[{}]", entry("One", 1)));
    assert_eq!(import_repository(&c, REPO).unwrap_err(), RepoError::InvalidManifest);
}

#[test]
fn duplicates_keep_highest_version() {
    let c = MockClient::new();
    c.set(REPO, &manifest(&[LIST1, LIST2]));
    c.set(LIST1, &format!("[{}]", entry("Dup", 3)));
    c.set(LIST2, &format!("[{},{}]", entry("Dup", 7), entry("Other", 1)));
    let r = import_repository(&c, REPO).unwrap();
    assert_eq!(r.plugins.len(), 2);
    assert_eq!(r.plugins.iter().find(|p| p.internal_name == "Dup").unwrap().version, Some(7));
}

#[test]
fn invalid_and_insecure_urls_make_no_request() {
    let c = MockClient::new();
    for bad in ["not a url", "http://example.com/repo.json", "ftp://x.com/a", "https://user:pw@example.com/r.json"] {
        let err = import_repository(&c, bad).unwrap_err();
        assert!(matches!(err, RepoError::Net(_)), "{bad}");
    }
    assert!(c.requested().is_empty());
}

#[test]
fn non_https_plugin_list_skipped_with_warning() {
    let c = MockClient::new();
    c.set(REPO, &manifest(&[LIST1, "http://insecure.example.com/p.json"]));
    c.set(LIST1, &format!("[{}]", entry("One", 1)));
    let r = import_repository(&c, REPO).unwrap();
    assert_eq!(r.plugins.len(), 1);
    assert!(!r.warnings.is_empty());
}

#[test]
fn one_failing_list_does_not_fail_import() {
    let c = MockClient::new();
    c.set(REPO, &manifest(&[LIST1, LIST2]));
    c.set(LIST1, &format!("[{}]", entry("One", 1)));
    let r = import_repository(&c, REPO).unwrap();
    assert_eq!(r.plugins.len(), 1);
    assert_eq!(r.warnings.len(), 1);
}

#[test]
fn all_lists_failing_errors() {
    let c = MockClient::new();
    c.set(REPO, &manifest(&[LIST1]));
    assert!(matches!(import_repository(&c, REPO).unwrap_err(), RepoError::NoPluginsFound(_)));
}

#[test]
fn failed_manifest_request() {
    let c = MockClient::new();
    c.set_err(REPO, NetError::Offline);
    assert_eq!(import_repository(&c, REPO).unwrap_err(), RepoError::Net(NetError::Offline));
}

#[test]
fn empty_repositories() {
    let c = MockClient::new();
    c.set(REPO, &manifest(&[]));
    assert_eq!(import_repository(&c, REPO).unwrap_err(), RepoError::NoPluginLists);
    c.set(REPO, &manifest(&[LIST1]));
    c.set(LIST1, "[]");
    assert_eq!(import_repository(&c, REPO).unwrap().plugins.len(), 0);
}

#[test]
fn refresh_reflects_new_data() {
    let c = MockClient::new();
    c.set(REPO, &manifest(&[LIST1]));
    c.set(LIST1, &format!("[{}]", entry("One", 1)));
    let first = import_repository(&c, REPO).unwrap();
    c.set(LIST1, &format!("[{}]", entry("One", 2)));
    let second = import_repository(&c, REPO).unwrap();
    assert_eq!(first.plugins[0].version, Some(1));
    assert_eq!(second.plugins[0].version, Some(2));
}

#[test]
fn packed_types_authors_and_icon_validation() {
    let (e, _) = decode_plugin_list(br#"[{"url":"https://e.com/a.cs3","name":"X","authors":["A,B"],"tvTypes":["Movie,Anime,Cartoon"]}]"#).unwrap();
    assert_eq!(e[0].media_types().len(), 3);
    assert_eq!(e[0].display_authors(), ["A", "B"]);

    let mut p = PluginEntry::new("https://e.com/a", "X");
    p.icon_url = Some("https://e.com/i.png?sz=%size%".into());
    assert_eq!(p.safe_icon_url().as_deref(), Some("https://e.com/i.png?sz=128"));
    p.icon_url = Some("http://e.com/i.png".into());
    assert!(p.safe_icon_url().is_none());
    p.icon_url = Some("javascript:alert(1)".into());
    assert!(p.safe_icon_url().is_none());
}
