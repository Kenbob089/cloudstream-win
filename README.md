# CloudStream for Windows (Rust core + WinUI 3)

A Windows counterpart to the iOS app: a **Rust core** (`core/`, exposed as a C ABI DLL) and a thin
**WinUI 3 / C#** shell (`app/`) that calls it through a JSON bridge (`cs_init`, `cs_call`, `cs_free`).

## Status: please read

**Nothing in this project has been compiled or run.** It was written in an environment without the
Windows SDK, .NET or the ability to download Rust crates. Expect compile errors on the first CI run,
in both the Rust modules and the WinUI code. Run the workflow, then fix whatever the log reports.

## Sources

Built in (turn on or off in Extensions):

- **Web links**: paste an https link into Search. Recognizes YouTube, Vimeo, Dailymotion, PeerTube,
  direct media files and ordinary pages (it reads `video`, `source`, `og:video` and `iframe` tags).
- **Internet Archive** (movies collection; multi-file items become episodes).
- **PeerTube** (default instance framatube.org), **Dailymotion** (public API).
- **YouTube** (YouTube Data API v3). Needs your own API key in Settings. Pasted YouTube links work without one.

Your own providers (Extensions, Your providers) are data-only definitions, never code:
`rss`, `m3u`, `json`, `html` (CSS selectors) and `peertube`. Use **From link** to paste a site, feed,
playlist or channel address and let the app detect the type, then Test and Save.

## Repositories

Import a CloudStream-style `repo.json` link. `pluginLists` entries may be relative to the repo file.
Plugin entries can carry an optional `definition` object, which makes them installable natively:

```json
{
  "name": "My Videos", "internalName": "MyVideos", "version": 1, "language": "en",
  "definition": { "kind": "rss", "url": "https://example.com/feed.xml" }
}
```

- **Install all** installs every entry with a definition; per-entry Install/Update is on the repository page.
- Refreshing a repository updates installed providers whose repo version is higher.
- **Export** (Extensions, Your providers) writes `repo.json` + `providers.json` to
  `%LOCALAPPDATA%\CloudStream\export`. Host those two files anywhere over https (GitHub Pages, for example)
  and share the `repo.json` link.
- The Phisher repository is a one-click preset. Its Android `.cs3` entries are listed as metadata only.

## Player

Multiple sources per title with a Sources menu (title and quality), automatic fallback to the next
source when one fails, subtitles when a source provides them, speed, full screen, resume and auto-next.
Embedded players (YouTube, Vimeo, Dailymotion, generic player pages) are shown in WebView2.
Keys: Space, Left/Right (10 s), M, F, N/P, Esc.

## Deliberately not included

- No `.cs3` or other Android plugin is downloaded, translated or run.
- No ports of scrapers for third-party sites whose content is not authorized for distribution.
- No DRM or access-control bypass. No analytics or ads.

## Known gaps

- MediaPlayer cannot send custom HTTP headers; sources needing them are skipped.
- Embedded players report no position, so only a history entry is saved for them.
- No offline downloads, no per-provider settings screens, English only.

## Build

```
cargo test --workspace
cargo build --release -p cloudstream-core
dotnet publish app/CloudStream.csproj -c Release -r win-x64 --self-contained true -p:Platform=x64 -o publish
```

`.github/workflows/build-windows.yml` does this on `windows-latest` and uploads `CloudStream-win-x64.zip`.
`delete-all-files.yml` empties the repo (manual, type DELETE).

## Adding a native provider

Implement `MediaProvider` in `core/src/provider.rs` and register it in the context's native list.
Providers must only access content they are authorized to access.
