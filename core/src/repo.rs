use crate::http::{require_secure, secure_url, HttpClient, NetError};
use crate::models::{now_secs, MediaType};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fmt;

pub const PHISHER_NAME: &str = "Phisher Repo";
pub const PHISHER_REPO_URL: &str =
    "https://raw.githubusercontent.com/phisher98/cloudstream-extensions-phisher/refs/heads/builds/repo.json";
pub const PHISHER_SOURCE_URL: &str = "https://github.com/phisher98/cloudstream-extensions-phisher";

#[derive(Debug, Clone, PartialEq)]
pub enum RepoError {
    Net(NetError),
    InvalidManifest,
    NoPluginLists,
    NoPluginsFound(Vec<String>),
}

impl fmt::Display for RepoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Net(e) => write!(f, "{e}"),
            Self::InvalidManifest => {
                write!(f, "This address did not return a valid CloudStream repository manifest (repo.json).")
            }
            Self::NoPluginLists => write!(f, "The repository manifest does not list any plugin lists."),
            Self::NoPluginsFound(r) => write!(f, "None of the plugin lists could be loaded. {}", r.join(" ")),
        }
    }
}

impl std::error::Error for RepoError {}

impl From<NetError> for RepoError {
    fn from(e: NetError) -> Self {
        Self::Net(e)
    }
}

/// `repo.json` — a repository manifest. It does NOT contain providers; it points at plugin lists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryManifest {
    pub name: String,
    pub description: Option<String>,
    pub icon_url: Option<String>,
    pub manifest_version: Option<i64>,
    pub plugin_lists: Vec<String>,
}

/// One entry of a `plugins.json` list. All values are untrusted data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginEntry {
    pub url: String,
    pub name: String,
    pub internal_name: String,
    pub status: Option<i64>,
    pub version: Option<i64>,
    #[serde(default)]
    pub authors: Vec<String>,
    pub description: Option<String>,
    pub file_size: Option<i64>,
    pub repository_url: Option<String>,
    pub language: Option<String>,
    #[serde(default)]
    pub tv_types: Vec<String>,
    pub icon_url: Option<String>,
    pub api_version: Option<i64>,
    pub file_hash: Option<String>,
}

impl PluginEntry {
    pub fn new(url: &str, name: &str) -> Self {
        Self {
            url: url.into(),
            name: name.into(),
            internal_name: name.into(),
            status: Some(1),
            version: None,
            authors: vec![],
            description: None,
            file_size: None,
            repository_url: None,
            language: None,
            tv_types: vec![],
            icon_url: None,
            api_version: None,
            file_hash: None,
        }
    }

    /// Some entries pack several values into one string ("Movie,Anime").
    pub fn media_types(&self) -> Vec<MediaType> {
        let mut out: Vec<MediaType> = vec![];
        for raw in &self.tv_types {
            for part in raw.split(',') {
                let t = MediaType::from_cs(part);
                if !out.contains(&t) {
                    out.push(t);
                }
            }
        }
        out
    }

