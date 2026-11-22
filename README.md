# CloudStream for Windows (Rust core + WinUI 3)

A Windows counterpart to the iOS app: a **Rust core** (`core/`, exposed as a C ABI DLL) and a thin
**WinUI 3 / C#** shell (`app/`) that calls it through a JSON bridge (`cs_init`, `cs_call`, `cs_free`).

## Status — please read

**Nothing in this project has been compiled or run.** It was written in an environment without the
Windows SDK, .NET or the ability to download Rust crates. Expect compile errors on the first CI run
(especially in the WinUI code). Run the workflow, then fix whatever the log reports.

## What works (by design)

- Import a repository (`repo.json` → `pluginLists` → `plugins.json`, merged and deduplicated), with the
  Phisher repository as a one-click preset. Importing never installs anything.
- Plugin entries are **metadata only**: Android `.cs3` files are never downloaded or executed.
  Every entry shows a compatibility status (Native Available / Metadata Only / Not Supported).
  The native mapping table is intentionally empty, so none of the Phisher entries are enabled.
- One built-in provider: **Internet Archive** (public APIs). Install/remove it in Extensions.
- Home, Search, Details, Library (continue / bookmarks / favorites / history), Extensions,
  Repository browser, Settings, and a `MediaPlayer`-based player with resume, auto-next and retry.
- Local JSON persistence (`%LOCALAPPDATA%\CloudStream\library.json`, atomic writes). No analytics or ads.

## Known gaps

- Custom HTTP headers are not supported in playback; sources needing them are refused.
- No offline downloads, no per-provider settings screens, English only.
- Mica/Acrylic stand in for iOS "Liquid Glass".

## Build

```
cargo test --workspace
cargo build --release -p cloudstream-core
dotnet publish app/CloudStream.csproj -c Release -r win-x64 --self-contained true -p:Platform=x64 -o publish
```

The `.github/workflows/build-windows.yml` workflow does this on `windows-latest` and uploads
`CloudStream-win-x64.zip`. The app is unpackaged (no MSIX signing required).

## Adding a provider

Implement `MediaProvider` in `core/src/provider.rs`, register it in the context's native list, and
only then add a mapping in `core/src/compat.rs` so matching repository entries become installable.
Providers must only access content they are authorized to access.
