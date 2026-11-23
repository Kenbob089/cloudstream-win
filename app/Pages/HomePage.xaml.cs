using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;

namespace CloudStream.Pages;

public sealed partial class HomePage : Page
{
    public HomePage() => InitializeComponent();

    protected override async void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        Root.Children.Clear();
        Root.Children.Add(Ui.Title("Home"));
        Root.Children.Add(Ui.Busy());
        try
        {
            var snap = await AppState.RefreshAsync();
            var enabled = snap.Providers.Any(p => p.Installed);
            var groups = enabled ? await Core.CallAsync<List<HomeGroup>>("home") : new List<HomeGroup>();
            Build(snap, groups, enabled);
        }
        catch (Exception ex)
        {
            Root.Children.Clear();
            Root.Children.Add(Ui.Title("Home"));
            Root.Children.Add(Ui.Error(ex.Message));
        }
    }

    private void Build(Snapshot snap, List<HomeGroup> groups, bool enabled)
    {
        Root.Children.Clear();
        Root.Children.Add(Ui.Title("Home"));
        if (App.StartupError != null)
            Root.Children.Add(Ui.Error("The core library could not be loaded: " + App.StartupError));

        // Continue Watching: unfinished, latest entry per title.
        if (AppState.Setting("continueWatching", true))
        {
            var resume = snap.Progress.Where(p => !p.Completed && p.Position > 5)
                .OrderByDescending(p => p.UpdatedAt).GroupBy(p => p.MediaKey).Select(g => g.First()).Take(20).ToList();
            if (resume.Count > 0)
                Root.Children.Add(Ui.Shelf("Continue Watching", resume.Select(r => r.Item),
                    item => resume.First(r => r.MediaKey == item.Key).Fraction));
        }

        foreach (var group in groups)
            foreach (var catalog in group.Catalogs.Where(c => c.Items.Count > 0))
                Root.Children.Add(Ui.Shelf(catalog.Title, catalog.Items));

        if (snap.Bookmarks.Count > 0)
            Root.Children.Add(Ui.Shelf("Bookmarked", snap.Bookmarks.Take(20).Select(b => b.Item)));

        var recent = snap.Progress.OrderByDescending(p => p.UpdatedAt).GroupBy(p => p.MediaKey)
            .Select(g => g.First().Item).Take(20).ToList();
        if (recent.Count > 1) Root.Children.Add(Ui.Shelf("Recently Watched", recent));

        if (!enabled)
        {
            Root.Children.Add(Ui.State(Symbol.AllApps, "No sources installed",
                "CloudStream doesn't include any video sources. Open Extensions to import a repository and enable a compatible provider.",
                "Open Extensions", () => App.MainWin?.GoTo("extensions")));
        }
        else if (groups.All(g => g.Catalogs.Count == 0) && Root.Children.Count == 1)
        {
            Root.Children.Add(Ui.State(Symbol.Important, "Nothing to show yet",
                "The enabled providers returned no home content. Check your connection and reopen Home."));
        }
    }
}
