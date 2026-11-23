//! JSON method dispatcher used by the FFI layer. The WinUI front-end only talks to this.

use crate::archive::{self, InternetArchiveProvider};
use crate::compat::{self, evaluate};
use crate::custom::{self, CustomDefinition, CustomProvider};
use crate::dailymotion::DailymotionProvider;
use crate::http::HttpClient;
use crate::models::{MediaEpisode, MediaItem};
use crate::peertube::{self, PeerTubeProvider};
use crate::web::WebProvider;
use crate::youtube::{KeyHandle, YouTubeProvider};
use crate::provider::{MediaProvider, ProviderRegistry};
use crate::repo::{self, PHISHER_NAME, PHISHER_REPO_URL, PHISHER_SOURCE_URL};
use crate::store::Store;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

pub struct Context {
    pub client: Arc<dyn HttpClient>,
    pub registry: ProviderRegistry,
    pub store: Store,
    pub native: Vec<Arc<dyn MediaProvider>>,
    pub mappings: HashMap<String, String>,
    pub youtube_key: KeyHandle,
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key).and_then(|v| v.as_str()).ok_or_else(|| format!("Missing argument: {key}"))
}

fn parse<T: serde::de::DeserializeOwned>(args: &Value, key: &str) -> Result<T, String> {
    serde_json::from_value(args.get(key).cloned().unwrap_or(Value::Null)).map_err(|e| format!("Bad argument {key}: {e}"))
}

impl Context {
    pub fn new(client: Arc<dyn HttpClient>, store: Store) -> Self {
        let key: KeyHandle = Arc::new(std::sync::RwLock::new(
            store.read(|d| d.settings.get("youtubeApiKey").and_then(|v| v.as_str()).unwrap_or("").to_string()),
        ));
        let mut native: Vec<Arc<dyn MediaProvider>> = vec![
            Arc::new(WebProvider::new(client.clone())),
            Arc::new(InternetArchiveProvider::new(client.clone())),
            Arc::new(YouTubeProvider::new(client.clone(), key.clone())),
            Arc::new(DailymotionProvider::new(client.clone())),
        ];
        if let Ok(p) = PeerTubeProvider::new(client.clone(), peertube::ID, "PeerTube", peertube::DEFAULT_INSTANCE) {
            native.push(Arc::new(p));
        }
        let ctx = Self {
            client,
            registry: ProviderRegistry::new(),
            store,
            native,
            mappings: compat::default_native_mappings(),
            youtube_key: key,
        };
        ctx.sync_registry();
        ctx
    }

    /// Rebuilds the registry from persisted installs.
    pub fn sync_registry(&self) {
        let installs = self.store.read(|d| d.installs.clone());
        for p in &self.native {
            let id = p.descriptor().id.clone();
            if installs.contains(&id) {
                self.registry.register(p.clone());
            } else {
                self.registry.unregister(&id);
            }
        }
        // User-defined providers are enabled as soon as they exist.
        let defs = self.store.read(|d| d.custom_providers.clone());
        let live: Vec<String> = defs.iter().map(|d| d.id.clone()).collect();
        for id in self.registry.ids() {
            if id.starts_with("custom.") && !live.contains(&id) {
                self.registry.unregister(&id);
            }
        }
        for def in defs {
            if let Ok(p) = CustomProvider::new(def, self.client.clone()) {
                self.registry.register(Arc::new(p));
            }
        }
    }

    fn provider(&self, args: &Value) -> Result<Arc<dyn MediaProvider>, String> {
        let id = str_arg(args, "providerId")?;
        self.registry.get(id).ok_or_else(|| "This provider is not enabled. Enable it in Extensions.".to_string())
    }

    fn import_repo(&self, url: &str) -> Result<(), String> {
        let imp = repo::import_repository(&*self.client, url).map_err(|e| e.to_string())?;
        let repo_url = imp.repository_url.clone();
        self.store.with(|d| d.upsert_repo(&imp));
        self.update_from_repo(&repo_url);
        Ok(())
    }

