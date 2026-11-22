//! JSON method dispatcher used by the FFI layer. The WinUI front-end only talks to this.

use crate::archive::{self, InternetArchiveProvider};
use crate::compat::{self, evaluate};
use crate::http::HttpClient;
use crate::models::{MediaEpisode, MediaItem};
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
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key).and_then(|v| v.as_str()).ok_or_else(|| format!("Missing argument: {key}"))
}

fn parse<T: serde::de::DeserializeOwned>(args: &Value, key: &str) -> Result<T, String> {
    serde_json::from_value(args.get(key).cloned().unwrap_or(Value::Null)).map_err(|e| format!("Bad argument {key}: {e}"))
}

impl Context {
    pub fn new(client: Arc<dyn HttpClient>, store: Store) -> Self {
        let native: Vec<Arc<dyn MediaProvider>> = vec![Arc::new(InternetArchiveProvider::new(client.clone()))];
        let ctx = Self {
            client,
            registry: ProviderRegistry::new(),
            store,
            native,
            mappings: compat::default_native_mappings(),
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
    }

    fn provider(&self, args: &Value) -> Result<Arc<dyn MediaProvider>, String> {
        let id = str_arg(args, "providerId")?;
        self.registry.get(id).ok_or_else(|| "This provider is not enabled. Enable it in Extensions.".to_string())
    }

    fn import_repo(&self, url: &str) -> Result<(), String> {
        let imp = repo::import_repository(&*self.client, url).map_err(|e| e.to_string())?;
        self.store.with(|d| d.upsert_repo(&imp));
        Ok(())
    }

    pub fn snapshot(&self) -> Value {
        let installs = self.store.read(|d| d.installs.clone());
        let providers: Vec<Value> = self
            .native
            .iter()
            .map(|p| json!({ "descriptor": p.descriptor(), "installed": installs.contains(&p.descriptor().id) }))
            .collect();
        self.store.read(|d| {
            let repositories: Vec<Value> = d
                .repositories
                .iter()
                .map(|r| {
                    let plugins: Vec<Value> = r
                        .plugins
                        .iter()
                        .map(|p| json!({ "plugin": p, "status": evaluate(p, &self.mappings).to_json() }))
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
                });
                Ok(Value::Null)
            }
            other => Err(format!("Unknown method: {other}")),
        }
    }
}

pub use archive::ID as INTERNET_ARCHIVE_ID;
