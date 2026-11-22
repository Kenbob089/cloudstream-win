using System.Net.NetworkInformation;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;
using Windows.Media.Core;
using Windows.Media.Playback;

namespace CloudStream.Pages;

/// <summary>
/// Plays a source resolved by a native provider. Only https URLs are played. Custom HTTP headers
/// (which some sources require) are not supported by MediaPlayer, so such sources are rejected with a clear message.
/// </summary>
public sealed partial class PlayerPage : Page
{
    private readonly MediaPlayerElement _view = new() { AreTransportControlsEnabled = true, Stretch = Microsoft.UI.Xaml.Media.Stretch.Uniform };
    private readonly StackPanel _overlay = new() { Orientation = Orientation.Horizontal, Spacing = 8, Margin = new Thickness(16), HorizontalAlignment = HorizontalAlignment.Left, VerticalAlignment = VerticalAlignment.Top };
    private readonly Grid _message = new() { Visibility = Visibility.Collapsed };
    private readonly TextBlock _messageText = new() { TextWrapping = TextWrapping.Wrap, TextAlignment = TextAlignment.Center, Foreground = new Microsoft.UI.Xaml.Media.SolidColorBrush(Microsoft.UI.Colors.White), MaxWidth = 480 };
    private readonly Button _retry = new() { Content = "Retry", HorizontalAlignment = HorizontalAlignment.Center };
    private readonly Button _prev = new() { Content = "⏮ Previous" };
    private readonly Button _next = new() { Content = "Next ⏭" };
    private readonly TextBlock _title = new() { Foreground = new Microsoft.UI.Xaml.Media.SolidColorBrush(Microsoft.UI.Colors.White), VerticalAlignment = VerticalAlignment.Center };
    private readonly DispatcherTimer _saveTimer = new() { Interval = TimeSpan.FromSeconds(10) };

    private PlayerRequest _req = new();
    private int? _index;
    private MediaPlayer? _player;
    private bool _opened, _failed, _leaving;
    private double _resumeAt;
    private DispatcherQueue _ui = DispatcherQueue.GetForCurrentThread();

    public PlayerPage()
    {
        InitializeComponent();
        Root.Children.Add(_view);

        var close = new Button { Content = "✕ Close" };
        close.Click += (_, _) => Leave();
        _prev.Click += async (_, _) => await Step(-1);
        _next.Click += async (_, _) => await Step(1);
        _overlay.Children.Add(close);
        _overlay.Children.Add(_prev);
        _overlay.Children.Add(_next);
        _overlay.Children.Add(_title);
        Root.Children.Add(_overlay);

        var box = new StackPanel { Spacing = 12, VerticalAlignment = VerticalAlignment.Center, HorizontalAlignment = HorizontalAlignment.Center };
        box.Children.Add(_messageText);
        box.Children.Add(_retry);
        _message.Children.Add(box);
        Root.Children.Add(_message);

        _retry.Click += async (_, _) => await Load();
        _saveTimer.Tick += async (_, _) => await Save(false);
    }

