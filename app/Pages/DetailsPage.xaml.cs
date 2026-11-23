using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;

namespace CloudStream.Pages;

public sealed partial class DetailsPage : Page
{
    private MediaItem _item = new();
    private MediaDetails? _details;
    private List<MediaEpisode> _episodes = new();
    private int? _season;
    private readonly StackPanel _episodeHost = new() { Spacing = 8 };
    private Button? _bookmarkBtn, _favBtn;

    public DetailsPage() => InitializeComponent();

    protected override async void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        if (e.Parameter is not MediaItem item) return;
        _item = item;
        Root.Children.Clear();
        Root.Children.Add(Ui.Busy());
        try
        {
            await AppState.RefreshAsync();
            _details = await Core.CallAsync<MediaDetails>("details", new { providerId = item.ProviderId, id = item.Id });
            _item = _details.Item;
            _season = _details.Seasons.Count > 0 ? _details.Seasons[0] : null;
            _episodes = await LoadEpisodes(_season);
            Build();
        }
        catch (Exception ex)
        {
            Root.Children.Clear();
            Root.Children.Add(Ui.BackLink("Back", () => App.MainWin?.GoBack()));
            Root.Children.Add(Ui.Error(ex.Message));
        }
    }

    private Task<List<MediaEpisode>> LoadEpisodes(int? season) =>
        Core.CallAsync<List<MediaEpisode>>("episodes", new { providerId = _item.ProviderId, mediaId = _item.Id, season });

    private Bookmark? Saved => AppState.Current.Bookmarks.FirstOrDefault(b => b.Item.Key == _item.Key);

    private void Build()
    {
        Root.Children.Clear();
        Root.Children.Add(Ui.BackLink("Back", () => App.MainWin?.GoBack()));

        var header = new Grid { ColumnSpacing = 22 };
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        header.Children.Add(new Border
        {
            Width = 200, Height = 300, CornerRadius = new CornerRadius(14), Background = Ui.Card,
            Child = Ui.Img(_item.PosterUrl ?? _item.BackdropUrl, 400),
        });

        var info = new StackPanel { Spacing = 10 };
        info.Children.Add(Ui.Text(_item.Title, 30, true));
        if (_item.OriginalTitle is { Length: > 0 } ot && ot != _item.Title) info.Children.Add(Ui.Text(ot, 13, false, 0.6));
        var chips = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6 };
        chips.Children.Add(Ui.Chip(Labels.Type(_item.MediaType), true));
        if (_item.ReleaseYear is int y) chips.Children.Add(Ui.Chip(y.ToString()));
        if (_item.Duration is double d && d > 0) chips.Children.Add(Ui.Chip(Labels.Runtime(d)));
        if (_item.Rating is double r && r > 0) chips.Children.Add(Ui.Chip($"{r:0.0}", false, Symbol.Favorite));
        info.Children.Add(chips);
        if (_item.Genres.Count > 0) info.Children.Add(Ui.Text(string.Join(" · ", _item.Genres), 12, false, 0.65));
        info.Children.Add(Ui.Text("Source: " + AppState.ProviderName(_item.ProviderId), 12, false, 0.6, false));
        if (!string.IsNullOrWhiteSpace(_item.Description)) info.Children.Add(Ui.Text(_item.Description!, 14, false, 0.85));

        var actions = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        actions.Children.Add(Ui.Accent(PlayLabel(), () => PlayDefault()));
        _bookmarkBtn = new Button();
        _bookmarkBtn.Click += async (_, _) =>
        {
            await Core.CallAsync<bool>("bookmark.toggle", new { item = _item });
            await AppState.RefreshAsync();
            UpdateButtons();
        };
        _favBtn = new Button();
        _favBtn.Click += async (_, _) =>
        {
            await Core.CallAsync<JsonElement>("favorite.toggle", new { item = _item });
            await AppState.RefreshAsync();
            UpdateButtons();
        };
        actions.Children.Add(_bookmarkBtn);
        actions.Children.Add(_favBtn);
        info.Children.Add(actions);
        UpdateButtons();

        Grid.SetColumn(info, 1);
        header.Children.Add(info);
        Root.Children.Add(header);

        if (_details!.Seasons.Count > 1)
        {
            var combo = new ComboBox { MinWidth = 160, Header = "Season" };
            foreach (var s in _details.Seasons) combo.Items.Add(new ComboBoxItem { Content = "Season " + s, Tag = s });
            combo.SelectedIndex = 0;
            combo.SelectionChanged += async (_, _) =>
            {
                if ((combo.SelectedItem as ComboBoxItem)?.Tag is not int s) return;
                _season = s;
                _episodeHost.Children.Clear();
                _episodeHost.Children.Add(Ui.Busy());
                try { _episodes = await LoadEpisodes(s); RenderEpisodes(); }
                catch (Exception ex) { _episodeHost.Children.Clear(); _episodeHost.Children.Add(Ui.Error(ex.Message)); }
            };
            Root.Children.Add(combo);
        }

        if (_episodes.Count > 0) Root.Children.Add(Ui.Header("Episodes"));
        Root.Children.Add(_episodeHost);
        RenderEpisodes();
    }

    private void UpdateButtons()
    {
        var saved = Saved;
        if (_bookmarkBtn != null) _bookmarkBtn.Content = saved != null ? Ui.IconLabel(Symbol.Accept, "Bookmarked") : Ui.IconLabel(Symbol.Add, "Bookmark");
        if (_favBtn != null) _favBtn.Content = saved?.IsFavorite == true ? Ui.IconLabel(Symbol.Favorite, "Favorite") : Ui.IconLabel(Symbol.OutlineStar, "Favorite");
    }

    private ProgressEntry? LastProgress() => AppState.Current.Progress
        .Where(p => p.MediaKey == _item.Key).OrderByDescending(p => p.UpdatedAt).FirstOrDefault();

    private string PlayLabel()
    {
        var p = LastProgress();
        return p != null && !p.Completed && p.Position > 5 ? "Resume" : "Play";
    }

    private void PlayDefault()
    {
        if (_episodes.Count == 0)
        {
            App.MainWin?.Navigate(typeof(PlayerPage), new PlayerRequest { Item = _item, Episodes = _episodes, Index = null });
            return;
        }
        var last = LastProgress();
        int idx = 0;
        if (last?.EpisodeId != null)
        {
            int found = _episodes.FindIndex(e => e.Id == last.EpisodeId);
            if (found >= 0) idx = last.Completed && found + 1 < _episodes.Count ? found + 1 : found;
        }
        Play(idx);
    }

    private void Play(int index) =>
        App.MainWin?.Navigate(typeof(PlayerPage), new PlayerRequest { Item = _item, Episodes = _episodes, Index = index });

    private void RenderEpisodes()
    {
        _episodeHost.Children.Clear();
        for (int i = 0; i < _episodes.Count; i++)
        {
            int index = i;
            var ep = _episodes[i];
            var prog = AppState.Current.Progress.FirstOrDefault(p => p.MediaKey == _item.Key && p.EpisodeId == ep.Id);

            var row = new Grid { ColumnSpacing = 12, Padding = new Thickness(10), Background = Ui.Card };
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            row.Children.Add(new Border { Width = 120, Height = 68, CornerRadius = new CornerRadius(8), Child = Ui.Img(ep.ThumbnailUrl ?? _item.BackdropUrl, 240) });

            var text = new StackPanel { Spacing = 3, VerticalAlignment = VerticalAlignment.Center };
            text.Children.Add(Ui.Text($"{(ep.EpisodeNumber is int n ? n + ". " : "")}{ep.Title}", 14, true, 1, true, 2));
            if (ep.Duration is double dur && dur > 0) text.Children.Add(Ui.Text(Labels.Runtime(dur), 11, false, 0.6, false));
            if (!string.IsNullOrWhiteSpace(ep.Description)) text.Children.Add(Ui.Text(ep.Description!, 12, false, 0.7, true, 2));
            if (prog != null)
            {
                if (prog.Completed) text.Children.Add(Ui.Text("Watched", 11, true, 0.8, false));
                else text.Children.Add(new ProgressBar { Value = prog.Fraction * 100, Maximum = 100 });
            }
            Grid.SetColumn(text, 1);
            row.Children.Add(text);
            _episodeHost.Children.Add(Ui.Plain(row, () => Play(index), ep.Title));
        }
    }
}
