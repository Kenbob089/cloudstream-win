using System.Text.Json;

namespace CloudStream;

/// <summary>Cached view of the core's persisted state plus typed settings access.</summary>
public static class AppState
{
    public static string DataDir => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "CloudStream");

    public static Snapshot Current { get; private set; } = new();

    public static async Task<Snapshot> RefreshAsync()
    {
        Current = await Core.CallAsync<Snapshot>("snapshot");
        return Current;
    }

    public static T Setting<T>(string key, T fallback)
    {
        if (Current.Settings.TryGetValue(key, out var el))
        {
            try { return el.Deserialize<T>(Core.Json) ?? fallback; } catch { }
        }
        return fallback;
    }

    public static async Task SetSettingAsync(string key, object value)
    {
        Current.Settings[key] = JsonSerializer.SerializeToElement(value, Core.Json);
        await Core.CallAsync<JsonElement>("settings.set", new { key, value });
    }

    public static string ProviderName(string providerId) =>
        Current.Providers.FirstOrDefault(p => p.Descriptor.Id == providerId)?.Descriptor.Name ?? providerId;
}
