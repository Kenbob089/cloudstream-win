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

    private async Task Reload(string? error = null)
    {
        try { await AppState.RefreshAsync(); }
        catch (Exception ex) { error ??= ex.Message; }
        Build(error);
    }

    private void Build(string? error)
    {
        Root.Children.Clear();
        Root.Children.Add(Ui.Title("Extensions"));
        if (error != null) Root.Children.Add(Ui.Error(error));
        var snap = AppState.Current;

        Root.Children.Add(Ui.Text(
            "Importing a repository only loads its list of plugins. Installing a provider is a separate step, and only providers that are natively implemented in this app can be enabled.",
            13, false, 0.7));

        // Built-in providers
        Root.Children.Add(Ui.Header("Built-in providers"));
        foreach (var info in snap.Providers)
        {
            var d = info.Descriptor;
            var row = Row();
            var text = new StackPanel { Spacing = 3 };
            text.Children.Add(Ui.Text(d.Name, 15, true));
            text.Children.Add(Ui.Text(d.Description, 12, false, 0.7));
            text.Children.Add(Ui.Text(d.AuthorizationNote, 11, false, 0.55));
            row.Children.Add(text);
            var btn = new Button { Content = info.Installed ? "Remove" : "Install", VerticalAlignment = VerticalAlignment.Center };
            var id = d.Id; bool installed = info.Installed;
            btn.Click += async (_, _) =>
            {
                btn.IsEnabled = false;
                try { await Core.CallAsync<JsonElement>(installed ? "provider.uninstall" : "provider.install", new { id }); await Reload(); }
                catch (Exception ex) { await Reload(ex.Message); }
            };
            Grid.SetColumn(btn, 1);
            row.Children.Add(btn);
            Root.Children.Add(row);
        }

        // Repositories
        var head = new Grid();
        head.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        head.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        head.Children.Add(Ui.Header("Repositories"));
        var tools = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        var phisher = new Button { Content = "Add " + snap.Presets.PhisherName };
        phisher.Click += async (_, _) => await AddPhisher();
        var manual = new Button { Content = "Add URL…" };
        manual.Click += async (_, _) => await AddManual();
        tools.Children.Add(phisher);
        tools.Children.Add(manual);
        Grid.SetColumn(tools, 1);
        head.Children.Add(tools);
        Root.Children.Add(head);

        if (snap.Repositories.Count == 0)
        {
            Root.Children.Add(Ui.State(Symbol.Download, "No repositories",
                "Import a repository to browse its plugin list. Nothing is installed automatically."));
            return;
        }
        foreach (var repo in snap.Repositories) Root.Children.Add(RepoRow(repo));
    }

    private static Grid Row()
    {
        var g = new Grid { Padding = new Thickness(14), ColumnSpacing = 12, Background = Ui.Card, CornerRadius = new CornerRadius(12) };
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        return g;
    }

    private FrameworkElement RepoRow(RepoDto repo)
    {
        var row = Row();
        var text = new StackPanel { Spacing = 3 };
        text.Children.Add(Ui.Text(repo.Name, 15, true));
        if (!string.IsNullOrEmpty(repo.Description)) text.Children.Add(Ui.Text(repo.Description!, 12, false, 0.7, true, 2));
        var meta = $"{repo.Plugins.Count} plugins";
        if (repo.LastRefreshed is long t) meta += " · refreshed " + DateTimeOffset.FromUnixTimeSeconds(t).LocalDateTime.ToString("g");
        text.Children.Add(Ui.Text(meta, 11, false, 0.55, false));
        if (repo.LastError != null) text.Children.Add(Ui.Text("Last refresh failed: " + repo.LastError, 11, false, 0.9));
        foreach (var w in repo.Warnings.Take(3)) text.Children.Add(Ui.Text("⚠ " + w, 11, false, 0.7));
        row.Children.Add(text);

        var actions = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6, VerticalAlignment = VerticalAlignment.Center };
        var open = new Button { Content = "Open" };
        open.Click += (_, _) => App.MainWin?.Navigate(typeof(RepositoryPage), repo.Url);
        var refresh = new Button { Content = "Refresh" };
        refresh.Click += async (_, _) =>
        {
            refresh.IsEnabled = false;
            try { await Core.CallAsync<JsonElement>("repo.refresh", new { url = repo.Url }); await Reload(); }
            catch (Exception ex) { await Reload(ex.Message); }
        };
        var remove = new Button { Content = "Remove" };
        remove.Click += async (_, _) =>
        {
            if (!await Ui.ConfirmAsync(XamlRoot, $"Remove {repo.Name}?", "Its cached plugin list is deleted. Providers you've enabled stay enabled.", "Remove")) return;
            await Core.CallAsync<JsonElement>("repo.remove", new { url = repo.Url });
            await Reload();
        };
        actions.Children.Add(open); actions.Children.Add(refresh); actions.Children.Add(remove);
        Grid.SetColumn(actions, 1);
        row.Children.Add(actions);
        return row;
    }

    private async Task AddPhisher()
    {
        var p = AppState.Current.Presets;
        var ok = await Ui.ConfirmAsync(XamlRoot, "Import " + p.PhisherName + "?",
            $"This downloads the plugin list from:\n{p.PhisherRepoUrl}\n\nThe plugins are Android .cs3 files. They are shown as metadata only and are never downloaded or run by this app.",
            "Import");
        if (ok) await Import(p.PhisherRepoUrl);
    }

    private async Task AddManual()
    {
        var box = new TextBox { PlaceholderText = "https://…/repo.json" };
        var dlg = new ContentDialog
        {
            XamlRoot = XamlRoot, Title = "Add repository", Content = box,
            PrimaryButtonText = "Import", CloseButtonText = "Cancel", DefaultButton = ContentDialogButton.Primary,
        };
        if (await dlg.ShowAsync() != ContentDialogResult.Primary) return;
        var url = box.Text.Trim();
        if (!Uri.TryCreate(url, UriKind.Absolute, out var u) || u.Scheme != Uri.UriSchemeHttps)
        {
            Build("Repository URLs must start with https://");
            return;
        }
        await Import(url);
    }

    private async Task Import(string url)
    {
        Root.Children.Add(Ui.Busy());
        try { await Core.CallAsync<JsonElement>("repo.import", new { url }); await Reload(); }
        catch (Exception ex) { await Reload(ex.Message); }
    }
}