    /// Installs one entry that carries a declarative definition. The id is derived from the entry's
    /// internal name, so installing again (or refreshing) replaces the same provider.
    fn install_definition(&self, repo_url: &str, plugin: &repo::PluginEntry) -> Result<String, String> {
        let mut def = plugin.definition.clone().ok_or("This entry has no native definition, so it cannot be installed on Windows.")?;
        def.id = format!("custom.{}", custom::slug(&plugin.internal_name));
        if def.name.trim().is_empty() {
            def.name = plugin.name.clone();
        }
        def.source = repo_url.to_string();
        def.version = plugin.version.unwrap_or(1);
        def.validate()?;
        let id = def.id.clone();
        self.store.with(|d| match d.custom_providers.iter_mut().find(|c| c.id == id) {
            Some(existing) => *existing = def.clone(),
            None => d.custom_providers.push(def.clone()),
        });
        self.sync_registry();
        Ok(id)
    }

    /// After a repository refresh, bring providers installed from it up to date.
    fn update_from_repo(&self, repo_url: &str) {
        let plugins: Vec<repo::PluginEntry> =
            self.store.read(|d| d.repositories.iter().find(|r| r.url == repo_url).map(|r| r.plugins.clone()).unwrap_or_default());
        let installed: Vec<(String, i64)> = self.store.read(|d| {
            d.custom_providers.iter().filter(|c| c.source == repo_url).map(|c| (c.id.clone(), c.version)).collect()
        });
        for (id, version) in installed {
            if let Some(p) = plugins.iter().find(|p| p.definition.is_some() && format!("custom.{}", custom::slug(&p.internal_name)) == id) {
                if p.version.unwrap_or(1) > version {
                    let _ = self.install_definition(repo_url, p);
                }
            }
        }
    }

    pub fn snapshot(&self) -> Value {
        let installs = self.store.read(|d| d.installs.clone());
        let providers: Vec<Value> = self
            .native
            .iter()
            .map(|p| json!({ "descriptor": p.descriptor(), "installed": installs.contains(&p.descriptor().id), "custom": false }))
            .collect();
        let custom_defs = self.store.read(|d| d.custom_providers.clone());
        let mut providers = providers;
        for def in custom_defs {
            if let Ok(p) = CustomProvider::new(def.clone(), self.client.clone()) {
                providers.push(json!({ "descriptor": p.descriptor(), "installed": true, "custom": true, "definition": def }));
            }
        }
        self.store.read(|d| {
            let repositories: Vec<Value> = d
                .repositories
                .iter()
                .map(|r| {
                    let plugins: Vec<Value> = r
                        .plugins
                        .iter()
                        .map(|p| {
                            let pid = format!("custom.{}", custom::slug(&p.internal_name));
                            let have = d.custom_providers.iter().find(|c| c.id == pid && c.source == r.url);
                            json!({
                                "plugin": p,
                                "status": evaluate(p, &self.mappings).to_json(),
                                "installed": have.is_some(),
                                "updateAvailable": have.map_or(false, |c| p.version.unwrap_or(1) > c.version),
                            })
                        })
                        .collect();
                    json!({
                        "url": r.url, "name": r.name, "description": r.description, "iconUrl": r.icon_url,
                        "lastRefreshed": r.last_refreshed, "lastError": r.last_error,
                        "warnings": r.warnings, "plugins": plugins,
                    })
                })
                .collect();
            json!({
                "bookmarks": d.bookmarks,
                "progress": d.progress,
                "searchHistory": d.search_history.iter().map(|h| h.query.clone()).collect::<Vec<_>>(),
                "repositories": repositories,
                "providers": providers,
                "settings": d.settings,
                "presets": { "phisherName": PHISHER_NAME, "phisherRepoUrl": PHISHER_REPO_URL, "phisherSourceUrl": PHISHER_SOURCE_URL },
            })
        })
    }

