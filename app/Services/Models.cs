using System.Text.Json;
using System.Text.Json.Serialization;

namespace CloudStream;

public class MediaItem
{
    public string Id { get; set; } = "";
    public string ProviderId { get; set; } = "";
    public string Title { get; set; } = "";
    public string? OriginalTitle { get; set; }
    public string? Description { get; set; }
    public string? PosterUrl { get; set; }
    public string? BackdropUrl { get; set; }
    public int? ReleaseYear { get; set; }
    public List<string> Genres { get; set; } = new();
    public string MediaType { get; set; } = "movie";
    public double? Duration { get; set; }
    public double? Rating { get; set; }

    [JsonIgnore] public string Key => $"{ProviderId}::{Id}";
}

public class MediaDetails
{
    public MediaItem Item { get; set; } = new();
    public List<int> Seasons { get; set; } = new();
    public List<string> Languages { get; set; } = new();
}

public class MediaEpisode
{
    public string Id { get; set; } = "";
    public string Title { get; set; } = "";
    public int? EpisodeNumber { get; set; }
    public int? SeasonNumber { get; set; }
    public string? Description { get; set; }
    public string? ThumbnailUrl { get; set; }
    public double? Duration { get; set; }
}

public class Catalog
{
    public string Id { get; set; } = "";
    public string Title { get; set; } = "";
    public List<MediaItem> Items { get; set; } = new();
}

public class VideoSource
{
    public string Id { get; set; } = "";
    public string Url { get; set; } = "";
    public string Quality { get; set; } = "";
    public string Title { get; set; } = "";
    public string Container { get; set; } = "";
    public string? MimeType { get; set; }
    public Dictionary<string, string> Headers { get; set; } = new();
    public string ProviderId { get; set; } = "";
}

public class ProviderDescriptor
{
    public string Id { get; set; } = "";
    public string Name { get; set; } = "";
    public string Description { get; set; } = "";
    public string Language { get; set; } = "";
    public List<string> SupportedTypes { get; set; } = new();
    public int Version { get; set; }
    public string Author { get; set; } = "";
    public string? SourceUrl { get; set; }
    public string AuthorizationNote { get; set; } = "";
}

public class ProviderInfo
{
    public ProviderDescriptor Descriptor { get; set; } = new();
    public bool Installed { get; set; }
}

public class Bookmark
{
    public MediaItem Item { get; set; } = new();
    public bool IsFavorite { get; set; }
    public long AddedAt { get; set; }
}

public class ProgressEntry
{
    public string Key { get; set; } = "";
    public string MediaKey { get; set; } = "";
    public MediaItem Item { get; set; } = new();
    public string? EpisodeId { get; set; }
    public string? EpisodeTitle { get; set; }
    public int? SeasonNumber { get; set; }
    public int? EpisodeNumber { get; set; }
    public double Position { get; set; }
    public double Duration { get; set; }
    public bool Completed { get; set; }
    public long UpdatedAt { get; set; }

    [JsonIgnore] public double Fraction => Duration > 0 ? Math.Min(Position / Duration, 1) : 0;
}

public class PluginEntry
{
    public string Url { get; set; } = "";
    public string Name { get; set; } = "";
    public string InternalName { get; set; } = "";
    public long? Status { get; set; }
    public long? Version { get; set; }
    public List<string> Authors { get; set; } = new();
    public string? Description { get; set; }
    public long? FileSize { get; set; }
    public string? RepositoryUrl { get; set; }
    public string? Language { get; set; }
    public List<string> TvTypes { get; set; } = new();
    public string? IconUrl { get; set; }
    public long? ApiVersion { get; set; }
    public string? FileHash { get; set; }
}

public class CompatStatus
{
    public string Kind { get; set; } = "metadataOnly";
    public string Label { get; set; } = "";
    public string Explanation { get; set; } = "";
    public bool Installable { get; set; }
    public string? ProviderId { get; set; }
    public string? Reason { get; set; }
}

public class PluginWithStatus
{
    public PluginEntry Plugin { get; set; } = new();
    public CompatStatus Status { get; set; } = new();
}

public class RepoDto
{
    public string Url { get; set; } = "";
    public string Name { get; set; } = "";
    public string? Description { get; set; }
    public string? IconUrl { get; set; }
    public long? LastRefreshed { get; set; }
    public string? LastError { get; set; }
    public List<string> Warnings { get; set; } = new();
    public List<PluginWithStatus> Plugins { get; set; } = new();
}

public class Presets
{
    public string PhisherName { get; set; } = "";
    public string PhisherRepoUrl { get; set; } = "";
    public string PhisherSourceUrl { get; set; } = "";
}

public class Snapshot
{
    public List<Bookmark> Bookmarks { get; set; } = new();
    public List<ProgressEntry> Progress { get; set; } = new();
    public List<string> SearchHistory { get; set; } = new();
    public List<RepoDto> Repositories { get; set; } = new();
    public List<ProviderInfo> Providers { get; set; } = new();
    public Dictionary<string, JsonElement> Settings { get; set; } = new();
    public Presets Presets { get; set; } = new();
}

public class HomeGroup
{
    public ProviderDescriptor Provider { get; set; } = new();
    public List<Catalog> Catalogs { get; set; } = new();
}

public class SearchGroup
{
    public ProviderDescriptor Provider { get; set; } = new();
    public List<MediaItem> Items { get; set; } = new();
    public string? Error { get; set; }
}

public class PlayerRequest
{
    public MediaItem Item { get; set; } = new();
    public List<MediaEpisode> Episodes { get; set; } = new();
    public int? Index { get; set; }
}

public static class Labels
{
    public static string Type(string t) => t switch
    {
        "movie" => "Movie", "tvSeries" => "TV Series", "anime" => "Anime", "animeMovie" => "Anime Movie",
        "ova" => "OVA", "cartoon" => "Cartoon", "asianDrama" => "Asian Drama", "documentary" => "Documentary",
        "live" => "Live", "music" => "Music", "audioBook" => "Audiobook", "torrent" => "Torrent", _ => "Other",
    };

    public static string Runtime(double seconds)
    {
        int m = (int)(seconds / 60);
        return m >= 60 ? $"{m / 60}h {m % 60}m" : $"{m}m";
    }

    public static string Plugin(PluginEntry p) => p.TvTypes
        .SelectMany(t => t.Split(','))
        .Select(t => t.Trim().ToLowerInvariant() switch
        {
            "movie" => "Movie", "tvseries" => "TV Series", "anime" => "Anime", "animemovie" => "Anime Movie",
            "ova" => "OVA", "cartoon" => "Cartoon", "asiandrama" => "Asian Drama", "documentary" => "Documentary",
            "live" => "Live", "music" => "Music", "audiobook" => "Audiobook", "torrent" => "Torrent", _ => "Other",
        })
        .Distinct().Aggregate("", (a, b) => a.Length == 0 ? b : a + ", " + b);
}