    protected override async void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        if (e.Parameter is not PlayerRequest req) return;
        _req = req;
        _index = req.Index;
        NetworkChange.NetworkAvailabilityChanged += OnNetwork;
        await Load();
    }

    protected override async void OnNavigatingFrom(NavigatingCancelEventArgs e)
    {
        base.OnNavigatingFrom(e);
        _leaving = true;
        NetworkChange.NetworkAvailabilityChanged -= OnNetwork;
        await TearDown();
    }

    private MediaEpisode? Episode => _index is int i && i >= 0 && i < _req.Episodes.Count ? _req.Episodes[i] : null;

    private void ShowMessage(string text, bool retry)
    {
        _messageText.Text = text;
        _retry.Visibility = retry ? Visibility.Visible : Visibility.Collapsed;
        _message.Visibility = Visibility.Visible;
    }

    private async Task Load()
    {
        _message.Visibility = Visibility.Collapsed;
        _failed = false;
        _opened = false;
        await TearDown(save: false);

        var ep = Episode;
        _title.Text = ep != null ? $"{_req.Item.Title} — {ep.Title}" : _req.Item.Title;
        _prev.IsEnabled = _index > 0;
        _next.IsEnabled = _index is int i && i + 1 < _req.Episodes.Count;

        List<VideoSource> sources;
        try
        {
            sources = await Core.CallAsync<List<VideoSource>>("sources", new { providerId = _req.Item.ProviderId, mediaId = _req.Item.Id, episode = ep });
        }
        catch (Exception ex) { ShowMessage(ex.Message, true); return; }

        var source = Pick(sources);
        if (source == null)
        {
            bool needsHeaders = sources.Any(s => s.Headers.Count > 0);
            ShowMessage(needsHeaders
                ? "This source requires custom HTTP headers, which the Windows player doesn't support."
                : "No playable source was found.", false);
            return;
        }

        var saved = AppState.Current.Progress.FirstOrDefault(p => p.MediaKey == _req.Item.Key && p.EpisodeId == ep?.Id);
        _resumeAt = saved is { Completed: false, Position: > 5 } ? saved.Position : 0;

        _player = new MediaPlayer { AutoPlay = true };
        _player.MediaOpened += (_, _) => _ui.TryEnqueue(OnOpened);
        _player.MediaFailed += (_, a) => _ui.TryEnqueue(() => OnFailed(a.ErrorMessage));
        _player.MediaEnded += (_, _) => _ui.TryEnqueue(async () => await OnEnded());
        _player.Source = MediaSource.CreateFromUri(new Uri(source.Url));
        _view.SetMediaPlayer(_player);
        _saveTimer.Start();
    }

    /// <summary>Prefers the configured quality, but only https sources that need no custom headers.</summary>
    private static VideoSource? Pick(List<VideoSource> sources)
    {
        var ok = sources.Where(s => s.Headers.Count == 0 && Uri.TryCreate(s.Url, UriKind.Absolute, out var u) && u.Scheme == Uri.UriSchemeHttps).ToList();
        if (ok.Count == 0) return null;
        var want = AppState.Setting("quality", "Auto");
        return ok.FirstOrDefault(s => s.Quality.Equals(want, StringComparison.OrdinalIgnoreCase)) ?? ok[0];
    }

    private void OnOpened()
    {
        if (_player == null) return;
        _opened = true;
        var speed = AppState.Setting("speed", "1x").TrimEnd('x');
        if (double.TryParse(speed, System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out var rate) && rate > 0)
            _player.PlaybackSession.PlaybackRate = rate;
        if (_resumeAt > 0 && _player.PlaybackSession.NaturalDuration.TotalSeconds > _resumeAt + 5)
            _player.PlaybackSession.Position = TimeSpan.FromSeconds(_resumeAt);
        _resumeAt = 0;
    }

    private void OnFailed(string message)
    {
        _failed = true;
        ShowMessage("Playback failed: " + (string.IsNullOrWhiteSpace(message) ? "the stream couldn't be opened." : message), true);
    }

    private async Task OnEnded()
    {
        await Save(true);
        if (AppState.Setting("autoPlayNext", true) && _next.IsEnabled) await Step(1);
    }

    private async Task Step(int delta)
    {
        if (_index is not int i) return;
        int n = i + delta;
        if (n < 0 || n >= _req.Episodes.Count) return;
        await Save(false);
        _index = n;
        await Load();
    }

    private void OnNetwork(object? sender, NetworkAvailabilityEventArgs e)
    {
        // Retry automatically when connectivity returns after a failure.
        if (e.IsAvailable && _failed && !_leaving) _ui.TryEnqueue(async () => await Load());
    }

    private async Task Save(bool completed)
    {
        if (_player == null || !_opened) return;
        var s = _player.PlaybackSession;
        double pos = s.Position.TotalSeconds, dur = s.NaturalDuration.TotalSeconds;
        if (dur <= 0) return;
        try
        {
            await Core.CallAsync<System.Text.Json.JsonElement>("progress.save",
                new { item = _req.Item, episode = Episode, position = pos, duration = dur, completed });
        }
        catch { /* progress is best-effort; never interrupt playback */ }
    }

    private async Task TearDown(bool save = true)
    {
        _saveTimer.Stop();
        if (save) await Save(false);
        if (_player != null)
        {
            var p = _player;
            _player = null;
            _view.SetMediaPlayer(null);
            p.Pause();
            p.Dispose();
        }
    }

    private void Leave()
    {
        _leaving = true;
        NetworkChange.NetworkAvailabilityChanged -= OnNetwork;
        App.MainWin?.GoBack();
    }
}