    pub fn display_authors(&self) -> Vec<String> {
        self.authors
            .iter()
            .flat_map(|a| a.split(',').map(|s| s.trim().to_string()).collect::<Vec<_>>())
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Replaces the `%size%` placeholder some favicon URLs use, then requires https.
    pub fn safe_icon_url(&self) -> Option<String> {
        secure_url(&self.icon_url.as_deref()?.replace("%size%", "128"))
    }

    /// CloudStream status: 0 down, 1 ok, 2 slow, 3 beta-only.
    pub fn status_label(&self) -> &'static str {
        match self.status {
            Some(0) => "Reported down",
            Some(2) => "Reported slow",
            Some(3) => "Beta",
            _ => "Reported OK",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RepositoryImport {
    pub repository_url: String,
    pub manifest: RepositoryManifest,
    pub plugins: Vec<PluginEntry>,
    pub warnings: Vec<String>,
    pub fetched_at: i64,
}

fn get_str(o: &Map<String, Value>, k: &str) -> Option<String> {
    o.get(k)?.as_str().map(String::from)
}
fn get_int(o: &Map<String, Value>, k: &str) -> Option<i64> {
    o.get(k)?.as_i64()
}
fn get_strs(o: &Map<String, Value>, k: &str) -> Vec<String> {
    o.get(k)
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

pub fn decode_manifest(data: &[u8]) -> Result<RepositoryManifest, RepoError> {
    let v: Value = serde_json::from_slice(data).map_err(|_| RepoError::InvalidManifest)?;
    let o = v.as_object().ok_or(RepoError::InvalidManifest)?; // a plugin list (array) is not a manifest
    Ok(RepositoryManifest {
        name: get_str(o, "name").unwrap_or_else(|| "Untitled Repository".into()),
        description: get_str(o, "description"),
        icon_url: get_str(o, "iconUrl"),
        manifest_version: get_int(o, "manifestVersion"),
        plugin_lists: get_strs(o, "pluginLists"),
    })
}

fn entry_from_value(v: &Value) -> Option<PluginEntry> {
    let o = v.as_object()?;
    let url = get_str(o, "url")?;
    let name = get_str(o, "name")?;
    Some(PluginEntry {
        internal_name: get_str(o, "internalName").unwrap_or_else(|| name.clone()),
        url,
        name,
        status: get_int(o, "status"),
        version: get_int(o, "version"),
        authors: get_strs(o, "authors"),
        description: get_str(o, "description"),
        file_size: get_int(o, "fileSize"),
        repository_url: get_str(o, "repositoryUrl"),
        language: get_str(o, "language"),
        tv_types: get_strs(o, "tvTypes"),
        icon_url: get_str(o, "iconUrl"),
        api_version: get_int(o, "apiVersion"),
        file_hash: get_str(o, "fileHash"),
    })
}

/// Returns decoded entries and the number skipped as malformed (one bad entry never fails the list).
pub fn decode_plugin_list(data: &[u8]) -> Result<(Vec<PluginEntry>, usize), RepoError> {
    let v: Value = serde_json::from_slice(data).map_err(|_| RepoError::InvalidManifest)?;
    let arr = v.as_array().ok_or(RepoError::InvalidManifest)?;
    let entries: Vec<PluginEntry> = arr.iter().filter_map(entry_from_value).collect();
    let skipped = arr.len() - entries.len();
    Ok((entries, skipped))
}

/// De-duplicates by `internal_name`, keeping the highest version (first wins on ties), in first-seen order.
pub fn merge(entries: Vec<PluginEntry>) -> Vec<PluginEntry> {
    let mut order: Vec<String> = vec![];
    let mut best: HashMap<String, PluginEntry> = HashMap::new();
    for e in entries {
        match best.get(&e.internal_name) {
            Some(existing) => {
                if e.version.unwrap_or(0) > existing.version.unwrap_or(0) {
                    best.insert(e.internal_name.clone(), e);
                }
            }
            None => {
                order.push(e.internal_name.clone());
                best.insert(e.internal_name.clone(), e);
            }
        }
    }
    order.into_iter().filter_map(|k| best.remove(&k)).collect()
}

/// repo.json → every `pluginLists` URL → merged catalog. Data only; nothing is executed.
pub fn import_repository(client: &dyn HttpClient, url: &str) -> Result<RepositoryImport, RepoError> {
    let repo_url = require_secure(url)?;
    let manifest = decode_manifest(&client.get(&repo_url)?)?;
    if manifest.plugin_lists.is_empty() {
        return Err(RepoError::NoPluginLists);
    }

    let mut warnings: Vec<String> = vec![];
    let mut list_urls: Vec<String> = vec![];
    for raw in &manifest.plugin_lists {
        match secure_url(raw) {
            Some(u) => list_urls.push(u),
            None => warnings.push(format!("Skipped plugin list with an invalid or non-HTTPS URL: {raw}")),
        }
    }

    // Fetch lists concurrently; one failing list must not fail the others.
    let results: Vec<Result<(Vec<PluginEntry>, usize), String>> = std::thread::scope(|s| {
        let handles: Vec<_> = list_urls
            .iter()
            .map(|u| {
                s.spawn(move || -> Result<(Vec<PluginEntry>, usize), String> {
                    let data = client.get(u).map_err(|e| e.to_string())?;
                    decode_plugin_list(&data).map_err(|e| e.to_string())
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|_| Err("Worker panicked.".to_string())))
            .collect()
    });

    let mut merged: Vec<PluginEntry> = vec![];
    let mut failures: Vec<String> = vec![];
    for (i, r) in results.into_iter().enumerate() {
        match r {
            Ok((entries, skipped)) => {
                merged.extend(entries);
                if skipped > 0 {
                    warnings.push(format!(
                        "{skipped} malformed entr{} skipped in list {}.",
                        if skipped == 1 { "y" } else { "ies" },
                        i + 1
                    ));
                }
            }
            Err(e) => {
                let msg = format!("List {}: {e}", i + 1);
                failures.push(msg.clone());
                warnings.push(msg);
            }
        }
    }
    if merged.is_empty() && !failures.is_empty() {
        return Err(RepoError::NoPluginsFound(failures));
    }

    Ok(RepositoryImport {
        repository_url: repo_url,
        manifest,
        plugins: merge(merged),
        warnings,
        fetched_at: now_secs(),
    })
}
