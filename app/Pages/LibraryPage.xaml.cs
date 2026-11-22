using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;

namespace CloudStream.Pages;

public sealed partial class LibraryPage : Page
{
    private static readonly string[] Sections = { "Continue", "Bookmarks", "Favorites", "History" };
    private readonly SelectorBar _bar = new();
    private readonly ComboBox _sort = new() { MinWidth = 150 };
    private readonly ComboBox _type = new() { MinWidth = 150 };
    private readonly StackPanel _content = new() { Spacing = 12 };
    private int _section;

    public LibraryPage()
    {
        InitializeComponent();
        Root.Children.Add(Ui.Title("Library"));

        foreach (var s in Sections) _bar.Items.Add(new SelectorBarItem { Text = s });
        _bar.SelectedItem = _bar.Items[0];
        _bar.SelectionChanged += (_, _) => { _section = _bar.Items.IndexOf(_bar.SelectedItem); Render(); };
        Root.Children.Add(_bar);

        _sort.Items.Add("Most recent");
        _sort.Items.Add("Title");
        _sort.SelectedIndex = 0;
        _type.Items.Add("All types");
        foreach (var t in new[] { "movie", "tvSeries", "anime", "cartoon", "documentary" }) _type.Items.Add(Labels.Type(t));
        _type.SelectedIndex = 0;
        _sort.SelectionChanged += (_, _) => Render();
        _type.SelectionChanged += (_, _) => Render();
        var tools = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 10 };
        tools.Children.Add(_sort);
        tools.Children.Add(_type);
        var clear = new Button { Content = "Clear watch history" };
        clear.Click += async (_, _) =>
        {
            if (!await Ui.ConfirmAsync(XamlRoot, "Clear all watch history?", "Resume points and watched marks will be removed.", "Clear")) return;
            await Core.CallAsync<JsonElement>("history.clear");
            await Reload();
        };
        tools.Children.Add(clear);
        Root.Children.Add(tools);
        Root.Children.Add(_content);
    }

    protected override async void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        await Reload();
    }

    private async Task Reload()
    {
        try { await AppState.RefreshAsync(); Render(); }
        catch (Exception ex) { _content.Children.Clear(); _content.Children.Add(Ui.Error(ex.Message)); }
    }

    private bool TypeOk(MediaItem i) =>
        _type.SelectedIndex <= 0 || Labels.Type(i.MediaType) == (string)_type.SelectedItem;

    private void Render()
    {
        _content.Children.Clear();
        var snap = AppState.Current;
        bool byTitle = _sort.SelectedIndex == 1;

        switch (_section)
        {
            case 1: Bookmarks(snap.Bookmarks, byTitle, "No bookmarks yet", "Tap Bookmark on any title to save it here."); break;
            case 2: Bookmarks(snap.Bookmarks.Where(b => b.IsFavorite).ToList(), byTitle, "No favorites yet", "Tap the heart on a title to add it."); break;
            case 0:
                Progress(snap.Progress.Where(p => !p.Completed && p.Position > 5).OrderByDescending(p => p.UpdatedAt)
                    .GroupBy(p => p.MediaKey).Select(g => g.First()).ToList(), byTitle, "Nothing to resume", "Titles you start watching appear here.");
                break;
            default:
                Progress(snap.Progress.OrderByDescending(p => p.UpdatedAt).ToList(), byTitle, "No watch history", "Everything you watch is listed here.");
                break;
        }
    }

    private void Bookmarks(List<Bookmark> list, bool byTitle, string empty, string hint)
    {
        var items = list.Where(b => TypeOk(b.Item)).ToList();
        if (byTitle) items = items.OrderBy(b => b.Item.Title, StringComparer.CurrentCultureIgnoreCase).ToList();
        if (items.Count == 0) { _content.Children.Add(Ui.State(Symbol.Library, empty, hint)); return; }

        _content.Children.Add(Ui.PosterGrid(items.Select(b =>
        {
            var btn = Ui.Poster(b.Item, AppState.ProviderName(b.Item.ProviderId), 0, () => Ui.Open(b.Item));
            var menu = new MenuFlyout();
            var fav = new MenuFlyoutItem { Text = b.IsFavorite ? "Remove favorite" : "Favorite" };
            fav.Click += async (_, _) => { await Core.CallAsync<JsonElement>("favorite.toggle", new { item = b.Item }); await Reload(); };
            var rm = new MenuFlyoutItem { Text = "Remove" };
            rm.Click += async (_, _) => { await Core.CallAsync<JsonElement>("bookmark.remove", new { key = b.Item.Key }); await Reload(); };
            menu.Items.Add(fav);
            menu.Items.Add(rm);
            btn.ContextFlyout = menu;
            return (UIElement)btn;
        })));
    }

    private void Progress(List<ProgressEntry> list, bool byTitle, string empty, string hint)
    {
        var items = list.Where(p => TypeOk(p.Item)).ToList();
        if (byTitle) items = items.OrderBy(p => p.Item.Title, StringComparer.CurrentCultureIgnoreCase).ToList();
        if (items.Count == 0) { _content.Children.Add(Ui.State(Symbol.Clock, empty, hint)); return; }

        foreach (var p in items)
        {
            var entry = p;
            var row = new Grid { ColumnSpacing = 12, Padding = new Thickness(10), Background = Ui.Card };
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

            var art = new Border { Width = 56, Height = 84, CornerRadius = new CornerRadius(8), Child = Ui.Img(entry.Item.PosterUrl, 120) };
            row.Children.Add(art);

            var text = new StackPanel { Spacing = 3, VerticalAlignment = VerticalAlignment.Center };
            text.Children.Add(Ui.Text(entry.Item.Title, 14, true, 1, true, 2));
            if (entry.EpisodeTitle != null)
                text.Children.Add(Ui.Text($"{(entry.SeasonNumber is int s ? $"S{s} " : "")}{(entry.EpisodeNumber is int n ? $"E{n} · " : "")}{entry.EpisodeTitle}", 12, false, 0.7, false));
            if (entry.Completed) text.Children.Add(Ui.Text("Watched", 12, true, 0.8, false));
            else text.Children.Add(new ProgressBar { Value = entry.Fraction * 100, Maximum = 100 });
            text.Children.Add(Ui.Text(DateTimeOffset.FromUnixTimeSeconds(entry.UpdatedAt).LocalDateTime.ToString("g"), 11, false, 0.5, false));
            Grid.SetColumn(text, 1);
            row.Children.Add(text);

            var remove = new Button { Content = new SymbolIcon(Symbol.Delete) };
            Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(remove, "Remove from history");
            remove.Click += async (_, _) => { await Core.CallAsync<JsonElement>("history.remove", new { key = entry.Key }); await Reload(); };
            Grid.SetColumn(remove, 2);
            row.Children.Add(remove);

            _content.Children.Add(Ui.Plain(row, () => Ui.Open(entry.Item), entry.Item.Title));
        }
    }
}
