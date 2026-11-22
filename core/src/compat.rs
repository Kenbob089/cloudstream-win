use crate::models::MediaType;
use crate::repo::PluginEntry;
use serde_json::{json, Value};
use std::collections::HashMap;

/// What the app can honestly say about a catalog entry on Windows.
#[derive(Debug, Clone, PartialEq)]
pub enum CompatibilityStatus {
    /// A reviewed, compiled native implementation ships inside the app.
    NativeAvailable { provider_id: String },
    /// Listed in the repository, but the Android `.cs3` plugin cannot run here and no native adapter exists.
    MetadataOnly,
    NotSupported { reason: String },
}

impl CompatibilityStatus {
    pub fn is_installable(&self) -> bool {
        matches!(self, Self::NativeAvailable { .. })
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::NativeAvailable { .. } => "Compatible with Windows",
            Self::MetadataOnly => "Metadata Only",
            Self::NotSupported { .. } => "Not Supported on Windows",
        }
    }

    pub fn explanation(&self) -> String {
        match self {
            Self::NativeAvailable { .. } => "A reviewed native implementation is included in this app.".into(),
            Self::MetadataOnly => "This entry is published as an Android .cs3 plugin. Android plugin code cannot run in this app and this app never executes downloaded code. No reviewed native implementation exists for it yet, so it cannot search or play anything.".into(),
            Self::NotSupported { reason } => reason.clone(),
        }
    }

    pub fn to_json(&self) -> Value {
        let (kind, provider_id, reason) = match self {
            Self::NativeAvailable { provider_id } => ("nativeAvailable", Some(provider_id.clone()), None),
            Self::MetadataOnly => ("metadataOnly", None, None),
            Self::NotSupported { reason } => ("notSupported", None, Some(reason.clone())),
        };
        json!({
            "kind": kind,
            "label": self.label(),
            "explanation": self.explanation(),
            "installable": self.is_installable(),
            "providerId": provider_id,
            "reason": reason,
        })
    }
}

/// Maps repository entries (by `internal_name`) to native providers compiled into the app.
/// Adding a mapping requires a reviewed Rust adapter in this crate — never remote code.
/// It ships empty on purpose: no Phisher entry has a reviewed native adapter.
pub fn default_native_mappings() -> HashMap<String, String> {
    HashMap::new()
}

pub fn evaluate(plugin: &PluginEntry, mappings: &HashMap<String, String>) -> CompatibilityStatus {
    if let Some(id) = mappings.get(&plugin.internal_name) {
        return CompatibilityStatus::NativeAvailable { provider_id: id.clone() };
    }
    if plugin.status == Some(0) {
        return CompatibilityStatus::NotSupported {
            reason: "The repository reports this plugin as down, and it has no native implementation.".into(),
        };
    }
    let types = plugin.media_types();
    if !types.is_empty() && types.iter().all(|t| *t == MediaType::Torrent) {
        return CompatibilityStatus::NotSupported { reason: "Torrent-only sources are not supported.".into() };
    }
    CompatibilityStatus::MetadataOnly
}
