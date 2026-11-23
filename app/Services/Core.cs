using System.Runtime.InteropServices;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace CloudStream;

public class CoreException : Exception
{
    public CoreException(string message) : base(message) { }
}

/// <summary>P/Invoke bridge to the Rust core (cloudstream_core.dll). All logic lives in Rust.</summary>
public static class Core
{
    private const string Dll = "cloudstream_core";

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int cs_init([MarshalAs(UnmanagedType.LPUTF8Str)] string dataDir);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr cs_call(
        [MarshalAs(UnmanagedType.LPUTF8Str)] string method,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string args);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern void cs_free(IntPtr ptr);

    public static readonly JsonSerializerOptions Json = new(JsonSerializerDefaults.Web)
    {
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
    };

    public static void Init(string dataDir)
    {
        Directory.CreateDirectory(dataDir);
        int code = cs_init(dataDir);
        if (code != 0) throw new CoreException($"The core failed to start (code {code}).");
    }

    /// <summary>Runs a core call off the UI thread.</summary>
    public static Task<T> CallAsync<T>(string method, object? args = null) => Task.Run(() => Call<T>(method, args));

    public static T Call<T>(string method, object? args = null)
    {
        string argJson = JsonSerializer.Serialize(args ?? new { }, Json);
        IntPtr ptr = cs_call(method, argJson);
        try
        {
            string text = Marshal.PtrToStringUTF8(ptr) ?? "{\"error\":\"Empty response.\"}";
            using var doc = JsonDocument.Parse(text);
            var root = doc.RootElement;
            if (root.TryGetProperty("error", out var err))
                throw new CoreException(err.GetString() ?? "Unknown error.");
            return root.GetProperty("ok").Deserialize<T>(Json)!;
        }
        finally
        {
            cs_free(ptr);
        }
    }
}
