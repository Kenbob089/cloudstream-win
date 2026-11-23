use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MediaType {
    Movie,
    TvSeries,
    Anime,
    AnimeMovie,
    Ova,
    Cartoon,
    AsianDrama,
    Documentary,
    Live,
    Music,
    AudioBook,
    Torrent,
    Video,
    Other,
}

impl MediaType {
    /// Maps a CloudStream `tvTypes` string. Unknown values become `Other`.
    pub fn from_cs(raw: &str) -> Self {
        match raw.trim().to_lowercase().as_str() {
            "movie" => Self::Movie,
            "tvseries" => Self::TvSeries,
            "anime" => Self::Anime,
            "animemovie" => Self::AnimeMovie,
            "ova" => Self::Ova,
            "cartoon" => Self::Cartoon,
            "asiandrama" => Self::AsianDrama,
            "documentary" => Self::Documentary,
            "live" => Self::Live,
            "music" => Self::Music,
            "audiobook" => Self::AudioBook,
            "torrent" => Self::Torrent,
            "video" => Self::Video,
            _ => Self::Other,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Movie => "Movie",
            Self::TvSeries => "TV Series",
            Self::Anime => "Anime",
            Self::AnimeMovie => "Anime Movie",
            Self::Ova => "OVA",
            Self::Cartoon => "Cartoon",
            Self::AsianDrama => "Asian Drama",
            Self::Documentary => "Documentary",
            Self::Live => "Live",
            Self::Music => "Music",
            Self::AudioBook => "Audiobook",
            Self::Torrent => "Torrent",
            Self::Video => "Video",
            Self::Other => "Other",
        }
    }
}

/// Media as returned by a provider. `id` is unique only within a provider; `key()` is global.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaItem {
    pub id: String,
    pub provider_id: String,
    pub title: String,
    #[serde(default)]
    pub original_title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub poster_url: Option<String>,
    #[serde(default)]
    pub backdrop_url: Option<String>,
    #[serde(default)]
    pub release_year: Option<i32>,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default = "default_media_type")]
    pub media_type: MediaType,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub rating: Option<f64>,
}

fn default_media_type() -> MediaType {
    MediaType::Movie
}

impl MediaItem {
    pub fn new(id: &str, provider_id: &str, title: &str) -> Self {
        Self {
            id: id.into(),
            provider_id: provider_id.into(),
            title: title.into(),
            original_title: None,
            description: None,
            poster_url: None,
            backdrop_url: None,
            release_year: None,
            genres: vec![],
            media_type: MediaType::Movie,
            duration: None,
            rating: None,
        }
    }

    pub fn key(&self) -> String {
        format!("{}::{}", self.provider_id, self.id)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaDetails {
    pub item: MediaItem,
    #[serde(default)]
    pub seasons: Vec<i32>,
    #[serde(default)]
    pub languages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaEpisode {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub episode_number: Option<i32>,
    #[serde(default)]
    pub season_number: Option<i32>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    #[serde(default)]
    pub duration: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub id: String,
    pub title: String,
    pub items: Vec<MediaItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subtitle {
    pub id: String,
    pub language: String,
    pub label: String,
    pub url: String,
    pub format: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoSource {
    pub id: String,
    pub url: String,
    pub quality: String,
    pub title: String,
    pub container: String,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub subtitles: Vec<Subtitle>,
    pub provider_id: String,
    /// True when `url` is a web page that must be shown in an embedded browser (YouTube, Vimeo, ...).
    #[serde(default)]
    pub embed: bool,
}
