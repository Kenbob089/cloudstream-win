using System.Diagnostics;
using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;

namespace CloudStream.Pages;

public sealed partial class ExtensionsPage : Page
{
    public ExtensionsPage() => InitializeComponent();

    protected override async void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        await Reload();
    }

    private async Task Reload(string? error = null, string? info = null)
    {
        try { await AppState.RefreshAsync(); }
        catch (Exception ex) { error ??= ex.Message; }
        Build(error, info);
    }

    private static Grid Row()
    {
        var g = new Grid { Padding = new Thickness(14), ColumnSpacing = 12, Background = Ui.Card, CornerRadius = new CornerRadius(12) };
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        return g;
    }

    private static Grid HeaderRow(string title, params Button[] actions)
    {
        var head = new Grid { ColumnSpacing = 8 };
        head.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        head.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        head.Children.Add(Ui.Header(title));
        var tools = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        foreach (var a in actions) tools.Children.Add(a);
        Grid.SetColumn(tools, 1);
        head.Children.Add(tools);
        return head;
    }

    private void Build(string? error, string? info)
    {
        Root.Children.Clear();
        Root.Children.Add(Ui.Title("Extensions"));
        if (error != null) Root.Children.Add(Ui.Error(error));
        if (info != null)
            Root.Children.Add(new InfoBar { IsOpen = true, Severity = InfoBarSeverity.Success, Message = info, IsClosable = true });
        var snap = AppState.Current;

        Root.Children.Add(Ui.Text(
            "Importing a repository only loads its list. Providers are installed separately, one by one or all at once. Android .cs3 plugins are never run by this app.",
            13, false, 0.7));

        // ---- built-in
        Root.Children.Add(Ui.Header("Built-in providers"));
        foreach (var info2 in snap.Providers.Where(p => !p.Custom)) Root.Children.Add(BuiltInRow(info2));

        // ---- yours
        var create = Ui.IconButton(Symbol.Add, "Create", async () =>
        {
            if (await ProviderEditor.EditAsync(XamlRoot, null)) await Reload();
        });
        var fromLink = Ui.IconButton(Symbol.Link, "From link", async () =>
        {
            if (await ProviderEditor.AddFromLinkAsync(XamlRoot)) await Reload();
        });
        var export = Ui.IconButton(Symbol.Share, "Export", async () => await ExportAsync());
        Root.Children.Add(HeaderRow("Your providers", create, fromLink, export));

        var custom = snap.Providers.Where(p => p.Custom).ToList();
        if (custom.Count == 0)
        {
            Root.Children.Add(Ui.Text(
                "Make a provider for any site that lists videos: a feed, playlist, JSON API, PeerTube instance or a web page. Use From link and the app will work out the settings.",
                12, false, 0.65));
        }
        foreach (var p in custom) Root.Children.Add(CustomRow(p));

        // ---- repositories
        var phisher = Ui.IconButton(Symbol.Download, "Add " + snap.Presets.PhisherName, async () => await AddPhisher());
        var manual = Ui.IconButton(Symbol.Link, "Add URL", async () => await AddManual());
        Root.Children.Add(HeaderRow("Repositories", phisher, manual));

        if (snap.Repositories.Count == 0)
        {
            Root.Children.Add(Ui.State(Symbol.Download, "No repositories",
                "Import a repository link to see its providers. Nothing is installed automatically."));
            return;
        }
        foreach (var repo in snap.Repositories) Root.Children.Add(RepoRow(repo));
    }

    private FrameworkElement BuiltInRow(ProviderInfo info)
    {
        var d = info.Descriptor;
        var row = Row();
        var text = new StackPanel { Spacing = 3 };
        text.Children.Add(Ui.Text(d.Name, 15, true));
        text.Children.Add(Ui.Text(d.Description, 12, false, 0.7));
        text.Children.Add(Ui.Text(d.AuthorizationNote, 11, false, 0.55));
        row.Children.Add(text);
        var btn = new Button { Content = info.Installed ? "Turn off" : "Turn on", VerticalAlignment = VerticalAlignment.Center };
        var id = d.Id; bool installed = info.Installed;
        btn.Click += async (_, _) =>
        {
            btn.IsEnabled = false;
            try { await Core.CallAsync<JsonElement>(installed ? "provider.uninstall" : "provider.install", new { id }); await Reload(); }
            catch (Exception ex) { await Reload(ex.Message); }
        };
        Grid.SetColumn(btn, 1);
        row.Children.Add(btn);
        return row;
    }

    private FrameworkElement CustomRow(ProviderInfo info)
    {
        var d = info.Descriptor;
        var def = info.Definition ?? new CustomDefinition();
        var row = Row();
        var text = new StackPanel { Spacing = 3 };
        text.Children.Add(Ui.Text(d.Name, 15, true));
        text.Children.Add(Ui.Text(d.Description, 12, false, 0.7));
        if (!string.IsNullOrEmpty(def.Source))
        {
            var repoName = AppState.Current.Repositories.FirstOrDefault(r => r.Url == def.Source)?.Name ?? def.Source;
            text.Children.Add(Ui.Text($"Installed from {repoName} (version {def.Version})", 11, false, 0.55));
        }
        row.Children.Add(text);

        var actions = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6, VerticalAlignment = VerticalAlignment.Center };
        var edit = Ui.IconOnly(Symbol.Edit, "Edit " + d.Name, async () =>
        {
            if (await ProviderEditor.EditAsync(XamlRoot, def)) await Reload();
        });
        var remove = Ui.IconOnly(Symbol.Delete, "Remove " + d.Name, async () =>
        {
            if (!await Ui.ConfirmAsync(XamlRoot, $"Remove {d.Name}?", "The provider is deleted. Your bookmarks and history stay.", "Remove")) return;
            await Core.CallAsync<JsonElement>("provider.delete", new { id = d.Id });
            await Reload();
        });
        actions.Children.Add(edit);
        actions.Children.Add(remove);
        Grid.SetColumn(actions, 1);
        row.Children.Add(actions);
        return row;
    }

    private FrameworkElement RepoRow(RepoDto repo)
    {
        var row = Row();
        var text = new StackPanel { Spacing = 3 };
        text.Children.Add(Ui.Text(repo.Name, 15, true));
        if (!string.IsNullOrEmpty(repo.Description)) text.Children.Add(Ui.Text(repo.Description!, 12, false, 0.7, true, 2));
        int ready = repo.Plugins.Count(p => p.Status.Kind == "declarative");
        var meta = $"{repo.Plugins.Count} plugins";
        if (ready > 0) meta += $" · {ready} ready to install";
        if (repo.LastRefreshed is long t) meta += " · refreshed " + DateTimeOffset.FromUnixTimeSeconds(t).LocalDateTime.ToString("g");
        text.Children.Add(Ui.Text(meta, 11, false, 0.55, false));
        if (repo.LastError != null)
            text.Children.Add(Ui.IconLabel(Symbol.Important, "Last refresh failed: " + repo.LastError, 12));
        foreach (var w in repo.Warnings.Take(3)) text.Children.Add(Ui.Text(w, 11, false, 0.7));
        row.Children.Add(text);

        var actions = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6, VerticalAlignment = VerticalAlignment.Center };
        if (ready > 0)
        {
            var installAll = Ui.Accent("Install all", () => { });
            installAll.Click += async (_, _) =>
            {
                installAll.IsEnabled = false;
                try
                {
                    var r = await Core.CallAsync<JsonElement>("plugin.installAll", new { repoUrl = repo.Url });
                    int n = r.GetProperty("installed").GetInt32();
                    var failed = r.GetProperty("failed").EnumerateArray().Select(x => x.GetString() ?? "").ToList();
                    await Reload(failed.Count > 0 ? string.Join("\n", failed) : null, $"Installed {n} provider{(n == 1 ? "" : "s")}.");
                }
                catch (Exception ex) { await Reload(ex.Message); }
            };
            actions.Children.Add(installAll);
        }
        var open = new Button { Content = "Open" };
        open.Click += (_, _) => App.MainWin?.Navigate(typeof(RepositoryPage), repo.Url);
        var refresh = Ui.IconOnly(Symbol.Refresh, "Refresh " + repo.Name, async () =>
        {
            try { await Core.CallAsync<JsonElement>("repo.refresh", new { url = repo.Url }); await Reload(null, "Repository refreshed. Installed providers were updated."); }
            catch (Exception ex) { await Reload(ex.Message); }
        });
        var remove = Ui.IconOnly(Symbol.Delete, "Remove " + repo.Name, async () =>
        {
            if (!await Ui.ConfirmAsync(XamlRoot, $"Remove {repo.Name}?", "Its cached plugin list is deleted. Providers you installed from it stay installed.", "Remove")) return;
            await Core.CallAsync<JsonElement>("repo.remove", new { url = repo.Url });
            await Reload();
        });
        actions.Children.Add(open); actions.Children.Add(refresh); actions.Children.Add(remove);
        Grid.SetColumn(actions, 1);
        row.Children.Add(actions);
        return row;
    }

    private async Task ExportAsync()
    {
        var dir = Path.Combine(AppState.DataDir, "export");
        try
        {
            await Core.CallAsync<JsonElement>("repo.export", new { dir });
            try { Process.Start(new ProcessStartInfo("explorer.exe", $"\"{dir}\"") { UseShellExecute = true }); } catch { }
            await Reload(null,
                "Exported repo.json and providers.json. Upload both to the same https address (for example a GitHub repository), then import the link to repo.json in any copy of this app.");
        }
        catch (Exception ex) { await Reload(ex.Message); }
    }

    private async Task AddPhisher()
    {
        var p = AppState.Current.Presets;
        var ok = await Ui.ConfirmAsync(XamlRoot, "Import " + p.PhisherName + "?",
            $"This downloads the plugin list from:\n{p.PhisherRepoUrl}\n\nThose plugins are Android .cs3 files. They are listed as metadata only and are never downloaded or run by this app.",
            "Import");
        if (ok) await Import(p.PhisherRepoUrl);
    }

    private async Task AddManual()
    {
        var box = new TextBox { PlaceholderText = "https://.../repo.json" };
        var dlg = new ContentDialog
        {
            XamlRoot = XamlRoot, Title = "Add repository", Content = box,
            PrimaryButtonText = "Import", CloseButtonText = "Cancel", DefaultButton = ContentDialogButton.Primary,
        };
        if (await dlg.ShowAsync() != ContentDialogResult.Primary) return;
        var url = box.Text.Trim();
        if (!Uri.TryCreate(url, UriKind.Absolute, out var u) || u.Scheme != Uri.UriSchemeHttps)
        {
            Build("Repository addresses must start with https://", null);
            return;
        }
        await Import(url);
    }

    private async Task Import(string url)
    {
        Root.Children.Add(Ui.Busy());
        try { await Core.CallAsync<JsonElement>("repo.import", new { url }); await Reload(null, "Repository imported. Nothing was installed yet."); }
        catch (Exception ex) { await Reload(ex.Message); }
    }
}