    pub fn call(&self, method: &str, args: &Value) -> Result<Value, String> {
        match method {
            "snapshot" => Ok(self.snapshot()),

            // ---- repositories (import only loads metadata; installing is separate) ----
            "repo.import" => {
                self.import_repo(str_arg(args, "url")?)?;
                Ok(Value::Null)
            }
            "repo.refresh" => {
                let url = str_arg(args, "url")?;
                match self.import_repo(url) {
                    Ok(()) => Ok(Value::Null),
                    Err(e) => {
                        // Keep the cached catalog; just record why the refresh failed.
                        self.store.with(|d| {
                            if let Some(r) = d.repositories.iter_mut().find(|r| r.url == url) {
                                r.last_error = Some(e.clone());
                            }
                        });
                        Err(e)
                    }
                }
            }
            "repo.remove" => {
                let url = str_arg(args, "url")?;
                self.store.with(|d| d.repositories.retain(|r| r.url != url));
                Ok(Value::Null)
            }

            // ---- native providers ----
            "provider.install" => {
                let id = str_arg(args, "id")?.to_string();
                if !self.native.iter().any(|p| p.descriptor().id == id) {
                    return Err("Unknown provider.".into());
                }
                self.store.with(|d| {
                    if !d.installs.contains(&id) {
                        d.installs.push(id.clone());
                    }
                });
                self.sync_registry();
                Ok(Value::Null)
            }
            "provider.uninstall" => {
                let id = str_arg(args, "id")?.to_string();
                self.store.with(|d| d.installs.retain(|i| *i != id));
                self.sync_registry();
                Ok(Value::Null)
            }

            // ---- repository entries that carry a native definition ----
            "plugin.install" => {
                let repo_url = str_arg(args, "repoUrl")?;
                let name = str_arg(args, "internalName")?;
                let plugin = self
                    .store
                    .read(|d| d.repositories.iter().find(|r| r.url == repo_url).and_then(|r| r.plugins.iter().find(|p| p.internal_name == name).cloned()))
                    .ok_or("That plugin is no longer in the repository.")?;
                Ok(json!(self.install_definition(repo_url, &plugin)?))
            }
            "plugin.installAll" => {
                let repo_url = str_arg(args, "repoUrl")?;
                let plugins: Vec<repo::PluginEntry> = self
                    .store
                    .read(|d| d.repositories.iter().find(|r| r.url == repo_url).map(|r| r.plugins.clone()))
                    .ok_or("Repository not found.")?;
                let (mut done, mut failed) = (0, vec![]);
                for p in plugins.iter().filter(|p| p.definition.is_some()) {
                    match self.install_definition(repo_url, p) {
                        Ok(_) => done += 1,
                        Err(e) => failed.push(format!("{}: {e}", p.name)),
                    }
                }
                Ok(json!({ "installed": done, "failed": failed }))
            }
            "repo.export" => {
                let dir = std::path::PathBuf::from(str_arg(args, "dir")?);
                let defs = self.store.read(|d| d.custom_providers.clone());
                if defs.is_empty() {
                    return Err("You have no custom providers to export yet.".into());
                }
                let (repo_json, list_json) = build_export(&defs);
                std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create the folder: {e}"))?;
                std::fs::write(dir.join("repo.json"), serde_json::to_vec_pretty(&repo_json).unwrap_or_default()).map_err(|e| e.to_string())?;
                std::fs::write(dir.join("providers.json"), serde_json::to_vec_pretty(&list_json).unwrap_or_default()).map_err(|e| e.to_string())?;
                Ok(json!(dir.to_string_lossy()))
            }

            // ---- user-defined providers ----
            "provider.detect" => {
                let found = custom::detect(self.client.clone(), str_arg(args, "url")?).map_err(|e| e.to_string())?;
                Ok(json!(found))
            }
            "provider.test" => {
                let def: CustomDefinition = parse(args, "definition")?;
                let p = CustomProvider::new(def, self.client.clone()).map_err(|e| e.to_string())?;
                let items: Vec<MediaItem> = match p.home_catalogs() {
                    Ok(cats) => cats.into_iter().flat_map(|c| c.items).collect(),
                    Err(e) => return Err(e.to_string()),
                };
                let sample: Vec<String> = items.iter().take(5).map(|i| i.title.clone()).collect();
                Ok(json!({ "count": items.len(), "sample": sample }))
            }
            "provider.save" => {
                let mut def: CustomDefinition = parse(args, "definition")?;
                def.validate()?;
                if def.id.trim().is_empty() {
                    let base = format!("custom.{}", custom::slug(&def.name));
                    let taken = self.store.read(|d| d.custom_providers.iter().map(|c| c.id.clone()).collect::<Vec<_>>());
                    let mut id = base.clone();
                    let mut n = 2;
                    while taken.contains(&id) {
                        id = format!("{base}-{n}");
                        n += 1;
                    }
                    def.id = id;
                } else if !def.id.starts_with("custom.") {
                    return Err("Invalid provider id.".into());
                }
                self.store.with(|d| match d.custom_providers.iter_mut().find(|c| c.id == def.id) {
                    Some(existing) => *existing = def.clone(),
                    None => d.custom_providers.push(def.clone()),
                });
                self.sync_registry();
                Ok(json!(def.id))
            }
            "provider.delete" => {
                let id = str_arg(args, "id")?.to_string();
                self.store.with(|d| d.custom_providers.retain(|c| c.id != id));
                self.sync_registry();
                Ok(Value::Null)
            }

            // ---- discovery ----
            "home" => {
                let providers = self.registry.all();
                let groups: Vec<Value> = std::thread::scope(|s| {
                    let handles: Vec<_> = providers.iter().map(|p| s.spawn(move || p.home_catalogs().unwrap_or_default())).collect();
                    handles
                        .into_iter()
                        .zip(providers.iter())
                        .map(|(h, p)| json!({ "provider": p.descriptor(), "catalogs": h.join().unwrap_or_default() }))
                        .collect()
                });
                Ok(Value::Array(groups))
            }
            "search" => {
                let query = str_arg(args, "query")?.trim().to_string();
                if query.is_empty() {
                    return Ok(json!([]));
                }
                let only = args.get("providerId").and_then(|v| v.as_str());
                self.store.with(|d| d.add_search(&query));
                let groups: Vec<Value> = self
                    .registry
                    .search_all(&query, 1)
                    .into_iter()
                    .filter(|(d, _)| only.map_or(true, |o| d.id == o))
                    .map(|(d, r)| match r {
                        Ok(items) => json!({ "provider": d, "items": items, "error": null }),
                        Err(e) => json!({ "provider": d, "items": [], "error": e.to_string() }),
                    })
                    .collect();
                Ok(Value::Array(groups))
            }
            "details" => {
                let p = self.provider(args)?;
                let d = p.load_details(str_arg(args, "id")?).map_err(|e| e.to_string())?;
                Ok(json!(d))
            }
            "episodes" => {
                let p = self.provider(args)?;
                let season = args.get("season").and_then(|v| v.as_i64()).map(|s| s as i32);
                let eps = p.load_episodes(str_arg(args, "mediaId")?, season).map_err(|e| e.to_string())?;
                Ok(json!(eps))
            }
            "sources" => {
                let p = self.provider(args)?;
                let ep: Option<MediaEpisode> = match args.get("episode") {
                    None | Some(Value::Null) => None,
                    Some(_) => Some(parse(args, "episode")?),
                };
                let s = p.resolve_sources(str_arg(args, "mediaId")?, ep.as_ref()).map_err(|e| e.to_string())?;
                Ok(json!(s))
            }

            // ---- library ----
            "bookmark.toggle" => {
                let item: MediaItem = parse(args, "item")?;
                Ok(json!(self.store.with(|d| d.toggle_bookmark(&item))))
            }
            "favorite.toggle" => {
                let item: MediaItem = parse(args, "item")?;
                self.store.with(|d| d.toggle_favorite(&item));
                Ok(Value::Null)
            }
            "bookmark.remove" => {
                let key = str_arg(args, "key")?;
                self.store.with(|d| d.bookmarks.retain(|b| b.item.key() != key));
                Ok(Value::Null)
            }
            "progress.save" => {
                let item: MediaItem = parse(args, "item")?;
                let episode: Option<MediaEpisode> = match args.get("episode") {
                    None | Some(Value::Null) => None,
                    Some(_) => Some(parse(args, "episode")?),
                };
                let position = args.get("position").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let duration = args.get("duration").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let completed = args.get("completed").and_then(|v| v.as_bool()).unwrap_or(false);
                self.store.with(|d| d.record_progress(&item, episode.as_ref(), position, duration, completed));
                Ok(Value::Null)
            }
            "history.remove" => {
                let key = str_arg(args, "key")?;
                self.store.with(|d| d.progress.retain(|p| p.key != key));
                Ok(Value::Null)
            }
            "history.clear" => {
                self.store.with(|d| d.progress.clear());
                Ok(Value::Null)
            }
            "search.clear" => {
                self.store.with(|d| d.search_history.clear());
                Ok(Value::Null)
            }

            // ---- settings / privacy ----
            "settings.set" => {
                let key = str_arg(args, "key")?.to_string();
                let value = args.get("value").cloned().unwrap_or(Value::Null);
                if key == "youtubeApiKey" {
                    *self.youtube_key.write().unwrap() = value.as_str().unwrap_or("").trim().to_string();
                }
                self.store.with(|d| {
                    d.settings.insert(key, value);
                });
                Ok(Value::Null)
            }
            "cache.clear" => {
                self.store.with(|d| {
                    for r in d.repositories.iter_mut() {
                        r.plugins.clear();
                        r.warnings.clear();
                        r.last_refreshed = None;
                    }
                });
                Ok(Value::Null)
            }
            "data.erase" => {
                self.store.with(|d| {
                    d.bookmarks.clear();
                    d.progress.clear();
                    d.search_history.clear();
                    d.repositories.clear();
                    d.custom_providers.clear();
                    d.settings.remove("youtubeApiKey");
                });
                *self.youtube_key.write().unwrap() = String::new();
                self.sync_registry();
                Ok(Value::Null)
            }
            other => Err(format!("Unknown method: {other}")),
        }
    }
}

pub use archive::ID as INTERNET_ARCHIVE_ID;

/// Builds an importable repository (repo.json + providers.json) from the user's own providers.
/// The list is referenced relatively, so the two files can be hosted together at any https address.
pub fn build_export(defs: &[CustomDefinition]) -> (Value, Value) {
    let entries: Vec<Value> = defs
        .iter()
        .map(|def| {
            let mut portable = def.clone();
            portable.id = String::new();
            portable.source = String::new();
            portable.version = 0;
            let internal = def.id.strip_prefix("custom.").unwrap_or(&def.id).to_string();
            json!({
                "url": def.url,
                "name": def.name,
                "internalName": internal,
                "status": 1,
                "version": def.version.max(1),
                "description": format!("{} provider for {}", def.kind, def.url),
                "authors": ["You"],
                "tvTypes": ["Video"],
                "definition": portable,
            })
        })
        .collect();
    let repo = json!({
        "name": "My CloudStream providers",
        "description": "Declarative providers exported from CloudStream for Windows.",
        "manifestVersion": 1,
        "pluginLists": ["providers.json"],
    });
    (repo, Value::Array(entries))
}
