using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;

namespace CloudStream.Pages;

public sealed partial class SearchPage : Page
{
    private readonly AutoSuggestBox _box = new()
    {
        PlaceholderText = "Movies, shows, anime",
        QueryIcon = new SymbolIcon(Symbol.Find),
        MaxWidth = 560,
        HorizontalAlignment = HorizontalAlignment.Left,
    };
    private readonly ComboBox _filter = new() { MinWidth = 200 };
    private readonly StackPanel _results = new() { Spacing = 22 };
    private string? _providerFilter;
    private string _submitted = "";
    private int _runId;

    public SearchPage()
    {
        InitializeComponent();
        Root.Children.Add(Ui.Title("Search"));
        Root.Children.Add(_box);
        Root.Children.Add(_filter);
        Root.Children.Add(_results);

        _box.TextChanged += (_, a) =>
        {
            if (a.Reason != AutoSuggestionBoxTextChangeReason.UserInput) return;
            _box.ItemsSource = AppState.Current.SearchHistory
                .Where(h => h.Contains(_box.Text, StringComparison.OrdinalIgnoreCase)).Take(6).ToList();
        };
        _box.QuerySubmitted += async (_, a) => await RunAsync(a.QueryText);
        _filter.SelectionChanged += async (_, _) =>
        {
            _providerFilter = (_filter.SelectedItem as ComboBoxItem)?.Tag as string;
            if (_submitted.Length > 0) await RunAsync(_submitted);
        };
    }

    protected override async void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        try { await AppState.RefreshAsync(); } catch (Exception ex) { _results.Children.Add(Ui.Error(ex.Message)); }

        var enabled = AppState.Current.Providers.Where(p => p.Installed).Select(p => p.Descriptor).ToList();
        _filter.Items.Clear();
        _filter.Items.Add(new ComboBoxItem { Content = "All sources", Tag = null });
        foreach (var d in enabled) _filter.Items.Add(new ComboBoxItem { Content = d.Name, Tag = d.Id });
        _filter.SelectedIndex = 0;
        _filter.Visibility = enabled.Count > 1 ? Visibility.Visible : Visibility.Collapsed;
        _box.IsEnabled = enabled.Count > 0;

        if (_submitted.Length == 0) ShowIdle(enabled.Count);
    }

    private void ShowIdle(int providers)
    {
        _results.Children.Clear();
        if (providers == 0)
        {
            _results.Children.Add(Ui.State(Symbol.AllApps, "No sources to search",
                "Search only queries providers you've enabled. Add one in Extensions.",
                "Open Extensions", () => App.MainWin?.GoTo("extensions")));
            return;
        }
        var history = AppState.Current.SearchHistory;
        if (history.Count == 0)
        {
            _results.Children.Add(Ui.State(Symbol.Find, "Search your sources", "Results show which provider returned them."));
            return;
        }
        var head = new Grid();
        head.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        head.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        head.Children.Add(Ui.Header("Recent searches"));
        var clear = new HyperlinkButton { Content = "Clear" };
        clear.Click += async (_, _) =>
        {
            await Core.CallAsync<System.Text.Json.JsonElement>("search.clear");
            await AppState.RefreshAsync();
            ShowIdle(providers);
        };
        Grid.SetColumn(clear, 1);
        head.Children.Add(clear);
        _results.Children.Add(head);
        foreach (var q in history.Take(12))
        {
            var query = q;
            var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 10 };
            row.Children.Add(new SymbolIcon(Symbol.Clock) { Opacity = 0.6 });
            row.Children.Add(Ui.Text(query, 14, false, 1, false));
            _results.Children.Add(Ui.Plain(row, async () => { _box.Text = query; await RunAsync(query); }, query));
        }
    }

    private async Task RunAsync(string raw)
    {
        var query = raw.Trim();
        if (query.Length == 0) return;
        _submitted = query;
        int myRun = ++_runId; // ignore stale responses if the user searches again
        _results.Children.Clear();
        _results.Children.Add(Ui.Busy());
        try
        {
            var groups = await Core.CallAsync<List<SearchGroup>>("search", new { query, providerId = _providerFilter });
            if (myRun != _runId) return;
            await AppState.RefreshAsync();
            Render(groups, query);
        }
        catch (Exception ex)
        {
            if (myRun != _runId) return;
            _results.Children.Clear();
            _results.Children.Add(Ui.Error(ex.Message));
        }
    }

    private void Render(List<SearchGroup> groups, string query)
    {
        _results.Children.Clear();
        if (groups.All(g => g.Items.Count == 0))
        {
            bool allFailed = groups.Count > 0 && groups.All(g => g.Error != null);
            _results.Children.Add(allFailed
                ? Ui.State(Symbol.Important, "Search failed", groups[0].Error ?? "Couldn't reach the providers.")
                : Ui.State(Symbol.Find, "No results", $"Nothing matched “{query}”."));
            return;
        }
        foreach (var g in groups)
        {
            var section = new StackPanel { Spacing = 10 };
            section.Children.Add(Ui.Header(g.Provider.Name));
            if (g.Error != null) section.Children.Add(Ui.Error(g.Error));
            section.Children.Add(Ui.PosterGrid(g.Items.Select(i =>
                (UIElement)Ui.Poster(i, g.Provider.Name, 0, () => Ui.Open(i)))));
            _results.Children.Add(section);
        }
    }
}
